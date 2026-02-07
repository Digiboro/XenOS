//! NT Types для FAT32 Driver
//!
//! Минимальный набор типов и констант из ntoskrnl.exe

#![allow(non_camel_case_types)]
#![allow(dead_code)]

use core::ptr;

// =============================================================================
// Basic Types
// =============================================================================

pub type NTSTATUS = i32;
pub type PVOID = *mut core::ffi::c_void;
pub type ULONG = u32;
pub type USHORT = u16;
pub type UCHAR = u8;
pub type CSHORT = i16;
pub type CCHAR = i8;
pub type BOOLEAN = u8;
pub type LONG = i32;
pub type ULONG_PTR = usize;

// =============================================================================
// Status Codes
// =============================================================================

pub const STATUS_SUCCESS: NTSTATUS = 0;
pub const STATUS_UNSUCCESSFUL: NTSTATUS = 0xC0000001u32 as i32;
pub const STATUS_NOT_SUPPORTED: NTSTATUS = 0xC00000BBu32 as i32;
pub const STATUS_INSUFFICIENT_RESOURCES: NTSTATUS = 0xC000009Au32 as i32;
pub const STATUS_INVALID_PARAMETER: NTSTATUS = 0xC000000Du32 as i32;
pub const STATUS_UNRECOGNIZED_VOLUME: NTSTATUS = 0xC000_0014u32 as i32;
pub const STATUS_WRONG_VOLUME: NTSTATUS = 0xC0000012u32 as i32;
pub const STATUS_INVALID_DEVICE_REQUEST: NTSTATUS = 0xC0000010u32 as i32;
pub const STATUS_PENDING: NTSTATUS = 0x00000103u32 as i32;

// =============================================================================
// UNICODE_STRING
// =============================================================================

#[repr(C)]
#[derive(Clone, Copy)]
pub struct UNICODE_STRING {
    pub length: USHORT,
    pub maximum_length: USHORT,
    pub buffer: *mut u16,
}

impl UNICODE_STRING {
    pub const fn new() -> Self {
        Self {
            length: 0,
            maximum_length: 0,
            buffer: ptr::null_mut(),
        }
    }
}

// =============================================================================
// LARGE_INTEGER
// =============================================================================

#[repr(C)]
#[derive(Clone, Copy)]
pub union LARGE_INTEGER {
    pub quad_part: i64,
    pub parts: LARGE_INTEGER_PARTS,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct LARGE_INTEGER_PARTS {
    pub low_part: u32,
    pub high_part: i32,
}

impl LARGE_INTEGER {
    pub const fn new(value: i64) -> Self {
        Self { quad_part: value }
    }
}

// =============================================================================
// LIST_ENTRY
// =============================================================================

#[repr(C)]
#[derive(Copy, Clone)]
pub struct LIST_ENTRY {
    pub flink: *mut LIST_ENTRY,
    pub blink: *mut LIST_ENTRY,
}

// =============================================================================
// Device Types
// =============================================================================

pub const FILE_DEVICE_DISK_FILE_SYSTEM: ULONG = 0x00000008;

// IRP Major Functions
pub const IRP_MJ_CREATE: u8 = 0x00;
pub const IRP_MJ_CLOSE: u8 = 0x02;
pub const IRP_MJ_READ: u8 = 0x03;
pub const IRP_MJ_WRITE: u8 = 0x04;
pub const IRP_MJ_QUERY_INFORMATION: u8 = 0x05;
pub const IRP_MJ_SET_INFORMATION: u8 = 0x06;
pub const IRP_MJ_FLUSH_BUFFERS: u8 = 0x09;
pub const IRP_MJ_QUERY_VOLUME_INFORMATION: u8 = 0x0A;
pub const IRP_MJ_DIRECTORY_CONTROL: u8 = 0x0C;
pub const IRP_MJ_FILE_SYSTEM_CONTROL: u8 = 0x0D;
pub const IRP_MJ_CLEANUP: u8 = 0x12;

// IRP Minor Functions для FILE_SYSTEM_CONTROL
pub const IRP_MN_MOUNT_VOLUME: u8 = 0x01;
pub const IRP_MN_VERIFY_VOLUME: u8 = 0x02;

// =============================================================================
// Forward Declarations
// =============================================================================

pub type PDRIVER_OBJECT = *mut core::ffi::c_void;
pub type PDEVICE_OBJECT = *mut core::ffi::c_void;
pub type PIRP = *mut core::ffi::c_void;
pub type PIO_STACK_LOCATION = *mut core::ffi::c_void;
pub type PVPB = *mut core::ffi::c_void;

// =============================================================================
// FILE_OBJECT - полная структура согласно MSDN
// =============================================================================

/// FILE_OBJECT - полная структура согласно WinDDK wdm.h:21212
#[repr(C)]
pub struct FILE_OBJECT {
    pub r#type: CSHORT,
    pub size: CSHORT,
    pub device_object: PDEVICE_OBJECT,
    pub vpb: PVPB,
    pub fs_context: PVOID,
    pub fs_context2: PVOID,
    pub section_object_pointer: PVOID,
    pub private_cache_map: PVOID,
    pub final_status: NTSTATUS,
    pub related_file_object: *mut FILE_OBJECT,
    pub lock_operation: BOOLEAN,
    pub delete_pending: BOOLEAN,
    pub read_access: BOOLEAN,
    pub write_access: BOOLEAN,
    pub delete_access: BOOLEAN,
    pub shared_read: BOOLEAN,
    pub shared_write: BOOLEAN,
    pub shared_delete: BOOLEAN,
    pub flags: ULONG,
    pub file_name: UNICODE_STRING,
    pub current_byte_offset: LARGE_INTEGER,
    pub waiters: ULONG,
    pub busy: ULONG,
    pub last_lock: PVOID,
    pub lock: KEVENT,
    pub event: KEVENT,
    pub completion_context: PVOID, // PIO_COMPLETION_CONTEXT
    pub irp_list_lock: ULONG_PTR,  // KSPIN_LOCK
    pub irp_list: LIST_ENTRY,
    pub file_object_extension: PVOID,
}

// =============================================================================
// DEVICE_OBJECT - полная структура согласно WinDDK wdm.h:20978
// =============================================================================

/// KDEVICE_QUEUE - queue для device
#[repr(C)]
#[derive(Copy, Clone)]
pub struct KDEVICE_QUEUE {
    pub r#type: CSHORT,
    pub size: CSHORT,
    pub device_list_head: LIST_ENTRY,
    pub lock: ULONG_PTR,
    pub busy: BOOLEAN,
}

/// KDPC - Deferred Procedure Call
#[repr(C)]
#[derive(Copy, Clone)]
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

/// KDEVICE_QUEUE_ENTRY
#[repr(C)]
#[derive(Copy, Clone)]
pub struct KDEVICE_QUEUE_ENTRY {
    pub device_list_entry: LIST_ENTRY,
    pub sort_key: ULONG,
    pub inserted: BOOLEAN,
}

/// WAIT_CONTEXT_BLOCK - согласно MSDN (opaque structure)
/// https://learn.microsoft.com/ru-ru/windows-hardware/drivers/ddi/wdm/ns-wdm-_wait_context_block
#[repr(C)]
#[derive(Copy, Clone)]
pub struct WAIT_CONTEXT_BLOCK {
    pub wait_queue_entry_or_dma: WaitContextBlockUnion,
    pub device_routine: PVOID, // PDRIVER_CONTROL
    pub device_context: PVOID,
    pub number_of_map_registers: ULONG,
    pub device_object: PVOID,
    pub current_irp: PVOID,
    pub buffer_chaining_dpc: PVOID, // PKDPC
}

#[repr(C)]
#[derive(Copy, Clone)]
pub union WaitContextBlockUnion {
    pub wait_queue_entry: KDEVICE_QUEUE_ENTRY,
    pub dma_wait_fields: DmaWaitFields,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct DmaWaitFields {
    pub dma_wait_entry: LIST_ENTRY,
    pub number_of_channels: ULONG,
    pub flags_and_pages: ULONG, // SyncCallback:1, DmaContext:1, ZeroMapRegisters:1, Reserved:9, NumberOfRemapPages:20
}

/// DEVICE_OBJECT - полная структура согласно MSDN/WDK
/// https://learn.microsoft.com/ru-ru/windows-hardware/drivers/ddi/wdm/ns-wdm-_device_object
#[repr(C)]
pub struct DEVICE_OBJECT {
    pub r#type: CSHORT,
    pub size: USHORT,
    pub reference_count: LONG,
    pub driver_object: PDRIVER_OBJECT,
    pub next_device: PDEVICE_OBJECT,
    pub attached_device: PDEVICE_OBJECT,
    pub current_irp: PIRP,
    pub timer: PVOID, // PIO_TIMER
    pub flags: ULONG,
    pub characteristics: ULONG,
    pub vpb: PVPB,
    pub device_extension: PVOID,
    pub device_type: ULONG,
    pub stack_size: CCHAR,
    pub queue: DeviceObjectQueue,
    pub alignment_requirement: ULONG,
    pub device_queue: KDEVICE_QUEUE,
    pub dpc: KDPC,
    pub active_thread_count: ULONG,
    pub security_descriptor: PVOID, // PSECURITY_DESCRIPTOR
    pub device_lock: KEVENT,
    pub sector_size: USHORT,
    pub spare1: USHORT,
    pub device_object_extension: PVOID, // PDEVOBJ_EXTENSION
    pub reserved: PVOID,
}

/// Union для Queue field в DEVICE_OBJECT
#[repr(C)]
#[derive(Copy, Clone)]
pub union DeviceObjectQueue {
    pub list_entry: LIST_ENTRY,
    pub wcb: WAIT_CONTEXT_BLOCK,
}

// =============================================================================
// KEVENT - согласно WinDDK wdm.h:11690
// =============================================================================

/// DISPATCHER_HEADER - заголовок dispatcher object
#[repr(C)]
#[derive(Copy, Clone)]
pub struct DISPATCHER_HEADER {
    pub r#type: UCHAR,
    pub absolute: UCHAR,
    pub size: UCHAR,
    pub inserted: UCHAR,
    pub signal_state: LONG,
    pub wait_list_head: LIST_ENTRY,
}

#[repr(C)]
pub struct KEVENT {
    pub header: DISPATCHER_HEADER,
}

// =============================================================================
// Imports from ntoskrnl.exe
// =============================================================================

#[link(name = "ntoskrnl")]
unsafe extern "win64" {
    pub fn IoCreateDevice(
        driver_object: PDRIVER_OBJECT,
        device_extension_size: ULONG,
        device_name: *const UNICODE_STRING,
        device_type: ULONG,
        device_characteristics: ULONG,
        exclusive: BOOLEAN,
        device_object: *mut PDEVICE_OBJECT,
    ) -> NTSTATUS;
    
    pub fn IoDeleteDevice(device_object: PDEVICE_OBJECT);
    
    pub fn IoRegisterFileSystem(device_object: PDEVICE_OBJECT);
    pub fn IoUnregisterFileSystem(device_object: PDEVICE_OBJECT);
    
    pub fn IoCompleteRequest(irp: PIRP, priority_boost: CCHAR);
    pub fn IoGetCurrentIrpStackLocation(irp: PIRP) -> PIO_STACK_LOCATION;
    pub fn IoCallDriver(device_object: PDEVICE_OBJECT, irp: PIRP) -> NTSTATUS;
    
    pub fn IoBuildSynchronousFsdRequest(
        major_function: ULONG,
        device_object: PDEVICE_OBJECT,
        buffer: PVOID,
        length: ULONG,
        starting_offset: i64,
        event: PVOID,
        io_status_block: PVOID,
    ) -> PIRP;
    
    pub fn ExAllocatePoolWithTag(pool_type: ULONG, size: ULONG_PTR, tag: ULONG) -> PVOID;
    pub fn ExFreePoolWithTag(ptr: PVOID, tag: ULONG);
    
    pub fn KeWaitForSingleObject(
        object: PVOID,
        wait_reason: ULONG,
        wait_mode: ULONG,
        alertable: BOOLEAN,
        timeout: PVOID,
    ) -> NTSTATUS;
    
    pub fn KeInitializeEvent(
        event: *mut KEVENT,
        event_type: ULONG,
        initial_state: BOOLEAN,
    );
    
    pub fn DbgPrint(format: *const u8) -> ULONG;
}

// Event types
pub const NOTIFICATION_EVENT: ULONG = 0;
pub const SYNCHRONIZATION_EVENT: ULONG = 1;

// =============================================================================
// Pool Types
// =============================================================================

pub const NON_PAGED_POOL: ULONG = 0;
pub const PAGED_POOL: ULONG = 1;

// =============================================================================
// Pool Tag
// =============================================================================

/// Pool tag для FAT32 allocations: 'Fat3'
pub const FAT32_POOL_TAG: ULONG = 0x33746146; // 'Fat3' в little-endian

// =============================================================================
// Priority Boost
// =============================================================================

pub const IO_NO_INCREMENT: CCHAR = 0;
pub const IO_DISK_INCREMENT: CCHAR = 1;

// =============================================================================
// IO_STATUS_BLOCK
// =============================================================================

/// IO_STATUS_BLOCK - согласно WinDDK wdm.h:4193
#[repr(C)]
pub struct IO_STATUS_BLOCK {
    pub status_or_pointer: IoStatusBlockUnion,
    pub information: ULONG_PTR,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub union IoStatusBlockUnion {
    pub status: NTSTATUS,
    pub pointer: PVOID,
}

impl IO_STATUS_BLOCK {
    pub fn new(status: NTSTATUS) -> Self {
        Self {
            status_or_pointer: IoStatusBlockUnion { status },
            information: 0,
        }
    }
}

// =============================================================================
// Status Codes (дополнительные)
// =============================================================================

pub const STATUS_OBJECT_NAME_INVALID: NTSTATUS = 0xC0000033u32 as i32;
pub const STATUS_OBJECT_NAME_NOT_FOUND: NTSTATUS = 0xC0000034u32 as i32;
pub const STATUS_OBJECT_NAME_COLLISION: NTSTATUS = 0xC0000035u32 as i32;
pub const STATUS_OBJECT_PATH_NOT_FOUND: NTSTATUS = 0xC000003Au32 as i32;
pub const STATUS_END_OF_FILE: NTSTATUS = 0xC0000011u32 as i32;
pub const STATUS_DISK_FULL: NTSTATUS = 0xC000007Fu32 as i32;
pub const STATUS_NOT_IMPLEMENTED: NTSTATUS = 0xC0000002u32 as i32;

