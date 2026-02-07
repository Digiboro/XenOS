//! File and Directory Creation
//!
//! Создание новых файлов и директорий в FAT32.

use crate::types::*;
use crate::vcb::FAT32_VCB;
use crate::fcb::FAT32_FCB;
use crate::bpb::ATTR_DIRECTORY;
use crate::fat::{fat_read_cluster_chain, fat_allocate_cluster, fat_write_cluster_chain};
use crate::time::{get_current_filetime, filetime_to_dos_datetime};

/// Создаёт новый файл или директорию
///
/// # Arguments
/// * `vcb` - Volume Control Block
/// * `parent_dir_cluster` - cluster родительской директории
/// * `file_name` - имя файла (8.3 формат)
/// * `is_directory` - создать директорию?
/// * `fcb_out` - выходной FCB
///
/// # Returns
/// * STATUS_SUCCESS если создан успешно
pub unsafe fn fat_create_file(
    vcb: *mut FAT32_VCB,
    parent_dir_cluster: u32,
    file_name: &[u8; 11],
    is_directory: bool,
    fcb_out: *mut *mut FAT32_FCB,
) -> NTSTATUS {
    unsafe {
        let vcb_ref = &*vcb;
        
        // 1. Найти свободный slot в родительской директории
        let (dir_entry_cluster, dir_entry_offset) = match find_free_directory_slot(vcb, parent_dir_cluster) {
            Ok((cluster, offset)) => (cluster, offset),
            Err(e) => return e,
        };
        
        // 2. Выделить cluster для файла/директории (если нужно)
        let first_cluster = if is_directory {
            // Директория всегда нуждается в кластере для . и ..
            match fat_allocate_cluster(vcb) {
                Ok(cluster) => {
                    // Инициализируем директорию с . и .. entries
                    if let Err(e) = initialize_directory(vcb, cluster, parent_dir_cluster) {
                        return e;
                    }
                    cluster
                }
                Err(e) => return e,
            }
        } else {
            0 // Пустой файл не нуждается в кластере
        };
        
        // 3. Создать directory entry
        let mut entry = create_directory_entry_struct(
            file_name,
            first_cluster,
            0, // size = 0 для нового файла
            is_directory,
        );
        
        // 4. Записать entry на диск
        if let Err(e) = write_directory_entry(vcb, dir_entry_cluster, dir_entry_offset, &entry) {
            // Откатываем allocation
            if first_cluster > 0 {
                let _ = crate::fat::fat_free_chain(vcb, first_cluster);
            }
            return e;
        }
        
        // 5. Создать FCB для нового файла
        let fcb = ExAllocatePoolWithTag(
            NON_PAGED_POOL,
            core::mem::size_of::<FAT32_FCB>(),
            FAT32_POOL_TAG,
        ) as *mut FAT32_FCB;
        
        if fcb.is_null() {
            return STATUS_INSUFFICIENT_RESOURCES;
        }
        
        core::ptr::write(fcb, FAT32_FCB::new(vcb));
        (*fcb).first_cluster = first_cluster;
        (*fcb).file_size = 0;
        (*fcb).attributes = if is_directory { ATTR_DIRECTORY } else { 0x20 }; // ARCHIVE
        (*fcb).parent_dir_cluster = parent_dir_cluster;
        
        // Копируем имя
        (&mut (*fcb).file_name)[..11].copy_from_slice(file_name);
        (*fcb).file_name[11] = 0;
        
        *fcb_out = fcb;
        STATUS_SUCCESS
    }
}

/// Находит свободный slot в директории
unsafe fn find_free_directory_slot(
    vcb: *mut FAT32_VCB,
    dir_cluster: u32,
) -> Result<(u32, u32), NTSTATUS> {
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
                
                // Ищем свободный slot (0x00 или 0xE5)
                for i in 0..num_entries {
                    let entry_offset = i * 32;
                    let entry_ptr = (buffer as *const u8).add(entry_offset as usize);
                    let first_byte = *entry_ptr;
                    
                    if first_byte == 0x00 || first_byte == 0xE5 {
                        // Нашли свободный slot
                        ExFreePoolWithTag(buffer, FAT32_POOL_TAG);
                        return Ok((dir_cluster, entry_offset));
                    }
                }
                
                // Нет свободных slots в текущем кластере
                // Расширяем директорию новым кластером
                match crate::fat::fat_extend_chain(vcb, dir_cluster) {
                    Ok(new_cluster) => {
                        // Новый кластер выделен, первый entry в нём свободен
                        Ok((new_cluster, 0))
                    }
                    Err(e) => Err(e)
                }
            }
            _ => Err(STATUS_UNSUCCESSFUL),
        };
        
        ExFreePoolWithTag(buffer, FAT32_POOL_TAG);
        result
    }
}

/// Инициализирует новую директорию с . и .. entries
unsafe fn initialize_directory(
    vcb: *mut FAT32_VCB,
    dir_cluster: u32,
    parent_cluster: u32,
) -> Result<(), NTSTATUS> {
    unsafe {
        let vcb_ref = &*vcb;
        let cluster_size = vcb_ref.bytes_per_cluster;
        
        // Выделяем буфер для кластера
        let buffer = ExAllocatePoolWithTag(
            NON_PAGED_POOL,
            cluster_size as usize,
            FAT32_POOL_TAG,
        );
        
        if buffer.is_null() {
            return Err(STATUS_INSUFFICIENT_RESOURCES);
        }
        
        // Очищаем буфер
        core::ptr::write_bytes(buffer as *mut u8, 0, cluster_size as usize);
        
        // Создаём . entry (ссылка на саму директорию)
        let dot_entry = create_directory_entry_struct(
            b".          ",
            dir_cluster,
            0,
            true,
        );
        core::ptr::copy_nonoverlapping(
            &dot_entry as *const _ as *const u8,
            buffer as *mut u8,
            32,
        );
        
        // Создаём .. entry (ссылка на родительскую директорию)
        let dotdot_entry = create_directory_entry_struct(
            b"..         ",
            parent_cluster,
            0,
            true,
        );
        core::ptr::copy_nonoverlapping(
            &dotdot_entry as *const _ as *const u8,
            (buffer as *mut u8).add(32),
            32,
        );
        
        // Записываем кластер на диск
        let status = fat_write_cluster_chain(
            vcb,
            dir_cluster,
            0,
            buffer as *const u8,
            cluster_size,
        );
        
        ExFreePoolWithTag(buffer, FAT32_POOL_TAG);
        
        match status {
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        }
    }
}

/// Создаёт структуру directory entry
fn create_directory_entry_struct(
    name: &[u8; 11],
    first_cluster: u32,
    file_size: u32,
    is_directory: bool,
) -> [u8; 32] {
    let mut entry = [0u8; 32];
    
    // Имя (11 bytes)
    entry[..11].copy_from_slice(name);
    
    // Атрибуты
    entry[11] = if is_directory { ATTR_DIRECTORY } else { 0x20 }; // ARCHIVE
    
    // NT Reserved
    entry[12] = 0;
    
    // Timestamps (используем текущее время)
    let current_time = get_current_filetime();
    let (date, time) = filetime_to_dos_datetime(current_time);
    
    // Creation time
    entry[14] = (time & 0xFF) as u8;
    entry[15] = ((time >> 8) & 0xFF) as u8;
    entry[16] = (date & 0xFF) as u8;
    entry[17] = ((date >> 8) & 0xFF) as u8;
    
    // Last access date
    entry[18] = (date & 0xFF) as u8;
    entry[19] = ((date >> 8) & 0xFF) as u8;
    
    // First cluster high
    entry[20] = ((first_cluster >> 16) & 0xFF) as u8;
    entry[21] = ((first_cluster >> 24) & 0xFF) as u8;
    
    // Write time
    entry[22] = (time & 0xFF) as u8;
    entry[23] = ((time >> 8) & 0xFF) as u8;
    entry[24] = (date & 0xFF) as u8;
    entry[25] = ((date >> 8) & 0xFF) as u8;
    
    // First cluster low
    entry[26] = (first_cluster & 0xFF) as u8;
    entry[27] = ((first_cluster >> 8) & 0xFF) as u8;
    
    // File size (директории имеют size = 0)
    let size_to_write = if is_directory { 0 } else { file_size };
    entry[28] = (size_to_write & 0xFF) as u8;
    entry[29] = ((size_to_write >> 8) & 0xFF) as u8;
    entry[30] = ((size_to_write >> 16) & 0xFF) as u8;
    entry[31] = ((size_to_write >> 24) & 0xFF) as u8;
    
    entry
}

/// Записывает directory entry на диск
unsafe fn write_directory_entry(
    vcb: *mut FAT32_VCB,
    dir_cluster: u32,
    entry_offset: u32,
    entry_data: &[u8; 32],
) -> Result<(), NTSTATUS> {
    unsafe {
        let vcb_ref = &*vcb;
        let cluster_size = vcb_ref.bytes_per_cluster;
        
        // Читаем весь кластер
        let buffer = ExAllocatePoolWithTag(
            NON_PAGED_POOL,
            cluster_size as usize,
            FAT32_POOL_TAG,
        );
        
        if buffer.is_null() {
            return Err(STATUS_INSUFFICIENT_RESOURCES);
        }
        
        // Читаем текущее содержимое
        let read_result = fat_read_cluster_chain(
            vcb,
            dir_cluster,
            0,
            buffer as *mut u8,
            cluster_size,
        );
        
        if read_result.is_err() {
            ExFreePoolWithTag(buffer, FAT32_POOL_TAG);
            return Err(STATUS_UNSUCCESSFUL);
        }
        
        // Модифицируем entry
        let entry_ptr = (buffer as *mut u8).add(entry_offset as usize);
        core::ptr::copy_nonoverlapping(
            entry_data.as_ptr(),
            entry_ptr,
            32,
        );
        
        // Записываем обратно
        let write_result = fat_write_cluster_chain(
            vcb,
            dir_cluster,
            0,
            buffer as *const u8,
            cluster_size,
        );
        
        ExFreePoolWithTag(buffer, FAT32_POOL_TAG);
        
        match write_result {
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        }
    }
}

// Константы (используем из bpb)
// const ATTR_DIRECTORY уже определён в bpb.rs

