//! GPT (GUID Partition Table)
//!
//! Чтение и парсинг GPT таблицы партиций согласно UEFI Specification.

use crate::types::*;
use crate::partition::{PartitionInfo, MAX_PARTITIONS};
use crate::{disk_print, disk_print_hex, disk_print_dec};

/// GPT Header signature "EFI PART" (little-endian: 0x5452415020494645)
const GPT_SIGNATURE: u64 = 0x5452415020494645;

/// GPT Revision 1.0
const GPT_REVISION_1_0: u32 = 0x00010000;

/// Известные GUID типы партиций
mod partition_types {
    /// EFI System Partition
    pub const EFI_SYSTEM: [u8; 16] = [
        0x28, 0x73, 0x2A, 0xC1, 0x1F, 0xF8, 0xD2, 0x11,
        0xBA, 0x4B, 0x00, 0xA0, 0xC9, 0x3E, 0xC9, 0x3B
    ];
    
    /// Microsoft Basic Data (NTFS, FAT32, etc.)
    pub const BASIC_DATA: [u8; 16] = [
        0xA2, 0xA0, 0xD0, 0xEB, 0xE5, 0xB9, 0x33, 0x44,
        0x87, 0xC0, 0x68, 0xB6, 0xB7, 0x26, 0x99, 0xC7
    ];
    
    /// Microsoft Reserved Partition
    pub const MS_RESERVED: [u8; 16] = [
        0x16, 0xE3, 0xC9, 0xE3, 0x5C, 0x0B, 0xB8, 0x4D,
        0x81, 0x7D, 0xF9, 0x2D, 0xF0, 0x02, 0x15, 0xAE
    ];
    
    /// Linux Filesystem
    pub const LINUX_FS: [u8; 16] = [
        0xAF, 0x3D, 0xC6, 0x0F, 0x83, 0x84, 0x72, 0x47,
        0x8E, 0x79, 0x3D, 0x69, 0xD8, 0x47, 0x7D, 0xE4
    ];
}

/// GPT Header (LBA 1)
#[repr(C, packed)]
struct GptHeader {
    signature: u64,              // "EFI PART"
    revision: u32,               // 0x00010000 для GPT 1.0
    header_size: u32,            // Обычно 92 bytes
    header_crc32: u32,           // CRC32 header
    reserved: u32,               // Должен быть 0
    my_lba: u64,                 // LBA этого header (обычно 1)
    alternate_lba: u64,          // LBA backup header (обычно последний сектор)
    first_usable_lba: u64,       // Первый LBA для партиций
    last_usable_lba: u64,        // Последний LBA для партиций
    disk_guid: [u8; 16],         // GUID диска
    partition_entry_lba: u64,    // LBA начала partition entries (обычно 2)
    number_of_entries: u32,      // Количество entries (обычно 128)
    size_of_entry: u32,          // Размер entry (обычно 128 bytes)
    partition_array_crc32: u32,  // CRC32 partition array
}

/// GPT Partition Entry (обычно 128 bytes)
#[repr(C, packed)]
struct GptPartitionEntry {
    partition_type_guid: [u8; 16],  // GUID типа партиции
    unique_partition_guid: [u8; 16], // Уникальный GUID партиции
    starting_lba: u64,              // Первый LBA
    ending_lba: u64,                // Последний LBA (включительно)
    attributes: u64,                // Атрибуты
    partition_name: [u16; 36],      // Unicode имя партиции (72 bytes)
}

/// Проверяет что GUID не пустой (все нули)
fn is_guid_empty(guid: &[u8; 16]) -> bool {
    for &byte in guid.iter() {
        if byte != 0 {
            return false;
        }
    }
    true
}

/// Конвертирует GPT partition type GUID в MBR-совместимый тип
fn guid_to_mbr_type(guid: &[u8; 16]) -> u8 {
    if *guid == partition_types::EFI_SYSTEM {
        0xEF // EFI System
    } else if *guid == partition_types::BASIC_DATA {
        0x07 // NTFS/exFAT/Basic Data
    } else if *guid == partition_types::MS_RESERVED {
        0x00 // Reserved (skip)
    } else if *guid == partition_types::LINUX_FS {
        0x83 // Linux
    } else {
        0x07 // Default to Basic Data for unknown GUIDs
    }
}

/// Читает GPT партиции с диска
///
/// # Arguments
/// * `fdo_ext` - FDO extension
/// * `partitions` - массив для сохранения партиций
///
/// # Returns
/// Ok(count) или Err(STATUS)
pub unsafe fn read_gpt_partitions(
    fdo_ext: *mut DISK_FDO_EXTENSION,
    partitions: &mut [PartitionInfo; MAX_PARTITIONS],
) -> Result<usize, NTSTATUS> {
    disk_print("[DISK/GPT] Reading GPT header from LBA 1...\n");
    
    // Выделяем буфер для сектора (GPT header)
    let buffer = ExAllocatePoolWithTag(NON_PAGED_POOL, 512, crate::types::DISK_POOL_TAG);
    if buffer.is_null() {
        disk_print("[DISK/GPT] ERROR: Cannot allocate buffer\n");
        return Err(STATUS_INSUFFICIENT_RESOURCES);
    }
    
    core::ptr::write_bytes(buffer as *mut u8, 0, 512);
    
    // Читаем LBA 1 (GPT header)
    let lower_device = (*fdo_ext).lower_device;
    let sector_size = (*fdo_ext).sector_size as u64;
    let byte_offset = sector_size; // LBA 1
    
    disk_print("[DISK/GPT] sector_size=");
    disk_print_dec(sector_size);
    disk_print(" byte_offset=");
    disk_print_dec(byte_offset);
    disk_print("\n");
    
    let status = super::io::read_from_device(lower_device, byte_offset, 512, buffer);
    
    if status < 0 {
        disk_print("[DISK/GPT] ERROR: Read failed, status=0x");
        disk_print_hex(status as u32 as u64);
        disk_print("\n");
        ExFreePoolWithTag(buffer, crate::types::DISK_POOL_TAG);
        return Err(status);
    }
    
    // Читаем signature напрямую из буфера
    let sig = core::ptr::read_unaligned(buffer as *const u64);
    
    disk_print("[DISK/GPT] Signature: 0x");
    disk_print_hex(sig);
    disk_print(" (expected 0x");
    disk_print_hex(GPT_SIGNATURE);
    disk_print(")\n");
    
    if sig != GPT_SIGNATURE {
        disk_print("[DISK/GPT] ERROR: Invalid GPT signature\n");
        ExFreePoolWithTag(buffer, crate::types::DISK_POOL_TAG);
        return Err(STATUS_UNSUCCESSFUL);
    }
    
    // Читаем revision
    let rev = core::ptr::read_unaligned((buffer as usize + 8) as *const u32);
    
    disk_print("[DISK/GPT] Revision: 0x");
    disk_print_hex(rev as u64);
    disk_print("\n");
    
    if rev != GPT_REVISION_1_0 {
        disk_print("[DISK/GPT] WARNING: Unexpected revision, trying anyway\n");
        // Не возвращаем ошибку - пробуем парсить
    }
    
    // Читаем параметры partition array
    // Offset 72: partition_entry_lba (u64)
    // Offset 80: number_of_entries (u32)
    // Offset 84: size_of_entry (u32)
    let partition_entry_lba = core::ptr::read_unaligned((buffer as usize + 72) as *const u64);
    let number_of_entries = core::ptr::read_unaligned((buffer as usize + 80) as *const u32);
    let size_of_entry = core::ptr::read_unaligned((buffer as usize + 84) as *const u32);
    
    disk_print("[DISK/GPT] partition_entry_lba=");
    disk_print_dec(partition_entry_lba);
    disk_print(" entries=");
    disk_print_dec(number_of_entries as u64);
    disk_print(" entry_size=");
    disk_print_dec(size_of_entry as u64);
    disk_print("\n");
    
    ExFreePoolWithTag(buffer, crate::types::DISK_POOL_TAG);
    
    // Валидация параметров
    if partition_entry_lba < 2 || number_of_entries == 0 || size_of_entry < 128 {
        disk_print("[DISK/GPT] ERROR: Invalid GPT parameters\n");
        return Err(STATUS_UNSUCCESSFUL);
    }
    
    // Ограничиваем количество entries
    let max_entries = core::cmp::min(number_of_entries as usize, 128);
    let entries_to_read = core::cmp::min(max_entries, MAX_PARTITIONS * 2); // Читаем с запасом
    
    // Выделяем буфер для partition entries
    let entries_size = entries_to_read * size_of_entry as usize;
    let entries_buffer = ExAllocatePoolWithTag(
        NON_PAGED_POOL,
        entries_size,
        crate::types::DISK_POOL_TAG
    );
    
    if entries_buffer.is_null() {
        disk_print("[DISK/GPT] ERROR: Cannot allocate entries buffer\n");
        return Err(STATUS_INSUFFICIENT_RESOURCES);
    }
    
    core::ptr::write_bytes(entries_buffer as *mut u8, 0, entries_size);
    
    // Читаем partition entries
    let entries_offset = partition_entry_lba * sector_size;
    
    disk_print("[DISK/GPT] Reading ");
    disk_print_dec(entries_to_read as u64);
    disk_print(" entries from offset ");
    disk_print_dec(entries_offset);
    disk_print(" (");
    disk_print_dec(entries_size as u64);
    disk_print(" bytes)\n");
    
    let status = super::io::read_from_device(
        lower_device,
        entries_offset,
        entries_size as u32,
        entries_buffer
    );
    
    if status < 0 {
        disk_print("[DISK/GPT] ERROR: Read entries failed, status=0x");
        disk_print_hex(status as u32 as u64);
        disk_print("\n");
        ExFreePoolWithTag(entries_buffer, crate::types::DISK_POOL_TAG);
        return Err(status);
    }
    
    // Парсим entries
    let mut count = 0usize;
    
    disk_print("[DISK/GPT] Parsing partition entries:\n");
    
    for i in 0..entries_to_read {
        if count >= MAX_PARTITIONS {
            break;
        }
        
        let entry_offset = i * size_of_entry as usize;
        let entry_ptr = (entries_buffer as usize + entry_offset) as *const u8;
        
        // Читаем type GUID (первые 16 байт)
        let mut type_guid = [0u8; 16];
        for j in 0..16 {
            type_guid[j] = core::ptr::read(entry_ptr.add(j));
        }
        
        // Проверяем что partition не пустая
        if is_guid_empty(&type_guid) {
            continue;
        }
        
        // Читаем LBA (offset 32 и 40)
        let starting_lba = core::ptr::read_unaligned((entry_ptr as usize + 32) as *const u64);
        let ending_lba = core::ptr::read_unaligned((entry_ptr as usize + 40) as *const u64);
        let attributes = core::ptr::read_unaligned((entry_ptr as usize + 48) as *const u64);
        
        // Вычисляем размер в секторах
        let sector_count = if ending_lba >= starting_lba {
            ending_lba - starting_lba + 1
        } else {
            0
        };
        
        // Конвертируем GUID в MBR тип
        let mbr_type = guid_to_mbr_type(&type_guid);
        
        disk_print("[DISK/GPT]   Entry ");
        disk_print_dec(i as u64);
        disk_print(": GUID=");
        // Выводим первые 4 байта GUID
        for j in 0..4 {
            disk_print_hex(type_guid[j] as u64);
        }
        disk_print("... LBA=");
        disk_print_dec(starting_lba);
        disk_print("-");
        disk_print_dec(ending_lba);
        disk_print(" (");
        disk_print_dec(sector_count);
        disk_print(" sectors) type=0x");
        disk_print_hex(mbr_type as u64);
        disk_print("\n");
        
        partitions[count] = PartitionInfo {
            partition_number: (count + 1) as u32, // 1-based
            partition_type: mbr_type,
            bootable: (attributes & 0x04) != 0, // Legacy BIOS bootable
            starting_lba,
            sector_count,
        };
        count += 1;
    }
    
    ExFreePoolWithTag(entries_buffer, crate::types::DISK_POOL_TAG);
    
    disk_print("[DISK/GPT] Found ");
    disk_print_dec(count as u64);
    disk_print(" partition(s)\n");
    
    Ok(count)
}
