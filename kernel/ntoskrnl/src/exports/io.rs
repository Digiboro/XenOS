//! I/O Manager PE-экспорты (`Io*`, `Zw*`)
//!
//! Экспорты для boot-драйверов и других PE модулей.
//!
//! Источники:
//! - MSDN/WDK (NT6.1): https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/

#![allow(non_snake_case)]

use ntoskrnl::io::device::{io_create_device, io_delete_device};
use ntoskrnl::io::file::{iop_create_file, iop_read_write_file, PIO_APC_ROUTINE};
use ntoskrnl::io::file::PIO_STATUS_BLOCK as PIO_STATUS_BLOCK_FILE;
use ntoskrnl::io::irp::{io_allocate_irp, io_free_irp, io_call_driver, io_complete_request, io_build_synchronous_fsd_request};
use ntoskrnl::io::types::{PDRIVER_OBJECT, PDEVICE_OBJECT, PIRP, PIO_STATUS_BLOCK};
use ntoskrnl::nt::ntstatus::NTSTATUS;
use ntoskrnl::nt::ntdef::{ULONG, BOOLEAN, UNICODE_STRING, LARGE_INTEGER, PVOID};
use ntoskrnl::ob::types::OBJECT_ATTRIBUTES;
use ntoskrnl::ob::handle::NtClose;
use ntoskrnl::ke::event::KEVENT;

/// CCHAR — signed char (NT typedef)
pub type CCHAR = i8;

// =============================================================================
// Device Object Operations
// =============================================================================

/// IoCreateDevice — создаёт объект устройства
///
/// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/nf-wdm-iocreatedevice
#[unsafe(export_name = "IoCreateDevice")]
pub unsafe extern "win64" fn IoCreateDevice(
    driver_object: PDRIVER_OBJECT,
    device_extension_size: ULONG,
    device_name: *const UNICODE_STRING,
    device_type: ULONG,
    device_characteristics: ULONG,
    exclusive: BOOLEAN,
    device_object: *mut PDEVICE_OBJECT,
) -> NTSTATUS {
    // Конвертируем указатель на имя в Option
    let name = if device_name.is_null() {
        None
    } else {
        Some(&*device_name)
    };
    
    io_create_device(
        driver_object,
        device_extension_size,
        name,
        device_type,
        device_characteristics,
        exclusive != 0,
        device_object,
    )
}

/// IoDeleteDevice — удаляет объект устройства
///
/// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/nf-wdm-iodeletedevice
#[unsafe(export_name = "IoDeleteDevice")]
pub unsafe extern "win64" fn IoDeleteDevice(device_object: PDEVICE_OBJECT) {
    unsafe { io_delete_device(device_object) }
}

/// IoAttachDeviceToDeviceStack — присоединяет устройство к стеку
///
/// Возвращает устройство, к которому присоединились (вершина стека).
#[unsafe(export_name = "IoAttachDeviceToDeviceStack")]
pub unsafe extern "win64" fn IoAttachDeviceToDeviceStack(
    source_device: PDEVICE_OBJECT,
    target_device: PDEVICE_OBJECT,
) -> PDEVICE_OBJECT {
    unsafe { ntoskrnl::io::device::io_attach_device_to_device_stack(source_device, target_device) }
}

// =============================================================================
// Driver Extension Operations
// =============================================================================

/// IoAllocateDriverObjectExtension — выделяет расширение для драйвера
///
/// Позволяет драйверу сохранить per-driver данные, привязанные к DRIVER_OBJECT.
/// Обычно client_identification_address - это адрес AddDevice функции драйвера.
///
/// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/nf-wdm-ioallocatedriverobjectextension
#[unsafe(export_name = "IoAllocateDriverObjectExtension")]
pub unsafe extern "win64" fn IoAllocateDriverObjectExtension(
    driver_object: PDRIVER_OBJECT,
    client_identification_address: PVOID,
    driver_object_extension_size: ULONG,
    driver_object_extension: *mut PVOID,
) -> NTSTATUS {
    #[cfg(feature = "storage-trace")]
    {
        ntoskrnl::kd::dbg_print("[DRIVER] IoAllocateDriverObjectExtension: driver=0x");
        ntoskrnl::kd::dbg_print_hex(driver_object as u64);
        ntoskrnl::kd::dbg_print(" client_id=0x");
        ntoskrnl::kd::dbg_print_hex(client_identification_address as u64);
        ntoskrnl::kd::dbg_print(" size=0x");
        ntoskrnl::kd::dbg_print_hex(driver_object_extension_size as u64);
        ntoskrnl::kd::dbg_print("\n");
    }
    
    unsafe {
        let status = ntoskrnl::io::driver::io_allocate_driver_object_extension(
            driver_object,
            client_identification_address,
            driver_object_extension_size,
            driver_object_extension,
        );
        
        #[cfg(feature = "storage-trace")]
        {
            ntoskrnl::kd::dbg_print("[DRIVER] IoAllocateDriverObjectExtension: status=0x");
            ntoskrnl::kd::dbg_print_hex(status as u64);
            if !driver_object_extension.is_null() && status == 0 {
                ntoskrnl::kd::dbg_print(" ext=0x");
                ntoskrnl::kd::dbg_print_hex(*driver_object_extension as u64);
            }
            ntoskrnl::kd::dbg_print("\n");
        }
        
        status
    }
}

/// IoGetDriverObjectExtension — получает расширение драйвера
///
/// Ищет расширение драйвера по client_identification_address.
///
/// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/nf-wdm-iogetdriverobjectextension
#[unsafe(export_name = "IoGetDriverObjectExtension")]
pub unsafe extern "win64" fn IoGetDriverObjectExtension(
    driver_object: PDRIVER_OBJECT,
    client_identification_address: PVOID,
) -> PVOID {
    #[cfg(feature = "storage-trace")]
    {
        ntoskrnl::kd::dbg_print("[DRIVER] IoGetDriverObjectExtension: driver=0x");
        ntoskrnl::kd::dbg_print_hex(driver_object as u64);
        ntoskrnl::kd::dbg_print(" client_id=0x");
        ntoskrnl::kd::dbg_print_hex(client_identification_address as u64);
        ntoskrnl::kd::dbg_print("\n");
    }
    
    unsafe {
        let ext = ntoskrnl::io::driver::io_get_driver_object_extension(
            driver_object,
            client_identification_address,
        );
        
        #[cfg(feature = "storage-trace")]
        {
            ntoskrnl::kd::dbg_print("[DRIVER] IoGetDriverObjectExtension: returned 0x");
            ntoskrnl::kd::dbg_print_hex(ext as u64);
            ntoskrnl::kd::dbg_print("\n");
        }
        
        ext
    }
}

// =============================================================================
// Device Stack Operations
// =============================================================================

/// IoDetachDevice — отсоединяет устройство от стека
#[unsafe(export_name = "IoDetachDevice")]
pub unsafe extern "win64" fn IoDetachDevice(target_device: PDEVICE_OBJECT) {
    unsafe { ntoskrnl::io::device::io_detach_device(target_device) }
}

// =============================================================================
// IRP Operations
// =============================================================================

/// IoAllocateIrp — выделяет IRP
///
/// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/nf-wdm-ioallocateirp
#[unsafe(export_name = "IoAllocateIrp")]
pub unsafe extern "win64" fn IoAllocateIrp(
    stack_size: CCHAR,
    charge_quota: BOOLEAN,
) -> PIRP {
    io_allocate_irp(stack_size, charge_quota != 0)
}

/// IoFreeIrp — освобождает IRP
///
/// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/nf-wdm-iofreeirp
#[unsafe(export_name = "IoFreeIrp")]
pub unsafe extern "win64" fn IoFreeIrp(irp: PIRP) {
    unsafe { io_free_irp(irp) }
}

// =============================================================================
// MDL Operations
// =============================================================================

/// IoAllocateMdl — выделяет MDL для буфера
///
/// MDL описывает физические страницы, составляющие виртуальный буфер.
/// Используется для DMA и Direct I/O.
///
/// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/nf-wdm-ioallocatemdl
#[unsafe(export_name = "IoAllocateMdl")]
pub unsafe extern "win64" fn IoAllocateMdl(
    virtual_address: PVOID,
    length: ULONG,
    secondary_buffer: BOOLEAN,
    charge_quota: BOOLEAN,
    irp: PIRP,
) -> PVOID {
    #[cfg(feature = "storage-trace")]
    {
        ntoskrnl::kd::dbg_print("[MDL] IoAllocateMdl: va=0x");
        ntoskrnl::kd::dbg_print_hex(virtual_address as u64);
        ntoskrnl::kd::dbg_print(" len=0x");
        ntoskrnl::kd::dbg_print_hex(length as u64);
        ntoskrnl::kd::dbg_print("\n");
    }
    
    let mdl = ntoskrnl::mm::mdl::io_allocate_mdl(
        virtual_address,
        length,
        secondary_buffer != 0,
        charge_quota != 0,
        irp as PVOID,
    );
    
    #[cfg(feature = "storage-trace")]
    {
        ntoskrnl::kd::dbg_print("[MDL] IoAllocateMdl: returned mdl=0x");
        ntoskrnl::kd::dbg_print_hex(mdl as u64);
        ntoskrnl::kd::dbg_print("\n");
    }
    
    mdl as PVOID
}

/// IoFreeMdl — освобождает MDL
///
/// Если страницы заблокированы (MmProbeAndLockPages был вызван),
/// они будут разблокированы. Если MDL отображен (MmMapLockedPages),
/// отображение будет удалено.
///
/// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/nf-wdm-iofreemdl
#[unsafe(export_name = "IoFreeMdl")]
pub unsafe extern "win64" fn IoFreeMdl(mdl: PVOID) {
    #[cfg(feature = "storage-trace")]
    {
        ntoskrnl::kd::dbg_print("[MDL] IoFreeMdl: mdl=0x");
        ntoskrnl::kd::dbg_print_hex(mdl as u64);
        ntoskrnl::kd::dbg_print("\n");
    }
    
    ntoskrnl::mm::mdl::io_free_mdl(mdl as *mut ntoskrnl::mm::mdl::MDL);
}

// =============================================================================
// IRP Cancel Operations (Async I/O)
// =============================================================================

/// IoAcquireCancelSpinLock — захватывает глобальный cancel spinlock
///
/// Должен быть вызван перед изменением cancel routine или перед вызовом
/// IoCancelIrp. Возвращает предыдущий IRQL.
///
/// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/nf-wdm-ioacquirecancelspinlock
#[unsafe(export_name = "IoAcquireCancelSpinLock")]
pub unsafe extern "win64" fn IoAcquireCancelSpinLock(irql: *mut u8) {
    #[cfg(feature = "storage-trace")]
    ntoskrnl::kd::dbg_print("[CANCEL] IoAcquireCancelSpinLock\n");
    
    unsafe {
        ntoskrnl::io::irp::io_acquire_cancel_spin_lock(irql);
    }
}

/// IoReleaseCancelSpinLock — освобождает глобальный cancel spinlock
///
/// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/nf-wdm-ioreleasecancelspinlock
#[unsafe(export_name = "IoReleaseCancelSpinLock")]
pub unsafe extern "win64" fn IoReleaseCancelSpinLock(irql: u8) {
    #[cfg(feature = "storage-trace")]
    ntoskrnl::kd::dbg_print("[CANCEL] IoReleaseCancelSpinLock\n");
    
    unsafe {
        ntoskrnl::io::irp::io_release_cancel_spin_lock(irql);
    }
}

/// IoSetCancelRoutine — устанавливает cancel routine для IRP
///
/// Возвращает предыдущую cancel routine (или NULL).
/// Не требует cancel spinlock (атомарная операция).
///
/// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/nf-wdm-iosetcancelroutine
#[unsafe(export_name = "IoSetCancelRoutine")]
pub unsafe extern "win64" fn IoSetCancelRoutine(
    irp: PIRP,
    cancel_routine: PVOID, // PDRIVER_CANCEL
) -> PVOID {
    #[cfg(feature = "storage-trace")]
    {
        ntoskrnl::kd::dbg_print("[CANCEL] IoSetCancelRoutine: irp=0x");
        ntoskrnl::kd::dbg_print_hex(irp as u64);
        ntoskrnl::kd::dbg_print("\n");
    }
    
    unsafe {
        let routine = if cancel_routine.is_null() {
            None
        } else {
            Some(core::mem::transmute(cancel_routine))
        };
        
        let old = ntoskrnl::io::irp::io_set_cancel_routine(irp, routine);
        
        match old {
            Some(f) => f as PVOID,
            None => core::ptr::null_mut(),
        }
    }
}

/// IoCancelIrp — отменяет IRP
///
/// Устанавливает флаг Cancel и вызывает cancel routine если установлена.
/// Возвращает TRUE если cancel routine была вызвана.
///
/// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/nf-wdm-iocancelirp
#[unsafe(export_name = "IoCancelIrp")]
pub unsafe extern "win64" fn IoCancelIrp(irp: PIRP) -> BOOLEAN {
    #[cfg(feature = "storage-trace")]
    {
        ntoskrnl::kd::dbg_print("[CANCEL] IoCancelIrp: irp=0x");
        ntoskrnl::kd::dbg_print_hex(irp as u64);
        ntoskrnl::kd::dbg_print("\n");
    }
    
    unsafe {
        let result = ntoskrnl::io::irp::io_cancel_irp(irp);
        
        #[cfg(feature = "storage-trace")]
        {
            ntoskrnl::kd::dbg_print("[CANCEL] IoCancelIrp: result=");
            ntoskrnl::kd::dbg_print_hex(result as u64);
            ntoskrnl::kd::dbg_print("\n");
        }
        
        if result { 1 } else { 0 }
    }
}

// =============================================================================
// IRP Call Operations (continued)
// =============================================================================

/// IoCallDriver — вызывает dispatch routine драйвера
///
/// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/nf-wdm-iocalldriver
#[unsafe(export_name = "IoCallDriver")]
pub unsafe extern "win64" fn IoCallDriver(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    unsafe { io_call_driver(device_object, irp) }
}

/// IofCallDriver — fast path версия IoCallDriver
///
/// В NT это fastcall версия, но мы используем тот же win64 ABI
#[unsafe(export_name = "IofCallDriver")]
pub unsafe extern "win64" fn IofCallDriver(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    unsafe { io_call_driver(device_object, irp) }
}

/// IoCompleteRequest — завершает IRP
///
/// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/nf-wdm-iocompleterequest
#[unsafe(export_name = "IoCompleteRequest")]
pub unsafe extern "win64" fn IoCompleteRequest(
    irp: PIRP,
    priority_boost: CCHAR,
) {
    unsafe { io_complete_request(irp, priority_boost) }
}

/// IofCompleteRequest — fast path версия IoCompleteRequest
#[unsafe(export_name = "IofCompleteRequest")]
pub unsafe extern "win64" fn IofCompleteRequest(
    irp: PIRP,
    priority_boost: CCHAR,
) {
    unsafe { io_complete_request(irp, priority_boost) }
}

/// IoGetCurrentIrpStackLocation — получает текущую позицию в стеке IRP
///
/// В NT это макрос, но мы экспортируем как функцию для драйверов.
#[unsafe(export_name = "IoGetCurrentIrpStackLocation")]
pub unsafe extern "win64" fn IoGetCurrentIrpStackLocation(
    irp: PIRP,
) -> *mut ntoskrnl::io::irp::IO_STACK_LOCATION {
    unsafe { ntoskrnl::io::irp::io_get_current_irp_stack_location(irp) }
}

/// IoSkipCurrentIrpStackLocation — пропускает текущую позицию в стеке IRP
///
/// Используется когда драйвер не устанавливает completion routine и просто
/// передаёт IRP вниз по стеку.
#[unsafe(export_name = "IoSkipCurrentIrpStackLocation")]
pub unsafe extern "win64" fn IoSkipCurrentIrpStackLocation(irp: PIRP) {
    unsafe { ntoskrnl::io::irp::io_skip_current_irp_stack_location(irp) }
}

/// IoGetNextIrpStackLocation — получает следующую позицию в стеке IRP
///
/// Используется при создании нового IRP для получения stack location для заполнения.
#[unsafe(export_name = "IoGetNextIrpStackLocation")]
pub unsafe extern "win64" fn IoGetNextIrpStackLocation(
    irp: PIRP,
) -> *mut ntoskrnl::io::irp::IO_STACK_LOCATION {
    unsafe { ntoskrnl::io::irp::io_get_next_irp_stack_location(irp) }
}

/// IoBuildSynchronousFsdRequest — строит синхронный IRP для файловой операции
///
/// Создаёт IRP_MJ_READ, IRP_MJ_WRITE, IRP_MJ_FLUSH_BUFFERS или IRP_MJ_SHUTDOWN.
#[unsafe(export_name = "IoBuildSynchronousFsdRequest")]
pub unsafe extern "win64" fn IoBuildSynchronousFsdRequest(
    major_function: ULONG,
    device_object: PDEVICE_OBJECT,
    buffer: PVOID,
    length: ULONG,
    starting_offset: i64,
    event: PVOID,
    io_status_block: PVOID,
) -> PIRP {
    unsafe {
        let offset = LARGE_INTEGER { quad_part: starting_offset };
        io_build_synchronous_fsd_request(
            major_function as u8,
            device_object,
            buffer,
            length,
            &offset as *const LARGE_INTEGER,
            event as *mut KEVENT,
            io_status_block as PIO_STATUS_BLOCK,
        )
    }
}

/// IoSetNextIrpStackLocation — продвигает current location в IRP
///
/// Используется после заполнения stack location перед отправкой IRP.
#[unsafe(export_name = "IoSetNextIrpStackLocation")]
pub unsafe extern "win64" fn IoSetNextIrpStackLocation(irp: PIRP) {
    unsafe { ntoskrnl::io::irp::io_set_next_irp_stack_location(irp) }
}

/// IoCopyCurrentIrpStackLocationToNext — копирует текущую позицию в следующую
///
/// Используется когда драйвер устанавливает completion routine.
#[unsafe(export_name = "IoCopyCurrentIrpStackLocationToNext")]
pub unsafe extern "win64" fn IoCopyCurrentIrpStackLocationToNext(irp: PIRP) {
    unsafe { ntoskrnl::io::irp::io_copy_current_irp_stack_location_to_next(irp) }
}

// =============================================================================
// Zw* File Operations (Kernel-mode)
// =============================================================================

/// ZwCreateFile — создаёт или открывает файл/устройство (kernel-mode)
///
/// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-zwcreatefile
#[unsafe(export_name = "ZwCreateFile")]
pub unsafe extern "win64" fn ZwCreateFile(
    file_handle: *mut usize,
    desired_access: ULONG,
    object_attributes: *const OBJECT_ATTRIBUTES,
    io_status_block: PIO_STATUS_BLOCK_FILE,
    allocation_size: *const LARGE_INTEGER,
    file_attributes: ULONG,
    share_access: ULONG,
    create_disposition: ULONG,
    create_options: ULONG,
    ea_buffer: PVOID,
    ea_length: ULONG,
) -> NTSTATUS {
    iop_create_file(
        file_handle,
        desired_access,
        object_attributes,
        io_status_block,
        allocation_size,
        file_attributes,
        share_access,
        create_disposition,
        create_options,
        ea_buffer,
        ea_length,
        0, // KernelMode
    )
}

/// ZwOpenFile — открывает существующий файл/устройство (kernel-mode)
///
/// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-zwopenfile
#[unsafe(export_name = "ZwOpenFile")]
pub unsafe extern "win64" fn ZwOpenFile(
    file_handle: *mut usize,
    desired_access: ULONG,
    object_attributes: *const OBJECT_ATTRIBUTES,
    io_status_block: PIO_STATUS_BLOCK_FILE,
    share_access: ULONG,
    open_options: ULONG,
) -> NTSTATUS {
    // ZwOpenFile эквивалентен ZwCreateFile с FILE_OPEN
    const FILE_OPEN: ULONG = 1;
    
    iop_create_file(
        file_handle,
        desired_access,
        object_attributes,
        io_status_block,
        core::ptr::null(), // allocation_size
        0,                 // file_attributes
        share_access,
        FILE_OPEN,
        open_options,
        core::ptr::null_mut(), // ea_buffer
        0,                     // ea_length
        0,                     // KernelMode
    )
}

/// ZwClose — закрывает handle (kernel-mode)
///
/// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-zwclose
#[unsafe(export_name = "ZwClose")]
pub unsafe extern "win64" fn ZwClose(handle: usize) -> NTSTATUS {
    // ZwClose использует ту же реализацию что и NtClose
    NtClose(handle)
}

/// ZwReadFile — читает данные из файла/устройства (kernel-mode)
///
/// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-zwreadfile
#[unsafe(export_name = "ZwReadFile")]
pub unsafe extern "win64" fn ZwReadFile(
    file_handle: usize,
    event: usize,
    apc_routine: *const PIO_APC_ROUTINE,
    apc_context: PVOID,
    io_status_block: PIO_STATUS_BLOCK_FILE,
    buffer: PVOID,
    length: ULONG,
    byte_offset: *const LARGE_INTEGER,
    key: *const ULONG,
) -> NTSTATUS {
    iop_read_write_file(
        file_handle,
        event,
        apc_routine,
        apc_context,
        io_status_block,
        buffer,
        length,
        byte_offset,
        key,
        false, // is_write = false (READ)
        0,     // KernelMode
    )
}

/// ZwWriteFile — записывает данные в файл/устройство (kernel-mode)
///
/// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-zwwritefile
#[unsafe(export_name = "ZwWriteFile")]
pub unsafe extern "win64" fn ZwWriteFile(
    file_handle: usize,
    event: usize,
    apc_routine: *const PIO_APC_ROUTINE,
    apc_context: PVOID,
    io_status_block: PIO_STATUS_BLOCK_FILE,
    buffer: PVOID,
    length: ULONG,
    byte_offset: *const LARGE_INTEGER,
    key: *const ULONG,
) -> NTSTATUS {
    iop_read_write_file(
        file_handle,
        event,
        apc_routine,
        apc_context,
        io_status_block,
        buffer,
        length,
        byte_offset,
        key,
        true, // is_write = true (WRITE)
        0,    // KernelMode
    )
}

// =============================================================================
// Zw* Cancel Operations (Kernel-mode)
// =============================================================================

/// ZwCancelIoFile — отменяет все I/O операции для файла от текущего потока (kernel-mode)
///
/// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-zwcancelio
#[unsafe(export_name = "ZwCancelIoFile")]
pub unsafe extern "win64" fn ZwCancelIoFile(
    file_handle: usize,
    io_status_block: PIO_STATUS_BLOCK_FILE,
) -> NTSTATUS {
    ntoskrnl::io::file::NtCancelIoFile(file_handle, io_status_block as PVOID)
}

/// ZwCancelIoFileEx — отменяет конкретную или все I/O операции для файла (kernel-mode)
///
/// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-zwcanceliofileex
#[unsafe(export_name = "ZwCancelIoFileEx")]
pub unsafe extern "win64" fn ZwCancelIoFileEx(
    file_handle: usize,
    io_request_to_cancel: PIO_STATUS_BLOCK,
    io_status_block: PIO_STATUS_BLOCK_FILE,
) -> NTSTATUS {
    ntoskrnl::io::file::NtCancelIoFileEx(
        file_handle, 
        io_request_to_cancel as PVOID, 
        io_status_block as PVOID
    )
}

/// IoCreateSymbolicLink - создаёт символическую ссылку
///
/// Используется для DOS device mapping (\??\C: -> \Device\Harddisk0\Partition1)
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn IoCreateSymbolicLink(
    symbolic_link_name: *const UNICODE_STRING,
    device_name: *const UNICODE_STRING,
) -> NTSTATUS {
    unsafe { ntoskrnl::io::device::io_create_symbolic_link(symbolic_link_name, device_name) }
}

/// IoDeleteSymbolicLink - удаляет символическую ссылку
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn IoDeleteSymbolicLink(
    symbolic_link_name: *const UNICODE_STRING,
) -> NTSTATUS {
    unsafe { ntoskrnl::io::device::io_delete_symbolic_link(symbolic_link_name) }
}

// =============================================================================
// Volume Parameter Block (VPB) Operations
// =============================================================================

/// IoAllocateVpb - выделяет VPB для storage device
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn IoAllocateVpb(
    device_object: PDEVICE_OBJECT,
) -> *mut ntoskrnl::io::vpb::VPB {
    unsafe { ntoskrnl::io::vpb::io_allocate_vpb(device_object) }
}

/// IoFreeVpb - освобождает VPB
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn IoFreeVpb(
    vpb: *mut ntoskrnl::io::vpb::VPB,
) {
    unsafe { ntoskrnl::io::vpb::io_free_vpb(vpb) }
}

/// IoRegisterFileSystem - регистрирует file system driver
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn IoRegisterFileSystem(
    device_object: PDEVICE_OBJECT,
) {
    unsafe { ntoskrnl::io::vpb::io_register_file_system(device_object) }
}

/// IoUnregisterFileSystem - отменяет регистрацию file system driver
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn IoUnregisterFileSystem(
    device_object: PDEVICE_OBJECT,
) {
    unsafe { ntoskrnl::io::vpb::io_unregister_file_system(device_object) }
}

/// IoMountVolume - монтирует файловую систему на устройстве
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn IoMountVolume(
    device_object: PDEVICE_OBJECT,
    allow_raw_mount: BOOLEAN,
) -> NTSTATUS {
    unsafe { ntoskrnl::io::vpb::iop_mount_volume(device_object, allow_raw_mount != 0) }
}

/// IoVerifyVolume - проверяет volume и при необходимости remount
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn IoVerifyVolume(
    device_object: PDEVICE_OBJECT,
    allow_raw_mount: BOOLEAN,
) -> NTSTATUS {
    unsafe { ntoskrnl::io::vpb::io_verify_volume(device_object, allow_raw_mount != 0) }
}

// =============================================================================
// Device Interface Operations (NT 6.1)
// =============================================================================

/// GUID type for device interfaces
pub type GUID = ntoskrnl::nt::security::GUID;

/// IoRegisterDeviceInterface - регистрирует device interface для PDO
///
/// Создаёт запись о device interface для указанного класса GUID.
/// Интерфейс создаётся в disabled состоянии.
///
/// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/nf-wdm-ioregisterdeviceinterface
#[unsafe(export_name = "IoRegisterDeviceInterface")]
pub unsafe extern "win64" fn IoRegisterDeviceInterface(
    physical_device_object: PDEVICE_OBJECT,
    interface_class_guid: *const GUID,
    reference_string: *const UNICODE_STRING,
    symbolic_link_name: *mut UNICODE_STRING,
) -> NTSTATUS {
    unsafe {
        ntoskrnl::io::interface::io_register_device_interface(
            physical_device_object,
            interface_class_guid,
            reference_string,
            symbolic_link_name,
        )
    }
}

/// IoSetDeviceInterfaceState - включает или выключает device interface
///
/// Когда интерфейс включается, создаётся symbolic link.
/// Когда выключается, symbolic link удаляется.
///
/// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/nf-wdm-iosetdeviceinterfacestate
#[unsafe(export_name = "IoSetDeviceInterfaceState")]
pub unsafe extern "win64" fn IoSetDeviceInterfaceState(
    symbolic_link_name: *const UNICODE_STRING,
    enable: BOOLEAN,
) -> NTSTATUS {
    unsafe {
        ntoskrnl::io::interface::io_set_device_interface_state(
            symbolic_link_name,
            enable != 0,
        )
    }
}

// =============================================================================
// PnP Notification Operations (NT 6.1)
// =============================================================================

/// IO_NOTIFICATION_EVENT_CATEGORY type
pub type IO_NOTIFICATION_EVENT_CATEGORY = ntoskrnl::io::interface::IO_NOTIFICATION_EVENT_CATEGORY;

/// Callback type for PnP notifications
pub type PDRIVER_NOTIFICATION_CALLBACK_ROUTINE = 
    ntoskrnl::io::interface::PDRIVER_NOTIFICATION_CALLBACK_ROUTINE;

/// IoRegisterPlugPlayNotification - регистрирует callback для PnP notifications
///
/// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/nf-wdm-ioregisterplugplaynotification
#[unsafe(export_name = "IoRegisterPlugPlayNotification")]
pub unsafe extern "win64" fn IoRegisterPlugPlayNotification(
    event_category: i32,
    event_category_flags: ULONG,
    event_category_data: PVOID,
    driver_object: PDRIVER_OBJECT,
    callback_routine: PDRIVER_NOTIFICATION_CALLBACK_ROUTINE,
    context: PVOID,
    notification_entry: *mut PVOID,
) -> NTSTATUS {
    unsafe {
        // Convert i32 to enum (C ABI compatibility)
        let category = match event_category {
            0 => IO_NOTIFICATION_EVENT_CATEGORY::EventCategoryDeviceInterfaceChange,
            1 => IO_NOTIFICATION_EVENT_CATEGORY::EventCategoryHardwareProfileChange,
            2 => IO_NOTIFICATION_EVENT_CATEGORY::EventCategoryTargetDeviceChange,
            _ => IO_NOTIFICATION_EVENT_CATEGORY::EventCategoryReserved,
        };
        
        ntoskrnl::io::interface::io_register_plug_play_notification(
            category,
            event_category_flags,
            event_category_data,
            driver_object,
            callback_routine,
            context,
            notification_entry,
        )
    }
}

/// IoUnregisterPlugPlayNotification - отменяет регистрацию notification callback
///
/// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/nf-wdm-iounregisterplugplaynotification
#[unsafe(export_name = "IoUnregisterPlugPlayNotification")]
pub unsafe extern "win64" fn IoUnregisterPlugPlayNotification(
    notification_entry: PVOID,
) -> NTSTATUS {
    unsafe {
        ntoskrnl::io::interface::io_unregister_plug_play_notification(notification_entry)
    }
}

/// IoUnregisterPlugPlayNotificationEx - безопасная отмена регистрации
///
/// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/nf-wdm-iounregisterplugplaynotificationex
#[unsafe(export_name = "IoUnregisterPlugPlayNotificationEx")]
pub unsafe extern "win64" fn IoUnregisterPlugPlayNotificationEx(
    notification_entry: PVOID,
) -> NTSTATUS {
    unsafe {
        ntoskrnl::io::interface::io_unregister_plug_play_notification_ex(notification_entry)
    }
}

