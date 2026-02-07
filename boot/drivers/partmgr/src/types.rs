//! Types for PartMgr driver

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
pub const STATUS_UNSUCCESSFUL: NTSTATUS = 0xC0000001u32 as i32;

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
pub const IRP_MJ_SCSI: UCHAR = 15;
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
pub const REMOVAL_RELATIONS: ULONG = 2;
pub const TARGET_DEVICE_RELATION: ULONG = 4;

pub const IO_NO_INCREMENT: CCHAR = 0;

// IOCTLs
pub const IOCTL_DISK_GET_PARTITION_INFO: u32 = 0x00074004;
pub const IOCTL_DISK_GET_LENGTH_INFO: u32 = 0x0007405C;
pub const IOCTL_DISK_GET_DRIVE_GEOMETRY: u32 = 0x00070000;

// Memory pool types
pub const NON_PAGED_POOL: u32 = 0;

// SRB constants
pub const SRB_FUNCTION_EXECUTE_SCSI: u8 = 0x00;
pub const SRB_STATUS_SUCCESS: u8 = 0x01;
pub const SRB_FLAGS_DATA_IN: ULONG = 0x00000040;
pub const SRB_FLAGS_DISABLE_SYNCH_TRANSFER: ULONG = 0x00000008;

pub type PDEVICE_OBJECT = *mut DEVICE_OBJECT;
pub type PDRIVER_OBJECT = *mut DRIVER_OBJECT;
pub type PIRP = *mut IRP;
pub type PIO_STACK_LOCATION = *mut IO_STACK_LOCATION;
pub type PVPB = *mut VPB;

/// VPB - Volume Parameter Block for file system mounting
#[repr(C)]
pub struct VPB {
    pub r#type: CSHORT,
    pub size: CSHORT,
    pub flags: USHORT,
    pub volume_label_length: USHORT,
    pub device_object: PDEVICE_OBJECT,  // FS device (NULL if not mounted)
    pub real_device: PDEVICE_OBJECT,     // Storage device
    pub serial_number: ULONG,
    pub reference_count: ULONG,
    pub volume_label: [u16; 32],
}

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
    pub vpb: PVPB,
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
pub union AssociatedIrp {
    pub system_buffer: PVOID,
    pub master_irp: PIRP,
}

#[repr(C)]
pub struct IRP {
    pub r#type: CSHORT,
    pub size: USHORT,
    pub mdl_address: PVOID,
    pub flags: ULONG,
    pub associated_irp: AssociatedIrp,
    pub thread_list_entry: [PVOID; 2],
    pub io_status: IO_STATUS_BLOCK,
    pub requestor_mode: CCHAR,
    pub pending_returned: BOOLEAN,
    pub stack_count: CCHAR,
    pub current_location: CCHAR,
    pub cancel: BOOLEAN,
    pub cancel_irql: u8,
    pub apc_environment: CCHAR,
    pub allocation_flags: UCHAR,
    pub user_io_status_block: *mut IO_STATUS_BLOCK,
    pub user_event: PVOID,
    pub overlay: [u8; 16],
    pub cancel_routine: PVOID,
    pub user_buffer: PVOID,
    pub tail: IrpTail,
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
    pub _padding: [u8; 4],  // Padding for 8-byte alignment of parameters
    pub parameters: [u8; 32], // Union of various parameter structs (aligned to 8 bytes)
    pub device_object: PDEVICE_OBJECT,
    pub file_object: PVOID,
    pub completion_routine: PVOID,
    pub context: PVOID,
}

#[repr(C)]
pub struct DEVICE_RELATIONS {
    pub count: ULONG,
    pub objects: [PDEVICE_OBJECT; 1], // Variable size array
}

/// DEVICE_CAPABILITIES - using u8 for bit flags for simplicity
#[repr(C)]
pub struct DEVICE_CAPABILITIES {
    pub size: USHORT,
    pub version: USHORT,
    // Bitfield flags - for simplicity use separate u8 fields
    pub device_d1: u8,
    pub device_d2: u8,
    pub lock_supported: u8,
    pub eject_supported: u8,
    pub removable: u8,
    pub dock_device: u8,
    pub unique_id: u8,
    pub silent_install: u8,
    pub raw_device_ok: u8,
    pub surprise_removal_ok: u8,
    pub wake_from_d0: u8,
    pub wake_from_d1: u8,
    pub wake_from_d2: u8,
    pub wake_from_d3: u8,
    pub hardware_disabled: u8,
    pub non_dynamic: u8,
    pub warm_eject_supported: u8,
    pub no_display_in_ui: u8,
    pub reserved1: u8,
    pub reserved2: u8,
    _padding: [u8; 2],
    pub address: ULONG,
    pub ui_number: ULONG,
    pub device_state: [u32; 7], // DEVICE_POWER_STATE[PowerSystemMaximum]
    pub system_wake: u32,
    pub device_wake: u32,
    pub d1_latency: ULONG,
    pub d2_latency: ULONG,
    pub d3_latency: ULONG,
}

/// PARTITION_INFORMATION structure (from ntdddisk.h)
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

/// GET_LENGTH_INFORMATION structure
#[repr(C)]
pub struct GET_LENGTH_INFORMATION {
    pub length: LONGLONG,
}

/// SCSI Request Block (simplified)
#[repr(C)]
pub struct SCSI_REQUEST_BLOCK {
    pub length: USHORT,
    pub function: UCHAR,
    pub srb_status: UCHAR,
    pub scsi_status: UCHAR,
    pub path_id: UCHAR,
    pub target_id: UCHAR,
    pub lun: UCHAR,
    pub queue_tag: UCHAR,
    pub queue_action: UCHAR,
    pub cdb_length: UCHAR,
    pub sense_info_buffer_length: UCHAR,
    pub srb_flags: ULONG,
    pub data_transfer_length: ULONG,
    pub time_out_value: ULONG,
    pub data_buffer: PVOID,
    pub sense_info_buffer: PVOID,
    pub next_srb: *mut SCSI_REQUEST_BLOCK,
    pub original_request: PVOID,
    pub srb_extension: PVOID,
    pub internal_status: ULONG,
    pub reserved: ULONG,
    pub cdb: [u8; 16],
}
