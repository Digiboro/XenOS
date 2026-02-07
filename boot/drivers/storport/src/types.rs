//! NT типы и константы для StorPort

// =============================================================================
// Базовые типы
// =============================================================================

pub type NTSTATUS = i32;
pub type ULONG = u32;
pub type USHORT = u16;
pub type UCHAR = u8;
pub type CSHORT = i16;
pub type BOOLEAN = u8;
pub type PVOID = *mut core::ffi::c_void;
pub type ULONG_PTR = usize;
pub type LONG = i32;
pub type LONGLONG = i64;
pub type ULONGLONG = u64;
pub type CCHAR = i8;

pub type PUCHAR = *mut UCHAR;
pub type PULONG = *mut ULONG;
pub type PUSHORT = *mut USHORT;

// =============================================================================
// Memory Constants
// =============================================================================

/// Page size (4KB for x86-64)
pub const PAGE_SIZE: usize = 0x1000;
/// Page shift (log2 of PAGE_SIZE)
pub const PAGE_SHIFT: usize = 12;
/// Page mask (PAGE_SIZE - 1)
pub const PAGE_MASK: usize = 0xFFF;

// =============================================================================
// Pool Types
// =============================================================================

/// Pool type for NonPaged pool (kernel memory, always resident)
pub const NonPagedPool: ULONG = 0;
/// Pool type for Paged pool (can be paged out)
pub const PagedPool: ULONG = 1;
/// NonPaged pool with cache alignment
pub const NonPagedPoolCacheAligned: ULONG = 4;

// =============================================================================
// Статусы
// =============================================================================

pub const STATUS_SUCCESS: NTSTATUS = 0;
pub const STATUS_UNSUCCESSFUL: NTSTATUS = 0xC0000001_u32 as i32;
pub const STATUS_NOT_IMPLEMENTED: NTSTATUS = 0xC0000002_u32 as i32;
pub const STATUS_INVALID_PARAMETER: NTSTATUS = 0xC000000D_u32 as i32;
pub const STATUS_NO_SUCH_DEVICE: NTSTATUS = 0xC000000E_u32 as i32;
pub const STATUS_INVALID_DEVICE_REQUEST: NTSTATUS = 0xC0000010_u32 as i32;
pub const STATUS_INSUFFICIENT_RESOURCES: NTSTATUS = 0xC000009A_u32 as i32;
pub const STATUS_DEVICE_NOT_CONNECTED: NTSTATUS = 0xC000009D_u32 as i32;
pub const STATUS_DEVICE_BUSY: NTSTATUS = 0x80000011_u32 as i32;
pub const STATUS_NOT_SUPPORTED: NTSTATUS = 0xC00000BB_u32 as i32;
pub const STATUS_PENDING: NTSTATUS = 0x00000103;
pub const STATUS_REQUEST_ABORTED: NTSTATUS = 0xC0000240_u32 as i32;
pub const STATUS_IO_DEVICE_ERROR: NTSTATUS = 0xC0000185_u32 as i32;
pub const STATUS_IO_TIMEOUT: NTSTATUS = 0xC00000B5_u32 as i32;
pub const STATUS_ADAPTER_HARDWARE_ERROR: NTSTATUS = 0xC00000C5_u32 as i32;
pub const STATUS_DATA_OVERRUN: NTSTATUS = 0xC000003C_u32 as i32;
pub const STATUS_DEVICE_POWERED_OFF: NTSTATUS = 0xC00002D4_u32 as i32;

// =============================================================================
// IRP Major Functions
// =============================================================================

pub const IRP_MJ_CREATE: u8 = 0x00;
pub const IRP_MJ_CLOSE: u8 = 0x02;
pub const IRP_MJ_READ: u8 = 0x03;
pub const IRP_MJ_WRITE: u8 = 0x04;
pub const IRP_MJ_DEVICE_CONTROL: u8 = 0x0E;
pub const IRP_MJ_INTERNAL_DEVICE_CONTROL: u8 = 0x0F;
pub const IRP_MJ_PNP: u8 = 0x1B;
pub const IRP_MJ_POWER: u8 = 0x16;
pub const IRP_MJ_SCSI: u8 = 0x0F; // Same as INTERNAL_DEVICE_CONTROL

// IRP Minor Functions (PnP)
pub const IRP_MN_START_DEVICE: u8 = 0x00;
pub const IRP_MN_QUERY_REMOVE_DEVICE: u8 = 0x01;
pub const IRP_MN_REMOVE_DEVICE: u8 = 0x02;
pub const IRP_MN_CANCEL_REMOVE_DEVICE: u8 = 0x03;
pub const IRP_MN_STOP_DEVICE: u8 = 0x04;
pub const IRP_MN_QUERY_STOP_DEVICE: u8 = 0x05;
pub const IRP_MN_CANCEL_STOP_DEVICE: u8 = 0x06;
pub const IRP_MN_QUERY_DEVICE_RELATIONS: u8 = 0x07;
pub const IRP_MN_QUERY_INTERFACE: u8 = 0x08;
pub const IRP_MN_QUERY_CAPABILITIES: u8 = 0x09;
pub const IRP_MN_QUERY_ID: u8 = 0x13;

// Device Relations
pub const BUS_RELATIONS: ULONG = 0;
pub const TARGET_DEVICE_RELATION: ULONG = 3;

// IRP Minor Functions (PnP) - Additional
pub const IRP_MN_SURPRISE_REMOVAL: u8 = 0x17;
pub const IRP_MN_QUERY_RESOURCE_REQUIREMENTS: u8 = 0x0B;
pub const IRP_MN_QUERY_RESOURCES: u8 = 0x0A;
pub const IRP_MN_QUERY_DEVICE_TEXT: u8 = 0x0C;
pub const IRP_MN_FILTER_RESOURCE_REQUIREMENTS: u8 = 0x0D;
pub const IRP_MN_QUERY_BUS_INFORMATION: u8 = 0x15;
pub const IRP_MN_DEVICE_USAGE_NOTIFICATION: u8 = 0x16;

// IRP Minor Functions (Power)
pub const IRP_MN_WAIT_WAKE: u8 = 0x00;
pub const IRP_MN_POWER_SEQUENCE: u8 = 0x01;
pub const IRP_MN_SET_POWER: u8 = 0x02;
pub const IRP_MN_QUERY_POWER: u8 = 0x03;

// IO_STACK_LOCATION Control flags (for completion routine)
pub const SL_PENDING_RETURNED: UCHAR = 0x01;
pub const SL_INVOKE_ON_CANCEL: UCHAR = 0x20;
pub const SL_INVOKE_ON_SUCCESS: UCHAR = 0x40;
pub const SL_INVOKE_ON_ERROR: UCHAR = 0x80;

// KWAIT_REASON enum values
pub const EXECUTIVE: ULONG = 0;
pub const FREEPAGE: ULONG = 1;
pub const PAGEIN: ULONG = 2;
pub const POOLALLOCATION: ULONG = 3;
pub const DELAYTIMEOUT: ULONG = 4;
pub const SUSPENDED: ULONG = 5;
pub const USERREQUESTED: ULONG = 6;
pub const WRKERNELQUEUE: ULONG = 7;
pub const WRFREEPAGE: ULONG = 8;
pub const WRPAGEIN: ULONG = 9;
pub const WRPOOLALLOCATION: ULONG = 10;
pub const WRDELAYTIMEOUT: ULONG = 11;
pub const WRSUSPENDED: ULONG = 12;
pub const WRUSERREQUESTED: ULONG = 13;
pub const WREVENTPAIR: ULONG = 14;
pub const WRQUEUE: ULONG = 15;
pub const WRLDRINITIALIZE: ULONG = 16;
pub const WRRESOURCEDEADLOCKPOSSIBLE: ULONG = 17;
pub const WRPUSHLOCK: ULONG = 18;
pub const WRMUTEX: ULONG = 19;

// KPROCESSOR_MODE enum
pub const KERNEL_MODE: CCHAR = 0;
pub const USER_MODE: CCHAR = 1;

// MORE_PROCESSING_REQUIRED status for completion routines
pub const STATUS_MORE_PROCESSING_REQUIRED: NTSTATUS = 0xC0000016_u32 as i32;

// =============================================================================
// Device Types
// =============================================================================

pub const FILE_DEVICE_CONTROLLER: ULONG = 0x00000004;
pub const FILE_DEVICE_DISK: ULONG = 0x00000007;
pub const FILE_DEVICE_MASS_STORAGE: ULONG = 0x0000002D;

// =============================================================================
// Device Flags
// =============================================================================

pub const DO_DIRECT_IO: ULONG = 0x00000010;
pub const DO_BUFFERED_IO: ULONG = 0x00000004;
pub const DO_DEVICE_INITIALIZING: ULONG = 0x00000080;
pub const DO_POWER_PAGABLE: ULONG = 0x00002000;

// =============================================================================
// Pool Types
// =============================================================================

pub const NON_PAGED_POOL: ULONG = 0;
pub const PAGED_POOL: ULONG = 1;

// =============================================================================
// IO Priority
// =============================================================================

pub const IO_NO_INCREMENT: CCHAR = 0;

// =============================================================================
// Структуры
// =============================================================================

#[repr(C)]
pub struct UNICODE_STRING {
    pub length: USHORT,
    pub maximum_length: USHORT,
    pub buffer: *mut u16,
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
    pub vpb: PVOID,
    pub device_extension: PVOID,
    pub device_type: ULONG,
    pub stack_size: CCHAR,
    pub queue: [u8; 120], // KDEVICE_QUEUE
    pub alignment_requirement: ULONG,
    pub device_queue: [u8; 80], // KDEVICE_QUEUE_ENTRY list
    pub dpc: [u8; 64], // KDPC
    pub active_threads: ULONG,
    pub security_descriptor: PVOID,
    pub device_lock: [u8; 16], // KEVENT
    pub sector_size: USHORT,
    pub spare1: USHORT,
    pub device_object_extension: PVOID,
    pub reserved: PVOID,
}

pub type PDEVICE_OBJECT = *mut DEVICE_OBJECT;

#[repr(C)]
pub struct DRIVER_EXTENSION {
    pub driver_object: PDRIVER_OBJECT,
    pub add_device: Option<unsafe extern "win64" fn(PDRIVER_OBJECT, PDEVICE_OBJECT) -> NTSTATUS>,
    pub count: ULONG,
    pub service_key_name: UNICODE_STRING,
}

pub type PDRIVER_EXTENSION = *mut DRIVER_EXTENSION;

#[repr(C)]
pub struct DRIVER_OBJECT {
    pub r#type: CSHORT,
    pub size: CSHORT,
    pub device_object: PDEVICE_OBJECT,
    pub flags: ULONG,
    pub driver_start: PVOID,
    pub driver_size: ULONG,
    pub driver_section: PVOID,
    pub driver_extension: PDRIVER_EXTENSION,
    pub driver_name: UNICODE_STRING,
    pub hardware_database: *const UNICODE_STRING,
    pub fast_io_dispatch: PVOID,
    pub driver_init: PVOID,
    pub driver_start_io: PVOID,
    pub driver_unload: Option<unsafe extern "win64" fn(PDRIVER_OBJECT)>,
    pub major_function: [Option<unsafe extern "win64" fn(PDEVICE_OBJECT, PIRP) -> NTSTATUS>; 28],
}

pub type PDRIVER_OBJECT = *mut DRIVER_OBJECT;

#[repr(C)]
pub struct IRP {
    pub r#type: CSHORT,
    pub size: USHORT,
    pub mdl_address: PVOID,
    pub flags: ULONG,
    pub associated_irp: PVOID,
    pub thread_list_entry: [PVOID; 2],
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

#[repr(C)]
pub struct IO_STATUS_BLOCK {
    pub status: NTSTATUS,
    pub information: ULONG_PTR,
}

#[repr(C)]
pub struct IO_STACK_LOCATION {
    pub major_function: UCHAR,
    pub minor_function: UCHAR,
    pub flags: UCHAR,
    pub control: UCHAR,
    pub _padding: [u8; 4],      // Padding для выравнивания parameters на 8 байт
    pub parameters: [u8; 40],   // offset 8
    pub device_object: PDEVICE_OBJECT,
    pub file_object: PVOID,
    pub completion_routine: PVOID,
    pub context: PVOID,
}

pub type PIO_STACK_LOCATION = *mut IO_STACK_LOCATION;

// =============================================================================
// Hardware Initialization Data
// =============================================================================
//
// Reference: WinDDK 7600.16385.1/inc/ddk/storport.h (lines 4931-5073)
// CRITICAL: Field order MUST match WinDDK exactly for ABI compatibility!
//

/// Miniport callback function types
pub type PHW_INITIALIZE = unsafe extern "win64" fn(device_extension: PVOID) -> BOOLEAN;

pub type PHW_STARTIO = unsafe extern "win64" fn(device_extension: PVOID, srb: PSCSI_REQUEST_BLOCK) -> BOOLEAN;

pub type PHW_INTERRUPT = unsafe extern "win64" fn(device_extension: PVOID) -> BOOLEAN;

pub type PHW_FIND_ADAPTER = unsafe extern "win64" fn(
    device_extension: PVOID,
    hw_context: PVOID,
    bus_information: PVOID,
    argument_string: PVOID,
    config_info: PPORT_CONFIGURATION_INFORMATION,
    again: *mut BOOLEAN,
) -> ULONG;

pub type PHW_RESET_BUS = unsafe extern "win64" fn(device_extension: PVOID, path_id: ULONG) -> BOOLEAN;

pub type PHW_DMA_STARTED = unsafe extern "win64" fn(device_extension: PVOID);

pub type PHW_ADAPTER_STATE = unsafe extern "win64" fn(
    device_extension: PVOID,
    context: PVOID,
    save_state: BOOLEAN,
) -> SCSI_ADAPTER_CONTROL_STATUS;

pub type PHW_ADAPTER_CONTROL = unsafe extern "win64" fn(
    device_extension: PVOID,
    control_type: SCSI_ADAPTER_CONTROL_TYPE,
    parameters: PVOID,
) -> SCSI_ADAPTER_CONTROL_STATUS;

pub type PHW_BUILDIO = unsafe extern "win64" fn(device_extension: PVOID, srb: PSCSI_REQUEST_BLOCK) -> BOOLEAN;

/// SCSI_ADAPTER_CONTROL_TYPE enumeration
pub type SCSI_ADAPTER_CONTROL_TYPE = ULONG;
pub const SCSI_ADAPTER_CONTROL_STOP: SCSI_ADAPTER_CONTROL_TYPE = 0;
pub const SCSI_ADAPTER_CONTROL_RESTART: SCSI_ADAPTER_CONTROL_TYPE = 1;

/// SCSI_ADAPTER_CONTROL_STATUS enumeration
pub type SCSI_ADAPTER_CONTROL_STATUS = ULONG;
pub const SCSI_ADAPTER_CONTROL_SUCCESS: SCSI_ADAPTER_CONTROL_STATUS = 0;
pub const SCSI_ADAPTER_CONTROL_UNSUCCESSFUL: SCSI_ADAPTER_CONTROL_STATUS = 1;

/// Hardware Initialization Data
///
/// Structure used by miniport drivers to register with StorPort.
/// Field order matches WinDDK 7600.16385.1 storport.h exactly.
///
/// CRITICAL: Do NOT reorder fields! ABI compatibility with Windows drivers depends on this.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct HW_INITIALIZATION_DATA {
    /// Size of this structure
    pub HwInitializationDataSize: ULONG,
    /// Adapter interface type (PCIBus = 5, etc.)
    pub AdapterInterfaceType: ULONG,  // INTERFACE_TYPE enum
    
    // Miniport driver routines - ORDER IS CRITICAL!
    pub HwInitialize: Option<PHW_INITIALIZE>,
    pub HwStartIo: Option<PHW_STARTIO>,
    pub HwInterrupt: Option<PHW_INTERRUPT>,
    pub HwFindAdapter: Option<PHW_FIND_ADAPTER>,
    pub HwResetBus: Option<PHW_RESET_BUS>,
    pub HwDmaStarted: Option<PHW_DMA_STARTED>,
    pub HwAdapterState: Option<PHW_ADAPTER_STATE>,
    
    // Miniport driver resources
    pub DeviceExtensionSize: ULONG,
    pub SpecificLuExtensionSize: ULONG,
    pub SrbExtensionSize: ULONG,
    pub NumberOfAccessRanges: ULONG,
    pub Reserved: PVOID,
    
    // Flags
    pub MapBuffers: UCHAR,  // Note: UCHAR, not BOOLEAN
    pub NeedPhysicalAddresses: BOOLEAN,
    pub TaggedQueuing: BOOLEAN,
    pub AutoRequestSense: BOOLEAN,
    pub MultipleRequestPerLu: BOOLEAN,
    pub ReceiveEvent: BOOLEAN,
    
    // Vendor/Device identification
    pub VendorIdLength: USHORT,
    pub VendorId: PVOID,
    pub PortVersionFlags: USHORT,  // Union with ReservedUshort
    pub DeviceIdLength: USHORT,
    pub DeviceId: PVOID,
    
    // Additional callbacks
    pub HwAdapterControl: Option<PHW_ADAPTER_CONTROL>,
    pub HwBuildIo: Option<PHW_BUILDIO>,
}

pub type PHW_INITIALIZATION_DATA = *mut HW_INITIALIZATION_DATA;

// Legacy type alias for backward compatibility
pub type PHW_START_IO = PHW_STARTIO;

// =============================================================================
// Port Configuration Information
// =============================================================================
//
// Reference: WinDDK 7600.16385.1/inc/ddk/storport.h (lines 4145-4400)
// CRITICAL: Field order MUST match WinDDK exactly for ABI compatibility!
//

/// KINTERRUPT_MODE enumeration
pub type KINTERRUPT_MODE = ULONG;
pub const LEVEL_SENSITIVE: KINTERRUPT_MODE = 0;
pub const LATCHED: KINTERRUPT_MODE = 1;

/// DMA_WIDTH enumeration  
pub type DMA_WIDTH = ULONG;
pub const WIDTH8BITS: DMA_WIDTH = 0;
pub const WIDTH16BITS: DMA_WIDTH = 1;
pub const WIDTH32BITS: DMA_WIDTH = 2;
pub const MAXIMUMDMAWIDTH: DMA_WIDTH = 3;

/// DMA_SPEED enumeration
pub type DMA_SPEED = ULONG;
pub const COMPATIBLE: DMA_SPEED = 0;
pub const TYPEA: DMA_SPEED = 1;
pub const TYPEB: DMA_SPEED = 2;
pub const TYPEC: DMA_SPEED = 3;
pub const TYPEF: DMA_SPEED = 4;
pub const MAXIMUMDMASPEED: DMA_SPEED = 5;

/// Port Configuration Information
///
/// Structure passed to HwFindAdapter containing adapter configuration.
/// Field order matches WinDDK 7600.16385.1 storport.h exactly.
#[repr(C)]
pub struct PORT_CONFIGURATION_INFORMATION {
    /// Length of this structure
    pub Length: ULONG,
    /// IO bus number
    pub SystemIoBusNumber: ULONG,
    /// Adapter interface type (PCIBus = 5, etc.)
    pub AdapterInterfaceType: ULONG,  // INTERFACE_TYPE enum
    /// Interrupt request level
    pub BusInterruptLevel: ULONG,
    /// Bus interrupt vector
    pub BusInterruptVector: ULONG,
    /// Interrupt mode (level-sensitive or edge-triggered)
    pub InterruptMode: KINTERRUPT_MODE,
    /// Maximum bytes per SRB transfer
    pub MaximumTransferLength: ULONG,
    /// Number of physical breaks (scatter/gather segments)
    pub NumberOfPhysicalBreaks: ULONG,
    /// DMA channel for system DMA
    pub DmaChannel: ULONG,
    pub DmaPort: ULONG,
    pub DmaWidth: DMA_WIDTH,
    pub DmaSpeed: DMA_SPEED,
    /// Alignment mask for data transfers
    pub AlignmentMask: ULONG,
    /// Number of access range elements allocated
    pub NumberOfAccessRanges: ULONG,
    /// Pointer to array of access range elements
    pub AccessRanges: *mut ACCESS_RANGE,
    /// Reserved
    pub Reserved: PVOID,
    /// Number of SCSI buses
    pub NumberOfBuses: UCHAR,
    /// SCSI bus ID for adapter (per bus)
    pub InitiatorBusId: [CCHAR; 8],
    /// Adapter does scatter/gather
    pub ScatterGather: BOOLEAN,
    /// Adapter is bus master
    pub Master: BOOLEAN,
    /// Host caches data
    pub CachesData: BOOLEAN,
    /// Adapter scans down for BIOS devices
    pub AdapterScansDown: BOOLEAN,
    /// Primary AT disk address claimed
    pub AtdiskPrimaryClaimed: BOOLEAN,
    /// Secondary AT disk address claimed
    pub AtdiskSecondaryClaimed: BOOLEAN,
    /// Master uses 32-bit DMA addresses
    pub Dma32BitAddresses: BOOLEAN,
    /// Use demand mode DMA
    pub DemandMode: BOOLEAN,
    /// Data buffers must be mapped to virtual address space
    pub MapBuffers: UCHAR,  // Note: UCHAR, not BOOLEAN
    /// Miniport needs virtual to physical translation
    pub NeedPhysicalAddresses: BOOLEAN,
    /// Supports tagged queuing
    pub TaggedQueuing: BOOLEAN,
    /// Supports auto request sense
    pub AutoRequestSense: BOOLEAN,
    /// Supports multiple requests per LU
    pub MultipleRequestPerLu: BOOLEAN,
    /// Support receive event function
    pub ReceiveEvent: BOOLEAN,
    /// Real-mode driver initialized the card
    pub RealModeInitialized: BOOLEAN,
    /// Miniport will not touch data buffers directly
    pub BufferAccessScsiPortControlled: BOOLEAN,
    /// Maximum number of targets (wide SCSI)
    pub MaximumNumberOfTargets: UCHAR,
    /// Reserved for alignment
    pub ReservedUchars: [UCHAR; 2],
    /// Adapter slot number
    pub SlotNumber: ULONG,
    /// Second IRQ info
    pub BusInterruptLevel2: ULONG,
    pub BusInterruptVector2: ULONG,
    pub InterruptMode2: KINTERRUPT_MODE,
    /// Second DMA channel info
    pub DmaChannel2: ULONG,
    pub DmaPort2: ULONG,
    pub DmaWidth2: DMA_WIDTH,
    pub DmaSpeed2: DMA_SPEED,
    /// Extension sizes (can be updated by miniport)
    pub DeviceExtensionSize: ULONG,
    pub SpecificLuExtensionSize: ULONG,
    pub SrbExtensionSize: ULONG,
    /// 64-bit DMA support flags
    pub Dma64BitAddresses: UCHAR,
    /// Supports SRB_FUNCTION_RESET_DEVICE
    pub ResetTargetSupported: BOOLEAN,
    /// Maximum number of logical units per target
    pub MaximumNumberOfLogicalUnits: UCHAR,
    /// WMI data provider
    pub WmiDataProvider: BOOLEAN,
    // Note: StorPort adds more fields after this point for newer versions
}

pub type PPORT_CONFIGURATION_INFORMATION = *mut PORT_CONFIGURATION_INFORMATION;

/// Access Range structure for MMIO/IO port resources
#[repr(C)]
#[derive(Copy, Clone)]
pub struct ACCESS_RANGE {
    /// Physical start address (PHYSICAL_ADDRESS)
    pub RangeStart: ULONGLONG,
    /// Length in bytes
    pub RangeLength: ULONG,
    /// TRUE if memory-mapped, FALSE if IO port
    pub RangeInMemory: BOOLEAN,
}

// =============================================================================
// StorPort Notification Types
// =============================================================================

pub const REQUEST_COMPLETE: ULONG = 0;
pub const NEXT_REQUEST: ULONG = 1;
pub const NEXT_LU_REQUEST: ULONG = 2;
pub const RESET_DETECTED: ULONG = 3;
pub const BUS_CHANGE_DETECTED: ULONG = 4;
pub const REQUEST_TIMER_CALL: ULONG = 5;

// =============================================================================
// SCSI Request Block (SRB)
// =============================================================================
//
// Reference: WinDDK 7600.16385.1/inc/ddk/srb.h (lines 462-500)
// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/srb/ns-srb-_scsi_request_block
//

/// SCSI_REQUEST_BLOCK structure
///
/// The SCSI_REQUEST_BLOCK structure is used to communicate SCSI requests
/// between class drivers and port drivers.
///
/// Binary compatible with Windows NT 6.1 (Win7) x64 layout.
#[repr(C)]
pub struct SCSI_REQUEST_BLOCK {
    pub Length: USHORT,                     // offset 0x00
    pub Function: UCHAR,                    // offset 0x02
    pub SrbStatus: UCHAR,                   // offset 0x03
    pub ScsiStatus: UCHAR,                  // offset 0x04
    pub PathId: UCHAR,                      // offset 0x05
    pub TargetId: UCHAR,                    // offset 0x06
    pub Lun: UCHAR,                         // offset 0x07
    pub QueueTag: UCHAR,                    // offset 0x08
    pub QueueAction: UCHAR,                 // offset 0x09
    pub CdbLength: UCHAR,                   // offset 0x0A
    pub SenseInfoBufferLength: UCHAR,       // offset 0x0B
    pub SrbFlags: ULONG,                    // offset 0x0C
    pub DataTransferLength: ULONG,          // offset 0x10
    pub TimeOutValue: ULONG,                // offset 0x14
    pub DataBuffer: PVOID,                  // offset 0x18
    pub SenseInfoBuffer: PVOID,             // offset 0x20 (x64)
    pub NextSrb: *mut SCSI_REQUEST_BLOCK,   // offset 0x28 (x64)
    pub OriginalRequest: PVOID,             // offset 0x30 (x64)
    pub SrbExtension: PVOID,                // offset 0x38 (x64)
    pub InternalStatus: ULONG,              // offset 0x40 (x64) - union with QueueSortKey, LinkTimeoutValue
    pub Reserved: ULONG,                    // offset 0x44 (x64) - WIN64 only, for PVOID alignment
    pub Cdb: [UCHAR; 16],                   // offset 0x48 (x64)
}

pub type PSCSI_REQUEST_BLOCK = *mut SCSI_REQUEST_BLOCK;

// SRB Functions
pub const SRB_FUNCTION_EXECUTE_SCSI: UCHAR = 0x00;
pub const SRB_FUNCTION_CLAIM_DEVICE: UCHAR = 0x01;
pub const SRB_FUNCTION_IO_CONTROL: UCHAR = 0x02;
pub const SRB_FUNCTION_RECEIVE_EVENT: UCHAR = 0x03;
pub const SRB_FUNCTION_RELEASE_QUEUE: UCHAR = 0x04;
pub const SRB_FUNCTION_ATTACH_DEVICE: UCHAR = 0x05;
pub const SRB_FUNCTION_RELEASE_DEVICE: UCHAR = 0x06;
pub const SRB_FUNCTION_SHUTDOWN: UCHAR = 0x07;
pub const SRB_FUNCTION_FLUSH: UCHAR = 0x08;
pub const SRB_FUNCTION_ABORT_COMMAND: UCHAR = 0x10;
pub const SRB_FUNCTION_RELEASE_RECOVERY: UCHAR = 0x11;
pub const SRB_FUNCTION_RESET_BUS: UCHAR = 0x12;
pub const SRB_FUNCTION_RESET_DEVICE: UCHAR = 0x13;
pub const SRB_FUNCTION_TERMINATE_IO: UCHAR = 0x14;
pub const SRB_FUNCTION_FLUSH_QUEUE: UCHAR = 0x15;
pub const SRB_FUNCTION_REMOVE_DEVICE: UCHAR = 0x16;
pub const SRB_FUNCTION_WMI: UCHAR = 0x17;
pub const SRB_FUNCTION_LOCK_QUEUE: UCHAR = 0x18;
pub const SRB_FUNCTION_UNLOCK_QUEUE: UCHAR = 0x19;
pub const SRB_FUNCTION_RESET_LOGICAL_UNIT: UCHAR = 0x20;
pub const SRB_FUNCTION_SET_LINK_TIMEOUT: UCHAR = 0x21;
pub const SRB_FUNCTION_LINK_TIMEOUT_OCCURRED: UCHAR = 0x22;
pub const SRB_FUNCTION_LINK_TIMEOUT_COMPLETE: UCHAR = 0x23;
pub const SRB_FUNCTION_PNP: UCHAR = 0x25;
pub const SRB_FUNCTION_POWER: UCHAR = 0x24;

// SRB Status
pub const SRB_STATUS_PENDING: UCHAR = 0x00;
pub const SRB_STATUS_SUCCESS: UCHAR = 0x01;
pub const SRB_STATUS_ABORTED: UCHAR = 0x02;
pub const SRB_STATUS_ABORT_FAILED: UCHAR = 0x03;
pub const SRB_STATUS_ERROR: UCHAR = 0x04;
pub const SRB_STATUS_BUSY: UCHAR = 0x05;
pub const SRB_STATUS_INVALID_REQUEST: UCHAR = 0x06;
pub const SRB_STATUS_INVALID_PATH_ID: UCHAR = 0x07;
pub const SRB_STATUS_NO_DEVICE: UCHAR = 0x08;
pub const SRB_STATUS_TIMEOUT: UCHAR = 0x09;
pub const SRB_STATUS_SELECTION_TIMEOUT: UCHAR = 0x0A;
pub const SRB_STATUS_COMMAND_TIMEOUT: UCHAR = 0x0B;
pub const SRB_STATUS_MESSAGE_REJECTED: UCHAR = 0x0D;
pub const SRB_STATUS_BUS_RESET: UCHAR = 0x0E;
pub const SRB_STATUS_PARITY_ERROR: UCHAR = 0x0F;
pub const SRB_STATUS_REQUEST_SENSE_FAILED: UCHAR = 0x10;
pub const SRB_STATUS_NO_HBA: UCHAR = 0x11;
pub const SRB_STATUS_DATA_OVERRUN: UCHAR = 0x12;
pub const SRB_STATUS_UNEXPECTED_BUS_FREE: UCHAR = 0x13;
pub const SRB_STATUS_PHASE_SEQUENCE_FAILURE: UCHAR = 0x14;
pub const SRB_STATUS_BAD_SRB_BLOCK_LENGTH: UCHAR = 0x15;
pub const SRB_STATUS_REQUEST_FLUSHED: UCHAR = 0x16;
pub const SRB_STATUS_INVALID_LUN: UCHAR = 0x20;
pub const SRB_STATUS_INVALID_TARGET_ID: UCHAR = 0x21;
pub const SRB_STATUS_BAD_FUNCTION: UCHAR = 0x22;
pub const SRB_STATUS_ERROR_RECOVERY: UCHAR = 0x23;
pub const SRB_STATUS_NOT_POWERED: UCHAR = 0x24;
pub const SRB_STATUS_LINK_DOWN: UCHAR = 0x25;

// SRB Flags
pub const SRB_FLAGS_QUEUE_ACTION_ENABLE: ULONG = 0x00000002;
pub const SRB_FLAGS_DISABLE_DISCONNECT: ULONG = 0x00000004;
pub const SRB_FLAGS_DISABLE_SYNCH_TRANSFER: ULONG = 0x00000008;
pub const SRB_FLAGS_BYPASS_FROZEN_QUEUE: ULONG = 0x00000010;
pub const SRB_FLAGS_DISABLE_AUTOSENSE: ULONG = 0x00000020;
pub const SRB_FLAGS_DATA_IN: ULONG = 0x00000040;
pub const SRB_FLAGS_DATA_OUT: ULONG = 0x00000080;
pub const SRB_FLAGS_NO_DATA_TRANSFER: ULONG = 0x00000000;
pub const SRB_FLAGS_UNSPECIFIED_DIRECTION: ULONG = SRB_FLAGS_DATA_IN | SRB_FLAGS_DATA_OUT;
pub const SRB_FLAGS_NO_QUEUE_FREEZE: ULONG = 0x00000100;
pub const SRB_FLAGS_ADAPTER_CACHE_ENABLE: ULONG = 0x00000200;
pub const SRB_FLAGS_FREE_SENSE_BUFFER: ULONG = 0x00000400;
pub const SRB_FLAGS_IS_ACTIVE: ULONG = 0x00010000;
pub const SRB_FLAGS_ALLOCATED_FROM_ZONE: ULONG = 0x00020000;
pub const SRB_FLAGS_SGLIST_FROM_POOL: ULONG = 0x00040000;
pub const SRB_FLAGS_BYPASS_LOCKED_QUEUE: ULONG = 0x00080000;
pub const SRB_FLAGS_NO_KEEP_AWAKE: ULONG = 0x00100000;
pub const SRB_FLAGS_PORT_DRIVER_ALLOCSENSE: ULONG = 0x00200000;
pub const SRB_FLAGS_PORT_DRIVER_SENSEHASPORT: ULONG = 0x00400000;
pub const SRB_FLAGS_DONT_START_NEXT_PACKET: ULONG = 0x00800000;
pub const SRB_FLAGS_PORT_DRIVER_RESERVED: ULONG = 0x0F000000;
pub const SRB_FLAGS_CLASS_DRIVER_RESERVED: ULONG = 0xF0000000;

// =============================================================================
// SCSI Command Descriptor Block (CDB) Operation Codes
// =============================================================================

pub const SCSIOP_TEST_UNIT_READY: UCHAR = 0x00;
pub const SCSIOP_REZERO_UNIT: UCHAR = 0x01;
pub const SCSIOP_REWIND: UCHAR = 0x01;
pub const SCSIOP_REQUEST_BLOCK_ADDR: UCHAR = 0x02;
pub const SCSIOP_REQUEST_SENSE: UCHAR = 0x03;
pub const SCSIOP_FORMAT_UNIT: UCHAR = 0x04;
pub const SCSIOP_READ_BLOCK_LIMITS: UCHAR = 0x05;
pub const SCSIOP_REASSIGN_BLOCKS: UCHAR = 0x07;
pub const SCSIOP_INIT_ELEMENT_STATUS: UCHAR = 0x07;
pub const SCSIOP_READ6: UCHAR = 0x08;
pub const SCSIOP_RECEIVE: UCHAR = 0x08;
pub const SCSIOP_WRITE6: UCHAR = 0x0A;
pub const SCSIOP_PRINT: UCHAR = 0x0A;
pub const SCSIOP_SEND: UCHAR = 0x0A;
pub const SCSIOP_SEEK6: UCHAR = 0x0B;
pub const SCSIOP_TRACK_SELECT: UCHAR = 0x0B;
pub const SCSIOP_SLEW_PRINT: UCHAR = 0x0B;
pub const SCSIOP_SET_CAPACITY: UCHAR = 0x0B;
pub const SCSIOP_SEEK_BLOCK: UCHAR = 0x0C;
pub const SCSIOP_PARTITION: UCHAR = 0x0D;
pub const SCSIOP_READ_REVERSE: UCHAR = 0x0F;
pub const SCSIOP_WRITE_FILEMARKS: UCHAR = 0x10;
pub const SCSIOP_FLUSH_BUFFER: UCHAR = 0x10;
pub const SCSIOP_SPACE: UCHAR = 0x11;
pub const SCSIOP_INQUIRY: UCHAR = 0x12;
pub const SCSIOP_VERIFY6: UCHAR = 0x13;
pub const SCSIOP_RECOVER_BUF_DATA: UCHAR = 0x14;
pub const SCSIOP_MODE_SELECT: UCHAR = 0x15;
pub const SCSIOP_RESERVE_UNIT: UCHAR = 0x16;
pub const SCSIOP_RELEASE_UNIT: UCHAR = 0x17;
pub const SCSIOP_COPY: UCHAR = 0x18;
pub const SCSIOP_ERASE: UCHAR = 0x19;
pub const SCSIOP_MODE_SENSE: UCHAR = 0x1A;
pub const SCSIOP_START_STOP_UNIT: UCHAR = 0x1B;
pub const SCSIOP_STOP_PRINT: UCHAR = 0x1B;
pub const SCSIOP_LOAD_UNLOAD: UCHAR = 0x1B;
pub const SCSIOP_RECEIVE_DIAGNOSTIC: UCHAR = 0x1C;
pub const SCSIOP_SEND_DIAGNOSTIC: UCHAR = 0x1D;
pub const SCSIOP_MEDIUM_REMOVAL: UCHAR = 0x1E;
pub const SCSIOP_READ_FORMATTED_CAPACITY: UCHAR = 0x23;
pub const SCSIOP_READ_CAPACITY: UCHAR = 0x25;
pub const SCSIOP_READ: UCHAR = 0x28;
pub const SCSIOP_READ10: UCHAR = 0x28;
pub const SCSIOP_WRITE: UCHAR = 0x2A;
pub const SCSIOP_WRITE10: UCHAR = 0x2A;
pub const SCSIOP_SEEK: UCHAR = 0x2B;
pub const SCSIOP_LOCATE: UCHAR = 0x2B;
pub const SCSIOP_POSITION_TO_ELEMENT: UCHAR = 0x2B;
pub const SCSIOP_WRITE_VERIFY: UCHAR = 0x2E;
pub const SCSIOP_VERIFY: UCHAR = 0x2F;
pub const SCSIOP_SEARCH_DATA_HIGH: UCHAR = 0x30;
pub const SCSIOP_SEARCH_DATA_EQUAL: UCHAR = 0x31;
pub const SCSIOP_SEARCH_DATA_LOW: UCHAR = 0x32;
pub const SCSIOP_SET_LIMITS: UCHAR = 0x33;
pub const SCSIOP_READ_POSITION: UCHAR = 0x34;
pub const SCSIOP_SYNCHRONIZE_CACHE: UCHAR = 0x35;
pub const SCSIOP_COMPARE: UCHAR = 0x39;
pub const SCSIOP_COPY_COMPARE: UCHAR = 0x3A;
pub const SCSIOP_WRITE_DATA_BUFF: UCHAR = 0x3B;
pub const SCSIOP_READ_DATA_BUFF: UCHAR = 0x3C;
pub const SCSIOP_WRITE_LONG: UCHAR = 0x3F;
pub const SCSIOP_CHANGE_DEFINITION: UCHAR = 0x40;
pub const SCSIOP_WRITE_SAME: UCHAR = 0x41;
pub const SCSIOP_READ_SUB_CHANNEL: UCHAR = 0x42;
pub const SCSIOP_UNMAP: UCHAR = 0x42;
pub const SCSIOP_READ_TOC: UCHAR = 0x43;
pub const SCSIOP_READ_HEADER: UCHAR = 0x44;
pub const SCSIOP_REPORT_DENSITY_SUPPORT: UCHAR = 0x44;
pub const SCSIOP_PLAY_AUDIO: UCHAR = 0x45;
pub const SCSIOP_GET_CONFIGURATION: UCHAR = 0x46;
pub const SCSIOP_PLAY_AUDIO_MSF: UCHAR = 0x47;
pub const SCSIOP_PLAY_TRACK_INDEX: UCHAR = 0x48;
pub const SCSIOP_SANITIZE: UCHAR = 0x48;
pub const SCSIOP_PLAY_TRACK_RELATIVE: UCHAR = 0x49;
pub const SCSIOP_GET_EVENT_STATUS: UCHAR = 0x4A;
pub const SCSIOP_PAUSE_RESUME: UCHAR = 0x4B;
pub const SCSIOP_LOG_SELECT: UCHAR = 0x4C;
pub const SCSIOP_LOG_SENSE: UCHAR = 0x4D;
pub const SCSIOP_STOP_PLAY_SCAN: UCHAR = 0x4E;
pub const SCSIOP_XDWRITE: UCHAR = 0x50;
pub const SCSIOP_XPWRITE: UCHAR = 0x51;
pub const SCSIOP_READ_DISC_INFORMATION: UCHAR = 0x51;
pub const SCSIOP_READ_TRACK_INFORMATION: UCHAR = 0x52;
pub const SCSIOP_XDWRITE_READ: UCHAR = 0x53;
pub const SCSIOP_RESERVE_TRACK_RZONE: UCHAR = 0x53;
pub const SCSIOP_SEND_OPC_INFORMATION: UCHAR = 0x54;
pub const SCSIOP_MODE_SELECT10: UCHAR = 0x55;
pub const SCSIOP_RESERVE_UNIT10: UCHAR = 0x56;
pub const SCSIOP_RESERVE_ELEMENT: UCHAR = 0x56;
pub const SCSIOP_RELEASE_UNIT10: UCHAR = 0x57;
pub const SCSIOP_RELEASE_ELEMENT: UCHAR = 0x57;
pub const SCSIOP_REPAIR_TRACK: UCHAR = 0x58;
pub const SCSIOP_MODE_SENSE10: UCHAR = 0x5A;
pub const SCSIOP_CLOSE_TRACK_SESSION: UCHAR = 0x5B;
pub const SCSIOP_READ_BUFFER_CAPACITY: UCHAR = 0x5C;
pub const SCSIOP_SEND_CUE_SHEET: UCHAR = 0x5D;
pub const SCSIOP_PERSISTENT_RESERVE_IN: UCHAR = 0x5E;
pub const SCSIOP_PERSISTENT_RESERVE_OUT: UCHAR = 0x5F;
pub const SCSIOP_XDWRITE_EXTENDED16: UCHAR = 0x80;
pub const SCSIOP_WRITE_FILEMARKS16: UCHAR = 0x80;
pub const SCSIOP_REBUILD16: UCHAR = 0x81;
pub const SCSIOP_READ_REVERSE16: UCHAR = 0x81;
pub const SCSIOP_REGENERATE16: UCHAR = 0x82;
pub const SCSIOP_EXTENDED_COPY: UCHAR = 0x83;
pub const SCSIOP_POPULATE_TOKEN: UCHAR = 0x83;
pub const SCSIOP_WRITE_USING_TOKEN: UCHAR = 0x83;
pub const SCSIOP_RECEIVE_COPY_RESULTS: UCHAR = 0x84;
pub const SCSIOP_RECEIVE_ROD_TOKEN_INFORMATION: UCHAR = 0x84;
pub const SCSIOP_ATA_PASSTHROUGH16: UCHAR = 0x85;
pub const SCSIOP_ACCESS_CONTROL_IN: UCHAR = 0x86;
pub const SCSIOP_ACCESS_CONTROL_OUT: UCHAR = 0x87;
pub const SCSIOP_READ16: UCHAR = 0x88;
pub const SCSIOP_COMPARE_AND_WRITE: UCHAR = 0x89;
pub const SCSIOP_WRITE16: UCHAR = 0x8A;
pub const SCSIOP_READ_ATTRIBUTES: UCHAR = 0x8C;
pub const SCSIOP_WRITE_ATTRIBUTES: UCHAR = 0x8D;
pub const SCSIOP_WRITE_VERIFY16: UCHAR = 0x8E;
pub const SCSIOP_VERIFY16: UCHAR = 0x8F;
pub const SCSIOP_PREFETCH16: UCHAR = 0x90;
pub const SCSIOP_SYNCHRONIZE_CACHE16: UCHAR = 0x91;
pub const SCSIOP_SPACE16: UCHAR = 0x91;
pub const SCSIOP_LOCK_UNLOCK_CACHE16: UCHAR = 0x92;
pub const SCSIOP_LOCATE16: UCHAR = 0x92;
pub const SCSIOP_WRITE_SAME16: UCHAR = 0x93;
pub const SCSIOP_ERASE16: UCHAR = 0x93;
pub const SCSIOP_ZBC_OUT: UCHAR = 0x94;
pub const SCSIOP_ZBC_IN: UCHAR = 0x95;
pub const SCSIOP_READ_DATA_BUFF16: UCHAR = 0x9B;
pub const SCSIOP_READ_CAPACITY16: UCHAR = 0x9E;
pub const SCSIOP_GET_LBA_STATUS: UCHAR = 0x9E;
pub const SCSIOP_GET_PHYSICAL_ELEMENT_STATUS: UCHAR = 0x9E;
pub const SCSIOP_REMOVE_ELEMENT_AND_TRUNCATE: UCHAR = 0x9E;
pub const SCSIOP_SERVICE_ACTION_IN16: UCHAR = 0x9E;
pub const SCSIOP_SERVICE_ACTION_OUT16: UCHAR = 0x9F;
pub const SCSIOP_REPORT_LUNS: UCHAR = 0xA0;
pub const SCSIOP_BLANK: UCHAR = 0xA1;
pub const SCSIOP_ATA_PASSTHROUGH12: UCHAR = 0xA1;
pub const SCSIOP_SEND_EVENT: UCHAR = 0xA2;
pub const SCSIOP_SECURITY_PROTOCOL_IN: UCHAR = 0xA2;
pub const SCSIOP_SEND_KEY: UCHAR = 0xA3;
pub const SCSIOP_MAINTENANCE_IN: UCHAR = 0xA3;
pub const SCSIOP_REPORT_KEY: UCHAR = 0xA4;
pub const SCSIOP_MAINTENANCE_OUT: UCHAR = 0xA4;
pub const SCSIOP_MOVE_MEDIUM: UCHAR = 0xA5;
pub const SCSIOP_LOAD_UNLOAD_SLOT: UCHAR = 0xA6;
pub const SCSIOP_EXCHANGE_MEDIUM: UCHAR = 0xA6;
pub const SCSIOP_SET_READ_AHEAD: UCHAR = 0xA7;
pub const SCSIOP_MOVE_MEDIUM_ATTACHED: UCHAR = 0xA7;
pub const SCSIOP_READ12: UCHAR = 0xA8;
pub const SCSIOP_GET_MESSAGE: UCHAR = 0xA8;
pub const SCSIOP_SERVICE_ACTION_OUT12: UCHAR = 0xA9;
pub const SCSIOP_WRITE12: UCHAR = 0xAA;
pub const SCSIOP_SEND_MESSAGE: UCHAR = 0xAB;
pub const SCSIOP_SERVICE_ACTION_IN12: UCHAR = 0xAB;
pub const SCSIOP_GET_PERFORMANCE: UCHAR = 0xAC;
pub const SCSIOP_READ_DVD_STRUCTURE: UCHAR = 0xAD;
pub const SCSIOP_WRITE_VERIFY12: UCHAR = 0xAE;
pub const SCSIOP_VERIFY12: UCHAR = 0xAF;
pub const SCSIOP_SEARCH_DATA_HIGH12: UCHAR = 0xB0;
pub const SCSIOP_SEARCH_DATA_EQUAL12: UCHAR = 0xB1;
pub const SCSIOP_SEARCH_DATA_LOW12: UCHAR = 0xB2;
pub const SCSIOP_SET_LIMITS12: UCHAR = 0xB3;
pub const SCSIOP_READ_ELEMENT_STATUS_ATTACHED: UCHAR = 0xB4;
pub const SCSIOP_REQUEST_VOL_ELEMENT: UCHAR = 0xB5;
pub const SCSIOP_SECURITY_PROTOCOL_OUT: UCHAR = 0xB5;
pub const SCSIOP_SEND_VOLUME_TAG: UCHAR = 0xB6;
pub const SCSIOP_SET_STREAMING: UCHAR = 0xB6;
pub const SCSIOP_READ_DEFECT_DATA: UCHAR = 0xB7;
pub const SCSIOP_READ_ELEMENT_STATUS: UCHAR = 0xB8;
pub const SCSIOP_READ_CD_MSF: UCHAR = 0xB9;
pub const SCSIOP_SCAN_CD: UCHAR = 0xBA;
pub const SCSIOP_REDUNDANCY_GROUP_IN: UCHAR = 0xBA;
pub const SCSIOP_SET_CD_SPEED: UCHAR = 0xBB;
pub const SCSIOP_REDUNDANCY_GROUP_OUT: UCHAR = 0xBB;
pub const SCSIOP_PLAY_CD: UCHAR = 0xBC;
pub const SCSIOP_SPARE_IN: UCHAR = 0xBC;
pub const SCSIOP_MECHANISM_STATUS: UCHAR = 0xBD;
pub const SCSIOP_SPARE_OUT: UCHAR = 0xBD;
pub const SCSIOP_READ_CD: UCHAR = 0xBE;
pub const SCSIOP_VOLUME_SET_IN: UCHAR = 0xBE;
pub const SCSIOP_SEND_DVD_STRUCTURE: UCHAR = 0xBF;
pub const SCSIOP_VOLUME_SET_OUT: UCHAR = 0xBF;
pub const SCSIOP_INIT_ELEMENT_RANGE: UCHAR = 0xE7;

// =============================================================================
// SCSI Inquiry Data
// =============================================================================
//
// Reference: WinDDK 7600.16385.1/inc/ddk/scsi.h (lines 2193-2234)
// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/scsi/ns-scsi-_inquirydata
//

pub const INQUIRYDATABUFFERSIZE: usize = 36;

/// INQUIRYDATA structure
///
/// The INQUIRYDATA structure is used in conjunction with the TapeMiniGetMediaParameters
/// and TapeMiniGetDriveParameters routines to report drive and media parameters.
///
/// Binary compatible with Windows NT 6.1 (Win7) layout.
/// Note: Original structure uses bit fields, represented here as byte fields.
#[repr(C, packed)]
pub struct INQUIRYDATA {
    /// Byte 0: DeviceType (bits 0-4), DeviceTypeQualifier (bits 5-7)
    pub DeviceType: UCHAR,
    /// Byte 1: DeviceTypeModifier (bits 0-6), RemovableMedia (bit 7)
    pub DeviceTypeModifier: UCHAR,
    /// Byte 2: Versions (ANSIVersion bits 0-2, ECMAVersion bits 3-5, ISOVersion bits 6-7)
    pub Versions: UCHAR,
    /// Byte 3: ResponseDataFormat (bits 0-3), HiSupport (bit 4), NormACA (bit 5), TerminateTask (bit 6), AERC (bit 7)
    pub ResponseDataFormat: UCHAR,
    /// Byte 4: AdditionalLength (n-4, where n is total length)
    pub AdditionalLength: UCHAR,
    /// Byte 5: Reserved
    pub Reserved: UCHAR,
    /// Byte 6: Addr16, Addr32, AckReqQ, MediumChanger, MultiPort, ReservedBit2, EnclosureServices, ReservedBit3
    pub Reserved2: UCHAR,
    /// Byte 7: SoftReset, CommandQueue, TransferDisable, LinkedCommands, Synchronous, Wide16Bit, Wide32Bit, RelativeAddressing
    pub CommandQueue: UCHAR,
    /// Bytes 8-15: VendorId (8 bytes, space padded ASCII)
    pub VendorId: [UCHAR; 8],
    /// Bytes 16-31: ProductId (16 bytes, space padded ASCII)
    pub ProductId: [UCHAR; 16],
    /// Bytes 32-35: ProductRevisionLevel (4 bytes ASCII)
    pub ProductRevisionLevel: [UCHAR; 4],
    // Bytes 36-55: VendorSpecific[20] - optional
    // Bytes 56-95: Reserved3[40] - optional
}

// Device types from INQUIRY response
pub const DIRECT_ACCESS_DEVICE: UCHAR = 0x00;
pub const SEQUENTIAL_ACCESS_DEVICE: UCHAR = 0x01;
pub const PRINTER_DEVICE: UCHAR = 0x02;
pub const PROCESSOR_DEVICE: UCHAR = 0x03;
pub const WRITE_ONCE_READ_MULTIPLE_DEVICE: UCHAR = 0x04;
pub const READ_ONLY_DIRECT_ACCESS_DEVICE: UCHAR = 0x05;
pub const SCANNER_DEVICE: UCHAR = 0x06;
pub const OPTICAL_DEVICE: UCHAR = 0x07;
pub const MEDIUM_CHANGER: UCHAR = 0x08;
pub const COMMUNICATION_DEVICE: UCHAR = 0x09;
pub const ARRAY_CONTROLLER_DEVICE: UCHAR = 0x0C;
pub const SCSI_ENCLOSURE_DEVICE: UCHAR = 0x0D;
pub const REDUCED_BLOCK_COMMANDS: UCHAR = 0x0E;
pub const OPTICAL_CARD_READER_WRITER_DEVICE: UCHAR = 0x0F;
pub const BRIDGE_CONTROLLER_DEVICE: UCHAR = 0x10;
pub const OBJECT_BASED_STORAGE_DEVICE: UCHAR = 0x11;
pub const HOST_MANAGED_ZONED_BLOCK_DEVICE: UCHAR = 0x14;
pub const UNKNOWN_OR_NO_DEVICE: UCHAR = 0x1F;
pub const LOGICAL_UNIT_NOT_PRESENT: UCHAR = 0x7F;

// Peripheral qualifier values (bits 5-7 of device_type)
pub const DEVICE_QUALIFIER_ACTIVE: UCHAR = 0x00;
pub const DEVICE_QUALIFIER_NOT_ACTIVE: UCHAR = 0x01;
pub const DEVICE_NOT_CAPABLE: UCHAR = 0x03;

// =============================================================================
// Scatter/Gather DMA Structures
// =============================================================================
//
// Reference: WinDDK 7600.16385.1/inc/ddk/storport.h
// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/storport/ns-storport-_stor_scatter_gather_element
//

/// Physical address type for DMA operations
pub type STOR_PHYSICAL_ADDRESS = ULONGLONG;

/// Single scatter/gather element describing a physically contiguous memory region
///
/// Used by StorPort miniport drivers to describe DMA transfer segments.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct STOR_SCATTER_GATHER_ELEMENT {
    /// Physical address of the memory region
    pub PhysicalAddress: STOR_PHYSICAL_ADDRESS,
    /// Length of the memory region in bytes
    pub Length: ULONG,
    /// Reserved, must be zero
    pub Reserved: ULONG_PTR,
}

/// Scatter/gather list for DMA transfers
///
/// Contains an array of STOR_SCATTER_GATHER_ELEMENT entries describing
/// the physical memory layout of a transfer buffer.
#[repr(C)]
pub struct STOR_SCATTER_GATHER_LIST {
    /// Number of elements in the scatter/gather list
    pub NumberOfElements: ULONG,
    /// Reserved
    pub Reserved: ULONG_PTR,
    /// First element (array extends beyond this)
    pub List: [STOR_SCATTER_GATHER_ELEMENT; 1],
}

pub type PSTOR_SCATTER_GATHER_LIST = *mut STOR_SCATTER_GATHER_LIST;

impl STOR_SCATTER_GATHER_LIST {
    /// Calculate the size of a scatter/gather list with the given number of elements
    #[inline]
    pub const fn size_for_elements(count: usize) -> usize {
        // Base structure size + additional elements beyond the first
        core::mem::size_of::<STOR_SCATTER_GATHER_LIST>() +
            (count.saturating_sub(1)) * core::mem::size_of::<STOR_SCATTER_GATHER_ELEMENT>()
    }
}

// Maximum number of scatter/gather elements per request
// NT6.1 default is typically 17 (64KB / 4KB pages + 1 for misalignment)
pub const STOR_MAX_SG_ELEMENTS: usize = 33;

// =============================================================================
// Device Relations
// =============================================================================

#[repr(C)]
pub struct DEVICE_RELATIONS {
    pub count: ULONG,
    pub objects: [PDEVICE_OBJECT; 1],
}

pub type PDEVICE_RELATIONS = *mut DEVICE_RELATIONS;

