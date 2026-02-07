//! FILE_OBJECT - Объект файла
//!
//! # Архитектура
//!
//! ```text
//! NtCreateFile / NtOpenFile
//!        │
//!        ▼
//! ┌─────────────────┐
//! │ IopCreateFile   │ ─────► Парсинг пути, поиск устройства
//! └────────┬────────┘
//!          │
//!          ▼
//! ┌─────────────────┐
//! │ FILE_OBJECT     │ ◄───── Выделение и инициализация
//! │  + SHARE_ACCESS │
//! └────────┬────────┘
//!          │
//!          ▼
//! ┌─────────────────┐
//! │ IRP_MJ_CREATE   │ ─────► Отправка драйверу
//! └────────┬────────┘
//!          │
//!          ▼
//! ┌─────────────────┐
//! │ Handle Table    │ ─────► Создание хэндла для процесса
//! └─────────────────┘
//!
//! Закрытие файла:
//!
//! NtClose ──► IRP_MJ_CLEANUP ──► IRP_MJ_CLOSE ──► Освобождение
//!             (последний handle)  (последняя ref)
//! ```
//!
//! # Share Access
//!
//! SHARE_ACCESS отслеживает режимы открытия файла:
//! - Количество открытий с каждым типом доступа (read/write/delete)
//! - Количество открытий с разрешённым sharing (FILE_SHARE_*)
//!
//! # Отступления и упрощения
//!
//! - Упрощённый parsing пути (нет полной поддержки reparse points)
//! - Нет поддержки named streams
//! - Нет поддержки oplock
//!
//! Источники:
//! - ReactOS: sdk/include/xdk/iotypes.h, ntoskrnl/io/iomgr/file.c
//! - Windows XP: base/ntos/io/iomgr/parse.c

use super::device::DEVICE_OBJECT;
use super::device::VPB;
use super::types::*;
use crate::ke::event::KEVENT;
use crate::ke::spinlock::KSPIN_LOCK;
use crate::nt::CSHORT;
use crate::nt::LARGE_INTEGER;
use crate::nt::LIST_ENTRY;
use crate::nt::NTSTATUS;
use crate::nt::PVOID;
use crate::nt::ULONG;
use crate::nt::ntdef::UNICODE_STRING;
use crate::nt::ntstatus::*;

// =============================================================================
// SHARE_ACCESS
// =============================================================================

/// SHARE_ACCESS — структура для отслеживания режимов доступа к файлу
///
/// Хранится в FILE_OBJECT или устройстве (для exclusive devices).
/// Используется для проверки совместимости при открытии файла.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SHARE_ACCESS {
    /// Количество открытий с доступом на чтение
    pub open_count: ULONG,
    /// Количество readers
    pub readers: ULONG,
    /// Количество writers
    pub writers: ULONG,
    /// Количество deleters
    pub deleters: ULONG,
    /// Количество shared readers
    pub shared_read: ULONG,
    /// Количество shared writers
    pub shared_write: ULONG,
    /// Количество shared deleters
    pub shared_delete: ULONG,
}

impl SHARE_ACCESS {
    pub const fn new() -> Self {
        Self {
            open_count: 0,
            readers: 0,
            writers: 0,
            deleters: 0,
            shared_read: 0,
            shared_write: 0,
            shared_delete: 0,
        }
    }
}

// =============================================================================
// SECTION_OBJECT_POINTERS
// =============================================================================

/// SECTION_OBJECT_POINTERS - указатели для Cache Manager / Memory Manager
#[repr(C)]
pub struct SECTION_OBJECT_POINTERS {
    pub data_section_object: PVOID,
    pub shared_cache_map: PVOID,
    pub image_section_object: PVOID,
}

impl SECTION_OBJECT_POINTERS {
    pub const fn new() -> Self {
        Self {
            data_section_object: core::ptr::null_mut(),
            shared_cache_map: core::ptr::null_mut(),
            image_section_object: core::ptr::null_mut(),
        }
    }
}

// =============================================================================
// IO_COMPLETION_CONTEXT
// =============================================================================

/// IO_COMPLETION_CONTEXT - контекст завершения I/O
#[repr(C)]
pub struct IO_COMPLETION_CONTEXT {
    pub port: PVOID,
    pub key: PVOID,
}

// =============================================================================
// FILE_OBJECT
// =============================================================================

/// FILE_OBJECT - объект файла
///
/// Представляет открытый экземпляр файла или устройства.
/// Создается при каждом вызове NtCreateFile/NtOpenFile.
#[repr(C)]
pub struct FILE_OBJECT {
    /// Тип объекта (IO_TYPE_FILE)
    pub r#type: CSHORT,
    /// Размер структуры
    pub size: CSHORT,
    /// Устройство, на котором открыт файл
    pub device_object: *mut DEVICE_OBJECT,
    /// Volume Parameter Block
    pub vpb: *mut VPB,
    /// Контекст файловой системы (1)
    pub fs_context: PVOID,
    /// Контекст файловой системы (2)
    pub fs_context2: PVOID,
    /// Указатели для section objects
    pub section_object_pointer: *mut SECTION_OBJECT_POINTERS,
    /// Private cache map
    pub private_cache_map: PVOID,
    /// Финальный статус (для async I/O)
    pub final_status: NTSTATUS,
    /// Связанный файловый объект
    pub related_file_object: *mut FILE_OBJECT,
    /// Была ли операция блокировки
    pub lock_operation: bool,
    /// Ожидается удаление
    pub delete_pending: bool,
    /// Доступ на чтение
    pub read_access: bool,
    /// Доступ на запись
    pub write_access: bool,
    /// Доступ на удаление
    pub delete_access: bool,
    /// Разделяемый доступ на чтение
    pub shared_read: bool,
    /// Разделяемый доступ на запись
    pub shared_write: bool,
    /// Разделяемый доступ на удаление
    pub shared_delete: bool,
    /// Флаги файла (FO_*)
    pub flags: ULONG,
    /// Имя файла
    pub file_name: UNICODE_STRING,
    /// Текущая позиция
    pub current_byte_offset: LARGE_INTEGER,
    /// Количество ожидающих потоков
    pub waiters: ULONG,
    /// Флаг занятости
    pub busy: ULONG,
    /// Последняя блокировка
    pub last_lock: PVOID,
    /// Блокировка файла
    pub lock: KEVENT,
    /// Событие файла
    pub event: KEVENT,
    /// Контекст завершения
    pub completion_context: *mut IO_COMPLETION_CONTEXT,
    /// Блокировка списка IRP
    pub irp_list_lock: KSPIN_LOCK,
    /// Список IRP
    pub irp_list: LIST_ENTRY,
    /// Расширение файлового объекта
    pub file_object_extension: PVOID,
}

impl FILE_OBJECT {
    /// Размер структуры
    pub const SIZE: usize = core::mem::size_of::<Self>();
}

// =============================================================================
// File Object Functions
// =============================================================================

/// IoCreateFileObject - создает объект файла
///
/// Внутренняя функция для создания FILE_OBJECT.
pub unsafe fn iop_create_file_object(device_object: *mut DEVICE_OBJECT) -> PFILE_OBJECT {
    unsafe {
        use crate::ex::pool::POOL_TYPE;
        use crate::ex::pool::ex_allocate_pool_with_tag;

        // Выделяем память
        let file = ex_allocate_pool_with_tag(
            POOL_TYPE::NonPagedPool,
            FILE_OBJECT::SIZE,
            u32::from_le_bytes(*b"File"),
        );

        if file.is_null() {
            return core::ptr::null_mut();
        }

        // Обнуляем
        core::ptr::write_bytes(file as *mut u8, 0, FILE_OBJECT::SIZE);

        let fo = file as PFILE_OBJECT;

        // Инициализируем
        (*fo).r#type = IO_TYPE_FILE as CSHORT;
        (*fo).size = FILE_OBJECT::SIZE as CSHORT;
        (*fo).device_object = device_object;

        // Инициализируем события
        crate::ke::event::ke_initialize_event(
            &mut (*fo).lock,
            crate::ke::event::EVENT_TYPE::SynchronizationEvent,
            false,
        );
        crate::ke::event::ke_initialize_event(
            &mut (*fo).event,
            crate::ke::event::EVENT_TYPE::NotificationEvent,
            false,
        );

        // Инициализируем список IRP (head указывает на себя)
        LIST_ENTRY::init_head(&mut (*fo).irp_list);

        fo
    }
}

/// IopDeleteFileObject - удаляет объект файла
pub unsafe fn iop_delete_file_object(file_object: PFILE_OBJECT) {
    unsafe {
        use crate::ex::pool::ex_free_pool_with_tag;

        if file_object.is_null() {
            return;
        }

        // Примечание: file_name.buffer не освобождается здесь
        // В текущей реализации buffer указывает на remaining_name из caller'а
        // или на буфер из OBJECT_ATTRIBUTES, который управляется вызывающей стороной
        // 
        // Если в будущем будем копировать имя в отдельный буфер, нужно будет:
        // - Установить флаг FO_FILE_NAME_ALLOCATED
        // - Освобождать buffer здесь с тегом 'mNlF'

        ex_free_pool_with_tag(file_object as PVOID, u32::from_le_bytes(*b"File"));
    }
}

/// IoGetRelatedDeviceObject - возвращает устройство для FILE_OBJECT
///
/// Учитывает VPB для томов файловой системы.
pub unsafe fn io_get_related_device_object(file_object: PFILE_OBJECT) -> *mut DEVICE_OBJECT {
    unsafe {
        if file_object.is_null() {
            return core::ptr::null_mut();
        }

        // Если есть VPB - используем устройство файловой системы
        let vpb = (*file_object).vpb;
        if !vpb.is_null() && ((*file_object).flags & FO_FILE_OPEN) != 0 {
            return (*vpb).device_object;
        }

        // Иначе возвращаем прямое устройство
        (*file_object).device_object
    }
}

/// IoGetBaseFileSystemDeviceObject - возвращает базовое устройство ФС
pub unsafe fn io_get_base_file_system_device_object(
    file_object: PFILE_OBJECT,
) -> *mut DEVICE_OBJECT {
    unsafe {
        if file_object.is_null() {
            return core::ptr::null_mut();
        }

        let vpb = (*file_object).vpb;
        if !vpb.is_null() {
            return (*vpb).device_object;
        }

        (*file_object).device_object
    }
}

// =============================================================================
// Share Access Functions
// =============================================================================

/// IoCheckShareAccess — проверяет совместимость share access
///
/// Проверяет, можно ли открыть файл с заданным доступом учитывая
/// уже существующие открытия.
///
/// # Arguments
/// * `desired_access` - запрашиваемый доступ (FILE_READ_DATA и т.д.)
/// * `desired_share_access` - запрашиваемый share (FILE_SHARE_*)
/// * `file_object` - открываемый файл (для установки флагов)
/// * `share_access` - текущее состояние share access
/// * `update` - если true, обновляет share_access при успехе
pub unsafe fn io_check_share_access(
    desired_access: ULONG,
    desired_share_access: ULONG,
    file_object: PFILE_OBJECT,
    share_access: *mut SHARE_ACCESS,
    update: bool,
) -> NTSTATUS {
    unsafe {
        if share_access.is_null() {
            return STATUS_INVALID_PARAMETER;
        }

        // Определяем запрашиваемые типы доступа
        let read_access = (desired_access & (FILE_READ_DATA | FILE_EXECUTE)) != 0;
        let write_access = (desired_access & (FILE_WRITE_DATA | FILE_APPEND_DATA)) != 0;
        let delete_access = (desired_access & 0x00010000) != 0; // DELETE

        // Проверяем share access
        let share_read = (desired_share_access & FILE_SHARE_READ) != 0;
        let share_write = (desired_share_access & FILE_SHARE_WRITE) != 0;
        let share_delete = (desired_share_access & FILE_SHARE_DELETE) != 0;

        let sa = &*share_access;

        // Проверка конфликтов:
        // 1. Если кто-то читает, а мы не разрешаем share read - конфликт
        // 2. Если кто-то пишет, а мы не разрешаем share write - конфликт
        // 3. Если кто-то удаляет, а мы не разрешаем share delete - конфликт

        // Проверяем: существующие readers vs наш share
        if sa.readers > 0 && !share_read {
            return STATUS_SHARING_VIOLATION;
        }

        // Проверяем: существующие writers vs наш share
        if sa.writers > 0 && !share_write {
            return STATUS_SHARING_VIOLATION;
        }

        // Проверяем: существующие deleters vs наш share
        if sa.deleters > 0 && !share_delete {
            return STATUS_SHARING_VIOLATION;
        }

        // Обратная проверка: наш доступ vs существующий share
        if read_access && sa.shared_read < sa.open_count {
            return STATUS_SHARING_VIOLATION;
        }

        if write_access && sa.shared_write < sa.open_count {
            return STATUS_SHARING_VIOLATION;
        }

        if delete_access && sa.shared_delete < sa.open_count {
            return STATUS_SHARING_VIOLATION;
        }

        // Устанавливаем флаги в FILE_OBJECT
        if !file_object.is_null() {
            (*file_object).read_access = read_access;
            (*file_object).write_access = write_access;
            (*file_object).delete_access = delete_access;
            (*file_object).shared_read = share_read;
            (*file_object).shared_write = share_write;
            (*file_object).shared_delete = share_delete;
        }

        // Обновляем share access если нужно
        if update {
            io_update_share_access(file_object, share_access);
        }

        STATUS_SUCCESS
    }
}

/// IoSetShareAccess — устанавливает начальный share access
///
/// Вызывается для первого открытия файла.
pub unsafe fn io_set_share_access(
    desired_access: ULONG,
    desired_share_access: ULONG,
    file_object: PFILE_OBJECT,
    share_access: *mut SHARE_ACCESS,
) {
    unsafe {
        if share_access.is_null() {
            return;
        }

        // Определяем типы доступа
        let read_access = (desired_access & (FILE_READ_DATA | FILE_EXECUTE)) != 0;
        let write_access = (desired_access & (FILE_WRITE_DATA | FILE_APPEND_DATA)) != 0;
        let delete_access = (desired_access & 0x00010000) != 0; // DELETE

        // Определяем sharing
        let share_read = (desired_share_access & FILE_SHARE_READ) != 0;
        let share_write = (desired_share_access & FILE_SHARE_WRITE) != 0;
        let share_delete = (desired_share_access & FILE_SHARE_DELETE) != 0;

        // Устанавливаем флаги в FILE_OBJECT
        if !file_object.is_null() {
            (*file_object).read_access = read_access;
            (*file_object).write_access = write_access;
            (*file_object).delete_access = delete_access;
            (*file_object).shared_read = share_read;
            (*file_object).shared_write = share_write;
            (*file_object).shared_delete = share_delete;
        }

        // Инициализируем SHARE_ACCESS
        let sa = &mut *share_access;
        sa.open_count = 1;
        sa.readers = if read_access { 1 } else { 0 };
        sa.writers = if write_access { 1 } else { 0 };
        sa.deleters = if delete_access { 1 } else { 0 };
        sa.shared_read = if share_read { 1 } else { 0 };
        sa.shared_write = if share_write { 1 } else { 0 };
        sa.shared_delete = if share_delete { 1 } else { 0 };
    }
}

/// IoUpdateShareAccess — обновляет share access после успешной проверки
pub unsafe fn io_update_share_access(file_object: PFILE_OBJECT, share_access: *mut SHARE_ACCESS) {
    unsafe {
        if share_access.is_null() || file_object.is_null() {
            return;
        }

        let sa = &mut *share_access;

        sa.open_count += 1;

        if (*file_object).read_access {
            sa.readers += 1;
        }
        if (*file_object).write_access {
            sa.writers += 1;
        }
        if (*file_object).delete_access {
            sa.deleters += 1;
        }
        if (*file_object).shared_read {
            sa.shared_read += 1;
        }
        if (*file_object).shared_write {
            sa.shared_write += 1;
        }
        if (*file_object).shared_delete {
            sa.shared_delete += 1;
        }
    }
}

/// IoRemoveShareAccess — удаляет share access при закрытии файла
pub unsafe fn io_remove_share_access(file_object: PFILE_OBJECT, share_access: *mut SHARE_ACCESS) {
    unsafe {
        if share_access.is_null() || file_object.is_null() {
            return;
        }

        let sa = &mut *share_access;

        if sa.open_count > 0 {
            sa.open_count -= 1;
        }

        if (*file_object).read_access && sa.readers > 0 {
            sa.readers -= 1;
        }
        if (*file_object).write_access && sa.writers > 0 {
            sa.writers -= 1;
        }
        if (*file_object).delete_access && sa.deleters > 0 {
            sa.deleters -= 1;
        }
        if (*file_object).shared_read && sa.shared_read > 0 {
            sa.shared_read -= 1;
        }
        if (*file_object).shared_write && sa.shared_write > 0 {
            sa.shared_write -= 1;
        }
        if (*file_object).shared_delete && sa.shared_delete > 0 {
            sa.shared_delete -= 1;
        }
    }
}

// =============================================================================
// IRP_MJ_CREATE / CLEANUP / CLOSE
// =============================================================================

/// IopSendCreateIrp — отправляет IRP_MJ_CREATE драйверу
///
/// Создаёт и отправляет IRP для открытия файла/устройства.
pub unsafe fn iop_send_create_irp(
    device_object: *mut DEVICE_OBJECT,
    file_object: PFILE_OBJECT,
    _desired_access: ULONG,
    share_access: ULONG,
    create_disposition: ULONG,
    create_options: ULONG,
    _io_status_block: PIO_STATUS_BLOCK,
) -> NTSTATUS {
    unsafe {
        use super::irp::io_allocate_irp;
        use super::irp::io_call_driver;
        use super::irp::io_get_next_irp_stack_location;

        if device_object.is_null() || file_object.is_null() {
            return STATUS_INVALID_PARAMETER;
        }

        // Получаем top device в стеке
        let top_device = super::device::io_get_attached_device(device_object);
        let stack_size = (*top_device).stack_size as i8;

        // Выделяем IRP
        let irp = io_allocate_irp(stack_size, false);
        if irp.is_null() {
            return STATUS_INSUFFICIENT_RESOURCES;
        }

        // Настраиваем IRP
        (*irp).flags = IRP_CREATE_OPERATION | IRP_SYNCHRONOUS_API;
        (*irp).requestor_mode = 0; // KernelMode

        // Настраиваем stack location
        let stack = io_get_next_irp_stack_location(irp);
        (*stack).major_function = IRP_MJ_CREATE;
        (*stack).file_object = file_object;
        (*stack).device_object = top_device;

        // Параметры CREATE
        (*stack).parameters.create.security_context = core::ptr::null_mut();
        (*stack).parameters.create.options = (create_disposition << 24) | create_options;
        (*stack).parameters.create.file_attributes = 0;
        (*stack).parameters.create.share_access = share_access as u16;
        (*stack).parameters.create.ea_length = 0;

        // Отправляем IRP драйверу
        let status = io_call_driver(top_device, irp);

        // Для синхронного IRP - IRP уже освобождён в IoCompleteRequest
        // или нужно дождаться завершения

        status
    }
}

/// IopSendCleanupIrp — отправляет IRP_MJ_CLEANUP драйверу
///
/// Вызывается при закрытии последнего хэндла файла.
/// Драйвер должен отменить все pending IRP для этого файла.
pub unsafe fn iop_send_cleanup_irp(file_object: PFILE_OBJECT) -> NTSTATUS {
    unsafe {
        use super::irp::io_allocate_irp;
        use super::irp::io_call_driver;
        use super::irp::io_get_next_irp_stack_location;

        if file_object.is_null() {
            return STATUS_INVALID_PARAMETER;
        }

        let device_object = io_get_related_device_object(file_object);
        if device_object.is_null() {
            return STATUS_INVALID_DEVICE_REQUEST;
        }

        // Получаем top device
        let top_device = super::device::io_get_attached_device(device_object);
        let stack_size = (*top_device).stack_size as i8;

        // Выделяем IRP
        let irp = io_allocate_irp(stack_size, false);
        if irp.is_null() {
            return STATUS_INSUFFICIENT_RESOURCES;
        }

        // Настраиваем IRP
        (*irp).flags = IRP_CLOSE_OPERATION | IRP_SYNCHRONOUS_API;
        (*irp).requestor_mode = 0;

        // Настраиваем stack location
        let stack = io_get_next_irp_stack_location(irp);
        (*stack).major_function = IRP_MJ_CLEANUP;
        (*stack).file_object = file_object;
        (*stack).device_object = top_device;

        // Отправляем IRP
        let status = io_call_driver(top_device, irp);

        // Устанавливаем флаг cleanup complete
        (*file_object).flags |= FO_CLEANUP_COMPLETE;

        status
    }
}

/// IopSendCloseIrp — отправляет IRP_MJ_CLOSE драйверу
///
/// Вызывается при удалении последней ссылки на FILE_OBJECT.
/// После этого файловый объект будет уничтожен.
pub unsafe fn iop_send_close_irp(file_object: PFILE_OBJECT) -> NTSTATUS {
    unsafe {
        use super::irp::io_allocate_irp;
        use super::irp::io_call_driver;
        use super::irp::io_get_next_irp_stack_location;

        if file_object.is_null() {
            return STATUS_INVALID_PARAMETER;
        }

        let device_object = io_get_related_device_object(file_object);
        if device_object.is_null() {
            return STATUS_INVALID_DEVICE_REQUEST;
        }

        // Получаем top device
        let top_device = super::device::io_get_attached_device(device_object);
        let stack_size = (*top_device).stack_size as i8;

        // Выделяем IRP
        let irp = io_allocate_irp(stack_size, false);
        if irp.is_null() {
            return STATUS_INSUFFICIENT_RESOURCES;
        }

        // Настраиваем IRP
        (*irp).flags = IRP_CLOSE_OPERATION | IRP_SYNCHRONOUS_API;
        (*irp).requestor_mode = 0;

        // Настраиваем stack location
        let stack = io_get_next_irp_stack_location(irp);
        (*stack).major_function = IRP_MJ_CLOSE;
        (*stack).file_object = file_object;
        (*stack).device_object = top_device;

        // Отправляем IRP
        io_call_driver(top_device, irp)
    }
}

/// IopCloseFile — закрывает файл (CLEANUP + CLOSE)
///
/// Полная процедура закрытия файла.
pub unsafe fn iop_close_file(file_object: PFILE_OBJECT) -> NTSTATUS {
    unsafe {
        if file_object.is_null() {
            return STATUS_INVALID_PARAMETER;
        }

        // Отправляем CLEANUP если ещё не было
        if ((*file_object).flags & FO_CLEANUP_COMPLETE) == 0 {
            let _ = iop_send_cleanup_irp(file_object);
        }

        // Отправляем CLOSE
        let status = iop_send_close_irp(file_object);

        // Освобождаем FILE_OBJECT
        iop_delete_file_object(file_object);

        status
    }
}

// =============================================================================
// File Access Constants
// =============================================================================

/// Права доступа для файлов
pub const FILE_READ_DATA: ULONG = 0x0001;
pub const FILE_LIST_DIRECTORY: ULONG = 0x0001;
pub const FILE_WRITE_DATA: ULONG = 0x0002;
pub const FILE_ADD_FILE: ULONG = 0x0002;
pub const FILE_APPEND_DATA: ULONG = 0x0004;
pub const FILE_ADD_SUBDIRECTORY: ULONG = 0x0004;
pub const FILE_READ_EA: ULONG = 0x0008;
pub const FILE_WRITE_EA: ULONG = 0x0010;
pub const FILE_EXECUTE: ULONG = 0x0020;
pub const FILE_TRAVERSE: ULONG = 0x0020;
pub const FILE_DELETE_CHILD: ULONG = 0x0040;
pub const FILE_READ_ATTRIBUTES: ULONG = 0x0080;
pub const FILE_WRITE_ATTRIBUTES: ULONG = 0x0100;

pub const FILE_ALL_ACCESS: ULONG = 0x001F01FF;
pub const FILE_GENERIC_READ: ULONG = 0x00120089;
pub const FILE_GENERIC_WRITE: ULONG = 0x00120116;
pub const FILE_GENERIC_EXECUTE: ULONG = 0x001200A0;

// =============================================================================
// File Create Options
// =============================================================================

pub const FILE_DIRECTORY_FILE: ULONG = 0x00000001;
pub const FILE_WRITE_THROUGH: ULONG = 0x00000002;
pub const FILE_SEQUENTIAL_ONLY: ULONG = 0x00000004;
pub const FILE_NO_INTERMEDIATE_BUFFERING: ULONG = 0x00000008;
pub const FILE_SYNCHRONOUS_IO_ALERT: ULONG = 0x00000010;
pub const FILE_SYNCHRONOUS_IO_NONALERT: ULONG = 0x00000020;
pub const FILE_NON_DIRECTORY_FILE: ULONG = 0x00000040;
pub const FILE_CREATE_TREE_CONNECTION: ULONG = 0x00000080;
pub const FILE_COMPLETE_IF_OPLOCKED: ULONG = 0x00000100;
pub const FILE_NO_EA_KNOWLEDGE: ULONG = 0x00000200;
pub const FILE_OPEN_REMOTE_INSTANCE: ULONG = 0x00000400;
pub const FILE_RANDOM_ACCESS: ULONG = 0x00000800;
pub const FILE_DELETE_ON_CLOSE: ULONG = 0x00001000;
pub const FILE_OPEN_BY_FILE_ID: ULONG = 0x00002000;
pub const FILE_OPEN_FOR_BACKUP_INTENT: ULONG = 0x00004000;
pub const FILE_NO_COMPRESSION: ULONG = 0x00008000;

// =============================================================================
// File Create Disposition
// =============================================================================

pub const FILE_SUPERSEDE: ULONG = 0x00000000;
pub const FILE_OPEN: ULONG = 0x00000001;
pub const FILE_CREATE: ULONG = 0x00000002;
pub const FILE_OPEN_IF: ULONG = 0x00000003;
pub const FILE_OVERWRITE: ULONG = 0x00000004;
pub const FILE_OVERWRITE_IF: ULONG = 0x00000005;

// =============================================================================
// File Share Access
// =============================================================================

pub const FILE_SHARE_READ: ULONG = 0x00000001;
pub const FILE_SHARE_WRITE: ULONG = 0x00000002;
pub const FILE_SHARE_DELETE: ULONG = 0x00000004;

// =============================================================================
// IO_STATUS_BLOCK
// =============================================================================

/// IO_STATUS_BLOCK — блок статуса I/O операции
///
/// Используется для возврата статуса и информации из асинхронных I/O операций.
#[repr(C)]
pub struct IO_STATUS_BLOCK {
    /// Status/Pointer union (status при завершении, pointer при pending)
    pub status: NTSTATUS,
    /// Количество переданных байтов или другая информация
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

/// Тип указателя на IO_STATUS_BLOCK
pub type PIO_STATUS_BLOCK = *mut IO_STATUS_BLOCK;

/// Тип APC routine для асинхронных I/O операций
pub type PIO_APC_ROUTINE = unsafe extern "win64" fn(
    apc_context: PVOID,
    io_status_block: PIO_STATUS_BLOCK,
    reserved: ULONG,
);

// =============================================================================
// IopCreateFile - Internal Implementation
// =============================================================================

/// IopCheckVolumeMount — проверяет и монтирует том при необходимости
///
/// Если устройство имеет VPB и том не смонтирован, вызывает `iop_mount_volume`.
/// Возвращает target device (FS device если смонтирован) и статус.
///
/// # Arguments
/// * `device_object` - Storage device
/// * `remaining_path` - Путь к файлу после имени устройства
///
/// # Returns
/// (target_device, status) - FS device если смонтирован, NULL если прямой доступ к storage
unsafe fn iop_check_volume_mount(
    device_object: *mut DEVICE_OBJECT,
    remaining_path: &UNICODE_STRING,
) -> (*mut DEVICE_OBJECT, NTSTATUS) {
    use super::vpb::{iop_mount_volume, VPB_MOUNTED};
    
    if device_object.is_null() {
        return (core::ptr::null_mut(), STATUS_INVALID_PARAMETER);
    }
    
    // Получаем VPB
    let vpb = (*device_object).vpb;
    
    // Если нет VPB - это не storage device, работаем напрямую
    if vpb.is_null() {
        return (core::ptr::null_mut(), STATUS_SUCCESS);
    }
    
    // Проверяем смонтирован ли том
    if ((*vpb).flags & VPB_MOUNTED) == 0 {
        // Том не смонтирован - пробуем смонтировать
        #[cfg(feature = "trace-io")]
        crate::dbg_print!("[IO] Volume not mounted, attempting mount...\n");
        
        // allow_raw_mount = true для fallback на RAW FS
        let mount_status = iop_mount_volume(device_object, true);
        
        if mount_status != STATUS_SUCCESS {
            #[cfg(feature = "trace-io")]
            crate::dbg_print!("[IO] Mount failed with status 0x{:08X}\n", mount_status as u32);
            // Если путь пустой (открываем сам том), разрешаем прямой доступ
            if remaining_path.length == 0 {
                return (core::ptr::null_mut(), STATUS_SUCCESS);
            }
            // Если путь непустой - ошибка, нужна FS для доступа к файлам
            return (core::ptr::null_mut(), mount_status);
        }
        
        #[cfg(feature = "trace-io")]
        crate::dbg_print!("[IO] Volume mounted successfully!\n");
    }
    
    // Том смонтирован - получаем FS device
    let fs_device = (*vpb).device_object;
    
    // Если путь к файлу непустой, используем FS device
    if remaining_path.length > 0 && !fs_device.is_null() {
        return (fs_device, STATUS_SUCCESS);
    }
    
    // Путь пустой или нет FS device - прямой доступ к storage
    if !fs_device.is_null() {
        return (fs_device, STATUS_SUCCESS);
    }
    
    (core::ptr::null_mut(), STATUS_SUCCESS)
}

/// IopParseDevice — парсит путь и находит целевое устройство
///
/// Разбирает путь вида `\Device\HarddiskVolume1\path\to\file`
/// и возвращает устройство и оставшийся путь.
unsafe fn iop_parse_device(
    object_attributes: *const crate::ob::types::OBJECT_ATTRIBUTES,
    remaining_path: *mut UNICODE_STRING,
) -> (*mut DEVICE_OBJECT, NTSTATUS) {
    unsafe {
        use crate::ob;

        if object_attributes.is_null() {
            return (core::ptr::null_mut(), STATUS_INVALID_PARAMETER);
        }

        let attrs = &*object_attributes;
        if attrs.object_name.is_null() {
            return (core::ptr::null_mut(), STATUS_OBJECT_NAME_INVALID);
        }

        let name = &*attrs.object_name;
        if name.buffer.is_null() || name.length == 0 {
            return (core::ptr::null_mut(), STATUS_OBJECT_NAME_INVALID);
        }

        // Открываем объект по имени - получаем HANDLE
        let mut handle: PVOID = core::ptr::null_mut();
        let status = ob::handle::ob_open_object_by_name(
            object_attributes as *mut _,
            core::ptr::null_mut(), // any type
            0,                     // KernelMode
            core::ptr::null_mut(),
            0,
            core::ptr::null_mut(),
            &mut handle,
        );

        if status != STATUS_SUCCESS {
            return (core::ptr::null_mut(), status);
        }

        if handle.is_null() {
            return (core::ptr::null_mut(), STATUS_OBJECT_NAME_NOT_FOUND);
        }

        // Преобразуем handle в указатель на объект
        let mut found_object: PVOID = core::ptr::null_mut();
        let ref_status = ob::refcount::ob_reference_object_by_handle(
            handle,
            0,                     // desired_access
            core::ptr::null_mut(), // any type
            0,                     // KernelMode
            &mut found_object,
            core::ptr::null_mut(),
        );

        // Закрываем handle - нам нужен только указатель на объект
        ob::handle::NtClose(handle as usize);

        if ref_status != STATUS_SUCCESS {
            return (core::ptr::null_mut(), ref_status);
        }

        if found_object.is_null() {
            return (core::ptr::null_mut(), STATUS_OBJECT_NAME_NOT_FOUND);
        }

        // Если это DEVICE_OBJECT - проверяем по типу структуры
        let device = found_object as *mut DEVICE_OBJECT;
        if (*device).r#type == IO_TYPE_DEVICE as i16 {
            // Пока не поддерживаем remaining path
            if !remaining_path.is_null() {
                (*remaining_path).length = 0;
                (*remaining_path).maximum_length = 0;
                (*remaining_path).buffer = core::ptr::null_mut();
            }
            return (device, STATUS_SUCCESS);
        }

        // Объект не является устройством
        ob::refcount::ob_dereference_object(found_object);
        (core::ptr::null_mut(), STATUS_OBJECT_TYPE_MISMATCH)
    }
}

/// IopCreateFile — внутренняя реализация создания/открытия файла
///
/// Это основная функция, которую вызывают NtCreateFile и NtOpenFile.
pub unsafe fn iop_create_file(
    file_handle: *mut usize,
    desired_access: ULONG,
    object_attributes: *const crate::ob::types::OBJECT_ATTRIBUTES,
    io_status_block: PIO_STATUS_BLOCK,
    _allocation_size: *const LARGE_INTEGER,
    _file_attributes: ULONG,
    share_access: ULONG,
    create_disposition: ULONG,
    create_options: ULONG,
    _ea_buffer: PVOID,
    _ea_length: ULONG,
    _requestor_mode: u8,
) -> NTSTATUS {
    unsafe {
        use crate::ob;

        // Валидация параметров
        if file_handle.is_null() || io_status_block.is_null() {
            return STATUS_INVALID_PARAMETER;
        }

        if object_attributes.is_null() {
            (*io_status_block).status = STATUS_INVALID_PARAMETER;
            (*io_status_block).information = 0;
            return STATUS_INVALID_PARAMETER;
        }

        // Парсим путь и находим устройство
        let mut remaining_path = UNICODE_STRING::new();
        let (device_object, status) = iop_parse_device(object_attributes, &mut remaining_path);

        if status != STATUS_SUCCESS {
            (*io_status_block).status = status;
            (*io_status_block).information = 0;
            return status;
        }

        if device_object.is_null() {
            (*io_status_block).status = STATUS_OBJECT_NAME_NOT_FOUND;
            (*io_status_block).information = 0;
            return STATUS_OBJECT_NAME_NOT_FOUND;
        }

        // Проверяем VPB и монтируем том если необходимо
        let (target_device, mount_status) = iop_check_volume_mount(device_object, &remaining_path);
        if mount_status != STATUS_SUCCESS {
            ob::refcount::ob_dereference_object(device_object as PVOID);
            (*io_status_block).status = mount_status;
            (*io_status_block).information = 0;
            return mount_status;
        }

        // Используем target device (FS device если смонтирован, иначе storage device)
        let actual_device = if !target_device.is_null() { target_device } else { device_object };

        // Создаём FILE_OBJECT для actual_device (FS или storage)
        let file_object = iop_create_file_object(actual_device);
        if file_object.is_null() {
            ob::refcount::ob_dereference_object(device_object as PVOID);
            (*io_status_block).status = STATUS_INSUFFICIENT_RESOURCES;
            (*io_status_block).information = 0;
            return STATUS_INSUFFICIENT_RESOURCES;
        }

        // Устанавливаем флаги FILE_OBJECT на основе create_options
        if (create_options & FILE_SYNCHRONOUS_IO_ALERT) != 0 {
            (*file_object).flags |= FO_SYNCHRONOUS_IO | FO_ALERTABLE_IO;
        }
        if (create_options & FILE_SYNCHRONOUS_IO_NONALERT) != 0 {
            (*file_object).flags |= FO_SYNCHRONOUS_IO;
        }
        if (create_options & FILE_NO_INTERMEDIATE_BUFFERING) != 0 {
            (*file_object).flags |= FO_NO_INTERMEDIATE_BUFFERING;
        }
        if (create_options & FILE_WRITE_THROUGH) != 0 {
            (*file_object).flags |= FO_WRITE_THROUGH;
        }
        if (create_options & FILE_SEQUENTIAL_ONLY) != 0 {
            (*file_object).flags |= FO_SEQUENTIAL_ONLY;
        }
        if (create_options & FILE_RANDOM_ACCESS) != 0 {
            (*file_object).flags |= FO_RANDOM_ACCESS;
        }
        if (create_options & FILE_DELETE_ON_CLOSE) != 0 {
            (*file_object).flags |= FO_DELETE_ON_CLOSE;
        }

        // Отправляем IRP_MJ_CREATE драйверу
        let status = iop_send_create_irp(
            device_object,
            file_object,
            desired_access,
            share_access,
            create_disposition,
            create_options,
            io_status_block,
        );

        if status != STATUS_SUCCESS && status != STATUS_PENDING {
            // CREATE не удался — освобождаем FILE_OBJECT
            iop_delete_file_object(file_object);
            ob::refcount::ob_dereference_object(device_object as PVOID);
            return status;
        }

        // Создаём хэндл для FILE_OBJECT
        let mut handle: PVOID = core::ptr::null_mut();
        let handle_status = ob::handle::ob_open_object_by_pointer(
            file_object as PVOID,
            0,                     // attributes
            core::ptr::null_mut(), // access state
            desired_access,
            core::ptr::null_mut(), // object type (FILE)
            0,                     // access mode
            &mut handle,
        );

        if handle_status != STATUS_SUCCESS {
            // Отправляем CLOSE
            iop_close_file(file_object);
            ob::refcount::ob_dereference_object(device_object as PVOID);
            (*io_status_block).status = handle_status;
            return handle_status;
        }

        // Устанавливаем флаг успешного открытия
        (*file_object).flags |= FO_FILE_OPEN | FO_HANDLE_CREATED;

        // Возвращаем хэндл
        *file_handle = handle as usize;
        (*io_status_block).status = status;

        // Information: действие которое было выполнено
        (*io_status_block).information = match create_disposition {
            FILE_SUPERSEDE => 0, // FILE_SUPERSEDED
            FILE_CREATE => 2,    // FILE_CREATED
            FILE_OPEN => 1,      // FILE_OPENED
            FILE_OPEN_IF => 1,   // FILE_OPENED или FILE_CREATED
            FILE_OVERWRITE => 3, // FILE_OVERWRITTEN
            FILE_OVERWRITE_IF => 3,
            _ => 0,
        };

        status
    }
}

// =============================================================================
// NtCreateFile
// =============================================================================

/// NtCreateFile
///
/// Создаёт или открывает файл, устройство, каталог или именованный канал.
///
/// # Arguments
/// * `file_handle` - указатель для возврата хэндла файла
/// * `desired_access` - запрашиваемые права доступа (FILE_READ_DATA и т.д.)
/// * `object_attributes` - атрибуты объекта (имя, директория, флаги)
/// * `io_status_block` - блок статуса I/O
/// * `allocation_size` - начальный размер (для создания)
/// * `file_attributes` - атрибуты файла (FILE_ATTRIBUTE_*)
/// * `share_access` - режим совместного доступа (FILE_SHARE_*)
/// * `create_disposition` - действие (FILE_CREATE, FILE_OPEN и т.д.)
/// * `create_options` - опции создания (FILE_DIRECTORY_FILE и т.д.)
/// * `ea_buffer` - расширенные атрибуты
/// * `ea_length` - длина EA буфера
///
/// # Returns
/// * `STATUS_SUCCESS` - файл создан/открыт
/// * `STATUS_NOT_IMPLEMENTED` - функция не реализована (заглушка)
/// * `STATUS_INVALID_PARAMETER` - невалидные параметры
#[unsafe(no_mangle)]
pub extern "win64" fn NtCreateFile(
    file_handle: *mut usize,
    desired_access: u32,
    object_attributes: PVOID,
    io_status_block: PVOID,
    allocation_size: *const LARGE_INTEGER,
    file_attributes: ULONG,
    share_access: ULONG,
    create_disposition: ULONG,
    create_options: ULONG,
    ea_buffer: PVOID,
    ea_length: ULONG,
) -> NTSTATUS {
    // Валидация обязательных параметров
    if file_handle.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    if io_status_block.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    unsafe {
        iop_create_file(
            file_handle,
            desired_access,
            object_attributes as *const crate::ob::types::OBJECT_ATTRIBUTES,
            io_status_block as PIO_STATUS_BLOCK,
            allocation_size,
            file_attributes,
            share_access,
            create_disposition,
            create_options,
            ea_buffer,
            ea_length,
            1, // UserMode
        )
    }
}

// =============================================================================
// IopReadWriteFile - Internal Implementation
// =============================================================================

/// IopReadWriteFile — внутренняя реализация чтения/записи
///
/// Общая функция для NtReadFile и NtWriteFile.
///
/// # Arguments
/// * `requestor_mode` - 0 = KernelMode, 1 = UserMode
pub unsafe fn iop_read_write_file(
    file_handle: usize,
    _event_handle: usize,
    _apc_routine: *const PIO_APC_ROUTINE,
    _apc_context: PVOID,
    io_status_block: PIO_STATUS_BLOCK,
    buffer: PVOID,
    length: ULONG,
    byte_offset: *const LARGE_INTEGER,
    _key: *const ULONG,
    is_write: bool,
    requestor_mode: u8,
) -> NTSTATUS {
    unsafe {
        use super::irp::io_allocate_irp;
        use super::irp::io_call_driver;
        use super::irp::io_get_next_irp_stack_location;
        use crate::ob;

        // Получаем FILE_OBJECT по хэндлу
        let mut file_object: PVOID = core::ptr::null_mut();
        let status = ob::refcount::ob_reference_object_by_handle(
            file_handle as PVOID,
            if is_write {
                FILE_WRITE_DATA
            } else {
                FILE_READ_DATA
            },
            core::ptr::null_mut(), // FILE_OBJECT type
            requestor_mode,        // KernelMode (0) или UserMode (1)
            &mut file_object,
            core::ptr::null_mut(),
        );

        if status != STATUS_SUCCESS {
            (*io_status_block).status = status;
            (*io_status_block).information = 0;
            return status;
        }

        let file = file_object as PFILE_OBJECT;

        // Получаем устройство
        let device_object = io_get_related_device_object(file);
        if device_object.is_null() {
            ob::refcount::ob_dereference_object(file_object);
            (*io_status_block).status = STATUS_INVALID_DEVICE_REQUEST;
            (*io_status_block).information = 0;
            return STATUS_INVALID_DEVICE_REQUEST;
        }

        // Получаем top device
        let top_device = super::device::io_get_attached_device(device_object);
        let stack_size = (*top_device).stack_size as i8;

        // Создаём IRP
        let irp = io_allocate_irp(stack_size, false);
        if irp.is_null() {
            ob::refcount::ob_dereference_object(file_object);
            (*io_status_block).status = STATUS_INSUFFICIENT_RESOURCES;
            (*io_status_block).information = 0;
            return STATUS_INSUFFICIENT_RESOURCES;
        }

        // Определяем смещение
        let offset = if !byte_offset.is_null() {
            *byte_offset
        } else {
            (*file).current_byte_offset
        };

        // Настраиваем IRP (приводим тип указателя)
        (*irp).user_iosb = io_status_block as super::types::PIO_STATUS_BLOCK;
        (*irp).user_buffer = buffer;
        (*irp).requestor_mode = 1; // UserMode

        // Определяем буферизацию по флагам устройства
        let device_flags = (*device_object).flags;
        if (device_flags & DO_BUFFERED_IO) != 0 {
            // Buffered I/O - выделяем системный буфер
            if length > 0 {
                let input_buffer = if is_write {
                    buffer
                } else {
                    core::ptr::null_mut()
                };
                let input_len = if is_write { length as usize } else { 0 };

                let alloc_status = super::irp::iop_allocate_system_buffer(
                    irp,
                    length as usize,
                    input_buffer,
                    input_len,
                );
                if alloc_status != STATUS_SUCCESS {
                    super::irp::io_free_irp(irp);
                    ob::refcount::ob_dereference_object(file_object);
                    (*io_status_block).status = alloc_status;
                    return alloc_status;
                }
            }
            if !is_write {
                (*irp).flags |= IRP_INPUT_OPERATION;
            }
        } else if (device_flags & DO_DIRECT_IO) != 0 {
            // Direct I/O - строим MDL для user buffer
            if length > 0 {
                let mdl_status = super::irp::iop_build_mdl_for_buffer(
                    irp, buffer, length, is_write, 1, // UserMode
                );
                if mdl_status != STATUS_SUCCESS {
                    super::irp::io_free_irp(irp);
                    ob::refcount::ob_dereference_object(file_object);
                    (*io_status_block).status = mdl_status;
                    return mdl_status;
                }
            }
        } else {
            // Neither I/O - буфер передаётся напрямую
            (*irp).user_buffer = buffer;
        }

        // Синхронный или асинхронный режим
        let synchronous = ((*file).flags & FO_SYNCHRONOUS_IO) != 0;
        if synchronous {
            (*irp).flags |= IRP_SYNCHRONOUS_API;
        }

        // Настраиваем stack location
        let stack = io_get_next_irp_stack_location(irp);
        (*stack).major_function = if is_write { IRP_MJ_WRITE } else { IRP_MJ_READ };
        (*stack).file_object = file;
        (*stack).device_object = top_device;

        // Параметры READ/WRITE
        (*stack).parameters.read.length = length;
        (*stack).parameters.read.key = 0;
        (*stack).parameters.read.byte_offset = offset;

        // Отправляем IRP
        let mut status = io_call_driver(top_device, irp);

        // Для синхронного режима ждём завершения
        if synchronous && status == STATUS_PENDING {
            // Ждём на event из FILE_OBJECT
            use crate::ke::wait::ke_wait_for_single_object;
            
            let wait_status = ke_wait_for_single_object(
                &mut (*file).event as *mut _ as PVOID,
                0, // Executive wait reason
                0, // KernelMode
                false, // not alertable
                None, // no timeout
            );
            
            if wait_status == STATUS_SUCCESS {
                // Event был signaled, берём финальный статус из IOSB
                status = (*io_status_block).status;
            }
        }

        // Обновляем позицию в файле если синхронный режим
        if synchronous && status == STATUS_SUCCESS {
            (*file).current_byte_offset.quad_part =
                offset.quad_part + (*io_status_block).information as i64;
        }

        ob::refcount::ob_dereference_object(file_object);
        status
    }
}

/// IopDeviceIoControl — внутренняя реализация IOCTL
unsafe fn iop_device_io_control(
    file_handle: usize,
    _event_handle: usize,
    _apc_routine: *const PIO_APC_ROUTINE,
    _apc_context: PVOID,
    io_status_block: PIO_STATUS_BLOCK,
    io_control_code: ULONG,
    input_buffer: PVOID,
    input_buffer_length: ULONG,
    output_buffer: PVOID,
    output_buffer_length: ULONG,
    is_fsctl: bool,
) -> NTSTATUS {
    unsafe {
        use super::irp::io_call_driver;
        use super::irp::io_get_next_irp_stack_location;
        use crate::ob;

        // Получаем FILE_OBJECT по хэндлу
        let mut file_object: PVOID = core::ptr::null_mut();
        let status = ob::refcount::ob_reference_object_by_handle(
            file_handle as PVOID,
            0, // No specific access required
            core::ptr::null_mut(),
            1, // UserMode
            &mut file_object,
            core::ptr::null_mut(),
        );

        if status != STATUS_SUCCESS {
            (*io_status_block).status = status;
            (*io_status_block).information = 0;
            return status;
        }

        let file = file_object as PFILE_OBJECT;

        // Получаем устройство
        let device_object = io_get_related_device_object(file);
        if device_object.is_null() {
            ob::refcount::ob_dereference_object(file_object);
            (*io_status_block).status = STATUS_INVALID_DEVICE_REQUEST;
            (*io_status_block).information = 0;
            return STATUS_INVALID_DEVICE_REQUEST;
        }

        // Получаем top device
        let top_device = super::device::io_get_attached_device(device_object);

        // Создаём IRP через builder
        let irp = super::irp::io_build_device_io_control_request(
            io_control_code,
            top_device,
            input_buffer,
            input_buffer_length,
            output_buffer,
            output_buffer_length,
            is_fsctl,              // internal = fsctl
            core::ptr::null_mut(), // event
            io_status_block as super::types::PIO_STATUS_BLOCK,
        );

        if irp.is_null() {
            ob::refcount::ob_dereference_object(file_object);
            (*io_status_block).status = STATUS_INSUFFICIENT_RESOURCES;
            (*io_status_block).information = 0;
            return STATUS_INSUFFICIENT_RESOURCES;
        }

        // Устанавливаем file_object в stack location
        let stack = io_get_next_irp_stack_location(irp);
        (*stack).file_object = file;

        // Меняем major function для FSCTL
        if is_fsctl {
            (*stack).major_function = IRP_MJ_FILE_SYSTEM_CONTROL;
        }

        // Синхронный режим
        let synchronous = ((*file).flags & FO_SYNCHRONOUS_IO) != 0;
        if synchronous {
            (*irp).flags |= IRP_SYNCHRONOUS_API;
        }

        // Отправляем IRP
        let status = io_call_driver(top_device, irp);

        ob::refcount::ob_dereference_object(file_object);
        status
    }
}

// =============================================================================
// NtOpenFile
// =============================================================================

/// NtOpenFile
///
/// Открывает существующий файл или устройство.
/// Упрощённая версия NtCreateFile с фиксированным create_disposition = FILE_OPEN.
///
/// # Arguments
/// * `file_handle` - указатель для возврата хэндла файла
/// * `desired_access` - запрашиваемые права доступа
/// * `object_attributes` - атрибуты объекта
/// * `io_status_block` - блок статуса I/O
/// * `share_access` - режим совместного доступа
/// * `open_options` - опции открытия
///
/// # Returns
/// * `STATUS_SUCCESS` - файл открыт
/// * `STATUS_NOT_IMPLEMENTED` - функция не реализована (заглушка)
#[unsafe(no_mangle)]
pub extern "win64" fn NtOpenFile(
    file_handle: *mut usize,
    desired_access: u32,
    object_attributes: PVOID,
    io_status_block: PVOID,
    share_access: ULONG,
    open_options: ULONG,
) -> NTSTATUS {
    // NtOpenFile = NtCreateFile с FILE_OPEN disposition
    NtCreateFile(
        file_handle,
        desired_access,
        object_attributes,
        io_status_block,
        core::ptr::null(), // allocation_size
        0,                 // file_attributes
        share_access,
        FILE_OPEN, // create_disposition
        open_options,
        core::ptr::null_mut(), // ea_buffer
        0,                     // ea_length
    )
}

// =============================================================================
// NtReadFile
// =============================================================================

/// NtReadFile
///
/// Читает данные из файла или устройства.
///
/// # Arguments
/// * `file_handle` - хэндл файла
/// * `event` - опциональное событие для сигнализации завершения
/// * `apc_routine` - опциональная APC routine для асинхронного завершения
/// * `apc_context` - контекст для APC
/// * `io_status_block` - блок статуса I/O
/// * `buffer` - буфер для данных
/// * `length` - количество байтов для чтения
/// * `byte_offset` - смещение в файле (NULL = текущая позиция)
/// * `key` - ключ блокировки (для locked regions)
///
/// # Returns
/// * `STATUS_SUCCESS` - данные прочитаны
/// * `STATUS_PENDING` - операция асинхронная
/// * `STATUS_NOT_IMPLEMENTED` - функция не реализована (заглушка)
#[unsafe(no_mangle)]
pub extern "win64" fn NtReadFile(
    file_handle: usize,
    event: usize,
    apc_routine: *const PIO_APC_ROUTINE,
    apc_context: PVOID,
    io_status_block: PVOID,
    buffer: PVOID,
    length: ULONG,
    byte_offset: *const LARGE_INTEGER,
    key: *const ULONG,
) -> NTSTATUS {
    // Валидация обязательных параметров
    if io_status_block.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    if buffer.is_null() && length > 0 {
        return STATUS_INVALID_PARAMETER;
    }

    unsafe {
        iop_read_write_file(
            file_handle,
            event,
            apc_routine,
            apc_context,
            io_status_block as PIO_STATUS_BLOCK,
            buffer,
            length,
            byte_offset,
            key,
            false, // is_write = false (READ)
            1,     // UserMode
        )
    }
}

// =============================================================================
// NtWriteFile
// =============================================================================

/// NtWriteFile
///
/// Записывает данные в файл или устройство.
///
/// # Arguments
/// * `file_handle` - хэндл файла
/// * `event` - опциональное событие для сигнализации завершения
/// * `apc_routine` - опциональная APC routine для асинхронного завершения
/// * `apc_context` - контекст для APC
/// * `io_status_block` - блок статуса I/O
/// * `buffer` - буфер с данными для записи
/// * `length` - количество байтов для записи
/// * `byte_offset` - смещение в файле (NULL = текущая позиция)
/// * `key` - ключ блокировки (для locked regions)
///
/// # Returns
/// * `STATUS_SUCCESS` - данные записаны
/// * `STATUS_PENDING` - операция асинхронная
/// * `STATUS_NOT_IMPLEMENTED` - функция не реализована (заглушка)
#[unsafe(no_mangle)]
pub extern "win64" fn NtWriteFile(
    file_handle: usize,
    event: usize,
    apc_routine: *const PIO_APC_ROUTINE,
    apc_context: PVOID,
    io_status_block: PVOID,
    buffer: PVOID,
    length: ULONG,
    byte_offset: *const LARGE_INTEGER,
    key: *const ULONG,
) -> NTSTATUS {
    // Валидация обязательных параметров
    if io_status_block.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    if buffer.is_null() && length > 0 {
        return STATUS_INVALID_PARAMETER;
    }

    unsafe {
        iop_read_write_file(
            file_handle,
            event,
            apc_routine,
            apc_context,
            io_status_block as PIO_STATUS_BLOCK,
            buffer,
            length,
            byte_offset,
            key,
            true, // is_write = true (WRITE)
            1,    // UserMode
        )
    }
}

// =============================================================================
// NtDeviceIoControlFile
// =============================================================================

/// NtDeviceIoControlFile
///
/// Отправляет управляющий код (IOCTL) на устройство.
///
/// # Arguments
/// * `file_handle` - хэндл файла/устройства
/// * `event` - опциональное событие
/// * `apc_routine` - опциональная APC routine
/// * `apc_context` - контекст для APC
/// * `io_status_block` - блок статуса I/O
/// * `io_control_code` - код управления (IOCTL)
/// * `input_buffer` - входной буфер
/// * `input_buffer_length` - размер входного буфера
/// * `output_buffer` - выходной буфер
/// * `output_buffer_length` - размер выходного буфера
///
/// # Returns
/// * `STATUS_SUCCESS` - операция выполнена
/// * `STATUS_NOT_IMPLEMENTED` - функция не реализована (заглушка)
#[unsafe(no_mangle)]
pub extern "win64" fn NtDeviceIoControlFile(
    file_handle: usize,
    event: usize,
    apc_routine: *const PIO_APC_ROUTINE,
    apc_context: PVOID,
    io_status_block: PVOID,
    io_control_code: ULONG,
    input_buffer: PVOID,
    input_buffer_length: ULONG,
    output_buffer: PVOID,
    output_buffer_length: ULONG,
) -> NTSTATUS {
    if io_status_block.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    unsafe {
        iop_device_io_control(
            file_handle,
            event,
            apc_routine,
            apc_context,
            io_status_block as PIO_STATUS_BLOCK,
            io_control_code,
            input_buffer,
            input_buffer_length,
            output_buffer,
            output_buffer_length,
            false, // is_fsctl = false (IOCTL)
        )
    }
}

// =============================================================================
// NtFsControlFile
// =============================================================================

/// NtFsControlFile
///
/// Отправляет управляющий код файловой системы (FSCTL) на файловую систему.
///
/// Аналогично NtDeviceIoControlFile, но использует IRP_MJ_FILE_SYSTEM_CONTROL.
///
/// # Arguments
/// Аналогичны NtDeviceIoControlFile.
///
/// # Returns
/// * `STATUS_SUCCESS` - операция выполнена
/// * `STATUS_NOT_IMPLEMENTED` - функция не реализована (заглушка)
#[unsafe(no_mangle)]
pub extern "win64" fn NtFsControlFile(
    file_handle: usize,
    event: usize,
    apc_routine: *const PIO_APC_ROUTINE,
    apc_context: PVOID,
    io_status_block: PVOID,
    fs_control_code: ULONG,
    input_buffer: PVOID,
    input_buffer_length: ULONG,
    output_buffer: PVOID,
    output_buffer_length: ULONG,
) -> NTSTATUS {
    if io_status_block.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    unsafe {
        iop_device_io_control(
            file_handle,
            event,
            apc_routine,
            apc_context,
            io_status_block as PIO_STATUS_BLOCK,
            fs_control_code,
            input_buffer,
            input_buffer_length,
            output_buffer,
            output_buffer_length,
            true, // is_fsctl = true (FSCTL)
        )
    }
}

// =============================================================================
// NtCancelIoFile
// =============================================================================

/// NtCancelIoFile
///
/// Отменяет все ожидающие I/O операции для файла, инициированные текущим потоком.
///
/// # Arguments
/// * `file_handle` - хэндл файла
/// * `io_status_block` - блок статуса результата отмены
///
/// # Returns
/// * `STATUS_SUCCESS` - операции отменены
/// * `STATUS_INVALID_HANDLE` - невалидный хэндл
#[unsafe(no_mangle)]
pub extern "win64" fn NtCancelIoFile(file_handle: usize, io_status_block: PVOID) -> NTSTATUS {
    if io_status_block.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    unsafe {
        // Получаем FILE_OBJECT по handle
        let mut file_object: PVOID = core::ptr::null_mut();
        let status = crate::ob::refcount::ob_reference_object_by_handle(
            file_handle as PVOID,
            0, // DesiredAccess - любой доступ для отмены
            core::ptr::null_mut(), // ObjectType - FILE_OBJECT type
            1, // AccessMode - UserMode
            &mut file_object,
            core::ptr::null_mut(),
        );

        if status != STATUS_SUCCESS {
            let iosb = io_status_block as PIO_STATUS_BLOCK;
            (*iosb).status = status;
            (*iosb).information = 0;
            return status;
        }

        let fo = file_object as PFILE_OBJECT;
        
        // Получаем текущий поток
        let current_thread = crate::ps::process::ps_get_current_thread() as *mut crate::ke::thread::KTHREAD;
        
        // Отменяем IRP от текущего потока
        let cancelled = iop_cancel_file_irps(fo, current_thread, core::ptr::null_mut());

        // Уменьшаем reference count
        crate::ob::refcount::ob_dereference_object(file_object);

        let iosb = io_status_block as PIO_STATUS_BLOCK;
        (*iosb).status = STATUS_SUCCESS;
        (*iosb).information = cancelled as usize;
        
        STATUS_SUCCESS
    }
}

// =============================================================================
// NtCancelIoFileEx
// =============================================================================

/// NtCancelIoFileEx
///
/// Отменяет конкретную I/O операцию для файла или все операции.
///
/// # Arguments
/// * `file_handle` - хэндл файла
/// * `io_request_to_cancel` - IO_STATUS_BLOCK операции для отмены (или NULL для всех от всех потоков)
/// * `io_status_block` - блок статуса результата отмены
///
/// # Returns
/// * `STATUS_SUCCESS` - операция отменена
/// * `STATUS_NOT_FOUND` - указанная операция не найдена
#[unsafe(no_mangle)]
pub extern "win64" fn NtCancelIoFileEx(
    file_handle: usize,
    io_request_to_cancel: PVOID,
    io_status_block: PVOID,
) -> NTSTATUS {
    if io_status_block.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    unsafe {
        // Получаем FILE_OBJECT по handle
        let mut file_object: PVOID = core::ptr::null_mut();
        let status = crate::ob::refcount::ob_reference_object_by_handle(
            file_handle as PVOID,
            0, // DesiredAccess
            core::ptr::null_mut(),
            1, // UserMode
            &mut file_object,
            core::ptr::null_mut(),
        );

        if status != STATUS_SUCCESS {
            let iosb = io_status_block as PIO_STATUS_BLOCK;
            (*iosb).status = status;
            (*iosb).information = 0;
            return status;
        }

        let fo = file_object as PFILE_OBJECT;
        let target_iosb = io_request_to_cancel as PIO_STATUS_BLOCK;
        
        // Если target_iosb == NULL, отменяем все IRP от всех потоков
        // Иначе ищем конкретный IRP по его user_iosb
        let cancelled = iop_cancel_file_irps(fo, core::ptr::null_mut(), target_iosb);

        // Уменьшаем reference count
        crate::ob::refcount::ob_dereference_object(file_object);

        let iosb = io_status_block as PIO_STATUS_BLOCK;
        
        if cancelled == 0 && !target_iosb.is_null() {
            // Конкретный IRP не найден
            (*iosb).status = STATUS_OBJECT_NAME_NOT_FOUND;
            (*iosb).information = 0;
            return STATUS_OBJECT_NAME_NOT_FOUND;
        }
        
        (*iosb).status = STATUS_SUCCESS;
        (*iosb).information = cancelled as usize;
        
        STATUS_SUCCESS
    }
}

/// Внутренняя функция для отмены IRP на FILE_OBJECT
///
/// # Arguments
/// * `file_object` - объект файла
/// * `thread` - поток (если не NULL, отменяем только IRP от этого потока)
/// * `target_iosb` - конкретный IOSB для поиска (если не NULL, ищем IRP с этим user_iosb)
///
/// # Returns
/// Количество отмененных IRP
unsafe fn iop_cancel_file_irps(
    file_object: PFILE_OBJECT,
    thread: *mut crate::ke::thread::KTHREAD,
    target_iosb: PIO_STATUS_BLOCK,
) -> u32 {
    use crate::io::irp::io_cancel_irp;
    use crate::io::irp::IRP;
    
    if file_object.is_null() {
        return 0;
    }
    
    let mut cancelled: u32 = 0;
    
    // Захватываем spinlock для списка IRP
    // DISPATCH_LEVEL = 2
    let irql = crate::ke::spinlock::ke_acquire_spin_lock_raise_to(
        &(*file_object).irp_list_lock,
        2, // DISPATCH_LEVEL
    );
    
    // Собираем IRP для отмены (нельзя отменять под spinlock напрямую)
    // т.к. cancel routine может потребовать других locks
    let mut irps_to_cancel: [*mut IRP; 64] = [core::ptr::null_mut(); 64];
    let mut count: usize = 0;
    
    // Проходим по списку IRP
    let head = &mut (*file_object).irp_list as *mut LIST_ENTRY;
    let mut entry = (*head).flink;
    
    // Смещение list_entry внутри IRP.tail.overlay
    // IRP.tail - это IRP_TAIL union, .overlay.list_entry - это LIST_ENTRY
    let list_entry_offset = core::mem::offset_of!(IRP, tail);
    // + offset of list_entry in IRP_TAIL_OVERLAY = 32 (4 pointers + thread + aux)
    // driver_context: [PVOID; 4] = 32 bytes
    // thread: *mut KTHREAD = 8 bytes  
    // auxiliary_buffer: *mut i8 = 8 bytes
    // list_entry: LIST_ENTRY
    let overlay_list_entry_offset = 32 + 8 + 8; // = 48 bytes from tail start
    let total_offset = list_entry_offset + overlay_list_entry_offset;
    
    while entry != head && count < 64 {
        // Получаем IRP из list_entry (entry - offset = irp)
        let irp = ((entry as usize) - total_offset) as *mut IRP;
        
        // Проверяем условия отмены
        let should_cancel = if !target_iosb.is_null() {
            // Ищем конкретный IRP по user_iosb (сравниваем указатели)
            (*irp).user_iosb as PVOID == target_iosb as PVOID
        } else if !thread.is_null() {
            // Отменяем только IRP от указанного потока
            (*irp).tail.overlay.thread == thread
        } else {
            // Отменяем все IRP
            true
        };
        
        if should_cancel && !(*irp).cancel {
            irps_to_cancel[count] = irp;
            count += 1;
            
            // Если ищем конкретный IRP, прекращаем поиск
            if !target_iosb.is_null() {
                break;
            }
        }
        
        entry = (*entry).flink;
    }
    
    // Освобождаем spinlock
    crate::ke::spinlock::ke_release_spin_lock(&(*file_object).irp_list_lock, irql);
    
    // Теперь отменяем собранные IRP
    for i in 0..count {
        let irp = irps_to_cancel[i];
        if !irp.is_null() {
            if io_cancel_irp(irp) {
                cancelled += 1;
            }
        }
    }
    
    cancelled
}
