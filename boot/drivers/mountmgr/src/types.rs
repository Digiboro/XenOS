//! Types for MountMgr driver (from mountmgr.h)

pub type NTSTATUS = i32;
pub type ULONG = u32;
pub type USHORT = u16;
pub type UCHAR = u8;
pub type BOOLEAN = u8;
pub type CCHAR = i8;
pub type CSHORT = i16;
pub type PVOID = *mut core::ffi::c_void;
pub type ULONG_PTR = usize;
pub type LONG = i32;
pub type WCHAR = u16;

// =============================================================================
// Status Codes
// =============================================================================

pub const STATUS_SUCCESS: NTSTATUS = 0;
pub const STATUS_PENDING: NTSTATUS = 0x00000103;
pub const STATUS_NO_SUCH_DEVICE: NTSTATUS = 0xC000000Eu32 as i32;
pub const STATUS_INVALID_DEVICE_REQUEST: NTSTATUS = 0xC0000010u32 as i32;
pub const STATUS_NOT_SUPPORTED: NTSTATUS = 0xC00000BBu32 as i32;
pub const STATUS_INSUFFICIENT_RESOURCES: NTSTATUS = 0xC000009Au32 as i32;
pub const STATUS_INVALID_PARAMETER: NTSTATUS = 0xC000000Du32 as i32;
pub const STATUS_BUFFER_TOO_SMALL: NTSTATUS = 0xC0000023u32 as i32;
pub const STATUS_BUFFER_OVERFLOW: NTSTATUS = 0x80000005u32 as i32;
pub const STATUS_OBJECT_NAME_NOT_FOUND: NTSTATUS = 0xC0000034u32 as i32;
pub const STATUS_OBJECT_NAME_COLLISION: NTSTATUS = 0xC0000035u32 as i32;

// =============================================================================
// Device Types and Flags
// =============================================================================

pub const FILE_DEVICE_NETWORK: ULONG = 0x00000012;
pub const FILE_DEVICE_UNKNOWN: ULONG = 0x00000022;

pub const DO_DIRECT_IO: ULONG = 0x00000010;
pub const DO_BUFFERED_IO: ULONG = 0x00000004;
pub const DO_DEVICE_INITIALIZING: ULONG = 0x00000080;

// Access modes for IOCTL
pub const FILE_READ_ACCESS: ULONG = 0x0001;
pub const FILE_WRITE_ACCESS: ULONG = 0x0002;
pub const FILE_ANY_ACCESS: ULONG = 0x0000;

// Method types for IOCTL
pub const METHOD_BUFFERED: ULONG = 0;

// =============================================================================
// IRP Major/Minor Function Codes
// =============================================================================

pub const IRP_MJ_CREATE: UCHAR = 0;
pub const IRP_MJ_CLOSE: UCHAR = 2;
pub const IRP_MJ_READ: UCHAR = 3;
pub const IRP_MJ_WRITE: UCHAR = 4;
pub const IRP_MJ_DEVICE_CONTROL: UCHAR = 14;
pub const IRP_MJ_PNP: UCHAR = 0x1B;
pub const IRP_MJ_POWER: UCHAR = 0x16;
pub const IRP_MJ_CLEANUP: UCHAR = 18;

pub const IO_NO_INCREMENT: CCHAR = 0;

// =============================================================================
// MountMgr Device Names (from mountmgr.h)
// =============================================================================

// MOUNTMGR_DEVICE_NAME = L"\\Device\\MountPointManager"
// Length = 25 chars = 50 bytes
pub const MOUNTMGR_DEVICE_NAME: &[u16] = &[
    b'\\' as u16, b'D' as u16, b'e' as u16, b'v' as u16, b'i' as u16, b'c' as u16, b'e' as u16,
    b'\\' as u16, b'M' as u16, b'o' as u16, b'u' as u16, b'n' as u16, b't' as u16, b'P' as u16,
    b'o' as u16, b'i' as u16, b'n' as u16, b't' as u16, b'M' as u16, b'a' as u16, b'n' as u16,
    b'a' as u16, b'g' as u16, b'e' as u16, b'r' as u16, 0,
];

// MOUNTMGR_DOS_DEVICE_NAME = L"\\\\.\\MountPointManager"
pub const MOUNTMGR_DOS_DEVICE_NAME: &[u16] = &[
    b'\\' as u16, b'?' as u16, b'?' as u16, b'\\' as u16, b'M' as u16, b'o' as u16, b'u' as u16,
    b'n' as u16, b't' as u16, b'P' as u16, b'o' as u16, b'i' as u16, b'n' as u16, b't' as u16,
    b'M' as u16, b'a' as u16, b'n' as u16, b'a' as u16, b'g' as u16, b'e' as u16, b'r' as u16, 0,
];

// =============================================================================
// MountMgr IOCTL Codes (from mountmgr.h)
// =============================================================================

// Control types
pub const MOUNTMGRCONTROLTYPE: ULONG = 0x0000006D; // 'm'
pub const MOUNTDEVCONTROLTYPE: ULONG = 0x0000004D; // 'M'

// CTL_CODE macro equivalent
const fn ctl_code(device_type: ULONG, function: ULONG, method: ULONG, access: ULONG) -> ULONG {
    (device_type << 16) | (access << 14) | (function << 2) | method
}

// Mount Manager IOCTLs (Win2K+)
pub const IOCTL_MOUNTMGR_CREATE_POINT: ULONG = ctl_code(
    MOUNTMGRCONTROLTYPE, 0, METHOD_BUFFERED, FILE_READ_ACCESS | FILE_WRITE_ACCESS);
pub const IOCTL_MOUNTMGR_DELETE_POINTS: ULONG = ctl_code(
    MOUNTMGRCONTROLTYPE, 1, METHOD_BUFFERED, FILE_READ_ACCESS | FILE_WRITE_ACCESS);
pub const IOCTL_MOUNTMGR_QUERY_POINTS: ULONG = ctl_code(
    MOUNTMGRCONTROLTYPE, 2, METHOD_BUFFERED, FILE_ANY_ACCESS);
pub const IOCTL_MOUNTMGR_DELETE_POINTS_DBONLY: ULONG = ctl_code(
    MOUNTMGRCONTROLTYPE, 3, METHOD_BUFFERED, FILE_READ_ACCESS | FILE_WRITE_ACCESS);
pub const IOCTL_MOUNTMGR_NEXT_DRIVE_LETTER: ULONG = ctl_code(
    MOUNTMGRCONTROLTYPE, 4, METHOD_BUFFERED, FILE_READ_ACCESS | FILE_WRITE_ACCESS);
pub const IOCTL_MOUNTMGR_AUTO_DL_ASSIGNMENTS: ULONG = ctl_code(
    MOUNTMGRCONTROLTYPE, 5, METHOD_BUFFERED, FILE_READ_ACCESS | FILE_WRITE_ACCESS);
pub const IOCTL_MOUNTMGR_VOLUME_MOUNT_POINT_CREATED: ULONG = ctl_code(
    MOUNTMGRCONTROLTYPE, 6, METHOD_BUFFERED, FILE_READ_ACCESS | FILE_WRITE_ACCESS);
pub const IOCTL_MOUNTMGR_VOLUME_MOUNT_POINT_DELETED: ULONG = ctl_code(
    MOUNTMGRCONTROLTYPE, 7, METHOD_BUFFERED, FILE_READ_ACCESS | FILE_WRITE_ACCESS);
pub const IOCTL_MOUNTMGR_CHANGE_NOTIFY: ULONG = ctl_code(
    MOUNTMGRCONTROLTYPE, 8, METHOD_BUFFERED, FILE_READ_ACCESS);
pub const IOCTL_MOUNTMGR_KEEP_LINKS_WHEN_OFFLINE: ULONG = ctl_code(
    MOUNTMGRCONTROLTYPE, 9, METHOD_BUFFERED, FILE_READ_ACCESS | FILE_WRITE_ACCESS);
pub const IOCTL_MOUNTMGR_CHECK_UNPROCESSED_VOLUMES: ULONG = ctl_code(
    MOUNTMGRCONTROLTYPE, 10, METHOD_BUFFERED, FILE_READ_ACCESS);
pub const IOCTL_MOUNTMGR_VOLUME_ARRIVAL_NOTIFICATION: ULONG = ctl_code(
    MOUNTMGRCONTROLTYPE, 11, METHOD_BUFFERED, FILE_READ_ACCESS);

// Mount Manager IOCTLs (WinXP+)
pub const IOCTL_MOUNTMGR_QUERY_DOS_VOLUME_PATH: ULONG = ctl_code(
    MOUNTMGRCONTROLTYPE, 12, METHOD_BUFFERED, FILE_ANY_ACCESS);
pub const IOCTL_MOUNTMGR_QUERY_DOS_VOLUME_PATHS: ULONG = ctl_code(
    MOUNTMGRCONTROLTYPE, 13, METHOD_BUFFERED, FILE_ANY_ACCESS);

// Mount Manager IOCTLs (WS03+)
pub const IOCTL_MOUNTMGR_SCRUB_REGISTRY: ULONG = ctl_code(
    MOUNTMGRCONTROLTYPE, 14, METHOD_BUFFERED, FILE_READ_ACCESS | FILE_WRITE_ACCESS);
pub const IOCTL_MOUNTMGR_QUERY_AUTO_MOUNT: ULONG = ctl_code(
    MOUNTMGRCONTROLTYPE, 15, METHOD_BUFFERED, FILE_ANY_ACCESS);
pub const IOCTL_MOUNTMGR_SET_AUTO_MOUNT: ULONG = ctl_code(
    MOUNTMGRCONTROLTYPE, 16, METHOD_BUFFERED, FILE_READ_ACCESS | FILE_WRITE_ACCESS);

// Mount Device IOCTL
pub const IOCTL_MOUNTDEV_QUERY_DEVICE_NAME: ULONG = ctl_code(
    MOUNTDEVCONTROLTYPE, 2, METHOD_BUFFERED, FILE_ANY_ACCESS);
pub const IOCTL_MOUNTDEV_QUERY_UNIQUE_ID: ULONG = ctl_code(
    MOUNTDEVCONTROLTYPE, 0, METHOD_BUFFERED, FILE_ANY_ACCESS);
pub const IOCTL_MOUNTDEV_QUERY_SUGGESTED_LINK_NAME: ULONG = ctl_code(
    MOUNTDEVCONTROLTYPE, 3, METHOD_BUFFERED, FILE_ANY_ACCESS);

// =============================================================================
// MountMgr Structures (from mountmgr.h)
// =============================================================================

/// Input structure for IOCTL_MOUNTMGR_CREATE_POINT
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MOUNTMGR_CREATE_POINT_INPUT {
    pub symbolic_link_name_offset: USHORT,
    pub symbolic_link_name_length: USHORT,
    pub device_name_offset: USHORT,
    pub device_name_length: USHORT,
}

/// Input structure for IOCTL_MOUNTMGR_DELETE_POINTS, QUERY_POINTS, DELETE_POINTS_DBONLY
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MOUNTMGR_MOUNT_POINT {
    pub symbolic_link_name_offset: ULONG,
    pub symbolic_link_name_length: USHORT,
    pub unique_id_offset: ULONG,
    pub unique_id_length: USHORT,
    pub device_name_offset: ULONG,
    pub device_name_length: USHORT,
}

/// Output structure for IOCTL_MOUNTMGR_DELETE_POINTS, QUERY_POINTS, DELETE_POINTS_DBONLY
#[repr(C)]
pub struct MOUNTMGR_MOUNT_POINTS {
    pub size: ULONG,
    pub number_of_mount_points: ULONG,
    pub mount_points: [MOUNTMGR_MOUNT_POINT; 1], // Variable size
}

/// Input for IOCTL_MOUNTMGR_NEXT_DRIVE_LETTER
#[repr(C)]
pub struct MOUNTMGR_DRIVE_LETTER_TARGET {
    pub device_name_length: USHORT,
    pub device_name: [WCHAR; 1], // Variable size
}

/// Output for IOCTL_MOUNTMGR_NEXT_DRIVE_LETTER
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MOUNTMGR_DRIVE_LETTER_INFORMATION {
    pub drive_letter_was_assigned: BOOLEAN,
    pub current_drive_letter: UCHAR,
}

/// Input for IOCTL_MOUNTMGR_VOLUME_MOUNT_POINT_CREATED/DELETED
#[repr(C)]
pub struct MOUNTMGR_VOLUME_MOUNT_POINT {
    pub source_volume_name_offset: USHORT,
    pub source_volume_name_length: USHORT,
    pub target_volume_name_offset: USHORT,
    pub target_volume_name_length: USHORT,
}

/// Input/Output for IOCTL_MOUNTMGR_CHANGE_NOTIFY
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MOUNTMGR_CHANGE_NOTIFY_INFO {
    pub epic_number: ULONG,
}

/// Input for IOCTL_MOUNTMGR_KEEP_LINKS_WHEN_OFFLINE, VOLUME_ARRIVAL_NOTIFICATION,
/// QUERY_DOS_VOLUME_PATH, QUERY_DOS_VOLUME_PATHS
#[repr(C)]
pub struct MOUNTMGR_TARGET_NAME {
    pub device_name_length: USHORT,
    pub device_name: [WCHAR; 1], // Variable size
}

/// Output for IOCTL_MOUNTMGR_QUERY_DOS_VOLUME_PATH(S)
#[repr(C)]
pub struct MOUNTMGR_VOLUME_PATHS {
    pub multi_sz_length: ULONG,
    pub multi_sz: [WCHAR; 1], // Variable size
}

/// Output for IOCTL_MOUNTDEV_QUERY_DEVICE_NAME
#[repr(C)]
pub struct MOUNTDEV_NAME {
    pub name_length: USHORT,
    pub name: [WCHAR; 1], // Variable size
}

/// Output for IOCTL_MOUNTDEV_QUERY_UNIQUE_ID
#[repr(C)]
pub struct MOUNTDEV_UNIQUE_ID {
    pub unique_id_length: USHORT,
    pub unique_id: [UCHAR; 1], // Variable size
}

/// Output for IOCTL_MOUNTDEV_QUERY_SUGGESTED_LINK_NAME
#[repr(C)]
pub struct MOUNTDEV_SUGGESTED_LINK_NAME {
    pub use_only_if_there_are_no_other_links: BOOLEAN,
    pub name_length: USHORT,
    pub name: [WCHAR; 1], // Variable size
}

/// Auto-mount state enum
#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MOUNTMGR_AUTO_MOUNT_STATE {
    Disabled = 0,
    Enabled = 1,
}

/// Input/Output for IOCTL_MOUNTMGR_QUERY_AUTO_MOUNT / SET_AUTO_MOUNT
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MOUNTMGR_QUERY_AUTO_MOUNT {
    pub current_state: MOUNTMGR_AUTO_MOUNT_STATE,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MOUNTMGR_SET_AUTO_MOUNT {
    pub new_state: MOUNTMGR_AUTO_MOUNT_STATE,
}

// =============================================================================
// NT Kernel Structures
// =============================================================================

pub type PDEVICE_OBJECT = *mut DEVICE_OBJECT;
pub type PDRIVER_OBJECT = *mut DRIVER_OBJECT;
pub type PIRP = *mut IRP;
pub type PIO_STACK_LOCATION = *mut IO_STACK_LOCATION;

#[repr(C)]
pub struct DEVICE_OBJECT {
    pub r#type: CSHORT,
    pub size: USHORT,
    pub reference_count: ULONG,
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
    pub stack_size: CCHAR,
    _padding: [u8; 251],
}

#[repr(C)]
pub struct DRIVER_EXTENSION {
    pub driver_object: PDRIVER_OBJECT,
    pub add_device: Option<unsafe extern "win64" fn(PDRIVER_OBJECT, PDEVICE_OBJECT) -> NTSTATUS>,
    pub count: ULONG,
    pub service_key_name: UNICODE_STRING,
}

#[repr(C)]
pub struct DRIVER_OBJECT {
    pub r#type: CSHORT,
    pub size: CSHORT,
    pub device_object: PDEVICE_OBJECT,
    pub flags: ULONG,
    pub driver_start: PVOID,
    pub driver_size: ULONG,
    pub driver_section: PVOID,
    pub driver_extension: *mut DRIVER_EXTENSION,
    pub driver_name: UNICODE_STRING,
    pub hardware_database: *const UNICODE_STRING,
    pub fast_io_dispatch: PVOID,
    pub driver_init: PVOID,
    pub driver_start_io: PVOID,
    pub driver_unload: Option<unsafe extern "win64" fn(PDRIVER_OBJECT)>,
    pub major_function: [Option<unsafe extern "win64" fn(PDEVICE_OBJECT, PIRP) -> NTSTATUS>; 28],
}

#[repr(C)]
pub union IrpAssociated {
    pub master_irp: PIRP,
    pub irp_count: LONG,
    pub system_buffer: PVOID,
}

#[repr(C)]
pub struct IRP {
    pub r#type: CSHORT,
    pub size: USHORT,
    pub mdl_address: PVOID,
    pub flags: ULONG,
    pub associated_irp: IrpAssociated,
    pub thread_list_entry: [PVOID; 2],
    pub io_status: IO_STATUS_BLOCK,
    pub requestor_mode: CCHAR,
    pub pending_returned: bool,
    pub stack_count: CCHAR,
    pub current_location: CCHAR,
    pub cancel: bool,
    pub cancel_irql: UCHAR,
    pub apc_environment: CCHAR,
    pub allocation_flags: UCHAR,
    pub user_iosb: *mut IO_STATUS_BLOCK,
    pub user_event: PVOID,
    pub overlay: IrpOverlay,
    pub cancel_routine: PVOID,
    pub user_buffer: PVOID,
    pub tail: IrpTail,
}

#[repr(C)]
pub union IrpOverlay {
    pub asynchronous_parameters: AsyncParams,
    pub allocation_size: i64,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct AsyncParams {
    pub user_apc_routine: PVOID,
    pub user_apc_context: PVOID,
}

#[repr(C)]
pub union IrpTail {
    pub overlay: IrpTailOverlay,
    pub apc: [u8; 64],
    pub completion_key: PVOID,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct IrpTailOverlay {
    pub driver_context: [PVOID; 4],
    pub thread: PVOID,
    pub auxiliary_buffer: PVOID,
    pub list_entry: [PVOID; 2],
    pub current_stack_location: PIO_STACK_LOCATION,
    pub original_file_object: PVOID,
}

#[repr(C)]
pub struct IO_STATUS_BLOCK {
    pub status: NTSTATUS,
    pub information: ULONG_PTR,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct UNICODE_STRING {
    pub length: USHORT,
    pub maximum_length: USHORT,
    pub buffer: *mut u16,
}

impl UNICODE_STRING {
    pub const fn empty() -> Self {
        Self {
            length: 0,
            maximum_length: 0,
            buffer: core::ptr::null_mut(),
        }
    }
}

#[repr(C)]
pub struct IO_STACK_LOCATION {
    pub major_function: UCHAR,
    pub minor_function: UCHAR,
    pub flags: UCHAR,
    pub control: UCHAR,
    pub _padding: [u8; 4],
    pub parameters: [u8; 32],
    pub device_object: PDEVICE_OBJECT,
    pub file_object: PVOID,
    pub completion_routine: PVOID,
    pub context: PVOID,
}

// =============================================================================
// Memory Pool Constants
// =============================================================================

pub const NON_PAGED_POOL: u32 = 0;
pub const MOUNTMGR_POOL_TAG: u32 = u32::from_le_bytes(*b"MtMg");

// =============================================================================
// GUID Structure
// =============================================================================

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct GUID {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

impl GUID {
    pub const fn zero() -> Self {
        Self {
            data1: 0,
            data2: 0,
            data3: 0,
            data4: [0; 8],
        }
    }
}

/// MOUNTDEV_MOUNTED_DEVICE_GUID = {53F5630D-B6BF-11D0-94F2-00A0C91EFB8B}
/// Same as GUID_DEVINTERFACE_VOLUME
pub const MOUNTDEV_MOUNTED_DEVICE_GUID: GUID = GUID {
    data1: 0x53F5630D,
    data2: 0xB6BF,
    data3: 0x11D0,
    data4: [0x94, 0xF2, 0x00, 0xA0, 0xC9, 0x1E, 0xFB, 0x8B],
};
