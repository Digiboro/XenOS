//! MBR (Master Boot Record) Partition Table
//!
//! Чтение и парсинг MBR таблицы партиций.

use crate::types::*;
use crate::partition::{PartitionInfo, MAX_PARTITIONS};
use crate::{disk_print, disk_print_hex, disk_print_dec};

/// MBR signature (0xAA55 в конце сектора)
const MBR_SIGNATURE: u16 = 0xAA55;

/// MBR Partition Entry (16 bytes)
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct MbrPartitionEntry {
    pub boot_indicator: u8,       // 0x80 = bootable
    pub starting_chs: [u8; 3],    // CHS адрес (obsolete)
    pub partition_type: u8,       // Тип партиции
    pub ending_chs: [u8; 3],      // CHS адрес (obsolete)
    pub starting_lba: u32,        // Начальный LBA
    pub sector_count: u32,        // Размер в секторах
}

/// MBR Structure (первый сектор диска)
#[repr(C, packed)]
pub struct Mbr {
    pub bootstrap_code: [u8; 446],        // Boot code
    pub partition_table: [MbrPartitionEntry; 4], // 4 primary partitions
    pub signature: u16,                   // 0xAA55
}

/// Проверяет является ли partition type защитным GPT (0xEE)
#[inline]
pub fn is_gpt_protective(partition_type: u8) -> bool {
    partition_type == 0xEE
}

/// Проверяет является ли partition type extended (0x05, 0x0F)
#[inline]
pub fn is_extended_partition(partition_type: u8) -> bool {
    partition_type == 0x05 || partition_type == 0x0F
}

/// Читает MBR партиции с диска
///
/// # Arguments
/// * `fdo_ext` - FDO extension
/// * `partitions` - массив для сохранения партиций
///
/// # Returns
/// Ok(count) или Err(STATUS)
pub unsafe fn read_mbr_partitions(
    fdo_ext: *mut DISK_FDO_EXTENSION,
    partitions: &mut [PartitionInfo; MAX_PARTITIONS],
) -> Result<usize, NTSTATUS> {
    disk_print("[DISK/MBR] Reading MBR from LBA 0...\n");
    
    // Выделяем буфер для сектора (выровненный)
    let buffer = ExAllocatePoolWithTag(NON_PAGED_POOL, 512, crate::types::DISK_POOL_TAG);
    if buffer.is_null() {
        disk_print("[DISK/MBR] ERROR: Cannot allocate buffer\n");
        return Err(STATUS_INSUFFICIENT_RESOURCES);
    }
    
    // Очищаем буфер
    core::ptr::write_bytes(buffer as *mut u8, 0, 512);
    
    // Читаем первый сектор (LBA 0, byte offset 0)
    let lower_device = (*fdo_ext).lower_device;
    let status = super::io::read_from_device(lower_device, 0, 512, buffer);
    
    if status < 0 {
        disk_print("[DISK/MBR] ERROR: Read failed, status=0x");
        disk_print_hex(status as u32 as u64);
        disk_print("\n");
        ExFreePoolWithTag(buffer, crate::types::DISK_POOL_TAG);
        return Err(status);
    }
    
    // Проверяем MBR signature (offset 510-511)
    let sig_ptr = (buffer as usize + 510) as *const u16;
    let signature = core::ptr::read_unaligned(sig_ptr);
    
    disk_print("[DISK/MBR] Signature at offset 510: 0x");
    disk_print_hex(signature as u64);
    disk_print("\n");
    
    if signature != MBR_SIGNATURE {
        disk_print("[DISK/MBR] ERROR: Invalid MBR signature (expected 0xAA55)\n");
        ExFreePoolWithTag(buffer, crate::types::DISK_POOL_TAG);
        return Err(STATUS_UNSUCCESSFUL);
    }
    
    // Парсим partition table (offset 446, 4 записи по 16 байт)
    let table_ptr = (buffer as usize + 446) as *const MbrPartitionEntry;
    
    let mut count = 0usize;
    let mut extended_lba = 0u64;
    
    disk_print("[DISK/MBR] Parsing partition table:\n");
    
    for i in 0..4 {
        let entry_ptr = table_ptr.add(i);
        
        // Читаем поля с учетом packed структуры - используем raw pointer arithmetic
        let entry_bytes = entry_ptr as *const u8;
        let boot_indicator = core::ptr::read(entry_bytes);
        let partition_type = core::ptr::read(entry_bytes.add(4));
        let starting_lba = core::ptr::read_unaligned(entry_bytes.add(8) as *const u32);
        let sector_count = core::ptr::read_unaligned(entry_bytes.add(12) as *const u32);
        
        disk_print("[DISK/MBR]   Entry ");
        disk_print_dec(i as u64);
        disk_print(": type=0x");
        disk_print_hex(partition_type as u64);
        disk_print(" boot=0x");
        disk_print_hex(boot_indicator as u64);
        disk_print(" LBA=");
        disk_print_dec(starting_lba as u64);
        disk_print(" sectors=");
        disk_print_dec(sector_count as u64);
        disk_print("\n");
        
        // Пропускаем пустые записи
        if partition_type == 0 {
            continue;
        }
        
        // Сохраняем extended partition для последующей обработки
        if is_extended_partition(partition_type) {
            extended_lba = starting_lba as u64;
            disk_print("[DISK/MBR]     -> Extended partition at LBA ");
            disk_print_dec(extended_lba);
            disk_print("\n");
            continue;
        }
        
        if count < MAX_PARTITIONS {
            partitions[count] = PartitionInfo {
                partition_number: (count + 1) as u32, // 1-based
                partition_type,
                bootable: boot_indicator == 0x80,
                starting_lba: starting_lba as u64,
                sector_count: sector_count as u64,
            };
            count += 1;
        }
    }
    
    ExFreePoolWithTag(buffer, crate::types::DISK_POOL_TAG);
    
    disk_print("[DISK/MBR] Found ");
    disk_print_dec(count as u64);
    disk_print(" primary partition(s)\n");
    
    // Обрабатываем extended partitions (логические партиции)
    if extended_lba > 0 && count < MAX_PARTITIONS {
        disk_print("[DISK/MBR] Reading extended partitions...\n");
        let logical_count = read_extended_partitions(
            fdo_ext,
            extended_lba,
            extended_lba,
            &mut partitions[count..],
            MAX_PARTITIONS - count,
            count + 5, // Логические начинаются с 5
        );
        count += logical_count;
    }
    
    Ok(count)
}

/// Читает логические партиции из extended partition (рекурсивно)
unsafe fn read_extended_partitions(
    fdo_ext: *mut DISK_FDO_EXTENSION,
    extended_base: u64,
    ebr_lba: u64,
    partitions: &mut [PartitionInfo],
    max_count: usize,
    next_number: usize,
) -> usize {
    if max_count == 0 {
        return 0;
    }
    
    // Выделяем буфер для EBR
    let buffer = ExAllocatePoolWithTag(NON_PAGED_POOL, 512, crate::types::DISK_POOL_TAG);
    if buffer.is_null() {
        return 0;
    }
    
    core::ptr::write_bytes(buffer as *mut u8, 0, 512);
    
    // Читаем EBR
    let lower_device = (*fdo_ext).lower_device;
    let sector_size = (*fdo_ext).sector_size as u64;
    let byte_offset = ebr_lba * sector_size;
    
    let status = super::io::read_from_device(lower_device, byte_offset, 512, buffer);
    
    if status < 0 {
        ExFreePoolWithTag(buffer, crate::types::DISK_POOL_TAG);
        return 0;
    }
    
    // Проверяем EBR signature
    let sig_ptr = (buffer as usize + 510) as *const u16;
    let signature = core::ptr::read_unaligned(sig_ptr);
    
    if signature != MBR_SIGNATURE {
        ExFreePoolWithTag(buffer, crate::types::DISK_POOL_TAG);
        return 0;
    }
    
    let table_ptr = (buffer as usize + 446) as *const MbrPartitionEntry;
    let mut count = 0usize;
    
    // Первая запись - логическая партиция (относительно EBR)
    let logical_bytes = table_ptr as *const u8;
    let logical_boot = core::ptr::read(logical_bytes);
    let logical_type = core::ptr::read(logical_bytes.add(4));
    let logical_lba = core::ptr::read_unaligned(logical_bytes.add(8) as *const u32);
    let logical_count = core::ptr::read_unaligned(logical_bytes.add(12) as *const u32);
    
    if logical_type != 0 && !is_extended_partition(logical_type) {
        partitions[0] = PartitionInfo {
            partition_number: next_number as u32,
            partition_type: logical_type,
            bootable: logical_boot == 0x80,
            starting_lba: ebr_lba + logical_lba as u64,
            sector_count: logical_count as u64,
        };
        count = 1;
        
        disk_print("[DISK/MBR]   Logical partition ");
        disk_print_dec(next_number as u64);
        disk_print(": type=0x");
        disk_print_hex(logical_type as u64);
        disk_print(" LBA=");
        disk_print_dec(ebr_lba + logical_lba as u64);
        disk_print("\n");
    }
    
    // Вторая запись - указатель на следующий EBR (относительно extended_base)
    let next_ebr_bytes = (table_ptr as *const u8).add(16); // Следующая запись +16 bytes
    let next_type = core::ptr::read(next_ebr_bytes.add(4));
    let next_lba = core::ptr::read_unaligned(next_ebr_bytes.add(8) as *const u32);
    
    ExFreePoolWithTag(buffer, crate::types::DISK_POOL_TAG);
    
    if is_extended_partition(next_type) && next_lba > 0 && count < max_count {
        let next_ebr_lba = extended_base + next_lba as u64;
        
        // Рекурсивно читаем следующий EBR
        let additional = read_extended_partitions(
            fdo_ext,
            extended_base,
            next_ebr_lba,
            &mut partitions[count..],
            max_count - count,
            next_number + count,
        );
        count += additional;
    }
    
    count
}
