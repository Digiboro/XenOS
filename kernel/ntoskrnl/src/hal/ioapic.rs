//! IOAPIC - I/O Advanced Programmable Interrupt Controller
//!
//! Реализация IOAPIC для маршрутизации внешних прерываний (GSI).
//!
//! NT использует GSI (Global System Interrupt) для идентификации прерываний:
//! - ISA IRQ 0-15 могут быть remapped через ACPI MADT overrides
//! - Каждый IOAPIC обслуживает диапазон GSI начиная с gsi_base
//!
//! Источники:
//! - ReactOS: hal/halx86/apic/ioapic.c
//! - Intel IOAPIC Specification

#![allow(dead_code)]

use core::sync::atomic::AtomicU32;
use core::sync::atomic::AtomicU64;
use core::sync::atomic::Ordering;

use crate::mm::hal_mmio::HAL_IOAPIC_BASE;
use crate::mm::hal_mmio::mm_map_ioapic;

// =============================================================================
// IOAPIC Register Constants
// =============================================================================

/// IOAPIC Register Select offset
const IOAPIC_IOREGSEL: u32 = 0x00;
/// IOAPIC Window (Data) offset
const IOAPIC_IOWIN: u32 = 0x10;

/// IOAPIC ID Register
const IOAPIC_REG_ID: u8 = 0x00;
/// IOAPIC Version Register
const IOAPIC_REG_VER: u8 = 0x01;
/// IOAPIC Arbitration ID Register
const IOAPIC_REG_ARB: u8 = 0x02;
/// IOAPIC Redirection Table Base (entries at 0x10 + 2*n)
const IOAPIC_REG_REDTBL_BASE: u8 = 0x10;

/// Maximum number of GSI (Global System Interrupts)
pub const MAX_GSI: usize = 256;

// =============================================================================
// IOAPIC Redirection Table Entry
// =============================================================================

/// IOAPIC Redirection Table Entry (64-bit)
#[derive(Clone, Copy, Debug)]
pub struct IoApicRedirectionEntry {
    /// Interrupt vector (0-255)
    pub vector: u8,
    /// Delivery mode: 0=Fixed, 1=LowestPri, 2=SMI, 4=NMI, 5=INIT, 7=ExtINT
    pub delivery_mode: u8,
    /// Destination mode: 0=Physical, 1=Logical
    pub destination_mode: u8,
    /// Delivery status (read-only): 0=Idle, 1=SendPending
    pub delivery_status: bool,
    /// Polarity: 0=ActiveHigh, 1=ActiveLow
    pub polarity: u8,
    /// Remote IRR (read-only): Level-triggered IRR flag
    pub remote_irr: bool,
    /// Trigger mode: 0=Edge, 1=Level
    pub trigger_mode: u8,
    /// Mask: 0=Enabled, 1=Masked
    pub mask: bool,
    /// Destination CPU (physical APIC ID or logical)
    pub destination: u8,
}

impl IoApicRedirectionEntry {
    /// Creates a new masked entry
    pub const fn new() -> Self {
        Self {
            vector: 0,
            delivery_mode: 0,
            destination_mode: 0,
            delivery_status: false,
            polarity: 0,
            remote_irr: false,
            trigger_mode: 0,
            mask: true, // Masked by default
            destination: 0,
        }
    }

    /// Converts to 64-bit raw value for IOAPIC register
    pub fn to_u64(&self) -> u64 {
        let mut value: u64 = 0;
        value |= self.vector as u64;
        value |= (self.delivery_mode as u64) << 8;
        value |= (self.destination_mode as u64) << 11;
        value |= (self.delivery_status as u64) << 12;
        value |= (self.polarity as u64) << 13;
        value |= (self.remote_irr as u64) << 14;
        value |= (self.trigger_mode as u64) << 15;
        value |= (self.mask as u64) << 16;
        value |= (self.destination as u64) << 56;
        value
    }

    /// Creates from 64-bit raw value
    pub fn from_u64(value: u64) -> Self {
        Self {
            vector: (value & 0xFF) as u8,
            delivery_mode: ((value >> 8) & 0x7) as u8,
            destination_mode: ((value >> 11) & 0x1) as u8,
            delivery_status: ((value >> 12) & 0x1) != 0,
            polarity: ((value >> 13) & 0x1) as u8,
            remote_irr: ((value >> 14) & 0x1) != 0,
            trigger_mode: ((value >> 15) & 0x1) as u8,
            mask: ((value >> 16) & 0x1) != 0,
            destination: ((value >> 56) & 0xFF) as u8,
        }
    }
}

impl Default for IoApicRedirectionEntry {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// IRQ Override from ACPI MADT
// =============================================================================

/// Interrupt override information from ACPI MADT
#[derive(Clone, Copy, Debug)]
pub struct IrqOverride {
    /// ISA IRQ number (source)
    pub source_irq: u8,
    /// Global System Interrupt number (destination)
    pub global_irq: u32,
    /// Polarity: 0=default, 1=active high, 3=active low
    pub polarity: u8,
    /// Trigger mode: 0=default, 1=edge, 3=level
    pub trigger_mode: u8,
}

// =============================================================================
// IOAPIC State
// =============================================================================

/// IOAPIC initialized flag
static IOAPIC_INITIALIZED: AtomicU32 = AtomicU32::new(0);

/// Physical address of IOAPIC (from ACPI MADT)
static IOAPIC_PHYSICAL_ADDRESS: AtomicU64 = AtomicU64::new(0);

/// Number of redirection entries in IOAPIC
static IOAPIC_NUM_ENTRIES: AtomicU32 = AtomicU32::new(0);

/// GSI Base for this IOAPIC (from ACPI MADT)
static IOAPIC_GSI_BASE: AtomicU32 = AtomicU32::new(0);

/// GSI to Vector mapping table
static mut GSI_TO_VECTOR: [u8; MAX_GSI] = [0xFF; MAX_GSI];

/// Vector to GSI mapping table
static mut VECTOR_TO_GSI: [u8; 256] = [0xFF; 256];

/// IRQ overrides from ACPI MADT (ISA IRQ 0-15)
static mut IRQ_OVERRIDES: [Option<IrqOverride>; 16] = [None; 16];

// =============================================================================
// IOAPIC Register Access
// =============================================================================

/// Reads an IOAPIC register
#[inline]
fn ioapic_read(register: u8) -> u32 {
    unsafe {
        // Write register number to IOREGSEL
        core::ptr::write_volatile(
            (HAL_IOAPIC_BASE + IOAPIC_IOREGSEL as u64) as *mut u32,
            register as u32,
        );

        // Read value from IOWIN
        core::ptr::read_volatile((HAL_IOAPIC_BASE + IOAPIC_IOWIN as u64) as *const u32)
    }
}

/// Writes to an IOAPIC register
#[inline]
fn ioapic_write(register: u8, value: u32) {
    unsafe {
        // Write register number to IOREGSEL
        core::ptr::write_volatile(
            (HAL_IOAPIC_BASE + IOAPIC_IOREGSEL as u64) as *mut u32,
            register as u32,
        );

        // Write value to IOWIN
        core::ptr::write_volatile((HAL_IOAPIC_BASE + IOAPIC_IOWIN as u64) as *mut u32, value);
    }
}

// =============================================================================
// IOAPIC Redirection Table
// =============================================================================

/// Converts GSI to redirection table index
#[inline]
fn gsi_to_redirection_index(gsi: u32) -> u8 {
    let gsi_base = IOAPIC_GSI_BASE.load(Ordering::Acquire);
    let num_entries = IOAPIC_NUM_ENTRIES.load(Ordering::Acquire);

    assert!(
        gsi >= gsi_base,
        "GSI {} is below IOAPIC GSI base {}",
        gsi,
        gsi_base
    );
    let index = gsi - gsi_base;
    assert!(
        index < num_entries,
        "GSI {} (index {}) exceeds IOAPIC entries {}",
        gsi,
        index,
        num_entries
    );

    index as u8
}

/// Reads Redirection Table Entry for GSI
pub fn ioapic_read_redirection_entry(gsi: u32) -> IoApicRedirectionEntry {
    let index = gsi_to_redirection_index(gsi);

    let reg_low = IOAPIC_REG_REDTBL_BASE + (index * 2);
    let reg_high = reg_low + 1;

    let low = ioapic_read(reg_low) as u64;
    let high = ioapic_read(reg_high) as u64;

    let value = (high << 32) | low;
    IoApicRedirectionEntry::from_u64(value)
}

/// Writes Redirection Table Entry for GSI
pub fn ioapic_write_redirection_entry(gsi: u32, entry: &IoApicRedirectionEntry) {
    let index = gsi_to_redirection_index(gsi);

    let reg_low = IOAPIC_REG_REDTBL_BASE + (index * 2);
    let reg_high = reg_low + 1;

    let value = entry.to_u64();
    let low = (value & 0xFFFFFFFF) as u32;
    let high = (value >> 32) as u32;

    // Write high first, then low (to avoid spurious interrupts)
    ioapic_write(reg_high, high);
    ioapic_write(reg_low, low);
}

// =============================================================================
// IOAPIC Initialization
// =============================================================================

/// Initializes IOAPIC
///
/// # Arguments
/// * `ioapic_address` - Physical address of IOAPIC from ACPI MADT
/// * `gsi_base` - First GSI handled by this IOAPIC (from ACPI MADT)
///
/// # Safety
/// Must be called after mm_init_hal_mmio() and MADT parsing
pub unsafe fn ioapic_initialize(ioapic_address: u64, gsi_base: u32) -> bool {
    unsafe {
        // Save physical address and GSI base
        IOAPIC_PHYSICAL_ADDRESS.store(ioapic_address, Ordering::Release);
        IOAPIC_GSI_BASE.store(gsi_base, Ordering::Release);

        // Map IOAPIC to fixed virtual address using HAL MMIO
        // Index 0 for first IOAPIC (multiple IOAPIC support planned)
        let ioapic_virt = mm_map_ioapic(ioapic_address, 0);
        if ioapic_virt == 0 {
            return false;
        }

        // Verify we got expected address
        debug_assert_eq!(ioapic_virt, HAL_IOAPIC_BASE);

        // Read Version register
        let ver_reg = ioapic_read(IOAPIC_REG_VER);
        let _version = (ver_reg & 0xFF) as u8;
        let max_redir = ((ver_reg >> 16) & 0xFF) as u8;
        let num_entries = (max_redir + 1) as u32;

        IOAPIC_NUM_ENTRIES.store(num_entries, Ordering::Release);

        // Mask all GSIs initially
        for i in 0..num_entries {
            let gsi = gsi_base + i;
            let mut entry = IoApicRedirectionEntry::new();
            entry.mask = true;
            entry.vector = 0xFF; // Invalid vector
            ioapic_write_redirection_entry(gsi, &entry);
        }

        IOAPIC_INITIALIZED.store(1, Ordering::Release);
        true
    }
}

/// Checks if IOAPIC is initialized
#[inline]
pub fn is_ioapic_initialized() -> bool {
    IOAPIC_INITIALIZED.load(Ordering::Acquire) != 0
}

// =============================================================================
// GSI Configuration
// =============================================================================

/// Configures GSI (Global System Interrupt) for a device
///
/// # Arguments
/// * `gsi` - Global System Interrupt number
/// * `vector` - Interrupt vector (0x30-0xFE)
/// * `destination` - Target CPU APIC ID
/// * `trigger_mode` - 0=Edge, 1=Level
/// * `polarity` - 0=ActiveHigh, 1=ActiveLow
pub fn ioapic_configure_gsi(gsi: u32, vector: u8, destination: u8, trigger_mode: u8, polarity: u8) {
    assert!(vector >= 0x30 && vector < 0xFF, "Invalid vector {}", vector);
    assert!(
        (gsi as usize) < MAX_GSI,
        "GSI {} exceeds MAX_GSI {}",
        gsi,
        MAX_GSI
    );

    let mut entry = IoApicRedirectionEntry::new();
    entry.vector = vector;
    entry.delivery_mode = 0; // Fixed
    entry.destination_mode = 0; // Physical
    entry.polarity = polarity;
    entry.trigger_mode = trigger_mode;
    entry.mask = true; // Keep masked until enable
    entry.destination = destination;

    ioapic_write_redirection_entry(gsi, &entry);

    // Update mapping tables
    unsafe {
        GSI_TO_VECTOR[gsi as usize] = vector;
        VECTOR_TO_GSI[vector as usize] = gsi as u8;
    }
}

/// Enables (unmasks) GSI
pub fn ioapic_enable_gsi(gsi: u32) {
    assert!(
        (gsi as usize) < MAX_GSI,
        "GSI {} exceeds MAX_GSI {}",
        gsi,
        MAX_GSI
    );

    let mut entry = ioapic_read_redirection_entry(gsi);
    entry.mask = false;
    ioapic_write_redirection_entry(gsi, &entry);
}

/// Disables (masks) GSI
pub fn ioapic_disable_gsi(gsi: u32) {
    assert!(
        (gsi as usize) < MAX_GSI,
        "GSI {} exceeds MAX_GSI {}",
        gsi,
        MAX_GSI
    );

    let mut entry = ioapic_read_redirection_entry(gsi);
    entry.mask = true;
    ioapic_write_redirection_entry(gsi, &entry);
}

/// Converts GSI to vector
#[inline]
pub fn ioapic_gsi_to_vector(gsi: u32) -> u8 {
    assert!((gsi as usize) < MAX_GSI);
    unsafe { GSI_TO_VECTOR[gsi as usize] }
}

/// Converts vector to GSI
#[inline]
pub fn ioapic_vector_to_gsi(vector: u8) -> Option<u32> {
    let gsi = unsafe { VECTOR_TO_GSI[vector as usize] };
    if gsi != 0xFF { Some(gsi as u32) } else { None }
}

// =============================================================================
// IRQ Override Functions
// =============================================================================

/// Loads interrupt overrides from ACPI MADT
///
/// # Arguments
/// * `overrides` - Slice of override entries from MADT
pub fn ioapic_load_overrides(overrides: &[(u8, u32, u8, u8)]) {
    for &(source_irq, global_irq, polarity, trigger_mode) in overrides {
        if source_irq < 16 {
            let ovr = IrqOverride {
                source_irq,
                global_irq,
                polarity,
                trigger_mode,
            };

            unsafe {
                IRQ_OVERRIDES[source_irq as usize] = Some(ovr);
            }
        }
    }
}

/// Resolves ISA IRQ to GSI with override consideration
///
/// # Arguments
/// * `isa_irq` - ISA IRQ number (0-15)
///
/// # Returns
/// (gsi, polarity, trigger_mode)
///
/// # Typical overrides:
/// - IRQ 0 (PIT) -> GSI 2 on many systems
/// - IRQ 9 (ACPI) -> may be level-triggered
pub fn ioapic_resolve_isa_irq(isa_irq: u8) -> (u32, u8, u8) {
    if isa_irq >= 16 {
        return (isa_irq as u32, 0, 0); // Active High, Edge
    }

    // Check for override
    if let Some(ovr) = unsafe { IRQ_OVERRIDES[isa_irq as usize] } {
        let polarity = match ovr.polarity {
            0 => 0, // Default (Active High for ISA)
            1 => 0, // Active High
            3 => 1, // Active Low
            _ => 0, // Default
        };

        let trigger = match ovr.trigger_mode {
            0 => 0, // Default (Edge for ISA)
            1 => 0, // Edge
            3 => 1, // Level
            _ => 0, // Default
        };

        (ovr.global_irq, polarity, trigger)
    } else {
        // No override - use ISA IRQ as GSI directly
        // For most systems, first 16 GSIs correspond to ISA IRQ 0-15
        (isa_irq as u32, 0, 0) // Active High, Edge (ISA default)
    }
}

// =============================================================================
// Query Functions
// =============================================================================

/// Returns IOAPIC physical address
#[inline]
pub fn ioapic_get_physical_address() -> u64 {
    IOAPIC_PHYSICAL_ADDRESS.load(Ordering::Acquire)
}

/// Returns number of IOAPIC entries
#[inline]
pub fn ioapic_get_num_entries() -> u32 {
    IOAPIC_NUM_ENTRIES.load(Ordering::Acquire)
}

/// Returns IOAPIC GSI base
#[inline]
pub fn ioapic_get_gsi_base() -> u32 {
    IOAPIC_GSI_BASE.load(Ordering::Acquire)
}

// =============================================================================
// ACPI MADT Configuration
// =============================================================================

/// Stored IOAPIC configuration from ACPI MADT
static mut MADT_IOAPIC_ADDRESS: u64 = 0;
static mut MADT_IOAPIC_GSI_BASE: u32 = 0;
static mut MADT_IOAPIC_VALID: bool = false;

/// Stored IRQ overrides from ACPI MADT
static mut MADT_OVERRIDES: [(u8, u32, u8, u8); 16] = [(0, 0, 0, 0); 16];
static mut MADT_OVERRIDES_COUNT: usize = 0;

/// Sets IOAPIC configuration from ACPI MADT
///
/// # Arguments
/// * `address` - Physical address of IOAPIC
/// * `gsi_base` - GSI base for this IOAPIC
pub fn ioapic_set_madt_config(address: u64, gsi_base: u32) {
    unsafe {
        MADT_IOAPIC_ADDRESS = address;
        MADT_IOAPIC_GSI_BASE = gsi_base;
        MADT_IOAPIC_VALID = true;
    }
}

/// Adds an IRQ override from ACPI MADT
///
/// # Arguments
/// * `source_irq` - ISA IRQ number
/// * `global_irq` - Target GSI
/// * `polarity` - Polarity flags from MADT
/// * `trigger_mode` - Trigger mode flags from MADT
pub fn ioapic_add_madt_override(source_irq: u8, global_irq: u32, polarity: u8, trigger_mode: u8) {
    unsafe {
        if MADT_OVERRIDES_COUNT < 16 && source_irq < 16 {
            MADT_OVERRIDES[MADT_OVERRIDES_COUNT] = (source_irq, global_irq, polarity, trigger_mode);
            MADT_OVERRIDES_COUNT += 1;
        }
    }
}

/// Returns stored MADT IOAPIC configuration
pub fn ioapic_get_madt_config() -> Option<(u64, u32)> {
    unsafe {
        if MADT_IOAPIC_VALID {
            Some((MADT_IOAPIC_ADDRESS, MADT_IOAPIC_GSI_BASE))
        } else {
            None
        }
    }
}

/// Applies stored MADT overrides
pub fn ioapic_apply_madt_overrides() {
    unsafe {
        if MADT_OVERRIDES_COUNT > 0 {
            ioapic_load_overrides(&MADT_OVERRIDES[..MADT_OVERRIDES_COUNT]);
        }
    }
}
