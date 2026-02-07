//! Test Boot Driver
//!
//! Тестовый boot-драйвер для XenOS.
//! Используется для проверки механизма загрузки boot drivers и I/O Manager.
//!
//! # Тест Фазы D
//!
//! Драйвер:
//! 1. Загружается в память winload
//! 2. IAT fixup резолвит импорты из ntoskrnl
//! 3. DriverEntry вызывается ядром
//! 4. Создаёт устройство \Device\XenTest через IoCreateDevice
//! 5. Устанавливает dispatch-обработчики IRP_MJ_CREATE/CLOSE/READ/WRITE
//! 6. Поддерживает buffered I/O для чтения/записи данных

#![no_std]
#![no_main]
#![feature(lang_items)]
#![allow(internal_features)]
#![allow(non_snake_case)]
#![allow(non_camel_case_types)]

use core::panic::PanicInfo;

mod imports;

use imports::ntoskrnl::{IoCompleteRequest, IoCreateDevice, IoDeleteDevice};

// =============================================================================
// NT API типы
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

pub const STATUS_SUCCESS: NTSTATUS = 0;
pub const STATUS_BUFFER_TOO_SMALL: NTSTATUS = 0xC0000023_u32 as i32;
pub const FILE_DEVICE_UNKNOWN: ULONG = 0x00000022;

// Device flags
pub const DO_BUFFERED_IO: ULONG = 0x00000004;

// IRP Major Function codes
pub const IRP_MJ_CREATE: UCHAR = 0;
pub const IRP_MJ_CLOSE: UCHAR = 2;
pub const IRP_MJ_READ: UCHAR = 3;
pub const IRP_MJ_WRITE: UCHAR = 4;
pub const IRP_MJ_CLEANUP: UCHAR = 0x12;
pub const IRP_MJ_DEVICE_CONTROL: UCHAR = 14;

// IO_PRIORITY_BOOST
pub const IO_NO_INCREMENT: CCHAR = 0;

#[repr(C)]
pub struct UNICODE_STRING {
    pub length: u16,
    pub maximum_length: u16,
    pub buffer: *const u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct LIST_ENTRY {
    pub flink: *mut LIST_ENTRY,
    pub blink: *mut LIST_ENTRY,
}

#[repr(C)]
pub struct IO_STATUS_BLOCK {
    pub status: NTSTATUS,
    pub information: ULONG_PTR,
}

// =============================================================================
// IRP структуры (Standard Windows NT 6.1 layout)
// =============================================================================

/// AssociatedIrp union в IRP
#[repr(C)]
pub union IrpAssociatedIrp {
    pub master_irp: *mut IRP,
    pub irp_count: i32,
    pub system_buffer: PVOID,
}

/// Overlay union в IRP
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

/// Tail union в IRP
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

/// IRP — I/O Request Packet (Standard Windows NT 6.1 layout)
#[repr(C)]
pub struct IRP {
    pub r#type: CSHORT,
    pub size: USHORT,
    pub mdl_address: PVOID,
    pub flags: ULONG,
    pub associated_irp: IrpAssociatedIrp,      // AssociatedIrp union
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
    pub overlay: IrpOverlay,                   // Overlay union
    pub cancel_routine: PVOID,
    pub user_buffer: PVOID,
    pub tail: IrpTail,                         // Tail union
}

/// IO_STACK_LOCATION — позиция в стеке IRP
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

/// Parameters union для IO_STACK_LOCATION
#[repr(C)]
pub union IoParameters {
    pub read: ReadParameters,
    pub write: WriteParameters,
    pub raw: [u64; 4],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ReadParameters {
    pub length: ULONG,
    pub key: ULONG,
    pub byte_offset: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct WriteParameters {
    pub length: ULONG,
    pub key: ULONG,
    pub byte_offset: u64,
}

pub type PIRP = *mut IRP;
pub type PIO_STACK_LOCATION = *mut IO_STACK_LOCATION;

// =============================================================================
// Driver/Device Objects
// =============================================================================

pub type PDRIVER_DISPATCH = Option<unsafe extern "win64" fn(PDEVICE_OBJECT, PIRP) -> NTSTATUS>;

#[repr(C)]
pub struct DRIVER_OBJECT {
    pub r#type: i16,
    pub size: i16,
    pub device_object: PVOID,
    pub flags: ULONG,
    pub driver_start: PVOID,
    pub driver_size: ULONG,
    pub driver_section: PVOID,
    pub driver_extension: PVOID,
    pub driver_name: UNICODE_STRING,
    pub hardware_database: PVOID,
    pub fast_io_dispatch: PVOID,
    pub driver_init: PVOID,
    pub driver_start_io: PVOID,
    pub driver_unload: Option<unsafe extern "win64" fn(*mut DRIVER_OBJECT)>,
    pub major_function: [PDRIVER_DISPATCH; 28],
}

#[repr(C)]
pub struct DEVICE_OBJECT {
    pub r#type: CSHORT,
    pub size: USHORT,
    pub reference_count: i32,
    pub driver_object: *mut DRIVER_OBJECT,
    pub next_device: *mut DEVICE_OBJECT,
    pub attached_device: *mut DEVICE_OBJECT,
    pub current_irp: PIRP,
    pub timer: PVOID,
    pub flags: ULONG,
    pub characteristics: ULONG,
    pub vpb: PVOID,
    pub device_extension: PVOID,
    pub device_type: ULONG,
    pub stack_size: CCHAR,
    // ... остальные поля
}

pub type PDRIVER_OBJECT = *mut DRIVER_OBJECT;
pub type PDEVICE_OBJECT = *mut DEVICE_OBJECT;

// =============================================================================
// Буфер данных устройства
// =============================================================================

/// Размер буфера для хранения данных
const DEVICE_BUFFER_SIZE: usize = 256;

/// Буфер данных устройства (статический для простоты)
static mut DEVICE_BUFFER: [u8; DEVICE_BUFFER_SIZE] = [0u8; DEVICE_BUFFER_SIZE];
static mut DEVICE_BUFFER_LENGTH: usize = 0;

/// Глобальный указатель на device object (для DriverUnload)
static mut GLOBAL_DEVICE_OBJECT: PDEVICE_OBJECT = core::ptr::null_mut();

// =============================================================================
// Имя устройства
// =============================================================================

static DEVICE_NAME_BUFFER: [u16; 7] = [
    'X' as u16, 'e' as u16, 'n' as u16, 'T' as u16,
    'e' as u16, 's' as u16, 't' as u16,
];

// =============================================================================
// Dispatch функции
// =============================================================================

/// DispatchCreate — обработчик IRP_MJ_CREATE
unsafe extern "win64" fn DispatchCreate(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    (*irp).io_status.status = STATUS_SUCCESS;
    (*irp).io_status.information = 0;
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    STATUS_SUCCESS
}

/// DispatchClose — обработчик IRP_MJ_CLOSE
unsafe extern "win64" fn DispatchClose(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    (*irp).io_status.status = STATUS_SUCCESS;
    (*irp).io_status.information = 0;
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    STATUS_SUCCESS
}

/// DispatchCleanup — обработчик IRP_MJ_CLEANUP
///
/// Вызывается при закрытии последнего handle процесса.
/// Драйвер должен отменить все pending IRP для этого файла.
unsafe extern "win64" fn DispatchCleanup(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    // Для тестового драйвера просто завершаем успешно
    // В реальном драйвере здесь отменяются pending IRP
    (*irp).io_status.status = STATUS_SUCCESS;
    (*irp).io_status.information = 0;
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    STATUS_SUCCESS
}

/// DispatchRead — обработчик IRP_MJ_READ (buffered I/O)
///
/// Читает данные из внутреннего буфера устройства.
unsafe extern "win64" fn DispatchRead(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    // Получаем stack location (через Tail.Overlay.CurrentStackLocation)
    let stack = (*irp).tail.overlay.current_stack_location;
    let length = (*stack).parameters.read.length as usize;
    
    // Проверяем system buffer (buffered I/O через AssociatedIrp.SystemBuffer)
    let system_buffer = (*irp).associated_irp.system_buffer;
    if system_buffer.is_null() {
        (*irp).io_status.status = STATUS_BUFFER_TOO_SMALL;
        (*irp).io_status.information = 0;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_BUFFER_TOO_SMALL;
    }
    
    // Определяем сколько данных можно прочитать
    let available = unsafe { core::ptr::read_volatile(&raw const DEVICE_BUFFER_LENGTH) };
    let to_read = if length < available { length } else { available };
    
    // Копируем данные в system buffer
    if to_read > 0 {
        unsafe {
            core::ptr::copy_nonoverlapping(
                (&raw const DEVICE_BUFFER) as *const u8,
                system_buffer as *mut u8,
                to_read,
            );
        }
    }
    
    (*irp).io_status.status = STATUS_SUCCESS;
    (*irp).io_status.information = to_read;
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    STATUS_SUCCESS
}

/// DispatchWrite — обработчик IRP_MJ_WRITE (buffered I/O)
///
/// Записывает данные во внутренний буфер устройства.
unsafe extern "win64" fn DispatchWrite(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    // Получаем stack location (через Tail.Overlay.CurrentStackLocation)
    let stack = (*irp).tail.overlay.current_stack_location;
    let length = (*stack).parameters.write.length as usize;
    
    // Проверяем system buffer (buffered I/O через AssociatedIrp.SystemBuffer)
    let system_buffer = (*irp).associated_irp.system_buffer;
    if system_buffer.is_null() {
        (*irp).io_status.status = STATUS_BUFFER_TOO_SMALL;
        (*irp).io_status.information = 0;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_BUFFER_TOO_SMALL;
    }
    
    // Ограничиваем размер записи буфером
    let to_write = if length > DEVICE_BUFFER_SIZE { DEVICE_BUFFER_SIZE } else { length };
    
    // Копируем данные из system buffer
    if to_write > 0 {
        unsafe {
            core::ptr::copy_nonoverlapping(
                system_buffer as *const u8,
                (&raw mut DEVICE_BUFFER) as *mut u8,
                to_write,
            );
            core::ptr::write_volatile(&raw mut DEVICE_BUFFER_LENGTH, to_write);
        }
    }
    
    (*irp).io_status.status = STATUS_SUCCESS;
    (*irp).io_status.information = to_write;
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    STATUS_SUCCESS
}

// =============================================================================
// DriverEntry
// =============================================================================

#[unsafe(no_mangle)]
pub extern "win64" fn DriverEntry(
    driver_object: PDRIVER_OBJECT,
    _registry_path: *const UNICODE_STRING,
) -> NTSTATUS {
    let device_name = UNICODE_STRING {
        length: (DEVICE_NAME_BUFFER.len() * 2) as u16,
        maximum_length: (DEVICE_NAME_BUFFER.len() * 2) as u16,
        buffer: DEVICE_NAME_BUFFER.as_ptr(),
    };
    
    let mut device_object: PDEVICE_OBJECT = core::ptr::null_mut();
    
    let status = unsafe {
        IoCreateDevice(
            driver_object,
            0,
            &device_name,
            FILE_DEVICE_UNKNOWN,
            0,
            0,
            &mut device_object,
        )
    };
    
    if status != STATUS_SUCCESS {
        return status;
    }
    
    // Устанавливаем флаг buffered I/O
    unsafe {
        (*device_object).flags |= DO_BUFFERED_IO;
    }
    
    // Сохраняем device object для DriverUnload
    unsafe {
        core::ptr::write_volatile(&raw mut GLOBAL_DEVICE_OBJECT, device_object);
    }
    
    // Устанавливаем dispatch функции
    unsafe {
        (*driver_object).major_function[IRP_MJ_CREATE as usize] = Some(DispatchCreate);
        (*driver_object).major_function[IRP_MJ_CLOSE as usize] = Some(DispatchClose);
        (*driver_object).major_function[IRP_MJ_CLEANUP as usize] = Some(DispatchCleanup);
        (*driver_object).major_function[IRP_MJ_READ as usize] = Some(DispatchRead);
        (*driver_object).major_function[IRP_MJ_WRITE as usize] = Some(DispatchWrite);
        (*driver_object).driver_unload = Some(DriverUnload);
    }
    
    STATUS_SUCCESS
}

unsafe extern "win64" fn DriverUnload(_driver_object: *mut DRIVER_OBJECT) {
    let device = core::ptr::read_volatile(&raw const GLOBAL_DEVICE_OBJECT);
    if !device.is_null() {
        IoDeleteDevice(device);
        core::ptr::write_volatile(&raw mut GLOBAL_DEVICE_OBJECT, core::ptr::null_mut());
    }
}

// =============================================================================
// Panic handler & lang items
// =============================================================================

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

#[lang = "eh_personality"]
extern "C" fn eh_personality() {}

// Заглушка для compiler_builtins floating point
#[unsafe(no_mangle)]
pub static _fltused: i32 = 0;
