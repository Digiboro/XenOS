//! File Control Block (FCB) и Context Control Block (CCB)
//!
//! FCB хранит информацию об открытом файле/директории.
//! CCB хранит per-handle контекст (current position, flags).

pub use crate::types::*;
pub use crate::vcb::FAT32_VCB;

/// File Control Block
///
/// Создаётся один на файл (shared между handles).
#[repr(C)]
pub struct FAT32_FCB {
    /// Тип structure (для диагностики)
    pub node_type: u16,
    /// Размер structure
    pub node_size: u16,
    
    /// VCB для этого тома
    pub vcb: *mut FAT32_VCB,
    
    /// Первый cluster файла (0 для пустых файлов)
    pub first_cluster: u32,
    
    /// Размер файла в байтах
    pub file_size: u32,
    
    /// Флаги
    pub flags: u32,
    
    /// Атрибуты файла
    pub attributes: u8,
    
    /// Reference count (количество открытых handles)
    pub reference_count: u32,
    
    /// Имя файла (8.3 формат, NULL-terminated)
    pub file_name: [u8; 13],
    
    /// Cluster родительской директории (для обновления directory entry)
    pub parent_dir_cluster: u32,
}

impl FAT32_FCB {
    pub const NODE_TYPE: u16 = 0x0528; // FCB signature
    
    pub fn new(vcb: *mut FAT32_VCB) -> Self {
        Self {
            node_type: Self::NODE_TYPE,
            node_size: core::mem::size_of::<Self>() as u16,
            vcb,
            first_cluster: 0,
            file_size: 0,
            flags: 0,
            attributes: 0,
            reference_count: 1,
            file_name: [0; 13],
            parent_dir_cluster: 0,
        }
    }
    
    pub fn is_directory(&self) -> bool {
        (self.attributes & ATTR_DIRECTORY) != 0
    }
    
    pub fn is_root_directory(&self) -> bool {
        self.flags & FCB_FLAGS_ROOT_DIRECTORY != 0
    }
}

/// Context Control Block
///
/// Создаётся один на handle (каждый IoCreate создаёт новый CCB).
#[repr(C)]
pub struct FAT32_CCB {
    /// Тип structure
    pub node_type: u16,
    /// Размер structure
    pub node_size: u16,
    
    /// FCB для этого handle
    pub fcb: *mut FAT32_FCB,
    
    /// Текущая позиция в файле (для read/write)
    pub current_position: u64,
    
    /// Флаги
    pub flags: u32,
}

impl FAT32_CCB {
    pub const NODE_TYPE: u16 = 0x0527; // CCB signature
    
    pub fn new(fcb: *mut FAT32_FCB) -> Self {
        Self {
            node_type: Self::NODE_TYPE,
            node_size: core::mem::size_of::<Self>() as u16,
            fcb,
            current_position: 0,
            flags: 0,
        }
    }
}

// FCB Flags
pub const FCB_FLAGS_ROOT_DIRECTORY: u32 = 0x00000001;
pub const FCB_FLAGS_VOLUME_LABEL: u32 = 0x00000002;
pub const FCB_FLAGS_MODIFIED: u32 = 0x00000004;
pub const FCB_FLAGS_DELETE_ON_CLOSE: u32 = 0x00000008;

// CCB Flags
pub const CCB_FLAGS_DIRECTORY_SCAN: u32 = 0x00000001;

// File Attributes (копия из bpb.rs)
pub const ATTR_READ_ONLY: u8 = 0x01;
pub const ATTR_HIDDEN: u8 = 0x02;
pub const ATTR_SYSTEM: u8 = 0x04;
pub const ATTR_VOLUME_ID: u8 = 0x08;
pub const ATTR_DIRECTORY: u8 = 0x10;
pub const ATTR_ARCHIVE: u8 = 0x20;
pub const ATTR_LONG_NAME: u8 = 0x0F;

