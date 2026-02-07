//! Internal ACPI Bus Driver
//!
//! Создаёт и управляет internal DRIVER_OBJECT для обработки IRP на ACPI PDO.
//!
//! В отличие от внешних драйверов (pci.dll и т.д.), этот "драйвер" встроен
//! в ядро и используется для обработки PnP IRP для PDO, созданных
//! ACPI bus enumerator'ом.

extern crate alloc;

use core::ptr;
use core::sync::atomic::{AtomicPtr, Ordering};

use crate::ex::pool::{ex_allocate_pool_with_tag, POOL_TYPE};
use crate::io::driver::{DRIVER_OBJECT, DRIVER_EXTENSION};
use crate::io::irp::IRP;
use crate::io::device::DEVICE_OBJECT;
use crate::io::types::{IRP_MJ_PNP, IRP_MJ_POWER};
use crate::io::types::{PDEVICE_OBJECT, PDRIVER_OBJECT, PIRP};
use crate::nt::ntstatus::*;
use crate::nt::NTSTATUS;

use super::irp::{acpi_pdo_pnp_dispatch, acpi_pdo_power_dispatch};

/// Глобальный internal ACPI driver object
static ACPI_INTERNAL_DRIVER: AtomicPtr<DRIVER_OBJECT> = AtomicPtr::new(ptr::null_mut());

/// Инициализирует internal ACPI driver
///
/// Создаёт DRIVER_OBJECT с dispatch routines для PnP и Power IRP.
/// Должен вызываться один раз при инициализации PnP.
pub unsafe fn acpi_init_internal_driver() -> NTSTATUS {
    // Проверяем, не инициализирован ли уже
    if !ACPI_INTERNAL_DRIVER.load(Ordering::Acquire).is_null() {
        return STATUS_SUCCESS;
    }

    // Выделяем DRIVER_OBJECT
    let driver_size = core::mem::size_of::<DRIVER_OBJECT>();
    let driver_ptr = ex_allocate_pool_with_tag(
        POOL_TYPE::NonPagedPool,
        driver_size,
        u32::from_le_bytes(*b"AcDr"),
    );

    if driver_ptr.is_null() {
        return STATUS_INSUFFICIENT_RESOURCES;
    }

    // Инициализируем
    core::ptr::write_bytes(driver_ptr, 0, driver_size);

    let driver = driver_ptr as *mut DRIVER_OBJECT;
    
    // Устанавливаем dispatch routines
    (*driver).major_function[IRP_MJ_PNP as usize] = Some(acpi_internal_pnp_dispatch);
    (*driver).major_function[IRP_MJ_POWER as usize] = Some(acpi_internal_power_dispatch);

    // Выделяем driver extension
    let ext_size = core::mem::size_of::<DRIVER_EXTENSION>();
    let ext_ptr = ex_allocate_pool_with_tag(
        POOL_TYPE::NonPagedPool,
        ext_size,
        u32::from_le_bytes(*b"AcDx"),
    );

    if !ext_ptr.is_null() {
        core::ptr::write_bytes(ext_ptr, 0, ext_size);
        (*driver).driver_extension = ext_ptr as *mut DRIVER_EXTENSION;
    }

    // Сохраняем глобально
    ACPI_INTERNAL_DRIVER.store(driver, Ordering::Release);

    STATUS_SUCCESS
}

/// Возвращает указатель на internal ACPI driver
pub fn acpi_get_internal_driver() -> PDRIVER_OBJECT {
    ACPI_INTERNAL_DRIVER.load(Ordering::Acquire)
}

/// PnP dispatch routine для internal ACPI driver
unsafe extern "win64" fn acpi_internal_pnp_dispatch(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    acpi_pdo_pnp_dispatch(device_object, irp)
}

/// Power dispatch routine для internal ACPI driver
unsafe extern "win64" fn acpi_internal_power_dispatch(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    acpi_pdo_power_dispatch(device_object, irp)
}

