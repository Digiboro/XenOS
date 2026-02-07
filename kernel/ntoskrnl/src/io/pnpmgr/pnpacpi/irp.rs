//! ACPI Bus Driver IRP Handlers
//!
//! Обработчики PnP и Power IRP для ACPI bus driver.
//!
//! # Поддерживаемые IRP
//!
//! ## PnP IRP (IRP_MJ_PNP)
//! - IRP_MN_START_DEVICE
//! - IRP_MN_QUERY_DEVICE_RELATIONS (BusRelations)
//! - IRP_MN_QUERY_ID (HardwareID, CompatibleIDs, InstanceID)
//! - IRP_MN_QUERY_CAPABILITIES
//! - IRP_MN_QUERY_RESOURCES
//!
//! ## Power IRP (IRP_MJ_POWER)
//! - IRP_MN_SET_POWER
//! - IRP_MN_QUERY_POWER

extern crate alloc;

use alloc::string::String;
use core::ptr;

use crate::io::device::DEVICE_OBJECT;
use crate::io::irp::{io_get_current_irp_stack_location, iof_complete_request, IRP};
use crate::io::pnp::{
    BUS_QUERY_ID_TYPE, DEVICE_CAPABILITIES, DEVICE_RELATIONS, DEVICE_RELATION_TYPE,
    IRP_MN_QUERY_CAPABILITIES, IRP_MN_QUERY_DEVICE_RELATIONS, IRP_MN_QUERY_ID,
    IRP_MN_QUERY_RESOURCES, IRP_MN_START_DEVICE, IRP_MN_SET_POWER, IRP_MN_QUERY_POWER,
};
use crate::io::types::PIRP;
use crate::nt::ntstatus::*;
use crate::nt::NTSTATUS;

use super::device::AcpiDeviceExtension;

/// Обработчик PnP IRP для ACPI PDO
///
/// Вызывается для PDO, созданных ACPI bus driver'ом.
pub unsafe fn acpi_pdo_pnp_dispatch(
    device_object: *mut DEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    if device_object.is_null() || irp.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    let stack = io_get_current_irp_stack_location(irp);
    if stack.is_null() {
        (*irp).io_status.status = STATUS_UNSUCCESSFUL;
        iof_complete_request(irp, 0);
        return STATUS_UNSUCCESSFUL;
    }

    let minor = (*stack).minor_function;

    let status = match minor {
        IRP_MN_START_DEVICE => acpi_pdo_start_device(device_object, irp),
        IRP_MN_QUERY_DEVICE_RELATIONS => acpi_pdo_query_device_relations(device_object, irp),
        IRP_MN_QUERY_ID => acpi_pdo_query_id(device_object, irp),
        IRP_MN_QUERY_CAPABILITIES => acpi_pdo_query_capabilities(device_object, irp),
        IRP_MN_QUERY_RESOURCES => acpi_pdo_query_resources(device_object, irp),
        _ => {
            // Неподдерживаемые запросы — возвращаем текущий статус
            // (STATUS_NOT_SUPPORTED если IRP новый)
            (*irp).io_status.status
        }
    };

    (*irp).io_status.status = status;
    iof_complete_request(irp, 0);
    status
}

/// IRP_MN_START_DEVICE — запуск ACPI устройства
unsafe fn acpi_pdo_start_device(
    device_object: *mut DEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    // Для ACPI PDO, запуск означает что устройство готово к работе
    // Мы могли бы вызвать _STA и _INI здесь, но обычно это делается при enumeration

    // TODO: Обработать ресурсы из AllocatedResources и AllocatedResourcesTranslated
    // в IO_STACK_LOCATION.Parameters.StartDevice

    (*irp).io_status.information = 0;
    STATUS_SUCCESS
}

/// IRP_MN_QUERY_DEVICE_RELATIONS — запрос связей устройства
unsafe fn acpi_pdo_query_device_relations(
    device_object: *mut DEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    let stack = io_get_current_irp_stack_location(irp);

    // Получаем тип связей из Parameters.QueryDeviceRelations.Type
    // В нашей упрощённой IO_STACK_LOCATION это в parameters
    // TODO: Правильно получить тип из parameters

    // Для PDO:
    // - BusRelations: возвращаем дочерние устройства (для bus PDO)
    // - TargetDeviceRelation: возвращаем себя (обязательно для PDO)
    // - Остальные: не поддерживаем

    // Пока возвращаем пустой список BusRelations
    // Реальная реализация должна enumeration ACPI namespace

    // Выделяем DEVICE_RELATIONS для одного устройства (TargetDeviceRelation)
    use crate::ex::pool::{ex_allocate_pool_with_tag, POOL_TYPE};

    let relations_size = core::mem::size_of::<DEVICE_RELATIONS>();
    let relations = ex_allocate_pool_with_tag(
        POOL_TYPE::NonPagedPool,
        relations_size,
        u32::from_le_bytes(*b"AcRl"),
    ) as *mut DEVICE_RELATIONS;

    if relations.is_null() {
        return STATUS_INSUFFICIENT_RESOURCES;
    }

    // Для TargetDeviceRelation — возвращаем PDO
    (*relations).count = 1;
    (*relations).objects[0] = device_object;

    // Увеличиваем reference count на устройство
    // TODO: ObReferenceObject(device_object);

    (*irp).io_status.information = relations as usize;
    STATUS_SUCCESS
}

/// IRP_MN_QUERY_ID — запрос ID устройства
unsafe fn acpi_pdo_query_id(
    device_object: *mut DEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    use crate::ex::pool::{ex_allocate_pool_with_tag, POOL_TYPE};

    let stack = io_get_current_irp_stack_location(irp);

    // Получаем DeviceExtension
    let dev_ext = (*device_object).device_extension as *const AcpiDeviceExtension;
    if dev_ext.is_null() {
        return STATUS_INVALID_DEVICE_REQUEST;
    }

    // TODO: Правильно получить IdType из parameters
    // Пока возвращаем HardwareID

    let hardware_id = &(*dev_ext).hardware_id;

    // Формируем Device ID в формате "ACPI\{HID}"
    let device_id = super::ids::format_hardware_id(hardware_id);

    // Выделяем буфер для ID (multi-sz: строка + два нуля)
    let buffer_size = (device_id.len() + 2) * 2; // Unicode
    let buffer = ex_allocate_pool_with_tag(
        POOL_TYPE::PagedPool,
        buffer_size,
        u32::from_le_bytes(*b"AcId"),
    ) as *mut u16;

    if buffer.is_null() {
        return STATUS_INSUFFICIENT_RESOURCES;
    }

    // Копируем строку в Unicode
    let bytes = device_id.as_bytes();
    for (i, &byte) in bytes.iter().enumerate() {
        *buffer.add(i) = byte as u16;
    }
    // Два терминатора
    *buffer.add(bytes.len()) = 0;
    *buffer.add(bytes.len() + 1) = 0;

    (*irp).io_status.information = buffer as usize;
    STATUS_SUCCESS
}

/// IRP_MN_QUERY_CAPABILITIES — запрос возможностей устройства
unsafe fn acpi_pdo_query_capabilities(
    device_object: *mut DEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    let stack = io_get_current_irp_stack_location(irp);

    // TODO: Получить указатель на DEVICE_CAPABILITIES из parameters
    // и заполнить его

    // Пока просто возвращаем успех
    (*irp).io_status.information = 0;
    STATUS_SUCCESS
}

/// IRP_MN_QUERY_RESOURCES — запрос ресурсов устройства
unsafe fn acpi_pdo_query_resources(
    device_object: *mut DEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    // Получаем DeviceExtension
    let dev_ext = (*device_object).device_extension as *const AcpiDeviceExtension;
    if dev_ext.is_null() {
        return STATUS_INVALID_DEVICE_REQUEST;
    }

    // TODO: Вызвать _CRS (Current Resource Settings) из AML
    // и преобразовать в CM_RESOURCE_LIST

    // Пока возвращаем NULL (нет ресурсов)
    (*irp).io_status.information = 0;
    STATUS_SUCCESS
}

/// Обработчик Power IRP для ACPI PDO
pub unsafe fn acpi_pdo_power_dispatch(
    device_object: *mut DEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    use crate::io::pnp::po_start_next_power_irp;

    if device_object.is_null() || irp.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    let stack = io_get_current_irp_stack_location(irp);
    let minor = (*stack).minor_function;

    // Сигнализируем готовность к следующему Power IRP
    po_start_next_power_irp(irp);

    let status = match minor {
        IRP_MN_SET_POWER | IRP_MN_QUERY_POWER => {
            // TODO: Для SET_POWER вызвать соответствующие ACPI методы
            // (_PS0, _PS1, _PS2, _PS3 для power states)
            STATUS_SUCCESS
        }
        _ => STATUS_SUCCESS,
    };

    (*irp).io_status.status = status;
    (*irp).io_status.information = 0;
    iof_complete_request(irp, 0);
    status
}

