//! Volume Information Queries
//!
//! Реализация IRP_MJ_QUERY_VOLUME_INFORMATION для получения информации о томе.

use crate::types::*;
use crate::vcb::FAT32_VCB;

// =============================================================================
// FS Information Classes
// =============================================================================

pub const FS_VOLUME_INFORMATION: u32 = 1;
pub const FS_SIZE_INFORMATION: u32 = 3;
pub const FS_DEVICE_INFORMATION: u32 = 4;
pub const FS_ATTRIBUTE_INFORMATION: u32 = 5;
pub const FS_FULL_SIZE_INFORMATION: u32 = 7;

// =============================================================================
// Structures
// =============================================================================

/// FILE_FS_VOLUME_INFORMATION
#[repr(C)]
pub struct FILE_FS_VOLUME_INFORMATION {
    pub volume_creation_time: i64,
    pub volume_serial_number: u32,
    pub volume_label_length: u32,
    pub supports_objects: u8,
    // volume_label follows (variable length)
}

/// FILE_FS_SIZE_INFORMATION
#[repr(C)]
pub struct FILE_FS_SIZE_INFORMATION {
    pub total_allocation_units: i64,      // Total clusters
    pub available_allocation_units: i64,  // Free clusters
    pub sectors_per_allocation_unit: u32, // Sectors per cluster
    pub bytes_per_sector: u32,
}

/// FILE_FS_DEVICE_INFORMATION
#[repr(C)]
pub struct FILE_FS_DEVICE_INFORMATION {
    pub device_type: u32,
    pub characteristics: u32,
}

/// FILE_FS_ATTRIBUTE_INFORMATION
#[repr(C)]
pub struct FILE_FS_ATTRIBUTE_INFORMATION {
    pub file_system_attributes: u32,
    pub maximum_component_name_length: u32,
    pub file_system_name_length: u32,
    // file_system_name follows (variable length)
}

// =============================================================================
// Query Volume Information Handler
// =============================================================================

/// Обрабатывает IRP_MJ_QUERY_VOLUME_INFORMATION
pub unsafe fn fat_query_volume_information(
    vcb: *mut FAT32_VCB,
    fs_info_class: u32,
    buffer: *mut u8,
    buffer_length: u32,
    bytes_written: *mut u32,
) -> NTSTATUS {
    unsafe {
        if vcb.is_null() || buffer.is_null() {
            return STATUS_INVALID_PARAMETER;
        }
        
        match fs_info_class {
            FS_VOLUME_INFORMATION => {
                query_volume_info(vcb, buffer, buffer_length, bytes_written)
            }
            FS_SIZE_INFORMATION => {
                query_size_info(vcb, buffer, buffer_length, bytes_written)
            }
            FS_DEVICE_INFORMATION => {
                query_device_info(buffer, buffer_length, bytes_written)
            }
            FS_ATTRIBUTE_INFORMATION => {
                query_attribute_info(buffer, buffer_length, bytes_written)
            }
            _ => STATUS_INVALID_PARAMETER,
        }
    }
}

/// FILE_FS_VOLUME_INFORMATION
unsafe fn query_volume_info(
    vcb: *mut FAT32_VCB,
    buffer: *mut u8,
    buffer_length: u32,
    bytes_written: *mut u32,
) -> NTSTATUS {
    unsafe {
        if buffer_length < core::mem::size_of::<FILE_FS_VOLUME_INFORMATION>() as u32 {
            return STATUS_BUFFER_TOO_SMALL;
        }
        
        let info = buffer as *mut FILE_FS_VOLUME_INFORMATION;
        
        (*info).volume_creation_time = crate::time::get_current_filetime();
        (*info).volume_serial_number = (*vcb).serial_number;
        (*info).supports_objects = 0; // FAT32 не поддерживает object IDs
        
        // Копируем volume label
        let label_len = 11u32; // FAT32 volume label = 11 chars
        (*info).volume_label_length = label_len * 2;
        
        let label_ptr = buffer.add(core::mem::size_of::<FILE_FS_VOLUME_INFORMATION>()) as *mut u16;
        for i in 0..11 {
            *label_ptr.add(i) = (*vcb).volume_label[i];
        }
        
        *bytes_written = core::mem::size_of::<FILE_FS_VOLUME_INFORMATION>() as u32 + label_len * 2;
        STATUS_SUCCESS
    }
}

/// FILE_FS_SIZE_INFORMATION
unsafe fn query_size_info(
    vcb: *mut FAT32_VCB,
    buffer: *mut u8,
    buffer_length: u32,
    bytes_written: *mut u32,
) -> NTSTATUS {
    unsafe {
        if buffer_length < core::mem::size_of::<FILE_FS_SIZE_INFORMATION>() as u32 {
            return STATUS_BUFFER_TOO_SMALL;
        }
        
        let info = buffer as *mut FILE_FS_SIZE_INFORMATION;
        
        // Вычисляем total clusters
        let total_sectors = (*vcb).bpb.total_sectors_32 as u64;
        let data_start = (*vcb).data_start_sector as u64;
        let sectors_per_cluster = (*vcb).sectors_per_cluster as u64;
        
        let data_sectors = if total_sectors > data_start {
            total_sectors - data_start
        } else {
            0
        };
        
        let total_clusters = data_sectors / sectors_per_cluster;
        
        // Вычисляем реальное количество free clusters
        let free_clusters = count_free_clusters(vcb, total_clusters as u32);
        
        (*info).total_allocation_units = total_clusters as i64;
        (*info).available_allocation_units = free_clusters as i64;
        (*info).sectors_per_allocation_unit = sectors_per_cluster as u32;
        (*info).bytes_per_sector = (*vcb).bytes_per_sector;
        
        *bytes_written = core::mem::size_of::<FILE_FS_SIZE_INFORMATION>() as u32;
        STATUS_SUCCESS
    }
}

/// FILE_FS_DEVICE_INFORMATION
unsafe fn query_device_info(
    buffer: *mut u8,
    buffer_length: u32,
    bytes_written: *mut u32,
) -> NTSTATUS {
    unsafe {
        if buffer_length < core::mem::size_of::<FILE_FS_DEVICE_INFORMATION>() as u32 {
            return STATUS_BUFFER_TOO_SMALL;
        }
        
        let info = buffer as *mut FILE_FS_DEVICE_INFORMATION;
        
        (*info).device_type = FILE_DEVICE_DISK_FILE_SYSTEM;
        (*info).characteristics = 0x00020000; // FILE_REMOVABLE_MEDIA (для съёмных дисков)
        
        *bytes_written = core::mem::size_of::<FILE_FS_DEVICE_INFORMATION>() as u32;
        STATUS_SUCCESS
    }
}

/// FILE_FS_ATTRIBUTE_INFORMATION
unsafe fn query_attribute_info(
    buffer: *mut u8,
    buffer_length: u32,
    bytes_written: *mut u32,
) -> NTSTATUS {
    unsafe {
        if buffer_length < core::mem::size_of::<FILE_FS_ATTRIBUTE_INFORMATION>() as u32 + 10 {
            return STATUS_BUFFER_TOO_SMALL;
        }
        
        let info = buffer as *mut FILE_FS_ATTRIBUTE_INFORMATION;
        
        // File system attributes
        (*info).file_system_attributes = 
            FS_CASE_PRESERVED_NAMES |  // Сохраняет регистр
            FS_UNICODE_STORED_ON_DISK; // Unicode имена через LFN
        
        (*info).maximum_component_name_length = 255; // LFN max = 255 chars
        (*info).file_system_name_length = 10; // "FAT32" = 5 chars * 2
        
        // Копируем "FAT32"
        let name_ptr = buffer.add(core::mem::size_of::<FILE_FS_ATTRIBUTE_INFORMATION>()) as *mut u16;
        let fs_name = ['F' as u16, 'A' as u16, 'T' as u16, '3' as u16, '2' as u16];
        for (i, &ch) in fs_name.iter().enumerate() {
            *name_ptr.add(i) = ch;
        }
        
        *bytes_written = core::mem::size_of::<FILE_FS_ATTRIBUTE_INFORMATION>() as u32 + 10;
        STATUS_SUCCESS
    }
}

// =============================================================================
// Constants
// =============================================================================

const FILE_DEVICE_DISK_FILE_SYSTEM: u32 = 0x00000008;

const FS_CASE_PRESERVED_NAMES: u32 = 0x00000002;
const FS_UNICODE_STORED_ON_DISK: u32 = 0x00000004;

const STATUS_BUFFER_TOO_SMALL: NTSTATUS = 0xC0000023u32 as i32;

/// Подсчитывает свободные кластеры в FAT
unsafe fn count_free_clusters(vcb: *mut FAT32_VCB, total_clusters: u32) -> i64 {
    unsafe {
        let mut free_count = 0u32;
        let max_to_scan = total_clusters.min(10000); // Ограничиваем для производительности
        
        // Сканируем FAT начиная с кластера 2
        for cluster in 2..max_to_scan {
            match crate::fat::fat_read_next_cluster(vcb, cluster) {
                Ok(entry) if entry == crate::bpb::FAT32_FREE => {
                    free_count += 1;
                }
                _ => {}
            }
        }
        
        // Если сканировали не всё, экстраполируем
        if max_to_scan < total_clusters {
            let scanned_used = max_to_scan - free_count;
            let usage_ratio = scanned_used as f32 / max_to_scan as f32;
            let estimated_used = (total_clusters as f32 * usage_ratio) as u32;
            free_count = total_clusters - estimated_used;
        }
        
        free_count as i64
    }
}

