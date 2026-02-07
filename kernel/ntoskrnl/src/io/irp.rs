//! IRP — I/O Request Packet
//!
//! IRP — основная структура для передачи запросов между драйверами.
//! Каждый I/O запрос (чтение, запись, IOCTL и т.д.) представлен пакетом IRP,
//! который передаётся по стеку устройств.
//!
//! # Архитектура
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                         IRP Flow                                    │
//! ├─────────────────────────────────────────────────────────────────────┤
//! │                                                                     │
//! │  User Request                                                       │
//! │       │                                                            │
//! │       ▼                                                            │
//! │  IoAllocateIrp(stack_size)                                         │
//! │       │                                                            │
//! │       ▼                                                            │
//! │  ┌───────────────────────────────────────┐                         │
//! │  │              IRP                       │                         │
//! │  ├───────────────────────────────────────┤                         │
//! │  │ type, size, flags                     │                         │
//! │  │ io_status, mdl_address                │                         │
//! │  │ current_location, stack_count         │                         │
//! │  │ current_stack_location ───────────────┼──┐                      │
//! │  └───────────────────────────────────────┘  │                      │
//! │                                              │                      │
//! │  ┌───────────────────────────────────────┐  │                      │
//! │  │      IO_STACK_LOCATION[0]             │◄─┘                      │
//! │  │ major_function (IRP_MJ_*)             │                         │
//! │  │ minor_function                        │                         │
//! │  │ parameters (union)                    │                         │
//! │  │ device_object, file_object            │                         │
//! │  │ completion_routine                    │                         │
//! │  ├───────────────────────────────────────┤                         │
//! │  │      IO_STACK_LOCATION[1]             │                         │
//! │  │              ...                       │                         │
//! │  └───────────────────────────────────────┘                         │
//! │                                                                     │
//! │  IoCallDriver() ──► dispatch ──► completion ──► IoCompleteRequest() │
//! │                                                                     │
//! └─────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Основные функции
//!
//! | Функция | Описание |
//! |---------|----------|
//! | `io_allocate_irp` | IoAllocateIrp — выделяет IRP |
//! | `io_free_irp` | IoFreeIrp — освобождает IRP |
//! | `io_call_driver` | IoCallDriver — передаёт IRP драйверу |
//! | `io_complete_request` | IoCompleteRequest — завершает IRP |
//! | `io_cancel_irp` | IoCancelIrp — отменяет IRP |
//! | `io_acquire_cancel_spin_lock` | IoAcquireCancelSpinLock — захват cancel spinlock |
//! | `io_set_cancel_routine` | IoSetCancelRoutine — установка cancel routine |
//! | `io_start_packet` | IoStartPacket — запуск обработки через StartIo |
//!
//! # Completion Semantics
//!
//! - `IoCompleteRequest` обходит стек снизу вверх
//! - `PendingReturned` распространяется вверх по стеку
//! - `STATUS_MORE_PROCESSING_REQUIRED` останавливает completion chain
//! - Финальная сигнализация `user_event` и запись в `user_iosb`
//!
//! # Отступления и упрощения
//!
//! - Lookaside списки для IRP — не реализованы (используется прямое выделение из пула)
//! - Associated IRP — не реализовано
//! - Quota tracking — не реализовано
//! - KDEVICE_QUEUE — упрощённая реализация в IoStartPacket
//! - APC completion — не реализовано (TODO)
//!
//! Источники:
//! - ReactOS: sdk/include/xdk/iotypes.h, ntoskrnl/io/iomgr/irp.c
//! - NT5: ntos/io/iomgr/irp.c

use super::types::*;
use crate::ke::event::KEVENT;
use crate::ke::thread::KTHREAD;
use crate::mm::mdl::MDL;
use crate::nt::CSHORT;
use crate::nt::LARGE_INTEGER;
use crate::nt::LIST_ENTRY;
use crate::nt::NTSTATUS;
use crate::nt::PVOID;
use crate::nt::UCHAR;
use crate::nt::ULONG;
use crate::nt::USHORT;
use crate::nt::ntstatus::*;

// =============================================================================
// IO_STACK_LOCATION
// =============================================================================

/// IO_STACK_LOCATION.Control flags
pub const SL_PENDING_RETURNED: u8 = 0x01;
pub const SL_ERROR_RETURNED: u8 = 0x02;
pub const SL_INVOKE_ON_CANCEL: u8 = 0x20;
pub const SL_INVOKE_ON_SUCCESS: u8 = 0x40;
pub const SL_INVOKE_ON_ERROR: u8 = 0x80;

/// Параметры для IRP_MJ_CREATE
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IO_STACK_CREATE {
    pub security_context: PVOID, // PIO_SECURITY_CONTEXT
    pub options: ULONG,
    pub file_attributes: USHORT,
    pub share_access: USHORT,
    pub ea_length: ULONG,
}

/// Параметры для IRP_MJ_READ / IRP_MJ_WRITE
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IO_STACK_READ_WRITE {
    pub length: ULONG,
    pub key: ULONG,
    pub byte_offset: LARGE_INTEGER,
}

/// Параметры для IRP_MJ_DEVICE_CONTROL
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IO_STACK_DEVICE_CONTROL {
    pub output_buffer_length: ULONG,
    pub input_buffer_length: ULONG,
    pub io_control_code: ULONG,
    pub type3_input_buffer: PVOID,
}

/// Параметры для IRP_MJ_QUERY_INFORMATION
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IO_STACK_QUERY_FILE {
    pub length: ULONG,
    pub file_information_class: ULONG,
}

/// Параметры для IRP_MJ_SET_INFORMATION
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IO_STACK_SET_FILE {
    pub length: ULONG,
    pub file_information_class: ULONG,
    pub file_object: PFILE_OBJECT,
    pub replace_if_exists: bool,
    pub advance_only: bool,
}

/// Параметры для IRP_MN_QUERY_DEVICE_RELATIONS
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IO_STACK_QUERY_DEVICE_RELATIONS {
    pub relation_type: super::pnp::DEVICE_RELATION_TYPE,
}

/// Параметры для IRP_MN_QUERY_ID
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IO_STACK_QUERY_ID {
    /// Тип ID: BUS_QUERY_DEVICE_ID (0), BUS_QUERY_HARDWARE_IDS (1), etc.
    pub id_type: ULONG,
}

/// Параметры для IRP_MN_START_DEVICE
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IO_STACK_START_DEVICE {
    pub allocated_resources: PVOID,      // PCM_RESOURCE_LIST
    pub allocated_resources_translated: PVOID, // PCM_RESOURCE_LIST
}

/// Параметры для IRP_MN_QUERY_CAPABILITIES
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IO_STACK_DEVICE_CAPABILITIES {
    pub capabilities: PVOID, // PDEVICE_CAPABILITIES
}

/// Параметры для IRP_MN_MOUNT_VOLUME
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IO_STACK_MOUNT_VOLUME {
    pub vpb: *mut super::vpb::VPB,
    pub device_object: PDEVICE_OBJECT,
}

/// Параметры для IRP_MN_VERIFY_VOLUME
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IO_STACK_VERIFY_VOLUME {
    pub vpb: *mut super::vpb::VPB,
    pub device_object: PDEVICE_OBJECT,
}

/// Параметры для IRP_MJ_POWER (IRP_MN_SET_POWER, IRP_MN_QUERY_POWER)
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IO_STACK_POWER {
    pub system_context: ULONG,
    pub power_type: super::pnp::POWER_STATE_TYPE,
    pub power_state: super::pnp::POWER_STATE,
    pub shutdown_type: super::pnp::POWER_ACTION,
}

/// Объединение параметров для разных типов IRP
#[repr(C)]
#[derive(Clone, Copy)]
pub union IO_STACK_PARAMETERS {
    pub create: IO_STACK_CREATE,
    pub read: IO_STACK_READ_WRITE,
    pub write: IO_STACK_READ_WRITE,
    pub device_io_control: IO_STACK_DEVICE_CONTROL,
    pub query_file: IO_STACK_QUERY_FILE,
    pub set_file: IO_STACK_SET_FILE,
    // PnP parameters
    pub query_device_relations: IO_STACK_QUERY_DEVICE_RELATIONS,
    pub query_id: IO_STACK_QUERY_ID,
    pub start_device: IO_STACK_START_DEVICE,
    pub device_capabilities: IO_STACK_DEVICE_CAPABILITIES,
    pub mount_volume: IO_STACK_MOUNT_VOLUME,
    pub verify_volume: IO_STACK_VERIFY_VOLUME,
    // Power parameters
    pub power: IO_STACK_POWER,
    pub others: [u8; 32], // Для других типов
}

impl Default for IO_STACK_PARAMETERS {
    fn default() -> Self {
        Self { others: [0; 32] }
    }
}

/// IO_STACK_LOCATION - элемент стека IRP
///
/// Каждый драйвер в стеке устройств имеет свой IO_STACK_LOCATION
/// для хранения параметров запроса.
#[repr(C)]
pub struct IO_STACK_LOCATION {
    /// Основной код функции (IRP_MJ_*)
    pub major_function: UCHAR,
    /// Дополнительный код функции (IRP_MN_*)
    pub minor_function: UCHAR,
    /// Флаги
    pub flags: UCHAR,
    /// Управляющие флаги (SL_*)
    pub control: UCHAR,
    /// Параметры запроса
    pub parameters: IO_STACK_PARAMETERS,
    /// Объект устройства для этого уровня стека
    pub device_object: PDEVICE_OBJECT,
    /// Объект файла
    pub file_object: PFILE_OBJECT,
    /// Функция завершения
    pub completion_routine: PIO_COMPLETION_ROUTINE,
    /// Контекст для функции завершения
    pub context: PVOID,
}

impl IO_STACK_LOCATION {
    pub const SIZE: usize = core::mem::size_of::<Self>();

    pub const fn new() -> Self {
        Self {
            major_function: 0,
            minor_function: 0,
            flags: 0,
            control: 0,
            parameters: IO_STACK_PARAMETERS { others: [0; 32] },
            device_object: core::ptr::null_mut(),
            file_object: core::ptr::null_mut(),
            completion_routine: None,
            context: core::ptr::null_mut(),
        }
    }
}

// =============================================================================
// IRP
// =============================================================================

/// IRP - I/O Request Packet
///
/// Основная структура для запросов ввода-вывода.
/// Передается от драйвера к драйверу по стеку устройств.
///
/// Layout соответствует Windows NT 6.1 (Win7) для совместимости с драйверами.
#[repr(C)]
pub struct IRP {
    /// Тип объекта (IO_TYPE_IRP)
    pub r#type: CSHORT,
    /// Размер IRP (включая stack locations)
    pub size: USHORT,
    /// MDL для буфера данных
    pub mdl_address: *mut MDL,
    /// Флаги IRP (IRP_*)
    pub flags: ULONG,
    /// AssociatedIrp union: SystemBuffer для buffered I/O
    pub associated_irp: IRP_ASSOCIATED_IRP,
    /// Связь с потоком
    pub thread_list_entry: LIST_ENTRY,
    /// Статус операции
    pub io_status: IO_STATUS_BLOCK,
    /// Режим вызывающего (UserMode/KernelMode)
    pub requestor_mode: i8,
    /// Был ли возвращен STATUS_PENDING
    pub pending_returned: bool,
    /// Количество stack locations
    pub stack_count: i8,
    /// Текущая позиция в стеке
    pub current_location: i8,
    /// Флаг отмены
    pub cancel: bool,
    /// IRQL при отмене
    pub cancel_irql: u8,
    /// APC environment
    pub apc_environment: i8,
    /// Флаги выделения
    pub allocation_flags: u8,
    /// User IOSB
    pub user_iosb: PIO_STATUS_BLOCK,
    /// User event
    pub user_event: *mut KEVENT,
    /// Overlay union: AllocationSize или AsynchronousParameters
    pub overlay: IRP_OVERLAY,
    /// Функция отмены
    pub cancel_routine: PVOID,
    /// User buffer (для neither I/O)
    pub user_buffer: PVOID,
    /// Tail union: содержит CurrentStackLocation и другие поля
    pub tail: IRP_TAIL,
}

/// AssociatedIrp union в IRP
#[repr(C)]
pub union IRP_ASSOCIATED_IRP {
    pub master_irp: *mut IRP,
    pub irp_count: i32,
    pub system_buffer: PVOID,
}

/// Overlay union в IRP
#[repr(C)]
pub union IRP_OVERLAY {
    pub asynchronous_parameters: IRP_ASYNC_PARAMS,
    pub allocation_size: LARGE_INTEGER,
}

/// Асинхронные параметры в Overlay
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IRP_ASYNC_PARAMS {
    pub user_apc_routine: PVOID,
    pub user_apc_context: PVOID,
}

/// Tail union в IRP
#[repr(C)]
pub union IRP_TAIL {
    pub overlay: IRP_TAIL_OVERLAY,
    pub apc_reserved: [PVOID; 6],
}

/// Overlay структура внутри Tail
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IRP_TAIL_OVERLAY {
    pub driver_context: [PVOID; 4],
    pub thread: *mut KTHREAD,
    pub auxiliary_buffer: *mut i8,
    pub list_entry: LIST_ENTRY,
    pub current_stack_location: *mut IO_STACK_LOCATION,
    pub original_file_object: PFILE_OBJECT,
}

impl IRP {
    /// Минимальный размер IRP (без stack locations)
    pub const BASE_SIZE: usize = core::mem::size_of::<Self>();
}

// =============================================================================
// IRP Functions
// =============================================================================

/// IoAllocateIrp - выделяет IRP
///
/// # Arguments
/// * `stack_size` - количество stack locations
/// * `charge_quota` - учитывать квоту процесса
///
/// # Returns
/// Указатель на IRP или NULL
pub unsafe fn io_allocate_irp(stack_size: i8, _charge_quota: bool) -> PIRP {
    unsafe {
        use crate::ex::pool::POOL_TYPE;
        use crate::ex::pool::ex_allocate_pool_with_tag;

        if stack_size <= 0 {
            return core::ptr::null_mut();
        }

        // Вычисляем размер
        let irp_size = IRP::BASE_SIZE + (stack_size as usize * IO_STACK_LOCATION::SIZE);

        // Выделяем память
        let irp = ex_allocate_pool_with_tag(
            POOL_TYPE::NonPagedPool,
            irp_size,
            u32::from_le_bytes(*b"Irp "),
        );

        if irp.is_null() {
            return core::ptr::null_mut();
        }

        // Обнуляем
        core::ptr::write_bytes(irp as *mut u8, 0, irp_size);

        let irp = irp as PIRP;

        // Инициализируем
        (*irp).r#type = IO_TYPE_IRP as CSHORT;
        (*irp).size = irp_size as USHORT;
        (*irp).stack_count = stack_size;
        (*irp).current_location = stack_size + 1;

        // Stack locations находятся сразу после IRP
        let stack_base = (irp as usize + IRP::BASE_SIZE) as *mut IO_STACK_LOCATION;

        // Текущий stack location изначально указывает "за" последний элемент
        // IoGetCurrentIrpStackLocation() уменьшит его
        (*irp).tail.overlay.current_stack_location = stack_base.add(stack_size as usize);

        // Отслеживаем выделение IRP
        super::iop::iop_track_irp_allocate();

        irp
    }
}

/// IoFreeIrp - освобождает IRP
pub unsafe fn io_free_irp(irp: PIRP) {
    use crate::ex::pool::ex_free_pool_with_tag;

    if irp.is_null() {
        return;
    }

    // Отслеживаем освобождение IRP
    super::iop::iop_track_irp_free();

    ex_free_pool_with_tag(irp as PVOID, u32::from_le_bytes(*b"Irp "));
}

/// IoInitializeIrp - инициализирует уже выделенный IRP
pub unsafe fn io_initialize_irp(irp: PIRP, packet_size: USHORT, stack_size: i8) {
    unsafe {
        if irp.is_null() {
            return;
        }

        // Обнуляем (кроме allocation_flags если IRP был из lookaside)
        let alloc_flags = (*irp).allocation_flags;
        core::ptr::write_bytes(irp as *mut u8, 0, packet_size as usize);
        (*irp).allocation_flags = alloc_flags;

        // Инициализируем
        (*irp).r#type = IO_TYPE_IRP as CSHORT;
        (*irp).size = packet_size;
        (*irp).stack_count = stack_size;
        (*irp).current_location = stack_size + 1;

        let stack_base = (irp as usize + IRP::BASE_SIZE) as *mut IO_STACK_LOCATION;
        (*irp).tail.overlay.current_stack_location = stack_base.add(stack_size as usize);
    }
}

/// IoGetCurrentIrpStackLocation - возвращает текущий stack location
///
/// В NT: IoGetCurrentIrpStackLocation(Irp) = Irp->Tail.Overlay.CurrentStackLocation
#[inline]
pub unsafe fn io_get_current_irp_stack_location(irp: PIRP) -> *mut IO_STACK_LOCATION {
    unsafe { (*irp).tail.overlay.current_stack_location }
}

/// IoGetNextIrpStackLocation - возвращает следующий stack location
///
/// В NT: IoGetNextIrpStackLocation(Irp) = Irp->Tail.Overlay.CurrentStackLocation - 1
#[inline]
pub unsafe fn io_get_next_irp_stack_location(irp: PIRP) -> *mut IO_STACK_LOCATION {
    unsafe {
        // Проверка bounds для предотвращения overflow
        if (*irp).current_location <= 1 {
            // Уже на минимальной позиции stack
            panic!("IoGetNextIrpStackLocation: no more stack locations (current_location={})", (*irp).current_location);
        }
        (*irp).tail.overlay.current_stack_location.sub(1)
    }
}

/// IoSetNextIrpStackLocation - переходит к следующему stack location
#[inline]
pub unsafe fn io_set_next_irp_stack_location(irp: PIRP) {
    unsafe {
        (*irp).current_location -= 1;
        (*irp).tail.overlay.current_stack_location = (*irp).tail.overlay.current_stack_location.sub(1);
    }
}

/// IoSkipCurrentIrpStackLocation - пропускает текущий stack location
#[inline]
pub unsafe fn io_skip_current_irp_stack_location(irp: PIRP) {
    unsafe {
        (*irp).current_location += 1;
        (*irp).tail.overlay.current_stack_location = (*irp).tail.overlay.current_stack_location.add(1);
    }
}

/// IoCopyCurrentIrpStackLocationToNext - копирует текущий stack location в следующий
pub unsafe fn io_copy_current_irp_stack_location_to_next(irp: PIRP) {
    unsafe {
        let current = io_get_current_irp_stack_location(irp);
        let next = io_get_next_irp_stack_location(irp);

        // Копируем все кроме completion routine
        (*next).major_function = (*current).major_function;
        (*next).minor_function = (*current).minor_function;
        (*next).flags = (*current).flags;
        (*next).parameters = (*current).parameters;
        (*next).device_object = (*current).device_object;
        (*next).file_object = (*current).file_object;
    }
}

/// IoSetCompletionRoutine - устанавливает completion routine
pub unsafe fn io_set_completion_routine(
    irp: PIRP,
    completion_routine: PIO_COMPLETION_ROUTINE,
    context: PVOID,
    invoke_on_success: bool,
    invoke_on_error: bool,
    invoke_on_cancel: bool,
) {
    unsafe {
        let next = io_get_next_irp_stack_location(irp);
        (*next).completion_routine = completion_routine;
        (*next).context = context;

        let mut control: u8 = 0;
        if invoke_on_success {
            control |= SL_INVOKE_ON_SUCCESS;
        }
        if invoke_on_error {
            control |= SL_INVOKE_ON_ERROR;
        }
        if invoke_on_cancel {
            control |= SL_INVOKE_ON_CANCEL;
        }
        (*next).control = control;
    }
}

/// IoMarkIrpPending - отмечает IRP как pending
#[inline]
pub unsafe fn io_mark_irp_pending(irp: PIRP) {
    unsafe {
        let stack = io_get_current_irp_stack_location(irp);
        (*stack).control |= SL_PENDING_RETURNED;
    }
}

/// IofCallDriver - вызывает драйвер для обработки IRP
pub unsafe fn iof_call_driver(device_object: PDEVICE_OBJECT, irp: PIRP) -> NTSTATUS {
    unsafe {
        if device_object.is_null() || irp.is_null() {
            return STATUS_INVALID_PARAMETER;
        }

        // Переходим к следующему stack location
        io_set_next_irp_stack_location(irp);

        // Устанавливаем device object
        let stack = io_get_current_irp_stack_location(irp);
        (*stack).device_object = device_object;

        // Вызываем dispatch routine
        let driver = (*device_object).driver_object;
        if driver.is_null() {
            return STATUS_INVALID_DEVICE_REQUEST;
        }

        let major = (*stack).major_function as usize;
        if major > IRP_MJ_MAXIMUM_FUNCTION as usize {
            return STATUS_INVALID_DEVICE_REQUEST;
        }

        if let Some(dispatch) = (*driver).major_function[major] {
            dispatch(device_object, irp)
        } else {
            STATUS_INVALID_DEVICE_REQUEST
        }
    }
}

/// IoCallDriver - синоним IofCallDriver
#[inline]
pub unsafe fn io_call_driver(device_object: PDEVICE_OBJECT, irp: PIRP) -> NTSTATUS {
    unsafe { iof_call_driver(device_object, irp) }
}

/// IofCompleteRequest — завершает IRP
///
/// Обходит стек снизу вверх, вызывая completion routines.
/// Обрабатывает PendingReturned и STATUS_MORE_PROCESSING_REQUIRED.
pub unsafe fn iof_complete_request(irp: PIRP, priority_boost: i8) {
    unsafe {
        if irp.is_null() {
            return;
        }

        // Проверяем что IRP не был уже освобождён или отменён без cancel routine
        debug_assert!(
            (*irp).r#type == IO_TYPE_IRP as i16,
            "IofCompleteRequest: invalid IRP type"
        );

        // Обходим стек снизу вверх, вызывая completion routines
        let mut location = (*irp).current_location;
        let stack_base = (irp as usize + IRP::BASE_SIZE) as *mut IO_STACK_LOCATION;

        // Проверка валидности location перед началом
        if location < 1 || location > (*irp).stack_count + 1 {
            panic!(
                "IofCompleteRequest: invalid current_location={} (stack_count={})",
                location,
                (*irp).stack_count
            );
        }

        while location <= (*irp).stack_count {
            // Увеличиваем current_location (двигаемся вверх по стеку)
            (*irp).current_location += 1;
            (*irp).tail.overlay.current_stack_location = (*irp).tail.overlay.current_stack_location.add(1);

            // location - 1 должно быть >= 0
            if location < 1 {
                panic!("IofCompleteRequest: location underflow (location={})", location);
            }
            let stack = stack_base.add((location - 1) as usize);

            // Обрабатываем PendingReturned
            // Если нижний драйвер вернул STATUS_PENDING, распространяем флаг вверх
            if (*irp).pending_returned && location > 1 {
                if location < 2 {
                    panic!("IofCompleteRequest: prev_stack underflow (location={})", location);
                }
                let prev_stack = stack_base.add((location - 2) as usize);
                (*prev_stack).control |= SL_PENDING_RETURNED;
            }

            // Проверяем нужно ли вызвать completion routine
            let control = (*stack).control;
            let status = (*irp).io_status.status;
            
            if let Some(completion) = (*stack).completion_routine {
                // Определяем условия вызова
                let invoke_on_success = (control & SL_INVOKE_ON_SUCCESS) != 0;
                let invoke_on_error = (control & SL_INVOKE_ON_ERROR) != 0;
                let invoke_on_cancel = (control & SL_INVOKE_ON_CANCEL) != 0;

                let should_call = (status >= 0 && invoke_on_success)
                    || (status < 0 && invoke_on_error)
                    || ((*irp).cancel && invoke_on_cancel);

                if should_call {
                    // Сбрасываем pending_returned перед вызовом completion
                    (*irp).pending_returned = false;

                    let result = completion((*stack).device_object, irp, (*stack).context);

                    // Если вернули STATUS_MORE_PROCESSING_REQUIRED, останавливаемся
                    // Драйвер возьмёт на себя ответственность за IRP
                    if result == STATUS_MORE_PROCESSING_REQUIRED {
                        return;
                    }
                } else {
                    // Completion routine не вызывается — сбрасываем pending_returned
                    (*irp).pending_returned = false;
                }
            } else {
                // Нет completion routine — сбрасываем pending_returned
                (*irp).pending_returned = false;
            }

            location += 1;
        }

        // Финальная обработка — IRP полностью завершён

        // Сигнализируем событие если есть
        if !(*irp).user_event.is_null() {
            crate::ke::event::ke_set_event(&mut *(*irp).user_event, priority_boost as i32, false);
        }

        // Копируем статус в user IOSB
        if !(*irp).user_iosb.is_null() {
            (*(*irp).user_iosb) = (*irp).io_status;
        }

        // APC completion для async I/O
        // Если есть ApcRoutine в IRP overlay, вставляем kernel APC
        iop_queue_completion_apc(irp, priority_boost);

        // Освобождаем MDL если был выделен для Direct I/O
        if !(*irp).mdl_address.is_null() {
            iop_cleanup_mdl(irp);
        }

        // Освобождаем system buffer если был выделен для Buffered I/O
        if ((*irp).flags & IRP_DEALLOCATE_BUFFER) != 0 {
            iop_free_system_buffer(irp);
        }

        // Освобождаем IRP
        io_free_irp(irp);
    }
}

/// IoCompleteRequest - синоним IofCompleteRequest
#[inline]
pub unsafe fn io_complete_request(irp: PIRP, priority_boost: i8) {
    unsafe {
        iof_complete_request(irp, priority_boost);
    }
}

/// Приоритеты завершения IRP
pub const IO_NO_INCREMENT: i8 = 0;
pub const IO_CD_ROM_INCREMENT: i8 = 1;
pub const IO_DISK_INCREMENT: i8 = 1;
pub const IO_KEYBOARD_INCREMENT: i8 = 6;
pub const IO_MOUSE_INCREMENT: i8 = 6;
pub const IO_NAMED_PIPE_INCREMENT: i8 = 2;
pub const IO_NETWORK_INCREMENT: i8 = 2;
pub const IO_PARALLEL_INCREMENT: i8 = 1;
pub const IO_SERIAL_INCREMENT: i8 = 2;
pub const IO_SOUND_INCREMENT: i8 = 8;
pub const IO_VIDEO_INCREMENT: i8 = 1;

/// STATUS_MORE_PROCESSING_REQUIRED - completion routine хочет продолжить обработку
pub const STATUS_MORE_PROCESSING_REQUIRED: NTSTATUS = 0xC0000016_u32 as i32;

// =============================================================================
// APC Completion
// =============================================================================

/// Kernel APC routine для I/O completion
type PKNORMAL_ROUTINE = Option<unsafe extern "win64" fn(
    normal_context: PVOID,
    system_argument1: PVOID,
    system_argument2: PVOID,
)>;

/// Ставит в очередь APC для завершения async I/O
unsafe fn iop_queue_completion_apc(irp: PIRP, _priority_boost: i8) {
    unsafe {
        if irp.is_null() {
            return;
        }

        // Проверяем есть ли APC routine в overlay
        let user_apc_routine = (*irp).overlay.asynchronous_parameters.user_apc_routine;
        if user_apc_routine.is_null() {
            return;
        }

        // Получаем thread из tail
        let thread = (*irp).tail.overlay.thread;
        if thread.is_null() {
            return;
        }

        // Для async I/O с user APC routine нужно вставить kernel APC
        // который затем вызовет user APC routine
        
        // В NT это делается так:
        // 1. Выделяется KAPC structure из NonPagedPool
        // 2. KeInitializeApc(apc, thread, OriginalApcEnvironment,
        //                    IopCompleteRequest, NULL, user_apc_routine, UserMode, user_apc_context)
        // 3. KeInsertQueueApc(apc, irp->UserIosb, NULL, priority_boost)
        
        // Упрощённая реализация для boot scenario:
        // Kernel mode I/O обычно синхронное (через event), APC не используется
        // User mode async I/O потребует полной реализации KAPC subsystem
        
        // TODO: Полная реализация когда будет:
        // - KAPC structure и KeInitializeApc
        // - KeInsertQueueApc с proper APC delivery
        // - APC environment handling (OriginalApcEnvironment vs AttachedApcEnvironment)
        // - User mode APC delivery через trap frame
    }
}

// =============================================================================
// IRP Cancel
// =============================================================================

/// Тип cancel routine
pub type PDRIVER_CANCEL =
    Option<unsafe extern "win64" fn(device_object: PDEVICE_OBJECT, irp: PIRP)>;

/// IoAcquireCancelSpinLock — захватывает глобальный cancel spinlock
///
/// Возвращает предыдущий IRQL для последующего освобождения.
#[inline]
pub unsafe fn io_acquire_cancel_spin_lock(irql: *mut u8) {
    unsafe {
        use crate::ke::spinlock::ke_acquire_spin_lock;

        // Используем &raw mut для Rust 2024 edition
        let lock_ref = &*(&raw mut super::iop::IO_CANCEL_SPIN_LOCK);
        *irql = ke_acquire_spin_lock(lock_ref);
    }
}

/// IoReleaseCancelSpinLock — освобождает глобальный cancel spinlock
#[inline]
pub unsafe fn io_release_cancel_spin_lock(irql: u8) {
    unsafe {
        use crate::ke::spinlock::ke_release_spin_lock;

        // Используем &raw mut для Rust 2024 edition
        let lock_ref = &*(&raw mut super::iop::IO_CANCEL_SPIN_LOCK);
        ke_release_spin_lock(lock_ref, irql);
    }
}

/// IoSetCancelRoutine — устанавливает cancel routine для IRP
///
/// Возвращает предыдущую cancel routine (или None).
/// Вызывающий должен держать cancel spinlock!
#[inline]
pub unsafe fn io_set_cancel_routine(irp: PIRP, cancel_routine: PDRIVER_CANCEL) -> PDRIVER_CANCEL {
    unsafe {
        if irp.is_null() {
            return None;
        }

        // Атомарно заменяем cancel routine
        let old = (*irp).cancel_routine;
        (*irp).cancel_routine = match cancel_routine {
            Some(f) => f as PVOID,
            None => core::ptr::null_mut(),
        };

        if old.is_null() {
            None
        } else {
            Some(core::mem::transmute(old))
        }
    }
}

/// IoCancelIrp — отменяет IRP
///
/// Устанавливает флаг Cancel и вызывает cancel routine если она установлена.
///
/// # Returns
/// true если cancel routine была вызвана, false если нет cancel routine
pub unsafe fn io_cancel_irp(irp: PIRP) -> bool {
    unsafe {
        if irp.is_null() {
            return false;
        }

        // Захватываем cancel spinlock
        let mut irql: u8 = 0;
        io_acquire_cancel_spin_lock(&mut irql);

        // Устанавливаем флаг Cancel
        (*irp).cancel = true;
        (*irp).cancel_irql = irql;

        // Получаем и очищаем cancel routine атомарно
        let cancel_routine_ptr = (*irp).cancel_routine;
        (*irp).cancel_routine = core::ptr::null_mut();

        if cancel_routine_ptr.is_null() {
            // Нет cancel routine — освобождаем spinlock и возвращаем false
            io_release_cancel_spin_lock(irql);
            return false;
        }

        // Есть cancel routine — вызываем её
        // Cancel routine должна освободить spinlock!
        let cancel_routine: unsafe extern "win64" fn(PDEVICE_OBJECT, PIRP) =
            core::mem::transmute(cancel_routine_ptr);

        // Получаем device object из текущего stack location
        let stack = io_get_current_irp_stack_location(irp);
        let device_object = if !stack.is_null() {
            (*stack).device_object
        } else {
            core::ptr::null_mut()
        };

        // Вызываем cancel routine (она освободит spinlock)
        cancel_routine(device_object, irp);

        true
    }
}

/// IoStartPacket — запускает обработку IRP через StartIo
///
/// Если устройство не занято, вызывает StartIo напрямую.
/// Иначе ставит IRP в очередь устройства.
///
/// # Arguments
/// * `device_object` - объект устройства
/// * `irp` - IRP для обработки  
/// * `key` - ключ для сортированной вставки (или NULL для FIFO)
/// * `cancel_function` - cancel routine для установки
pub unsafe fn io_start_packet(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
    key: *const ULONG,
    cancel_function: PDRIVER_CANCEL,
) {
    use crate::io::device::{ke_insert_device_queue, ke_insert_by_key_device_queue, KDEVICE_QUEUE_ENTRY};
    
    unsafe {
        if device_object.is_null() || irp.is_null() {
            return;
        }

        // Устанавливаем cancel routine под spinlock
        if cancel_function.is_some() {
            let mut cancel_irql: u8 = 0;
            io_acquire_cancel_spin_lock(&mut cancel_irql);
            io_set_cancel_routine(irp, cancel_function);
            io_release_cancel_spin_lock(cancel_irql);
        }

        // Получаем KDEVICE_QUEUE_ENTRY из IRP.tail.overlay
        // В NT это встроено в IRP (tail.overlay.device_queue_entry)
        // Для нас используем list_entry как queue entry (упрощение)
        let queue_entry = &mut (*irp).tail.overlay.list_entry as *mut _ as *mut KDEVICE_QUEUE_ENTRY;
        
        // Пытаемся вставить в очередь устройства
        let inserted = if key.is_null() {
            // FIFO - просто вставляем в конец
            ke_insert_device_queue(&mut (*device_object).device_queue, queue_entry)
        } else {
            // Сортированная вставка по ключу
            ke_insert_by_key_device_queue(&mut (*device_object).device_queue, queue_entry, *key)
        };
        
        if !inserted {
            // Устройство было свободно - запускаем StartIo напрямую
            (*device_object).current_irp = irp;
            
            let driver = (*device_object).driver_object;
            if !driver.is_null() {
                if let Some(start_io) = (*driver).driver_start_io {
                    start_io(device_object, irp);
                }
            }
        }
        // Если inserted == true, IRP в очереди и будет обработан позже через IoStartNextPacket
    }
}

/// IoStartNextPacket — запускает следующий IRP из очереди устройства
///
/// Извлекает следующий IRP из KDEVICE_QUEUE и вызывает StartIo.
///
/// # Arguments
/// * `device_object` - объект устройства
/// * `cancelable` - если true, проверяет cancel flag перед запуском
pub unsafe fn io_start_next_packet(device_object: PDEVICE_OBJECT, cancelable: bool) {
    use crate::io::device::{ke_remove_device_queue, KDEVICE_QUEUE_ENTRY};
    
    unsafe {
        if device_object.is_null() {
            return;
        }

        // Очищаем текущий IRP
        (*device_object).current_irp = core::ptr::null_mut();
        
        // Извлекаем следующий элемент из очереди
        let queue_entry = ke_remove_device_queue(&mut (*device_object).device_queue);
        
        if queue_entry.is_null() {
            // Очередь пуста
            return;
        }
        
        // Получаем IRP из queue_entry
        // queue_entry = &irp.tail.overlay.list_entry
        let list_entry_offset = core::mem::offset_of!(IRP, tail);
        // tail.overlay.list_entry offset (после driver_context[4], thread, auxiliary_buffer)
        let overlay_list_entry_offset = 32 + 8 + 8; // 48 bytes
        let total_offset = list_entry_offset + overlay_list_entry_offset;
        
        let irp = ((queue_entry as usize) - total_offset) as PIRP;
        
        // Если cancelable - проверяем cancel flag
        if cancelable {
            let mut cancel_irql: u8 = 0;
            io_acquire_cancel_spin_lock(&mut cancel_irql);
            
            if (*irp).cancel {
                // IRP был отменён - вызываем cancel routine и пропускаем
                let cancel_routine = io_set_cancel_routine(irp, None);
                io_release_cancel_spin_lock(cancel_irql);
                
                if let Some(routine) = cancel_routine {
                    let stack = io_get_current_irp_stack_location(irp);
                    let dev = if !stack.is_null() { (*stack).device_object } else { core::ptr::null_mut() };
                    routine(dev, irp);
                }
                
                // Рекурсивно пытаемся получить следующий IRP
                io_start_next_packet(device_object, cancelable);
                return;
            }
            
            io_release_cancel_spin_lock(cancel_irql);
        }
        
        // Запускаем StartIo для этого IRP
        (*device_object).current_irp = irp;
        
        let driver = (*device_object).driver_object;
        if !driver.is_null() {
            if let Some(start_io) = (*driver).driver_start_io {
                start_io(device_object, irp);
            }
        }
    }
}

// =============================================================================
// IRP Buffering
// =============================================================================

/// IopAllocateSystemBuffer — выделяет системный буфер для buffered I/O
///
/// Используется для DO_BUFFERED_IO.
/// Выделяет буфер и копирует входные данные.
pub unsafe fn iop_allocate_system_buffer(
    irp: PIRP,
    length: usize,
    input_buffer: PVOID,
    input_length: usize,
) -> NTSTATUS {
    unsafe {
        use crate::ex::pool::POOL_TYPE;
        use crate::ex::pool::ex_allocate_pool_with_tag;

        if irp.is_null() || length == 0 {
            return STATUS_INVALID_PARAMETER;
        }

        // Выделяем буфер из NonPagedPool
        let buffer = ex_allocate_pool_with_tag(
            POOL_TYPE::NonPagedPool,
            length,
            u32::from_le_bytes(*b"IrpB"), // IRP Buffer tag
        );

        if buffer.is_null() {
            return STATUS_INSUFFICIENT_RESOURCES;
        }

        // Обнуляем буфер
        core::ptr::write_bytes(buffer as *mut u8, 0, length);

        // Копируем входные данные если есть
        if !input_buffer.is_null() && input_length > 0 {
            let copy_len = core::cmp::min(input_length, length);
            core::ptr::copy_nonoverlapping(input_buffer as *const u8, buffer as *mut u8, copy_len);
        }

        (*irp).associated_irp.system_buffer = buffer;
        (*irp).flags |= IRP_BUFFERED_IO | IRP_DEALLOCATE_BUFFER;

        STATUS_SUCCESS
    }
}

/// IopFreeSystemBuffer — освобождает системный буфер
pub unsafe fn iop_free_system_buffer(irp: PIRP) {
    unsafe {
        use crate::ex::pool::ex_free_pool_with_tag;

        if irp.is_null() {
            return;
        }

        if ((*irp).flags & IRP_DEALLOCATE_BUFFER) != 0 && !(*irp).associated_irp.system_buffer.is_null() {
            ex_free_pool_with_tag((*irp).associated_irp.system_buffer, u32::from_le_bytes(*b"IrpB"));
            (*irp).associated_irp.system_buffer = core::ptr::null_mut();
            (*irp).flags &= !IRP_DEALLOCATE_BUFFER;
        }
    }
}

/// IopCopyBackSystemBuffer — копирует данные обратно в user buffer
///
/// Вызывается при завершении buffered I/O для записи.
pub unsafe fn iop_copy_back_system_buffer(irp: PIRP, user_buffer: PVOID, length: usize) {
    unsafe {
        if irp.is_null() || user_buffer.is_null() || (*irp).associated_irp.system_buffer.is_null() {
            return;
        }

        if ((*irp).flags & IRP_INPUT_OPERATION) != 0 {
            // Для READ операций копируем данные обратно
            core::ptr::copy_nonoverlapping(
                (*irp).associated_irp.system_buffer as *const u8,
                user_buffer as *mut u8,
                length,
            );
        }
    }
}

// =============================================================================
// MDL Functions for Direct I/O
// =============================================================================

/// IopBuildMdlForBuffer — строит MDL для буфера Direct I/O
///
/// Выделяет MDL, выполняет page walk и блокирует страницы.
/// Затем отображает страницы в системное адресное пространство.
///
/// # Arguments
/// * `irp` - IRP для связывания MDL
/// * `buffer` - виртуальный адрес user buffer
/// * `length` - размер буфера
/// * `is_write` - true если операция записи (нужен write access к страницам)
/// * `access_mode` - KernelMode (0) или UserMode (1)
///
/// # Returns
/// STATUS_SUCCESS при успехе, код ошибки иначе.
pub unsafe fn iop_build_mdl_for_buffer(
    irp: PIRP,
    buffer: PVOID,
    length: u32,
    is_write: bool,
    access_mode: u8,
) -> NTSTATUS {
    unsafe {
        use crate::mm::mdl::LOCK_OPERATION;
        use crate::mm::mdl::MDL_PAGES_LOCKED;
        use crate::mm::mdl::io_allocate_mdl;
        use crate::mm::mdl::mm_map_locked_pages;
        use crate::mm::mdl::mm_probe_and_lock_pages;

        #[cfg(feature = "trace-io")]
        {
            crate::kd::dbg_print("[IO] build_mdl: buf=0x");
            crate::kd::dbg_print_hex(buffer as u64);
            crate::kd::dbg_print(" len=");
            crate::kd::dbg_print_num(length as u64);
            crate::kd::dbg_print("\n");
        }
        
        if irp.is_null() || buffer.is_null() || length == 0 {
            return STATUS_INVALID_PARAMETER;
        }

        // Выделяем MDL
        #[cfg(feature = "trace-io")]
        crate::kd::dbg_print("[IO] build_mdl: allocating MDL...\n");
        
        let mdl = io_allocate_mdl(
            buffer,
            length,
            false, // secondary_buffer
            false, // charge_quota
            irp as PVOID,
        );

        if mdl.is_null() {
            #[cfg(feature = "trace-io")]
            crate::kd::dbg_print("[IO] build_mdl: io_allocate_mdl FAILED\n");
            return STATUS_INSUFFICIENT_RESOURCES;
        }
        
        #[cfg(feature = "trace-io")]
        {
            crate::kd::dbg_print("[IO] build_mdl: MDL=0x");
            crate::kd::dbg_print_hex(mdl as u64);
            crate::kd::dbg_print("\n");
        }

        // Определяем тип операции для lock
        let operation = if is_write {
            // Для WRITE драйверу нужен Read access к user buffer (читает оттуда)
            LOCK_OPERATION::IoReadAccess
        } else {
            // Для READ драйверу нужен Write access к user buffer (пишет туда)
            LOCK_OPERATION::IoWriteAccess
        };

        // Probe и lock страницы
        #[cfg(feature = "trace-io")]
        crate::kd::dbg_print("[IO] build_mdl: probe_and_lock...\n");
        
        let status = mm_probe_and_lock_pages(mdl, access_mode, operation);
        if status != STATUS_SUCCESS {
            #[cfg(feature = "trace-io")]
            {
                crate::kd::dbg_print("[IO] build_mdl: probe_and_lock FAILED 0x");
                crate::kd::dbg_print_hex(status as u64);
                crate::kd::dbg_print("\n");
            }
            crate::mm::mdl::io_free_mdl(mdl);
            return status;
        }
        
        #[cfg(feature = "trace-io")]
        crate::kd::dbg_print("[IO] build_mdl: probe_and_lock OK\n");

        // Маппим страницы в system VA
        #[cfg(feature = "trace-io")]
        crate::kd::dbg_print("[IO] build_mdl: mm_map_locked_pages...\n");
        
        let system_va = mm_map_locked_pages(mdl, 0); // KernelMode
        if system_va.is_null() && ((*mdl).mdl_flags as u32 & MDL_PAGES_LOCKED) != 0 {
            // Если маппинг не удался но страницы залочены - освобождаем
            crate::mm::mdl::mm_unlock_pages(mdl);
            crate::mm::mdl::io_free_mdl(mdl);
            return STATUS_INSUFFICIENT_RESOURCES;
        }

        // Связываем MDL с IRP
        (*irp).mdl_address = mdl;

        STATUS_SUCCESS
    }
}

/// IopCleanupMdl — освобождает MDL при завершении IRP
///
/// Размаппит страницы, разблокирует их и освобождает MDL.
pub unsafe fn iop_cleanup_mdl(irp: PIRP) {
    unsafe {
        use crate::mm::mdl::MDL_MAPPED_TO_SYSTEM_VA;
        use crate::mm::mdl::MDL_PAGES_LOCKED;
        use crate::mm::mdl::io_free_mdl;
        use crate::mm::mdl::mm_unlock_pages;
        use crate::mm::mdl::mm_unmap_locked_pages;

        if irp.is_null() {
            return;
        }

        let mdl = (*irp).mdl_address;
        if mdl.is_null() {
            return;
        }

        // Размаппим если был отображён
        if ((*mdl).mdl_flags as u32 & MDL_MAPPED_TO_SYSTEM_VA) != 0 {
            mm_unmap_locked_pages((*mdl).mapped_system_va, mdl);
        }

        // Разблокируем страницы если были залочены
        if ((*mdl).mdl_flags as u32 & MDL_PAGES_LOCKED) != 0 {
            mm_unlock_pages(mdl);
        }

        // Освобождаем MDL
        io_free_mdl(mdl);
        (*irp).mdl_address = core::ptr::null_mut();
    }
}

/// MmGetSystemAddressForMdlSafe — возвращает system VA для MDL
///
/// Эквивалент MmGetSystemAddressForMdlSafe из NT.
/// Если MDL не отображён, возвращает NULL (safe версия не делает mapping).
#[inline]
pub fn mm_get_system_address_for_mdl_safe(mdl: *mut MDL) -> PVOID {
    if mdl.is_null() {
        return core::ptr::null_mut();
    }
    unsafe { (*mdl).system_address() }
}

// =============================================================================
// IRP Builder Functions
// =============================================================================

/// IoBuildDeviceIoControlRequest — строит IRP для DeviceIoControl
///
/// Создаёт IRP_MJ_DEVICE_CONTROL или IRP_MJ_INTERNAL_DEVICE_CONTROL.
pub unsafe fn io_build_device_io_control_request(
    io_control_code: ULONG,
    device_object: PDEVICE_OBJECT,
    input_buffer: PVOID,
    input_buffer_length: ULONG,
    output_buffer: PVOID,
    output_buffer_length: ULONG,
    internal_device_io_control: bool,
    event: *mut KEVENT,
    io_status_block: PIO_STATUS_BLOCK,
) -> PIRP {
    unsafe {
        if device_object.is_null() {
            return core::ptr::null_mut();
        }

        // Выделяем IRP
        let stack_size = (*device_object).stack_size as i8;
        let irp = io_allocate_irp(stack_size, false);
        if irp.is_null() {
            return core::ptr::null_mut();
        }

        // Настраиваем IRP
        (*irp).user_event = event;
        (*irp).user_iosb = io_status_block;
        (*irp).requestor_mode = 0; // KernelMode
        (*irp).flags = IRP_SYNCHRONOUS_API;

        // Определяем метод буферизации
        let method = super::types::io_method_from_ctl_code(io_control_code);

        // Получаем следующий stack location
        let stack = io_get_next_irp_stack_location(irp);
        (*stack).major_function = if internal_device_io_control {
            super::types::IRP_MJ_INTERNAL_DEVICE_CONTROL
        } else {
            super::types::IRP_MJ_DEVICE_CONTROL
        };
        (*stack).parameters.device_io_control.io_control_code = io_control_code;
        (*stack).parameters.device_io_control.input_buffer_length = input_buffer_length;
        (*stack).parameters.device_io_control.output_buffer_length = output_buffer_length;

        match method {
            super::types::METHOD_BUFFERED => {
                // Выделяем системный буфер
                let buffer_len = core::cmp::max(input_buffer_length, output_buffer_length) as usize;
                if buffer_len > 0 {
                    let status = iop_allocate_system_buffer(
                        irp,
                        buffer_len,
                        input_buffer,
                        input_buffer_length as usize,
                    );
                    if status != STATUS_SUCCESS {
                        io_free_irp(irp);
                        return core::ptr::null_mut();
                    }
                }
                (*irp).user_buffer = output_buffer;
            },
            super::types::METHOD_IN_DIRECT | super::types::METHOD_OUT_DIRECT => {
                // Direct I/O:
                // - input buffer копируется в system_buffer (как METHOD_BUFFERED)
                // - output buffer отображается через MDL

                // Копируем input в system_buffer
                if input_buffer_length > 0 && !input_buffer.is_null() {
                    let status = iop_allocate_system_buffer(
                        irp,
                        input_buffer_length as usize,
                        input_buffer,
                        input_buffer_length as usize,
                    );
                    if status != STATUS_SUCCESS {
                        io_free_irp(irp);
                        return core::ptr::null_mut();
                    }
                }

                // Строим MDL для output buffer
                if output_buffer_length > 0 && !output_buffer.is_null() {
                    // METHOD_IN_DIRECT: драйвер читает из output buffer (IoReadAccess)
                    // METHOD_OUT_DIRECT: драйвер пишет в output buffer (IoWriteAccess)
                    let is_write = method == super::types::METHOD_OUT_DIRECT;
                    let mdl_status = iop_build_mdl_for_buffer(
                        irp,
                        output_buffer,
                        output_buffer_length,
                        is_write,
                        0, // KernelMode (для IoBuildDeviceIoControlRequest)
                    );
                    if mdl_status != STATUS_SUCCESS {
                        iop_free_system_buffer(irp);
                        io_free_irp(irp);
                        return core::ptr::null_mut();
                    }
                }
                (*irp).user_buffer = output_buffer;
            },
            super::types::METHOD_NEITHER => {
                // Neither - передаём буферы напрямую
                (*stack).parameters.device_io_control.type3_input_buffer = input_buffer;
                (*irp).user_buffer = output_buffer;
            },
            _ => {},
        }

        irp
    }
}

/// IoBuildSynchronousFsdRequest — строит синхронный IRP для файловой операции
///
/// Создаёт IRP_MJ_READ, IRP_MJ_WRITE, IRP_MJ_FLUSH_BUFFERS или IRP_MJ_SHUTDOWN.
pub unsafe fn io_build_synchronous_fsd_request(
    major_function: u8,
    device_object: PDEVICE_OBJECT,
    buffer: PVOID,
    length: ULONG,
    starting_offset: *const LARGE_INTEGER,
    event: *mut KEVENT,
    io_status_block: PIO_STATUS_BLOCK,
) -> PIRP {
    unsafe {
        #[cfg(feature = "trace-io")]
        {
            crate::kd::dbg_print("[IO] BuildSyncFsd: major=");
            crate::kd::dbg_print_num(major_function as u64);
            crate::kd::dbg_print(" dev=0x");
            crate::kd::dbg_print_hex(device_object as u64);
            crate::kd::dbg_print(" len=");
            crate::kd::dbg_print_num(length as u64);
            crate::kd::dbg_print("\n");
        }
        
        if device_object.is_null() {
            return core::ptr::null_mut();
        }

        // Выделяем IRP
        let stack_size = (*device_object).stack_size as i8;
        
        #[cfg(feature = "trace-io")]
        {
            crate::kd::dbg_print("[IO] BuildSyncFsd: stack_size=");
            crate::kd::dbg_print_num(stack_size as u64);
            crate::kd::dbg_print("\n");
        }
        
        let irp = io_allocate_irp(stack_size, false);
        if irp.is_null() {
            #[cfg(feature = "trace-io")]
            crate::kd::dbg_print("[IO] BuildSyncFsd: allocate_irp FAILED\n");
            return core::ptr::null_mut();
        }
        
        #[cfg(feature = "trace-io")]
        {
            crate::kd::dbg_print("[IO] BuildSyncFsd: IRP=0x");
            crate::kd::dbg_print_hex(irp as u64);
            crate::kd::dbg_print("\n");
        }

        // Настраиваем IRP
        (*irp).user_event = event;
        (*irp).user_iosb = io_status_block;
        (*irp).requestor_mode = 0; // KernelMode
        (*irp).flags = IRP_SYNCHRONOUS_API;

        // Получаем следующий stack location
        let stack = io_get_next_irp_stack_location(irp);
        (*stack).major_function = major_function;
        
        #[cfg(feature = "trace-io")]
        {
            crate::kd::dbg_print("[IO] BuildSyncFsd: setting params...\n");
        }

        match major_function {
            super::types::IRP_MJ_READ | super::types::IRP_MJ_WRITE => {
                (*stack).parameters.read.length = length;
                (*stack).parameters.read.key = 0;
                if !starting_offset.is_null() {
                    (*stack).parameters.read.byte_offset = *starting_offset;
                }

                // Определяем тип буферизации по флагам устройства
                let device_flags = (*device_object).flags;
                
                #[cfg(feature = "trace-io")]
                {
                    crate::kd::dbg_print("[IO] BuildSyncFsd: flags=0x");
                    crate::kd::dbg_print_hex(device_flags as u64);
                    crate::kd::dbg_print("\n");
                }

                if (device_flags & super::types::DO_BUFFERED_IO) != 0 {
                    // Buffered I/O
                    if length > 0 {
                        let input_buffer = if major_function == super::types::IRP_MJ_WRITE {
                            buffer
                        } else {
                            core::ptr::null_mut()
                        };
                        let input_len = if major_function == super::types::IRP_MJ_WRITE {
                            length as usize
                        } else {
                            0
                        };

                        let status = iop_allocate_system_buffer(
                            irp,
                            length as usize,
                            input_buffer,
                            input_len,
                        );
                        if status != STATUS_SUCCESS {
                            io_free_irp(irp);
                            return core::ptr::null_mut();
                        }
                    }
                    (*irp).user_buffer = buffer;
                    if major_function == super::types::IRP_MJ_READ {
                        (*irp).flags |= IRP_INPUT_OPERATION;
                    }
                } else if (device_flags & super::types::DO_DIRECT_IO) != 0 {
                    // Direct I/O - строим MDL
                    #[cfg(feature = "trace-io")]
                    {
                        crate::kd::dbg_print("[IO] BuildSyncFsd: DIRECT_IO path, buf=0x");
                        crate::kd::dbg_print_hex(buffer as u64);
                        crate::kd::dbg_print("\n");
                    }
                    
                    if length > 0 {
                        let is_write = major_function == super::types::IRP_MJ_WRITE;
                        
                        #[cfg(feature = "trace-io")]
                        crate::kd::dbg_print("[IO] BuildSyncFsd: calling iop_build_mdl_for_buffer\n");
                        
                        let mdl_status = iop_build_mdl_for_buffer(
                            irp, buffer, length, is_write, 0, // KernelMode
                        );
                        
                        #[cfg(feature = "trace-io")]
                        {
                            crate::kd::dbg_print("[IO] BuildSyncFsd: MDL status=0x");
                            crate::kd::dbg_print_hex(mdl_status as u64);
                            crate::kd::dbg_print("\n");
                        }
                        
                        if mdl_status != STATUS_SUCCESS {
                            io_free_irp(irp);
                            return core::ptr::null_mut();
                        }
                    }
                } else {
                    // Neither I/O - буфер напрямую
                    (*irp).user_buffer = buffer;
                }
            },
            super::types::IRP_MJ_FLUSH_BUFFERS | super::types::IRP_MJ_SHUTDOWN => {
                // Нет буферов
            },
            _ => {},
        }

        irp
    }
}

/// IoBuildAsynchronousFsdRequest — строит асинхронный IRP для файловой операции
pub unsafe fn io_build_asynchronous_fsd_request(
    major_function: u8,
    device_object: PDEVICE_OBJECT,
    buffer: PVOID,
    length: ULONG,
    starting_offset: *const LARGE_INTEGER,
    io_status_block: PIO_STATUS_BLOCK,
) -> PIRP {
    unsafe {
        // Асинхронный IRP не имеет события для ожидания
        let irp = io_build_synchronous_fsd_request(
            major_function,
            device_object,
            buffer,
            length,
            starting_offset,
            core::ptr::null_mut(), // no event
            io_status_block,
        );

        if !irp.is_null() {
            (*irp).flags &= !IRP_SYNCHRONOUS_API;
        }

        irp
    }
}
