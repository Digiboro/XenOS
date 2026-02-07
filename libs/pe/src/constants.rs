//! PE format constants
//!
//! Contains all magic numbers, signatures, flags and indices
//! used in the Portable Executable format.

// =============================================================================
// Signatures
// =============================================================================

/// DOS signature "MZ" (0x5A4D in little-endian)
pub const IMAGE_DOS_SIGNATURE: u16 = 0x5A4D;

/// PE signature "PE\0\0" (0x00004550 in little-endian)
pub const IMAGE_NT_SIGNATURE: u32 = 0x00004550;

/// PE32 magic (32-bit)
pub const IMAGE_NT_OPTIONAL_HDR32_MAGIC: u16 = 0x10B;

/// PE32+ magic (64-bit)
pub const IMAGE_NT_OPTIONAL_HDR64_MAGIC: u16 = 0x20B;

// =============================================================================
// Machine Types
// =============================================================================

/// Unknown machine type
pub const IMAGE_FILE_MACHINE_UNKNOWN: u16 = 0x0000;

/// Intel 386 or later processors
pub const IMAGE_FILE_MACHINE_I386: u16 = 0x014C;

/// ARM little endian
pub const IMAGE_FILE_MACHINE_ARM: u16 = 0x01C0;

/// ARM Thumb-2 little endian
pub const IMAGE_FILE_MACHINE_ARMNT: u16 = 0x01C4;

/// ARM64 little endian
pub const IMAGE_FILE_MACHINE_ARM64: u16 = 0xAA64;

/// AMD64 (x86-64)
pub const IMAGE_FILE_MACHINE_AMD64: u16 = 0x8664;

/// EFI byte code
pub const IMAGE_FILE_MACHINE_EBC: u16 = 0x0EBC;

/// RISC-V 32-bit
pub const IMAGE_FILE_MACHINE_RISCV32: u16 = 0x5032;

/// RISC-V 64-bit
pub const IMAGE_FILE_MACHINE_RISCV64: u16 = 0x5064;

// =============================================================================
// File Characteristics
// =============================================================================

/// Image only, no relocations
pub const IMAGE_FILE_RELOCS_STRIPPED: u16 = 0x0001;

/// Image is executable
pub const IMAGE_FILE_EXECUTABLE_IMAGE: u16 = 0x0002;

/// Line numbers stripped
pub const IMAGE_FILE_LINE_NUMS_STRIPPED: u16 = 0x0004;

/// Local symbols stripped
pub const IMAGE_FILE_LOCAL_SYMS_STRIPPED: u16 = 0x0008;

/// Obsolete: Aggressively trim working set
pub const IMAGE_FILE_AGGRESSIVE_WS_TRIM: u16 = 0x0010;

/// App can handle > 2GB addresses
pub const IMAGE_FILE_LARGE_ADDRESS_AWARE: u16 = 0x0020;

/// Machine word is 32-bit
pub const IMAGE_FILE_32BIT_MACHINE: u16 = 0x0100;

/// Debugging information stripped
pub const IMAGE_FILE_DEBUG_STRIPPED: u16 = 0x0200;

/// If on removable media, copy to swap
pub const IMAGE_FILE_REMOVABLE_RUN_FROM_SWAP: u16 = 0x0400;

/// If on network media, copy to swap
pub const IMAGE_FILE_NET_RUN_FROM_SWAP: u16 = 0x0800;

/// File is a system file
pub const IMAGE_FILE_SYSTEM: u16 = 0x1000;

/// File is a DLL
pub const IMAGE_FILE_DLL: u16 = 0x2000;

/// File should only run on uniprocessor
pub const IMAGE_FILE_UP_SYSTEM_ONLY: u16 = 0x4000;

// =============================================================================
// Section Characteristics
// =============================================================================

/// Section contains code
pub const IMAGE_SCN_CNT_CODE: u32 = 0x00000020;

/// Section contains initialized data
pub const IMAGE_SCN_CNT_INITIALIZED_DATA: u32 = 0x00000040;

/// Section contains uninitialized data
pub const IMAGE_SCN_CNT_UNINITIALIZED_DATA: u32 = 0x00000080;

/// Section contains extended relocations
pub const IMAGE_SCN_LNK_NRELOC_OVFL: u32 = 0x01000000;

/// Section can be discarded
pub const IMAGE_SCN_MEM_DISCARDABLE: u32 = 0x02000000;

/// Section is not cachable
pub const IMAGE_SCN_MEM_NOT_CACHED: u32 = 0x04000000;

/// Section is not pageable
pub const IMAGE_SCN_MEM_NOT_PAGED: u32 = 0x08000000;

/// Section can be shared
pub const IMAGE_SCN_MEM_SHARED: u32 = 0x10000000;

/// Section is executable
pub const IMAGE_SCN_MEM_EXECUTE: u32 = 0x20000000;

/// Section is readable
pub const IMAGE_SCN_MEM_READ: u32 = 0x40000000;

/// Section is writable
pub const IMAGE_SCN_MEM_WRITE: u32 = 0x80000000;

// =============================================================================
// Data Directory Indices
// =============================================================================

/// Export Directory
pub const IMAGE_DIRECTORY_ENTRY_EXPORT: usize = 0;

/// Import Directory
pub const IMAGE_DIRECTORY_ENTRY_IMPORT: usize = 1;

/// Resource Directory
pub const IMAGE_DIRECTORY_ENTRY_RESOURCE: usize = 2;

/// Exception Directory
pub const IMAGE_DIRECTORY_ENTRY_EXCEPTION: usize = 3;

/// Security Directory (Certificate Table)
pub const IMAGE_DIRECTORY_ENTRY_SECURITY: usize = 4;

/// Base Relocation Table
pub const IMAGE_DIRECTORY_ENTRY_BASERELOC: usize = 5;

/// Debug Directory
pub const IMAGE_DIRECTORY_ENTRY_DEBUG: usize = 6;

/// Architecture (reserved, must be 0)
pub const IMAGE_DIRECTORY_ENTRY_ARCHITECTURE: usize = 7;

/// Global Pointer
pub const IMAGE_DIRECTORY_ENTRY_GLOBALPTR: usize = 8;

/// TLS Directory
pub const IMAGE_DIRECTORY_ENTRY_TLS: usize = 9;

/// Load Configuration Directory
pub const IMAGE_DIRECTORY_ENTRY_LOAD_CONFIG: usize = 10;

/// Bound Import Directory
pub const IMAGE_DIRECTORY_ENTRY_BOUND_IMPORT: usize = 11;

/// Import Address Table
pub const IMAGE_DIRECTORY_ENTRY_IAT: usize = 12;

/// Delay Import Descriptor
pub const IMAGE_DIRECTORY_ENTRY_DELAY_IMPORT: usize = 13;

/// CLR Runtime Header
pub const IMAGE_DIRECTORY_ENTRY_COM_DESCRIPTOR: usize = 14;

/// Reserved, must be zero
pub const IMAGE_DIRECTORY_ENTRY_RESERVED: usize = 15;

/// Number of standard data directories
pub const IMAGE_NUMBEROF_DIRECTORY_ENTRIES: usize = 16;

// =============================================================================
// Base Relocation Types
// =============================================================================

/// Padding (skip)
pub const IMAGE_REL_BASED_ABSOLUTE: u16 = 0;

/// High 16 bits of 32-bit field
pub const IMAGE_REL_BASED_HIGH: u16 = 1;

/// Low 16 bits of 32-bit field
pub const IMAGE_REL_BASED_LOW: u16 = 2;

/// Full 32-bit field
pub const IMAGE_REL_BASED_HIGHLOW: u16 = 3;

/// High 16 bits with adjustment
pub const IMAGE_REL_BASED_HIGHADJ: u16 = 4;

/// MIPS jump address
pub const IMAGE_REL_BASED_MIPS_JMPADDR: u16 = 5;

/// ARM MOV32
pub const IMAGE_REL_BASED_ARM_MOV32: u16 = 5;

/// RISC-V high 20 bits
pub const IMAGE_REL_BASED_RISCV_HIGH20: u16 = 5;

/// Thumb MOV32
pub const IMAGE_REL_BASED_THUMB_MOV32: u16 = 7;

/// RISC-V low 12 bits (I-type)
pub const IMAGE_REL_BASED_RISCV_LOW12I: u16 = 7;

/// RISC-V low 12 bits (S-type)
pub const IMAGE_REL_BASED_RISCV_LOW12S: u16 = 8;

/// MIPS16 jump address
pub const IMAGE_REL_BASED_MIPS_JMPADDR16: u16 = 9;

/// Full 64-bit field (PE32+ only)
pub const IMAGE_REL_BASED_DIR64: u16 = 10;

// =============================================================================
// Import Flags
// =============================================================================

/// Import by ordinal flag (PE32)
pub const IMAGE_ORDINAL_FLAG32: u32 = 0x80000000;

/// Import by ordinal flag (PE32+)
pub const IMAGE_ORDINAL_FLAG64: u64 = 0x8000000000000000;

// =============================================================================
// DLL Characteristics
// =============================================================================

/// Image can be loaded at high addresses (>2GB)
pub const IMAGE_DLLCHARACTERISTICS_HIGH_ENTROPY_VA: u16 = 0x0020;

/// DLL can be relocated at load time
pub const IMAGE_DLLCHARACTERISTICS_DYNAMIC_BASE: u16 = 0x0040;

/// Code integrity checks are enforced
pub const IMAGE_DLLCHARACTERISTICS_FORCE_INTEGRITY: u16 = 0x0080;

/// Image is NX compatible
pub const IMAGE_DLLCHARACTERISTICS_NX_COMPAT: u16 = 0x0100;

/// Isolation aware, but do not isolate
pub const IMAGE_DLLCHARACTERISTICS_NO_ISOLATION: u16 = 0x0200;

/// No structured exception handling
pub const IMAGE_DLLCHARACTERISTICS_NO_SEH: u16 = 0x0400;

/// Do not bind image
pub const IMAGE_DLLCHARACTERISTICS_NO_BIND: u16 = 0x0800;

/// Image must execute in an AppContainer
pub const IMAGE_DLLCHARACTERISTICS_APPCONTAINER: u16 = 0x1000;

/// WDM driver
pub const IMAGE_DLLCHARACTERISTICS_WDM_DRIVER: u16 = 0x2000;

/// Image supports Control Flow Guard
pub const IMAGE_DLLCHARACTERISTICS_GUARD_CF: u16 = 0x4000;

/// Terminal Server aware
pub const IMAGE_DLLCHARACTERISTICS_TERMINAL_SERVER_AWARE: u16 = 0x8000;

// =============================================================================
// Subsystem Types
// =============================================================================

/// Unknown subsystem
pub const IMAGE_SUBSYSTEM_UNKNOWN: u16 = 0;

/// Device drivers and native NT processes
pub const IMAGE_SUBSYSTEM_NATIVE: u16 = 1;

/// Windows GUI subsystem
pub const IMAGE_SUBSYSTEM_WINDOWS_GUI: u16 = 2;

/// Windows console subsystem
pub const IMAGE_SUBSYSTEM_WINDOWS_CUI: u16 = 3;

/// OS/2 console subsystem
pub const IMAGE_SUBSYSTEM_OS2_CUI: u16 = 5;

/// POSIX console subsystem
pub const IMAGE_SUBSYSTEM_POSIX_CUI: u16 = 7;

/// Native Windows 9x driver
pub const IMAGE_SUBSYSTEM_NATIVE_WINDOWS: u16 = 8;

/// Windows CE
pub const IMAGE_SUBSYSTEM_WINDOWS_CE_GUI: u16 = 9;

/// EFI application
pub const IMAGE_SUBSYSTEM_EFI_APPLICATION: u16 = 10;

/// EFI boot service driver
pub const IMAGE_SUBSYSTEM_EFI_BOOT_SERVICE_DRIVER: u16 = 11;

/// EFI runtime driver
pub const IMAGE_SUBSYSTEM_EFI_RUNTIME_DRIVER: u16 = 12;

/// EFI ROM image
pub const IMAGE_SUBSYSTEM_EFI_ROM: u16 = 13;

/// Xbox
pub const IMAGE_SUBSYSTEM_XBOX: u16 = 14;

/// Windows Boot Application
pub const IMAGE_SUBSYSTEM_WINDOWS_BOOT_APPLICATION: u16 = 16;

// =============================================================================
// Sizes
// =============================================================================

/// Size of DOS header
pub const DOS_HEADER_SIZE: usize = 64;

/// Size of COFF file header
pub const FILE_HEADER_SIZE: usize = 20;

/// Size of section header
pub const SECTION_HEADER_SIZE: usize = 40;

/// Maximum section name length
pub const SECTION_NAME_SIZE: usize = 8;

