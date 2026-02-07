//! PCI Bus Driver Types
//!
//! NT типы и структуры для PCI bus driver.

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
pub type USHORT_PTR = usize;

// =============================================================================
// NTSTATUS Codes
// =============================================================================

pub const STATUS_SUCCESS: NTSTATUS = 0;
pub const STATUS_NOT_SUPPORTED: NTSTATUS = 0xC00000BBu32 as i32;
pub const STATUS_INSUFFICIENT_RESOURCES: NTSTATUS = 0xC000009Au32 as i32;
pub const STATUS_NO_SUCH_DEVICE: NTSTATUS = 0xC000000Eu32 as i32;
pub const STATUS_INVALID_DEVICE_REQUEST: NTSTATUS = 0xC0000010u32 as i32;
pub const STATUS_INVALID_PARAMETER: NTSTATUS = 0xC000000Du32 as i32;
pub const STATUS_DEVICE_NOT_CONNECTED: NTSTATUS = 0xC000009Du32 as i32;

// =============================================================================
// Device Types
// =============================================================================

pub const FILE_DEVICE_BUS_EXTENDER: ULONG = 0x0000002A;

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
pub const IRP_MJ_PNP: UCHAR = 0x1B;
pub const IRP_MJ_POWER: UCHAR = 0x16;

// =============================================================================
// IRP Minor Function Codes - PnP
// =============================================================================

pub const IRP_MN_START_DEVICE: UCHAR = 0x00;
pub const IRP_MN_QUERY_REMOVE_DEVICE: UCHAR = 0x01;
pub const IRP_MN_REMOVE_DEVICE: UCHAR = 0x02;
pub const IRP_MN_CANCEL_REMOVE_DEVICE: UCHAR = 0x03;
pub const IRP_MN_STOP_DEVICE: UCHAR = 0x04;
pub const IRP_MN_QUERY_STOP_DEVICE: UCHAR = 0x05;
pub const IRP_MN_CANCEL_STOP_DEVICE: UCHAR = 0x06;
pub const IRP_MN_QUERY_DEVICE_RELATIONS: UCHAR = 0x07;
pub const IRP_MN_QUERY_INTERFACE: UCHAR = 0x08;
pub const IRP_MN_QUERY_CAPABILITIES: UCHAR = 0x09;
pub const IRP_MN_QUERY_RESOURCES: UCHAR = 0x0A;
pub const IRP_MN_QUERY_RESOURCE_REQUIREMENTS: UCHAR = 0x0B;
pub const IRP_MN_QUERY_DEVICE_TEXT: UCHAR = 0x0C;
pub const IRP_MN_FILTER_RESOURCE_REQUIREMENTS: UCHAR = 0x0D;
pub const IRP_MN_READ_CONFIG: UCHAR = 0x0F;
pub const IRP_MN_WRITE_CONFIG: UCHAR = 0x10;
pub const IRP_MN_EJECT: UCHAR = 0x11;
pub const IRP_MN_SET_LOCK: UCHAR = 0x12;
pub const IRP_MN_QUERY_ID: UCHAR = 0x13;
pub const IRP_MN_QUERY_PNP_DEVICE_STATE: UCHAR = 0x14;
pub const IRP_MN_QUERY_BUS_INFORMATION: UCHAR = 0x15;
pub const IRP_MN_DEVICE_USAGE_NOTIFICATION: UCHAR = 0x16;
pub const IRP_MN_SURPRISE_REMOVAL: UCHAR = 0x17;

// =============================================================================
// Device Relations Types
// =============================================================================

pub const BUS_RELATIONS: ULONG = 0;
pub const EJECTION_RELATIONS: ULONG = 1;
pub const POWER_RELATIONS: ULONG = 2;
pub const REMOVAL_RELATIONS: ULONG = 3;
pub const TARGET_DEVICE_RELATION: ULONG = 4;

// =============================================================================
// Bus Query ID Types
// =============================================================================

pub const BUS_QUERY_DEVICE_ID: ULONG = 0;
pub const BUS_QUERY_HARDWARE_IDS: ULONG = 1;
pub const BUS_QUERY_COMPATIBLE_IDS: ULONG = 2;
pub const BUS_QUERY_INSTANCE_ID: ULONG = 3;

// =============================================================================
// IO Constants
// =============================================================================

pub const IO_NO_INCREMENT: CCHAR = 0;

// =============================================================================
// UNICODE_STRING
// =============================================================================

#[repr(C)]
pub struct UNICODE_STRING {
    pub length: u16,
    pub maximum_length: u16,
    pub buffer: *const u16,
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

impl LIST_ENTRY {
    pub const fn new() -> Self {
        Self {
            flink: ptr::null_mut(),
            blink: ptr::null_mut(),
        }
    }
}

// =============================================================================
// IO_STATUS_BLOCK
// =============================================================================

#[repr(C)]
pub struct IO_STATUS_BLOCK {
    pub status: NTSTATUS,
    pub information: ULONG_PTR,
}

// =============================================================================
// IRP Structures
// =============================================================================

#[repr(C)]
pub union IrpAssociatedIrp {
    pub master_irp: *mut IRP,
    pub irp_count: i32,
    pub system_buffer: PVOID,
}

#[repr(C)]
pub union IrpOverlay {
    pub async_params: IrpAsyncParams,
    pub allocation_size: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct IrpAsyncParams {
    pub user_apc_routine: PVOID,
    pub user_apc_context: PVOID,
}

#[repr(C)]
pub union IrpTail {
    pub overlay: IrpTailOverlay,
    pub apc_reserved: [PVOID; 6],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct IrpTailOverlay {
    pub driver_context: [PVOID; 4],
    pub thread: PVOID,
    pub auxiliary_buffer: PVOID,
    pub list_entry: LIST_ENTRY,
    pub current_stack_location: PIO_STACK_LOCATION,
    pub original_file_object: PVOID,
}

#[repr(C)]
pub struct IRP {
    pub r#type: CSHORT,
    pub size: USHORT,
    pub mdl_address: PVOID,
    pub flags: ULONG,
    pub associated_irp: IrpAssociatedIrp,
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
    pub overlay: IrpOverlay,
    pub cancel_routine: PVOID,
    pub user_buffer: PVOID,
    pub tail: IrpTail,
}

pub type PIRP = *mut IRP;

// =============================================================================
// IO_STACK_LOCATION
// =============================================================================

#[repr(C)]
pub struct IO_STACK_LOCATION {
    pub major_function: UCHAR,
    pub minor_function: UCHAR,
    pub flags: UCHAR,
    pub control: UCHAR,
    pub parameters: IoParameters,
    pub device_object: PDEVICE_OBJECT,
    pub file_object: PVOID,
    pub completion_routine: PVOID,
    pub context: PVOID,
}

#[repr(C)]
pub union IoParameters {
    pub query_device_relations: QueryDeviceRelationsParams,
    pub query_id: QueryIdParams,
    pub start_device: StartDeviceParams,
    pub raw: [u64; 4],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct QueryDeviceRelationsParams {
    pub r#type: ULONG, // DEVICE_RELATION_TYPE
    pub _reserved: [ULONG; 3],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct QueryIdParams {
    pub id_type: ULONG, // BUS_QUERY_ID_TYPE
    pub _reserved: [ULONG; 3],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct StartDeviceParams {
    pub allocated_resources: PVOID,           // PCM_RESOURCE_LIST
    pub allocated_resources_translated: PVOID, // PCM_RESOURCE_LIST
}

pub type PIO_STACK_LOCATION = *mut IO_STACK_LOCATION;

// =============================================================================
// DEVICE_RELATIONS
// =============================================================================

#[repr(C)]
pub struct DEVICE_RELATIONS {
    pub count: ULONG,
    pub objects: [PDEVICE_OBJECT; 1], // Variable length
}

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
// Размер для x64: ~240-280 bytes (зависит от inline structures)
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
    pub queue: LIST_ENTRY, // union { LIST_ENTRY ListEntry; WAIT_CONTEXT_BLOCK Wcb; }
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

// Вспомогательные структуры (должны соответствовать ntoskrnl)

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
    pub lock: ULONG_PTR, // AtomicUsize в реальности
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

#[repr(C)]
pub struct KEVENT {
    pub header: DISPATCHER_HEADER,
}

#[repr(C)]
pub struct DISPATCHER_HEADER {
    pub r#type: UCHAR,
    pub reserved: [UCHAR; 3],
    pub signal_state: i32, // LONG (AtomicI32 в реальности)
    pub wait_list_head: LIST_ENTRY,
}

pub type PDRIVER_OBJECT = *mut DRIVER_OBJECT;
pub type PDEVICE_OBJECT = *mut DEVICE_OBJECT;

// =============================================================================
// PCI Device Extensions
// =============================================================================

/// Common extension header for both FDO and PDO
#[repr(C)]
pub struct PCI_COMMON_EXTENSION {
    /// 1 if FDO, 0 if PDO
    pub is_fdo: u8,
    pub _reserved: [u8; 7],
    /// Back pointer to self
    pub self_device: PDEVICE_OBJECT,
}

/// FDO Extension for PCI bus
#[repr(C)]
pub struct PCI_FDO_EXTENSION {
    pub common: PCI_COMMON_EXTENSION,
    /// Physical Device Object (ACPI PDO)
    pub physical_device_object: PDEVICE_OBJECT,
    /// Lower device in stack
    pub lower_device: PDEVICE_OBJECT,
    /// Bus number for this host bridge
    pub bus_number: u8,
    /// Segment group
    pub segment: u16,
    /// List of child PDOs
    pub child_list: LIST_ENTRY,
    /// Number of children
    pub child_count: u32,
    /// Device is started
    pub started: u8,
}

/// Информация о PCI BAR (для хранения в extension)
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PCI_BAR_INFO {
    /// Базовый адрес (программируемый)
    pub base_address: u64,
    /// Размер BAR
    pub size: u64,
    /// Индекс BAR (0-5)
    pub index: u8,
    /// Memory BAR (иначе I/O)
    pub is_memory: u8,
    /// 64-bit BAR
    pub is_64bit: u8,
    /// Prefetchable
    pub is_prefetchable: u8,
    /// BAR был назначен (имеет ресурс)
    pub assigned: u8,
    pub _pad: [u8; 3],
}

impl Default for PCI_BAR_INFO {
    fn default() -> Self {
        Self {
            base_address: 0,
            size: 0,
            index: 0,
            is_memory: 0,
            is_64bit: 0,
            is_prefetchable: 0,
            assigned: 0,
            _pad: [0; 3],
        }
    }
}

/// PDO Extension for PCI device
#[repr(C)]
pub struct PCI_PDO_EXTENSION {
    pub common: PCI_COMMON_EXTENSION,
    /// Parent FDO
    pub parent_fdo: PDEVICE_OBJECT,
    /// Link in parent's child list
    pub list_entry: LIST_ENTRY,
    /// PCI location
    pub bus_number: u8,
    pub device_number: u8,
    pub function_number: u8,
    pub _pad: u8,
    /// PCI IDs
    pub vendor_id: u16,
    pub device_id: u16,
    pub subsystem_vendor_id: u16,
    pub subsystem_id: u16,
    /// Class codes
    pub base_class: u8,
    pub sub_class: u8,
    pub programming_interface: u8,
    pub revision_id: u8,
    /// Header type
    pub header_type: u8,
    /// Device present
    pub present: u8,
    /// Device reported in BusRelations
    pub reported: u8,
    /// Device started
    pub started: u8,
    /// Interrupt line
    pub interrupt_line: u8,
    /// Interrupt pin (0 = none, 1 = INTA, etc.)
    pub interrupt_pin: u8,
    pub _pad2: [u8; 2],
    /// BAR information (6 BARs max for Type 0)
    pub bars: [PCI_BAR_INFO; 6],
    /// Number of valid BARs
    pub num_bars: u8,
}

// =============================================================================
// Imports (PE/IAT) — единое место: src/imports/*
// =============================================================================

pub use crate::imports::ntoskrnl::*;

// =============================================================================
// Pool Types
// =============================================================================

pub const NON_PAGED_POOL: ULONG = 0;
pub const PAGED_POOL: ULONG = 1;

// =============================================================================
// Resource Structures (для IRP_MN_QUERY_RESOURCE_REQUIREMENTS и START_DEVICE)
// =============================================================================

/// Тип интерфейса шины
pub const INTERFACE_TYPE_PCI: i32 = 5;

/// Типы ресурсов
pub const CM_RESOURCE_TYPE_NULL: u8 = 0;
pub const CM_RESOURCE_TYPE_PORT: u8 = 1;
pub const CM_RESOURCE_TYPE_INTERRUPT: u8 = 2;
pub const CM_RESOURCE_TYPE_MEMORY: u8 = 3;
pub const CM_RESOURCE_TYPE_DMA: u8 = 4;
pub const CM_RESOURCE_TYPE_BUS_NUMBER: u8 = 6;

/// Resource flags - Memory
pub const CM_RESOURCE_MEMORY_READ_WRITE: u16 = 0x0000;
pub const CM_RESOURCE_MEMORY_PREFETCHABLE: u16 = 0x0004;
pub const CM_RESOURCE_MEMORY_BAR: u16 = 0x0080;

/// Resource flags - Port
pub const CM_RESOURCE_PORT_IO: u16 = 0x0001;
pub const CM_RESOURCE_PORT_BAR: u16 = 0x0100;

/// Resource flags - Interrupt
pub const CM_RESOURCE_INTERRUPT_LEVEL_SENSITIVE: u16 = 0x0000;
pub const CM_RESOURCE_INTERRUPT_LATCHED: u16 = 0x0001;

/// IO Resource option flags
pub const IO_RESOURCE_PREFERRED: u8 = 0x01;
pub const IO_RESOURCE_ALTERNATIVE: u8 = 0x08;

/// Port requirement
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IO_PORT_REQUIREMENT {
    pub minimum_address: u64,
    pub maximum_address: u64,
    pub alignment: ULONG,
    pub length: ULONG,
}

/// Memory requirement  
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IO_MEMORY_REQUIREMENT {
    pub minimum_address: u64,
    pub maximum_address: u64,
    pub alignment: ULONG,
    pub length: ULONG,
}

/// Interrupt requirement
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IO_INTERRUPT_REQUIREMENT {
    pub minimum_vector: ULONG,
    pub maximum_vector: ULONG,
    pub affinity: usize,
}

/// Union для IO_RESOURCE_DESCRIPTOR
#[repr(C)]
#[derive(Clone, Copy)]
pub union IO_RESOURCE_UNION {
    pub port: IO_PORT_REQUIREMENT,
    pub memory: IO_MEMORY_REQUIREMENT,
    pub interrupt: IO_INTERRUPT_REQUIREMENT,
    pub raw: [u64; 4],
}

/// IO_RESOURCE_DESCRIPTOR
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IO_RESOURCE_DESCRIPTOR {
    pub option: u8,
    pub r#type: u8,
    pub share_disposition: u8,
    pub spare1: u8,
    pub flags: u16,
    pub spare2: u16,
    pub u: IO_RESOURCE_UNION,
}

/// IO_RESOURCE_LIST (один альтернативный набор)
#[repr(C)]
pub struct IO_RESOURCE_LIST {
    pub version: u16,
    pub revision: u16,
    pub count: ULONG,
    pub descriptors: [IO_RESOURCE_DESCRIPTOR; 1],
}

/// IO_RESOURCE_REQUIREMENTS_LIST
#[repr(C)]
pub struct IO_RESOURCE_REQUIREMENTS_LIST {
    pub list_size: ULONG,
    pub interface_type: i32,
    pub bus_number: ULONG,
    pub slot_number: ULONG,
    pub reserved: [ULONG; 3],
    pub alternative_lists: ULONG,
    pub list: [IO_RESOURCE_LIST; 1],
}

/// Port resource (allocated)
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CM_PORT_RESOURCE {
    pub start: u64,
    pub length: ULONG,
}

/// Memory resource (allocated)
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CM_MEMORY_RESOURCE {
    pub start: u64,
    pub length: ULONG,
}

/// Interrupt resource (allocated)
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CM_INTERRUPT_RESOURCE {
    pub level: ULONG,
    pub vector: ULONG,
    pub affinity: usize,
}

/// Union для CM_PARTIAL_RESOURCE_DESCRIPTOR
#[repr(C)]
#[derive(Clone, Copy)]
pub union CM_RESOURCE_UNION {
    pub port: CM_PORT_RESOURCE,
    pub memory: CM_MEMORY_RESOURCE,
    pub interrupt: CM_INTERRUPT_RESOURCE,
    pub raw: [ULONG; 3],
}

/// CM_PARTIAL_RESOURCE_DESCRIPTOR
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CM_PARTIAL_RESOURCE_DESCRIPTOR {
    pub r#type: u8,
    pub share_disposition: u8,
    pub flags: u16,
    pub u: CM_RESOURCE_UNION,
}

/// CM_PARTIAL_RESOURCE_LIST
#[repr(C)]
pub struct CM_PARTIAL_RESOURCE_LIST {
    pub version: u16,
    pub revision: u16,
    pub count: ULONG,
    pub partial_descriptors: [CM_PARTIAL_RESOURCE_DESCRIPTOR; 1],
}

/// CM_FULL_RESOURCE_DESCRIPTOR
#[repr(C)]
pub struct CM_FULL_RESOURCE_DESCRIPTOR {
    pub interface_type: i32,
    pub bus_number: ULONG,
    pub partial_resource_list: CM_PARTIAL_RESOURCE_LIST,
}

/// CM_RESOURCE_LIST
#[repr(C)]
pub struct CM_RESOURCE_LIST {
    pub count: ULONG,
    pub list: [CM_FULL_RESOURCE_DESCRIPTOR; 1],
}

// =============================================================================
// DEVICE_CAPABILITIES
// =============================================================================

/// DEVICE_CAPABILITIES — возможности устройства (PnP и Power)
///
/// Источник: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/ns-wdm-_device_capabilities
#[repr(C)]
pub struct DEVICE_CAPABILITIES {
    pub size: u16,
    pub version: u16,
    pub device_d1: u32,
    pub device_d2: u32,
    pub lock_supported: u32,
    pub eject_supported: u32,
    pub removable: u32,
    pub dock_device: u32,
    pub unique_id: u32,
    pub silent_install: u32,
    pub raw_device_ok: u32,
    pub surprise_removal_ok: u32,
    pub wake_from_d0: u32,
    pub wake_from_d1: u32,
    pub wake_from_d2: u32,
    pub wake_from_d3: u32,
    pub hardware_disabled: u32,
    pub non_dynamic: u32,
    pub warm_eject_supported: u32,
    pub no_display_in_ui: u32,
    pub reserved1: u32,
    pub wake_from_interrupt: u32,
    pub secure_device: u32,
    pub child_of_vga_enabled_bridge: u32,
    pub decode_io_on_boot: u32,
    pub reserved: u32,
    pub address: u32,
    pub ui_number: u32,
    pub device_state: [u32; 7],
    pub system_wake: u32,
    pub device_wake: u32,
    pub d1_latency: u32,
    pub d2_latency: u32,
    pub d3_latency: u32,
}

// =============================================================================
// Helper macros/functions
// =============================================================================

/// Pool tag для PCI allocations: 'PciX'
pub const PCI_POOL_TAG: ULONG = 0x58696350; // 'XicP' in little-endian

