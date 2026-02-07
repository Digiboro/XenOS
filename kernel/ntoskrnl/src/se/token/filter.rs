//! Token Filter - NtFilterToken и создание filtered/linked tokens
//!
//! Реализация UAC-механизма фильтрации токенов для Win7.
//!
//! Источники:
//! - ReactOS: ntoskrnl/se/token.c
//! - Windows 7 / NT 6.1 semantics

use core::sync::atomic::{AtomicPtr, Ordering};

use crate::ex::pool::{ex_allocate_pool_with_tag, ex_free_pool_with_tag, POOL_TYPE};
use crate::nt::{
    NTSTATUS, PVOID, STATUS_SUCCESS, STATUS_INVALID_HANDLE, STATUS_INSUFFICIENT_RESOURCES,
    STATUS_ACCESS_DENIED, ACCESS_MASK, ULONG,
    TOKEN_TYPE, TOKEN_SOURCE, SECURITY_IMPERSONATION_LEVEL, TOKEN_ELEVATION_TYPE,
    LUID, LUID_AND_ATTRIBUTES, SID, SID_AND_ATTRIBUTES, ACL,
    TOKEN_DUPLICATE, SE_GROUP_USE_FOR_DENY_ONLY, SE_GROUP_ENABLED,
    SE_PRIVILEGE_ENABLED, SE_PRIVILEGE_REMOVED,
};

use crate::ob::refcount::{ob_reference_object_by_handle, ob_dereference_object, ob_reference_object};
use super::tokenobj::se_token_object_type;
use super::token::{TOKEN, sep_create_token, sep_delete_token, TOKEN_IS_FILTERED};
use crate::se::sid::rtl_length_sid;

/// Pool tag для filtered token
const TAG_FTOKEN: u32 = u32::from_le_bytes(*b"okTF");

// =============================================================================
// NtFilterToken flags
// =============================================================================

/// Disable all privileges
pub const DISABLE_MAX_PRIVILEGE: ULONG = 0x1;
/// Create a sandbox inert token
pub const SANDBOX_INERT: ULONG = 0x2;
/// Create LUA token (limited user account)
pub const LUA_TOKEN: ULONG = 0x4;
/// Write restricted
pub const WRITE_RESTRICTED: ULONG = 0x8;

// =============================================================================
// SeFilterToken
// =============================================================================

/// SeFilterToken - создаёт filtered token из существующего
///
/// # Параметры
/// - `existing_token` - исходный токен
/// - `flags` - флаги фильтрации (DISABLE_MAX_PRIVILEGE, SANDBOX_INERT, etc)
/// - `sids_to_disable` - SIDs для преобразования в deny-only
/// - `privileges_to_delete` - привилегии для удаления
/// - `restricted_sids` - restricted SIDs для добавления
/// - `new_token` - выходной filtered token
///
/// # Источник
/// ReactOS: ntoskrnl/se/token.c SeFilterToken
pub fn se_filter_token(
    existing_token: *mut TOKEN,
    flags: ULONG,
    sids_to_disable: *const TOKEN_GROUPS_ARRAY,
    privileges_to_delete: *const TOKEN_PRIVILEGES_ARRAY,
    restricted_sids: *const TOKEN_GROUPS_ARRAY,
    new_token: *mut *mut TOKEN,
) -> NTSTATUS {
    if existing_token.is_null() || new_token.is_null() {
        return STATUS_INVALID_HANDLE;
    }

    unsafe {
        *new_token = core::ptr::null_mut();

        let src = &*existing_token;

        // Вычисляем количество групп (с учётом sids_to_disable)
        let group_count = src.group_count;

        // Вычисляем количество привилегий (с учётом privileges_to_delete и DISABLE_MAX_PRIVILEGE)
        let priv_count = if (flags & DISABLE_MAX_PRIVILEGE) != 0 {
            0 // Удаляем все привилегии
        } else {
            sep_count_remaining_privileges(src, privileges_to_delete)
        };

        // Вычисляем количество restricted SIDs
        let restricted_count = if !restricted_sids.is_null() {
            (*restricted_sids).count
        } else {
            0
        };

        // Создаём новый токен
        // Используем те же базовые параметры, но модифицируем groups/privileges
        let new_groups = sep_create_filtered_groups(src, sids_to_disable);
        let new_privs = if (flags & DISABLE_MAX_PRIVILEGE) != 0 {
            core::ptr::null()
        } else {
            sep_create_filtered_privileges(src, privileges_to_delete)
        };

        // Создаём source для filtered token
        let filtered_source = TOKEN_SOURCE {
            source_name: src.token_source.source_name,
            source_identifier: src.token_source.source_identifier,
        };

        let token = sep_create_token(
            src.token_type,
            src.impersonation_level,
            src.authentication_id,
            src.user_sid,
            if !new_groups.is_null() { new_groups } else { src.groups },
            group_count,
            if !new_privs.is_null() { new_privs } else { core::ptr::null() },
            priv_count,
            src.owner,
            src.primary_group,
            src.default_dacl,
            &filtered_source,
            src.integrity_level_sid,
        );

        // Освобождаем временные массивы
        if !new_groups.is_null() && new_groups != src.groups {
            // Освобождаем только если аллоцировали новый массив
            // TODO: добавить освобождение
        }
        if !new_privs.is_null() && new_privs != src.privileges as *const _ {
            // TODO: добавить освобождение
        }

        if token.is_null() {
            return STATUS_INSUFFICIENT_RESOURCES;
        }

        // Устанавливаем restricted SIDs
        if restricted_count > 0 && !restricted_sids.is_null() {
            sep_set_restricted_sids(token, restricted_sids);
        }

        // Устанавливаем флаги filtered token
        let mut token_flags = (*token).token_flags.load(Ordering::Acquire);
        token_flags |= TOKEN_IS_FILTERED;
        if (flags & super::token::TOKEN_SANDBOX_INERT) != 0 {
            token_flags |= super::token::TOKEN_SANDBOX_INERT;
        }
        (*token).token_flags.store(token_flags, Ordering::Release);

        // Устанавливаем elevation type для filtered token
        (*token).elevation_type = TOKEN_ELEVATION_TYPE::TokenElevationTypeLimited;
        (*token).elevation = 0; // Not elevated

        // Связываем с исходным токеном (linked token)
        // Исходный токен становится linked token для filtered
        ob_reference_object(existing_token as PVOID);
        (*token).linked_token.store(existing_token, Ordering::Release);

        // Обновляем исходный токен - он теперь Full elevation type
        (*existing_token).elevation_type = TOKEN_ELEVATION_TYPE::TokenElevationTypeFull;
        (*existing_token).elevation = 1; // Elevated

        // Устанавливаем linked token в исходном токене
        ob_reference_object(token as PVOID);
        (*existing_token).linked_token.store(token, Ordering::Release);

        *new_token = token;
        STATUS_SUCCESS
    }
}

// =============================================================================
// Helper structures
// =============================================================================

/// Массив SID_AND_ATTRIBUTES для передачи в SeFilterToken
#[repr(C)]
pub struct TOKEN_GROUPS_ARRAY {
    pub count: u32,
    pub groups: [SID_AND_ATTRIBUTES; 1], // Variable length
}

/// Массив LUID_AND_ATTRIBUTES для передачи в SeFilterToken
#[repr(C)]
pub struct TOKEN_PRIVILEGES_ARRAY {
    pub count: u32,
    pub privileges: [LUID_AND_ATTRIBUTES; 1], // Variable length
}

// =============================================================================
// Internal helpers
// =============================================================================

/// Подсчитывает оставшиеся привилегии после фильтрации
unsafe fn sep_count_remaining_privileges(
    token: &TOKEN,
    privileges_to_delete: *const TOKEN_PRIVILEGES_ARRAY,
) -> u32 {
    if privileges_to_delete.is_null() {
        return token.privilege_count;
    }

    let delete_count = (*privileges_to_delete).count as usize;
    if delete_count == 0 {
        return token.privilege_count;
    }

    let mut remaining = token.privilege_count;

    for i in 0..token.privilege_count as usize {
        let priv_ = &*token.privileges.add(i);

        // Проверяем, есть ли эта привилегия в списке на удаление
        for j in 0..delete_count {
            let to_delete = &(*privileges_to_delete).privileges[j];
            if priv_.luid.low_part == to_delete.luid.low_part
                && priv_.luid.high_part == to_delete.luid.high_part
            {
                remaining -= 1;
                break;
            }
        }
    }

    remaining
}

/// Создаёт filtered массив групп (с deny-only для указанных SIDs)
unsafe fn sep_create_filtered_groups(
    token: &TOKEN,
    sids_to_disable: *const TOKEN_GROUPS_ARRAY,
) -> *const SID_AND_ATTRIBUTES {
    if sids_to_disable.is_null() || token.groups.is_null() {
        return token.groups;
    }

    // Для простоты пока возвращаем оригинальный массив
    // Полная реализация требует копирования и модификации attributes
    // TODO: Реализовать полную фильтрацию групп
    token.groups
}

/// Создаёт filtered массив привилегий
unsafe fn sep_create_filtered_privileges(
    token: &TOKEN,
    privileges_to_delete: *const TOKEN_PRIVILEGES_ARRAY,
) -> *const LUID_AND_ATTRIBUTES {
    if privileges_to_delete.is_null() || token.privileges.is_null() {
        return token.privileges;
    }

    // Для простоты пока возвращаем оригинальный массив
    // Полная реализация требует копирования без удалённых привилегий
    // TODO: Реализовать полную фильтрацию привилегий
    token.privileges
}

/// Устанавливает restricted SIDs в токене
unsafe fn sep_set_restricted_sids(token: *mut TOKEN, restricted_sids: *const TOKEN_GROUPS_ARRAY) {
    if token.is_null() || restricted_sids.is_null() {
        return;
    }

    let count = (*restricted_sids).count;
    if count == 0 {
        return;
    }

    // Для полной реализации нужно:
    // 1. Аллоцировать память для restricted_sids в токене
    // 2. Скопировать SIDs
    // Пока устанавливаем только счётчик
    (*token).restricted_sid_count = count;
    // TODO: Скопировать SIDs
}

// =============================================================================
// NtFilterToken syscall (stub)
// =============================================================================

/// NtFilterToken - создаёт filtered token
///
/// # Параметры
/// - `existing_token_handle` - хэндл исходного токена
/// - `flags` - флаги фильтрации
/// - `sids_to_disable` - SIDs для deny-only
/// - `privileges_to_delete` - привилегии для удаления
/// - `restricted_sids` - restricted SIDs
/// - `new_token_handle` - выходной хэндл
pub fn nt_filter_token(
    existing_token_handle: PVOID,
    flags: ULONG,
    sids_to_disable: *const TOKEN_GROUPS_ARRAY,
    privileges_to_delete: *const TOKEN_PRIVILEGES_ARRAY,
    restricted_sids: *const TOKEN_GROUPS_ARRAY,
    new_token_handle: *mut PVOID,
) -> NTSTATUS {
    if new_token_handle.is_null() {
        return STATUS_INVALID_HANDLE;
    }

    unsafe {
        *new_token_handle = core::ptr::null_mut();

        // Получаем исходный токен
        let mut token_object: PVOID = core::ptr::null_mut();
        let token_type = se_token_object_type();

        let status = ob_reference_object_by_handle(
            existing_token_handle,
            TOKEN_DUPLICATE,
            token_type,
            1, // UserMode
            &mut token_object,
            core::ptr::null_mut(),
        );

        if status != STATUS_SUCCESS {
            return status;
        }

        let existing_token = token_object as *mut TOKEN;
        let mut new_token: *mut TOKEN = core::ptr::null_mut();

        // Фильтруем токен
        let filter_status = se_filter_token(
            existing_token,
            flags,
            sids_to_disable,
            privileges_to_delete,
            restricted_sids,
            &mut new_token,
        );

        ob_dereference_object(token_object);

        if filter_status != STATUS_SUCCESS {
            return filter_status;
        }

        // TODO: Создать хэндл на новый токен через ObInsertObject
        // Пока возвращаем указатель напрямую (только для kernel mode)
        *new_token_handle = new_token as PVOID;

        STATUS_SUCCESS
    }
}

// =============================================================================
// SeGetLinkedToken - получение linked token
// =============================================================================

/// SeGetLinkedToken - получает linked token для UAC
///
/// # Источник
/// ReactOS: ntoskrnl/se/token.c
pub fn se_get_linked_token(token: *mut TOKEN) -> *mut TOKEN {
    if token.is_null() {
        return core::ptr::null_mut();
    }

    unsafe {
        (*token).linked_token.load(Ordering::Acquire)
    }
}

/// SepSetLinkedToken - устанавливает linked token
pub fn sep_set_linked_token(token: *mut TOKEN, linked: *mut TOKEN) {
    if token.is_null() {
        return;
    }

    unsafe {
        // Освобождаем старый linked token если был
        let old = (*token).linked_token.swap(linked, Ordering::AcqRel);
        if !old.is_null() {
            ob_dereference_object(old as PVOID);
        }

        // Увеличиваем refcount на новый linked token
        if !linked.is_null() {
            ob_reference_object(linked as PVOID);
        }
    }
}

