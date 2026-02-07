//! StorPort Driver for XenOS
//!
//! Storage Port Driver предоставляет интерфейс для storage miniport драйверов.
//! Это port driver, который управляет miniports (msahci, nvme, и т.д.)
//!
//! # Архитектура
//!
//! ```text
//! ┌──────────────┐
//! │   disk.sys   │ ◄── Class driver (SCSI abstraction)
//! └──────┬───────┘
//!        │
//!   ┌────▼──────────┐
//!   │ storport.sys  │ ◄── Port driver (этот модуль)
//!   │               │     • Miniport interface
//!   │               │     • SRB management
//!   │               │     • PnP/Power
//!   └───────┬───────┘
//!           │
//!      ┌────▼────┐
//!      │ miniport│ ◄── HW-specific (msahci.sys, nvme.sys, ...)
//!      └────┬────┘
//!           │
//!           ▼
//!      [Hardware]
//! ```
//!
//! # Источники
//! - Windows DDK: Storage Miniport Drivers
//! - ReactOS: drivers/storage/port/storport/

#![no_std]
#![allow(non_snake_case)]
#![allow(static_mut_refs)]

mod types;
mod imports;
mod miniport;
mod pnp;
mod power;
mod scsi;
mod layout_checks;

use types::*;
use imports::ntoskrnl::*;
use miniport::*;

// =============================================================================
// Global State
// =============================================================================

/// Pool tag для StorPort: 'SPRT'
pub const STORPORT_POOL_TAG: ULONG = 0x54525053;

// =============================================================================
// Driver Entry
// =============================================================================

/// DriverEntry - точка входа StorPort драйвера
///
/// Вызывается системой при загрузке драйвера.
/// НЕ создаёт устройства напрямую - это будет делать StorPortInitialize
/// вызванный из miniport DriverEntry.
#[unsafe(no_mangle)]
pub extern "win64" fn DriverEntry(
    driver_object: PDRIVER_OBJECT,
    _registry_path: *const UNICODE_STRING,
) -> NTSTATUS {
    unsafe {
        storport_print("[STORPORT] DriverEntry called\n");
        
        // Устанавливаем AddDevice callback
        let driver_ext = (*driver_object).driver_extension;
        if !driver_ext.is_null() {
            (*driver_ext).add_device = Some(miniport::StorPortAddDevice);
        }
        
        // Устанавливаем dispatch функции
        (*driver_object).major_function[IRP_MJ_PNP as usize] = Some(StorPortDispatchPnp);
        (*driver_object).major_function[IRP_MJ_POWER as usize] = Some(StorPortDispatchPower);
        (*driver_object).major_function[IRP_MJ_SCSI as usize] = Some(StorPortDispatchScsi);
        (*driver_object).major_function[IRP_MJ_CREATE as usize] = Some(StorPortDispatchCreate);
        (*driver_object).major_function[IRP_MJ_CLOSE as usize] = Some(StorPortDispatchClose);

        storport_print("[STORPORT] Driver initialized successfully\n");
        
        STATUS_SUCCESS
    }
}

// =============================================================================
// Dispatch Routines
// =============================================================================

/// PnP dispatch
unsafe extern "win64" fn StorPortDispatchPnp(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    pnp::storport_pnp_dispatch(device_object, irp)
}

/// Power dispatch
/// 
/// Phase 2.5: Корректная обработка Power IRP через модуль power.
unsafe extern "win64" fn StorPortDispatchPower(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    unsafe {
        power::storport_power_dispatch(device_object, irp)
    }
}

/// SCSI dispatch (IRP_MJ_INTERNAL_DEVICE_CONTROL)
unsafe extern "win64" fn StorPortDispatchScsi(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    scsi::storport_scsi_dispatch(device_object, irp)
}

/// CREATE dispatch
unsafe extern "win64" fn StorPortDispatchCreate(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    unsafe {
        (*irp).io_status.status = STATUS_SUCCESS;
        (*irp).io_status.information = 0;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        STATUS_SUCCESS
    }
}

/// CLOSE dispatch
unsafe extern "win64" fn StorPortDispatchClose(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    unsafe {
        (*irp).io_status.status = STATUS_SUCCESS;
        (*irp).io_status.information = 0;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        STATUS_SUCCESS
    }
}

// =============================================================================
// Debug Helpers
// =============================================================================

/// Выводит строку через DbgPrint (только при storage-trace feature)
#[cfg(feature = "storage-trace")]
pub unsafe fn storport_print(s: &str) {
    let mut buf = [0u8; 256];
    let len = s.len().min(255);
    for (i, &b) in s.as_bytes().iter().take(len).enumerate() {
        buf[i] = b;
    }
    buf[len] = 0;
    DbgPrint(buf.as_ptr());
}

#[cfg(not(feature = "storage-trace"))]
#[inline(always)]
pub unsafe fn storport_print(_s: &str) {}

/// Выводит число в hex (только при storage-trace feature)
#[cfg(feature = "storage-trace")]
pub unsafe fn storport_print_hex(value: u64) {
    const HEX_CHARS: &[u8] = b"0123456789ABCDEF";
    let mut buf = [0u8; 17];
    
    if value == 0 {
        buf[0] = b'0';
        buf[1] = 0;
        DbgPrint(buf.as_ptr());
        return;
    }
    
    let mut start = 0;
    for i in 0..16 {
        let shift = (15 - i) * 4;
        let digit = ((value >> shift) & 0xF) as usize;
        if digit != 0 || start > 0 {
            buf[start] = HEX_CHARS[digit];
            start += 1;
        }
    }
    buf[start] = 0;
    
    DbgPrint(buf.as_ptr());
}

#[cfg(not(feature = "storage-trace"))]
#[inline(always)]
pub unsafe fn storport_print_hex(_value: u64) {}

// =============================================================================
// Panic Handler
// =============================================================================

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

/// Required by compiler_builtins
#[unsafe(no_mangle)]
pub static _fltused: i32 = 0;

