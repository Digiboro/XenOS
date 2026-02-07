//! IoConnectInterrupt and related interrupt management
//!
//! Реализация IoConnectInterrupt для подключения драйверных прерываний.
//!
//! NT модель:
//! - IoConnectInterrupt регистрирует ISR для указанного прерывания
//! - При прерывании вызывается зарегистрированный ISR
//! - IoDisconnectInterrupt отключает ISR
//!
//! Источники:
//! - ReactOS: ntoskrnl/io/iomgr/irq.c
//! - WDK: wdm.h

use crate::hal::init::{hal_register_interrupt_handler, HalInterruptCallback};
use crate::hal::ioapic::{ioapic_enable_gsi, ioapic_disable_gsi, ioapic_gsi_to_vector, ioapic_configure_gsi};
use crate::hal::irql::KIRQL;
use crate::nt::{NTSTATUS, PVOID, BOOLEAN, ULONG, TRUE, FALSE, STATUS_SUCCESS, STATUS_INVALID_PARAMETER, STATUS_INSUFFICIENT_RESOURCES};
use crate::ex::pool::{ex_allocate_pool_with_tag, ex_free_pool_with_tag, POOL_TYPE};
use core::sync::atomic::{AtomicU32, Ordering};

// =============================================================================
// KINTERRUPT Structure
// =============================================================================

/// KINTERRUPT - Kernel Interrupt Object
///
/// Содержит информацию о подключённом прерывании.
#[repr(C)]
pub struct KINTERRUPT {
    /// Signature for validation
    pub signature: u32,
    /// Interrupt vector (system vector)
    pub vector: u8,
    /// IRQL for this interrupt
    pub irql: KIRQL,
    /// GSI (Global System Interrupt)
    pub gsi: u32,
    /// Service routine provided by driver
    pub service_routine: PKSERVICE_ROUTINE,
    /// Service context passed to ISR
    pub service_context: PVOID,
    /// Connected flag
    pub connected: bool,
    /// Shared interrupt flag
    pub share_vector: bool,
    /// Interrupt mode (level/edge)
    pub mode: u8,
}

/// KINTERRUPT signature
const KINTERRUPT_SIGNATURE: u32 = 0x4B494E54; // 'KINT'

/// Pointer to KINTERRUPT
pub type PKINTERRUPT = *mut KINTERRUPT;

/// Service routine type (driver's ISR)
///
/// Returns TRUE if interrupt was handled, FALSE otherwise
pub type PKSERVICE_ROUTINE = unsafe extern "win64" fn(
    interrupt: PKINTERRUPT,
    service_context: PVOID,
) -> BOOLEAN;

// =============================================================================
// Global Interrupt Table
// =============================================================================

/// Maximum connected interrupts
const MAX_CONNECTED_INTERRUPTS: usize = 64;

/// Connected interrupt objects
static mut CONNECTED_INTERRUPTS: [Option<*mut KINTERRUPT>; 256] = [None; 256];

/// Number of connected interrupts (for debugging)
static CONNECTED_COUNT: AtomicU32 = AtomicU32::new(0);

// =============================================================================
// IoConnectInterrupt
// =============================================================================

/// IoConnectInterrupt - connects a driver's ISR to an interrupt
///
/// # Arguments
/// * `interrupt_object` - receives pointer to KINTERRUPT
/// * `service_routine` - driver's ISR function
/// * `service_context` - context passed to ISR
/// * `spin_lock` - optional spin lock (unused in this implementation)
/// * `vector` - interrupt vector from resources
/// * `irql` - IRQL for interrupt
/// * `synchronize_irql` - IRQL for synchronization (unused)
/// * `interrupt_mode` - edge (1) or level (0) triggered
/// * `share_vector` - whether interrupt can be shared
/// * `processor_number` - target processor (0 = any)
/// * `floating_save` - whether to save FP state (unused)
///
/// # Returns
/// STATUS_SUCCESS on success
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn IoConnectInterrupt(
    interrupt_object: *mut PKINTERRUPT,
    service_routine: PKSERVICE_ROUTINE,
    service_context: PVOID,
    _spin_lock: PVOID,
    vector: ULONG,
    irql: KIRQL,
    _synchronize_irql: KIRQL,
    interrupt_mode: ULONG,
    share_vector: BOOLEAN,
    _processor_number: ULONG,
    _floating_save: BOOLEAN,
) -> NTSTATUS {
    unsafe {
        crate::kd::dbg_print("[IO] IoConnectInterrupt: vector=");
        crate::kd::dbg_print_hex(vector as u64);
        crate::kd::dbg_print(" irql=");
        crate::kd::dbg_print_hex(irql as u64);
        crate::kd::dbg_print("\n");
        
        if interrupt_object.is_null() {
            return STATUS_INVALID_PARAMETER;
        }
        
        // Allocate KINTERRUPT structure
        let kinterrupt = ex_allocate_pool_with_tag(
            POOL_TYPE::NonPagedPool,
            core::mem::size_of::<KINTERRUPT>(),
            0x544E494B, // 'KINT'
        ) as *mut KINTERRUPT;
        
        if kinterrupt.is_null() {
            return STATUS_INSUFFICIENT_RESOURCES;
        }
        
        // Vector from PCI is typically the IRQ/GSI
        // TODO: For PCI MSI, this would be different
        let gsi = vector;
        
        // Calculate system vector: 0x30 + GSI for standard mapping
        // Device IRQ vectors start at 0x30 in our IDT setup
        let system_vector: u8 = (0x30 + gsi).min(0x47) as u8;
        
        crate::kd::dbg_print("[IO] IoConnectInterrupt: gsi=");
        crate::kd::dbg_print_hex(gsi as u64);
        crate::kd::dbg_print(" system_vector=0x");
        crate::kd::dbg_print_hex(system_vector as u64);
        crate::kd::dbg_print("\n");
        
        // Initialize KINTERRUPT
        (*kinterrupt).signature = KINTERRUPT_SIGNATURE;
        (*kinterrupt).vector = system_vector;
        (*kinterrupt).irql = irql;
        (*kinterrupt).gsi = gsi;
        (*kinterrupt).service_routine = service_routine;
        (*kinterrupt).service_context = service_context;
        (*kinterrupt).connected = false;
        (*kinterrupt).share_vector = share_vector != 0;
        (*kinterrupt).mode = interrupt_mode as u8;
        
        // Check if vector is already in use
        if CONNECTED_INTERRUPTS[system_vector as usize].is_some() {
            if !(*kinterrupt).share_vector {
                crate::kd::dbg_print("[IO] IoConnectInterrupt: vector already in use\n");
                ex_free_pool_with_tag(kinterrupt as PVOID, 0x544E494B);
                return STATUS_INSUFFICIENT_RESOURCES;
            }
            // TODO: implement shared interrupt chain
        }
        
        // Register our dispatch routine with HAL
        hal_register_interrupt_handler(
            system_vector,
            io_interrupt_dispatch,
            kinterrupt as PVOID,
        );
        
        // Store in our table
        CONNECTED_INTERRUPTS[system_vector as usize] = Some(kinterrupt);
        
        // Configure the interrupt in IOAPIC (vector, destination, trigger mode, polarity)
        // WDK: interrupt_mode = 0 (LevelSensitive), 1 (Latched/Edge)
        // IOAPIC: trigger_mode = 0 (Edge), 1 (Level)
        // So we use the interrupt_mode directly for PCI which passes Level=1
        // But WDK uses inverted logic: LevelSensitive=0, Latched=1
        // We need: Level -> 1, Edge -> 0
        // If interrupt_mode == 0 (LevelSensitive) -> trigger_mode = 1 (Level)
        // If interrupt_mode == 1 (Latched) -> trigger_mode = 0 (Edge)
        // Actually StorPort passes 1 for Level, so we need to adjust
        let trigger_mode = interrupt_mode as u8; // Level=1, Edge=0
        let polarity = 1u8; // Active Low for PCI (common)
        ioapic_configure_gsi(gsi, system_vector, 0, trigger_mode, polarity);
        
        // Enable the interrupt in IOAPIC
        ioapic_enable_gsi(gsi);
        
        (*kinterrupt).connected = true;
        *interrupt_object = kinterrupt;
        
        CONNECTED_COUNT.fetch_add(1, Ordering::Relaxed);
        
        crate::kd::dbg_print("[IO] IoConnectInterrupt: SUCCESS, KINTERRUPT=0x");
        crate::kd::dbg_print_hex(kinterrupt as u64);
        crate::kd::dbg_print("\n");
        
        
        STATUS_SUCCESS
    }
}

/// IoDisconnectInterrupt - disconnects driver's ISR
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn IoDisconnectInterrupt(interrupt_object: PKINTERRUPT) {
    unsafe {
        if interrupt_object.is_null() {
            return;
        }
        
        if (*interrupt_object).signature != KINTERRUPT_SIGNATURE {
            return;
        }
        
        let vector = (*interrupt_object).vector;
        let gsi = (*interrupt_object).gsi;
        
        crate::kd::dbg_print("[IO] IoDisconnectInterrupt: vector=");
        crate::kd::dbg_print_hex(vector as u64);
        crate::kd::dbg_print("\n");
        
        // Disable interrupt in IOAPIC
        ioapic_disable_gsi(gsi);
        
        // Remove from table
        CONNECTED_INTERRUPTS[vector as usize] = None;
        
        // Clear and free
        (*interrupt_object).connected = false;
        ex_free_pool_with_tag(interrupt_object as PVOID, 0x544E494B);
        
        CONNECTED_COUNT.fetch_sub(1, Ordering::Relaxed);
    }
}

// =============================================================================
// Interrupt Dispatch
// =============================================================================

/// Internal interrupt dispatcher
///
/// Called by HAL when interrupt occurs. Calls driver's ISR.
fn io_interrupt_dispatch(vector: u8, context: *mut core::ffi::c_void) {
    unsafe {
        let kinterrupt = context as *mut KINTERRUPT;
        
        if kinterrupt.is_null() {
            return;
        }
        
        if (*kinterrupt).signature != KINTERRUPT_SIGNATURE {
            return;
        }
        
        if !(*kinterrupt).connected {
            return;
        }
        
        // Call driver's ISR
        let isr = (*kinterrupt).service_routine;
        let _handled = isr(kinterrupt, (*kinterrupt).service_context);
        
        // TODO: For level-triggered interrupts, if not handled, might need to mask
        // For now, assume ISR always handles it
    }
}

// =============================================================================
// Exports
// =============================================================================

/// KeConnectInterrupt - kernel version (same as Io version for our purposes)
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn KeConnectInterrupt(interrupt: PKINTERRUPT) -> BOOLEAN {
    unsafe {
        if interrupt.is_null() || (*interrupt).signature != KINTERRUPT_SIGNATURE {
            return FALSE;
        }
        
        if (*interrupt).connected {
            return TRUE;
        }
        
        let vector = (*interrupt).vector;
        let gsi = (*interrupt).gsi;
        
        // Register with HAL
        hal_register_interrupt_handler(
            vector,
            io_interrupt_dispatch,
            interrupt as PVOID,
        );
        
        CONNECTED_INTERRUPTS[vector as usize] = Some(interrupt);
        ioapic_enable_gsi(gsi);
        (*interrupt).connected = true;
        
        TRUE
    }
}

/// KeDisconnectInterrupt - kernel version
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn KeDisconnectInterrupt(interrupt: PKINTERRUPT) -> BOOLEAN {
    unsafe {
        if interrupt.is_null() || (*interrupt).signature != KINTERRUPT_SIGNATURE {
            return FALSE;
        }
        
        if !(*interrupt).connected {
            return TRUE;
        }
        
        let vector = (*interrupt).vector;
        let gsi = (*interrupt).gsi;
        
        ioapic_disable_gsi(gsi);
        CONNECTED_INTERRUPTS[vector as usize] = None;
        (*interrupt).connected = false;
        
        TRUE
    }
}

