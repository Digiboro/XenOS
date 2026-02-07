//! FAT32 File System Driver
//!
//! Базовая реализация FAT32 для чтения файлов.
//!
//! Источники:
//! - FAT32 File System Specification (Microsoft)
//! - ReactOS: drivers/filesystems/fastfat/

#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]
#![feature(offset_of_enum)]

mod types;
mod bpb;
mod vcb;
mod fat;
mod fcb;
mod dir;
mod query_info;
mod dir_control;
mod volume_info;
mod time;
mod create;
mod delete;
mod set_info;
mod update;

use types::*;
use bpb::{FAT32_BPB, FAT32DirEntry};
use vcb::FAT32_VCB;
use fat::{fat_read_cluster_chain, fat_read_next_cluster, fat_allocate_cluster, fat_extend_chain, fat_free_chain, fat_write_cluster_chain, fat_expand_file_size};
use fcb::{FAT32_FCB, FAT32_CCB, FCB_FLAGS_MODIFIED, FCB_FLAGS_DELETE_ON_CLOSE};
use dir::{fat_find_file, fat_find_file_by_path, fat_create_root_fcb, convert_to_83_name};
use query_info::fat_query_information;
use dir_control::fat_directory_control;
use volume_info::fat_query_volume_information;
use time::{dos_datetime_to_filetime, filetime_to_dos_datetime, get_current_filetime};
use create::fat_create_file;
use delete::fat_delete_file;
use set_info::fat_set_information;
pub use update::fat_update_directory_entry;
use core::ptr;

// =============================================================================
// Driver Entry
// =============================================================================

/// DriverEntry - точка входа FAT32 драйвера
#[unsafe(no_mangle)]
pub extern "win64" fn DriverEntry(
    driver_object: PDRIVER_OBJECT,
    _registry_path: *const UNICODE_STRING,
) -> NTSTATUS {
    unsafe {
        fat_print("[FAT32] DriverEntry called\n");
        fat_print("[FAT32] driver_object=0x");
        fat_print_hex(driver_object as u64);
        fat_print("\n");
        
        if driver_object.is_null() {
            fat_print("[FAT32] ERROR: driver_object is NULL\n");
            return STATUS_INVALID_PARAMETER;
        }
        
        fat_print("[FAT32] Creating control device...\n");
        
        // Создаём control device для FS
        let mut fs_device: PDEVICE_OBJECT = core::ptr::null_mut();
        let status = IoCreateDevice(
            driver_object,
            0, // No extension for control device
            core::ptr::null(), // Unnamed
            FILE_DEVICE_DISK_FILE_SYSTEM,
            0,
            0,
            &mut fs_device,
        );
        
        if status < 0 {
            fat_print("[FAT32] ERROR: Failed to create control device, status=0x");
            fat_print_hex(status as u32 as u64);
            fat_print("\n");
            return status;
        }
        
        fat_print("[FAT32] Control device created: 0x");
        fat_print_hex(fs_device as u64);
        fat_print("\n");
        
        // Сохраняем fs_device в глобальной переменной
        GLOBAL_FS_DEVICE = fs_device;
        
        fat_print("[FAT32] Setting dispatch routines...\n");
        
        // Устанавливаем dispatch routines
        let driver = driver_object as *mut DriverObjectStruct;
        (*driver).major_function[IRP_MJ_FILE_SYSTEM_CONTROL as usize] = Some(FatFsControl);
        (*driver).major_function[IRP_MJ_CREATE as usize] = Some(FatCreate);
        (*driver).major_function[IRP_MJ_CLOSE as usize] = Some(FatClose);
        (*driver).major_function[IRP_MJ_CLEANUP as usize] = Some(FatCleanup);
        (*driver).major_function[IRP_MJ_READ as usize] = Some(FatRead);
        (*driver).major_function[IRP_MJ_WRITE as usize] = Some(FatWrite);
        (*driver).major_function[IRP_MJ_QUERY_INFORMATION as usize] = Some(FatQueryInformation);
        (*driver).major_function[IRP_MJ_SET_INFORMATION as usize] = Some(FatSetInformation);
        (*driver).major_function[IRP_MJ_FLUSH_BUFFERS as usize] = Some(FatFlush);
        (*driver).major_function[IRP_MJ_DIRECTORY_CONTROL as usize] = Some(FatDirectoryControl);
        (*driver).major_function[IRP_MJ_QUERY_VOLUME_INFORMATION as usize] = Some(FatQueryVolumeInformation);
        
        fat_print("[FAT32] Registering file system...\n");
        
        // Регистрируем FS
        IoRegisterFileSystem(fs_device);
        
        fat_print("[FAT32] Driver loaded successfully!\n");
        
        STATUS_SUCCESS
    }
}

// =============================================================================
// Global State
// =============================================================================

static mut GLOBAL_FS_DEVICE: PDEVICE_OBJECT = core::ptr::null_mut();

#[repr(C)]
struct DRIVER_OBJECT {
    _padding: [u8; 0x68],
    major_function: [Option<unsafe extern "win64" fn(PDEVICE_OBJECT, PIRP) -> NTSTATUS>; 28],
}

// =============================================================================
// Dispatch Routines
// =============================================================================

/// File System Control dispatcher
unsafe extern "win64" fn FatFsControl(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    unsafe {
        let stack = IoGetCurrentIrpStackLocation(irp) as *mut IoStackLocation;
        let minor = (*stack).minor_function;
        
        let irp_struct = irp as *mut IrpStruct;
        
        match minor {
            IRP_MN_MOUNT_VOLUME => fat_mount_volume(device_object, irp),
            IRP_MN_VERIFY_VOLUME => fat_verify_volume(device_object, irp),
            _ => {
                (*irp_struct).io_status.status = STATUS_INVALID_DEVICE_REQUEST;
                (*irp_struct).io_status.information = 0;
                IoCompleteRequest(irp, types::IO_NO_INCREMENT);
                STATUS_INVALID_DEVICE_REQUEST
            }
        }
    }
}

/// CREATE dispatcher
unsafe extern "win64" fn FatCreate(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    unsafe {
        fat_print("[FAT32] IRP_MJ_CREATE\n");
        
        let stack = IoGetCurrentIrpStackLocation(irp) as *mut IoStackLocation;
        let file_object = (*stack).file_object;
        let irp_struct = irp as *mut IrpStruct;
        
        // Получаем VCB из device extension
        let dev_obj = device_object as *mut DEVICE_OBJECT;
        let vcb = (*dev_obj).device_extension as *mut FAT32_VCB;
        
        if vcb.is_null() || (*vcb).node_type != FAT32_VCB::NODE_TYPE {
            (*irp_struct).io_status.status = STATUS_INVALID_DEVICE_REQUEST;
            (*irp_struct).io_status.information = 0;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            return STATUS_INVALID_DEVICE_REQUEST;
        }
        
        // Получаем имя файла из file object
        let file_obj = file_object as *mut FILE_OBJECT;
        if file_obj.is_null() {
            (*irp_struct).io_status.status = STATUS_INVALID_PARAMETER;
            (*irp_struct).io_status.information = 0;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            return STATUS_INVALID_PARAMETER;
        }
        
        // Получаем CREATE disposition из parameters
        // Получаем CREATE disposition из IO_STACK_LOCATION.parameters.create
        // Структура: security_context (8), options (4), file_attributes (2), share_access (2), ea_length (4)
        let params_ptr = &(*stack).parameters as *const _ as *const u8;
        let options = core::ptr::read(params_ptr.add(8) as *const u32);
        
        // Disposition находится в младших битах options (FILE_OPEN, FILE_CREATE, etc.)
        let create_disposition = (options >> 24) & 0xFF; // Disposition в верхнем байте
        
        let file_name_us = &(*file_obj).file_name;
        
        // Если имя пустое - открываем root directory
        let fcb = if file_name_us.length == 0 {
            fat_print("[FAT32] Opening root directory\n");
            fat_create_root_fcb(vcb)
        } else {
            fat_print("[FAT32] Opening file: ");
            fat_print_unicode_string(file_name_us);
            fat_print("\n");
            
            // Получаем Unicode путь
            let wide_len = (file_name_us.length / 2) as usize;
            if wide_len == 0 || wide_len > 256 {
                fat_print("[FAT32] Invalid path length\n");
                (*irp_struct).io_status.status = STATUS_OBJECT_NAME_INVALID;
                (*irp_struct).io_status.information = 0;
                IoCompleteRequest(irp, IO_NO_INCREMENT);
                return STATUS_OBJECT_NAME_INVALID;
            }
            
            // Копируем path в локальный буфер
            let mut path_buffer = [0u16; 256];
            for i in 0..wide_len {
                path_buffer[i] = *file_name_us.buffer.add(i);
            }
            
            // Ищем файл по полному пути
            let mut fcb_found: *mut FAT32_FCB = core::ptr::null_mut();
            let find_status = fat_find_file_by_path(vcb, &path_buffer, wide_len, &mut fcb_found);
            
            match (create_disposition, find_status) {
                // FILE_OPEN (0) - открыть существующий
                (0, STATUS_SUCCESS) => fcb_found,
                (0, _) => {
                    // Файл не найден и не нужно создавать
                    fat_print("[FAT32] File not found\n");
                    (*irp_struct).io_status.status = STATUS_OBJECT_NAME_NOT_FOUND;
                    (*irp_struct).io_status.information = 0;
                    IoCompleteRequest(irp, IO_NO_INCREMENT);
                    return STATUS_OBJECT_NAME_NOT_FOUND;
                }
                
                // FILE_CREATE (1) - создать новый
                (1, STATUS_OBJECT_NAME_NOT_FOUND) => {
                    fat_print("[FAT32] Creating new file\n");
                    
                    // Парсим путь для извлечения имени и parent directory
                    // Ищем последний '\' в пути для разделения на parent path и file name
                    let mut parent_end = 0usize;
                    for i in (0..wide_len).rev() {
                        if path_buffer[i] == b'\\' as u16 {
                            parent_end = i;
                            break;
                        }
                    }
                    
                    // Если нет '\' - создаём в root
                    let parent_cluster = if parent_end == 0 {
                        (*vcb).root_directory_cluster
                    } else {
                        // Ищем parent directory
                        let mut parent_fcb: *mut FAT32_FCB = core::ptr::null_mut();
                        let status = fat_find_file_by_path(vcb, &path_buffer, parent_end, &mut parent_fcb);
                        if status != STATUS_SUCCESS || parent_fcb.is_null() {
                            (*irp_struct).io_status.status = STATUS_OBJECT_PATH_NOT_FOUND;
                            (*irp_struct).io_status.information = 0;
                            IoCompleteRequest(irp, IO_NO_INCREMENT);
                            return STATUS_OBJECT_PATH_NOT_FOUND;
                        }
                        
                        let cluster = (*parent_fcb).first_cluster;
                        ExFreePoolWithTag(parent_fcb as PVOID, FAT32_POOL_TAG);
                        cluster
                    };
                    
                    // Конвертируем имя файла в 8.3
                    let file_name_start = parent_end + 1;
                    let mut name_83 = [b' '; 11];
                    
                    // Простая конвертация (без extension parsing)
                    let mut idx = 0;
                    for i in file_name_start..wide_len {
                        if idx >= 11 {
                            break;
                        }
                        let ch = path_buffer[i];
                        if ch < 128 && ch != 0 {
                            name_83[idx] = ch as u8;
                            idx += 1;
                        }
                    }
                    
                    // Создаём файл
                    let mut new_fcb: *mut FAT32_FCB = core::ptr::null_mut();
                    let create_status = fat_create_file(vcb, parent_cluster, &name_83, false, &mut new_fcb);
                    
                    if create_status != STATUS_SUCCESS {
                        (*irp_struct).io_status.status = create_status;
                        (*irp_struct).io_status.information = 0;
                        IoCompleteRequest(irp, IO_NO_INCREMENT);
                        return create_status;
                    }
                    
                    new_fcb
                }
                (1, STATUS_SUCCESS) => {
                    // Файл уже существует
                    ExFreePoolWithTag(fcb_found as PVOID, FAT32_POOL_TAG);
                    (*irp_struct).io_status.status = STATUS_OBJECT_NAME_COLLISION;
                    (*irp_struct).io_status.information = 0;
                    IoCompleteRequest(irp, IO_NO_INCREMENT);
                    return STATUS_OBJECT_NAME_COLLISION;
                }
                
                // Для остальных dispositions пока открываем если существует
                _ => {
                    if find_status == STATUS_SUCCESS {
                        fcb_found
                    } else {
                        (*irp_struct).io_status.status = STATUS_OBJECT_NAME_NOT_FOUND;
                        (*irp_struct).io_status.information = 0;
                        IoCompleteRequest(irp, IO_NO_INCREMENT);
                        return STATUS_OBJECT_NAME_NOT_FOUND;
                    }
                }
            }
        };
        
        if fcb.is_null() {
            (*irp_struct).io_status.status = STATUS_INSUFFICIENT_RESOURCES;
            (*irp_struct).io_status.information = 0;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            return STATUS_INSUFFICIENT_RESOURCES;
        }
        
        // Создаём CCB для этого handle
        let ccb = ExAllocatePoolWithTag(
            NON_PAGED_POOL,
            core::mem::size_of::<FAT32_CCB>(),
            FAT32_POOL_TAG,
        ) as *mut FAT32_CCB;
        
        if ccb.is_null() {
            ExFreePoolWithTag(fcb as PVOID, FAT32_POOL_TAG);
            (*irp_struct).io_status.status = STATUS_INSUFFICIENT_RESOURCES;
            (*irp_struct).io_status.information = 0;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            return STATUS_INSUFFICIENT_RESOURCES;
        }
        
        core::ptr::write(ccb, FAT32_CCB::new(fcb));
        
        // Сохраняем CCB в FsContext
        (*file_obj).fs_context = ccb as PVOID;
        
        fat_print("[FAT32] File opened successfully, cluster=");
        fat_print_dec((*fcb).first_cluster as u64);
        fat_print(", size=");
        fat_print_dec((*fcb).file_size as u64);
        fat_print("\n");
        
        (*irp_struct).io_status.status = STATUS_SUCCESS;
        (*irp_struct).io_status.information = 0;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        STATUS_SUCCESS
    }
}

/// CLOSE dispatcher
unsafe extern "win64" fn FatClose(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    unsafe {
        let irp_struct = irp as *mut IrpStruct;
        (*irp_struct).io_status.status = STATUS_SUCCESS;
        (*irp_struct).io_status.information = 0;
        IoCompleteRequest(irp, types::IO_NO_INCREMENT);
        STATUS_SUCCESS
    }
}

/// CLEANUP dispatcher
unsafe extern "win64" fn FatCleanup(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    unsafe {
        let stack = IoGetCurrentIrpStackLocation(irp) as *mut IoStackLocation;
        let file_object = (*stack).file_object;
        let irp_struct = irp as *mut IrpStruct;
        
        // Получаем CCB из file object
        let file_obj = file_object as *mut FILE_OBJECT;
        if !file_obj.is_null() && !(*file_obj).fs_context.is_null() {
            let ccb = (*file_obj).fs_context as *mut FAT32_CCB;
            
            if (*ccb).node_type == FAT32_CCB::NODE_TYPE {
                let fcb = (*ccb).fcb;
                
                // Проверяем флаг DELETE_ON_CLOSE
                if !fcb.is_null() && ((*fcb).flags & FCB_FLAGS_DELETE_ON_CLOSE) != 0 {
                    // Удаляем файл полностью
                    if (*fcb).parent_dir_cluster >= 2 {
                        let mut name_83 = [0u8; 11];
                        let file_name_slice = &(*fcb).file_name;
                        name_83[..11].copy_from_slice(&file_name_slice[..11]);
                        let _ = fat_delete_file((*fcb).vcb, (*fcb).parent_dir_cluster, &name_83, fcb);
                    }
                }
                
                // Уменьшаем reference count на FCB
                if !fcb.is_null() && (*fcb).reference_count > 0 {
                    (*fcb).reference_count -= 1;
                    
                    // Если это последний reference - освобождаем FCB
                    if (*fcb).reference_count == 0 {
                        ExFreePoolWithTag(fcb as PVOID, FAT32_POOL_TAG);
                    }
                }
                
                // Освобождаем CCB
                ExFreePoolWithTag(ccb as PVOID, FAT32_POOL_TAG);
                (*file_obj).fs_context = core::ptr::null_mut();
            }
        }
        
        (*irp_struct).io_status.status = STATUS_SUCCESS;
        (*irp_struct).io_status.information = 0;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        STATUS_SUCCESS
    }
}

/// READ dispatcher
unsafe extern "win64" fn FatRead(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    unsafe {
        fat_print("[FAT32] IRP_MJ_READ\n");
        
        let stack = IoGetCurrentIrpStackLocation(irp) as *mut IoStackLocation;
        let file_object = (*stack).file_object;
        let irp_struct = irp as *mut IrpStruct;
        
        // Получаем CCB из file object
        let file_obj = file_object as *mut FILE_OBJECT;
        if file_obj.is_null() || (*file_obj).fs_context.is_null() {
            (*irp_struct).io_status.status = STATUS_INVALID_PARAMETER;
            (*irp_struct).io_status.information = 0;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            return STATUS_INVALID_PARAMETER;
        }
        
        let ccb = (*file_obj).fs_context as *mut FAT32_CCB;
        if (*ccb).node_type != FAT32_CCB::NODE_TYPE {
            (*irp_struct).io_status.status = STATUS_INVALID_PARAMETER;
            (*irp_struct).io_status.information = 0;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            return STATUS_INVALID_PARAMETER;
        }
        
        let fcb = (*ccb).fcb;
        let vcb = (*fcb).vcb;
        
        // Получаем параметры read из IRP
        let length = (*stack).parameters.read.length;
        let byte_offset = (*stack).parameters.read.byte_offset as u64;
        
        // Получаем buffer из IRP
        let buffer = (*irp_struct).associated_irp as *mut u8;
        
        if buffer.is_null() {
            (*irp_struct).io_status.status = STATUS_INVALID_PARAMETER;
            (*irp_struct).io_status.information = 0;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            return STATUS_INVALID_PARAMETER;
        }
        
        fat_print("[FAT32] Read: offset=");
        fat_print_dec(byte_offset);
        fat_print(", length=");
        fat_print_dec(length as u64);
        fat_print("\n");
        
        // Проверяем границы
        if byte_offset >= (*fcb).file_size as u64 {
            // EOF
            (*irp_struct).io_status.status = STATUS_END_OF_FILE;
            (*irp_struct).io_status.information = 0;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            return STATUS_END_OF_FILE;
        }
        
        // Корректируем length если выходит за границы файла
        let remaining = (*fcb).file_size as u64 - byte_offset;
        let actual_length = (length as u64).min(remaining) as u32;
        
        // Читаем данные через FAT chain
        let bytes_read = fat_read_cluster_chain(
            vcb,
            (*fcb).first_cluster,
            byte_offset,
            buffer,
            actual_length,
        );
        
        match bytes_read {
            Ok(read) => {
                fat_print("[FAT32] Read ");
                fat_print_dec(read as u64);
                fat_print(" bytes\n");
                
                (*irp_struct).io_status.status = STATUS_SUCCESS;
                (*irp_struct).io_status.information = read as usize;
                IoCompleteRequest(irp, IO_NO_INCREMENT);
                STATUS_SUCCESS
            }
            Err(status) => {
                fat_print("[FAT32] Read failed\n");
                (*irp_struct).io_status.status = status;
                (*irp_struct).io_status.information = 0;
                IoCompleteRequest(irp, IO_NO_INCREMENT);
                status
            }
        }
    }
}

/// WRITE dispatcher
unsafe extern "win64" fn FatWrite(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    unsafe {
        fat_print("[FAT32] IRP_MJ_WRITE\n");
        
        let stack = IoGetCurrentIrpStackLocation(irp) as *mut IoStackLocation;
        let file_object = (*stack).file_object;
        let irp_struct = irp as *mut IrpStruct;
        
        // Получаем CCB из file object
        let file_obj = file_object as *mut FILE_OBJECT;
        if file_obj.is_null() || (*file_obj).fs_context.is_null() {
            (*irp_struct).io_status.status = STATUS_INVALID_PARAMETER;
            (*irp_struct).io_status.information = 0;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            return STATUS_INVALID_PARAMETER;
        }
        
        let ccb = (*file_obj).fs_context as *mut FAT32_CCB;
        if (*ccb).node_type != FAT32_CCB::NODE_TYPE {
            (*irp_struct).io_status.status = STATUS_INVALID_PARAMETER;
            (*irp_struct).io_status.information = 0;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            return STATUS_INVALID_PARAMETER;
        }
        
        let fcb = (*ccb).fcb;
        let vcb = (*fcb).vcb;
        
        // Нельзя писать в директории
        if (*fcb).is_directory() {
            (*irp_struct).io_status.status = STATUS_INVALID_PARAMETER;
            (*irp_struct).io_status.information = 0;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            return STATUS_INVALID_PARAMETER;
        }
        
        // Получаем параметры write из IRP
        let length = (*stack).parameters.write.length;
        let byte_offset = (*stack).parameters.write.byte_offset as u64;
        let buffer = (*irp_struct).associated_irp as *mut u8;
        
        if buffer.is_null() || length == 0 {
            (*irp_struct).io_status.status = STATUS_SUCCESS;
            (*irp_struct).io_status.information = 0;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            return STATUS_SUCCESS;
        }
        
        fat_print("[FAT32] Write: offset=");
        fat_print_dec(byte_offset);
        fat_print(", length=");
        fat_print_dec(length as u64);
        fat_print("\n");
        
        // Проверяем нужно ли расширить файл
        let new_size = byte_offset + length as u64;
        if new_size > (*fcb).file_size as u64 {
            // Нужно выделить дополнительные clusters
            let status = fat_expand_file_size(vcb, fcb, new_size);
            if status < 0 {
                fat_print("[FAT32] Failed to expand file\n");
                (*irp_struct).io_status.status = status;
                (*irp_struct).io_status.information = 0;
                IoCompleteRequest(irp, IO_NO_INCREMENT);
                return status;
            }
        }
        
        // Записываем данные через FAT chain
        let bytes_written = fat_write_cluster_chain(
            vcb,
            (*fcb).first_cluster,
            byte_offset,
            buffer,
            length,
        );
        
        match bytes_written {
            Ok(written) => {
                fat_print("[FAT32] Wrote ");
                fat_print_dec(written as u64);
                fat_print(" bytes\n");
                
                // Помечаем FCB как modified
                (*fcb).flags |= FCB_FLAGS_MODIFIED;
                
                (*irp_struct).io_status.status = STATUS_SUCCESS;
                (*irp_struct).io_status.information = written as usize;
                IoCompleteRequest(irp, IO_NO_INCREMENT);
                STATUS_SUCCESS
            }
            Err(status) => {
                fat_print("[FAT32] Write failed\n");
                (*irp_struct).io_status.status = status;
                (*irp_struct).io_status.information = 0;
                IoCompleteRequest(irp, IO_NO_INCREMENT);
                status
            }
        }
    }
}

/// QUERY_INFORMATION dispatcher
unsafe extern "win64" fn FatQueryInformation(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    unsafe {
        fat_print("[FAT32] IRP_MJ_QUERY_INFORMATION\n");
        
        let stack = IoGetCurrentIrpStackLocation(irp) as *mut IoStackLocation;
        let file_object = (*stack).file_object;
        let irp_struct = irp as *mut IrpStruct;
        
        // Получаем CCB из file object
        let file_obj = file_object as *mut FILE_OBJECT;
        if file_obj.is_null() || (*file_obj).fs_context.is_null() {
            (*irp_struct).io_status.status = STATUS_INVALID_PARAMETER;
            (*irp_struct).io_status.information = 0;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            return STATUS_INVALID_PARAMETER;
        }
        
        let ccb = (*file_obj).fs_context as *mut FAT32_CCB;
        if (*ccb).node_type != FAT32_CCB::NODE_TYPE {
            (*irp_struct).io_status.status = STATUS_INVALID_PARAMETER;
            (*irp_struct).io_status.information = 0;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            return STATUS_INVALID_PARAMETER;
        }
        
        let fcb = (*ccb).fcb;
        
        // Получаем параметры из IO_STACK_LOCATION
        // parameters.query_file содержит length и file_information_class
        let params_ptr = &(*stack).parameters as *const _ as *const u8;
        let file_info_class = core::ptr::read(params_ptr.add(4) as *const u32);
        let buffer_length = core::ptr::read(params_ptr as *const u32);
        let buffer = (*irp_struct).associated_irp as *mut u8;
        
        if buffer.is_null() {
            (*irp_struct).io_status.status = STATUS_INVALID_PARAMETER;
            (*irp_struct).io_status.information = 0;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            return STATUS_INVALID_PARAMETER;
        }
        
        let mut bytes_written = 0u32;
        let status = fat_query_information(
            fcb,
            ccb,
            file_info_class,
            buffer,
            buffer_length,
            &mut bytes_written,
        );
        
        (*irp_struct).io_status.status = status;
        (*irp_struct).io_status.information = bytes_written as usize;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        status
    }
}

/// QUERY_VOLUME_INFORMATION dispatcher
unsafe extern "win64" fn FatQueryVolumeInformation(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    unsafe {
        fat_print("[FAT32] IRP_MJ_QUERY_VOLUME_INFORMATION\n");
        
        let stack = IoGetCurrentIrpStackLocation(irp) as *mut IoStackLocation;
        let irp_struct = irp as *mut IrpStruct;
        
        // Получаем VCB из device extension
        let dev_obj = device_object as *mut DEVICE_OBJECT;
        let vcb = (*dev_obj).device_extension as *mut FAT32_VCB;
        
        if vcb.is_null() || (*vcb).node_type != FAT32_VCB::NODE_TYPE {
            (*irp_struct).io_status.status = STATUS_INVALID_DEVICE_REQUEST;
            (*irp_struct).io_status.information = 0;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            return STATUS_INVALID_DEVICE_REQUEST;
        }
        
        // Получаем параметры
        let fs_info_class = (*stack).parameters.read.length;
        let buffer = (*irp_struct).associated_irp as *mut u8;
        let buffer_length = (*stack).parameters.read.key;
        
        let mut bytes_written = 0u32;
        let status = fat_query_volume_information(
            vcb,
            fs_info_class,
            buffer,
            buffer_length,
            &mut bytes_written,
        );
        
        (*irp_struct).io_status.status = status;
        (*irp_struct).io_status.information = bytes_written as usize;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        status
    }
}

/// FLUSH_BUFFERS dispatcher
unsafe extern "win64" fn FatFlush(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    unsafe {
        fat_print("[FAT32] IRP_MJ_FLUSH_BUFFERS\n");
        
        let stack = IoGetCurrentIrpStackLocation(irp) as *mut IoStackLocation;
        let file_object = (*stack).file_object;
        let irp_struct = irp as *mut IrpStruct;
        
        // Получаем FCB
        let file_obj = file_object as *mut FILE_OBJECT;
        if !file_obj.is_null() && !(*file_obj).fs_context.is_null() {
            let ccb = (*file_obj).fs_context as *mut FAT32_CCB;
            let fcb = (*ccb).fcb;
            
            // Если FCB modified, обновляем directory entry
            if ((*fcb).flags & FCB_FLAGS_MODIFIED) != 0 {
                // Обновляем directory entry на диске
                let parent_cluster = (*fcb).parent_dir_cluster;
                if parent_cluster >= 2 || parent_cluster == 0 {
                    let fcb_vcb = (*fcb).vcb;
                    
                    // parent_cluster == 0 означает root directory
                    let actual_parent = if parent_cluster == 0 {
                        (*fcb_vcb).root_directory_cluster
                    } else {
                        parent_cluster
                    };
                    
                    let update_status = fat_update_directory_entry(fcb_vcb, fcb, actual_parent);
                    if update_status != STATUS_SUCCESS {
                        fat_print("[FAT32] Failed to update directory entry, status=0x");
                        fat_print_hex(update_status as u32 as u64);
                        fat_print("\n");
                    }
                }
                (*fcb).flags &= !FCB_FLAGS_MODIFIED;
            }
        }
        
        // Завершаем успешно (в полной реализации flush cache)
        (*irp_struct).io_status.status = STATUS_SUCCESS;
        (*irp_struct).io_status.information = 0;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        STATUS_SUCCESS
    }
}

/// SET_INFORMATION dispatcher
unsafe extern "win64" fn FatSetInformation(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    unsafe {
        fat_print("[FAT32] IRP_MJ_SET_INFORMATION\n");
        
        let stack = IoGetCurrentIrpStackLocation(irp) as *mut IoStackLocation;
        let file_object = (*stack).file_object;
        let irp_struct = irp as *mut IrpStruct;
        
        // Получаем CCB
        let file_obj = file_object as *mut FILE_OBJECT;
        if file_obj.is_null() || (*file_obj).fs_context.is_null() {
            (*irp_struct).io_status.status = STATUS_INVALID_PARAMETER;
            (*irp_struct).io_status.information = 0;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            return STATUS_INVALID_PARAMETER;
        }
        
        let ccb = (*file_obj).fs_context as *mut FAT32_CCB;
        let fcb = (*ccb).fcb;
        
        // Получаем параметры
        let file_info_class = (*stack).parameters.write.length;
        let buffer = (*irp_struct).associated_irp as *const u8;
        let buffer_length = (*stack).parameters.write.key;
        
        let status = fat_set_information(
            fcb,
            file_info_class,
            buffer,
            buffer_length,
        );
        
        (*irp_struct).io_status.status = status;
        (*irp_struct).io_status.information = 0;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        status
    }
}

/// DIRECTORY_CONTROL dispatcher
unsafe extern "win64" fn FatDirectoryControl(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    unsafe {
        fat_print("[FAT32] IRP_MJ_DIRECTORY_CONTROL\n");
        
        let stack = IoGetCurrentIrpStackLocation(irp) as *mut IoStackLocation;
        let file_object = (*stack).file_object;
        let minor = (*stack).minor_function;
        let irp_struct = irp as *mut IrpStruct;
        
        // Получаем CCB
        let file_obj = file_object as *mut FILE_OBJECT;
        if file_obj.is_null() || (*file_obj).fs_context.is_null() {
            (*irp_struct).io_status.status = STATUS_INVALID_PARAMETER;
            (*irp_struct).io_status.information = 0;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            return STATUS_INVALID_PARAMETER;
        }
        
        let ccb = (*file_obj).fs_context as *mut FAT32_CCB;
        let fcb = (*ccb).fcb;
        
        // Получаем параметры из IO_STACK_LOCATION
        let file_info_class = (*stack).parameters.read.length;
        let buffer = (*irp_struct).associated_irp as *mut u8;
        let buffer_length = (*stack).parameters.read.key;
        
        let mut bytes_written = 0u32;
        let status = fat_directory_control(
            fcb,
            ccb,
            minor,
            file_info_class,
            buffer,
            buffer_length,
            core::ptr::null(), // pattern
            false, // return_single_entry
            false, // restart_scan
            &mut bytes_written,
        );
        
        (*irp_struct).io_status.status = status;
        (*irp_struct).io_status.information = bytes_written as usize;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        status
    }
}

// =============================================================================
// Mount Volume
// =============================================================================

/// IRP_MN_MOUNT_VOLUME handler
unsafe fn fat_mount_volume(
    _fs_device: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    unsafe {
        fat_print("[FAT32] Mount volume request\n");
        
        let stack = IoGetCurrentIrpStackLocation(irp) as *mut IO_STACK_LOCATION;
        
        // Получаем параметры через union
        let vpb = (*stack).parameters.mount_volume.vpb;
        let storage_device = (*stack).parameters.mount_volume.device_object;
        
        if vpb.is_null() || storage_device.is_null() {
            let irp_struct = irp as *mut IrpStruct;
            (*irp_struct).io_status.status = STATUS_INVALID_PARAMETER;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            return STATUS_INVALID_PARAMETER;
        }
        
        fat_print("[FAT32] Reading boot sector from storage device 0x");
        fat_print_hex(storage_device as u64);
        fat_print("...\n");
        
        // Читаем boot sector
        let mut boot_sector = [0u8; 512];
        let read_status = read_boot_sector(storage_device, &mut boot_sector);
        
        if read_status < 0 {
            fat_print("[FAT32] ERROR: Failed to read boot sector, status=0x");
            fat_print_hex(read_status as u32 as u64);
            fat_print("\n");
            let irp_struct = irp as *mut IrpStruct;
            (*irp_struct).io_status.status = STATUS_UNRECOGNIZED_VOLUME;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            return STATUS_UNRECOGNIZED_VOLUME;
        }
        
        // Парсим BPB
        let bpb = &*(boot_sector.as_ptr() as *const FAT32_BPB);
        
        // Проверяем что это валидный FAT том
        if !bpb.is_valid(&boot_sector) {
            fat_print("[FAT32] Not a valid FAT volume (validation failed)\n");
            let irp_struct = irp as *mut IrpStruct;
            (*irp_struct).io_status.status = STATUS_UNRECOGNIZED_VOLUME;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            return STATUS_UNRECOGNIZED_VOLUME;
        }
        
        let fat_type = bpb.fat_type();
        if fat_type != 32 {
            fat_print("[FAT32] Found FAT");
            fat_print_dec(fat_type as u64);
            fat_print(" volume (not FAT32)\n");
            let irp_struct = irp as *mut IrpStruct;
            (*irp_struct).io_status.status = STATUS_UNRECOGNIZED_VOLUME;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            return STATUS_UNRECOGNIZED_VOLUME;
        }
        
        fat_print("[FAT32] Valid FAT32 volume detected!\n");
        fat_print("[FAT32]   Cluster size: ");
        fat_print_dec(bpb.cluster_size() as u64);
        fat_print(" bytes\n");
        
        // Создаём volume device object
        let mut volume_device: PDEVICE_OBJECT = core::ptr::null_mut();
        let vcb_size = core::mem::size_of::<FAT32_VCB>() as ULONG;
        
        // Получаем driver object от FS device (не от storage device!)
        let fs_dev = _fs_device as *mut DEVICE_OBJECT;
        let driver_obj = (*fs_dev).driver_object;
        
        let create_status = IoCreateDevice(
            driver_obj,
            vcb_size,
            ptr::null(), // Unnamed volume device
            FILE_DEVICE_DISK_FILE_SYSTEM,
            0,
            0,
            &mut volume_device,
        );
        
        if create_status < 0 {
            fat_print("[FAT32] ERROR: Failed to create volume device\n");
            let irp_struct = irp as *mut IrpStruct;
            (*irp_struct).io_status.status = STATUS_INSUFFICIENT_RESOURCES;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            return STATUS_INSUFFICIENT_RESOURCES;
        }
        
        // Инициализируем VCB
        let vol_dev = volume_device as *mut DEVICE_OBJECT;
        let vcb = (*vol_dev).device_extension as *mut FAT32_VCB;
        core::ptr::write(vcb, FAT32_VCB::new());
        
        (*vcb).target_device = storage_device;
        (*vcb).volume_device = volume_device;
        (*vcb).vpb = vpb;
        (*vcb).init_from_bpb(bpb);
        
        // Устанавливаем VPB linkage
        let vpb_ptr = vpb as *mut VPB;
        (*vpb_ptr).device_object = volume_device;
        (*vpb_ptr).flags |= VPB_MOUNTED;
        (*vpb_ptr).serial_number = bpb.volume_id;
        
        // Копируем volume label в VPB
        for i in 0..11 {
            (*vpb_ptr).volume_label[i] = bpb.volume_label[i] as u16;
        }
        (*vpb_ptr).volume_label_length = 22; // 11 chars * 2 bytes
        
        // Устанавливаем stack size
        let storage_dev = storage_device as *mut DEVICE_OBJECT;
        (*vol_dev).stack_size = (*storage_dev).stack_size + 1;
        
        fat_print("[FAT32] Volume mounted successfully\n");
        fat_print("[FAT32]   Serial: 0x");
        fat_print_hex(bpb.volume_id as u64);
        fat_print("\n");
        
        // Debug: выводим содержимое root directory
        fat_debug_list_root_directory(vcb);
        
        let irp_struct = irp as *mut IrpStruct;
        (*irp_struct).io_status.status = STATUS_SUCCESS;
        (*irp_struct).io_status.information = 0;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        STATUS_SUCCESS
    }
}

/// IRP_MN_VERIFY_VOLUME handler
unsafe fn fat_verify_volume(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    unsafe {
        // IRP_MN_VERIFY_VOLUME проверяет что media не изменилась
        // Полная реализация:
        // 1. Перечитать boot sector
        // 2. Проверить serial number
        // 3. Вернуть STATUS_WRONG_VOLUME если изменилась media
        
        let irp_struct = irp as *mut IrpStruct;
        (*irp_struct).io_status.status = STATUS_SUCCESS;
        (*irp_struct).io_status.information = 0;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        STATUS_SUCCESS
    }
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Читает boot sector с storage device
unsafe fn read_boot_sector(
    storage_device: PDEVICE_OBJECT,
    buffer: &mut [u8; 512],
) -> NTSTATUS {
    unsafe {
        // Создаём synchronous IRP для чтения boot sector
        let mut event: KEVENT = core::mem::zeroed();
        let mut iosb = IO_STATUS_BLOCK::new(STATUS_UNSUCCESSFUL);
        
        KeInitializeEvent(&mut event, NOTIFICATION_EVENT, 0);
        
        let irp = IoBuildSynchronousFsdRequest(
            IRP_MJ_READ as ULONG,
            storage_device,
            buffer.as_mut_ptr() as PVOID,
            512,
            0, // offset = 0 (boot sector)
            &mut event as *mut KEVENT as PVOID,
            &mut iosb as *mut IO_STATUS_BLOCK as PVOID,
        );
        
        if irp.is_null() {
            fat_print("[FAT32] ERROR: Failed to build IRP for boot sector read\n");
            return STATUS_INSUFFICIENT_RESOURCES;
        }
        
        let status = IoCallDriver(storage_device, irp);
        
        // Ждём завершения если pending
        if status == STATUS_PENDING {
            KeWaitForSingleObject(
                &mut event as *mut KEVENT as PVOID,
                0, // Executive
                0, // KernelMode  
                0, // Not alertable
                core::ptr::null_mut(), // No timeout
            );
        }
        
        iosb.status_or_pointer.status
    }
}

/// Выводит строку через DbgPrint
unsafe fn fat_print(s: &str) {
    let mut buf = [0u8; 256];
    let len = s.len().min(255);
    for (i, &b) in s.as_bytes().iter().take(len).enumerate() {
        buf[i] = b;
    }
    buf[len] = 0;
    DbgPrint(buf.as_ptr());
}

/// Выводит число в decimal
unsafe fn fat_print_dec(value: u64) {
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

/// Выводит число в hex
unsafe fn fat_print_hex(value: u64) {
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

/// Выводит Unicode строку (конвертируя в ASCII)
unsafe fn fat_print_unicode(wide_str: &[u16]) {
    let mut buf = [0u8; 256];
    let mut pos = 0;
    
    for &wide_char in wide_str {
        if wide_char == 0 || pos >= 254 {
            break;
        }
        
        // Простая конвертация Unicode -> ASCII (только для ASCII символов)
        if wide_char < 128 {
            buf[pos] = wide_char as u8;
            pos += 1;
        } else {
            // Не-ASCII символы заменяем на '?'
            buf[pos] = b'?';
            pos += 1;
        }
    }
    
    buf[pos] = 0;
    unsafe { DbgPrint(buf.as_ptr()); }
}

/// Выводит UNICODE_STRING
unsafe fn fat_print_unicode_string(us: &UNICODE_STRING) {
    if us.buffer.is_null() || us.length == 0 {
        return;
    }
    
    let wide_len = (us.length / 2) as usize;
    let mut buf = [0u8; 256];
    let mut pos = 0;
    
    for i in 0..wide_len.min(254) {
        let wide_char = *us.buffer.add(i);
        
        if wide_char == 0 {
            break;
        }
        
        if wide_char < 128 {
            buf[pos] = wide_char as u8;
            pos += 1;
        } else {
            buf[pos] = b'?';
            pos += 1;
        }
    }
    
    buf[pos] = 0;
    unsafe { DbgPrint(buf.as_ptr()); }
}

/// Выводит имя файла в формате 8.3
unsafe fn fat_print_name(name: &[u8; 11]) {
    let mut buf = [0u8; 14]; // "NAME    EXT\0"
    
    // Копируем имя и расширение, заменяя пробелы на точку
    let mut pos = 0;
    
    // Имя (8 символов)
    for i in 0..8 {
        if name[i] != b' ' {
            buf[pos] = name[i];
            pos += 1;
        } else if i > 0 && name[i-1] != b' ' {
            break; // Конец имени
        }
    }
    
    // Расширение (3 символа)
    if name[8] != b' ' {
        buf[pos] = b'.';
        pos += 1;
        for i in 8..11 {
            if name[i] != b' ' {
                buf[pos] = name[i];
                pos += 1;
            }
        }
    }
    
    buf[pos] = 0;
    DbgPrint(buf.as_ptr());
}

/// Debug: выводит дерево файловой системы
unsafe fn fat_debug_list_root_directory(vcb: *mut FAT32_VCB) {
    fat_print("\n");
    fat_print("   ┌───────────────────────────────────────┐\n");
    fat_print("   │      File System Tree (FAT32)        │\n");
    fat_print("   └───────────────────────────────────────┘\n");
    
    let vcb_ref = &*vcb;
    let cluster_size = vcb_ref.bytes_per_cluster;
    
    // Выделяем буфер для чтения cluster
    let buffer = ExAllocatePoolWithTag(
        NON_PAGED_POOL,
        cluster_size as usize,
        FAT32_POOL_TAG,
    );
    
    if buffer.is_null() {
        fat_print("[FAT32] ERROR: Cannot allocate buffer\n");
        return;
    }
    
    // Читаем root directory cluster
    let root_cluster = vcb_ref.root_directory_cluster;
    
    let bytes_read = fat_read_cluster_chain(
        vcb,
        root_cluster,
        0,
        buffer as *mut u8,
        cluster_size,
    );
    
    fat_print("[FAT32] Read result: ");
    match &bytes_read {
        Ok(read) => {
            fat_print("OK, ");
            fat_print_dec(*read as u64);
            fat_print(" bytes\n");
        }
        Err(status) => {
            fat_print("ERROR 0x");
            fat_print_hex(*status as u32 as u64);
            fat_print("\n");
        }
    }
    
    match bytes_read {
        Ok(read) if read > 0 => {
            let num_entries = read / 32; // 32 bytes per directory entry
            
            fat_print("[FAT32] Scanning ");
            fat_print_dec(num_entries as u64);
            fat_print(" directory entries\n");
            
            let mut lfn_buffer = [0u16; 256]; // Буфер для сборки LFN
            let mut lfn_length = 0usize;
            
            for i in 0..num_entries {
                let entry_offset = (i * 32) as isize;
                let entry_ptr = (buffer as *const u8).offset(entry_offset) 
                    as *const u8;
                
                // Читаем поля entry вручную
                let name_byte0 = *entry_ptr;
                let attr = *entry_ptr.add(11);
                
                // Конец директории
                if name_byte0 == 0x00 {
                    break;
                }
                
                // Пропускаем удалённые
                if name_byte0 == 0xE5 {
                    lfn_length = 0; // Сбрасываем LFN буфер
                    continue;
                }
                
                // LFN entry - собираем имя
                if attr == 0x0F {
                    // LFN entry содержит фрагменты длинного имени
                    // Sequence number в первом байте (0x01-0x14, 0x40=last)
                    let seq = name_byte0 & 0x1F;
                    let is_last = (name_byte0 & 0x40) != 0;
                    
                    if is_last {
                        lfn_length = 0; // Начинаем новое имя
                    }
                    
                    // LFN entry содержит 13 Unicode символов в позициях:
                    // 0x01-0x0A: 5 chars (10 bytes)
                    // 0x0E-0x19: 6 chars (12 bytes)
                    // 0x1C-0x1F: 2 chars (4 bytes)
                    
                    // Читаем символы (в обратном порядке - от конца к началу)
                    let idx_base = if seq > 0 { (seq - 1) as usize * 13 } else { 0 };
                    
                    // Первые 5 символов (offset 0x01)
                    for j in 0..5 {
                        let char_offset = 1 + j * 2;
                        let ch = u16::from_le_bytes([
                            *entry_ptr.add(char_offset),
                            *entry_ptr.add(char_offset + 1),
                        ]);
                        if ch != 0 && ch != 0xFFFF && (idx_base + j) < 256 {
                            lfn_buffer[idx_base + j] = ch;
                            lfn_length = (idx_base + j + 1).max(lfn_length);
                        }
                    }
                    
                    // Следующие 6 символов (offset 0x0E)
                    for j in 0..6 {
                        let char_offset = 0x0E + j * 2;
                        let ch = u16::from_le_bytes([
                            *entry_ptr.add(char_offset),
                            *entry_ptr.add(char_offset + 1),
                        ]);
                        if ch != 0 && ch != 0xFFFF && (idx_base + 5 + j) < 256 {
                            lfn_buffer[idx_base + 5 + j] = ch;
                            lfn_length = (idx_base + 5 + j + 1).max(lfn_length);
                        }
                    }
                    
                    // Последние 2 символа (offset 0x1C)
                    for j in 0..2 {
                        let char_offset = 0x1C + j * 2;
                        let ch = u16::from_le_bytes([
                            *entry_ptr.add(char_offset),
                            *entry_ptr.add(char_offset + 1),
                        ]);
                        if ch != 0 && ch != 0xFFFF && (idx_base + 11 + j) < 256 {
                            lfn_buffer[idx_base + 11 + j] = ch;
                            lfn_length = (idx_base + 11 + j + 1).max(lfn_length);
                        }
                    }
                    
                    continue; // Переходим к следующему entry
                }
                
                // Пропускаем volume labels
                if (attr & 0x08) != 0 {
                    continue;
                }
                
                // Обычный directory entry
                
                // Читаем имя (11 bytes)
                let mut name = [0u8; 11];
                for j in 0..11 {
                    name[j] = *entry_ptr.add(j);
                }
                
                // Читаем cluster и size
                let first_cluster_low = u16::from_le_bytes([
                    *entry_ptr.add(26),
                    *entry_ptr.add(27),
                ]) as u32;
                let first_cluster_high = u16::from_le_bytes([
                    *entry_ptr.add(20),
                    *entry_ptr.add(21),
                ]) as u32;
                let first_cluster = (first_cluster_high << 16) | first_cluster_low;
                
                let file_size = u32::from_le_bytes([
                    *entry_ptr.add(28),
                    *entry_ptr.add(29),
                    *entry_ptr.add(30),
                    *entry_ptr.add(31),
                ]);
                
                // Выводим в древовидном формате
                let is_last_entry = (i + 1 >= num_entries) || 
                    (i + 1 < num_entries && 
                     i + 2 < num_entries && 
                     *entry_ptr.add(32) == 0x00);
                
                fat_print("   ");
                if is_last_entry {
                    fat_print("└── ");
                } else {
                    fat_print("├── ");
                }
                
                // Если есть LFN, выводим его
                if lfn_length > 0 {
                    fat_print_unicode(&lfn_buffer[..lfn_length]);
                    lfn_length = 0;
                } else {
                    fat_print_name(&name);
                }
                
                if (attr & 0x10) != 0 {
                    fat_print("/\n");
                    
                    // Рекурсивно выводим содержимое (depth < 2 для ограничения)
                    if first_cluster >= 2 && i < 10 {
                        let new_prefix = if is_last_entry { "       " } else { "   │   " };
                        fat_print_subdir_recursive(vcb, first_cluster, new_prefix, 1);
                    }
                } else {
                    fat_print(" (");
                    fat_print_dec(file_size as u64);
                    fat_print(" bytes)\n");
                }
            }
        }
        Err(status) => {
            fat_print("[FAT32] ERROR: Failed to read root directory, status=0x");
            fat_print_hex(status as u32 as u64);
            fat_print("\n");
        }
        _ => {}
    }
    
    ExFreePoolWithTag(buffer, FAT32_POOL_TAG);
    
    fat_print("\n");
}

/// Рекурсивно выводит содержимое поддиректории
unsafe fn fat_print_subdir_recursive(
    vcb: *mut FAT32_VCB,
    dir_cluster: u32,
    prefix: &str,
    depth: u32,
) {
    if depth > 4 || dir_cluster < 2 {
        return; // Максимум 4 уровня вложенности
    }
    
    let vcb_ref = &*vcb;
    let cluster_size = vcb_ref.bytes_per_cluster;
    
    let buffer = ExAllocatePoolWithTag(
        NON_PAGED_POOL,
        cluster_size as usize,
        FAT32_POOL_TAG,
    );
    
    if buffer.is_null() {
        return;
    }
    
    let bytes_read = fat_read_cluster_chain(
        vcb,
        dir_cluster,
        0,
        buffer as *mut u8,
        cluster_size,
    );
    
    match bytes_read {
        Ok(read) if read > 0 => {
            let num_entries = read / 32;
            let mut lfn_buffer = [0u16; 256];
            let mut lfn_length = 0usize;
            let mut count = 0usize;
            
            for i in 0..num_entries {
                let entry_offset = (i * 32) as isize;
                let entry_ptr = (buffer as *const u8).offset(entry_offset) as *const u8;
                
                let name_byte0 = *entry_ptr;
                let attr = *entry_ptr.add(11);
                
                if name_byte0 == 0x00 {
                    break;
                }
                
                if name_byte0 == 0xE5 {
                    lfn_length = 0;
                    continue;
                }
                
                // LFN parsing - собираем длинное имя из последовательности LFN entries
                if attr == 0x0F {
                    let seq = name_byte0 & 0x1F;
                    let is_last_lfn = (name_byte0 & 0x40) != 0;
                    
                    if is_last_lfn {
                        lfn_length = 0;
                    }
                    
                    let idx_base = if seq > 0 { (seq - 1) as usize * 13 } else { 0 };
                    
                    for j in 0..5 {
                        let char_offset = 1 + j * 2;
                        let ch = u16::from_le_bytes([
                            *entry_ptr.add(char_offset),
                            *entry_ptr.add(char_offset + 1),
                        ]);
                        if ch != 0 && ch != 0xFFFF && (idx_base + j) < 256 {
                            lfn_buffer[idx_base + j] = ch;
                            lfn_length = (idx_base + j + 1).max(lfn_length);
                        }
                    }
                    
                    for j in 0..6 {
                        let char_offset = 0x0E + j * 2;
                        let ch = u16::from_le_bytes([
                            *entry_ptr.add(char_offset),
                            *entry_ptr.add(char_offset + 1),
                        ]);
                        if ch != 0 && ch != 0xFFFF && (idx_base + 5 + j) < 256 {
                            lfn_buffer[idx_base + 5 + j] = ch;
                            lfn_length = (idx_base + 5 + j + 1).max(lfn_length);
                        }
                    }
                    
                    for j in 0..2 {
                        let char_offset = 0x1C + j * 2;
                        let ch = u16::from_le_bytes([
                            *entry_ptr.add(char_offset),
                            *entry_ptr.add(char_offset + 1),
                        ]);
                        if ch != 0 && ch != 0xFFFF && (idx_base + 11 + j) < 256 {
                            lfn_buffer[idx_base + 11 + j] = ch;
                            lfn_length = (idx_base + 11 + j + 1).max(lfn_length);
                        }
                    }
                    
                    continue;
                }
                
                if (attr & 0x08) != 0 {
                    continue;
                }
                
                // Обычный entry
                let mut name = [0u8; 11];
                for j in 0..11 {
                    name[j] = *entry_ptr.add(j);
                }
                
                // Пропускаем . и .. entries
                if name[0] == b'.' && (name[1] == b' ' || 
                   (name[1] == b'.' && name[2] == b' ')) {
                    lfn_length = 0;
                    continue;
                }
                
                let first_cluster_low = u16::from_le_bytes([
                    *entry_ptr.add(26),
                    *entry_ptr.add(27),
                ]) as u32;
                let first_cluster_high = u16::from_le_bytes([
                    *entry_ptr.add(20),
                    *entry_ptr.add(21),
                ]) as u32;
                let first_cluster = (first_cluster_high << 16) | first_cluster_low;
                
                let file_size = u32::from_le_bytes([
                    *entry_ptr.add(28),
                    *entry_ptr.add(29),
                    *entry_ptr.add(30),
                    *entry_ptr.add(31),
                ]);
                
                count += 1;
                let is_last_item = count >= 10 || (i + 1 >= num_entries) || 
                    (i + 1 < num_entries && *entry_ptr.add(32) == 0x00);
                
                fat_print(prefix);
                if is_last_item {
                    fat_print("└── ");
                } else {
                    fat_print("├── ");
                }
                
                if lfn_length > 0 {
                    fat_print_unicode(&lfn_buffer[..lfn_length]);
                    lfn_length = 0;
                } else {
                    fat_print_name(&name);
                }
                
                if (attr & 0x10) != 0 {
                    fat_print("/\n");
                    
                    // Рекурсивно для поддиректорий
                    if first_cluster >= 2 && depth < 4 {
                        let mut new_prefix_buf = [0u8; 128];
                        let mut pos = 0;
                        
                        for &b in prefix.as_bytes() {
                            if pos < 115 {
                                new_prefix_buf[pos] = b;
                                pos += 1;
                            }
                        }
                        
                        if is_last_item {
                            for &b in b"    ".iter() {
                                if pos < 115 {
                                    new_prefix_buf[pos] = b;
                                    pos += 1;
                                }
                            }
                        } else {
                            for &b in b"|   ".iter() {
                                if pos < 115 {
                                    new_prefix_buf[pos] = b;
                                    pos += 1;
                                }
                            }
                        }
                        
                        let new_prefix_str = core::str::from_utf8(&new_prefix_buf[..pos]).unwrap_or("");
                        fat_print_subdir_recursive(vcb, first_cluster, new_prefix_str, depth + 1);
                    }
                } else {
                    fat_print(" (");
                    fat_print_dec(file_size as u64);
                    fat_print(" bytes)\n");
                }
                
                if count >= 20 {
                    fat_print(prefix);
                    fat_print("   ... (еще ");
                    fat_print_dec((num_entries - i) as u64);
                    fat_print(" элементов)\n");
                    break; // Ограничиваем вывод для производительности
                }
            }
        }
        _ => {}
    }
    
    ExFreePoolWithTag(buffer, FAT32_POOL_TAG);
}

/// Рекурсивно выводит содержимое директории в виде дерева (старая версия, не используется)
unsafe fn fat_print_directory_tree(
    vcb: *mut FAT32_VCB,
    dir_cluster: u32,
    prefix: &str,
    is_last: bool,
    depth: u32,
) {
    if depth > 8 {
        return; // Защита от слишком глубокой рекурсии
    }
    
    let vcb_ref = &*vcb;
    let cluster_size = vcb_ref.bytes_per_cluster;
    
    // Выделяем буфер для чтения cluster
    let buffer = ExAllocatePoolWithTag(
        NON_PAGED_POOL,
        cluster_size as usize,
        FAT32_POOL_TAG,
    );
    
    if buffer.is_null() {
        return;
    }
    
    // Читаем directory cluster
    let bytes_read = fat_read_cluster_chain(
        vcb,
        dir_cluster,
        0,
        buffer as *mut u8,
        cluster_size,
    );
    
    // Собираем список entries
    let mut entries: [DirectoryEntryInfo; 64] = [DirectoryEntryInfo::empty(); 64];
    let mut entry_count = 0usize;
    
    match bytes_read {
        Ok(read) if read > 0 => {
            let num_entries = read / 32;
            let mut lfn_buffer = [0u16; 256];
            let mut lfn_length = 0usize;
            
            for i in 0..num_entries {
                if entry_count >= 64 {
                    break;
                }
                
                let entry_offset = (i * 32) as isize;
                let entry_ptr = (buffer as *const u8).offset(entry_offset) as *const u8;
                
                let name_byte0 = *entry_ptr;
                let attr = *entry_ptr.add(11);
                
                // Конец директории
                if name_byte0 == 0x00 {
                    break;
                }
                
                // Пропускаем удалённые
                if name_byte0 == 0xE5 {
                    lfn_length = 0;
                    continue;
                }
                
                // LFN entry
                if attr == 0x0F {
                    let seq = name_byte0 & 0x1F;
                    let is_last_lfn = (name_byte0 & 0x40) != 0;
                    
                    if is_last_lfn {
                        lfn_length = 0;
                    }
                    
                    let idx_base = if seq > 0 { (seq - 1) as usize * 13 } else { 0 };
                    
                    // Читаем символы LFN
                    for j in 0..5 {
                        let char_offset = 1 + j * 2;
                        let ch = u16::from_le_bytes([
                            *entry_ptr.add(char_offset),
                            *entry_ptr.add(char_offset + 1),
                        ]);
                        if ch != 0 && ch != 0xFFFF && (idx_base + j) < 256 {
                            lfn_buffer[idx_base + j] = ch;
                            lfn_length = (idx_base + j + 1).max(lfn_length);
                        }
                    }
                    
                    for j in 0..6 {
                        let char_offset = 0x0E + j * 2;
                        let ch = u16::from_le_bytes([
                            *entry_ptr.add(char_offset),
                            *entry_ptr.add(char_offset + 1),
                        ]);
                        if ch != 0 && ch != 0xFFFF && (idx_base + 5 + j) < 256 {
                            lfn_buffer[idx_base + 5 + j] = ch;
                            lfn_length = (idx_base + 5 + j + 1).max(lfn_length);
                        }
                    }
                    
                    for j in 0..2 {
                        let char_offset = 0x1C + j * 2;
                        let ch = u16::from_le_bytes([
                            *entry_ptr.add(char_offset),
                            *entry_ptr.add(char_offset + 1),
                        ]);
                        if ch != 0 && ch != 0xFFFF && (idx_base + 11 + j) < 256 {
                            lfn_buffer[idx_base + 11 + j] = ch;
                            lfn_length = (idx_base + 11 + j + 1).max(lfn_length);
                        }
                    }
                    
                    continue;
                }
                
                // Пропускаем volume labels
                if (attr & 0x08) != 0 {
                    continue;
                }
                
                // Сохраняем entry info
                let mut entry_info = DirectoryEntryInfo::empty();
                
                // Копируем short name
                for j in 0..11 {
                    entry_info.short_name[j] = *entry_ptr.add(j);
                }
                
                // Копируем LFN если есть
                if lfn_length > 0 {
                    for j in 0..lfn_length.min(127) {
                        entry_info.long_name[j] = lfn_buffer[j];
                    }
                    entry_info.long_name_len = lfn_length.min(127);
                    lfn_length = 0;
                }
                
                // Читаем cluster и size
                let first_cluster_low = u16::from_le_bytes([
                    *entry_ptr.add(26),
                    *entry_ptr.add(27),
                ]) as u32;
                let first_cluster_high = u16::from_le_bytes([
                    *entry_ptr.add(20),
                    *entry_ptr.add(21),
                ]) as u32;
                entry_info.first_cluster = (first_cluster_high << 16) | first_cluster_low;
                entry_info.file_size = u32::from_le_bytes([
                    *entry_ptr.add(28),
                    *entry_ptr.add(29),
                    *entry_ptr.add(30),
                    *entry_ptr.add(31),
                ]);
                entry_info.is_directory = (attr & 0x10) != 0;
                
                entries[entry_count] = entry_info;
                entry_count += 1;
            }
        }
        _ => {}
    }
    
    ExFreePoolWithTag(buffer, FAT32_POOL_TAG);
    
    // Выводим entries с древовидной структурой
    for i in 0..entry_count {
        let entry = &entries[i];
        let is_last_entry = i == entry_count - 1;
        
        // Выводим prefix
        fat_print(prefix);
        
        // Выводим tree symbols
        if is_last_entry {
            fat_print("   └── ");
        } else {
            fat_print("   ├── ");
        }
        
        // Выводим имя
        if entry.long_name_len > 0 {
            // LFN
            let lfn_slice = &entry.long_name[..entry.long_name_len];
            fat_print_unicode(lfn_slice);
        } else {
            // 8.3 name
            fat_print_name(&entry.short_name);
        }
        
        if entry.is_directory {
            fat_print("/\n");
            
            // Рекурсивно выводим содержимое директории
            if entry.first_cluster >= 2 && depth < 3 {
                // Формируем новый prefix
                let mut new_prefix = [0u8; 128];
                let mut pos = 0;
                
                for &b in prefix.as_bytes() {
                    if pos < 120 {
                        new_prefix[pos] = b;
                        pos += 1;
                    }
                }
                
                // Добавляем отступ для вложенных элементов
                for &b in b"       ".iter() {
                    if pos < 120 {
                        new_prefix[pos] = b;
                        pos += 1;
                    }
                }
                new_prefix[pos] = 0;
                
                let new_prefix_str = core::str::from_utf8(&new_prefix[..pos]).unwrap_or("");
                
                fat_print_directory_tree(
                    vcb,
                    entry.first_cluster,
                    new_prefix_str,
                    is_last_entry,
                    depth + 1,
                );
            }
        } else {
            fat_print(" (");
            fat_print_dec(entry.file_size as u64);
            fat_print(" bytes)\n");
        }
    }
}

/// Информация о directory entry для древовидного вывода
#[derive(Clone, Copy)]
struct DirectoryEntryInfo {
    short_name: [u8; 11],
    long_name: [u16; 128],
    long_name_len: usize,
    first_cluster: u32,
    file_size: u32,
    is_directory: bool,
}

impl DirectoryEntryInfo {
    const fn empty() -> Self {
        Self {
            short_name: [0; 11],
            long_name: [0; 128],
            long_name_len: 0,
            first_cluster: 0,
            file_size: 0,
            is_directory: false,
        }
    }
}

/// Конвертирует Unicode строку в 8.3 формат
unsafe fn unicode_to_83(us: &UNICODE_STRING, output: &mut [u8; 11]) -> bool {
    if us.buffer.is_null() || us.length == 0 {
        return false;
    }
    
    // Конвертируем Unicode в ASCII
    let wide_len = (us.length / 2) as usize;
    let mut ascii_buf = [0u8; 256];
    let mut ascii_len = 0;
    
    for i in 0..wide_len.min(255) {
        let wide_char = *us.buffer.add(i);
        
        // Пропускаем начальный '\'
        if i == 0 && wide_char == b'\\' as u16 {
            continue;
        }
        
        // Простая конвертация (только ASCII)
        if wide_char < 128 {
            ascii_buf[ascii_len] = wide_char as u8;
            ascii_len += 1;
        } else {
            return false; // Не-ASCII символы не поддерживаются
        }
    }
    
    if ascii_len == 0 {
        return false;
    }
    
    // Конвертируем ASCII в 8.3
    match core::str::from_utf8(&ascii_buf[..ascii_len]) {
        Ok(ascii_str) => convert_to_83_name(ascii_str, output),
        Err(_) => false,
    }
}

// =============================================================================
// Structures для IRP manipulation
// =============================================================================

#[repr(C)]
pub union IoStackParameters {
    pub mount_volume: IoStackMountVolume,
    pub read: IoStackReadWrite,
    pub write: IoStackReadWrite,
    pub raw: [u8; 40],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct IoStackMountVolume {
    pub vpb: PVPB,
    pub device_object: PDEVICE_OBJECT,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct IoStackReadWrite {
    pub length: u32,
    pub key: u32,
    pub byte_offset: i64,
}

#[repr(C)]
pub struct IoStackLocation {
    pub major_function: u8,
    pub minor_function: u8,
    pub flags: u8,
    pub control: u8,
    pub parameters: IoStackParameters,
    pub device_object: PDEVICE_OBJECT,
    pub file_object: PVOID,
    pub completion_routine: PVOID,
    pub context: PVOID,
}

pub type IO_STACK_LOCATION = IoStackLocation;

#[repr(C)]
struct IrpStruct {
    _type: CSHORT,
    size: USHORT,
    mdl_address: PVOID,
    flags: ULONG,
    associated_irp: PVOID,
    thread_list_entry: [PVOID; 2], // LIST_ENTRY
    io_status: IoStatusBlock,
    // ... остальные поля не нужны для текущей реализации
}

#[repr(C)]
struct IoStatusBlock {
    status: NTSTATUS,
    information: ULONG_PTR,
}

#[repr(C)]
pub struct VPB {
    pub r#type: CSHORT,
    pub size: CSHORT,
    pub flags: USHORT,
    pub volume_label_length: USHORT,
    pub device_object: PDEVICE_OBJECT,
    pub real_device: PDEVICE_OBJECT,
    pub serial_number: ULONG,
    pub reference_count: ULONG,
    pub volume_label: [u16; 16],
}

pub const VPB_MOUNTED: u16 = 0x0001;

#[repr(C)]
pub struct DriverObjectStruct {
    pub _type: CSHORT,
    pub size: CSHORT,
    pub device_object: PDEVICE_OBJECT,
    pub flags: ULONG,
    pub driver_start: PVOID,
    pub driver_size: ULONG,
    pub driver_section: PVOID,
    pub driver_extension: PVOID,
    pub driver_name: UNICODE_STRING,
    pub hardware_database: PVOID,
    pub fast_io_dispatch: PVOID,
    pub driver_init: PVOID,
    pub driver_start_io: PVOID,
    pub driver_unload: PVOID,
    pub major_function: [Option<unsafe extern "win64" fn(PDEVICE_OBJECT, PIRP) -> NTSTATUS>; 28],
}

// =============================================================================
// Panic Handler
// =============================================================================

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

// =============================================================================
// Global Static (required by MSVC ABI)
// =============================================================================

#[unsafe(no_mangle)]
pub static _fltused: i32 = 0;

