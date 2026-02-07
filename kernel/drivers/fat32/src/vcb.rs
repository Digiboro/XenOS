//! Volume Control Block (VCB) для FAT32
//!
//! VCB хранит информацию о смонтированном FAT32 томе.

use crate::bpb::FAT32_BPB;
use crate::types::*;

/// Volume Control Block для FAT32
#[repr(C)]
pub struct FAT32_VCB {
    /// Тип structure (для диагностики)
    pub node_type: u16,
    /// Размер structure
    pub node_size: u16,
    
    /// Target device object (storage device)
    pub target_device: PDEVICE_OBJECT,
    /// Volume device object (этот FS device)
    pub volume_device: PDEVICE_OBJECT,
    /// VPB для этого тома
    pub vpb: PVPB,
    
    /// Копия BPB
    pub bpb: FAT32_BPB,
    
    /// Cached значения из BPB
    pub bytes_per_sector: u32,
    pub sectors_per_cluster: u32,
    pub bytes_per_cluster: u32,
    pub fat_start_sector: u32,
    pub data_start_sector: u32,
    pub root_directory_cluster: u32,
    
    /// Volume serial number
    pub serial_number: u32,
    
    /// Volume label (Unicode)
    pub volume_label: [u16; 11],
    
    /// Flags
    pub flags: u32,
    
    /// Reference count
    pub reference_count: u32,
    
    /// Open file count
    pub open_file_count: u32,
}

impl FAT32_VCB {
    pub const NODE_TYPE: u16 = 0x0530; // VCB signature
    
    pub fn new() -> Self {
        Self {
            node_type: Self::NODE_TYPE,
            node_size: core::mem::size_of::<Self>() as u16,
            target_device: core::ptr::null_mut(),
            volume_device: core::ptr::null_mut(),
            vpb: core::ptr::null_mut(),
            bpb: unsafe { core::mem::zeroed() },
            bytes_per_sector: 0,
            sectors_per_cluster: 0,
            bytes_per_cluster: 0,
            fat_start_sector: 0,
            data_start_sector: 0,
            root_directory_cluster: 0,
            serial_number: 0,
            volume_label: [0; 11],
            flags: 0,
            reference_count: 1,
            open_file_count: 0,
        }
    }
    
    /// Инициализирует VCB из BPB
    pub fn init_from_bpb(&mut self, bpb: &FAT32_BPB) {
        self.bpb = *bpb;
        self.bytes_per_sector = bpb.bytes_per_sector as u32;
        self.sectors_per_cluster = bpb.sectors_per_cluster as u32;
        self.bytes_per_cluster = bpb.cluster_size();
        self.fat_start_sector = bpb.fat_start_sector();
        self.data_start_sector = bpb.data_start_sector();
        self.root_directory_cluster = bpb.root_cluster;
        self.serial_number = bpb.volume_id;
        
        // Конвертируем volume label из ASCII в Unicode
        for i in 0..11 {
            self.volume_label[i] = bpb.volume_label[i] as u16;
        }
    }
}

// VCB Flags
pub const VCB_FLAGS_VOLUME_MOUNTED: u32 = 0x00000001;
pub const VCB_FLAGS_VOLUME_LOCKED: u32 = 0x00000002;
pub const VCB_FLAGS_DISMOUNT_IN_PROGRESS: u32 = 0x00000004;

