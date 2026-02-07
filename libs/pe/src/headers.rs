//! PE header structures
//!
//! Contains raw C-compatible structures for DOS header, COFF header,
//! Optional header (PE32/PE32+), data directories, and section headers.

use crate::constants::*;

// =============================================================================
// DOS Header
// =============================================================================

/// DOS Header (IMAGE_DOS_HEADER)
///
/// The DOS header is at the very beginning of every PE file.
/// It's used to maintain backward compatibility with DOS.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct IMAGE_DOS_HEADER {
    /// Magic number ("MZ")
    pub e_magic: u16,
    /// Bytes on last page of file
    pub e_cblp: u16,
    /// Pages in file
    pub e_cp: u16,
    /// Relocations
    pub e_crlc: u16,
    /// Size of header in paragraphs
    pub e_cparhdr: u16,
    /// Minimum extra paragraphs needed
    pub e_minalloc: u16,
    /// Maximum extra paragraphs needed
    pub e_maxalloc: u16,
    /// Initial (relative) SS value
    pub e_ss: u16,
    /// Initial SP value
    pub e_sp: u16,
    /// Checksum
    pub e_csum: u16,
    /// Initial IP value
    pub e_ip: u16,
    /// Initial (relative) CS value
    pub e_cs: u16,
    /// File address of relocation table
    pub e_lfarlc: u16,
    /// Overlay number
    pub e_ovno: u16,
    /// Reserved words
    pub e_res: [u16; 4],
    /// OEM identifier (for e_oeminfo)
    pub e_oemid: u16,
    /// OEM information; e_oemid specific
    pub e_oeminfo: u16,
    /// Reserved words
    pub e_res2: [u16; 10],
    /// File offset to PE header (e_lfanew)
    pub e_lfanew: i32,
}

impl IMAGE_DOS_HEADER {
    /// Check if this is a valid DOS header
    #[inline]
    pub fn is_valid(&self) -> bool {
        self.e_magic == IMAGE_DOS_SIGNATURE
    }

    /// Get the offset to the PE header
    #[inline]
    pub fn pe_offset(&self) -> usize {
        self.e_lfanew as usize
    }
}

// =============================================================================
// COFF File Header
// =============================================================================

/// COFF File Header (IMAGE_FILE_HEADER)
///
/// Located immediately after the PE signature ("PE\0\0").
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct IMAGE_FILE_HEADER {
    /// Target machine type
    pub machine: u16,
    /// Number of sections
    pub number_of_sections: u16,
    /// Timestamp (seconds since 1970-01-01)
    pub time_date_stamp: u32,
    /// Pointer to symbol table (deprecated)
    pub pointer_to_symbol_table: u32,
    /// Number of symbols (deprecated)
    pub number_of_symbols: u32,
    /// Size of optional header
    pub size_of_optional_header: u16,
    /// File characteristics flags
    pub characteristics: u16,
}

impl IMAGE_FILE_HEADER {
    /// Check if this is AMD64 architecture
    #[inline]
    pub fn is_amd64(&self) -> bool {
        self.machine == IMAGE_FILE_MACHINE_AMD64
    }

    /// Check if this is i386 architecture
    #[inline]
    pub fn is_i386(&self) -> bool {
        self.machine == IMAGE_FILE_MACHINE_I386
    }

    /// Check if this is a DLL
    #[inline]
    pub fn is_dll(&self) -> bool {
        self.characteristics & IMAGE_FILE_DLL != 0
    }

    /// Check if this is executable
    #[inline]
    pub fn is_executable(&self) -> bool {
        self.characteristics & IMAGE_FILE_EXECUTABLE_IMAGE != 0
    }
}

// =============================================================================
// Data Directory Entry
// =============================================================================

/// Data Directory Entry (IMAGE_DATA_DIRECTORY)
///
/// Points to a specific data structure in the PE file.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug, Default)]
pub struct IMAGE_DATA_DIRECTORY {
    /// Relative virtual address
    pub virtual_address: u32,
    /// Size in bytes
    pub size: u32,
}

impl IMAGE_DATA_DIRECTORY {
    /// Check if this directory entry is present
    #[inline]
    pub fn is_present(&self) -> bool {
        self.virtual_address != 0 && self.size != 0
    }
}

// =============================================================================
// Optional Header (PE32+, 64-bit)
// =============================================================================

/// Optional Header for PE32+ (64-bit) (IMAGE_OPTIONAL_HEADER64)
///
/// Contains information about the image layout in memory.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct IMAGE_OPTIONAL_HEADER64 {
    /// Magic number (0x20B for PE32+)
    pub magic: u16,
    /// Major linker version
    pub major_linker_version: u8,
    /// Minor linker version
    pub minor_linker_version: u8,
    /// Size of code section(s)
    pub size_of_code: u32,
    /// Size of initialized data section(s)
    pub size_of_initialized_data: u32,
    /// Size of uninitialized data section(s)
    pub size_of_uninitialized_data: u32,
    /// RVA of entry point
    pub address_of_entry_point: u32,
    /// RVA of code section start
    pub base_of_code: u32,
    /// Preferred base address (64-bit)
    pub image_base: u64,
    /// Section alignment in memory
    pub section_alignment: u32,
    /// File alignment on disk
    pub file_alignment: u32,
    /// Required OS major version
    pub major_operating_system_version: u16,
    /// Required OS minor version
    pub minor_operating_system_version: u16,
    /// Image major version
    pub major_image_version: u16,
    /// Image minor version
    pub minor_image_version: u16,
    /// Subsystem major version
    pub major_subsystem_version: u16,
    /// Subsystem minor version
    pub minor_subsystem_version: u16,
    /// Reserved (must be zero)
    pub win32_version_value: u32,
    /// Size of image in memory
    pub size_of_image: u32,
    /// Size of headers (rounded up to file alignment)
    pub size_of_headers: u32,
    /// Image checksum
    pub checksum: u32,
    /// Required subsystem
    pub subsystem: u16,
    /// DLL characteristics flags
    pub dll_characteristics: u16,
    /// Size of stack to reserve
    pub size_of_stack_reserve: u64,
    /// Size of stack to commit
    pub size_of_stack_commit: u64,
    /// Size of heap to reserve
    pub size_of_heap_reserve: u64,
    /// Size of heap to commit
    pub size_of_heap_commit: u64,
    /// Loader flags (reserved, must be zero)
    pub loader_flags: u32,
    /// Number of data directory entries
    pub number_of_rva_and_sizes: u32,
    // Data directories follow (not included in this struct)
}

impl IMAGE_OPTIONAL_HEADER64 {
    /// Check if this is a valid PE32+ header
    #[inline]
    pub fn is_valid(&self) -> bool {
        self.magic == IMAGE_NT_OPTIONAL_HDR64_MAGIC
    }

    /// Check if ASLR is enabled
    #[inline]
    pub fn is_dynamic_base(&self) -> bool {
        self.dll_characteristics & IMAGE_DLLCHARACTERISTICS_DYNAMIC_BASE != 0
    }

    /// Check if NX compatible
    #[inline]
    pub fn is_nx_compat(&self) -> bool {
        self.dll_characteristics & IMAGE_DLLCHARACTERISTICS_NX_COMPAT != 0
    }
}

// =============================================================================
// Optional Header (PE32, 32-bit)
// =============================================================================

/// Optional Header for PE32 (32-bit) (IMAGE_OPTIONAL_HEADER32)
///
/// Contains information about the image layout in memory.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct IMAGE_OPTIONAL_HEADER32 {
    /// Magic number (0x10B for PE32)
    pub magic: u16,
    /// Major linker version
    pub major_linker_version: u8,
    /// Minor linker version
    pub minor_linker_version: u8,
    /// Size of code section(s)
    pub size_of_code: u32,
    /// Size of initialized data section(s)
    pub size_of_initialized_data: u32,
    /// Size of uninitialized data section(s)
    pub size_of_uninitialized_data: u32,
    /// RVA of entry point
    pub address_of_entry_point: u32,
    /// RVA of code section start
    pub base_of_code: u32,
    /// RVA of data section start (not in PE32+)
    pub base_of_data: u32,
    /// Preferred base address (32-bit)
    pub image_base: u32,
    /// Section alignment in memory
    pub section_alignment: u32,
    /// File alignment on disk
    pub file_alignment: u32,
    /// Required OS major version
    pub major_operating_system_version: u16,
    /// Required OS minor version
    pub minor_operating_system_version: u16,
    /// Image major version
    pub major_image_version: u16,
    /// Image minor version
    pub minor_image_version: u16,
    /// Subsystem major version
    pub major_subsystem_version: u16,
    /// Subsystem minor version
    pub minor_subsystem_version: u16,
    /// Reserved (must be zero)
    pub win32_version_value: u32,
    /// Size of image in memory
    pub size_of_image: u32,
    /// Size of headers (rounded up to file alignment)
    pub size_of_headers: u32,
    /// Image checksum
    pub checksum: u32,
    /// Required subsystem
    pub subsystem: u16,
    /// DLL characteristics flags
    pub dll_characteristics: u16,
    /// Size of stack to reserve
    pub size_of_stack_reserve: u32,
    /// Size of stack to commit
    pub size_of_stack_commit: u32,
    /// Size of heap to reserve
    pub size_of_heap_reserve: u32,
    /// Size of heap to commit
    pub size_of_heap_commit: u32,
    /// Loader flags (reserved, must be zero)
    pub loader_flags: u32,
    /// Number of data directory entries
    pub number_of_rva_and_sizes: u32,
    // Data directories follow (not included in this struct)
}

impl IMAGE_OPTIONAL_HEADER32 {
    /// Check if this is a valid PE32 header
    #[inline]
    pub fn is_valid(&self) -> bool {
        self.magic == IMAGE_NT_OPTIONAL_HDR32_MAGIC
    }
}

// =============================================================================
// Section Header
// =============================================================================

/// Section Header (IMAGE_SECTION_HEADER)
///
/// Describes a section in the PE file.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct IMAGE_SECTION_HEADER {
    /// Section name (8 bytes, null-padded)
    pub name: [u8; SECTION_NAME_SIZE],
    /// Virtual size (or physical address for object files)
    pub virtual_size: u32,
    /// RVA of section start
    pub virtual_address: u32,
    /// Size of raw data on disk
    pub size_of_raw_data: u32,
    /// File offset of raw data
    pub pointer_to_raw_data: u32,
    /// File offset of relocations
    pub pointer_to_relocations: u32,
    /// File offset of line numbers
    pub pointer_to_linenumbers: u32,
    /// Number of relocations
    pub number_of_relocations: u16,
    /// Number of line numbers
    pub number_of_linenumbers: u16,
    /// Section characteristics flags
    pub characteristics: u32,
}

impl IMAGE_SECTION_HEADER {
    /// Get section name as string slice (trimming null bytes)
    pub fn name_str(&self) -> &str {
        let len = self
            .name
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(SECTION_NAME_SIZE);
        core::str::from_utf8(&self.name[..len]).unwrap_or("???")
    }

    /// Check if section contains code
    #[inline]
    pub fn is_code(&self) -> bool {
        self.characteristics & IMAGE_SCN_CNT_CODE != 0
    }

    /// Check if section is executable
    #[inline]
    pub fn is_executable(&self) -> bool {
        self.characteristics & IMAGE_SCN_MEM_EXECUTE != 0
    }

    /// Check if section is readable
    #[inline]
    pub fn is_readable(&self) -> bool {
        self.characteristics & IMAGE_SCN_MEM_READ != 0
    }

    /// Check if section is writable
    #[inline]
    pub fn is_writable(&self) -> bool {
        self.characteristics & IMAGE_SCN_MEM_WRITE != 0
    }

    /// Check if section contains initialized data
    #[inline]
    pub fn is_initialized_data(&self) -> bool {
        self.characteristics & IMAGE_SCN_CNT_INITIALIZED_DATA != 0
    }

    /// Check if section contains uninitialized data (BSS)
    #[inline]
    pub fn is_uninitialized_data(&self) -> bool {
        self.characteristics & IMAGE_SCN_CNT_UNINITIALIZED_DATA != 0
    }

    /// Check if section can be discarded
    #[inline]
    pub fn is_discardable(&self) -> bool {
        self.characteristics & IMAGE_SCN_MEM_DISCARDABLE != 0
    }
}

// =============================================================================
// Export Directory
// =============================================================================

/// Export Directory (IMAGE_EXPORT_DIRECTORY)
///
/// Contains information about exported functions.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct IMAGE_EXPORT_DIRECTORY {
    /// Reserved, must be 0
    pub characteristics: u32,
    /// Time and date the export data was created
    pub time_date_stamp: u32,
    /// Major version number
    pub major_version: u16,
    /// Minor version number
    pub minor_version: u16,
    /// RVA of the ASCII string containing the name of the DLL
    pub name: u32,
    /// Starting ordinal number for exports
    pub base: u32,
    /// Number of entries in the Export Address Table
    pub number_of_functions: u32,
    /// Number of entries in the Name Pointer Table
    pub number_of_names: u32,
    /// RVA of the Export Address Table
    pub address_of_functions: u32,
    /// RVA of the Export Name Pointer Table
    pub address_of_names: u32,
    /// RVA of the Ordinal Table
    pub address_of_name_ordinals: u32,
}

// =============================================================================
// Import Directory
// =============================================================================

/// Import Descriptor (IMAGE_IMPORT_DESCRIPTOR)
///
/// Describes imports from a single DLL.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct IMAGE_IMPORT_DESCRIPTOR {
    /// RVA of Import Lookup Table (or Characteristics if bound)
    pub original_first_thunk: u32,
    /// Time/date stamp (0 if not bound, -1 if bound with new-style binding)
    pub time_date_stamp: u32,
    /// Index of first forwarder reference (-1 if no forwarders)
    pub forwarder_chain: u32,
    /// RVA of DLL name
    pub name: u32,
    /// RVA of Import Address Table
    pub first_thunk: u32,
}

impl IMAGE_IMPORT_DESCRIPTOR {
    /// Check if this is the terminating null descriptor
    #[inline]
    pub fn is_null(&self) -> bool {
        self.name == 0 && self.first_thunk == 0
    }
}

// =============================================================================
// Base Relocation
// =============================================================================

/// Base Relocation Block Header (IMAGE_BASE_RELOCATION)
///
/// Each relocation block starts with this header.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct IMAGE_BASE_RELOCATION {
    /// Page RVA (base for relocations in this block)
    pub virtual_address: u32,
    /// Size of this block including header
    pub size_of_block: u32,
    // Type/Offset entries follow (each 2 bytes)
}

impl IMAGE_BASE_RELOCATION {
    /// Calculate number of entries in this block
    #[inline]
    pub fn entry_count(&self) -> usize {
        if self.size_of_block < 8 {
            0
        } else {
            (self.size_of_block as usize - 8) / 2
        }
    }
}

// =============================================================================
// Import by Name
// =============================================================================

/// Import by Name Entry (IMAGE_IMPORT_BY_NAME)
///
/// Used for imports by name (as opposed to ordinal).
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct IMAGE_IMPORT_BY_NAME {
    /// Hint (index into export name table)
    pub hint: u16,
    /// Name starts here (null-terminated)
    pub name: [u8; 1],
}

