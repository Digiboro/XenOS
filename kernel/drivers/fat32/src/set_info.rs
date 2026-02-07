//! Set File Information
//!
//! Реализация IRP_MJ_SET_INFORMATION для изменения метаданных файлов.

use crate::types::*;
use crate::fcb::{FAT32_FCB, FCB_FLAGS_DELETE_ON_CLOSE};

// =============================================================================
// File Information Classes
// =============================================================================

pub const FILE_BASIC_INFORMATION: u32 = 4;
pub const FILE_DISPOSITION_INFORMATION: u32 = 13;
pub const FILE_END_OF_FILE_INFORMATION: u32 = 20;
pub const FILE_ALLOCATION_INFORMATION: u32 = 19;

// =============================================================================
// Structures
// =============================================================================

/// FILE_DISPOSITION_INFORMATION
#[repr(C)]
pub struct FILE_DISPOSITION_INFORMATION {
    pub delete_file: u8,
}

/// FILE_END_OF_FILE_INFORMATION
#[repr(C)]
pub struct FILE_END_OF_FILE_INFORMATION {
    pub end_of_file: i64,
}

// =============================================================================
// Set Information Handler
// =============================================================================

/// Обрабатывает IRP_MJ_SET_INFORMATION
pub unsafe fn fat_set_information(
    fcb: *mut FAT32_FCB,
    file_info_class: u32,
    buffer: *const u8,
    buffer_length: u32,
) -> NTSTATUS {
    unsafe {
        if fcb.is_null() || buffer.is_null() {
            return STATUS_INVALID_PARAMETER;
        }
        
        match file_info_class {
            FILE_DISPOSITION_INFORMATION => {
                set_disposition_information(fcb, buffer, buffer_length)
            }
            FILE_END_OF_FILE_INFORMATION => {
                set_end_of_file_information(fcb, buffer, buffer_length)
            }
            FILE_ALLOCATION_INFORMATION => {
                // Аналогично END_OF_FILE
                set_end_of_file_information(fcb, buffer, buffer_length)
            }
            FILE_BASIC_INFORMATION => {
                // Изменение timestamps и attributes
                // Пока не реализовано, возвращаем SUCCESS
                STATUS_SUCCESS
            }
            _ => STATUS_INVALID_PARAMETER,
        }
    }
}

/// FILE_DISPOSITION_INFORMATION - пометка на удаление
unsafe fn set_disposition_information(
    fcb: *mut FAT32_FCB,
    buffer: *const u8,
    buffer_length: u32,
) -> NTSTATUS {
    unsafe {
        if buffer_length < core::mem::size_of::<FILE_DISPOSITION_INFORMATION>() as u32 {
            return STATUS_BUFFER_TOO_SMALL;
        }
        
        let info = buffer as *const FILE_DISPOSITION_INFORMATION;
        
        if (*info).delete_file != 0 {
            // Помечаем файл на удаление при закрытии
            (*fcb).flags |= FCB_FLAGS_DELETE_ON_CLOSE;
        } else {
            // Снимаем флаг
            (*fcb).flags &= !FCB_FLAGS_DELETE_ON_CLOSE;
        }
        
        STATUS_SUCCESS
    }
}

/// FILE_END_OF_FILE_INFORMATION - изменение размера файла
unsafe fn set_end_of_file_information(
    fcb: *mut FAT32_FCB,
    buffer: *const u8,
    buffer_length: u32,
) -> NTSTATUS {
    unsafe {
        if buffer_length < core::mem::size_of::<FILE_END_OF_FILE_INFORMATION>() as u32 {
            return STATUS_BUFFER_TOO_SMALL;
        }
        
        let info = buffer as *const FILE_END_OF_FILE_INFORMATION;
        let new_size = (*info).end_of_file;
        
        if new_size < 0 {
            return STATUS_INVALID_PARAMETER;
        }
        
        let current_size = (*fcb).file_size as i64;
        
        if new_size > current_size {
            // Расширить файл
            let vcb = (*fcb).vcb;
            let status = crate::fat::fat_expand_file_size(vcb, fcb, new_size as u64);
            if status < 0 {
                return status;
            }
        } else if new_size < current_size {
            // Truncate файл
            // Освобождаем лишние clusters
            let vcb = (*fcb).vcb;
            let vcb_ref = &*vcb;
            let cluster_size = vcb_ref.bytes_per_cluster as u64;
            
            let old_clusters = ((current_size as u64 + cluster_size - 1) / cluster_size) as u32;
            let new_clusters = ((new_size as u64 + cluster_size - 1) / cluster_size) as u32;
            
            if new_clusters < old_clusters && (*fcb).first_cluster >= 2 {
                // Находим cluster где нужно обрезать
                let mut current = (*fcb).first_cluster;
                
                for _ in 0..new_clusters.saturating_sub(1) {
                    match crate::fat::fat_read_next_cluster(vcb, current) {
                        Ok(next) if crate::fat::is_valid_cluster(next) => {
                            current = next;
                        }
                        _ => break,
                    }
                }
                
                // Обрезаем chain после этого кластера
                match crate::fat::fat_read_next_cluster(vcb, current) {
                    Ok(next_to_free) if crate::fat::is_valid_cluster(next_to_free) => {
                        // Помечаем current как EOC
                        let _ = crate::fat::fat_write_fat_entry(vcb, current, crate::bpb::FAT32_EOC_MIN);
                        // Освобождаем остальные
                        let _ = crate::fat::fat_free_chain(vcb, next_to_free);
                    }
                    _ => {}
                }
            }
            
            (*fcb).file_size = new_size as u32;
            (*fcb).flags |= 0x04; // FCB_FLAGS_MODIFIED
        }
        
        STATUS_SUCCESS
    }
}

const STATUS_BUFFER_TOO_SMALL: NTSTATUS = 0xC0000023u32 as i32;
const STATUS_INVALID_PARAMETER: NTSTATUS = 0xC000000Du32 as i32;

