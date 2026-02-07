//! File Information Queries
//!
//! Реализация IRP_MJ_QUERY_INFORMATION для получения информации о файлах.

use crate::types::*;
use crate::fcb::FAT32_FCB;

// =============================================================================
// File Information Classes
// =============================================================================

pub const FILE_DIRECTORY_INFORMATION: u32 = 1;
pub const FILE_BASIC_INFORMATION: u32 = 4;
pub const FILE_STANDARD_INFORMATION: u32 = 5;
pub const FILE_INTERNAL_INFORMATION: u32 = 6;
pub const FILE_EA_INFORMATION: u32 = 7;
pub const FILE_ACCESS_INFORMATION: u32 = 8;
pub const FILE_NAME_INFORMATION: u32 = 9;
pub const FILE_POSITION_INFORMATION: u32 = 14;
pub const FILE_ALL_INFORMATION: u32 = 18;
pub const FILE_NETWORK_OPEN_INFORMATION: u32 = 34;

// =============================================================================
// Structures
// =============================================================================

/// FILE_BASIC_INFORMATION - базовая информация о файле
#[repr(C)]
pub struct FILE_BASIC_INFORMATION {
    pub creation_time: i64,        // FILETIME
    pub last_access_time: i64,     // FILETIME
    pub last_write_time: i64,      // FILETIME
    pub change_time: i64,          // FILETIME
    pub file_attributes: u32,      // FILE_ATTRIBUTE_*
}

/// FILE_STANDARD_INFORMATION - стандартная информация
#[repr(C)]
pub struct FILE_STANDARD_INFORMATION {
    pub allocation_size: i64,      // Выделенный размер (в кластерах)
    pub end_of_file: i64,          // Реальный размер файла
    pub number_of_links: u32,      // Количество hard links (FAT32 = 1)
    pub delete_pending: u8,        // Файл помечен на удаление
    pub directory: u8,             // Это директория
}

/// FILE_POSITION_INFORMATION - текущая позиция
#[repr(C)]
pub struct FILE_POSITION_INFORMATION {
    pub current_byte_offset: i64,
}

/// FILE_NAME_INFORMATION - имя файла
#[repr(C)]
pub struct FILE_NAME_INFORMATION {
    pub file_name_length: u32,     // Длина в байтах (не включая NUL)
    pub file_name: [u16; 260],     // Unicode имя (MAX_PATH)
}

// =============================================================================
// Query Information Handler
// =============================================================================

/// Обрабатывает IRP_MJ_QUERY_INFORMATION
pub unsafe fn fat_query_information(
    fcb: *mut FAT32_FCB,
    ccb: *mut crate::fcb::FAT32_CCB,
    file_info_class: u32,
    buffer: *mut u8,
    buffer_length: u32,
    bytes_written: *mut u32,
) -> NTSTATUS {
    unsafe {
        if fcb.is_null() || buffer.is_null() {
            return STATUS_INVALID_PARAMETER;
        }
        
        match file_info_class {
            FILE_BASIC_INFORMATION => {
                query_basic_information(fcb, buffer, buffer_length, bytes_written)
            }
            FILE_STANDARD_INFORMATION => {
                query_standard_information(fcb, buffer, buffer_length, bytes_written)
            }
            FILE_POSITION_INFORMATION => {
                query_position_information(ccb, buffer, buffer_length, bytes_written)
            }
            FILE_NAME_INFORMATION => {
                query_name_information(fcb, buffer, buffer_length, bytes_written)
            }
            FILE_ALL_INFORMATION => {
                // FILE_ALL_INFORMATION объединяет Basic + Standard + Internal + Ea + Access + Position + Name
                // Возвращаем Basic + Standard (минимальный набор)
                if buffer_length < core::mem::size_of::<FILE_BASIC_INFORMATION>() as u32 + 
                                    core::mem::size_of::<FILE_STANDARD_INFORMATION>() as u32 {
                    return STATUS_BUFFER_TOO_SMALL;
                }
                
                let mut offset = 0u32;
                
                // Basic information
                let mut basic_written = 0u32;
                let status = query_basic_information(fcb, buffer, buffer_length, &mut basic_written as *mut u32);
                if status != STATUS_SUCCESS {
                    return status;
                }
                offset += basic_written;
                
                // Standard information
                let mut standard_written = 0u32;
                let status = query_standard_information(fcb, buffer.add(offset as usize), buffer_length - offset, &mut standard_written as *mut u32);
                if status != STATUS_SUCCESS {
                    return status;
                }
                offset += standard_written;
                
                *bytes_written = offset;
                STATUS_SUCCESS
            }
            _ => STATUS_INVALID_PARAMETER,
        }
    }
}

/// FILE_BASIC_INFORMATION
unsafe fn query_basic_information(
    fcb: *mut FAT32_FCB,
    buffer: *mut u8,
    buffer_length: u32,
    bytes_written: *mut u32,
) -> NTSTATUS {
    unsafe {
        if buffer_length < core::mem::size_of::<FILE_BASIC_INFORMATION>() as u32 {
            return STATUS_BUFFER_TOO_SMALL;
        }
        
        let info = buffer as *mut FILE_BASIC_INFORMATION;
        
        // Получаем timestamps из FCB
        // FAT32 хранит timestamps в directory entry, но мы пока возвращаем текущее время
        let current_time = crate::time::get_current_filetime();
        (*info).creation_time = current_time;
        (*info).last_access_time = current_time;
        (*info).last_write_time = current_time;
        (*info).change_time = current_time;
        
        // Конвертируем FAT attributes в NT attributes
        let mut attrs = 0u32;
        
        if (*fcb).attributes & 0x01 != 0 {
            attrs |= FILE_ATTRIBUTE_READONLY;
        }
        if (*fcb).attributes & 0x02 != 0 {
            attrs |= FILE_ATTRIBUTE_HIDDEN;
        }
        if (*fcb).attributes & 0x04 != 0 {
            attrs |= FILE_ATTRIBUTE_SYSTEM;
        }
        if (*fcb).attributes & 0x10 != 0 {
            attrs |= FILE_ATTRIBUTE_DIRECTORY;
        }
        if (*fcb).attributes & 0x20 != 0 {
            attrs |= FILE_ATTRIBUTE_ARCHIVE;
        }
        
        if attrs == 0 {
            attrs = FILE_ATTRIBUTE_NORMAL;
        }
        
        (*info).file_attributes = attrs;
        
        *bytes_written = core::mem::size_of::<FILE_BASIC_INFORMATION>() as u32;
        STATUS_SUCCESS
    }
}

/// FILE_STANDARD_INFORMATION
unsafe fn query_standard_information(
    fcb: *mut FAT32_FCB,
    buffer: *mut u8,
    buffer_length: u32,
    bytes_written: *mut u32,
) -> NTSTATUS {
    unsafe {
        if buffer_length < core::mem::size_of::<FILE_STANDARD_INFORMATION>() as u32 {
            return STATUS_BUFFER_TOO_SMALL;
        }
        
        let info = buffer as *mut FILE_STANDARD_INFORMATION;
        let vcb = (*fcb).vcb;
        
        // Allocation size = округлено до размера кластера
        let cluster_size = (*vcb).bytes_per_cluster as i64;
        let file_size = (*fcb).file_size as i64;
        let allocation_size = if file_size > 0 {
            ((file_size + cluster_size - 1) / cluster_size) * cluster_size
        } else {
            0
        };
        
        (*info).allocation_size = allocation_size;
        (*info).end_of_file = file_size;
        (*info).number_of_links = 1; // FAT32 не поддерживает hard links
        (*info).delete_pending = if (*fcb).flags & 0x100 != 0 { 1 } else { 0 };
        (*info).directory = if (*fcb).is_directory() { 1 } else { 0 };
        
        *bytes_written = core::mem::size_of::<FILE_STANDARD_INFORMATION>() as u32;
        STATUS_SUCCESS
    }
}

/// FILE_POSITION_INFORMATION
unsafe fn query_position_information(
    ccb: *mut crate::fcb::FAT32_CCB,
    buffer: *mut u8,
    buffer_length: u32,
    bytes_written: *mut u32,
) -> NTSTATUS {
    unsafe {
        if ccb.is_null() {
            return STATUS_INVALID_PARAMETER;
        }
        
        if buffer_length < core::mem::size_of::<FILE_POSITION_INFORMATION>() as u32 {
            return STATUS_BUFFER_TOO_SMALL;
        }
        
        let info = buffer as *mut FILE_POSITION_INFORMATION;
        (*info).current_byte_offset = (*ccb).current_position as i64;
        
        *bytes_written = core::mem::size_of::<FILE_POSITION_INFORMATION>() as u32;
        STATUS_SUCCESS
    }
}

/// FILE_NAME_INFORMATION
unsafe fn query_name_information(
    fcb: *mut FAT32_FCB,
    buffer: *mut u8,
    buffer_length: u32,
    bytes_written: *mut u32,
) -> NTSTATUS {
    unsafe {
        if buffer_length < core::mem::size_of::<FILE_NAME_INFORMATION>() as u32 {
            return STATUS_BUFFER_TOO_SMALL;
        }
        
        let info = buffer as *mut FILE_NAME_INFORMATION;
        
        // Конвертируем 8.3 имя в Unicode
        let mut name_len = 0u32;
        for i in 0..11 {
            let c = (*fcb).file_name[i];
            if c != 0 && c != b' ' {
                if name_len < 260 {
                    (*info).file_name[name_len as usize] = c as u16;
                    name_len += 1;
                }
            }
        }
        
        (*info).file_name_length = name_len * 2; // Байты
        
        *bytes_written = 4 + name_len * 2; // file_name_length + actual name
        STATUS_SUCCESS
    }
}

// =============================================================================
// File Attributes
// =============================================================================

pub const FILE_ATTRIBUTE_READONLY: u32 = 0x00000001;
pub const FILE_ATTRIBUTE_HIDDEN: u32 = 0x00000002;
pub const FILE_ATTRIBUTE_SYSTEM: u32 = 0x00000004;
pub const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x00000010;
pub const FILE_ATTRIBUTE_ARCHIVE: u32 = 0x00000020;
pub const FILE_ATTRIBUTE_NORMAL: u32 = 0x00000080;

// =============================================================================
// Status Codes
// =============================================================================

pub const STATUS_BUFFER_TOO_SMALL: NTSTATUS = 0xC0000023u32 as i32;
pub const STATUS_NOT_IMPLEMENTED: NTSTATUS = 0xC0000002u32 as i32;

