//! PnP IRP Handlers for PCI Bus Driver
//!
//! Обработчики PnP IRP для FDO (Host Bridge) и PDO (PCI devices).

use crate::config::{
    PciDeviceInfo, pci_program_bar, pci_enable_device,
};
use crate::enumerate::{
    pci_enumerate_bus, pci_create_pdo,
    format_device_id, format_hardware_ids, format_instance_id, format_compatible_ids,
};
use crate::types::*;
use core::ptr;

// =============================================================================
// FDO PnP Dispatch
// =============================================================================

/// Обработчик PnP IRP для FDO (PCI Host Bridge)
pub unsafe fn pci_fdo_pnp_dispatch(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    let stack = IoGetCurrentIrpStackLocation(irp);
    let minor = (*stack).minor_function;
    let fdo_ext = (*device_object).device_extension as *mut PCI_FDO_EXTENSION;

    match minor {
        IRP_MN_START_DEVICE => pci_fdo_start_device(device_object, irp, fdo_ext),
        IRP_MN_QUERY_DEVICE_RELATIONS => pci_fdo_query_device_relations(device_object, irp, fdo_ext),
        IRP_MN_QUERY_STOP_DEVICE
        | IRP_MN_CANCEL_STOP_DEVICE
        | IRP_MN_QUERY_REMOVE_DEVICE
        | IRP_MN_CANCEL_REMOVE_DEVICE => {
            // Передаём вниз и возвращаем успех
            IoSkipCurrentIrpStackLocation(irp);
            IoCallDriver((*fdo_ext).lower_device, irp)
        }
        IRP_MN_STOP_DEVICE => {
            (*fdo_ext).started = 0;
            IoSkipCurrentIrpStackLocation(irp);
            IoCallDriver((*fdo_ext).lower_device, irp)
        }
        IRP_MN_REMOVE_DEVICE => pci_fdo_remove_device(device_object, irp, fdo_ext),
        IRP_MN_SURPRISE_REMOVAL => {
            (*irp).io_status.status = STATUS_SUCCESS;
            IoSkipCurrentIrpStackLocation(irp);
            IoCallDriver((*fdo_ext).lower_device, irp)
        }
        _ => {
            // Передаём неподдерживаемые IRP вниз
            IoSkipCurrentIrpStackLocation(irp);
            IoCallDriver((*fdo_ext).lower_device, irp)
        }
    }
}

/// IRP_MN_START_DEVICE для FDO
unsafe fn pci_fdo_start_device(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
    fdo_ext: *mut PCI_FDO_EXTENSION,
) -> NTSTATUS {
    // Сначала передаём вниз и ждём завершения
    // В упрощённой версии просто передаём и помечаем как started

    (*fdo_ext).started = 1;

    // Получаем bus number из allocated resources
    let stack = IoGetCurrentIrpStackLocation(irp);
    let allocated_resources = (*stack).parameters.start_device.allocated_resources_translated as *const CM_RESOURCE_LIST;
    
    if !allocated_resources.is_null() && (*allocated_resources).count > 0 {
        let partial_list = &(*allocated_resources).list[0].partial_resource_list;
        let descriptors = &partial_list.partial_descriptors as *const CM_PARTIAL_RESOURCE_DESCRIPTOR;
        
        // Ищем BUS_NUMBER resource
        for i in 0..partial_list.count as usize {
            let desc = descriptors.add(i);
            if (*desc).r#type == CM_RESOURCE_TYPE_BUS_NUMBER {
                // BUS_NUMBER resource: u.bus_number.Start содержит начальный bus number
                // В нашей реализации используем первые 4 байта union как start bus
                let bus_start = (*desc).u.raw[0];
                (*fdo_ext).bus_number = (bus_start & 0xFF) as u8;
                break;
            }
        }
    }
    
    // Если bus number не был найден в ресурсах, используем 0 (root bus)
    // Это типичное поведение для PCI Host Bridge

    (*irp).io_status.status = STATUS_SUCCESS;
    IoSkipCurrentIrpStackLocation(irp);
    IoCallDriver((*fdo_ext).lower_device, irp)
}

/// IRP_MN_QUERY_DEVICE_RELATIONS для FDO
unsafe fn pci_fdo_query_device_relations(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
    fdo_ext: *mut PCI_FDO_EXTENSION,
) -> NTSTATUS {
    let stack = IoGetCurrentIrpStackLocation(irp);
    let relation_type = (*stack).parameters.query_device_relations.r#type;

    if relation_type != BUS_RELATIONS {
        // Не BusRelations — передаём вниз
        IoSkipCurrentIrpStackLocation(irp);
        return IoCallDriver((*fdo_ext).lower_device, irp);
    }

    // BusRelations — enumerate PCI bus и создаём PDOs

    // Считаем устройства
    struct CountContext {
        count: u32,
    }
    let mut count_ctx = CountContext { count: 0 };

    unsafe fn count_callback(_info: &PciDeviceInfo, context: PVOID) -> bool {
        let ctx = context as *mut CountContext;
        (*ctx).count += 1;
        true
    }

    let enumerated = pci_enumerate_bus(
        (*fdo_ext).bus_number,
        count_callback,
        &mut count_ctx as *mut _ as PVOID,
    );

    // Debug: print enumeration results
    // (В реальном коде нужно использовать правильный debug output, но пока так)
    
    if count_ctx.count == 0 {
        // Нет устройств
        (*irp).io_status.status = STATUS_SUCCESS;
        (*irp).io_status.information = 0;
        IoSkipCurrentIrpStackLocation(irp);
        return IoCallDriver((*fdo_ext).lower_device, irp);
    }

    // Выделяем DEVICE_RELATIONS
    let relations_size = core::mem::size_of::<DEVICE_RELATIONS>()
        + (count_ctx.count as usize - 1) * core::mem::size_of::<PDEVICE_OBJECT>();

    let relations = ExAllocatePoolWithTag(NON_PAGED_POOL, relations_size, PCI_POOL_TAG)
        as *mut DEVICE_RELATIONS;

    if relations.is_null() {
        (*irp).io_status.status = STATUS_INSUFFICIENT_RESOURCES;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_INSUFFICIENT_RESOURCES;
    }

    (*relations).count = 0;

    // Создаём PDOs
    struct CreateContext {
        driver: PDRIVER_OBJECT,
        fdo: PDEVICE_OBJECT,
        relations: *mut DEVICE_RELATIONS,
    }
    let mut create_ctx = CreateContext {
        driver: (*device_object).driver_object,
        fdo: device_object,
        relations,
    };

    unsafe fn create_callback(info: &PciDeviceInfo, context: PVOID) -> bool {
        let ctx = &mut *(context as *mut CreateContext);

        let pdo = pci_create_pdo(ctx.driver, ctx.fdo, info);
        if !pdo.is_null() {
            let idx = (*ctx.relations).count as usize;
            
            // Reference для PnP Manager
            ObReferenceObject(pdo as PVOID);
            
            // Записываем PDO в relations после ObReferenceObject
            let objects_field_ptr = (ctx.relations as usize + 8) as *mut PDEVICE_OBJECT;
            let target = objects_field_ptr.add(idx);
            ptr::write(target, pdo);
            
            (*ctx.relations).count += 1;
        }
        true
    }

    pci_enumerate_bus(
        (*fdo_ext).bus_number,
        create_callback,
        &mut create_ctx as *mut _ as PVOID,
    );

    (*irp).io_status.status = STATUS_SUCCESS;
    (*irp).io_status.information = relations as ULONG_PTR;

    // FDO завершает IRP, а не передаёт вниз
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    STATUS_SUCCESS
}

/// IRP_MN_REMOVE_DEVICE для FDO
unsafe fn pci_fdo_remove_device(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
    fdo_ext: *mut PCI_FDO_EXTENSION,
) -> NTSTATUS {
    // Передаём вниз
    IoSkipCurrentIrpStackLocation(irp);
    let status = IoCallDriver((*fdo_ext).lower_device, irp);

    // Отсоединяемся
    IoDetachDevice((*fdo_ext).lower_device);

    // Удаляем FDO
    IoDeleteDevice(device_object);

    status
}

// =============================================================================
// PDO PnP Dispatch
// =============================================================================

/// Обработчик PnP IRP для PDO (PCI device)
pub unsafe fn pci_pdo_pnp_dispatch(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    let stack = IoGetCurrentIrpStackLocation(irp);
    let minor = (*stack).minor_function;
    let pdo_ext = (*device_object).device_extension as *mut PCI_PDO_EXTENSION;

    match minor {
        IRP_MN_START_DEVICE => pci_pdo_start_device(device_object, irp, pdo_ext),
        IRP_MN_QUERY_DEVICE_RELATIONS => pci_pdo_query_device_relations(device_object, irp, pdo_ext),
        IRP_MN_QUERY_ID => pci_pdo_query_id(device_object, irp, pdo_ext),
        IRP_MN_QUERY_CAPABILITIES => pci_pdo_query_capabilities(device_object, irp, pdo_ext),
        IRP_MN_QUERY_RESOURCES => pci_pdo_query_resources(device_object, irp, pdo_ext),
        IRP_MN_QUERY_RESOURCE_REQUIREMENTS => pci_pdo_query_resource_requirements(device_object, irp, pdo_ext),
        IRP_MN_QUERY_STOP_DEVICE
        | IRP_MN_CANCEL_STOP_DEVICE
        | IRP_MN_STOP_DEVICE
        | IRP_MN_QUERY_REMOVE_DEVICE
        | IRP_MN_CANCEL_REMOVE_DEVICE => {
            (*irp).io_status.status = STATUS_SUCCESS;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            STATUS_SUCCESS
        }
        IRP_MN_REMOVE_DEVICE => {
            (*pdo_ext).present = 0;
            (*irp).io_status.status = STATUS_SUCCESS;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            STATUS_SUCCESS
        }
        IRP_MN_SURPRISE_REMOVAL => {
            (*pdo_ext).present = 0;
            (*irp).io_status.status = STATUS_SUCCESS;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            STATUS_SUCCESS
        }
        _ => {
            // Неподдерживаемые запросы
            let status = (*irp).io_status.status;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            status
        }
    }
}

/// IRP_MN_QUERY_DEVICE_RELATIONS для PDO
unsafe fn pci_pdo_query_device_relations(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
    pdo_ext: *mut PCI_PDO_EXTENSION,
) -> NTSTATUS {
    let stack = IoGetCurrentIrpStackLocation(irp);
    let relation_type = (*stack).parameters.query_device_relations.r#type;

    if relation_type != TARGET_DEVICE_RELATION {
        // Только TargetDeviceRelation поддерживается для PDO
        (*irp).io_status.status = STATUS_NOT_SUPPORTED;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_NOT_SUPPORTED;
    }

    // TargetDeviceRelation — возвращаем себя
    let relations = ExAllocatePoolWithTag(
        NON_PAGED_POOL,
        core::mem::size_of::<DEVICE_RELATIONS>(),
        PCI_POOL_TAG,
    ) as *mut DEVICE_RELATIONS;

    if relations.is_null() {
        (*irp).io_status.status = STATUS_INSUFFICIENT_RESOURCES;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_INSUFFICIENT_RESOURCES;
    }

    (*relations).count = 1;
    (*relations).objects[0] = device_object;
    ObReferenceObject(device_object as PVOID);

    (*irp).io_status.status = STATUS_SUCCESS;
    (*irp).io_status.information = relations as ULONG_PTR;
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    STATUS_SUCCESS
}

/// IRP_MN_QUERY_ID для PDO
unsafe fn pci_pdo_query_id(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
    pdo_ext: *mut PCI_PDO_EXTENSION,
) -> NTSTATUS {
    let stack = IoGetCurrentIrpStackLocation(irp);
    let id_type = (*stack).parameters.query_id.id_type;

    // Создаём PciDeviceInfo для форматирования
    let info = PciDeviceInfo {
        bus: (*pdo_ext).bus_number,
        device: (*pdo_ext).device_number,
        function: (*pdo_ext).function_number,
        vendor_id: (*pdo_ext).vendor_id,
        device_id: (*pdo_ext).device_id,
        base_class: (*pdo_ext).base_class,
        sub_class: (*pdo_ext).sub_class,
        prog_if: (*pdo_ext).programming_interface,
        revision_id: (*pdo_ext).revision_id,
        header_type: (*pdo_ext).header_type,
        subsystem_vendor_id: (*pdo_ext).subsystem_vendor_id,
        subsystem_id: (*pdo_ext).subsystem_id,
    };

    // Буфер для ID (256 символов должно хватить)
    const BUFFER_SIZE: usize = 256;
    let buffer = ExAllocatePoolWithTag(
        PAGED_POOL,
        BUFFER_SIZE * 2, // Unicode
        PCI_POOL_TAG,
    ) as *mut u16;

    if buffer.is_null() {
        (*irp).io_status.status = STATUS_INSUFFICIENT_RESOURCES;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_INSUFFICIENT_RESOURCES;
    }

    let slice = core::slice::from_raw_parts_mut(buffer, BUFFER_SIZE);

    let len = match id_type {
        BUS_QUERY_DEVICE_ID => format_device_id(&info, slice),
        BUS_QUERY_HARDWARE_IDS => format_hardware_ids(&info, slice),
        BUS_QUERY_COMPATIBLE_IDS => {
            // Генерируем compatible IDs на основе class codes:
            // - PCI\VEN_xxxx&CC_ccsspp
            // - PCI\VEN_xxxx&CC_ccss
            // - PCI\CC_ccsspp
            // - PCI\CC_ccss
            format_compatible_ids(&info, slice)
        }
        BUS_QUERY_INSTANCE_ID => format_instance_id(&info, slice),
        _ => {
            ExFreePoolWithTag(buffer as PVOID, PCI_POOL_TAG);
            (*irp).io_status.status = STATUS_NOT_SUPPORTED;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            return STATUS_NOT_SUPPORTED;
        }
    };

    (*irp).io_status.status = STATUS_SUCCESS;
    (*irp).io_status.information = buffer as ULONG_PTR;
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    STATUS_SUCCESS
}

/// IRP_MN_QUERY_CAPABILITIES для PDO
unsafe fn pci_pdo_query_capabilities(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
    pdo_ext: *mut PCI_PDO_EXTENSION,
) -> NTSTATUS {
    let stack = IoGetCurrentIrpStackLocation(irp);
    
    // Получаем указатель на DEVICE_CAPABILITIES из parameters
    let params_ptr = &(*stack).parameters as *const _ as *const u8;
    let capabilities = core::ptr::read(params_ptr as *const *mut DEVICE_CAPABILITIES);
    
    if capabilities.is_null() {
        (*irp).io_status.status = STATUS_INVALID_PARAMETER;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_INVALID_PARAMETER;
    }
    
    // Заполняем DEVICE_CAPABILITIES для PCI устройства
    // Size и Version уже установлены вызывающим кодом
    
    // Power capabilities - PCI устройства обычно поддерживают D0 и D3
    (*capabilities).device_d1 = 0; // D1 обычно не поддерживается
    (*capabilities).device_d2 = 0; // D2 обычно не поддерживается
    
    // Physical capabilities
    (*capabilities).lock_supported = 0;
    (*capabilities).eject_supported = 0;
    (*capabilities).removable = 0; // PCI устройства не removable
    (*capabilities).dock_device = 0;
    
    // ID и UI флаги
    (*capabilities).unique_id = 1; // PCI location уникален
    (*capabilities).silent_install = 1; // Не требует UI при установке
    (*capabilities).no_display_in_ui = 0;
    
    // Device capabilities
    (*capabilities).raw_device_ok = 1; // Можем работать без function driver
    (*capabilities).surprise_removal_ok = 0; // PCI не поддерживает hot removal
    (*capabilities).hardware_disabled = 0;
    
    // Wake capabilities - зависит от конкретного устройства
    (*capabilities).wake_from_d0 = 0;
    (*capabilities).wake_from_d1 = 0;
    (*capabilities).wake_from_d2 = 0;
    (*capabilities).wake_from_d3 = 0;
    
    // Reserved/future
    (*capabilities).non_dynamic = 0;
    (*capabilities).warm_eject_supported = 0;
    (*capabilities).reserved1 = 0;
    (*capabilities).wake_from_interrupt = 0;
    (*capabilities).secure_device = 0;
    (*capabilities).child_of_vga_enabled_bridge = 0;
    (*capabilities).decode_io_on_boot = 0;
    (*capabilities).reserved = 0;
    
    // Address и UINumber
    // Address = (device << 16) | function для PCI
    (*capabilities).address = (((*pdo_ext).device_number as u32) << 16) 
        | ((*pdo_ext).function_number as u32);
    (*capabilities).ui_number = 0xFFFFFFFF; // Unknown
    
    // Device state mapping - D0 во всех system states
    // PowerSystemUnspecified = 0, Working = 1, Sleeping1 = 2, ..., Shutdown = 6
    for i in 0..7 {
        (*capabilities).device_state[i] = 1; // PowerDeviceD0
    }
    
    // System wake и device wake
    (*capabilities).system_wake = 0; // PowerSystemUnspecified
    (*capabilities).device_wake = 0; // PowerDeviceUnspecified
    
    // Latencies
    (*capabilities).d1_latency = 0;
    (*capabilities).d2_latency = 0;
    (*capabilities).d3_latency = 0;
    
    (*irp).io_status.status = STATUS_SUCCESS;
    (*irp).io_status.information = 0;
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    STATUS_SUCCESS
}

// =============================================================================
// Resource Requirements и Start Device
// =============================================================================

/// IRP_MN_QUERY_RESOURCE_REQUIREMENTS для PDO
///
/// Возвращает требования к ресурсам на основе BAR'ов и interrupt.
unsafe fn pci_pdo_query_resource_requirements(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
    pdo_ext: *mut PCI_PDO_EXTENSION,
) -> NTSTATUS {
    // Подсчитываем количество дескрипторов: BAR'ы + interrupt (если есть)
    let mut num_descriptors = 0usize;
    
    for i in 0..6 {
        if (*pdo_ext).bars[i].size > 0 {
            num_descriptors += 1;
        }
    }
    
    // Добавляем interrupt если есть
    if (*pdo_ext).interrupt_pin != 0 {
        num_descriptors += 1;
    }
    
    if num_descriptors == 0 {
        // Нет ресурсов
        (*irp).io_status.status = STATUS_SUCCESS;
        (*irp).io_status.information = 0;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_SUCCESS;
    }
    
    // Вычисляем размер структуры
    // IO_RESOURCE_REQUIREMENTS_LIST включает один IO_RESOURCE_LIST с одним дескриптором
    // Нужно добавить (num_descriptors - 1) * size_of::<IO_RESOURCE_DESCRIPTOR>()
    let base_size = core::mem::size_of::<IO_RESOURCE_REQUIREMENTS_LIST>();
    let extra_desc_size = (num_descriptors.saturating_sub(1)) 
        * core::mem::size_of::<IO_RESOURCE_DESCRIPTOR>();
    let total_size = base_size + extra_desc_size;
    
    let req_list = ExAllocatePoolWithTag(NON_PAGED_POOL, total_size, PCI_POOL_TAG)
        as *mut IO_RESOURCE_REQUIREMENTS_LIST;
    
    if req_list.is_null() {
        (*irp).io_status.status = STATUS_INSUFFICIENT_RESOURCES;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_INSUFFICIENT_RESOURCES;
    }
    
    // Обнуляем
    ptr::write_bytes(req_list as *mut u8, 0, total_size);
    
    // Заполняем header
    (*req_list).list_size = total_size as ULONG;
    (*req_list).interface_type = INTERFACE_TYPE_PCI;
    (*req_list).bus_number = (*pdo_ext).bus_number as ULONG;
    (*req_list).slot_number = (((*pdo_ext).device_number as ULONG) << 3) 
        | ((*pdo_ext).function_number as ULONG);
    (*req_list).alternative_lists = 1;
    
    // Заполняем IO_RESOURCE_LIST
    (*req_list).list[0].version = 1;
    (*req_list).list[0].revision = 1;
    (*req_list).list[0].count = num_descriptors as ULONG;
    
    // Указатель на массив дескрипторов
    let descriptors = &raw mut (*req_list).list[0].descriptors as *mut IO_RESOURCE_DESCRIPTOR;
    let mut desc_idx = 0usize;
    
    // Добавляем BAR'ы
    for i in 0..6 {
        let bar = &(*pdo_ext).bars[i];
        if bar.size == 0 {
            continue;
        }
        
        let desc = descriptors.add(desc_idx);
        (*desc).option = IO_RESOURCE_PREFERRED;
        (*desc).share_disposition = 0; // DeviceExclusive
        
        if bar.is_memory != 0 {
            // Memory BAR
            (*desc).r#type = CM_RESOURCE_TYPE_MEMORY;
            (*desc).flags = CM_RESOURCE_MEMORY_READ_WRITE | CM_RESOURCE_MEMORY_BAR;
            if bar.is_prefetchable != 0 {
                (*desc).flags |= CM_RESOURCE_MEMORY_PREFETCHABLE;
            }
            
            (*desc).u.memory = IO_MEMORY_REQUIREMENT {
                minimum_address: bar.base_address,
                maximum_address: if bar.base_address != 0 {
                    bar.base_address.saturating_add(bar.size - 1)
                } else if bar.is_64bit != 0 {
                    u64::MAX
                } else {
                    0xFFFFFFFF
                },
                alignment: bar.size as ULONG,
                length: bar.size as ULONG,
            };
        } else {
            // I/O BAR
            (*desc).r#type = CM_RESOURCE_TYPE_PORT;
            (*desc).flags = CM_RESOURCE_PORT_IO | CM_RESOURCE_PORT_BAR;
            
            (*desc).u.port = IO_PORT_REQUIREMENT {
                minimum_address: bar.base_address,
                maximum_address: if bar.base_address != 0 {
                    bar.base_address.saturating_add(bar.size - 1)
                } else {
                    0xFFFF
                },
                alignment: bar.size as ULONG,
                length: bar.size as ULONG,
            };
        }
        
        desc_idx += 1;
    }
    
    // Добавляем interrupt
    if (*pdo_ext).interrupt_pin != 0 {
        let desc = descriptors.add(desc_idx);
        (*desc).option = IO_RESOURCE_PREFERRED;
        (*desc).r#type = CM_RESOURCE_TYPE_INTERRUPT;
        (*desc).share_disposition = 1; // Shared
        (*desc).flags = CM_RESOURCE_INTERRUPT_LEVEL_SENSITIVE;
        
        // Для PCI interrupt, используем interrupt line как вектор
        let irq = (*pdo_ext).interrupt_line as ULONG;
        (*desc).u.interrupt = IO_INTERRUPT_REQUIREMENT {
            minimum_vector: irq,
            maximum_vector: irq,
            affinity: !0usize, // All processors
        };
    }
    
    (*irp).io_status.status = STATUS_SUCCESS;
    (*irp).io_status.information = req_list as ULONG_PTR;
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    STATUS_SUCCESS
}

/// IRP_MN_QUERY_RESOURCES для PDO
///
/// Возвращает текущие (boot) ресурсы устройства.
unsafe fn pci_pdo_query_resources(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
    pdo_ext: *mut PCI_PDO_EXTENSION,
) -> NTSTATUS {
    // Подсчитываем количество ресурсов
    let mut num_resources = 0usize;
    
    for i in 0..6 {
        if (*pdo_ext).bars[i].size > 0 && (*pdo_ext).bars[i].base_address != 0 {
            num_resources += 1;
        }
    }
    
    if (*pdo_ext).interrupt_pin != 0 && (*pdo_ext).interrupt_line != 0xFF {
        num_resources += 1;
    }
    
    if num_resources == 0 {
        (*irp).io_status.status = STATUS_SUCCESS;
        (*irp).io_status.information = 0;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_SUCCESS;
    }
    
    // Вычисляем размер
    let base_size = core::mem::size_of::<CM_RESOURCE_LIST>();
    let extra_size = (num_resources.saturating_sub(1))
        * core::mem::size_of::<CM_PARTIAL_RESOURCE_DESCRIPTOR>();
    let total_size = base_size + extra_size;
    
    let res_list = ExAllocatePoolWithTag(NON_PAGED_POOL, total_size, PCI_POOL_TAG)
        as *mut CM_RESOURCE_LIST;
    
    if res_list.is_null() {
        (*irp).io_status.status = STATUS_INSUFFICIENT_RESOURCES;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_INSUFFICIENT_RESOURCES;
    }
    
    ptr::write_bytes(res_list as *mut u8, 0, total_size);
    
    (*res_list).count = 1;
    (*res_list).list[0].interface_type = INTERFACE_TYPE_PCI;
    (*res_list).list[0].bus_number = (*pdo_ext).bus_number as ULONG;
    (*res_list).list[0].partial_resource_list.version = 1;
    (*res_list).list[0].partial_resource_list.revision = 1;
    (*res_list).list[0].partial_resource_list.count = num_resources as ULONG;
    
    let descriptors = &raw mut (*res_list).list[0].partial_resource_list.partial_descriptors
        as *mut CM_PARTIAL_RESOURCE_DESCRIPTOR;
    let mut desc_idx = 0usize;
    
    // Добавляем BAR'ы
    for i in 0..6 {
        let bar = &(*pdo_ext).bars[i];
        if bar.size == 0 || bar.base_address == 0 {
            continue;
        }
        
        let desc = descriptors.add(desc_idx);
        (*desc).share_disposition = 0;
        
        if bar.is_memory != 0 {
            (*desc).r#type = CM_RESOURCE_TYPE_MEMORY;
            (*desc).flags = CM_RESOURCE_MEMORY_READ_WRITE | CM_RESOURCE_MEMORY_BAR;
            if bar.is_prefetchable != 0 {
                (*desc).flags |= CM_RESOURCE_MEMORY_PREFETCHABLE;
            }
            (*desc).u.memory = CM_MEMORY_RESOURCE {
                start: bar.base_address,
                length: bar.size as ULONG,
            };
        } else {
            (*desc).r#type = CM_RESOURCE_TYPE_PORT;
            (*desc).flags = CM_RESOURCE_PORT_IO | CM_RESOURCE_PORT_BAR;
            (*desc).u.port = CM_PORT_RESOURCE {
                start: bar.base_address,
                length: bar.size as ULONG,
            };
        }
        
        desc_idx += 1;
    }
    
    // Добавляем interrupt
    if (*pdo_ext).interrupt_pin != 0 && (*pdo_ext).interrupt_line != 0xFF {
        let desc = descriptors.add(desc_idx);
        (*desc).r#type = CM_RESOURCE_TYPE_INTERRUPT;
        (*desc).share_disposition = 1;
        (*desc).flags = CM_RESOURCE_INTERRUPT_LEVEL_SENSITIVE;
        (*desc).u.interrupt = CM_INTERRUPT_RESOURCE {
            level: (*pdo_ext).interrupt_line as ULONG,
            vector: (*pdo_ext).interrupt_line as ULONG,
            affinity: !0usize,
        };
    }
    
    (*irp).io_status.status = STATUS_SUCCESS;
    (*irp).io_status.information = res_list as ULONG_PTR;
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    STATUS_SUCCESS
}

/// IRP_MN_START_DEVICE для PDO
///
/// Создает CM_RESOURCE_LIST из BAR и включает устройство.
unsafe fn pci_pdo_start_device(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
    pdo_ext: *mut PCI_PDO_EXTENSION,
) -> NTSTATUS {
    // В правильной NT реализации ресурсы приходят через
    // stack->parameters.start_device.allocated_resources от PnP Manager
    // 
    // В boot-stage мы упрощаем: ресурсы уже известны из PCI config space
    // и записаны в pdo_ext->bars[]
    //
    // NOTE: Раньше здесь был boot-stage hack где мы создавали CM_RESOURCE_LIST
    // и сохраняли в irp->io_status.information, но это не-каноничный контракт
    // и приводил к утечкам памяти (кто должен был освобождать cm_list?).
    //
    // Вместо этого полагаемся на то, что:
    // 1. IRP_MN_QUERY_RESOURCES уже вернул ресурсы
    // 2. IRP_MN_QUERY_RESOURCE_REQUIREMENTS вернул requirements
    // 3. PnP Manager (в упрощённой boot версии) должен передать ресурсы
    //    через parameters.start_device, если нужно
    
    // Включаем устройство
    pci_enable_device(
        (*pdo_ext).bus_number,
        (*pdo_ext).device_number,
        (*pdo_ext).function_number,
    );
    
    (*pdo_ext).started = 1;
    
    // Помечаем в IRP старые allocated_resources если они есть
    let stack = IoGetCurrentIrpStackLocation(irp);
    let allocated_resources = (*stack).parameters.start_device.allocated_resources_translated as *const CM_RESOURCE_LIST;
    
    // Если есть allocated resources от PnP Manager, используем их для программирования BAR
    if !allocated_resources.is_null() && (*allocated_resources).count > 0 {
        let partial_list = &(*allocated_resources).list[0].partial_resource_list;
        let descriptors = &partial_list.partial_descriptors as *const CM_PARTIAL_RESOURCE_DESCRIPTOR;
        
        let mut bar_idx = 0usize;
        
        for i in 0..partial_list.count as usize {
            let desc = descriptors.add(i);
            
            match (*desc).r#type {
                CM_RESOURCE_TYPE_MEMORY | CM_RESOURCE_TYPE_PORT => {
                    // Находим соответствующий BAR
                    while bar_idx < 6 {
                        let bar = &mut (*pdo_ext).bars[bar_idx];
                        if bar.size > 0 {
                            // Программируем BAR с назначенным адресом
                            let new_address = if (*desc).r#type == CM_RESOURCE_TYPE_MEMORY {
                                (*desc).u.memory.start
                            } else {
                                (*desc).u.port.start
                            };
                            
                            pci_program_bar(
                                (*pdo_ext).bus_number,
                                (*pdo_ext).device_number,
                                (*pdo_ext).function_number,
                                bar.index,
                                new_address,
                                bar.is_64bit != 0,
                            );
                            
                            bar.base_address = new_address;
                            bar.assigned = 1;
                            
                            // Пропускаем второй слот для 64-bit BAR
                            if bar.is_64bit != 0 {
                                bar_idx += 2;
                            } else {
                                bar_idx += 1;
                            }
                            break;
                        }
                        bar_idx += 1;
                    }
                }
                _ => {}
            }
        }
    }
    
    // Включаем I/O, Memory и Bus Master
    pci_enable_device(
        (*pdo_ext).bus_number,
        (*pdo_ext).device_number,
        (*pdo_ext).function_number,
    );
    
    (*pdo_ext).started = 1;
    
    (*irp).io_status.status = STATUS_SUCCESS;
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    STATUS_SUCCESS
}

