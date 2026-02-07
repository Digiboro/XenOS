//! Disk Storage Class Driver for XenOS
//!
//! Provides block device access for SCSI\Disk devices.
//!
//! # Архитектура
//!
//! ```text
//! AHCI PDO (SCSI\Disk)
//!       │
//!       │  AddDevice
//!       ▼
//! ┌─────────────┐
//! │   Disk FDO  │  ← Этот драйвер
//! │ (disk.sys)  │
//! └──────┬──────┘
//!        │
//!        │  IRP_MJ_READ/WRITE
//!        ▼
//!    \Device\Harddisk0\DR0
//! ```

#![no_std]
#![allow(non_snake_case)]
#![allow(static_mut_refs)]
#![allow(unused_unsafe)]

mod imports;
mod types;
mod partition;
mod scsi;

use types::*;
use core::ptr;
use core::sync::atomic::{AtomicU32, Ordering};

// =============================================================================
// Global State
// =============================================================================

/// Счётчик harddisk устройств (для генерации уникальных номеров)
static HARDDISK_COUNT: AtomicU32 = AtomicU32::new(0);

// =============================================================================
// Driver Entry
// =============================================================================

/// DriverEntry - точка входа драйвера
#[unsafe(no_mangle)]
pub extern "win64" fn DriverEntry(
    driver_object: PDRIVER_OBJECT,
    _registry_path: *const UNICODE_STRING,
) -> NTSTATUS {
    unsafe {
        // Устанавливаем AddDevice callback
        let driver_ext = (*driver_object).driver_extension;
        if !driver_ext.is_null() {
            (*driver_ext).add_device = Some(DiskAddDevice);
        }

        // Устанавливаем dispatch функции
        (*driver_object).major_function[IRP_MJ_PNP as usize] = Some(DiskDispatchPnp);
        (*driver_object).major_function[IRP_MJ_POWER as usize] = Some(DiskDispatchPower);
        (*driver_object).major_function[IRP_MJ_CREATE as usize] = Some(DiskDispatchCreate);
        (*driver_object).major_function[IRP_MJ_CLOSE as usize] = Some(DiskDispatchClose);
        (*driver_object).major_function[IRP_MJ_CLEANUP as usize] = Some(DiskDispatchCleanup);
        (*driver_object).major_function[IRP_MJ_READ as usize] = Some(DiskDispatchReadWrite);
        (*driver_object).major_function[IRP_MJ_WRITE as usize] = Some(DiskDispatchReadWrite);
        (*driver_object).major_function[IRP_MJ_DEVICE_CONTROL as usize] = Some(DiskDispatchDeviceControl);

        STATUS_SUCCESS
    }
}

// =============================================================================
// AddDevice
// =============================================================================

/// DiskAddDevice - создание FDO для Disk устройства
/// 
/// NT 6.1 Architecture:
/// - Creates named FDO: \Device\HarddiskX\DR0 (raw disk access)
/// - Attaches FDO to PDO stack
/// - Does NOT create partition PDOs here (that's partmgr's job later)
unsafe extern "win64" fn DiskAddDevice(
    driver_object: PDRIVER_OBJECT,
    physical_device_object: PDEVICE_OBJECT,
) -> NTSTATUS {
    // Генерируем уникальный номер harddisk
    let harddisk_number = HARDDISK_COUNT.fetch_add(1, Ordering::SeqCst);
    
    // Build device name: "\Device\HarddiskX\DR0"
    let mut name_str = [0u8; 64];
    let mut idx = 0usize;
    
    for &b in b"\\Device\\Harddisk".iter() {
        name_str[idx] = b;
        idx += 1;
    }
    
    let num_str = format_u32(harddisk_number);
    for &b in num_str.iter() {
        if b == 0 { break; }
        name_str[idx] = b;
        idx += 1;
    }
    
    for &b in b"\\DR0".iter() {
        name_str[idx] = b;
        idx += 1;
    }
    
    // Allocate Unicode buffer from pool
    let name_buffer = ExAllocatePoolWithTag(
        NON_PAGED_POOL,
        (idx * 2) + 2,
        DISK_POOL_TAG
    ) as *mut u16;
    
    let device_name_ptr = if !name_buffer.is_null() {
        for (i, &b) in name_str[..idx].iter().enumerate() {
            *name_buffer.add(i) = b as u16;
        }
        *name_buffer.add(idx) = 0;
        
        let device_name = UNICODE_STRING {
            length: (idx * 2) as USHORT,
            maximum_length: ((idx * 2) + 2) as USHORT,
            buffer: name_buffer,
        };
        &device_name as *const UNICODE_STRING
    } else {
        ptr::null()
    };
    
    // Создаём FDO с именем (или anonymous если allocation failed)
    let mut fdo: PDEVICE_OBJECT = ptr::null_mut();
    let ext_size = core::mem::size_of::<DISK_FDO_EXTENSION>() as ULONG;

    let status = IoCreateDevice(
        driver_object,
        ext_size,
        device_name_ptr, // Named device: \Device\HarddiskX\DR0
        FILE_DEVICE_DISK,
        0,
        0,
        &mut fdo,
    );

    // Free name buffer (IoCreateDevice copies it)
    if !name_buffer.is_null() {
        ExFreePoolWithTag(name_buffer as PVOID, DISK_POOL_TAG);
    }

    if status < 0 {
        return status;
    }
    
    // Сохраняем номер в extension для будущего использования
    let fdo_ext = (*fdo).device_extension as *mut DISK_FDO_EXTENSION;
    ptr::write_bytes(fdo_ext, 0, core::mem::size_of::<DISK_FDO_EXTENSION>());
    
    (*fdo_ext).common.is_fdo = 1;
    (*fdo_ext).common.self_device = fdo;
    (*fdo_ext).physical_device_object = physical_device_object;
    (*fdo_ext).harddisk_number = harddisk_number;

    // Attach к стеку PDO
    let lower_device = IoAttachDeviceToDeviceStack(fdo, physical_device_object);
    if lower_device.is_null() {
        IoDeleteDevice(fdo);
        return STATUS_NO_SUCH_DEVICE;
    }

    (*fdo_ext).lower_device = lower_device;

    // Копируем флаги от нижнего устройства
    (*fdo).flags |= DO_DIRECT_IO | DO_POWER_PAGABLE;
    (*fdo).flags &= !DO_DEVICE_INITIALIZING;

    STATUS_SUCCESS
}

// =============================================================================
// Dispatch Routines
// =============================================================================

/// DiskDispatchPnp - обработка PnP IRP
unsafe extern "win64" fn DiskDispatchPnp(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    let ext = (*device_object).device_extension as *const DISK_COMMON_EXTENSION;

    if (*ext).is_fdo != 0 {
        disk_fdo_pnp_dispatch(device_object, irp)
    } else {
        // Partition PDO
        disk_pdo_pnp_dispatch(device_object, irp)
    }
}

/// DiskDispatchPower - обработка Power IRP
unsafe extern "win64" fn DiskDispatchPower(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    let ext = (*device_object).device_extension as *const DISK_FDO_EXTENSION;
    
    // Передаём вниз
    IoSkipCurrentIrpStackLocation(irp);
    IoCallDriver((*ext).lower_device, irp)
}

/// DiskDispatchCreate - обработка IRP_MJ_CREATE
unsafe extern "win64" fn DiskDispatchCreate(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    unsafe {
        let ext = (*device_object).device_extension as *const DISK_COMMON_EXTENSION;
        
        if (*ext).is_fdo != 0 {
            // FDO - разрешаем открытие для управляющих операций
            (*irp).io_status.status = STATUS_SUCCESS;
            (*irp).io_status.information = 0; // FILE_OPENED
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            STATUS_SUCCESS
        } else {
            // Partition PDO - проверяем что устройство готово
            let pdo_ext = ext as *const DISK_PARTITION_EXTENSION;
            let parent_fdo = (*pdo_ext).parent_fdo;
            
            if parent_fdo.is_null() {
                (*irp).io_status.status = STATUS_NO_SUCH_DEVICE;
                (*irp).io_status.information = 0;
                IoCompleteRequest(irp, IO_NO_INCREMENT);
                return STATUS_NO_SUCH_DEVICE;
            }
            
            let parent_ext = (*parent_fdo).device_extension as *const DISK_FDO_EXTENSION;
            
            // Проверяем что parent FDO started
            if (*parent_ext).started == 0 {
                (*irp).io_status.status = STATUS_DEVICE_NOT_READY;
                (*irp).io_status.information = 0;
                IoCompleteRequest(irp, IO_NO_INCREMENT);
                return STATUS_DEVICE_NOT_READY;
            }
            
            // Разрешаем открытие партиции
            (*irp).io_status.status = STATUS_SUCCESS;
            (*irp).io_status.information = 0; // FILE_OPENED
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            STATUS_SUCCESS
        }
    }
}

/// DiskDispatchCleanup - обработка IRP_MJ_CLEANUP
unsafe extern "win64" fn DiskDispatchCleanup(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    unsafe {
        // Cleanup - освобождаем ресурсы, связанные с file object
        // Для disk устройств обычно нечего делать
        (*irp).io_status.status = STATUS_SUCCESS;
        (*irp).io_status.information = 0;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        STATUS_SUCCESS
    }
}

/// DiskDispatchClose - обработка IRP_MJ_CLOSE
unsafe extern "win64" fn DiskDispatchClose(
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

/// DiskDispatchReadWrite - обработка IRP_MJ_READ/WRITE
unsafe extern "win64" fn DiskDispatchReadWrite(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    unsafe {
        let ext = (*device_object).device_extension as *const DISK_COMMON_EXTENSION;
        
        if (*ext).is_fdo != 0 {
            // FDO - конвертируем IRP_MJ_READ/WRITE в SRB
            disk_fdo_read_write(device_object, irp)
        } else {
            // Partition PDO - offset translation
            disk_pdo_read_write(device_object, irp)
        }
    }
}

/// READ/WRITE для FDO - конвертирует в SRB и отправляет в StorPort
unsafe fn disk_fdo_read_write(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    unsafe {
        let fdo_ext = (*device_object).device_extension as *mut DISK_FDO_EXTENSION;
        let stack = IoGetCurrentIrpStackLocation(irp);
        
        // Определяем тип операции по major function
        let is_read = (*stack).major_function == IRP_MJ_READ;
        
        // Получаем параметры из IRP
        let params_ptr = &(*stack).parameters as *const _ as *const u8;
        let length = core::ptr::read(params_ptr as *const u32);
        let byte_offset = core::ptr::read(params_ptr.add(8) as *const i64);
        
        if length == 0 {
            (*irp).io_status.status = STATUS_SUCCESS;
            (*irp).io_status.information = 0;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            return STATUS_SUCCESS;
        }
        
        let sector_size = (*fdo_ext).sector_size as u64;
        
        // Получаем буфер - для DIRECT_IO буфер в MDL, для BUFFERED_IO в AssociatedIrp.SystemBuffer
        let buffer = if !(*irp).mdl_address.is_null() {
            // DIRECT_IO - получаем system address из MDL
            let mdl = (*irp).mdl_address as *mut crate::types::MDL;
            // mapped_system_va содержит system VA после mm_map_locked_pages
            (*mdl).mapped_system_va
        } else if !(*irp).associated_irp.system_buffer.is_null() {
            // BUFFERED_IO
            (*irp).associated_irp.system_buffer
        } else {
            // Neither - user_buffer
            (*irp).user_buffer
        };
        
        if buffer.is_null() {
            disk_print("[DISK] ERROR: No buffer for READ/WRITE\n");
            (*irp).io_status.status = STATUS_INVALID_PARAMETER;
            (*irp).io_status.information = 0;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            return STATUS_INVALID_PARAMETER;
        }
        
        // Выполняем SCSI операцию
        let (status, bytes_transferred) = scsi::disk_scsi_execute(
            (*fdo_ext).lower_device,
            byte_offset as u64,
            length,
            buffer,
            sector_size as u32,
            is_read,
        );
        
        (*irp).io_status.status = status;
        (*irp).io_status.information = bytes_transferred as ULONG_PTR;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        status
    }
}

/// DiskDispatchDeviceControl - обработка IRP_MJ_DEVICE_CONTROL
unsafe extern "win64" fn DiskDispatchDeviceControl(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    unsafe {
        let ext = (*device_object).device_extension as *const DISK_COMMON_EXTENSION;
        
        if (*ext).is_fdo != 0 {
            // FDO - handle disk IOCTLs
            disk_fdo_device_control(device_object, irp)
        } else {
            // Partition PDO - обрабатываем IOCTLs
            disk_pdo_device_control(device_object, irp)
        }
    }
}

/// DEVICE_CONTROL для FDO - handles IOCTL_DISK_* for the whole disk
unsafe fn disk_fdo_device_control(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    let fdo_ext = (*device_object).device_extension as *mut DISK_FDO_EXTENSION;
    let stack = IoGetCurrentIrpStackLocation(irp);
    
    // Get IOCTL code from parameters
    let params_ptr = &(*stack).parameters as *const _ as *const u8;
    let io_control_code = core::ptr::read(params_ptr.add(8) as *const u32);
    
    match io_control_code {
        IOCTL_DISK_GET_DRIVE_GEOMETRY => {
            disk_fdo_ioctl_get_geometry(device_object, irp, fdo_ext)
        }
        IOCTL_DISK_GET_DRIVE_GEOMETRY_EX => {
            disk_fdo_ioctl_get_geometry_ex(device_object, irp, fdo_ext)
        }
        IOCTL_DISK_GET_LENGTH_INFO => {
            disk_fdo_ioctl_get_length_info(device_object, irp, fdo_ext)
        }
        IOCTL_DISK_GET_DRIVE_LAYOUT => {
            disk_fdo_ioctl_get_drive_layout(device_object, irp, fdo_ext)
        }
        IOCTL_DISK_GET_DRIVE_LAYOUT_EX => {
            disk_fdo_ioctl_get_drive_layout_ex(device_object, irp, fdo_ext)
        }
        IOCTL_DISK_IS_WRITABLE => {
            // Disk is always writable (no write protection)
            (*irp).io_status.status = STATUS_SUCCESS;
            (*irp).io_status.information = 0;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            STATUS_SUCCESS
        }
        IOCTL_DISK_GET_PARTITION_INFO_EX | IOCTL_DISK_GET_PARTITION_INFO => {
            // Partition info for whole disk (Partition0)
            disk_fdo_ioctl_get_partition_info(device_object, irp, fdo_ext)
        }
        IOCTL_DISK_MEDIA_REMOVAL | IOCTL_DISK_EJECT_MEDIA => {
            // Not supported for fixed disks
            (*irp).io_status.status = STATUS_INVALID_DEVICE_REQUEST;
            (*irp).io_status.information = 0;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            STATUS_INVALID_DEVICE_REQUEST
        }
        _ => {
            // Unknown IOCTL - pass down to StorPort
            IoSkipCurrentIrpStackLocation(irp);
            IoCallDriver((*fdo_ext).lower_device, irp)
        }
    }
}

/// IOCTL_DISK_GET_DRIVE_GEOMETRY для FDO
unsafe fn disk_fdo_ioctl_get_geometry(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
    fdo_ext: *mut DISK_FDO_EXTENSION,
) -> NTSTATUS {
    let buffer = (*irp).associated_irp.system_buffer;
    if buffer.is_null() {
        (*irp).io_status.status = STATUS_INVALID_PARAMETER;
        (*irp).io_status.information = 0;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_INVALID_PARAMETER;
    }
    
    let geometry = buffer as *mut DISK_GEOMETRY;
    let sector_count = (*fdo_ext).sector_count;
    let sector_size = (*fdo_ext).sector_size;
    
    // Calculate C/H/S from total sectors
    // Use standard geometry: 255 heads, 63 sectors per track
    let sectors_per_track = 63u32;
    let heads = 255u32;
    let cylinders = sector_count / (sectors_per_track as u64 * heads as u64);
    
    (*geometry).cylinders = cylinders as i64;
    (*geometry).media_type = MEDIA_TYPE_FIXED_MEDIA;
    (*geometry).tracks_per_cylinder = heads;
    (*geometry).sectors_per_track = sectors_per_track;
    (*geometry).bytes_per_sector = sector_size;
    
    (*irp).io_status.status = STATUS_SUCCESS;
    (*irp).io_status.information = core::mem::size_of::<DISK_GEOMETRY>();
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    STATUS_SUCCESS
}

/// IOCTL_DISK_GET_DRIVE_GEOMETRY_EX для FDO
unsafe fn disk_fdo_ioctl_get_geometry_ex(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
    fdo_ext: *mut DISK_FDO_EXTENSION,
) -> NTSTATUS {
    let buffer = (*irp).associated_irp.system_buffer;
    if buffer.is_null() {
        (*irp).io_status.status = STATUS_INVALID_PARAMETER;
        (*irp).io_status.information = 0;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_INVALID_PARAMETER;
    }
    
    let geometry_ex = buffer as *mut DISK_GEOMETRY_EX;
    let sector_count = (*fdo_ext).sector_count;
    let sector_size = (*fdo_ext).sector_size;
    
    // Calculate C/H/S
    let sectors_per_track = 63u32;
    let heads = 255u32;
    let cylinders = sector_count / (sectors_per_track as u64 * heads as u64);
    
    (*geometry_ex).geometry.cylinders = cylinders as i64;
    (*geometry_ex).geometry.media_type = MEDIA_TYPE_FIXED_MEDIA;
    (*geometry_ex).geometry.tracks_per_cylinder = heads;
    (*geometry_ex).geometry.sectors_per_track = sectors_per_track;
    (*geometry_ex).geometry.bytes_per_sector = sector_size;
    (*geometry_ex).disk_size = (sector_count * sector_size as u64) as i64;
    (*geometry_ex).data = [0u8; 1]; // No additional data
    
    (*irp).io_status.status = STATUS_SUCCESS;
    (*irp).io_status.information = core::mem::size_of::<DISK_GEOMETRY_EX>();
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    STATUS_SUCCESS
}

/// IOCTL_DISK_GET_LENGTH_INFO для FDO
unsafe fn disk_fdo_ioctl_get_length_info(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
    fdo_ext: *mut DISK_FDO_EXTENSION,
) -> NTSTATUS {
    let buffer = (*irp).associated_irp.system_buffer;
    if buffer.is_null() {
        (*irp).io_status.status = STATUS_INVALID_PARAMETER;
        (*irp).io_status.information = 0;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_INVALID_PARAMETER;
    }
    
    let length_info = buffer as *mut GET_LENGTH_INFORMATION;
    (*length_info).length = ((*fdo_ext).sector_count * (*fdo_ext).sector_size as u64) as i64;
    
    (*irp).io_status.status = STATUS_SUCCESS;
    (*irp).io_status.information = core::mem::size_of::<GET_LENGTH_INFORMATION>();
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    STATUS_SUCCESS
}

/// IOCTL_DISK_GET_DRIVE_LAYOUT для FDO
unsafe fn disk_fdo_ioctl_get_drive_layout(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
    fdo_ext: *mut DISK_FDO_EXTENSION,
) -> NTSTATUS {
    // Return partition layout information
    // This is used by disk management tools
    
    let buffer = (*irp).associated_irp.system_buffer;
    if buffer.is_null() {
        (*irp).io_status.status = STATUS_INVALID_PARAMETER;
        (*irp).io_status.information = 0;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_INVALID_PARAMETER;
    }
    
    let layout = buffer as *mut DRIVE_LAYOUT_INFORMATION;
    let sector_size = (*fdo_ext).sector_size as u64;
    
    // Fill with partition PDO info
    (*layout).partition_count = (*fdo_ext).partition_count;
    (*layout).signature = 0; // MBR signature - would need to read from disk
    
    // Fill partition entries
    for i in 0..(*fdo_ext).partition_count as usize {
        if i >= 4 { break; } // DRIVE_LAYOUT_INFORMATION supports up to 4 partitions
        
        let pdo = (*fdo_ext).partition_pdos[i];
        if pdo.is_null() { continue; }
        
        let pdo_ext = (*pdo).device_extension as *const DISK_PARTITION_EXTENSION;
        let entry = &mut (*layout).partition_entry[i];
        
        entry.starting_offset = ((*pdo_ext).starting_lba * sector_size) as i64;
        entry.partition_length = ((*pdo_ext).sector_count * sector_size) as i64;
        entry.hidden_sectors = 0;
        entry.partition_number = (*pdo_ext).partition_number;
        entry.partition_type = (*pdo_ext).partition_type;
        entry.bootable = (*pdo_ext).bootable;
        entry.recognized_partition = 1;
        entry.rewrite_partition = 0;
    }
    
    let info_size = core::mem::size_of::<DRIVE_LAYOUT_INFORMATION>() 
        + ((*fdo_ext).partition_count.saturating_sub(1) as usize) * core::mem::size_of::<PARTITION_INFORMATION>();
    
    (*irp).io_status.status = STATUS_SUCCESS;
    (*irp).io_status.information = info_size;
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    STATUS_SUCCESS
}

/// IOCTL_DISK_GET_DRIVE_LAYOUT_EX для FDO
unsafe fn disk_fdo_ioctl_get_drive_layout_ex(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
    fdo_ext: *mut DISK_FDO_EXTENSION,
) -> NTSTATUS {
    // For now, return not supported
    // Full implementation would require reading MBR/GPT header
    (*irp).io_status.status = STATUS_NOT_SUPPORTED;
    (*irp).io_status.information = 0;
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    STATUS_NOT_SUPPORTED
}

/// IOCTL_DISK_GET_PARTITION_INFO для FDO (whole disk = Partition0)
unsafe fn disk_fdo_ioctl_get_partition_info(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
    fdo_ext: *mut DISK_FDO_EXTENSION,
) -> NTSTATUS {
    let buffer = (*irp).associated_irp.system_buffer;
    if buffer.is_null() {
        (*irp).io_status.status = STATUS_INVALID_PARAMETER;
        (*irp).io_status.information = 0;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_INVALID_PARAMETER;
    }
    
    let partition_info = buffer as *mut PARTITION_INFORMATION;
    let sector_size = (*fdo_ext).sector_size as u64;
    
    // FDO represents the whole disk (Partition0)
    (*partition_info).starting_offset = 0;
    (*partition_info).partition_length = ((*fdo_ext).sector_count * sector_size) as i64;
    (*partition_info).hidden_sectors = 0;
    (*partition_info).partition_number = 0;
    (*partition_info).partition_type = 0; // Raw disk
    (*partition_info).bootable = 0;
    (*partition_info).recognized_partition = 1;
    (*partition_info).rewrite_partition = 0;
    
    (*irp).io_status.status = STATUS_SUCCESS;
    (*irp).io_status.information = core::mem::size_of::<PARTITION_INFORMATION>();
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    STATUS_SUCCESS
}

// =============================================================================
// PnP Handlers
// =============================================================================

/// Обработка PnP IRP для FDO
unsafe fn disk_fdo_pnp_dispatch(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    let stack = IoGetCurrentIrpStackLocation(irp);
    let minor = (*stack).minor_function;
    let fdo_ext = (*device_object).device_extension as *mut DISK_FDO_EXTENSION;

    match minor {
        IRP_MN_START_DEVICE => disk_fdo_start_device(device_object, irp, fdo_ext),
        IRP_MN_STOP_DEVICE => disk_fdo_stop_device(device_object, irp, fdo_ext),
        IRP_MN_REMOVE_DEVICE => disk_fdo_remove_device(device_object, irp, fdo_ext),
        IRP_MN_QUERY_DEVICE_RELATIONS => disk_fdo_query_device_relations(device_object, irp, fdo_ext),
        _ => {
            // Передаём неизвестные IRP вниз
            IoSkipCurrentIrpStackLocation(irp);
            IoCallDriver((*fdo_ext).lower_device, irp)
        }
    }
}

/// PnP dispatch для Partition PDO
unsafe fn disk_pdo_pnp_dispatch(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    let stack = IoGetCurrentIrpStackLocation(irp);
    let minor = (*stack).minor_function;
    let pdo_ext = (*device_object).device_extension as *mut DISK_PARTITION_EXTENSION;

    match minor {
        IRP_MN_START_DEVICE => {
            // PDO уже готов к работе
            (*irp).io_status.status = STATUS_SUCCESS;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            STATUS_SUCCESS
        }
        IRP_MN_QUERY_ID => disk_pdo_query_id(device_object, irp, pdo_ext),
        IRP_MN_QUERY_DEVICE_RELATIONS => disk_pdo_query_device_relations(device_object, irp, pdo_ext),
        IRP_MN_REMOVE_DEVICE => {
            // Завершаем успешно
            (*irp).io_status.status = STATUS_SUCCESS;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            STATUS_SUCCESS
        }
        IRP_MN_QUERY_CAPABILITIES => disk_pdo_query_capabilities(device_object, irp, pdo_ext),
        IRP_MN_QUERY_DEVICE_TEXT => disk_pdo_query_device_text(device_object, irp, pdo_ext),
        _ => {
            // Другие PnP IRP завершаем как есть
            let status = (*irp).io_status.status;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            status
        }
    }
}

/// IRP_MN_START_DEVICE для FDO
/// 
/// NT 6.1 Architecture: disk.sys is a class driver that:
/// - Creates \Device\HarddiskX\DR0 for raw disk access
/// - Handles IOCTL_DISK_* requests
/// - Does NOT create partition PDOs (that's partmgr's job)
/// - Does NOT create DOS device links (that's mountmgr's job)
/// - Does NOT mount filesystems (that's I/O manager's job)
unsafe fn disk_fdo_start_device(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
    fdo_ext: *mut DISK_FDO_EXTENSION,
) -> NTSTATUS {
    unsafe {
        // Передаём START_DEVICE вниз
        IoSkipCurrentIrpStackLocation(irp);
        let status = IoCallDriver((*fdo_ext).lower_device, irp);

        if status >= 0 {
            (*fdo_ext).started = 1;
            
            // Получаем geometry через SCSI READ CAPACITY
            let lower_device = (*fdo_ext).lower_device;
            if !lower_device.is_null() {
                let (sector_count, sector_size) = scsi::scsi_read_capacity(lower_device);
                
                if sector_count > 0 && sector_size > 0 {
                    (*fdo_ext).sector_count = sector_count;
                    (*fdo_ext).sector_size = sector_size;
                } else {
                    disk_print("[DISK] WARNING: READ CAPACITY failed, using fallback geometry\n");
                    (*fdo_ext).sector_size = 512;
                    (*fdo_ext).sector_count = 262144; // 128 MB
                }
            }
            
            // Выводим информацию о диске
            disk_print_info(fdo_ext);
            
            // Create named device: \Device\HarddiskX\DR0
            create_raw_disk_device(device_object, fdo_ext);
            
            // NOTE: In NT 6.1 architecture:
            // - Partition table reading is done by partmgr.sys (filter on disk FDO)
            // - Partition PDOs are created by partmgr.sys
            // - DOS device links (C:, D:) are managed by mountmgr.sys
            // - File system mounting is done by I/O Manager when volume is accessed
            
            // Читаем таблицу партиций (временно для отладки, будет перенесено в partmgr)
            disk_print("[DISK] Reading partition table...\n");
            let partition_list = partition::read_partition_table(fdo_ext);
            
            disk_print("[DISK] Found ");
            disk_print_dec(partition_list.count as u64);
            disk_print(" partition(s)\n");
            
            // NOTE: Partition PDO creation moved to partmgr.sys (Phase 5)
            // partmgr filter will enumerate partitions and create PDOs
            // disk.sys now only manages the whole disk FDO
            
            // Log found partitions for debugging
            for i in 0..partition_list.count {
                let partition = &partition_list.partitions[i];
                disk_print("[DISK]   Partition ");
                disk_print_dec(partition.partition_number as u64);
                disk_print(": LBA ");
                disk_print_dec(partition.starting_lba);
                disk_print(", sectors ");
                disk_print_dec(partition.sector_count);
                disk_print(", type 0x");
                disk_print_hex(partition.partition_type as u64);
                if partition.bootable {
                    disk_print(" [BOOT]");
                }
                disk_print("\n");
            }
        }

        status
    }
}

/// Log raw disk device name (NT naming: \Device\HarddiskX\DR0)
/// 
/// The FDO is already named (created in AddDevice with device name).
/// This function just logs the name for debugging.
unsafe fn create_raw_disk_device(
    _device_object: PDEVICE_OBJECT,
    fdo_ext: *mut DISK_FDO_EXTENSION,
) {
    let harddisk_num = (*fdo_ext).harddisk_number;
    
    // Log device name (already created in AddDevice)
    disk_print("[DISK] Raw disk device: \\Device\\Harddisk");
    disk_print_dec(harddisk_num as u64);
    disk_print("\\DR0\n");
    
    // NOTE: In NT 6.1, disk.sys also registers device interface:
    // IoRegisterDeviceInterface(device_object, &GUID_DEVINTERFACE_DISK, NULL, &interface_name);
    // IoSetDeviceInterfaceState(&interface_name, TRUE);
    // This allows user-mode applications to find disk devices.
}

/// IRP_MN_STOP_DEVICE для FDO
unsafe fn disk_fdo_stop_device(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
    fdo_ext: *mut DISK_FDO_EXTENSION,
) -> NTSTATUS {
    unsafe {
        (*fdo_ext).started = 0;
        IoSkipCurrentIrpStackLocation(irp);
        IoCallDriver((*fdo_ext).lower_device, irp)
    }
}

/// IRP_MN_REMOVE_DEVICE для FDO
unsafe fn disk_fdo_remove_device(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
    fdo_ext: *mut DISK_FDO_EXTENSION,
) -> NTSTATUS {
    unsafe {
        // Передаём вниз
        IoSkipCurrentIrpStackLocation(irp);
        let status = IoCallDriver((*fdo_ext).lower_device, irp);

        // Detach и удаляем FDO
        IoDetachDevice((*fdo_ext).lower_device);
        IoDeleteDevice(device_object);

        status
    }
}

/// IRP_MN_QUERY_DEVICE_RELATIONS для FDO
unsafe fn disk_fdo_query_device_relations(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
    fdo_ext: *mut DISK_FDO_EXTENSION,
) -> NTSTATUS {
    let stack = IoGetCurrentIrpStackLocation(irp);
    
    // Получаем тип связей
    let params_ptr = &(*stack).parameters as *const _ as *const u8;
    let relation_type = core::ptr::read(params_ptr as *const ULONG);
    
    if relation_type != BUS_RELATIONS {
        // Не BusRelations - передаём вниз
        IoSkipCurrentIrpStackLocation(irp);
        return IoCallDriver((*fdo_ext).lower_device, irp);
    }
    
    // BusRelations - partition PDOs are now managed by partmgr filter
    // Return empty relations - partmgr will intercept and return its own PDOs
    (*irp).io_status.status = STATUS_SUCCESS;
    (*irp).io_status.information = 0;
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    STATUS_SUCCESS
}

/// IRP_MN_QUERY_ID для Partition PDO
unsafe fn disk_pdo_query_id(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
    pdo_ext: *mut DISK_PARTITION_EXTENSION,
) -> NTSTATUS {
    let stack = IoGetCurrentIrpStackLocation(irp);
    
    // Получаем id_type из parameters
    let params_ptr = &(*stack).parameters as *const _ as *const u8;
    let id_type = core::ptr::read(params_ptr as *const ULONG);
    
    const BUS_QUERY_DEVICE_ID: ULONG = 0;
    const BUS_QUERY_HARDWARE_IDS: ULONG = 1;
    const BUS_QUERY_INSTANCE_ID: ULONG = 3;
    
    match id_type {
        BUS_QUERY_DEVICE_ID => {
            // Device ID: "STORAGE\\Partition"
            let device_id = b"STORAGE\\Partition\0";
            let len = (device_id.len() - 1) * 2; // Unicode length without null
            
            let buffer = ExAllocatePoolWithTag(
                NON_PAGED_POOL,
                (device_id.len() * 2),
                DISK_POOL_TAG
            ) as *mut u16;
            
            if buffer.is_null() {
                (*irp).io_status.status = STATUS_INSUFFICIENT_RESOURCES;
                IoCompleteRequest(irp, IO_NO_INCREMENT);
                return STATUS_INSUFFICIENT_RESOURCES;
            }
            
            // Конвертируем в Unicode
            for (i, &b) in device_id.iter().enumerate() {
                *buffer.add(i) = b as u16;
            }
            
            (*irp).io_status.status = STATUS_SUCCESS;
            (*irp).io_status.information = buffer as ULONG_PTR;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            STATUS_SUCCESS
        }
        BUS_QUERY_HARDWARE_IDS => {
            // Hardware IDs: multi-string "STORAGE\\Partition\0\0"
            let hw_id = b"STORAGE\\Partition\0\0";
            
            let buffer = ExAllocatePoolWithTag(
                NON_PAGED_POOL,
                hw_id.len() * 2,
                DISK_POOL_TAG
            ) as *mut u16;
            
            if buffer.is_null() {
                (*irp).io_status.status = STATUS_INSUFFICIENT_RESOURCES;
                IoCompleteRequest(irp, IO_NO_INCREMENT);
                return STATUS_INSUFFICIENT_RESOURCES;
            }
            
            // Конвертируем в Unicode
            for (i, &b) in hw_id.iter().enumerate() {
                *buffer.add(i) = b as u16;
            }
            
            (*irp).io_status.status = STATUS_SUCCESS;
            (*irp).io_status.information = buffer as ULONG_PTR;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            STATUS_SUCCESS
        }
        BUS_QUERY_INSTANCE_ID => {
            // Instance ID: "Partition<N>"
            let partition_num = (*pdo_ext).partition_number;
            
            let mut instance_id = [0u8; 32];
            let mut idx = 0usize;
            
            // "Partition"
            for &b in b"Partition".iter() {
                instance_id[idx] = b;
                idx += 1;
            }
            
            // Номер партиции
            let num_str = format_u32(partition_num);
            for &b in num_str.iter() {
                if b == 0 { break; }
                instance_id[idx] = b;
                idx += 1;
            }
            instance_id[idx] = 0; // null terminator
            idx += 1;
            
            let buffer = ExAllocatePoolWithTag(
                NON_PAGED_POOL,
                idx * 2,
                DISK_POOL_TAG
            ) as *mut u16;
            
            if buffer.is_null() {
                (*irp).io_status.status = STATUS_INSUFFICIENT_RESOURCES;
                IoCompleteRequest(irp, IO_NO_INCREMENT);
                return STATUS_INSUFFICIENT_RESOURCES;
            }
            
            // Конвертируем в Unicode
            for (i, &b) in instance_id[..idx].iter().enumerate() {
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

/// IRP_MN_QUERY_DEVICE_RELATIONS для Partition PDO
unsafe fn disk_pdo_query_device_relations(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
    _pdo_ext: *mut DISK_PARTITION_EXTENSION,
) -> NTSTATUS {
    let stack = IoGetCurrentIrpStackLocation(irp);
    
    // Получаем relation_type
    let params_ptr = &(*stack).parameters as *const _ as *const u8;
    let relation_type = core::ptr::read(params_ptr as *const ULONG);
    
    const TARGET_DEVICE_RELATION: ULONG = 0;
    
    if relation_type != TARGET_DEVICE_RELATION {
        // Не поддерживаем другие типы для PDO
        let status = (*irp).io_status.status;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return status;
    }
    
    // TargetDeviceRelation - возвращаем сам PDO
    let relations = ExAllocatePoolWithTag(
        NON_PAGED_POOL,
        core::mem::size_of::<DEVICE_RELATIONS>(),
        DISK_POOL_TAG
    ) as *mut DEVICE_RELATIONS;
    
    if relations.is_null() {
        (*irp).io_status.status = STATUS_INSUFFICIENT_RESOURCES;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_INSUFFICIENT_RESOURCES;
    }
    
    (*relations).count = 1;
    *(*relations).objects.as_mut_ptr() = device_object;
    ObReferenceObject(device_object as PVOID);
    
    (*irp).io_status.status = STATUS_SUCCESS;
    (*irp).io_status.information = relations as ULONG_PTR;
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    STATUS_SUCCESS
}

/// IRP_MN_QUERY_CAPABILITIES для Partition PDO
unsafe fn disk_pdo_query_capabilities(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
    _pdo_ext: *mut DISK_PARTITION_EXTENSION,
) -> NTSTATUS {
    let stack = IoGetCurrentIrpStackLocation(irp);
    
    // Получаем DEVICE_CAPABILITIES из parameters
    let params_ptr = &(*stack).parameters as *const _ as *const u8;
    let capabilities = core::ptr::read(params_ptr as *const PVOID) as *mut DEVICE_CAPABILITIES;
    
    if capabilities.is_null() {
        (*irp).io_status.status = STATUS_INVALID_PARAMETER;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_INVALID_PARAMETER;
    }
    
    // Заполняем capabilities
    (*capabilities).removable = 0;
    (*capabilities).ejectable = 0;
    (*capabilities).lockable = 0;
    (*capabilities).dock_device = 0;
    (*capabilities).unique_id = 1;
    (*capabilities).silent_install = 1;
    (*capabilities).raw_device_ok = 1;
    (*capabilities).surprise_removal_ok = 0;
    
    (*irp).io_status.status = STATUS_SUCCESS;
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    STATUS_SUCCESS
}

/// IRP_MN_QUERY_DEVICE_TEXT для Partition PDO
unsafe fn disk_pdo_query_device_text(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
    pdo_ext: *mut DISK_PARTITION_EXTENSION,
) -> NTSTATUS {
    let stack = IoGetCurrentIrpStackLocation(irp);
    
    // Получаем device_text_type из parameters
    let params_ptr = &(*stack).parameters as *const _ as *const u8;
    let text_type = core::ptr::read(params_ptr as *const ULONG);
    
    const DEVICE_TEXT_DESCRIPTION: ULONG = 0;
    const DEVICE_TEXT_LOCATION_INFORMATION: ULONG = 1;
    
    match text_type {
        DEVICE_TEXT_DESCRIPTION => {
            // "Disk Partition"
            let text = b"Disk Partition\0";
            
            let buffer = ExAllocatePoolWithTag(
                NON_PAGED_POOL,
                text.len() * 2,
                DISK_POOL_TAG
            ) as *mut u16;
            
            if buffer.is_null() {
                (*irp).io_status.status = STATUS_INSUFFICIENT_RESOURCES;
                IoCompleteRequest(irp, IO_NO_INCREMENT);
                return STATUS_INSUFFICIENT_RESOURCES;
            }
            
            // Конвертируем в Unicode
            for (i, &b) in text.iter().enumerate() {
                *buffer.add(i) = b as u16;
            }
            
            (*irp).io_status.status = STATUS_SUCCESS;
            (*irp).io_status.information = buffer as ULONG_PTR;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            STATUS_SUCCESS
        }
        DEVICE_TEXT_LOCATION_INFORMATION => {
            // "Partition N on Disk M"
            let partition_num = (*pdo_ext).partition_number;
            
            // Получаем harddisk number из parent FDO
            let parent_fdo = (*pdo_ext).parent_fdo;
            let parent_ext = (*parent_fdo).device_extension as *const DISK_FDO_EXTENSION;
            let harddisk_num = (*parent_ext).harddisk_number;
            
            let mut location = [0u8; 64];
            let mut idx = 0usize;
            
            // "Partition "
            for &b in b"Partition ".iter() {
                location[idx] = b;
                idx += 1;
            }
            
            // Номер партиции
            let num_str = format_u32(partition_num);
            for &b in num_str.iter() {
                if b == 0 { break; }
                location[idx] = b;
                idx += 1;
            }
            
            // " on Disk "
            for &b in b" on Disk ".iter() {
                location[idx] = b;
                idx += 1;
            }
            
            // Номер диска
            let disk_str = format_u32(harddisk_num);
            for &b in disk_str.iter() {
                if b == 0 { break; }
                location[idx] = b;
                idx += 1;
            }
            location[idx] = 0;
            idx += 1;
            
            let buffer = ExAllocatePoolWithTag(
                NON_PAGED_POOL,
                idx * 2,
                DISK_POOL_TAG
            ) as *mut u16;
            
            if buffer.is_null() {
                (*irp).io_status.status = STATUS_INSUFFICIENT_RESOURCES;
                IoCompleteRequest(irp, IO_NO_INCREMENT);
                return STATUS_INSUFFICIENT_RESOURCES;
            }
            
            // Конвертируем в Unicode
            for (i, &b) in location[..idx].iter().enumerate() {
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

/// READ/WRITE для Partition PDO
unsafe fn disk_pdo_read_write(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    unsafe {
        let pdo_ext = (*device_object).device_extension as *mut DISK_PARTITION_EXTENSION;
        let stack = IoGetCurrentIrpStackLocation(irp);
        
        // Получаем parent FDO
        let parent_fdo = (*pdo_ext).parent_fdo;
        if parent_fdo.is_null() {
            (*irp).io_status.status = STATUS_NO_SUCH_DEVICE;
            (*irp).io_status.information = 0;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            return STATUS_NO_SUCH_DEVICE;
        }
        
        let parent_ext = (*parent_fdo).device_extension as *const DISK_FDO_EXTENSION;
        let sector_size = (*parent_ext).sector_size as u64;
        
        // Получаем параметры из IRP
        let params_ptr = &(*stack).parameters as *const _ as *const u8;
        let length = core::ptr::read(params_ptr as *const u32);
        let irp_offset = core::ptr::read(params_ptr.add(8) as *const i64);
        
        // Проверяем что offset неотрицательный
        if irp_offset < 0 {
            (*irp).io_status.status = STATUS_INVALID_PARAMETER;
            (*irp).io_status.information = 0;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            return STATUS_INVALID_PARAMETER;
        }
        
        // Bounds checking - не выходить за границы партиции
        let partition_size_bytes = (*pdo_ext).sector_count * sector_size;
        let request_end = irp_offset as u64 + length as u64;
        
        if request_end > partition_size_bytes {
            (*irp).io_status.status = STATUS_INVALID_PARAMETER;
            (*irp).io_status.information = 0;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            return STATUS_INVALID_PARAMETER;
        }
        
        // Offset translation: partition start + IRP offset
        let partition_start_bytes = (*pdo_ext).starting_lba * sector_size;
        let actual_offset = partition_start_bytes + irp_offset as u64;
        
        // Модифицируем parameters в IRP для parent FDO
        let params_ptr_mut = &mut (*stack).parameters as *mut _ as *mut u8;
        core::ptr::write(params_ptr_mut.add(8) as *mut i64, actual_offset as i64);
        
        // Forward к parent FDO
        IoSkipCurrentIrpStackLocation(irp);
        IoCallDriver(parent_fdo, irp)
    }
}

/// DEVICE_CONTROL для Partition PDO
unsafe fn disk_pdo_device_control(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    unsafe {
        let pdo_ext = (*device_object).device_extension as *mut DISK_PARTITION_EXTENSION;
        let stack = IoGetCurrentIrpStackLocation(irp);
        
        // Получаем IOCTL code
        let params_ptr = &(*stack).parameters as *const _ as *const u8;
        let io_control_code = core::ptr::read(params_ptr.add(8) as *const u32);
        
        // Определяем buffer method
        let method = io_control_code & 3;
        
        match io_control_code {
            IOCTL_DISK_GET_DRIVE_GEOMETRY => {
                disk_pdo_ioctl_get_geometry(device_object, irp, pdo_ext)
            }
            IOCTL_DISK_GET_PARTITION_INFO => {
                disk_pdo_ioctl_get_partition_info(device_object, irp, pdo_ext)
            }
            IOCTL_DISK_GET_LENGTH_INFO => {
                disk_pdo_ioctl_get_length_info(device_object, irp, pdo_ext)
            }
            _ => {
                // Неизвестный IOCTL
                (*irp).io_status.status = STATUS_INVALID_DEVICE_REQUEST;
                (*irp).io_status.information = 0;
                IoCompleteRequest(irp, IO_NO_INCREMENT);
                STATUS_INVALID_DEVICE_REQUEST
            }
        }
    }
}

/// IOCTL_DISK_GET_DRIVE_GEOMETRY для Partition PDO
unsafe fn disk_pdo_ioctl_get_geometry(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
    pdo_ext: *mut DISK_PARTITION_EXTENSION,
) -> NTSTATUS {
    unsafe {
        // Получаем parent FDO для sector_size
        let parent_fdo = (*pdo_ext).parent_fdo;
        if parent_fdo.is_null() {
            (*irp).io_status.status = STATUS_NO_SUCH_DEVICE;
            (*irp).io_status.information = 0;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            return STATUS_NO_SUCH_DEVICE;
        }
        
        let parent_ext = (*parent_fdo).device_extension as *const DISK_FDO_EXTENSION;
        let sector_size = (*parent_ext).sector_size;
        let sector_count = (*pdo_ext).sector_count;
        
        // Получаем output buffer из IRP (METHOD_BUFFERED)
        let buffer = (*irp).associated_irp.system_buffer;
        if buffer.is_null() {
            (*irp).io_status.status = STATUS_INVALID_PARAMETER;
            (*irp).io_status.information = 0;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            return STATUS_INVALID_PARAMETER;
        }
        
        // Заполняем DISK_GEOMETRY
        let geometry = buffer as *mut DISK_GEOMETRY;
        (*geometry).cylinders = sector_count as i64;
        (*geometry).media_type = 0; // FixedMedia
        (*geometry).tracks_per_cylinder = 1;
        (*geometry).sectors_per_track = 1;
        (*geometry).bytes_per_sector = sector_size;
        
        (*irp).io_status.status = STATUS_SUCCESS;
        (*irp).io_status.information = core::mem::size_of::<DISK_GEOMETRY>();
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        STATUS_SUCCESS
    }
}

/// IOCTL_DISK_GET_PARTITION_INFO для Partition PDO
unsafe fn disk_pdo_ioctl_get_partition_info(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
    pdo_ext: *mut DISK_PARTITION_EXTENSION,
) -> NTSTATUS {
    unsafe {
        // Получаем parent FDO для sector_size
        let parent_fdo = (*pdo_ext).parent_fdo;
        if parent_fdo.is_null() {
            (*irp).io_status.status = STATUS_NO_SUCH_DEVICE;
            (*irp).io_status.information = 0;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            return STATUS_NO_SUCH_DEVICE;
        }
        
        let parent_ext = (*parent_fdo).device_extension as *const DISK_FDO_EXTENSION;
        let sector_size = (*parent_ext).sector_size as u64;
        
        // Получаем output buffer
        let buffer = (*irp).associated_irp.system_buffer;
        if buffer.is_null() {
            (*irp).io_status.status = STATUS_INVALID_PARAMETER;
            (*irp).io_status.information = 0;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            return STATUS_INVALID_PARAMETER;
        }
        
        // Заполняем PARTITION_INFORMATION
        let partition_info = buffer as *mut PARTITION_INFORMATION;
        (*partition_info).starting_offset = ((*pdo_ext).starting_lba * sector_size) as i64;
        (*partition_info).partition_length = ((*pdo_ext).sector_count * sector_size) as i64;
        (*partition_info).hidden_sectors = 0;
        (*partition_info).partition_number = (*pdo_ext).partition_number;
        (*partition_info).partition_type = (*pdo_ext).partition_type;
        (*partition_info).bootable = (*pdo_ext).bootable;
        (*partition_info).recognized_partition = 1;
        (*partition_info).rewrite_partition = 0;
        
        (*irp).io_status.status = STATUS_SUCCESS;
        (*irp).io_status.information = core::mem::size_of::<PARTITION_INFORMATION>();
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        STATUS_SUCCESS
    }
}

/// IOCTL_DISK_GET_LENGTH_INFO для Partition PDO
unsafe fn disk_pdo_ioctl_get_length_info(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
    pdo_ext: *mut DISK_PARTITION_EXTENSION,
) -> NTSTATUS {
    unsafe {
        // Получаем parent FDO для sector_size
        let parent_fdo = (*pdo_ext).parent_fdo;
        if parent_fdo.is_null() {
            (*irp).io_status.status = STATUS_NO_SUCH_DEVICE;
            (*irp).io_status.information = 0;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            return STATUS_NO_SUCH_DEVICE;
        }
        
        let parent_ext = (*parent_fdo).device_extension as *const DISK_FDO_EXTENSION;
        let sector_size = (*parent_ext).sector_size as u64;
        
        // Получаем output buffer
        let buffer = (*irp).associated_irp.system_buffer;
        if buffer.is_null() {
            (*irp).io_status.status = STATUS_INVALID_PARAMETER;
            (*irp).io_status.information = 0;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            return STATUS_INVALID_PARAMETER;
        }
        
        // Заполняем GET_LENGTH_INFORMATION
        let length_info = buffer as *mut GET_LENGTH_INFORMATION;
        (*length_info).length = ((*pdo_ext).sector_count * sector_size) as i64;
        
        (*irp).io_status.status = STATUS_SUCCESS;
        (*irp).io_status.information = core::mem::size_of::<GET_LENGTH_INFORMATION>();
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        STATUS_SUCCESS
    }
}

#[repr(C)]
struct DEVICE_CAPABILITIES {
    size: USHORT,
    version: USHORT,
    device_d1: u32,
    device_d2: u32,
    lock_supported: u32,
    eject_supported: u32,
    removable: u32,
    dock_device: u32,
    unique_id: u32,
    silent_install: u32,
    raw_device_ok: u32,
    surprise_removal_ok: u32,
    // ... остальные поля
    hardware_disabled: u32,
    non_dynamic: u32,
    _reserved: [u32; 13],
    address: ULONG,
    ui_number: ULONG,
    device_state: [u32; 7],
    system_wake: u32,
    device_wake: u32,
    d1_latency: ULONG,
    d2_latency: ULONG,
    d3_latency: ULONG,
    ejectable: u32,
    lockable: u32,
}

#[repr(C)]
struct DEVICE_RELATIONS {
    count: ULONG,
    objects: [PDEVICE_OBJECT; 1],
}

// =============================================================================
// Panic Handler
// =============================================================================

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

// =============================================================================
// Partition PDO Creation
// =============================================================================

/// Создаёт PDO для партиции
///
/// # Arguments
/// * `driver_object` - драйвер
/// * `parent_fdo` - parent FDO (disk)
/// * `harddisk_number` - номер harddisk
/// * `partition` - информация о партиции
///
/// # Returns
/// Указатель на PDO или null
unsafe fn create_partition_pdo(
    driver_object: PDRIVER_OBJECT,
    parent_fdo: PDEVICE_OBJECT,
    harddisk_number: u32,
    partition: &partition::PartitionInfo,
) -> PDEVICE_OBJECT {
    // Создаём имя \Device\Harddisk<N>\Partition<M>
    let mut device_name_str = [0u8; 64];
    let mut idx = 0usize;
    
    // "\Device\Harddisk"
    for &b in b"\\Device\\Harddisk".iter() {
        device_name_str[idx] = b;
        idx += 1;
    }
    
    // Номер harddisk
    let num_str = format_u32(harddisk_number);
    for &b in num_str.iter() {
        if b == 0 { break; }
        device_name_str[idx] = b;
        idx += 1;
    }
    
    // "\Partition"
    for &b in b"\\Partition".iter() {
        device_name_str[idx] = b;
        idx += 1;
    }
    
    // Номер партиции
    let part_num_str = format_u32(partition.partition_number);
    for &b in part_num_str.iter() {
        if b == 0 { break; }
        device_name_str[idx] = b;
        idx += 1;
    }
    
    // Выделяем постоянный буфер для имени (из NonPagedPool)
    let name_buffer = ExAllocatePoolWithTag(
        NON_PAGED_POOL, 
        (idx * 2) + 2, // Unicode + null terminator
        DISK_POOL_TAG
    ) as *mut u16;
    
    if name_buffer.is_null() {
        return ptr::null_mut();
    }
    
    // Конвертируем в Unicode
    for (i, &b) in device_name_str[..idx].iter().enumerate() {
        *name_buffer.add(i) = b as u16;
    }
    *name_buffer.add(idx) = 0; // null terminator
    
    let device_name = UNICODE_STRING {
        length: (idx * 2) as USHORT,
        maximum_length: ((idx * 2) + 2) as USHORT,
        buffer: name_buffer,
    };
    
    // Создаём PDO с именем
    let mut pdo: PDEVICE_OBJECT = ptr::null_mut();
    let ext_size = core::mem::size_of::<DISK_PARTITION_EXTENSION>() as ULONG;
    
    let status = IoCreateDevice(
        driver_object,
        ext_size,
        &device_name,
        FILE_DEVICE_DISK,
        0,
        0,
        &mut pdo,
    );
    
    if status < 0 || pdo.is_null() {
        ExFreePoolWithTag(name_buffer as PVOID, DISK_POOL_TAG);
        return ptr::null_mut();
    }
    
    // Инициализируем PDO extension
    let pdo_ext = (*pdo).device_extension as *mut DISK_PARTITION_EXTENSION;
    ptr::write_bytes(pdo_ext, 0, core::mem::size_of::<DISK_PARTITION_EXTENSION>());
    
    (*pdo_ext).common.is_fdo = 0;
    (*pdo_ext).common.self_device = pdo;
    (*pdo_ext).parent_fdo = parent_fdo;
    (*pdo_ext).partition_number = partition.partition_number;
    (*pdo_ext).starting_lba = partition.starting_lba;
    (*pdo_ext).sector_count = partition.sector_count;
    (*pdo_ext).partition_type = partition.partition_type;
    (*pdo_ext).bootable = if partition.bootable { 1 } else { 0 };
    
    // NOTE: NT 6.1 Architecture changes:
    // - VPB allocation and filesystem mounting are NOT done here
    // - Filesystem mounting happens when the volume is first accessed
    // - The I/O Manager calls file system recognizers automatically
    // - mountmgr.sys manages drive letter assignments
    
    // Create VPB for partition device (required for filesystem access later)
    // Skip Partition0 (raw disk doesn't need VPB)
    if partition.partition_number > 0 {
        let vpb = IoAllocateVpb(pdo);
        if !vpb.is_null() {
            (*pdo).vpb = vpb;
            disk_print("[DISK] VPB allocated for Partition");
            disk_print_dec(partition.partition_number as u64);
            disk_print("\n");
        }
        
        // NOTE: IoMountVolume is NOT called here anymore
        // Filesystem mounting is triggered by I/O Manager when volume is accessed
    }
    
    (*pdo).flags &= !DO_DEVICE_INITIALIZING;
    
    pdo
}

// =============================================================================
// DOS Device Links - REMOVED (Phase 4.1)
// =============================================================================
// 
// NOTE: DOS device links (drive letters like C:, D:) are NOT created by disk.sys
// in NT 6.1 architecture. This is the responsibility of mountmgr.sys.
// 
// The removed functions were:
// - create_dos_device_link() - moved to mountmgr.sys (Phase 6)
// - NEXT_DRIVE_LETTER static counter
//
// Mount manager receives PnP notifications about new volumes and assigns
// drive letters based on:
// - Persistent registry database (HKLM\SYSTEM\MountedDevices)
// - Auto-assignment rules
// - User preferences
//

// =============================================================================
// Helper Functions
// =============================================================================

/// Форматирует u32 в ASCII string
fn format_u32(mut n: u32) -> [u8; 10] {
    let mut buf = [0u8; 10];
    let mut idx = 0;
    
    if n == 0 {
        buf[0] = b'0';
        return buf;
    }
    
    // Записываем цифры в обратном порядке
    let mut temp = [0u8; 10];
    let mut temp_idx = 0;
    while n > 0 {
        temp[temp_idx] = b'0' + (n % 10) as u8;
        n /= 10;
        temp_idx += 1;
    }
    
    // Переворачиваем
    for i in 0..temp_idx {
        buf[idx] = temp[temp_idx - 1 - i];
        idx += 1;
    }
    
    buf
}

/// Выводит информацию о диске
unsafe fn disk_print_info(fdo_ext: *const DISK_FDO_EXTENSION) {
    disk_print("[DISK] Harddisk");
    disk_print_dec((*fdo_ext).harddisk_number as u64);
    disk_print(": ");
    
    // Размер в MB
    let size_mb = ((*fdo_ext).sector_count * (*fdo_ext).sector_size as u64) / (1024 * 1024);
    disk_print_dec(size_mb);
    disk_print(" MB");
    
    disk_print(" (");
    disk_print_dec((*fdo_ext).sector_count);
    disk_print(" sectors x ");
    disk_print_dec((*fdo_ext).sector_size as u64);
    disk_print(" bytes)\n");
}

/// Выводит строку через DbgPrint (только при storage-trace feature)
#[cfg(feature = "storage-trace")]
pub(crate) unsafe fn disk_print(s: &str) {
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
pub(crate) unsafe fn disk_print(_s: &str) {}

/// Выводит число в decimal (только при storage-trace feature)
#[cfg(feature = "storage-trace")]
pub(crate) unsafe fn disk_print_dec(value: u64) {
    let mut buf = [0u8; 32];
    let mut n = value;
    let mut i = 0;
    
    if n == 0 {
        buf[0] = b'0';
        i = 1;
    } else {
        while n > 0 {
            buf[i] = b'0' + (n % 10) as u8;
            n /= 10;
            i += 1;
        }
        buf[..i].reverse();
    }
    buf[i] = 0;
    
    DbgPrint(buf.as_ptr());
}

#[cfg(not(feature = "storage-trace"))]
#[inline(always)]
pub(crate) unsafe fn disk_print_dec(_value: u64) {}

/// Выводит число в hex (только при storage-trace feature)
#[cfg(feature = "storage-trace")]
pub(crate) unsafe fn disk_print_hex(value: u64) {
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
        let nibble = ((value >> (60 - i * 4)) & 0xF) as usize;
        if nibble != 0 || start > 0 {
            buf[start] = HEX_CHARS[nibble];
            start += 1;
        }
    }
    buf[start] = 0;
    
    DbgPrint(buf.as_ptr());
}

#[cfg(not(feature = "storage-trace"))]
#[inline(always)]
pub(crate) unsafe fn disk_print_hex(_value: u64) {}

