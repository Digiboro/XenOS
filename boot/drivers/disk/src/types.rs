//! Disk Driver Types
//!
//! NT типы и disk-специфичные структуры.

use core::ptr;

// =============================================================================
// Basic NT Types
// =============================================================================

pub type NTSTATUS = i32;
pub type ULONG = u32;
pub type USHORT = u16;
pub type UCHAR = u8;
pub type BOOLEAN = u8;
pub type CCHAR = i8;
pub type CSHORT = i16;
pub type PVOID = *mut core::ffi::c_void;
pub type ULONG_PTR = usize;
pub type LARGE_INTEGER = i64;
pub type PHYSICAL_ADDRESS = u64;

// =============================================================================
// NTSTATUS Codes
// =============================================================================

pub const STATUS_SUCCESS: NTSTATUS = 0;
pub const STATUS_UNSUCCESSFUL: NTSTATUS = 0xC0000001u32 as i32;
pub const STATUS_NOT_SUPPORTED: NTSTATUS = 0xC00000BBu32 as i32;
pub const STATUS_INSUFFICIENT_RESOURCES: NTSTATUS = 0xC000009Au32 as i32;
pub const STATUS_NO_SUCH_DEVICE: NTSTATUS = 0xC000000Eu32 as i32;
pub const STATUS_INVALID_DEVICE_REQUEST: NTSTATUS = 0xC0000010u32 as i32;
pub const STATUS_INVALID_PARAMETER: NTSTATUS = 0xC000000Du32 as i32;
pub const STATUS_DEVICE_NOT_CONNECTED: NTSTATUS = 0xC000009Du32 as i32;
pub const STATUS_DEVICE_NOT_READY: NTSTATUS = 0xC00000A3u32 as i32;
pub const STATUS_END_OF_FILE: NTSTATUS = 0xC0000011u32 as i32;
pub const STATUS_DEVICE_CONFIGURATION_ERROR: NTSTATUS = 0xC0000182u32 as i32;

// =============================================================================
// Device Types
// =============================================================================

pub const FILE_DEVICE_DISK: ULONG = 0x00000007;
pub const FILE_DEVICE_DISK_FILE_SYSTEM: ULONG = 0x00000008;

// =============================================================================
// IOCTL Codes (ntdddisk.h)
// =============================================================================

/// IOCTL для получения geometry диска
pub const IOCTL_DISK_GET_DRIVE_GEOMETRY: ULONG = 0x00070000; // CTL_CODE(IOCTL_DISK_BASE, 0, METHOD_BUFFERED, FILE_ANY_ACCESS)
/// IOCTL для получения extended geometry
pub const IOCTL_DISK_GET_DRIVE_GEOMETRY_EX: ULONG = 0x000700A0; // CTL_CODE(IOCTL_DISK_BASE, 0x28, METHOD_BUFFERED, FILE_ANY_ACCESS)
/// IOCTL для получения информации о партиции
pub const IOCTL_DISK_GET_PARTITION_INFO: ULONG = 0x00074004; // CTL_CODE(IOCTL_DISK_BASE, 1, METHOD_BUFFERED, FILE_READ_ACCESS)
/// IOCTL для получения extended информации о партиции
pub const IOCTL_DISK_GET_PARTITION_INFO_EX: ULONG = 0x00070048; // CTL_CODE(IOCTL_DISK_BASE, 0x12, METHOD_BUFFERED, FILE_ANY_ACCESS)
/// IOCTL для получения длины устройства
pub const IOCTL_DISK_GET_LENGTH_INFO: ULONG = 0x0007405C; // CTL_CODE(IOCTL_DISK_BASE, 0x17, METHOD_BUFFERED, FILE_READ_ACCESS)
/// IOCTL для получения drive layout
pub const IOCTL_DISK_GET_DRIVE_LAYOUT: ULONG = 0x0007400C; // CTL_CODE(IOCTL_DISK_BASE, 3, METHOD_BUFFERED, FILE_READ_ACCESS)
/// IOCTL для получения extended drive layout
pub const IOCTL_DISK_GET_DRIVE_LAYOUT_EX: ULONG = 0x00070050; // CTL_CODE(IOCTL_DISK_BASE, 0x14, METHOD_BUFFERED, FILE_ANY_ACCESS)
/// IOCTL для проверки writable
pub const IOCTL_DISK_IS_WRITABLE: ULONG = 0x00070024; // CTL_CODE(IOCTL_DISK_BASE, 9, METHOD_BUFFERED, FILE_ANY_ACCESS)
/// IOCTL для запрета извлечения носителя
pub const IOCTL_DISK_MEDIA_REMOVAL: ULONG = 0x00070C00; // CTL_CODE(IOCTL_DISK_BASE, 0x100, METHOD_BUFFERED, FILE_READ_ACCESS)
/// IOCTL для извлечения носителя
pub const IOCTL_DISK_EJECT_MEDIA: ULONG = 0x00074808; // CTL_CODE(IOCTL_DISK_BASE, 0x202, METHOD_BUFFERED, FILE_READ_ACCESS)

/// Media type for fixed disks
pub const MEDIA_TYPE_FIXED_MEDIA: u32 = 12;

/// Disk geometry structure
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DISK_GEOMETRY {
    pub cylinders: i64,
    pub media_type: u32,
    pub tracks_per_cylinder: u32,
    pub sectors_per_track: u32,
    pub bytes_per_sector: u32,
}

/// Extended disk geometry structure
#[repr(C)]
pub struct DISK_GEOMETRY_EX {
    pub geometry: DISK_GEOMETRY,
    pub disk_size: i64,
    pub data: [u8; 1], // Variable-length data follows
}

/// Partition information structure
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PARTITION_INFORMATION {
    pub starting_offset: i64,
    pub partition_length: i64,
    pub hidden_sectors: u32,
    pub partition_number: u32,
    pub partition_type: u8,
    pub bootable: u8,
    pub recognized_partition: u8,
    pub rewrite_partition: u8,
}

/// Drive layout information structure
#[repr(C)]
pub struct DRIVE_LAYOUT_INFORMATION {
    pub partition_count: u32,
    pub signature: u32,
    pub partition_entry: [PARTITION_INFORMATION; 4], // Up to 4 for MBR
}

/// Length information structure
#[repr(C)]
pub struct GET_LENGTH_INFORMATION {
    pub length: i64,
}

// =============================================================================
// Device Flags
// =============================================================================

pub const DO_BUFFERED_IO: ULONG = 0x00000004;
pub const DO_DIRECT_IO: ULONG = 0x00000010;
pub const DO_DEVICE_INITIALIZING: ULONG = 0x00000080;
pub const DO_POWER_PAGABLE: ULONG = 0x00002000;

// =============================================================================
// IRP Major Function Codes
// =============================================================================

pub const IRP_MJ_CREATE: UCHAR = 0;
pub const IRP_MJ_CLOSE: UCHAR = 2;
pub const IRP_MJ_CLEANUP: UCHAR = 18;
pub const IRP_MJ_READ: UCHAR = 3;
pub const IRP_MJ_WRITE: UCHAR = 4;
pub const IRP_MJ_DEVICE_CONTROL: UCHAR = 14;
pub const IRP_MJ_INTERNAL_DEVICE_CONTROL: UCHAR = 15;
pub const IRP_MJ_POWER: UCHAR = 0x16;
pub const IRP_MJ_PNP: UCHAR = 0x1B;
pub const IRP_MJ_SCSI: UCHAR = IRP_MJ_INTERNAL_DEVICE_CONTROL;

// =============================================================================
// IRP Minor Function Codes - PnP
// =============================================================================

pub const IRP_MN_START_DEVICE: UCHAR = 0x00;
pub const IRP_MN_QUERY_REMOVE_DEVICE: UCHAR = 0x01;
pub const IRP_MN_REMOVE_DEVICE: UCHAR = 0x02;
pub const IRP_MN_STOP_DEVICE: UCHAR = 0x04;
pub const IRP_MN_QUERY_DEVICE_RELATIONS: UCHAR = 0x07;
pub const IRP_MN_QUERY_CAPABILITIES: UCHAR = 0x09;
pub const IRP_MN_QUERY_DEVICE_TEXT: UCHAR = 0x0C;
pub const IRP_MN_QUERY_ID: UCHAR = 0x13;

// =============================================================================
// Device Relations Types
// =============================================================================

pub const BUS_RELATIONS: ULONG = 0;
pub const TARGET_DEVICE_RELATION: ULONG = 4;

// =============================================================================
// Bus Query ID Types
// =============================================================================

pub const BUS_QUERY_DEVICE_ID: ULONG = 0;
pub const BUS_QUERY_HARDWARE_IDS: ULONG = 1;
pub const BUS_QUERY_INSTANCE_ID: ULONG = 3;

// =============================================================================
// IO Constants
// =============================================================================

pub const IO_NO_INCREMENT: CCHAR = 0;

// =============================================================================
// Pool Types
// =============================================================================

pub const NON_PAGED_POOL: ULONG = 0;

// =============================================================================
// Unicode String
// =============================================================================

#[repr(C)]
#[derive(Clone, Copy)]
pub struct UNICODE_STRING {
    pub length: USHORT,
    pub maximum_length: USHORT,
    pub buffer: *mut u16,
}

// =============================================================================
// LIST_ENTRY
// =============================================================================

#[repr(C)]
#[derive(Clone, Copy)]
pub struct LIST_ENTRY {
    pub flink: *mut LIST_ENTRY,
    pub blink: *mut LIST_ENTRY,
}

// =============================================================================
// KEVENT - Kernel Event Object
// =============================================================================

/// DISPATCHER_HEADER (simplified)
#[repr(C)]
pub struct DISPATCHER_HEADER {
    pub r#type: u8,
    pub absolute: u8,
    pub size: u8,
    pub inserted: u8,
    pub signal_state: i32,
    pub wait_list_head: LIST_ENTRY,
}

/// KEVENT structure
#[repr(C)]
pub struct KEVENT {
    pub header: DISPATCHER_HEADER,
}

// =============================================================================
// IRP Structures
// =============================================================================

#[repr(C)]
/// AssociatedIrp union в IRP
#[repr(C)]
pub union IRP_ASSOCIATED_IRP {
    pub master_irp: PIRP,
    pub irp_count: i32,
    pub system_buffer: PVOID,
}

#[repr(C)]
pub struct IRP {
    pub r#type: CSHORT,
    pub size: USHORT,
    pub mdl_address: PVOID,
    pub flags: ULONG,
    pub associated_irp: IRP_ASSOCIATED_IRP,
    pub thread_list_entry: LIST_ENTRY,
    pub io_status: IO_STATUS_BLOCK,
    pub requestor_mode: CCHAR,
    pub pending_returned: BOOLEAN,
    pub stack_count: CCHAR,
    pub current_location: CCHAR,
    pub cancel: BOOLEAN,
    pub cancel_irql: UCHAR,
    pub apc_environment: CCHAR,
    pub allocation_flags: UCHAR,
    pub user_iosb: *mut IO_STATUS_BLOCK,
    pub user_event: PVOID,
    pub overlay: [u8; 16],
    pub cancel_routine: PVOID,
    pub user_buffer: PVOID,
    pub tail: [u8; 64],
}

pub type PIRP = *mut IRP;

// =============================================================================
// MDL - Memory Descriptor List
// =============================================================================

#[repr(C)]
pub struct MDL {
    pub next: *mut MDL,
    pub size: i16,
    pub mdl_flags: i16,
    pub process: PVOID,
    pub mapped_system_va: PVOID,
    pub start_va: PVOID,
    pub byte_count: u32,
    pub byte_offset: u32,
    // После этого следует массив PFN_NUMBER
}

pub type PMDL = *mut MDL;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct IO_STATUS_BLOCK {
    pub status: NTSTATUS,
    pub information: ULONG_PTR,
}

// =============================================================================
// IO_STACK_LOCATION
// =============================================================================

#[repr(C)]
pub struct IO_STACK_LOCATION {
    pub major_function: UCHAR,
    pub minor_function: UCHAR,
    pub flags: UCHAR,
    pub control: UCHAR,
    pub _padding: [u8; 4],        // Padding для выравнивания parameters на 8 байт
    pub parameters: IoParameters, // offset 8
    pub device_object: PDEVICE_OBJECT,
    pub file_object: PVOID,
    pub completion_routine: PVOID,
    pub context: PVOID,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub union IoParameters {
    pub read: ReadWriteParams,
    pub write: ReadWriteParams,
    pub device_io_control: DeviceIoControlParams,
    pub scsi: ScsiParams,
    pub start_device: StartDeviceParams,
    pub query_device_relations: QueryDeviceRelationsParams,
    pub query_id: QueryIdParams,
    pub raw: [u64; 4],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ReadWriteParams {
    pub length: ULONG,
    pub key: ULONG,
    pub byte_offset: LARGE_INTEGER,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct DeviceIoControlParams {
    pub output_buffer_length: ULONG,
    pub input_buffer_length: ULONG,
    pub io_control_code: ULONG,
    pub type3_input_buffer: PVOID,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ScsiParams {
    pub srb: PVOID, // PSCSI_REQUEST_BLOCK
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct StartDeviceParams {
    pub allocated_resources: PVOID,
    pub allocated_resources_translated: PVOID,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct QueryDeviceRelationsParams {
    pub r#type: ULONG,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct QueryIdParams {
    pub id_type: ULONG,
}

pub type PIO_STACK_LOCATION = *mut IO_STACK_LOCATION;

// =============================================================================
// DRIVER_OBJECT & DEVICE_OBJECT
// =============================================================================

pub type PDRIVER_DISPATCH = Option<unsafe extern "win64" fn(PDEVICE_OBJECT, PIRP) -> NTSTATUS>;
pub type PDRIVER_ADD_DEVICE = Option<unsafe extern "win64" fn(PDRIVER_OBJECT, PDEVICE_OBJECT) -> NTSTATUS>;

#[repr(C)]
pub struct DRIVER_EXTENSION {
    pub driver_object: PDRIVER_OBJECT,
    pub add_device: PDRIVER_ADD_DEVICE,
    pub count: ULONG,
    pub service_key_name: UNICODE_STRING,
}

#[repr(C)]
pub struct DRIVER_OBJECT {
    pub r#type: i16,
    pub size: i16,
    pub device_object: PVOID,
    pub flags: ULONG,
    pub driver_start: PVOID,
    pub driver_size: ULONG,
    pub driver_section: PVOID,
    pub driver_extension: *mut DRIVER_EXTENSION,
    pub driver_name: UNICODE_STRING,
    pub hardware_database: PVOID,
    pub fast_io_dispatch: PVOID,
    pub driver_init: PVOID,
    pub driver_start_io: PVOID,
    pub driver_unload: Option<unsafe extern "win64" fn(PDRIVER_OBJECT)>,
    pub major_function: [PDRIVER_DISPATCH; 28],
}

// NOTE: DEVICE_OBJECT должен соответствовать ntoskrnl определению
// WinDDK 7600.16385.1/inc/ddk/wdm.h:20983-21016
#[repr(C)]
pub struct DEVICE_OBJECT {
    pub r#type: CSHORT,
    pub size: USHORT,
    pub reference_count: i32,
    pub driver_object: PDRIVER_OBJECT,
    pub next_device: PDEVICE_OBJECT,
    pub attached_device: PDEVICE_OBJECT,
    pub current_irp: PIRP,
    pub timer: PVOID,
    pub flags: ULONG,
    pub characteristics: ULONG,
    pub vpb: PVOID,
    pub device_extension: PVOID,
    pub device_type: ULONG,
    pub stack_size: i8, // CCHAR
    pub queue: LIST_ENTRY,
    pub alignment_requirement: ULONG,
    pub device_queue: KDEVICE_QUEUE,
    pub dpc: KDPC,
    pub active_thread_count: ULONG,
    pub security_descriptor: PVOID,
    pub device_lock: KEVENT,
    pub sector_size: USHORT,
    pub spare1: USHORT,
    pub device_object_extension: PVOID,
    pub reserved: PVOID,
}

// Вспомогательные структуры

#[repr(C)]
pub struct KDEVICE_QUEUE {
    pub r#type: CSHORT,
    pub size: CSHORT,
    pub device_list_head: LIST_ENTRY,
    pub lock: KSPIN_LOCK,
    pub busy: BOOLEAN,
}

#[repr(C)]
pub struct KSPIN_LOCK {
    pub lock: ULONG_PTR,
}

#[repr(C)]
pub struct KDPC {
    pub r#type: UCHAR,
    pub importance: UCHAR,
    pub number: USHORT,
    pub dpc_list_entry: LIST_ENTRY,
    pub deferred_routine: PVOID,
    pub deferred_context: PVOID,
    pub system_argument1: PVOID,
    pub system_argument2: PVOID,
    pub dpc_data: PVOID,
}

pub type PDRIVER_OBJECT = *mut DRIVER_OBJECT;
pub type PDEVICE_OBJECT = *mut DEVICE_OBJECT;

// =============================================================================
// Disk Device Extensions
// =============================================================================

/// Common extension header
#[repr(C)]
pub struct DISK_COMMON_EXTENSION {
    /// 1 if FDO, 0 if PDO
    pub is_fdo: u8,
    pub _reserved: [u8; 7],
    /// Back pointer to self
    pub self_device: PDEVICE_OBJECT,
}

/// FDO Extension for Disk Device
#[repr(C)]
pub struct DISK_FDO_EXTENSION {
    pub common: DISK_COMMON_EXTENSION,
    /// Physical Device Object (SCSI PDO from AHCI)
    pub physical_device_object: PDEVICE_OBJECT,
    /// Lower device in stack (AHCI PDO)
    pub lower_device: PDEVICE_OBJECT,
    /// Harddisk number (для \Device\Harddisk<N>)
    pub harddisk_number: u32,
    /// Disk geometry
    pub sector_count: u64,
    pub sector_size: u32,
    /// Device started
    pub started: u8,
    /// Partition PDOs (до 16 партиций)
    pub partition_pdos: [PDEVICE_OBJECT; 16],
    /// Количество партиций
    pub partition_count: u32,
}

/// PDO Extension for Partition
#[repr(C)]
pub struct DISK_PARTITION_EXTENSION {
    pub common: DISK_COMMON_EXTENSION,
    /// Parent FDO (disk)
    pub parent_fdo: PDEVICE_OBJECT,
    /// Номер партиции (0 = whole disk, 1+ = партиции)
    pub partition_number: u32,
    /// Начальный LBA
    pub starting_lba: u64,
    /// Размер в секторах
    pub sector_count: u64,
    /// Тип партиции (MBR type)
    pub partition_type: u8,
    /// Bootable flag
    pub bootable: u8,
}

// =============================================================================
// Imports (PE/IAT) — единое место: src/imports/*
// =============================================================================

pub use crate::imports::ntoskrnl::*;

// =============================================================================
// Pool Tag
// =============================================================================

/// Pool tag для disk allocations: 'Disk'
pub const DISK_POOL_TAG: ULONG = 0x6B736944; // 'ksiD' in little-endian

// =============================================================================
// Global static (required by MSVC ABI)
// =============================================================================

#[unsafe(no_mangle)]
pub static _fltused: i32 = 0;

