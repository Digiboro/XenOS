//! Symbolic Links
//!
//! Символические ссылки в namespace.
//!
//! Источники:
//! - ReactOS: ob/oblink.c

use super::types::*;
use crate::nt::LARGE_INTEGER;
use crate::nt::NTSTATUS;
use crate::nt::PVOID;
use crate::nt::STATUS_SUCCESS;
use crate::nt::ULONG;
use crate::nt::UNICODE_STRING;

// =============================================================================
// OBJECT_SYMBOLIC_LINK
// =============================================================================

/// Символическая ссылка
#[repr(C)]
pub struct OBJECT_SYMBOLIC_LINK {
    /// Время создания
    pub creation_time: LARGE_INTEGER,
    /// Целевое имя (путь к объекту)
    pub link_target: UNICODE_STRING,
    /// Drive letter (для DOS device mapping)
    pub dos_device_drive_index: ULONG,
    /// Link target может быть объектом для быстрого lookup
    pub link_target_object: PVOID,
    /// Access mask для link target
    pub link_target_remaining_access: ULONG,
    /// Флаги
    pub flags: ULONG,
    /// Access mask
    pub access_mask: ULONG,
}

impl OBJECT_SYMBOLIC_LINK {
    pub const fn new() -> Self {
        Self {
            creation_time: LARGE_INTEGER::new(0),
            link_target: UNICODE_STRING::new(),
            dos_device_drive_index: 0,
            link_target_object: core::ptr::null_mut(),
            link_target_remaining_access: 0,
            flags: 0,
            access_mask: 0,
        }
    }
}

// =============================================================================
// NtCreateSymbolicLinkObject
// =============================================================================

/// Создает символическую ссылку
pub fn nt_create_symbolic_link_object(
    link_handle: *mut PVOID,
    desired_access: u32,
    object_attributes: *mut OBJECT_ATTRIBUTES,
    link_target: *mut UNICODE_STRING,
) -> NTSTATUS {
    if link_handle.is_null() || link_target.is_null() {
        return crate::nt::STATUS_INVALID_PARAMETER;
    }

    unsafe {
        *link_handle = core::ptr::null_mut();

        let target = &*link_target;

        // Проверяем валидность target
        if target.buffer.is_null() || target.length == 0 {
            return crate::nt::STATUS_INVALID_PARAMETER;
        }

        // Получаем тип SymbolicLink
        let link_type = super::init::obp_get_symbolic_link_object_type();
        if link_type.is_null() {
            return crate::nt::STATUS_INSUFFICIENT_RESOURCES;
        }

        // Создаем объект
        let mut link_object: PVOID = core::ptr::null_mut();
        let status = super::life::ob_create_object(
            0, // KernelMode
            link_type,
            object_attributes,
            0, // KernelMode
            core::ptr::null_mut(),
            core::mem::size_of::<OBJECT_SYMBOLIC_LINK>(),
            0,
            0,
            &mut link_object,
        );

        if status != STATUS_SUCCESS {
            return status;
        }

        // Инициализируем link
        let link = link_object as *mut OBJECT_SYMBOLIC_LINK;
        *link = OBJECT_SYMBOLIC_LINK::new();

        // Копируем target string
        let target_buffer_size = (target.length + 2) as usize;
        let target_buffer = crate::ex::pool::ex_allocate_pool_with_tag(
            crate::ex::pool::POOL_TYPE::PagedPool,
            target_buffer_size,
            u32::from_le_bytes(*b"tmyS"), // 'Symt'
        );

        if target_buffer.is_null() {
            // Cleanup
            super::refcount::ob_dereference_object(link_object);
            return crate::nt::STATUS_INSUFFICIENT_RESOURCES;
        }

        core::ptr::copy_nonoverlapping(
            target.buffer as *const u8,
            target_buffer as *mut u8,
            target.length as usize,
        );
        // Null terminate
        *((target_buffer as *mut u16).add((target.length / 2) as usize)) = 0;

        (*link).link_target.buffer = target_buffer as *mut u16;
        (*link).link_target.length = target.length;
        (*link).link_target.maximum_length = target.length + 2;

        // Устанавливаем время создания
        (*link).creation_time.quad_part = crate::ke::time::ke_query_system_time() as i64;

        // Вставляем объект
        let status = super::life::ob_insert_object(
            link_object,
            core::ptr::null_mut(),
            desired_access,
            0,
            core::ptr::null_mut(),
            link_handle,
        );

        status
    }
}

// =============================================================================
// NtOpenSymbolicLinkObject
// =============================================================================

/// Открывает существующую символическую ссылку
pub fn nt_open_symbolic_link_object(
    link_handle: *mut PVOID,
    desired_access: u32,
    object_attributes: *mut OBJECT_ATTRIBUTES,
) -> NTSTATUS {
    use crate::nt::STATUS_INVALID_PARAMETER;

    // Проверяем параметры
    if link_handle.is_null() || object_attributes.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    // Этап 5: открытие должно возвращать реальный HANDLE_TABLE handle.
    unsafe {
        *link_handle = core::ptr::null_mut();
    }

    let symlink_type = super::init::obp_get_symbolic_link_object_type();
    super::handle::ob_open_object_by_name(
        object_attributes,
        symlink_type,
        0, // KernelMode
        core::ptr::null_mut(),
        desired_access,
        core::ptr::null_mut(),
        link_handle,
    )
}

// =============================================================================
// NtQuerySymbolicLinkObject
// =============================================================================

/// Запрашивает target символической ссылки
pub fn nt_query_symbolic_link_object(
    link_handle: PVOID,
    link_target: *mut UNICODE_STRING,
    returned_length: *mut ULONG,
) -> NTSTATUS {
    if link_handle.is_null() || link_target.is_null() {
        return crate::nt::STATUS_INVALID_PARAMETER;
    }

    unsafe {
        // Получаем объект по хэндлу
        let mut link_object: PVOID = core::ptr::null_mut();
        let link_type = super::init::obp_get_symbolic_link_object_type();

        let status = super::refcount::ob_reference_object_by_handle(
            link_handle,
            SYMBOLIC_LINK_QUERY,
            link_type,
            0, // KernelMode
            &mut link_object,
            core::ptr::null_mut(),
        );

        if status != STATUS_SUCCESS {
            return status;
        }

        let link = link_object as *mut OBJECT_SYMBOLIC_LINK;
        let target = &mut *link_target;

        // Проверяем размер буфера
        let required_length = (*link).link_target.length;

        if !returned_length.is_null() {
            *returned_length = required_length as ULONG;
        }

        let status = if target.maximum_length >= required_length {
            // Копируем target
            if !target.buffer.is_null() {
                core::ptr::copy_nonoverlapping(
                    (*link).link_target.buffer as *const u8,
                    target.buffer as *mut u8,
                    required_length as usize,
                );
                target.length = required_length;
                STATUS_SUCCESS
            } else {
                crate::nt::STATUS_INVALID_PARAMETER
            }
        } else {
            crate::nt::STATUS_BUFFER_TOO_SMALL
        };

        // Dereference объект
        super::refcount::ob_dereference_object(link_object);

        status
    }
}

// =============================================================================
// Internal Functions
// =============================================================================

/// Удаляет символическую ссылку (delete procedure)
pub unsafe extern "win64" fn obp_delete_symbolic_link_impl(object: PVOID) {
    if object.is_null() {
        return;
    }

    unsafe {
        let link = object as *mut OBJECT_SYMBOLIC_LINK;

        // Освобождаем target string
        if !(*link).link_target.buffer.is_null() {
            crate::ex::pool::ex_free_pool_with_tag(
                (*link).link_target.buffer as PVOID,
                u32::from_le_bytes(*b"tmyS"),
            );
            (*link).link_target.buffer = core::ptr::null_mut();
        }
    }
}
