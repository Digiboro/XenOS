//! Базовые типы I/O Manager
//!
//! Источники:
//! - ReactOS: sdk/include/xdk/iotypes.h

use crate::nt::NTSTATUS;
use crate::nt::PVOID;
use crate::nt::ntdef::UNICODE_STRING;

// =============================================================================
// IO Object Types
// =============================================================================

/// Типы объектов I/O подсистемы
pub const IO_TYPE_ADAPTER: u16 = 1;
pub const IO_TYPE_CONTROLLER: u16 = 2;
pub const IO_TYPE_DEVICE: u16 = 3;
pub const IO_TYPE_DRIVER: u16 = 4;
pub const IO_TYPE_FILE: u16 = 5;
pub const IO_TYPE_IRP: u16 = 6;
pub const IO_TYPE_MASTER_ADAPTER: u16 = 7;
pub const IO_TYPE_OPEN_PACKET: u16 = 8;
pub const IO_TYPE_TIMER: u16 = 9;
pub const IO_TYPE_VPB: u16 = 10;
pub const IO_TYPE_ERROR_LOG: u16 = 11;
pub const IO_TYPE_ERROR_MESSAGE: u16 = 12;
pub const IO_TYPE_DEVICE_OBJECT_EXTENSION: u16 = 13;

// =============================================================================
// Device Types (FILE_DEVICE_*)
// =============================================================================

pub const FILE_DEVICE_BEEP: u32 = 0x00000001;
pub const FILE_DEVICE_CD_ROM: u32 = 0x00000002;
pub const FILE_DEVICE_CD_ROM_FILE_SYSTEM: u32 = 0x00000003;
pub const FILE_DEVICE_CONTROLLER: u32 = 0x00000004;
pub const FILE_DEVICE_DATALINK: u32 = 0x00000005;
pub const FILE_DEVICE_DFS: u32 = 0x00000006;
pub const FILE_DEVICE_DISK: u32 = 0x00000007;
pub const FILE_DEVICE_DISK_FILE_SYSTEM: u32 = 0x00000008;
pub const FILE_DEVICE_FILE_SYSTEM: u32 = 0x00000009;
pub const FILE_DEVICE_INPORT_PORT: u32 = 0x0000000a;
pub const FILE_DEVICE_KEYBOARD: u32 = 0x0000000b;
pub const FILE_DEVICE_MAILSLOT: u32 = 0x0000000c;
pub const FILE_DEVICE_MIDI_IN: u32 = 0x0000000d;
pub const FILE_DEVICE_MIDI_OUT: u32 = 0x0000000e;
pub const FILE_DEVICE_MOUSE: u32 = 0x0000000f;
pub const FILE_DEVICE_MULTI_UNC_PROVIDER: u32 = 0x00000010;
pub const FILE_DEVICE_NAMED_PIPE: u32 = 0x00000011;
pub const FILE_DEVICE_NETWORK: u32 = 0x00000012;
pub const FILE_DEVICE_NETWORK_BROWSER: u32 = 0x00000013;
pub const FILE_DEVICE_NETWORK_FILE_SYSTEM: u32 = 0x00000014;
pub const FILE_DEVICE_NULL: u32 = 0x00000015;
pub const FILE_DEVICE_PARALLEL_PORT: u32 = 0x00000016;
pub const FILE_DEVICE_PHYSICAL_NETCARD: u32 = 0x00000017;
pub const FILE_DEVICE_PRINTER: u32 = 0x00000018;
pub const FILE_DEVICE_SCANNER: u32 = 0x00000019;
pub const FILE_DEVICE_SERIAL_MOUSE_PORT: u32 = 0x0000001a;
pub const FILE_DEVICE_SERIAL_PORT: u32 = 0x0000001b;
pub const FILE_DEVICE_SCREEN: u32 = 0x0000001c;
pub const FILE_DEVICE_SOUND: u32 = 0x0000001d;
pub const FILE_DEVICE_STREAMS: u32 = 0x0000001e;
pub const FILE_DEVICE_TAPE: u32 = 0x0000001f;
pub const FILE_DEVICE_TAPE_FILE_SYSTEM: u32 = 0x00000020;
pub const FILE_DEVICE_TRANSPORT: u32 = 0x00000021;
pub const FILE_DEVICE_UNKNOWN: u32 = 0x00000022;
pub const FILE_DEVICE_VIDEO: u32 = 0x00000023;
pub const FILE_DEVICE_VIRTUAL_DISK: u32 = 0x00000024;
pub const FILE_DEVICE_WAVE_IN: u32 = 0x00000025;
pub const FILE_DEVICE_WAVE_OUT: u32 = 0x00000026;
pub const FILE_DEVICE_8042_PORT: u32 = 0x00000027;
pub const FILE_DEVICE_NETWORK_REDIRECTOR: u32 = 0x00000028;
pub const FILE_DEVICE_BATTERY: u32 = 0x00000029;
pub const FILE_DEVICE_BUS_EXTENDER: u32 = 0x0000002a;
pub const FILE_DEVICE_MODEM: u32 = 0x0000002b;
pub const FILE_DEVICE_VDM: u32 = 0x0000002c;
pub const FILE_DEVICE_MASS_STORAGE: u32 = 0x0000002d;
pub const FILE_DEVICE_SMB: u32 = 0x0000002e;
pub const FILE_DEVICE_KS: u32 = 0x0000002f;
pub const FILE_DEVICE_CHANGER: u32 = 0x00000030;
pub const FILE_DEVICE_SMARTCARD: u32 = 0x00000031;
pub const FILE_DEVICE_ACPI: u32 = 0x00000032;
pub const FILE_DEVICE_DVD: u32 = 0x00000033;
pub const FILE_DEVICE_FULLSCREEN_VIDEO: u32 = 0x00000034;
pub const FILE_DEVICE_CONSOLE: u32 = 0x00000050;

// =============================================================================
// DEVICE_OBJECT.Flags (DO_*)
// =============================================================================

pub const DO_UNLOAD_PENDING: u32 = 0x00000001;
pub const DO_VERIFY_VOLUME: u32 = 0x00000002;
pub const DO_BUFFERED_IO: u32 = 0x00000004;
pub const DO_EXCLUSIVE: u32 = 0x00000008;
pub const DO_DIRECT_IO: u32 = 0x00000010;
pub const DO_MAP_IO_BUFFER: u32 = 0x00000020;
pub const DO_DEVICE_INITIALIZING: u32 = 0x00000080;
pub const DO_SHUTDOWN_REGISTERED: u32 = 0x00000800;
pub const DO_BUS_ENUMERATED_DEVICE: u32 = 0x00001000;
pub const DO_POWER_PAGABLE: u32 = 0x00002000;
pub const DO_POWER_INRUSH: u32 = 0x00004000;
pub const DO_DEVICE_TO_BE_RESET: u32 = 0x04000000;

// =============================================================================
// DEVICE_OBJECT.Characteristics
// =============================================================================

pub const FILE_REMOVABLE_MEDIA: u32 = 0x00000001;
pub const FILE_READ_ONLY_DEVICE: u32 = 0x00000002;
pub const FILE_FLOPPY_DISKETTE: u32 = 0x00000004;
pub const FILE_WRITE_ONCE_MEDIA: u32 = 0x00000008;
pub const FILE_REMOTE_DEVICE: u32 = 0x00000010;
pub const FILE_DEVICE_IS_MOUNTED: u32 = 0x00000020;
pub const FILE_VIRTUAL_VOLUME: u32 = 0x00000040;
pub const FILE_AUTOGENERATED_DEVICE_NAME: u32 = 0x00000080;
pub const FILE_DEVICE_SECURE_OPEN: u32 = 0x00000100;

// =============================================================================
// FILE_OBJECT.Flags (FO_*)
// =============================================================================

pub const FO_FILE_OPEN: u32 = 0x00000001;
pub const FO_SYNCHRONOUS_IO: u32 = 0x00000002;
pub const FO_ALERTABLE_IO: u32 = 0x00000004;
pub const FO_NO_INTERMEDIATE_BUFFERING: u32 = 0x00000008;
pub const FO_WRITE_THROUGH: u32 = 0x00000010;
pub const FO_SEQUENTIAL_ONLY: u32 = 0x00000020;
pub const FO_CACHE_SUPPORTED: u32 = 0x00000040;
pub const FO_NAMED_PIPE: u32 = 0x00000080;
pub const FO_STREAM_FILE: u32 = 0x00000100;
pub const FO_MAILSLOT: u32 = 0x00000200;
pub const FO_GENERATE_AUDIT_ON_CLOSE: u32 = 0x00000400;
pub const FO_QUEUE_IRP_TO_THREAD: u32 = 0x00000400;
pub const FO_DIRECT_DEVICE_OPEN: u32 = 0x00000800;
pub const FO_FILE_MODIFIED: u32 = 0x00001000;
pub const FO_FILE_SIZE_CHANGED: u32 = 0x00002000;
pub const FO_CLEANUP_COMPLETE: u32 = 0x00004000;
pub const FO_TEMPORARY_FILE: u32 = 0x00008000;
pub const FO_DELETE_ON_CLOSE: u32 = 0x00010000;
pub const FO_OPENED_CASE_SENSITIVE: u32 = 0x00020000;
pub const FO_HANDLE_CREATED: u32 = 0x00040000;
pub const FO_FILE_FAST_IO_READ: u32 = 0x00080000;
pub const FO_RANDOM_ACCESS: u32 = 0x00100000;
pub const FO_FILE_OPEN_CANCELLED: u32 = 0x00200000;
pub const FO_VOLUME_OPEN: u32 = 0x00400000;
pub const FO_REMOTE_ORIGIN: u32 = 0x01000000;

// =============================================================================
// IRP Major Function Codes (IRP_MJ_*)
// =============================================================================

pub const IRP_MJ_CREATE: u8 = 0x00;
pub const IRP_MJ_CREATE_NAMED_PIPE: u8 = 0x01;
pub const IRP_MJ_CLOSE: u8 = 0x02;
pub const IRP_MJ_READ: u8 = 0x03;
pub const IRP_MJ_WRITE: u8 = 0x04;
pub const IRP_MJ_QUERY_INFORMATION: u8 = 0x05;
pub const IRP_MJ_SET_INFORMATION: u8 = 0x06;
pub const IRP_MJ_QUERY_EA: u8 = 0x07;
pub const IRP_MJ_SET_EA: u8 = 0x08;
pub const IRP_MJ_FLUSH_BUFFERS: u8 = 0x09;
pub const IRP_MJ_QUERY_VOLUME_INFORMATION: u8 = 0x0a;
pub const IRP_MJ_SET_VOLUME_INFORMATION: u8 = 0x0b;
pub const IRP_MJ_DIRECTORY_CONTROL: u8 = 0x0c;
pub const IRP_MJ_FILE_SYSTEM_CONTROL: u8 = 0x0d;
pub const IRP_MJ_DEVICE_CONTROL: u8 = 0x0e;
pub const IRP_MJ_INTERNAL_DEVICE_CONTROL: u8 = 0x0f;
pub const IRP_MJ_SHUTDOWN: u8 = 0x10;

// IRP Minor Function Codes для IRP_MJ_FILE_SYSTEM_CONTROL
pub const IRP_MN_MOUNT_VOLUME: u8 = 0x01;
pub const IRP_MN_VERIFY_VOLUME: u8 = 0x02;
pub const IRP_MN_LOAD_FILE_SYSTEM: u8 = 0x03;
pub const IRP_MN_TRACK_LINK: u8 = 0x04;
pub const IRP_MN_USER_FS_REQUEST: u8 = 0x00;
pub const IRP_MJ_LOCK_CONTROL: u8 = 0x11;
pub const IRP_MJ_CLEANUP: u8 = 0x12;
pub const IRP_MJ_CREATE_MAILSLOT: u8 = 0x13;
pub const IRP_MJ_QUERY_SECURITY: u8 = 0x14;
pub const IRP_MJ_SET_SECURITY: u8 = 0x15;
pub const IRP_MJ_POWER: u8 = 0x16;
pub const IRP_MJ_SYSTEM_CONTROL: u8 = 0x17;
pub const IRP_MJ_DEVICE_CHANGE: u8 = 0x18;
pub const IRP_MJ_QUERY_QUOTA: u8 = 0x19;
pub const IRP_MJ_SET_QUOTA: u8 = 0x1a;
pub const IRP_MJ_PNP: u8 = 0x1b;
pub const IRP_MJ_PNP_POWER: u8 = IRP_MJ_PNP;
pub const IRP_MJ_MAXIMUM_FUNCTION: u8 = 0x1b;

// =============================================================================
// IRP Flags
// =============================================================================

pub const IRP_NOCACHE: u32 = 0x00000001;
pub const IRP_PAGING_IO: u32 = 0x00000002;
pub const IRP_MOUNT_COMPLETION: u32 = 0x00000002;
pub const IRP_SYNCHRONOUS_API: u32 = 0x00000004;
pub const IRP_ASSOCIATED_IRP: u32 = 0x00000008;
pub const IRP_BUFFERED_IO: u32 = 0x00000010;
pub const IRP_DEALLOCATE_BUFFER: u32 = 0x00000020;
pub const IRP_INPUT_OPERATION: u32 = 0x00000040;
pub const IRP_SYNCHRONOUS_PAGING_IO: u32 = 0x00000040;
pub const IRP_CREATE_OPERATION: u32 = 0x00000080;
pub const IRP_READ_OPERATION: u32 = 0x00000100;
pub const IRP_WRITE_OPERATION: u32 = 0x00000200;
pub const IRP_CLOSE_OPERATION: u32 = 0x00000400;
pub const IRP_DEFER_IO_COMPLETION: u32 = 0x00000800;
pub const IRP_OB_QUERY_NAME: u32 = 0x00001000;
pub const IRP_HOLD_DEVICE_QUEUE: u32 = 0x00002000;

// =============================================================================
// IO_STATUS_BLOCK
// =============================================================================

/// IO_STATUS_BLOCK - результат I/O операции
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct IO_STATUS_BLOCK {
    /// Статус операции или указатель (union)
    pub status: NTSTATUS,
    /// Количество переданных байт
    pub information: usize,
}

impl IO_STATUS_BLOCK {
    pub const fn new() -> Self {
        Self {
            status: 0,
            information: 0,
        }
    }
}

impl Default for IO_STATUS_BLOCK {
    fn default() -> Self {
        Self::new()
    }
}

pub type PIO_STATUS_BLOCK = *mut IO_STATUS_BLOCK;

// =============================================================================
// Forward declarations
// =============================================================================

pub type PDEVICE_OBJECT = *mut super::device::DEVICE_OBJECT;
pub type PDRIVER_OBJECT = *mut super::driver::DRIVER_OBJECT;
pub type PFILE_OBJECT = *mut super::file::FILE_OBJECT;
pub type PIRP = *mut super::irp::IRP;

// =============================================================================
// Driver dispatch function types
// =============================================================================

/// Тип функции-обработчика IRP
pub type PDRIVER_DISPATCH =
    Option<unsafe extern "win64" fn(device_object: PDEVICE_OBJECT, irp: PIRP) -> NTSTATUS>;

/// Тип функции инициализации драйвера (DriverEntry)
pub type PDRIVER_INITIALIZE = Option<
    unsafe extern "win64" fn(
        driver_object: PDRIVER_OBJECT,
        registry_path: *const UNICODE_STRING,
    ) -> NTSTATUS,
>;

/// Тип функции выгрузки драйвера
pub type PDRIVER_UNLOAD = Option<unsafe extern "win64" fn(driver_object: PDRIVER_OBJECT)>;

/// Тип функции StartIo
pub type PDRIVER_STARTIO =
    Option<unsafe extern "win64" fn(device_object: PDEVICE_OBJECT, irp: PIRP)>;

/// Тип функции AddDevice (PnP)
pub type PDRIVER_ADD_DEVICE = Option<
    unsafe extern "win64" fn(
        driver_object: PDRIVER_OBJECT,
        physical_device_object: PDEVICE_OBJECT,
    ) -> NTSTATUS,
>;

// =============================================================================
// IRP completion routine type
// =============================================================================

/// Тип функции завершения IRP
pub type PIO_COMPLETION_ROUTINE = Option<
    unsafe extern "win64" fn(device_object: PDEVICE_OBJECT, irp: PIRP, context: PVOID) -> NTSTATUS,
>;

// =============================================================================
// IOCTL helpers
// =============================================================================

/// Макрос для создания IOCTL кода
#[inline]
pub const fn ctl_code(device_type: u32, function: u32, method: u32, access: u32) -> u32 {
    (device_type << 16) | (access << 14) | (function << 2) | method
}

/// Методы буферизации для IOCTL
pub const METHOD_BUFFERED: u32 = 0;
pub const METHOD_IN_DIRECT: u32 = 1;
pub const METHOD_OUT_DIRECT: u32 = 2;
pub const METHOD_NEITHER: u32 = 3;

/// Права доступа для IOCTL
pub const FILE_ANY_ACCESS: u32 = 0;
pub const FILE_READ_ACCESS: u32 = 1;
pub const FILE_WRITE_ACCESS: u32 = 2;

/// Извлекает метод из IOCTL кода
#[inline]
pub const fn io_method_from_ctl_code(code: u32) -> u32 {
    code & 0x00000003
}

// =============================================================================
// INTERFACE_TYPE — тип интерфейса шины
// =============================================================================

/// INTERFACE_TYPE — тип интерфейса шины (bus interface)
///
/// Определяет тип шины, к которой подключено устройство.
#[repr(i32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum INTERFACE_TYPE {
    /// Тип не определён
    InterfaceTypeUndefined = -1,
    /// Internal bus
    Internal = 0,
    /// ISA bus
    Isa = 1,
    /// EISA bus
    Eisa = 2,
    /// MicroChannel bus
    MicroChannel = 3,
    /// TurboChannel bus
    TurboChannel = 4,
    /// PCI bus
    PCIBus = 5,
    /// VME bus
    VMEBus = 6,
    /// NuBus
    NuBus = 7,
    /// PCMCIA bus
    PCMCIABus = 8,
    /// C bus
    CBus = 9,
    /// MPI bus
    MPIBus = 10,
    /// MPSA bus
    MPSABus = 11,
    /// Processor internal
    ProcessorInternal = 12,
    /// Internal power bus
    InternalPowerBus = 13,
    /// PNP ISA bus
    PNPISABus = 14,
    /// PNP bus
    PNPBus = 15,
    /// Vmcs (virtualization)
    Vmcs = 16,
    /// ACPI bus
    ACPIBus = 17,
    /// Максимальное значение
    MaximumInterfaceType = 18,
}

impl Default for INTERFACE_TYPE {
    fn default() -> Self {
        Self::InterfaceTypeUndefined
    }
}
