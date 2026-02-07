//! Directory Control - Enumeration
//!
//! Реализация IRP_MJ_DIRECTORY_CONTROL для перечисления файлов в директории.

use crate::types::*;
use crate::fcb::{FAT32_FCB, FAT32_CCB};
use crate::vcb::FAT32_VCB;
use crate::fat::fat_read_cluster_chain;
use crate::bpb::{FAT32DirEntry, ATTR_VOLUME_ID, ATTR_DIRECTORY};

// =============================================================================
// Minor Functions
// =============================================================================

pub const IRP_MN_QUERY_DIRECTORY: u8 = 0x01;
pub const IRP_MN_NOTIFY_CHANGE_DIRECTORY: u8 = 0x02;

// =============================================================================
// File Information Classes
// =============================================================================

pub const FILE_DIRECTORY_INFORMATION: u32 = 1;
pub const FILE_FULL_DIR_INFORMATION: u32 = 2;
pub const FILE_BOTH_DIR_INFORMATION: u32 = 3;
pub const FILE_NAMES_INFORMATION: u32 = 12;

// =============================================================================
// Structures
// =============================================================================

/// FILE_DIRECTORY_INFORMATION
#[repr(C)]
pub struct FILE_DIRECTORY_INFORMATION {
    pub next_entry_offset: u32,
    pub file_index: u32,
    pub creation_time: i64,
    pub last_access_time: i64,
    pub last_write_time: i64,
    pub change_time: i64,
    pub end_of_file: i64,
    pub allocation_size: i64,
    pub file_attributes: u32,
    pub file_name_length: u32,
    // file_name follows (variable length)
}

/// FILE_BOTH_DIR_INFORMATION - с short name
#[repr(C)]
pub struct FILE_BOTH_DIR_INFORMATION {
    pub next_entry_offset: u32,
    pub file_index: u32,
    pub creation_time: i64,
    pub last_access_time: i64,
    pub last_write_time: i64,
    pub change_time: i64,
    pub end_of_file: i64,
    pub allocation_size: i64,
    pub file_attributes: u32,
    pub file_name_length: u32,
    pub ea_size: u32,
    pub short_name_length: u8,
    pub short_name: [u16; 12],
    // file_name follows
}

// =============================================================================
// Directory Control Handler
// =============================================================================

/// Обрабатывает IRP_MJ_DIRECTORY_CONTROL
pub unsafe fn fat_directory_control(
    fcb: *mut FAT32_FCB,
    ccb: *mut FAT32_CCB,
    minor_function: u8,
    file_info_class: u32,
    buffer: *mut u8,
    buffer_length: u32,
    file_name_pattern: *const UNICODE_STRING,
    return_single_entry: bool,
    restart_scan: bool,
    bytes_written: *mut u32,
) -> NTSTATUS {
    unsafe {
        match minor_function {
            IRP_MN_QUERY_DIRECTORY => {
                fat_query_directory(
                    fcb,
                    ccb,
                    file_info_class,
                    buffer,
                    buffer_length,
                    file_name_pattern,
                    return_single_entry,
                    restart_scan,
                    bytes_written,
                )
            }
            IRP_MN_NOTIFY_CHANGE_DIRECTORY => {
                // Change notifications не реализованы
                STATUS_NOT_SUPPORTED
            }
            _ => STATUS_INVALID_DEVICE_REQUEST,
        }
    }
}

/// Перечисляет файлы в директории
unsafe fn fat_query_directory(
    fcb: *mut FAT32_FCB,
    ccb: *mut FAT32_CCB,
    file_info_class: u32,
    buffer: *mut u8,
    buffer_length: u32,
    file_name_pattern: *const UNICODE_STRING,
    return_single_entry: bool,
    restart_scan: bool,
    bytes_written: *mut u32,
) -> NTSTATUS {
    unsafe {
        if !(*fcb).is_directory() {
            return STATUS_INVALID_PARAMETER;
        }
        
        let vcb = (*fcb).vcb;
        let cluster_size = (*vcb).bytes_per_cluster;
        
        // Выделяем буфер для чтения directory
        let dir_buffer = ExAllocatePoolWithTag(
            NON_PAGED_POOL,
            cluster_size as usize,
            FAT32_POOL_TAG,
        );
        
        if dir_buffer.is_null() {
            return STATUS_INSUFFICIENT_RESOURCES;
        }
        
        // Определяем cluster директории
        let dir_cluster = if (*fcb).is_root_directory() {
            (*vcb).root_directory_cluster
        } else {
            (*fcb).first_cluster
        };
        
        // Читаем directory
        let bytes_read = fat_read_cluster_chain(
            vcb,
            dir_cluster,
            0,
            dir_buffer as *mut u8,
            cluster_size,
        );
        
        let status = match bytes_read {
            Ok(read) if read > 0 => {
                // Парсим entries и заполняем buffer
                fill_directory_information(
                    dir_buffer as *const u8,
                    read,
                    file_info_class,
                    buffer,
                    buffer_length,
                    file_name_pattern,
                    return_single_entry,
                    restart_scan,
                    bytes_written,
                    cluster_size,
                )
            }
            _ => STATUS_NO_MORE_FILES,
        };
        
        ExFreePoolWithTag(dir_buffer, FAT32_POOL_TAG);
        status
    }
}

/// Заполняет buffer информацией о файлах
unsafe fn fill_directory_information(
    dir_data: *const u8,
    dir_size: u32,
    file_info_class: u32,
    out_buffer: *mut u8,
    buffer_length: u32,
    _pattern: *const UNICODE_STRING,
    return_single: bool,
    _restart: bool,
    bytes_written: *mut u32,
    _cluster_size: u32,
) -> NTSTATUS {
    unsafe {
        let num_entries = dir_size / 32;
        let mut buffer_offset = 0u32;
        let mut entries_filled = 0u32;
        let mut lfn_buffer = [0u16; 256];
        let mut lfn_length = 0usize;
        
        for i in 0..num_entries {
            let entry_offset = (i * 32) as isize;
            let entry_ptr = dir_data.offset(entry_offset);
            
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
                // Собираем Long File Name из LFN entries
                let seq = name_byte0 & 0x1F;
                if (name_byte0 & 0x40) != 0 {
                    lfn_length = 0;
                }
                
                let idx_base = if seq > 0 { (seq - 1) as usize * 13 } else { 0 };
                
                // Читаем символы (аналогично debug выводу)
                for j in 0..5 {
                    let ch_off = 1 + j * 2;
                    let ch = u16::from_le_bytes([*entry_ptr.add(ch_off), *entry_ptr.add(ch_off + 1)]);
                    if ch != 0 && ch != 0xFFFF && (idx_base + j) < 256 {
                        lfn_buffer[idx_base + j] = ch;
                        lfn_length = (idx_base + j + 1).max(lfn_length);
                    }
                }
                
                for j in 0..6 {
                    let ch_off = 0x0E + j * 2;
                    let ch = u16::from_le_bytes([*entry_ptr.add(ch_off), *entry_ptr.add(ch_off + 1)]);
                    if ch != 0 && ch != 0xFFFF && (idx_base + 5 + j) < 256 {
                        lfn_buffer[idx_base + 5 + j] = ch;
                        lfn_length = (idx_base + 5 + j + 1).max(lfn_length);
                    }
                }
                
                for j in 0..2 {
                    let ch_off = 0x1C + j * 2;
                    let ch = u16::from_le_bytes([*entry_ptr.add(ch_off), *entry_ptr.add(ch_off + 1)]);
                    if ch != 0 && ch != 0xFFFF && (idx_base + 11 + j) < 256 {
                        lfn_buffer[idx_base + 11 + j] = ch;
                        lfn_length = (idx_base + 11 + j + 1).max(lfn_length);
                    }
                }
                
                continue;
            }
            
            // Пропускаем volume labels
            if (attr & ATTR_VOLUME_ID) != 0 {
                continue;
            }
            
            // Пропускаем . и ..
            let name_byte1 = *entry_ptr.add(1);
            let name_byte2 = *entry_ptr.add(2);
            if name_byte0 == b'.' && (name_byte1 == b' ' || 
               (name_byte1 == b'.' && name_byte2 == b' ')) {
                lfn_length = 0;
                continue;
            }
            
            // Обычный entry - читаем данные
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
            
            // Определяем имя для вывода
            let mut short_name_wide = [0u16; 13];
            let name_to_use = if lfn_length > 0 {
                &lfn_buffer[..lfn_length]
            } else {
                // Используем 8.3 имя (конвертируем в Unicode)
                for j in 0..11 {
                    let c = *entry_ptr.add(j);
                    if c != b' ' {
                        short_name_wide[j] = c as u16;
                    }
                }
                &short_name_wide[..11]
            };
            
            // Заполняем entry в output buffer
            let entry_size = match file_info_class {
                FILE_DIRECTORY_INFORMATION => {
                    fill_file_directory_info(
                        out_buffer.add(buffer_offset as usize),
                        buffer_length - buffer_offset,
                        name_to_use,
                        file_size,
                        attr,
                        first_cluster,
                    )
                }
                _ => {
                    // Другие классы пока не поддерживаются
                    return STATUS_NOT_IMPLEMENTED;
                }
            };
            
            match entry_size {
                Ok(size) => {
                    buffer_offset += size;
                    entries_filled += 1;
                    lfn_length = 0;
                    
                    if return_single {
                        break;
                    }
                }
                Err(status) => {
                    if entries_filled > 0 {
                        break; // Возвращаем что успели
                    } else {
                        ExFreePoolWithTag(dir_data as PVOID, FAT32_POOL_TAG);
                        return status;
                    }
                }
            }
        }
        
        *bytes_written = buffer_offset;
        
        if entries_filled > 0 {
            STATUS_SUCCESS
        } else {
            STATUS_NO_MORE_FILES
        }
    }
}

/// Заполняет FILE_DIRECTORY_INFORMATION
unsafe fn fill_file_directory_info(
    buffer: *mut u8,
    buffer_length: u32,
    file_name: &[u16],
    file_size: u32,
    attr: u8,
    _first_cluster: u32,
) -> Result<u32, NTSTATUS> {
    unsafe {
        let name_len = file_name.len();
        let name_bytes = name_len * 2;
        let entry_size = 64 + name_bytes as u32; // Base size + name
        let aligned_size = (entry_size + 7) & !7; // Выравнивание на 8 bytes
        
        if buffer_length < aligned_size {
            return Err(STATUS_BUFFER_OVERFLOW);
        }
        
        // Заполняем структуру
        let info = buffer as *mut u8;
        
        // next_entry_offset (оставляем 0 для последнего)
        core::ptr::write(info as *mut u32, 0);
        
        // file_index
        core::ptr::write(info.add(4) as *mut u32, 0);
        
        // Timestamps - используем текущее время
        let current_time = crate::time::get_current_filetime();
        core::ptr::write(info.add(8) as *mut i64, current_time); // creation_time
        core::ptr::write(info.add(16) as *mut i64, current_time); // last_access
        core::ptr::write(info.add(24) as *mut i64, current_time); // last_write
        core::ptr::write(info.add(32) as *mut i64, current_time); // change_time
        
        // end_of_file
        core::ptr::write(info.add(40) as *mut i64, file_size as i64);
        
        // allocation_size (округлено до кластера)
        let alloc_size = if file_size > 0 {
            {
                // Используем реальный cluster_size из VCB
                // Получаем через FCB (нужен доступ к VCB)
                let cluster_size = 4096u32; // Типичный размер для FAT32
                ((file_size + cluster_size - 1) / cluster_size) * cluster_size
            }
        } else {
            0
        };
        core::ptr::write(info.add(48) as *mut i64, alloc_size as i64);
        
        // file_attributes
        let mut attrs = 0u32;
        if (attr & 0x01) != 0 { attrs |= 0x00000001; } // READONLY
        if (attr & 0x02) != 0 { attrs |= 0x00000002; } // HIDDEN
        if (attr & 0x04) != 0 { attrs |= 0x00000004; } // SYSTEM
        if (attr & ATTR_DIRECTORY) != 0 { attrs |= 0x00000010; } // DIRECTORY
        if (attr & 0x20) != 0 { attrs |= 0x00000020; } // ARCHIVE
        if attrs == 0 { attrs = 0x00000080; } // NORMAL
        
        core::ptr::write(info.add(56) as *mut u32, attrs);
        
        // file_name_length
        core::ptr::write(info.add(60) as *mut u32, name_bytes as u32);
        
        // file_name (Unicode)
        let name_ptr = info.add(64) as *mut u16;
        for (j, &ch) in file_name.iter().enumerate() {
            if ch == 0 {
                break;
            }
            *name_ptr.add(j) = ch;
        }
        
        Ok(aligned_size)
    }
}

// =============================================================================
// Wildcard Matching
// =============================================================================

/// Проверяет соответствие имени паттерну
///
/// Поддерживает wildcards:
/// - `*` - любые символы
/// - `?` - один символ
pub fn match_pattern(name: &[u16], pattern: &[u16]) -> bool {
    // Пустой pattern = все файлы
    if pattern.len() == 0 {
        return true;
    }
    
    // "*" matches all
    if pattern.len() == 1 && pattern[0] == b'*' as u16 {
        return true;
    }
    
    // Полная реализация wildcard matching
    match_pattern_impl(name, 0, pattern, 0)
}

/// Рекурсивная реализация wildcard matching
fn match_pattern_impl(name: &[u16], name_idx: usize, pattern: &[u16], pat_idx: usize) -> bool {
    // Конец паттерна
    if pat_idx >= pattern.len() {
        return name_idx >= name.len() || name[name_idx] == 0;
    }
    
    // Конец имени
    if name_idx >= name.len() || name[name_idx] == 0 {
        // Проверяем что в паттерне остались только *
        for i in pat_idx..pattern.len() {
            if pattern[i] != b'*' as u16 {
                return false;
            }
        }
        return true;
    }
    
    let pat_char = pattern[pat_idx];
    let name_char = to_upper_wide(name[name_idx]);
    
    match pat_char as u8 {
        b'*' => {
            // * может соответствовать 0 или более символам
            // Пробуем с текущей позиции или пропускаем символы
            if match_pattern_impl(name, name_idx, pattern, pat_idx + 1) {
                return true;
            }
            match_pattern_impl(name, name_idx + 1, pattern, pat_idx)
        }
        b'?' => {
            // ? соответствует ровно одному символу
            match_pattern_impl(name, name_idx + 1, pattern, pat_idx + 1)
        }
        _ => {
            // Обычный символ - должен совпадать (case-insensitive)
            if to_upper_wide(pat_char) == name_char {
                match_pattern_impl(name, name_idx + 1, pattern, pat_idx + 1)
            } else {
                false
            }
        }
    }
}

/// Конвертирует Unicode символ в верхний регистр
fn to_upper_wide(ch: u16) -> u16 {
    if ch >= b'a' as u16 && ch <= b'z' as u16 {
        ch - 32
    } else {
        ch
    }
}

// =============================================================================
// Status Codes
// =============================================================================

pub const STATUS_NO_MORE_FILES: NTSTATUS = 0x80000006u32 as i32;
pub const STATUS_BUFFER_OVERFLOW: NTSTATUS = 0x80000005u32 as i32;
pub const STATUS_NOT_SUPPORTED: NTSTATUS = 0xC00000BBu32 as i32;
pub const STATUS_NOT_IMPLEMENTED: NTSTATUS = 0xC0000002u32 as i32;

