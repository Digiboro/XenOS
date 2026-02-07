//! Directory Operations
//!
//! Модуль для чтения директорий и поиска файлов.

use crate::types::*;
use crate::bpb::{FAT32_BPB, FAT32DirEntry, ATTR_VOLUME_ID, ATTR_DIRECTORY};
use crate::vcb::FAT32_VCB;
use crate::fat::{fat_read_cluster_chain, fat_read_next_cluster, is_valid_cluster};
use crate::fcb::{FAT32_FCB, FCB_FLAGS_ROOT_DIRECTORY};

/// Разбивает путь на компоненты
///
/// Пример: "XenOS\boot\file.sys" -> ["XenOS", "boot", "file.sys"]
fn split_path(path: &[u16], path_len: usize) -> ([&[u16]; 16], usize) {
    let mut components: [&[u16]; 16] = [&[]; 16];
    let mut count = 0usize;
    let mut start = 0usize;
    
    // Пропускаем начальный '\'
    if path_len > 0 && path[0] == b'\\' as u16 {
        start = 1;
    }
    
    let mut i = start;
    while i <= path_len && count < 16 {
        // Ищем следующий '\' или конец строки
        if i == path_len || path[i] == b'\\' as u16 || path[i] == 0 {
            if i > start {
                components[count] = &path[start..i];
                count += 1;
            }
            start = i + 1;
        }
        i += 1;
    }
    
    (components, count)
}

/// Ищет файл в директории по имени (8.3 формат)
///
/// # Arguments
/// * `vcb` - Volume Control Block
/// * `dir_cluster` - cluster директории (или 0 для root)
/// * `file_name` - имя файла в формате 8.3 (например "BOOT    SYS")
/// * `fcb_out` - выходной FCB для найденного файла
///
/// # Returns
/// * STATUS_SUCCESS если файл найден
/// * STATUS_OBJECT_NAME_NOT_FOUND если файл не найден
pub unsafe fn fat_find_file(
    vcb: *mut FAT32_VCB,
    dir_cluster: u32,
    file_name: &[u8; 11],
    fcb_out: *mut *mut FAT32_FCB,
) -> NTSTATUS {
    unsafe {
        let vcb_ref = &*vcb;
        
        // Определяем cluster для поиска
        let search_cluster = if dir_cluster == 0 {
            vcb_ref.root_directory_cluster
        } else {
            dir_cluster
        };
        
        if search_cluster < 2 {
            return STATUS_INVALID_PARAMETER;
        }
        
        // Выделяем буфер для чтения cluster
        let cluster_size = vcb_ref.bytes_per_cluster;
        let buffer = ExAllocatePoolWithTag(
            NON_PAGED_POOL,
            cluster_size as usize,
            FAT32_POOL_TAG,
        );
        
        if buffer.is_null() {
            return STATUS_INSUFFICIENT_RESOURCES;
        }
        
        // Итерируемся по clusters в directory
        let mut current_cluster = search_cluster;
        let mut result = Err(STATUS_OBJECT_NAME_NOT_FOUND);
        
        'outer: loop {
            // Читаем cluster
            let bytes_read = fat_read_cluster_chain(
                vcb,
                current_cluster,
                0, // читаем с начала cluster
                buffer as *mut u8,
                cluster_size,
            );
            
            match bytes_read {
                Ok(read) if read > 0 => {
                    // Сканируем directory entries в этом cluster
                    let num_entries = read / FAT32DirEntry::SIZE as u32;
                    
                    for i in 0..num_entries {
                        let entry_offset = (i * FAT32DirEntry::SIZE as u32) as isize;
                        let entry_ptr = (buffer as *const u8).offset(entry_offset) 
                            as *const FAT32DirEntry;
                        let entry = &*entry_ptr;
                        
                        // Проверяем конец директории
                        if entry.name[0] == 0x00 {
                            break 'outer; // Конец directory
                        }
                        
                        // Пропускаем удалённые entries
                        if entry.name[0] == 0xE5 {
                            continue;
                        }
                        
                        // Пропускаем LFN entries
                        if entry.is_lfn() {
                            continue;
                        }
                        
                        // Пропускаем volume labels
                        if (entry.attr & ATTR_VOLUME_ID) != 0 {
                            continue;
                        }
                        
                        // Сравниваем имена (case-insensitive)
                        if names_match(&entry.name, file_name) {
                            // Нашли файл! Создаём FCB
                            let fcb = ExAllocatePoolWithTag(
                                NON_PAGED_POOL,
                                core::mem::size_of::<FAT32_FCB>(),
                                FAT32_POOL_TAG,
                            ) as *mut FAT32_FCB;
                            
                            if fcb.is_null() {
                                result = Err(STATUS_INSUFFICIENT_RESOURCES);
                                break 'outer;
                            }
                            
                            // Инициализируем FCB
                            core::ptr::write(fcb, FAT32_FCB::new(vcb));
                            (*fcb).first_cluster = entry.first_cluster();
                            (*fcb).file_size = entry.file_size;
                            (*fcb).attributes = entry.attr;
                            (*fcb).parent_dir_cluster = search_cluster;
                            
                            // Копируем имя
                            (&mut (*fcb).file_name)[..11].copy_from_slice(&entry.name);
                            (*fcb).file_name[11] = 0; // NULL terminator
                            
                            *fcb_out = fcb;
                            result = Ok(STATUS_SUCCESS);
                            break 'outer;
                        }
                    }
                }
                Err(status) => {
                    result = Err(status);
                    break 'outer;
                }
                _ => {}
            }
            
            // Переходим к следующему cluster в directory
            let next = fat_read_next_cluster(vcb, current_cluster);
            match next {
                Ok(next_cluster) if is_valid_cluster(next_cluster) => {
                    current_cluster = next_cluster;
                }
                _ => break 'outer, // Конец directory chain
            }
        }
        
        // Освобождаем буфер
        ExFreePoolWithTag(buffer, FAT32_POOL_TAG);
        
        match result {
            Ok(status) => status,
            Err(status) => status,
        }
    }
}

/// Создаёт FCB для root directory
pub unsafe fn fat_create_root_fcb(vcb: *mut FAT32_VCB) -> *mut FAT32_FCB {
    unsafe {
        let vcb_ref = &*vcb;
        
        let fcb = ExAllocatePoolWithTag(
            NON_PAGED_POOL,
            core::mem::size_of::<FAT32_FCB>(),
            FAT32_POOL_TAG,
        ) as *mut FAT32_FCB;
        
        if fcb.is_null() {
            return core::ptr::null_mut();
        }
        
        // Инициализируем FCB для root directory
        core::ptr::write(fcb, FAT32_FCB::new(vcb));
        (*fcb).first_cluster = vcb_ref.root_directory_cluster;
        (*fcb).file_size = 0; // Размер directory не используется
        (*fcb).attributes = ATTR_DIRECTORY;
        (*fcb).flags |= FCB_FLAGS_ROOT_DIRECTORY;
        (*fcb).parent_dir_cluster = 0; // Root не имеет parent
        
        // Имя root directory
        let root_name = b"\\          ";
        (&mut (*fcb).file_name)[..11].copy_from_slice(root_name);
        (*fcb).file_name[11] = 0;
        
        fcb
    }
}

/// Сравнивает два имени файлов (case-insensitive, 8.3 формат)
fn names_match(name1: &[u8; 11], name2: &[u8; 11]) -> bool {
    for i in 0..11 {
        let c1 = to_upper(name1[i]);
        let c2 = to_upper(name2[i]);
        if c1 != c2 {
            return false;
        }
    }
    true
}

/// Конвертирует ASCII символ в верхний регистр
#[inline]
fn to_upper(c: u8) -> u8 {
    if c >= b'a' && c <= b'z' {
        c - 32
    } else {
        c
    }
}

/// Ищет файл по полному пути (поддержка поддиректорий)
///
/// # Arguments
/// * `vcb` - Volume Control Block
/// * `path_wide` - путь в Unicode (например "XenOS\boot\file.sys")
/// * `path_len` - длина пути в wide chars (без NUL)
/// * `fcb_out` - выходной FCB
///
/// # Returns
/// * STATUS_SUCCESS если файл найден
pub unsafe fn fat_find_file_by_path(
    vcb: *mut FAT32_VCB,
    path_wide: &[u16],
    path_len: usize,
    fcb_out: *mut *mut FAT32_FCB,
) -> NTSTATUS {
    unsafe {
        if path_len == 0 {
            // Пустой путь = root directory
            let fcb = fat_create_root_fcb(vcb);
            if fcb.is_null() {
                return STATUS_INSUFFICIENT_RESOURCES;
            }
            *fcb_out = fcb;
            return STATUS_SUCCESS;
        }
        
        // Разбиваем путь на компоненты
        let (components, comp_count) = split_path(path_wide, path_len);
        
        if comp_count == 0 {
            // Пустой путь после split = root directory
            let fcb = fat_create_root_fcb(vcb);
            if fcb.is_null() {
                return STATUS_INSUFFICIENT_RESOURCES;
            }
            *fcb_out = fcb;
            return STATUS_SUCCESS;
        }
        
        // Начинаем с root directory
        let mut current_cluster = (*vcb).root_directory_cluster;
        
        // Проходим по всем компонентам пути
        for i in 0..comp_count {
            let component = components[i];
            
            // Конвертируем Unicode component в 8.3
            let mut name_83 = [0u8; 11];
            if !unicode_component_to_83(component, &mut name_83) {
                return STATUS_OBJECT_NAME_INVALID;
            }
            
            // Последний компонент?
            let is_last = i == comp_count - 1;
            
            // Ищем в текущей директории
            let mut found_fcb: *mut FAT32_FCB = core::ptr::null_mut();
            let status = fat_find_file(vcb, current_cluster, &name_83, &mut found_fcb);
            
            if status != STATUS_SUCCESS {
                return status;
            }
            
            if is_last {
                // Это файл который ищем
                *fcb_out = found_fcb;
                return STATUS_SUCCESS;
            } else {
                // Это промежуточная директория
                if !(*found_fcb).is_directory() {
                    // Компонент пути не директория
                    ExFreePoolWithTag(found_fcb as PVOID, FAT32_POOL_TAG);
                    return STATUS_OBJECT_PATH_NOT_FOUND;
                }
                
                // Переходим в эту директорию
                current_cluster = (*found_fcb).first_cluster;
                
                // Освобождаем временный FCB директории
                ExFreePoolWithTag(found_fcb as PVOID, FAT32_POOL_TAG);
                
                if current_cluster < 2 {
                    return STATUS_OBJECT_PATH_NOT_FOUND;
                }
            }
        }
        
        STATUS_OBJECT_NAME_NOT_FOUND
    }
}

/// Конвертирует Unicode component в 8.3 формат
fn unicode_component_to_83(wide_str: &[u16], output: &mut [u8; 11]) -> bool {
    // Конвертируем в ASCII
    let mut ascii_buf = [0u8; 256];
    let mut ascii_len = 0;
    
    for &wide_char in wide_str {
        if wide_char == 0 || ascii_len >= 255 {
            break;
        }
        
        // Простая конвертация (только ASCII)
        if wide_char < 128 {
            ascii_buf[ascii_len] = wide_char as u8;
            ascii_len += 1;
        } else {
            return false; // Не-ASCII не поддерживаются в 8.3
        }
    }
    
    if ascii_len == 0 {
        return false;
    }
    
    // Конвертируем в 8.3
    match core::str::from_utf8(&ascii_buf[..ascii_len]) {
        Ok(s) => convert_to_83_name(s, output),
        Err(_) => false,
    }
}

/// Конвертирует строку в формат 8.3 (space-padded)
///
/// Например: "boot.sys" -> "BOOT    SYS"
pub fn convert_to_83_name(name: &str, output: &mut [u8; 11]) -> bool {
    // Заполняем пробелами
    output.fill(b' ');
    
    // Разделяем на имя и расширение
    let (base, ext) = if let Some(dot_pos) = name.rfind('.') {
        (&name[..dot_pos], &name[dot_pos + 1..])
    } else {
        (name, "")
    };
    
    // Проверяем длину
    if base.len() > 8 || ext.len() > 3 {
        return false;
    }
    
    // Копируем base name (до 8 символов)
    for (i, &b) in base.as_bytes().iter().take(8).enumerate() {
        output[i] = to_upper(b);
    }
    
    // Копируем extension (до 3 символов)
    for (i, &b) in ext.as_bytes().iter().take(3).enumerate() {
        output[8 + i] = to_upper(b);
    }
    
    true
}

