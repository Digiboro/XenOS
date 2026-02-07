//! File and Directory Deletion
//!
//! Удаление файлов и директорий из FAT32.

use crate::types::*;
use crate::vcb::FAT32_VCB;
use crate::fcb::FAT32_FCB;
use crate::fat::{fat_read_cluster_chain, fat_free_chain, fat_write_cluster_chain};
use crate::bpb::ATTR_DIRECTORY;

/// Удаляет файл или директорию
///
/// # Arguments
/// * `vcb` - Volume Control Block
/// * `parent_dir_cluster` - cluster родительской директории
/// * `file_name` - имя файла (8.3 формат)
/// * `fcb` - FCB файла для удаления
///
/// # Returns
/// * STATUS_SUCCESS если удалён успешно
pub unsafe fn fat_delete_file(
    vcb: *mut FAT32_VCB,
    parent_dir_cluster: u32,
    file_name: &[u8; 11],
    fcb: *mut FAT32_FCB,
) -> NTSTATUS {
    unsafe {
        // Проверяем reference count
        if (*fcb).reference_count > 1 {
            return STATUS_SHARING_VIOLATION; // Файл открыт
        }
        
        // Если это директория, проверяем что она пуста
        if (*fcb).is_directory() && !(*fcb).is_root_directory() {
            if !is_directory_empty(vcb, (*fcb).first_cluster) {
                return STATUS_DIRECTORY_NOT_EMPTY;
            }
        }
        
        // Освобождаем cluster chain
        if (*fcb).first_cluster >= 2 {
            if let Err(e) = fat_free_chain(vcb, (*fcb).first_cluster) {
                return e;
            }
        }
        
        // Помечаем directory entry как deleted
        if let Err(e) = mark_directory_entry_deleted(vcb, parent_dir_cluster, file_name) {
            return e;
        }
        
        STATUS_SUCCESS
    }
}

/// Проверяет что директория пуста (содержит только . и ..)
unsafe fn is_directory_empty(vcb: *mut FAT32_VCB, dir_cluster: u32) -> bool {
    unsafe {
        let vcb_ref = &*vcb;
        let cluster_size = vcb_ref.bytes_per_cluster;
        
        let buffer = ExAllocatePoolWithTag(
            NON_PAGED_POOL,
            cluster_size as usize,
            FAT32_POOL_TAG,
        );
        
        if buffer.is_null() {
            return false;
        }
        
        let bytes_read = fat_read_cluster_chain(
            vcb,
            dir_cluster,
            0,
            buffer as *mut u8,
            cluster_size,
        );
        
        let is_empty = match bytes_read {
            Ok(read) if read > 0 => {
                let num_entries = read / 32;
                
                for i in 0..num_entries {
                    let entry_offset = (i * 32) as isize;
                    let entry_ptr = (buffer as *const u8).offset(entry_offset);
                    
                    let name_byte0 = *entry_ptr;
                    let attr = *entry_ptr.add(11);
                    
                    // Конец директории
                    if name_byte0 == 0x00 {
                        break;
                    }
                    
                    // Пропускаем удалённые
                    if name_byte0 == 0xE5 {
                        continue;
                    }
                    
                    // Пропускаем LFN entries
                    if attr == 0x0F {
                        continue;
                    }
                    
                    // Пропускаем volume labels
                    if (attr & 0x08) != 0 {
                        continue;
                    }
                    
                    // Пропускаем . и ..
                    let name_byte1 = *entry_ptr.add(1);
                    let name_byte2 = *entry_ptr.add(2);
                    if name_byte0 == b'.' && (name_byte1 == b' ' || 
                       (name_byte1 == b'.' && name_byte2 == b' ')) {
                        continue;
                    }
                    
                    // Нашли реальный entry - директория не пуста
                    ExFreePoolWithTag(buffer, FAT32_POOL_TAG);
                    return false;
                }
                
                true // Директория пуста
            }
            _ => false,
        };
        
        ExFreePoolWithTag(buffer, FAT32_POOL_TAG);
        is_empty
    }
}

/// Помечает directory entry как удалённый (0xE5)
unsafe fn mark_directory_entry_deleted(
    vcb: *mut FAT32_VCB,
    dir_cluster: u32,
    file_name: &[u8; 11],
) -> Result<(), NTSTATUS> {
    unsafe {
        let vcb_ref = &*vcb;
        let cluster_size = vcb_ref.bytes_per_cluster;
        
        let buffer = ExAllocatePoolWithTag(
            NON_PAGED_POOL,
            cluster_size as usize,
            FAT32_POOL_TAG,
        );
        
        if buffer.is_null() {
            return Err(STATUS_INSUFFICIENT_RESOURCES);
        }
        
        // Читаем directory
        let bytes_read = fat_read_cluster_chain(
            vcb,
            dir_cluster,
            0,
            buffer as *mut u8,
            cluster_size,
        );
        
        let result = match bytes_read {
            Ok(read) if read > 0 => {
                let num_entries = read / 32;
                let mut found = false;
                
                for i in 0..num_entries {
                    let entry_offset = (i * 32) as isize;
                    let entry_ptr = (buffer as *mut u8).offset(entry_offset);
                    
                    let name_byte0 = *entry_ptr;
                    let attr = *entry_ptr.add(11);
                    
                    if name_byte0 == 0x00 {
                        break;
                    }
                    
                    // Пропускаем уже удалённые
                    if name_byte0 == 0xE5 {
                        continue;
                    }
                    
                    // Пропускаем LFN и volume labels
                    if attr == 0x0F || (attr & 0x08) != 0 {
                        continue;
                    }
                    
                    // Проверяем имя
                    let mut name_match = true;
                    for j in 0..11 {
                        if *entry_ptr.add(j) != file_name[j] {
                            name_match = false;
                            break;
                        }
                    }
                    
                    if name_match {
                        // Нашли! Помечаем как удалённый
                        *entry_ptr = 0xE5;
                        found = true;
                        break;
                    }
                }
                
                if found {
                    // Записываем обратно
                    let write_result = fat_write_cluster_chain(
                        vcb,
                        dir_cluster,
                        0,
                        buffer as *const u8,
                        cluster_size,
                    );
                    
                    match write_result {
                        Ok(_) => Ok(()),
                        Err(e) => Err(e),
                    }
                } else {
                    Err(STATUS_OBJECT_NAME_NOT_FOUND)
                }
            }
            _ => Err(STATUS_UNSUCCESSFUL),
        };
        
        ExFreePoolWithTag(buffer, FAT32_POOL_TAG);
        result
    }
}

// Status codes
const STATUS_SHARING_VIOLATION: NTSTATUS = 0xC0000043u32 as i32;
const STATUS_DIRECTORY_NOT_EMPTY: NTSTATUS = 0xC0000101u32 as i32;
const STATUS_OBJECT_NAME_NOT_FOUND: NTSTATUS = 0xC0000034u32 as i32;
const STATUS_INSUFFICIENT_RESOURCES: NTSTATUS = 0xC000009Au32 as i32;
const STATUS_UNSUCCESSFUL: NTSTATUS = 0xC0000001u32 as i32;

