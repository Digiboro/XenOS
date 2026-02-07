//! FAT32 Boot Sector и BPB (BIOS Parameter Block)
//!
//! Структуры для чтения и парсинга FAT32 boot sector.
//!
//! Источники:
//! - FAT32 File System Specification (Microsoft)
//! - ReactOS: drivers/filesystems/fastfat/

use crate::types::*;

// =============================================================================
// FAT32 Boot Sector
// =============================================================================

/// FAT32 BIOS Parameter Block
///
/// Sector 0 содержит boot sector с BPB.
/// Бинарно совместим с FAT32 specification.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct FAT32_BPB {
    // DOS 2.0 BPB (offset 0x00-0x23)
    pub jump_boot: [u8; 3],              // 0x00: JMP instruction
    pub oem_name: [u8; 8],               // 0x03: OEM name
    pub bytes_per_sector: u16,           // 0x0B: Bytes per sector (обычно 512)
    pub sectors_per_cluster: u8,         // 0x0D: Sectors per cluster (power of 2)
    pub reserved_sector_count: u16,      // 0x0E: Reserved sectors (обычно 32 для FAT32)
    pub num_fats: u8,                    // 0x10: Number of FATs (обычно 2)
    pub root_entry_count: u16,           // 0x11: Root entries (0 для FAT32)
    pub total_sectors_16: u16,           // 0x13: Total sectors (0 если > 65535)
    pub media: u8,                       // 0x15: Media descriptor (0xF8 для HDD)
    pub fat_size_16: u16,                // 0x16: FAT size (0 для FAT32)
    pub sectors_per_track: u16,          // 0x18: Sectors per track (CHS)
    pub num_heads: u16,                  // 0x1A: Number of heads (CHS)
    pub hidden_sectors: u32,             // 0x1C: Hidden sectors
    pub total_sectors_32: u32,           // 0x20: Total sectors (если > 65535)
    
    // FAT32 Extended BPB (offset 0x24-0x59)
    pub fat_size_32: u32,                // 0x24: FAT size в секторах
    pub ext_flags: u16,                  // 0x28: Extended flags
    pub fs_version: u16,                 // 0x2A: File system version (0x00 0x00)
    pub root_cluster: u32,               // 0x2C: Root directory cluster (обычно 2)
    pub fs_info: u16,                    // 0x30: FSInfo sector number (обычно 1)
    pub backup_boot_sector: u16,         // 0x32: Backup boot sector (обычно 6)
    pub reserved: [u8; 12],              // 0x34: Reserved
    pub drive_number: u8,                // 0x40: Drive number (0x80 для HDD)
    pub reserved1: u8,                   // 0x41: Reserved
    pub boot_signature: u8,              // 0x42: Extended boot signature (0x29)
    pub volume_id: u32,                  // 0x43: Volume serial number
    pub volume_label: [u8; 11],          // 0x47: Volume label (ASCII, space-padded)
    pub file_system_type: [u8; 8],       // 0x52: "FAT32   " (ASCII)
    
    // Boot code (offset 0x5A-0x1FD)
    // pub boot_code: [u8; 420],         // Не включаем для экономии памяти
    
    // Signature (offset 0x1FE-0x1FF)
    // pub signature: u16,                // 0xAA55
}

impl FAT32_BPB {
    /// Размер BPB structure (без boot code и signature)
    pub const SIZE: usize = 0x5A; // 90 bytes
    
    /// Проверяет валидность FAT boot sector (FAT16 или FAT32)
    pub fn is_valid(&self, boot_sector: &[u8; 512]) -> bool {
        // Проверка signature 0xAA55
        if boot_sector[510] != 0x55 || boot_sector[511] != 0xAA {
            return false;
        }
        
        // Проверка bytes per sector (должно быть степенью 2, обычно 512)
        let bps = self.bytes_per_sector;
        if bps < 512 || bps > 4096 || (bps & (bps - 1)) != 0 {
            return false;
        }
        
        // Проверка sectors per cluster (должно быть степенью 2)
        let spc = self.sectors_per_cluster;
        if spc == 0 || (spc & (spc - 1)) != 0 {
            return false;
        }
        
        // Определяем тип FAT
        let is_fat32 = self.fat_size_16 == 0 && self.root_entry_count == 0 && self.fat_size_32 > 0;
        let is_fat16 = self.fat_size_16 > 0 && self.root_entry_count > 0;
        
        // Поддерживаем FAT16 и FAT32
        if !is_fat16 && !is_fat32 {
            return false;
        }
        
        true
    }
    
    /// Определяет тип FAT (16 или 32)
    pub fn fat_type(&self) -> u8 {
        if self.fat_size_16 == 0 && self.root_entry_count == 0 && self.fat_size_32 > 0 {
            32 // FAT32
        } else {
            16 // FAT16
        }
    }
    
    /// Возвращает размер кластера в байтах
    pub fn cluster_size(&self) -> u32 {
        self.bytes_per_sector as u32 * self.sectors_per_cluster as u32
    }
    
    /// Возвращает начало первой FAT (в секторах от начала партиции)
    pub fn fat_start_sector(&self) -> u32 {
        self.reserved_sector_count as u32
    }
    
    /// Возвращает начало data region (в секторах)
    pub fn data_start_sector(&self) -> u32 {
        self.reserved_sector_count as u32 + (self.num_fats as u32 * self.fat_size_32)
    }
    
    /// Конвертирует cluster number в LBA
    pub fn cluster_to_lba(&self, cluster: u32) -> u64 {
        // Cluster 2 это начало data region
        if cluster < 2 {
            return 0;
        }
        
        let cluster_offset = cluster - 2;
        let data_start = self.data_start_sector();
        let sectors = data_start as u64 + (cluster_offset as u64 * self.sectors_per_cluster as u64);
        
        sectors
    }
}

// =============================================================================
// FAT Entry
// =============================================================================

/// FAT32 entry type
pub type FAT32Entry = u32;

// FAT32 Special Values
pub const FAT32_FREE: u32 = 0x00000000;
pub const FAT32_BAD: u32 = 0x0FFFFFF7;
pub const FAT32_EOC_MIN: u32 = 0x0FFFFFF8; // End of chain
pub const FAT32_EOC_MAX: u32 = 0x0FFFFFFF;

/// Проверяет является ли FAT entry end-of-chain
#[inline]
pub fn is_eoc(entry: FAT32Entry) -> bool {
    let masked = entry & 0x0FFFFFFF;
    masked >= FAT32_EOC_MIN
}

/// Проверяет является ли cluster валидным
#[inline]
pub fn is_valid_cluster(entry: FAT32Entry) -> bool {
    let masked = entry & 0x0FFFFFFF;
    masked >= 2 && masked < FAT32_BAD
}

// =============================================================================
// Directory Entry
// =============================================================================

/// FAT32 Directory Entry (32 bytes)
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct FAT32DirEntry {
    pub name: [u8; 11],              // 0x00: 8.3 filename (space-padded)
    pub attr: u8,                    // 0x0B: Attributes
    pub nt_reserved: u8,             // 0x0C: Reserved for NT
    pub creation_time_tenth: u8,     // 0x0D: Creation time (tenths of second)
    pub creation_time: u16,          // 0x0E: Creation time
    pub creation_date: u16,          // 0x10: Creation date
    pub last_access_date: u16,       // 0x12: Last access date
    pub first_cluster_high: u16,     // 0x14: High word of first cluster
    pub write_time: u16,             // 0x16: Last write time
    pub write_date: u16,             // 0x18: Last write date
    pub first_cluster_low: u16,      // 0x1A: Low word of first cluster
    pub file_size: u32,              // 0x1C: File size in bytes
}

impl FAT32DirEntry {
    pub const SIZE: usize = 32;
    
    /// Получает полный cluster number (комбинация high и low)
    pub fn first_cluster(&self) -> u32 {
        ((self.first_cluster_high as u32) << 16) | (self.first_cluster_low as u32)
    }
    
    /// Проверяет что entry не удалён и не конец directory
    pub fn is_valid(&self) -> bool {
        self.name[0] != 0x00 && self.name[0] != 0xE5
    }
    
    /// Проверяет что это не LFN entry
    pub fn is_lfn(&self) -> bool {
        self.attr == ATTR_LONG_NAME
    }
    
    /// Проверяет что это directory
    pub fn is_directory(&self) -> bool {
        (self.attr & ATTR_DIRECTORY) != 0
    }
}

// FAT Attributes
pub const ATTR_READ_ONLY: u8 = 0x01;
pub const ATTR_HIDDEN: u8 = 0x02;
pub const ATTR_SYSTEM: u8 = 0x04;
pub const ATTR_VOLUME_ID: u8 = 0x08;
pub const ATTR_DIRECTORY: u8 = 0x10;
pub const ATTR_ARCHIVE: u8 = 0x20;
pub const ATTR_LONG_NAME: u8 = 0x0F; // READ_ONLY | HIDDEN | SYSTEM | VOLUME_ID


