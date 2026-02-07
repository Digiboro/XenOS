//! PnP обработчики для StorPort
//!
//! Фаза 2.5: Корректная обработка PnP IRP с completion routines

use crate::types::*;
use crate::imports::ntoskrnl::*;
use crate::miniport::*;
use crate::{storport_print, storport_print_hex, STORPORT_POOL_TAG};
use core::ptr;

// =============================================================================
// IRP Completion Helpers
// =============================================================================

/// Completion routine для синхронной обработки IRP вниз по стеку
/// 
/// Устанавливает событие и возвращает STATUS_MORE_PROCESSING_REQUIRED
/// чтобы IRP не завершился до возврата из IoCallDriver.
unsafe extern "win64" fn storport_pnp_completion_routine(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
    context: PVOID,
) -> NTSTATUS {
    // Устанавливаем pending_returned если IRP был pending
    if (*irp).pending_returned != 0 {
        // Передаём флаг обратно caller'у
    }
    
    // Context - это указатель на KEVENT
    let event = context as *mut KEVENT;
    if !event.is_null() {
        KeSetEvent(event, 0, 0);
    }
    
    STATUS_MORE_PROCESSING_REQUIRED
}

/// Установить completion routine в IRP
/// 
/// Реализует IoSetCompletionRoutine как inline функцию (в WDK это макрос)
unsafe fn set_completion_routine(
    irp: PIRP,
    completion_routine: PIO_COMPLETION_ROUTINE,
    context: PVOID,
    invoke_on_success: bool,
    invoke_on_error: bool,
    invoke_on_cancel: bool,
) {
    let next_stack = IoGetNextIrpStackLocation(irp);
    (*next_stack).completion_routine = completion_routine as PVOID;
    (*next_stack).context = context;
    
    // Устанавливаем флаги вызова
    let mut control: UCHAR = 0;
    if invoke_on_success {
        control |= SL_INVOKE_ON_SUCCESS;
    }
    if invoke_on_error {
        control |= SL_INVOKE_ON_ERROR;
    }
    if invoke_on_cancel {
        control |= SL_INVOKE_ON_CANCEL;
    }
    (*next_stack).control = control;
}

/// Передать IRP вниз по стеку и дождаться завершения
/// 
/// Использует completion routine + KEVENT для синхронного ожидания.
/// Возвращает статус из IRP после завершения.
unsafe fn forward_irp_synchronous(
    device_extension: *mut STORPORT_DEVICE_EXTENSION,
    irp: PIRP,
) -> NTSTATUS {
    // Инициализируем событие для ожидания
    let mut event = KEVENT::zeroed();
    KeInitializeEvent(&mut event, NOTIFICATION_EVENT, 0);
    
    // Копируем текущий stack location в следующий
    IoCopyCurrentIrpStackLocationToNext(irp);
    
    // Устанавливаем completion routine
    set_completion_routine(
        irp,
        storport_pnp_completion_routine,
        &mut event as *mut KEVENT as PVOID,
        true,  // invoke on success
        true,  // invoke on error
        true,  // invoke on cancel
    );
    
    // Передаём IRP вниз
    let status = IoCallDriver((*device_extension).lower_device, irp);
    
    // Если статус PENDING - ждём завершения
    if status == STATUS_PENDING {
        #[cfg(feature = "storage-trace")]
        storport_print("[STORPORT] IRP pending, waiting...\n");
        
        KeWaitForSingleObject(
            &mut event as *mut KEVENT as PVOID,
            EXECUTIVE,
            KERNEL_MODE,
            0,           // not alertable
            ptr::null(), // no timeout (infinite wait)
        );
    }
    
    // Возвращаем финальный статус из IRP
    (*irp).io_status.status
}

/// Главный PnP dispatch handler
pub unsafe fn storport_pnp_dispatch(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    unsafe {
        let stack = IoGetCurrentIrpStackLocation(irp);
        let minor = (*stack).minor_function;
        
        storport_print("[STORPORT] PnP IRP: minor=0x");
        storport_print_hex(minor as u64);
        storport_print("\n");
        
        match minor {
            IRP_MN_START_DEVICE => handle_start_device(device_object, irp),
            IRP_MN_QUERY_DEVICE_RELATIONS => handle_query_device_relations(device_object, irp),
            IRP_MN_QUERY_ID => handle_query_id(device_object, irp),
            IRP_MN_REMOVE_DEVICE => handle_remove_device(device_object, irp),
            IRP_MN_STOP_DEVICE => handle_stop_device(device_object, irp),
            IRP_MN_SURPRISE_REMOVAL => handle_surprise_removal(device_object, irp),
            IRP_MN_QUERY_STOP_DEVICE => handle_query_stop_device(device_object, irp),
            IRP_MN_CANCEL_STOP_DEVICE => handle_cancel_stop_device(device_object, irp),
            IRP_MN_QUERY_REMOVE_DEVICE => handle_query_remove_device(device_object, irp),
            IRP_MN_CANCEL_REMOVE_DEVICE => handle_cancel_remove_device(device_object, irp),
            _ => {
                // Pass down по умолчанию
                let ext = (*device_object).device_extension as *const STORPORT_DEVICE_EXTENSION;
                if !ext.is_null() && (*ext).signature == STORPORT_DEVICE_EXTENSION_SIGNATURE {
                    IoSkipCurrentIrpStackLocation(irp);
                    IoCallDriver((*ext).lower_device, irp)
                } else {
                    // Это PDO - завершаем
                    let status = (*irp).io_status.status;
                    IoCompleteRequest(irp, IO_NO_INCREMENT);
                    status
                }
            }
        }
    }
}

/// IRP_MN_START_DEVICE
/// 
/// Корректная обработка START_DEVICE с completion routine для STATUS_PENDING.
unsafe fn handle_start_device(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    unsafe {
        storport_print("[STORPORT] START_DEVICE\n");
        
        let ext = (*device_object).device_extension as *mut STORPORT_DEVICE_EXTENSION;
        
        // Проверяем является ли это FDO или PDO
        if !ext.is_null() && (*ext).signature == STORPORT_DEVICE_EXTENSION_SIGNATURE {
            // Это FDO - выполняем полную инициализацию адаптера
            
            // ВАЖНО: Читаем ресурсы ДО передачи вниз
            let stack = IoGetCurrentIrpStackLocation(irp);
            let params_ptr = &(*stack).parameters as *const _ as *const u8;
            
            // IO_STACK_START_DEVICE layout:
            // offset 0: allocated_resources (PVOID)
            // offset 8: allocated_resources_translated (PVOID)
            let allocated_resources = core::ptr::read(params_ptr as *const PVOID);
            let allocated_resources_translated = core::ptr::read(params_ptr.add(8) as *const PVOID);
            
            storport_print("[STORPORT] allocated_resources=0x");
            storport_print_hex(allocated_resources as u64);
            storport_print(" translated=0x");
            storport_print_hex(allocated_resources_translated as u64);
            storport_print("\n");
            
            // Передаём IRP вниз по стеку синхронно (с ожиданием если STATUS_PENDING)
            // Phase 2.5: Корректная обработка STATUS_PENDING через completion routine
            let status = forward_irp_synchronous(ext, irp);
        
        if status >= 0 {
            storport_print("[STORPORT] Calling miniport HwFindAdapter...\n");
            
            // Парсим CM_RESOURCE_LIST для получения BAR информации
            // Используем translated resources для правильного маппинга
            let mut access_ranges_array: [ACCESS_RANGE; 8] = [ACCESS_RANGE {
                RangeStart: 0,
                RangeLength: 0,
                RangeInMemory: 0,
            }; 8];
            let mut num_ranges = 0;
            
            // Предпочитаем translated resources, но fallback на raw если нет
            let resources_to_parse = if !allocated_resources_translated.is_null() {
                allocated_resources_translated
            } else {
                allocated_resources
            };
            
            if !resources_to_parse.is_null() {
                num_ranges = parse_cm_resources(resources_to_parse, &mut access_ranges_array);
            }
            
            storport_print("[STORPORT] Found ");
            storport_print_hex(num_ranges as u64);
            storport_print(" access ranges\n");
            
            // Parse interrupt resources
            let interrupt_info = parse_interrupt_resource(resources_to_parse);
            if interrupt_info.found {
                storport_print("[STORPORT] Interrupt: level=");
                storport_print_hex(interrupt_info.level as u64);
                storport_print(" vector=");
                storport_print_hex(interrupt_info.vector as u64);
                storport_print("\n");
            }
            
            // Подготавливаем PORT_CONFIGURATION_INFORMATION
            let mut config_info = PORT_CONFIGURATION_INFORMATION {
                Length: core::mem::size_of::<PORT_CONFIGURATION_INFORMATION>() as ULONG,
                SystemIoBusNumber: 0,
                AdapterInterfaceType: (*ext).hw_init_data.AdapterInterfaceType,
                BusInterruptLevel: if interrupt_info.found { interrupt_info.level } else { 0 },
                BusInterruptVector: if interrupt_info.found { interrupt_info.vector } else { 0 },
                InterruptMode: 1, // Level triggered for PCI
                MaximumTransferLength: 0x10000,
                NumberOfPhysicalBreaks: 16,
                DmaChannel: 0,
                DmaPort: 0,
                DmaWidth: 0,
                DmaSpeed: 0,
                AlignmentMask: 0,
                NumberOfAccessRanges: num_ranges as ULONG,
                AccessRanges: if num_ranges > 0 { access_ranges_array.as_mut_ptr() } else { ptr::null_mut() },
                Reserved: ptr::null_mut(),
                NumberOfBuses: 0,
                InitiatorBusId: [0; 8],
                ScatterGather: 0,
                Master: 0,
                CachesData: 0,
                AdapterScansDown: 0,
                AtdiskPrimaryClaimed: 0,
                AtdiskSecondaryClaimed: 0,
                Dma32BitAddresses: 0,
                DemandMode: 0,
                MapBuffers: (*ext).hw_init_data.MapBuffers,
                NeedPhysicalAddresses: 0,
                TaggedQueuing: 0,
                AutoRequestSense: 0,
                MultipleRequestPerLu: 0,
                ReceiveEvent: 0,
                RealModeInitialized: 0,
                BufferAccessScsiPortControlled: 0,
                MaximumNumberOfTargets: 0,
                ReservedUchars: [0; 2],
                SlotNumber: 0,
                BusInterruptLevel2: 0,
                BusInterruptVector2: 0,
                InterruptMode2: 0,
                DmaChannel2: 0,
                DmaPort2: 0,
                DmaWidth2: 0,
                DmaSpeed2: 0,
                DeviceExtensionSize: 0,
                SpecificLuExtensionSize: 0,
                SrbExtensionSize: 0,
                Dma64BitAddresses: 0,
                ResetTargetSupported: 0,
                MaximumNumberOfLogicalUnits: 0,
                WmiDataProvider: 0,
            };
            
            // Вызываем HwFindAdapter
            let mut again = 0u8;
            if let Some(hw_find_adapter) = (*ext).hw_init_data.HwFindAdapter {
                let result = hw_find_adapter(
                    (*ext).miniport_device_extension,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    &mut config_info,
                    &mut again,
                );
                
                if result != 0 {
                    storport_print("[STORPORT] ERROR: HwFindAdapter failed\n");
                    return STATUS_UNSUCCESSFUL;
                }
            }
            
            // Сохраняем DMA/Scatter-Gather параметры из PORT_CONFIGURATION_INFORMATION (Phase 2.4)
            (*ext).max_transfer_length = config_info.MaximumTransferLength;
            (*ext).number_of_physical_breaks = config_info.NumberOfPhysicalBreaks;
            (*ext).alignment_mask = config_info.AlignmentMask;
            (*ext).scatter_gather_supported = config_info.ScatterGather;
            (*ext).dma64_supported = if config_info.Dma64BitAddresses != 0 { 1 } else { 0 };
            
            #[cfg(feature = "storage-trace")]
            {
                storport_print("[STORPORT] DMA config: MaxTransfer=");
                storport_print_hex(config_info.MaximumTransferLength as u64);
                storport_print(", PhysBreaks=");
                storport_print_hex(config_info.NumberOfPhysicalBreaks as u64);
                storport_print(", Alignment=");
                storport_print_hex(config_info.AlignmentMask as u64);
                storport_print(", SG=");
                storport_print_hex(config_info.ScatterGather as u64);
                storport_print(", DMA64=");
                storport_print_hex(config_info.Dma64BitAddresses as u64);
                storport_print("\n");
            }
            
            storport_print("[STORPORT] Calling miniport HwInitialize...\n");
            
            // Вызываем HwInitialize
            if let Some(hw_initialize) = (*ext).hw_init_data.HwInitialize {
                let result = hw_initialize((*ext).miniport_device_extension);
                
                if result == 0 {
                    storport_print("[STORPORT] ERROR: HwInitialize failed\n");
                    return STATUS_UNSUCCESSFUL;
                }
            }
            
            // Connect interrupt after HwInitialize if we have interrupt info
            if interrupt_info.found {
                storport_print("[STORPORT] Connecting interrupt vector=");
                storport_print_hex(interrupt_info.vector as u64);
                storport_print("...\n");
                
                // Store HwInterrupt callback
                (*ext).hw_interrupt = (*ext).hw_init_data.HwInterrupt;
                
                // Store interrupt params for use by ISR
                (*ext).interrupt_vector = interrupt_info.vector;
                (*ext).interrupt_irql = interrupt_info.level as UCHAR;
                
                // Connect interrupt using IoConnectInterrupt
                let status = crate::imports::ntoskrnl::IoConnectInterrupt(
                    &mut (*ext).interrupt_object,
                    storport_isr_entry,
                    ext as PVOID,
                    ptr::null_mut(), // spin lock
                    interrupt_info.vector,
                    interrupt_info.level as UCHAR,
                    interrupt_info.level as UCHAR, // synchronize IRQL
                    1, // Level triggered
                    1, // Share vector
                    0, // Processor 0
                    0, // Don't save FP state
                );
                
                if status >= 0 {
                    (*ext).interrupt_enabled = 1;
                    storport_print("[STORPORT] Interrupt connected successfully\n");
                } else {
                    storport_print("[STORPORT] WARNING: Failed to connect interrupt, status=0x");
                    storport_print_hex(status as u64);
                    storport_print("\n");
                }
            }
            
            // Обнаруживаем устройства через SCSI INQUIRY
            storport_print("[STORPORT] Scanning for devices using SCSI INQUIRY...\n");
            
            let num_buses = config_info.NumberOfBuses.max(1) as usize;
            let max_targets = config_info.InitiatorBusId[0].max(1) as usize;
            
            storport_print("[STORPORT]   num_buses=");
            storport_print_hex(num_buses as u64);
            storport_print(", max_targets=");
            storport_print_hex(max_targets as u64);
            storport_print("\n");
            
            // Scan each bus and target
            for path_id in 0..num_buses.min(8) {
                for target_id in 0..max_targets.min(32) {
                    // Send INQUIRY to probe device presence
                    let inquiry_result = crate::scsi::probe_target_with_inquiry(
                        ext,
                        path_id as UCHAR,
                        target_id as UCHAR,
                        0,  // LUN 0
                    );
                    
                    if inquiry_result.present {
                        // Device found - create PDO based on device type
                        storport_print("[STORPORT] Device found at path=");
                        storport_print_hex(path_id as u64);
                        storport_print(" target=");
                        storport_print_hex(target_id as u64);
                        storport_print(" type=0x");
                        storport_print_hex(inquiry_result.device_type as u64);
                        storport_print("\n");
                        
                        // Create PDO only for supported device types
                        let pdo = match inquiry_result.device_type {
                            DIRECT_ACCESS_DEVICE => {
                                // Disk device
                                create_scsi_disk_pdo(
                                    (*ext).driver_object,
                                    device_object,
                                    path_id as u8,
                                    target_id as u8,
                                    0,
                                )
                            }
                            READ_ONLY_DIRECT_ACCESS_DEVICE => {
                                // CD-ROM (treat as disk for now)
                                storport_print("[STORPORT]   CD-ROM device, creating as disk PDO\n");
                                create_scsi_disk_pdo(
                                    (*ext).driver_object,
                                    device_object,
                                    path_id as u8,
                                    target_id as u8,
                                    0,
                                )
                            }
                            _ => {
                                // Unsupported device type
                                storport_print("[STORPORT]   Unsupported device type 0x");
                                storport_print_hex(inquiry_result.device_type as u64);
                                storport_print(", skipping\n");
                                core::ptr::null_mut()
                            }
                        };
                        
                        if !pdo.is_null() {
                            (*ext).child_pdos[(*ext).child_count as usize] = pdo;
                            (*ext).child_count += 1;
                            
                            storport_print("[STORPORT]   Created PDO for path=");
                            storport_print_hex(path_id as u64);
                            storport_print(" target=");
                            storport_print_hex(target_id as u64);
                            storport_print("\n");
                        }
                    }
                }
            }
            
            (*ext).started = 1;
            
            // PHASE 2.3: Инициализируем DPC и Timer для async completion
            crate::miniport::storport_init_dpc_timer(ext);
            
            storport_print("[STORPORT] Adapter started with ");
            storport_print_hex((*ext).child_count as u64);
            storport_print(" devices\n");
            }
            
            // Phase 2.5: Завершаем IRP после синхронного forward
            // (completion routine вернула STATUS_MORE_PROCESSING_REQUIRED)
            (*irp).io_status.status = status;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            status
        } else {
            // Это PDO (SCSI\Disk) - просто завершаем успешно
            storport_print("[STORPORT] PDO START_DEVICE - completing\n");
            (*irp).io_status.status = STATUS_SUCCESS;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            STATUS_SUCCESS
        }
    }
}

/// IRP_MN_QUERY_DEVICE_RELATIONS
unsafe fn handle_query_device_relations(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    unsafe {
        let stack = IoGetCurrentIrpStackLocation(irp);
        
        // Получаем relation type из parameters
        let params_ptr = &(*stack).parameters as *const _ as *const u8;
        let relation_type = core::ptr::read(params_ptr as *const ULONG);
        
        storport_print("[STORPORT] QUERY_DEVICE_RELATIONS: type=");
        storport_print_hex(relation_type as u64);
        storport_print("\n");
        
        let ext = (*device_object).device_extension as *mut STORPORT_DEVICE_EXTENSION;
        
        // Проверяем является ли это FDO или PDO
        if ext.is_null() || (*ext).signature != STORPORT_DEVICE_EXTENSION_SIGNATURE {
            // Это PDO - обрабатываем TargetDeviceRelation
            if relation_type == TARGET_DEVICE_RELATION {
                let relations = ExAllocatePoolWithTag(
                    NON_PAGED_POOL,
                    core::mem::size_of::<DEVICE_RELATIONS>(),
                    STORPORT_POOL_TAG,
                ) as PDEVICE_RELATIONS;
                
                if !relations.is_null() {
                    (*relations).count = 1;
                    *(*relations).objects.as_mut_ptr() = device_object;
                    ObReferenceObject(device_object as PVOID);
                    
                    (*irp).io_status.status = STATUS_SUCCESS;
                    (*irp).io_status.information = relations as ULONG_PTR;
                    IoCompleteRequest(irp, IO_NO_INCREMENT);
                    return STATUS_SUCCESS;
                }
            }
            
            let status = (*irp).io_status.status;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            return status;
        }
        
        // Это FDO - обрабатываем BusRelations
        if relation_type == BUS_RELATIONS {
            let child_count = (*ext).child_count as usize;
            
            if child_count > 0 {
                storport_print("[STORPORT] Returning ");
                storport_print_hex(child_count as u64);
                storport_print(" child devices\n");
                
                let relations_size = core::mem::size_of::<DEVICE_RELATIONS>()
                    + (child_count - 1) * core::mem::size_of::<PDEVICE_OBJECT>();
                
                let relations = ExAllocatePoolWithTag(
                    NON_PAGED_POOL,
                    relations_size,
                    STORPORT_POOL_TAG,
                ) as PDEVICE_RELATIONS;
                
                if relations.is_null() {
                    (*irp).io_status.status = STATUS_INSUFFICIENT_RESOURCES;
                    IoCompleteRequest(irp, IO_NO_INCREMENT);
                    return STATUS_INSUFFICIENT_RESOURCES;
                }
                
                (*relations).count = child_count as ULONG;
                for i in 0..child_count {
                    let pdo = (*ext).child_pdos[i];
                    *(*relations).objects.as_mut_ptr().add(i) = pdo;
                    ObReferenceObject(pdo as PVOID);
                }
                
                (*irp).io_status.status = STATUS_SUCCESS;
                (*irp).io_status.information = relations as ULONG_PTR;
                IoCompleteRequest(irp, IO_NO_INCREMENT);
                return STATUS_SUCCESS;
            }
        }
        
        // Pass down для других types
        IoSkipCurrentIrpStackLocation(irp);
        IoCallDriver((*ext).lower_device, irp)
    }
}

/// IRP_MN_QUERY_ID
unsafe fn handle_query_id(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    unsafe {
        storport_print("[STORPORT] QUERY_ID\n");
        
        // Это обрабатывается только для PDO
        // FDO должен пробрасывать вниз
        
        let ext = (*device_object).device_extension as *const STORPORT_DEVICE_EXTENSION;
        if !ext.is_null() && (*ext).signature == STORPORT_DEVICE_EXTENSION_SIGNATURE {
            // FDO - pass down
            IoSkipCurrentIrpStackLocation(irp);
            return IoCallDriver((*ext).lower_device, irp);
        }
        
        // PDO - возвращаем ID
        // Для StorPort PDO это будет "SCSI\Disk..."
        let stack = IoGetCurrentIrpStackLocation(irp);
        let params_ptr = &(*stack).parameters as *const _ as *const u8;
        let id_type = core::ptr::read(params_ptr as *const ULONG);
        
        const BUS_QUERY_DEVICE_ID: ULONG = 0;
        const BUS_QUERY_HARDWARE_IDS: ULONG = 1;
        const BUS_QUERY_INSTANCE_ID: ULONG = 3;
        
        match id_type {
            BUS_QUERY_DEVICE_ID => {
                let device_id = b"SCSI\\Disk\0";
                let buffer = ExAllocatePoolWithTag(
                    NON_PAGED_POOL,
                    device_id.len() * 2,
                    STORPORT_POOL_TAG,
                ) as *mut u16;
                
                if buffer.is_null() {
                    (*irp).io_status.status = STATUS_INSUFFICIENT_RESOURCES;
                    IoCompleteRequest(irp, IO_NO_INCREMENT);
                    return STATUS_INSUFFICIENT_RESOURCES;
                }
                
                for (i, &b) in device_id.iter().enumerate() {
                    *buffer.add(i) = b as u16;
                }
                
                (*irp).io_status.status = STATUS_SUCCESS;
                (*irp).io_status.information = buffer as ULONG_PTR;
                IoCompleteRequest(irp, IO_NO_INCREMENT);
                STATUS_SUCCESS
            }
            BUS_QUERY_HARDWARE_IDS => {
                let hw_id = b"SCSI\\Disk\0\0";
                let buffer = ExAllocatePoolWithTag(
                    NON_PAGED_POOL,
                    hw_id.len() * 2,
                    STORPORT_POOL_TAG,
                ) as *mut u16;
                
                if buffer.is_null() {
                    (*irp).io_status.status = STATUS_INSUFFICIENT_RESOURCES;
                    IoCompleteRequest(irp, IO_NO_INCREMENT);
                    return STATUS_INSUFFICIENT_RESOURCES;
                }
                
                for (i, &b) in hw_id.iter().enumerate() {
                    *buffer.add(i) = b as u16;
                }
                
                (*irp).io_status.status = STATUS_SUCCESS;
                (*irp).io_status.information = buffer as ULONG_PTR;
                IoCompleteRequest(irp, IO_NO_INCREMENT);
                STATUS_SUCCESS
            }
            _ => {
                (*irp).io_status.status = STATUS_NOT_SUPPORTED;
                IoCompleteRequest(irp, IO_NO_INCREMENT);
                STATUS_NOT_SUPPORTED
            }
        }
    }
}

/// IRP_MN_REMOVE_DEVICE
/// 
/// Phase 2.5: Полная очистка ресурсов при удалении устройства
unsafe fn handle_remove_device(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    unsafe {
        storport_print("[STORPORT] REMOVE_DEVICE\n");
        
        let ext = (*device_object).device_extension as *mut STORPORT_DEVICE_EXTENSION;
        
        if ext.is_null() || (*ext).signature != STORPORT_DEVICE_EXTENSION_SIGNATURE {
            // PDO - просто завершаем
            (*irp).io_status.status = STATUS_SUCCESS;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            return STATUS_SUCCESS;
        }
        
        // FDO - выполняем cleanup
        
        // 1. Отменяем timeout timer если активен
        if (*ext).timeout_timer_initialized != 0 {
            KeCancelTimer(&mut (*ext).timeout_timer);
            storport_print("[STORPORT] Cancelled timeout timer\n");
        }
        
        // 2. Удаляем DPC из очереди если pending
        if (*ext).completion_dpc_initialized != 0 {
            KeRemoveQueueDpc(&mut (*ext).completion_dpc);
            storport_print("[STORPORT] Removed completion DPC\n");
        }
        
        // 3. Отменяем pending запросы
        (*ext).cancel_all_pending();
        
        // 4. Вызываем HwAdapterControl(STOP) если есть
        if let Some(hw_adapter_control) = (*ext).hw_init_data.HwAdapterControl {
            storport_print("[STORPORT] Calling HwAdapterControl(STOP)\n");
            hw_adapter_control(
                (*ext).miniport_device_extension,
                SCSI_ADAPTER_CONTROL_STOP,
                ptr::null_mut(),
            );
        }
        
        // 5. Очищаем started flag
        (*ext).started = 0;
        
        // 6. Передаём IRP вниз
        IoSkipCurrentIrpStackLocation(irp);
        let status = IoCallDriver((*ext).lower_device, irp);
        
        // 7. Отключаем от device stack и удаляем FDO
        IoDetachDevice((*ext).lower_device);
        IoDeleteDevice(device_object);
        
        storport_print("[STORPORT] Device removed\n");
        status
    }
}

/// IRP_MN_STOP_DEVICE
/// 
/// Phase 2.5: Корректная остановка адаптера
unsafe fn handle_stop_device(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    unsafe {
        storport_print("[STORPORT] STOP_DEVICE\n");
        
        let ext = (*device_object).device_extension as *mut STORPORT_DEVICE_EXTENSION;
        
        if !ext.is_null() && (*ext).signature == STORPORT_DEVICE_EXTENSION_SIGNATURE {
            // 1. Устанавливаем флаг остановки (больше не принимаем новые запросы)
            (*ext).started = 0;
            
            // 2. Отменяем timeout timer
            if (*ext).timeout_timer_initialized != 0 {
                KeCancelTimer(&mut (*ext).timeout_timer);
            }
            
            // 3. Дожидаемся завершения in-flight запросов
            // В простой реализации просто отменяем их
            if (*ext).in_flight_count > 0 {
                storport_print("[STORPORT] Cancelling ");
                storport_print_hex((*ext).in_flight_count as u64);
                storport_print(" in-flight requests\n");
                (*ext).cancel_all_pending();
            }
            
            // 4. Вызываем HwAdapterControl(STOP) если есть
            if let Some(hw_adapter_control) = (*ext).hw_init_data.HwAdapterControl {
                storport_print("[STORPORT] Calling HwAdapterControl(STOP)\n");
                hw_adapter_control(
                    (*ext).miniport_device_extension,
                    SCSI_ADAPTER_CONTROL_STOP,
                    ptr::null_mut(),
                );
            }
            
            // 5. Передаём IRP вниз
            IoSkipCurrentIrpStackLocation(irp);
            IoCallDriver((*ext).lower_device, irp)
        } else {
            // PDO - просто завершаем
            (*irp).io_status.status = STATUS_SUCCESS;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            STATUS_SUCCESS
        }
    }
}

/// IRP_MN_SURPRISE_REMOVAL
/// 
/// Phase 2.5: Обработка неожиданного удаления устройства
unsafe fn handle_surprise_removal(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    unsafe {
        storport_print("[STORPORT] SURPRISE_REMOVAL\n");
        
        let ext = (*device_object).device_extension as *mut STORPORT_DEVICE_EXTENSION;
        
        if !ext.is_null() && (*ext).signature == STORPORT_DEVICE_EXTENSION_SIGNATURE {
            // Устройство было неожиданно удалено - немедленно отменяем все запросы
            (*ext).started = 0;
            
            // Отменяем все pending запросы с ошибкой
            (*ext).cancel_all_pending();
            
            // Передаём вниз
            IoSkipCurrentIrpStackLocation(irp);
            IoCallDriver((*ext).lower_device, irp)
        } else {
            (*irp).io_status.status = STATUS_SUCCESS;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            STATUS_SUCCESS
        }
    }
}

/// IRP_MN_QUERY_STOP_DEVICE
/// 
/// Phase 2.5: Запрос на остановку устройства. 
/// Драйвер может отклонить если не может остановиться безопасно.
unsafe fn handle_query_stop_device(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    unsafe {
        #[cfg(feature = "storage-trace")]
        storport_print("[STORPORT] QUERY_STOP_DEVICE\n");
        
        let ext = (*device_object).device_extension as *mut STORPORT_DEVICE_EXTENSION;
        
        if !ext.is_null() && (*ext).signature == STORPORT_DEVICE_EXTENSION_SIGNATURE {
            // Проверяем можем ли безопасно остановиться
            // Для StorPort - если есть in-flight запросы, можем отклонить
            // или согласиться и потом выполнить очистку при STOP_DEVICE
            
            // TODO: В базовой реализации всегда разрешаем (SUCCESS)
            // Можно добавить проверку критических операций
            
            IoSkipCurrentIrpStackLocation(irp);
            IoCallDriver((*ext).lower_device, irp)
        } else {
            (*irp).io_status.status = STATUS_SUCCESS;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            STATUS_SUCCESS
        }
    }
}

/// IRP_MN_CANCEL_STOP_DEVICE
/// 
/// Phase 2.5: Отмена запроса на остановку. Устройство продолжает работу.
unsafe fn handle_cancel_stop_device(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    unsafe {
        #[cfg(feature = "storage-trace")]
        storport_print("[STORPORT] CANCEL_STOP_DEVICE\n");
        
        let ext = (*device_object).device_extension as *mut STORPORT_DEVICE_EXTENSION;
        
        if !ext.is_null() && (*ext).signature == STORPORT_DEVICE_EXTENSION_SIGNATURE {
            // Ничего особенного не делаем - просто передаём вниз
            IoSkipCurrentIrpStackLocation(irp);
            IoCallDriver((*ext).lower_device, irp)
        } else {
            (*irp).io_status.status = STATUS_SUCCESS;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            STATUS_SUCCESS
        }
    }
}

/// IRP_MN_QUERY_REMOVE_DEVICE
/// 
/// Phase 2.5: Запрос на удаление устройства.
/// Драйвер может отклонить если есть открытые handle'ы или критические операции.
unsafe fn handle_query_remove_device(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    unsafe {
        #[cfg(feature = "storage-trace")]
        storport_print("[STORPORT] QUERY_REMOVE_DEVICE\n");
        
        let ext = (*device_object).device_extension as *mut STORPORT_DEVICE_EXTENSION;
        
        if !ext.is_null() && (*ext).signature == STORPORT_DEVICE_EXTENSION_SIGNATURE {
            // Проверяем можем ли безопасно удалиться
            // Для StorPort - если есть активные handle'ы на дочерние устройства,
            // PnP manager должен сам это отслеживать
            
            // TODO: В базовой реализации всегда разрешаем
            IoSkipCurrentIrpStackLocation(irp);
            IoCallDriver((*ext).lower_device, irp)
        } else {
            (*irp).io_status.status = STATUS_SUCCESS;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            STATUS_SUCCESS
        }
    }
}

/// IRP_MN_CANCEL_REMOVE_DEVICE
/// 
/// Phase 2.5: Отмена запроса на удаление. Устройство остаётся в системе.
unsafe fn handle_cancel_remove_device(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    unsafe {
        #[cfg(feature = "storage-trace")]
        storport_print("[STORPORT] CANCEL_REMOVE_DEVICE\n");
        
        let ext = (*device_object).device_extension as *mut STORPORT_DEVICE_EXTENSION;
        
        if !ext.is_null() && (*ext).signature == STORPORT_DEVICE_EXTENSION_SIGNATURE {
            // Ничего особенного - просто передаём вниз
            IoSkipCurrentIrpStackLocation(irp);
            IoCallDriver((*ext).lower_device, irp)
        } else {
            (*irp).io_status.status = STATUS_SUCCESS;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            STATUS_SUCCESS
        }
    }
}

// =============================================================================
// Resource Parsing
// =============================================================================

#[repr(C)]
struct CM_RESOURCE_LIST {
    count: ULONG,
    list: [CM_FULL_RESOURCE_DESCRIPTOR; 1],
}

#[repr(C)]
struct CM_FULL_RESOURCE_DESCRIPTOR {
    interface_type: i32,
    bus_number: ULONG,
    partial_resource_list: CM_PARTIAL_RESOURCE_LIST,
}

#[repr(C)]
struct CM_PARTIAL_RESOURCE_LIST {
    version: USHORT,
    revision: USHORT,
    count: ULONG,
    partial_descriptors: [CM_PARTIAL_RESOURCE_DESCRIPTOR; 1],
}

#[repr(C)]
struct CM_PARTIAL_RESOURCE_DESCRIPTOR {
    r#type: UCHAR,
    share_disposition: UCHAR,
    flags: USHORT,
    u: CM_PARTIAL_RESOURCE_UNION,
}

#[repr(C)]
union CM_PARTIAL_RESOURCE_UNION {
    memory: CM_PARTIAL_RESOURCE_MEMORY,
    port: CM_PARTIAL_RESOURCE_PORT,
    interrupt: CM_PARTIAL_RESOURCE_INTERRUPT,
    raw: [u8; 16],
}

#[repr(C)]
#[derive(Copy, Clone)]
struct CM_PARTIAL_RESOURCE_MEMORY {
    start: ULONGLONG,
    length: ULONG,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct CM_PARTIAL_RESOURCE_PORT {
    start: ULONGLONG,
    length: ULONG,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct CM_PARTIAL_RESOURCE_INTERRUPT {
    level: ULONG,
    vector: ULONG,
    affinity: ULONG_PTR,
}

const CM_RESOURCE_TYPE_PORT: UCHAR = 1;
const CM_RESOURCE_TYPE_INTERRUPT: UCHAR = 2;
const CM_RESOURCE_TYPE_MEMORY: UCHAR = 3;

/// Парсит CM_RESOURCE_LIST и заполняет access_ranges
unsafe fn parse_cm_resources(resources: PVOID, access_ranges: &mut [ACCESS_RANGE; 8]) -> usize {
    if resources.is_null() {
        storport_print("[STORPORT] parse_cm_resources: NULL resources\n");
        return 0;
    }

    let res_list = resources as *const CM_RESOURCE_LIST;
    let list_count = (*res_list).count;
    
    storport_print("[STORPORT] CM_RESOURCE_LIST count=");
    storport_print_hex(list_count as u64);
    storport_print("\n");
    
    if list_count == 0 {
        return 0;
    }
    
    let full_desc = &(*res_list).list[0];
    let partial_list = &full_desc.partial_resource_list;
    let desc_count = partial_list.count;
    
    storport_print("[STORPORT] Partial descriptors count=");
    storport_print_hex(desc_count as u64);
    storport_print("\n");
    
    let mut range_idx = 0;
    let descriptors = &partial_list.partial_descriptors as *const CM_PARTIAL_RESOURCE_DESCRIPTOR;
    
    for i in 0..desc_count as usize {
        if range_idx >= 8 {
            break;
        }
        
        let desc = descriptors.add(i);
        let res_type = (*desc).r#type;
        
        storport_print("[STORPORT]   Descriptor[");
        storport_print_hex(i as u64);
        storport_print("] type=");
        storport_print_hex(res_type as u64);
        
        match res_type {
            CM_RESOURCE_TYPE_MEMORY => {
                let mem = (*desc).u.memory;
                let start = mem.start;
                let length = mem.length;
                
                storport_print(" MEMORY start=0x");
                storport_print_hex(start);
                storport_print(" length=0x");
                storport_print_hex(length as u64);
                storport_print("\n");
                
                if length > 0 {
                    access_ranges[range_idx] = ACCESS_RANGE {
                        RangeStart: start,
                        RangeLength: length,
                        RangeInMemory: 1,
                    };
                    range_idx += 1;
                }
            }
            CM_RESOURCE_TYPE_PORT => {
                let port = (*desc).u.port;
                let start = port.start;
                let length = port.length;
                
                storport_print(" PORT start=0x");
                storport_print_hex(start);
                storport_print(" length=0x");
                storport_print_hex(length as u64);
                storport_print("\n");
                
                if length > 0 {
                    access_ranges[range_idx] = ACCESS_RANGE {
                        RangeStart: start,
                        RangeLength: length,
                        RangeInMemory: 0,
                    };
                    range_idx += 1;
                }
            }
            CM_RESOURCE_TYPE_INTERRUPT => {
                let intr = (*desc).u.interrupt;
                let level = intr.level;
                let vector = intr.vector;
                let affinity = intr.affinity;
                
                storport_print(" INTERRUPT level=");
                storport_print_hex(level as u64);
                storport_print(" vector=");
                storport_print_hex(vector as u64);
                storport_print(" affinity=0x");
                storport_print_hex(affinity as u64);
                storport_print("\n");
            }
            _ => {
                storport_print(" OTHER\n");
            }
        }
    }
    
    storport_print("[STORPORT] Total access_ranges parsed: ");
    storport_print_hex(range_idx as u64);
    storport_print("\n");
    
    range_idx
}

/// Parsed interrupt resource info
#[derive(Clone, Copy)]
pub struct InterruptResource {
    pub level: ULONG,
    pub vector: ULONG,
    pub affinity: ULONG,
    pub found: bool,
}

impl InterruptResource {
    pub fn new() -> Self {
        Self {
            level: 0,
            vector: 0,
            affinity: 0,
            found: false,
        }
    }
}

/// Парсит CM_RESOURCE_LIST и извлекает interrupt resource
unsafe fn parse_interrupt_resource(resources: PVOID) -> InterruptResource {
    let mut result = InterruptResource::new();
    
    if resources.is_null() {
        return result;
    }

    let res_list = resources as *const CM_RESOURCE_LIST;
    let list_count = (*res_list).count;
    
    if list_count == 0 {
        return result;
    }
    
    let full_desc = &(*res_list).list[0];
    let partial_list = &full_desc.partial_resource_list;
    let desc_count = partial_list.count;
    
    let descriptors = &partial_list.partial_descriptors as *const CM_PARTIAL_RESOURCE_DESCRIPTOR;
    
    for i in 0..desc_count as usize {
        let desc = descriptors.add(i);
        let res_type = (*desc).r#type;
        
        if res_type == CM_RESOURCE_TYPE_INTERRUPT {
            let intr = (*desc).u.interrupt;
            result.level = intr.level;
            result.vector = intr.vector;
            result.affinity = intr.affinity as ULONG;
            result.found = true;
            break;
        }
    }
    
    result
}

// =============================================================================
// PDO Creation
// =============================================================================

/// SCSI PDO Extension
#[repr(C)]
struct SCSI_PDO_EXTENSION {
    signature: ULONG,
    parent_fdo: PDEVICE_OBJECT,
    path_id: UCHAR,
    target_id: UCHAR,
    lun: UCHAR,
}

const SCSI_PDO_SIGNATURE: ULONG = 0x4F445053; // 'SPDO'

/// Создает PDO для SCSI устройства
unsafe fn create_scsi_disk_pdo(
    driver_object: PDRIVER_OBJECT,
    parent_fdo: PDEVICE_OBJECT,
    path_id: u8,
    target_id: u8,
    lun: u8,
) -> PDEVICE_OBJECT {
    let mut pdo: PDEVICE_OBJECT = ptr::null_mut();
    let ext_size = core::mem::size_of::<SCSI_PDO_EXTENSION>() as ULONG;
    
    let status = IoCreateDevice(
        driver_object,
        ext_size,
        ptr::null(),
        FILE_DEVICE_DISK,
        0,
        0,
        &mut pdo,
    );
    
    if status < 0 || pdo.is_null() {
        return ptr::null_mut();
    }
    
    let pdo_ext = (*pdo).device_extension as *mut SCSI_PDO_EXTENSION;
    (*pdo_ext).signature = SCSI_PDO_SIGNATURE;
    (*pdo_ext).parent_fdo = parent_fdo;
    (*pdo_ext).path_id = path_id;
    (*pdo_ext).target_id = target_id;
    (*pdo_ext).lun = lun;
    
    (*pdo).flags &= !DO_DEVICE_INITIALIZING;
    
    pdo
}

// =============================================================================
// Interrupt Service Routine
// =============================================================================

/// StorPort ISR entry point - called by IoConnectInterrupt
/// 
/// Dispatches to miniport's HwInterrupt callback.
unsafe extern "win64" fn storport_isr_entry(
    _interrupt: crate::imports::ntoskrnl::PKINTERRUPT,
    service_context: PVOID,
) -> BOOLEAN {
    let ext = service_context as *mut STORPORT_DEVICE_EXTENSION;
    
    if ext.is_null() {
        return 0;
    }
    
    if (*ext).signature != STORPORT_DEVICE_EXTENSION_SIGNATURE {
        return 0;
    }
    
    // Call miniport's HwInterrupt if available
    if let Some(hw_interrupt) = (*ext).hw_interrupt {
        // Call miniport ISR with its device extension
        let handled = hw_interrupt((*ext).miniport_device_extension);
        
        #[cfg(feature = "storage-trace")]
        {
            if handled != 0 {
                storport_print("[STORPORT/ISR] Interrupt handled by miniport\n");
            }
        }
        
        return handled;
    }
    
    // No miniport ISR registered
    0
}

