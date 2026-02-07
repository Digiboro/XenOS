//! Directory Entry Updates
//!
//! Обновление directory entries на диске после модификации файлов.

use crate::types::*;
use crate::vcb::FAT32_VCB;
use crate::fcb::FAT32_FCB;
use crate::fat::{fat_read_cluster_chain, fat_write_cluster_chain};
use crate::time::{get_current_filetime, filetime_to_dos_datetime};

/// Обновляет directory entry для файла на диске
///
/// Обновляет: file_size, first_cluster, timestamps
pub unsafe fn fat_update_directory_entry(
    vcb: *mut FAT32_VCB,
    fcb: *mut FAT32_FCB,
    parent_dir_cluster: u32,
) -> NTSTATUS {
    unsafe {
        if fcb.is_null() || parent_dir_cluster < 2 {
            return STATUS_INVALID_PARAMETER;
        }
        
        let vcb_ref = &*vcb;
        let cluster_size = vcb_ref.bytes_per_cluster;
        
        // Выделяем буфер для directory
        let buffer = ExAllocatePoolWithTag(
            NON_PAGED_POOL,
            cluster_size as usize,
            FAT32_POOL_TAG,
        );
        
        if buffer.is_null() {
            return STATUS_INSUFFICIENT_RESOURCES;
        }
        
        // Читаем directory
        let bytes_read = fat_read_cluster_chain(
            vcb,
            parent_dir_cluster,
            0,
            buffer as *mut u8,
            cluster_size,
        );
        
        let status = match bytes_read {
            Ok(read) if read > 0 => {
                let num_entries = read / 32;
                let mut found = false;
                
                // Ищем entry по имени
                for i in 0..num_entries {
                    let entry_offset = (i * 32) as usize;
                    let entry_ptr = (buffer as *mut u8).add(entry_offset);
                    
                    let name_byte0 = *entry_ptr;
                    let attr = *entry_ptr.add(11);
                    
                    if name_byte0 == 0x00 {
                        break;
                    }
                    
                    if name_byte0 == 0xE5 || attr == 0x0F {
                        continue;
                    }
                    
                    // Сравниваем имя
                    let mut name_match = true;
                    for j in 0..11 {
                        if *entry_ptr.add(j) != (*fcb).file_name[j] {
                            name_match = false;
                            break;
                        }
                    }
                    
                    if name_match {
                        // Нашли! Обновляем поля
                        
                        // First cluster
                        let first_cluster = (*fcb).first_cluster;
                        *entry_ptr.add(26) = (first_cluster & 0xFF) as u8;
                        *entry_ptr.add(27) = ((first_cluster >> 8) & 0xFF) as u8;
                        *entry_ptr.add(20) = ((first_cluster >> 16) & 0xFF) as u8;
                        *entry_ptr.add(21) = ((first_cluster >> 24) & 0xFF) as u8;
                        
                        // File size (только для файлов, не для директорий)
                        if !(*fcb).is_directory() {
                            let file_size = (*fcb).file_size;
                            *entry_ptr.add(28) = (file_size & 0xFF) as u8;
                            *entry_ptr.add(29) = ((file_size >> 8) & 0xFF) as u8;
                            *entry_ptr.add(30) = ((file_size >> 16) & 0xFF) as u8;
                            *entry_ptr.add(31) = ((file_size >> 24) & 0xFF) as u8;
                        }
                        
                        // Timestamps - обновляем write time
                        let current_time = get_current_filetime();
                        let (date, time) = filetime_to_dos_datetime(current_time);
                        
                        *entry_ptr.add(22) = (time & 0xFF) as u8;
                        *entry_ptr.add(23) = ((time >> 8) & 0xFF) as u8;
                        *entry_ptr.add(24) = (date & 0xFF) as u8;
                        *entry_ptr.add(25) = ((date >> 8) & 0xFF) as u8;
                        
                        found = true;
                        break;
                    }
                }
                
                if found {
                    // Записываем обратно
                    match fat_write_cluster_chain(vcb, parent_dir_cluster, 0, buffer as *const u8, cluster_size) {
                        Ok(_) => STATUS_SUCCESS,
                        Err(e) => e,
                    }
                } else {
                    STATUS_OBJECT_NAME_NOT_FOUND
                }
            }
            _ => STATUS_UNSUCCESSFUL,
        };
        
        ExFreePoolWithTag(buffer, FAT32_POOL_TAG);
        status
    }
}

const STATUS_INVALID_PARAMETER: NTSTATUS = 0xC000000Du32 as i32;
const STATUS_INSUFFICIENT_RESOURCES: NTSTATUS = 0xC000009Au32 as i32;
const STATUS_OBJECT_NAME_NOT_FOUND: NTSTATUS = 0xC0000034u32 as i32;
const STATUS_UNSUCCESSFUL: NTSTATUS = 0xC0000001u32 as i32;

