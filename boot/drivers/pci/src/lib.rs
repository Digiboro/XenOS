//! PCI Bus Driver for XenOS
//!
//! Системный PCI bus driver, который:
//! - Загружается как boot driver при обнаружении ACPI PCI Host Bridge (PNP0A03/PNP0A08)
//! - Выполняет enumeration PCI конфигурационного пространства
//! - Создаёт PDO для каждого обнаруженного PCI устройства
//! - Обрабатывает PnP IRP для BusRelations, QueryID, и т.д.
//!
//! # Архитектура
//!
//! ```text
//! ACPI PDO (PNP0A03)
//!       │
//!       │  AddDevice
//!       ▼
//! ┌─────────────┐
//! │   PCI FDO   │  ← Этот драйвер
//! │ (pci.sys)   │
//! └──────┬──────┘
//!        │
//!        │  BusRelations enumeration
//!        ▼
//! ┌──────────────────────────────────────┐
//! │ PCI PDOs (VEN_xxxx&DEV_yyyy)         │
//! │                                      │
//! │ ┌─────────┐ ┌─────────┐ ┌─────────┐  │
//! │ │ Device 0│ │ Device 1│ │ Device N│  │
//! │ │ Func 0  │ │ Func 0  │ │ Func 0  │  │
//! │ └─────────┘ └─────────┘ └─────────┘  │
//! └──────────────────────────────────────┘
//! ```
//!
//! Источники:
//! - ReactOS: drivers/bus/pci/
//! - Windows DDK: PCI bus driver documentation

#![no_std]
#![no_main]
#![feature(lang_items)]
#![allow(internal_features)]
#![allow(non_snake_case)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]

use core::panic::PanicInfo;
use core::ptr;

mod config;
mod debug;
mod enumerate;
mod imports;
mod pnp;
mod types;

use types::*;

// =============================================================================
// Глобальные переменные драйвера
// =============================================================================

/// Глобальный указатель на FDO (для отладки)
static mut GLOBAL_FDO: PDEVICE_OBJECT = ptr::null_mut();

/// Количество enumerated устройств
static mut PCI_DEVICE_COUNT: u32 = 0;

// =============================================================================
// DriverEntry
// =============================================================================

/// DriverEntry — точка входа PCI bus driver
///
/// Устанавливает:
/// - AddDevice callback для привязки к ACPI PDO
/// - PnP dispatch handlers
/// - Power dispatch handlers
#[unsafe(no_mangle)]
pub extern "win64" fn DriverEntry(
    driver_object: PDRIVER_OBJECT,
    _registry_path: *const UNICODE_STRING,
) -> NTSTATUS {
    unsafe {
        // Устанавливаем AddDevice callback
        let driver_ext = (*driver_object).driver_extension;
        if !driver_ext.is_null() {
            (*driver_ext).add_device = Some(PciAddDevice);
        }

        // Устанавливаем dispatch функции
        (*driver_object).major_function[IRP_MJ_PNP as usize] = Some(PciDispatchPnp);
        (*driver_object).major_function[IRP_MJ_POWER as usize] = Some(PciDispatchPower);
        (*driver_object).major_function[IRP_MJ_CREATE as usize] = Some(PciDispatchCreate);
        (*driver_object).major_function[IRP_MJ_CLOSE as usize] = Some(PciDispatchClose);

        // DriverUnload для динамической выгрузки (не используется для boot drivers)
        (*driver_object).driver_unload = Some(PciDriverUnload);
    }

    STATUS_SUCCESS
}

// =============================================================================
// AddDevice
// =============================================================================

/// PciAddDevice — вызывается PnP Manager при обнаружении PCI Host Bridge
///
/// Создаёт FDO и привязывает его к PDO (ACPI устройство PNP0A03/PNP0A08)
unsafe extern "win64" fn PciAddDevice(
    driver_object: PDRIVER_OBJECT,
    physical_device_object: PDEVICE_OBJECT, // ACPI PDO
) -> NTSTATUS {
    // 1. Создаём FDO
    let mut fdo: PDEVICE_OBJECT = ptr::null_mut();

    let status = IoCreateDevice(
        driver_object,
        core::mem::size_of::<PCI_FDO_EXTENSION>() as u32,
        ptr::null(), // Без имени
        FILE_DEVICE_BUS_EXTENDER,
        0,
        0, // Not exclusive
        &mut fdo,
    );

    if status != STATUS_SUCCESS {
        return status;
    }

    // 2. Инициализируем FDO extension
    let fdo_ext = (*fdo).device_extension as *mut PCI_FDO_EXTENSION;
    ptr::write_bytes(fdo_ext, 0, 1);

    (*fdo_ext).common.is_fdo = 1;
    (*fdo_ext).common.self_device = fdo;
    (*fdo_ext).physical_device_object = physical_device_object;

    // 3. Присоединяем FDO к device stack
    let lower_device = IoAttachDeviceToDeviceStack(fdo, physical_device_object);
    if lower_device.is_null() {
        IoDeleteDevice(fdo);
        return STATUS_NO_SUCH_DEVICE;
    }
    (*fdo_ext).lower_device = lower_device;

    // 4. Копируем флаги с нижележащего устройства
    (*fdo).flags |= (*lower_device).flags & (DO_BUFFERED_IO | DO_DIRECT_IO | DO_POWER_PAGABLE);

    // 5. Устройство готово
    (*fdo).flags &= !DO_DEVICE_INITIALIZING;

    // Сохраняем для отладки
    core::ptr::write_volatile(&raw mut GLOBAL_FDO, fdo);

    STATUS_SUCCESS
}

// =============================================================================
// Dispatch Functions
// =============================================================================

/// PciDispatchPnp — обработчик IRP_MJ_PNP
unsafe extern "win64" fn PciDispatchPnp(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    let ext = (*device_object).device_extension as *const PCI_COMMON_EXTENSION;

    if (*ext).is_fdo != 0 {
        pnp::pci_fdo_pnp_dispatch(device_object, irp)
    } else {
        pnp::pci_pdo_pnp_dispatch(device_object, irp)
    }
}

/// PciDispatchPower — обработчик IRP_MJ_POWER
unsafe extern "win64" fn PciDispatchPower(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    let ext = (*device_object).device_extension as *const PCI_COMMON_EXTENSION;

    // Для Power IRP нужно вызвать PoStartNextPowerIrp
    PoStartNextPowerIrp(irp);

    if (*ext).is_fdo != 0 {
        // FDO: передаём вниз
        let fdo_ext = ext as *const PCI_FDO_EXTENSION;
        IoSkipCurrentIrpStackLocation(irp);
        PoCallDriver((*fdo_ext).lower_device, irp)
    } else {
        // PDO: успех
        (*irp).io_status.status = STATUS_SUCCESS;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        STATUS_SUCCESS
    }
}

/// PciDispatchCreate — обработчик IRP_MJ_CREATE
unsafe extern "win64" fn PciDispatchCreate(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    (*irp).io_status.status = STATUS_SUCCESS;
    (*irp).io_status.information = 0;
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    STATUS_SUCCESS
}

/// PciDispatchClose — обработчик IRP_MJ_CLOSE
unsafe extern "win64" fn PciDispatchClose(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    (*irp).io_status.status = STATUS_SUCCESS;
    (*irp).io_status.information = 0;
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    STATUS_SUCCESS
}

/// PciDriverUnload — выгрузка драйвера
unsafe extern "win64" fn PciDriverUnload(_driver_object: PDRIVER_OBJECT) {
    // Boot drivers обычно не выгружаются
}

// =============================================================================
// Panic handler & lang items
// =============================================================================

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

#[lang = "eh_personality"]
extern "C" fn eh_personality() {}

#[unsafe(no_mangle)]
pub static _fltused: i32 = 0;

