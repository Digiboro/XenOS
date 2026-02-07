//! Types for VolMgr driver

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
pub type LONGLONG = i64;

// Status codes
pub const STATUS_SUCCESS: NTSTATUS = 0;
pub const STATUS_NO_SUCH_DEVICE: NTSTATUS = 0xC000000Eu32 as i32;
pub const STATUS_INVALID_DEVICE_REQUEST: NTSTATUS = 0xC0000010u32 as i32;
pub const STATUS_NOT_SUPPORTED: NTSTATUS = 0xC00000BBu32 as i32;
pub const STATUS_INSUFFICIENT_RESOURCES: NTSTATUS = 0xC000009Au32 as i32;
pub const STATUS_INVALID_PARAMETER: NTSTATUS = 0xC000000Du32 as i32;
pub const STATUS_BUFFER_TOO_SMALL: NTSTATUS = 0xC0000023u32 as i32;

// Device types
pub const FILE_DEVICE_DISK: ULONG = 0x00000007;
pub const FILE_DEVICE_UNKNOWN: ULONG = 0x00000022;

// Device flags
pub const DO_DIRECT_IO: ULONG = 0x00000010;
pub const DO_BUFFERED_IO: ULONG = 0x00000004;
pub const DO_POWER_PAGABLE: ULONG = 0x00002000;
pub const DO_DEVICE_INITIALIZING: ULONG = 0x00000080;

// IRP major function codes
pub const IRP_MJ_CREATE: UCHAR = 0;
pub const IRP_MJ_CLOSE: UCHAR = 2;
pub const IRP_MJ_READ: UCHAR = 3;
pub const IRP_MJ_WRITE: UCHAR = 4;
pub const IRP_MJ_DEVICE_CONTROL: UCHAR = 14;
pub const IRP_MJ_PNP: UCHAR = 0x1B;
pub const IRP_MJ_POWER: UCHAR = 0x16;

// IRP minor PnP codes
pub const IRP_MN_START_DEVICE: UCHAR = 0x00;
pub const IRP_MN_QUERY_DEVICE_RELATIONS: UCHAR = 0x07;
pub const IRP_MN_QUERY_ID: UCHAR = 0x13;
pub const IRP_MN_QUERY_CAPABILITIES: UCHAR = 0x09;
pub const IRP_MN_REMOVE_DEVICE: UCHAR = 0x02;

// Device relations type
pub const BUS_RELATIONS: ULONG = 0;
pub const TARGET_DEVICE_RELATION: ULONG = 4;

pub const IO_NO_INCREMENT: CCHAR = 0;

// Volume IOCTLs (from ntddvol.h)
pub const IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS: u32 = 0x00560000;
pub const IOCTL_VOLUME_ONLINE: u32 = 0x0056C008;
pub const IOCTL_VOLUME_OFFLINE: u32 = 0x0056C00C;
pub const IOCTL_VOLUME_IS_OFFLINE: u32 = 0x00560010;
pub const IOCTL_VOLUME_SUPPORTS_ONLINE_OFFLINE: u32 = 0x00560014;
pub const IOCTL_VOLUME_GET_GPT_ATTRIBUTES: u32 = 0x00560038;

// Disk IOCTLs (passed through)
pub const IOCTL_DISK_GET_LENGTH_INFO: u32 = 0x0007405C;
pub const IOCTL_DISK_GET_DRIVE_GEOMETRY: u32 = 0x00070000;
pub const IOCTL_DISK_GET_PARTITION_INFO: u32 = 0x00074004;

// Memory pool types
pub const NON_PAGED_POOL: u32 = 0;

pub const VOLMGR_POOL_TAG: u32 = u32::from_le_bytes(*b"VolM");

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
pub struct UNICODE_STRING {
    pub length: USHORT,
    pub maximum_length: USHORT,
    pub buffer: *mut u16,
}

#[repr(C)]
pub struct IO_STACK_LOCATION {
    pub major_function: UCHAR,
    pub minor_function: UCHAR,
    pub flags: UCHAR,
    pub control: UCHAR,
    pub _padding: [u8; 4],  // Alignment for parameters
    pub parameters: [u8; 32],
    pub device_object: PDEVICE_OBJECT,
    pub file_object: PVOID,
    pub completion_routine: PVOID,
    pub context: PVOID,
}

// =============================================================================
// Volume Structures (from ntddvol.h)
// =============================================================================

/// DISK_EXTENT - describes a single extent of a volume
#[repr(C)]
pub struct DISK_EXTENT {
    pub disk_number: ULONG,
    pub starting_offset: LONGLONG,
    pub extent_length: LONGLONG,
}

/// VOLUME_DISK_EXTENTS - returned by IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS
#[repr(C)]
pub struct VOLUME_DISK_EXTENTS {
    pub number_of_disk_extents: ULONG,
    pub extents: [DISK_EXTENT; 1],  // Variable size array
}

/// GET_LENGTH_INFORMATION - for IOCTL_DISK_GET_LENGTH_INFO
#[repr(C)]
pub struct GET_LENGTH_INFORMATION {
    pub length: LONGLONG,
}

/// PARTITION_INFORMATION - for IOCTL_DISK_GET_PARTITION_INFO
#[repr(C)]
pub struct PARTITION_INFORMATION {
    pub starting_offset: LONGLONG,
    pub partition_length: LONGLONG,
    pub hidden_sectors: ULONG,
    pub partition_number: ULONG,
    pub partition_type: UCHAR,
    pub bootable: BOOLEAN,
    pub recognized_partition: BOOLEAN,
    pub rewrite_partition: BOOLEAN,
}

// STATUS constants
pub const STATUS_PENDING: NTSTATUS = 0x00000103;
pub const STATUS_UNSUCCESSFUL: NTSTATUS = 0xC0000001u32 as i32;
pub const STATUS_MORE_PROCESSING_REQUIRED: NTSTATUS = 0xC0000016u32 as i32;

// =============================================================================
// GUID structures
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

/// GUID_DEVINTERFACE_VOLUME = {53F5630D-B6BF-11D0-94F2-00A0C91EFB8B}
pub const GUID_DEVINTERFACE_VOLUME: GUID = GUID {
    data1: 0x53F5630D,
    data2: 0xB6BF,
    data3: 0x11D0,
    data4: [0x94, 0xF2, 0x00, 0xA0, 0xC9, 0x1E, 0xFB, 0x8B],
};
