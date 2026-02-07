//! Partition Manager для disk.sys
//!
//! Читает MBR/GPT таблицы партиций и создаёт PDO для каждой партиции.

pub mod mbr;
pub mod gpt;
pub mod io;

pub use mbr::*;
pub use gpt::*;
pub use io::*;

use crate::types::*;
use crate::{disk_print, disk_print_dec};

/// Информация о партиции
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PartitionInfo {
    /// Номер партиции (0 = whole disk, 1+ = партиции)
    pub partition_number: u32,
    /// Тип партиции (MBR type или GPT type mapped)
    pub partition_type: u8,
    /// Bootable flag
    pub bootable: bool,
    /// Начальный LBA
    pub starting_lba: u64,
    /// Размер в секторах
    pub sector_count: u64,
}

impl PartitionInfo {
    pub const fn new() -> Self {
        Self {
            partition_number: 0,
            partition_type: 0,
            bootable: false,
            starting_lba: 0,
            sector_count: 0,
        }
    }
}

/// Максимальное количество партиций
pub const MAX_PARTITIONS: usize = 16;

/// Результат чтения партиций
pub struct PartitionList {
    pub partitions: [PartitionInfo; MAX_PARTITIONS],
    pub count: usize,
}

impl PartitionList {
    pub const fn new() -> Self {
        Self {
            partitions: [PartitionInfo::new(); MAX_PARTITIONS],
            count: 0,
        }
    }
}

/// Читает таблицу партиций с диска
///
/// Автоматически определяет формат (MBR или GPT) и парсит партиции.
/// Всегда создает Partition0 для raw disk access.
///
/// # Arguments
/// * `fdo_ext` - FDO extension с информацией о диске
///
/// # Returns
/// PartitionList со списком партиций (Partition0 + реальные партиции)
pub unsafe fn read_partition_table(
    fdo_ext: *mut DISK_FDO_EXTENSION,
) -> PartitionList {
    disk_print("[DISK/PART] Starting partition table scan\n");
    
    let mut result = PartitionList::new();
    let mut temp_partitions = [PartitionInfo::new(); MAX_PARTITIONS];
    let mut partition_count = 0usize;
    
    // Сначала всегда пробуем прочитать MBR
    let mut is_gpt = false;
    let mut mbr_valid = false;
    
    match mbr::read_mbr_partitions(fdo_ext, &mut temp_partitions) {
        Ok(count) => {
            mbr_valid = true;
            disk_print("[DISK/PART] MBR found, ");
            disk_print_dec(count as u64);
            disk_print(" partition(s)\n");
            
            if count > 0 {
                // Проверяем protective MBR (type 0xEE означает GPT)
                if count == 1 && temp_partitions[0].partition_type == 0xEE {
                    disk_print("[DISK/PART] Detected GPT protective MBR (type 0xEE)\n");
                    is_gpt = true;
                } else {
                    // Это обычный MBR - копируем партиции
                    disk_print("[DISK/PART] Using MBR partitions\n");
                    for i in 0..count {
                        if partition_count < MAX_PARTITIONS - 1 {
                            let mut part = temp_partitions[i];
                            part.partition_number = (partition_count + 1) as u32;
                            result.partitions[partition_count + 1] = part;
                            partition_count += 1;
                        }
                    }
                }
            }
        }
        Err(status) => {
            disk_print("[DISK/PART] MBR read failed (status=0x");
            crate::disk_print_hex(status as u32 as u64);
            disk_print("), trying GPT directly\n");
            is_gpt = true; // Пробуем GPT если MBR не найден
        }
    }
    
    // Читаем GPT если обнаружен protective MBR или MBR невалиден
    if is_gpt {
        disk_print("[DISK/PART] Attempting GPT parsing...\n");
        
        // Очищаем temp_partitions для GPT
        for part in temp_partitions.iter_mut() {
            *part = PartitionInfo::new();
        }
        
        match gpt::read_gpt_partitions(fdo_ext, &mut temp_partitions) {
            Ok(count) => {
                disk_print("[DISK/PART] GPT found, ");
                disk_print_dec(count as u64);
                disk_print(" partition(s)\n");
                
                if count > 0 {
                    // Сбрасываем MBR партиции если GPT валиден
                    partition_count = 0;
                    
                    for i in 0..count {
                        if partition_count < MAX_PARTITIONS - 1 {
                            let mut part = temp_partitions[i];
                            part.partition_number = (partition_count + 1) as u32;
                            result.partitions[partition_count + 1] = part;
                            partition_count += 1;
                        }
                    }
                }
            }
            Err(status) => {
                disk_print("[DISK/PART] GPT read failed (status=0x");
                crate::disk_print_hex(status as u32 as u64);
                disk_print(")\n");
                
                // Если ни MBR ни GPT не работают - диск пустой или поврежден
                if !mbr_valid {
                    disk_print("[DISK/PART] WARNING: No valid partition table found\n");
                }
            }
        }
    }
    
    // Всегда создаём Partition0 = весь диск (для raw access)
    result.partitions[0] = PartitionInfo {
        partition_number: 0,
        partition_type: 0,
        bootable: false,
        starting_lba: 0,
        sector_count: (*fdo_ext).sector_count,
    };
    
    // Общее количество = Partition0 + реальные партиции
    result.count = partition_count + 1;
    
    disk_print("[DISK/PART] Total partitions: ");
    disk_print_dec(result.count as u64);
    disk_print(" (including Partition0)\n");
    
    result
}
