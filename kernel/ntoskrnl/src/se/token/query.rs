//! Token Query - NtQueryInformationToken
//!
//! Реализация syscall для запроса информации о токене.
//!
//! Источники:
//! - ReactOS: ntoskrnl/se/token.c
//! - Windows 7 / NT 6.1 semantics

use crate::nt::{
    NTSTATUS, PVOID, STATUS_SUCCESS, STATUS_INVALID_HANDLE, STATUS_INVALID_INFO_CLASS,
    STATUS_BUFFER_TOO_SMALL, STATUS_ACCESS_DENIED, ACCESS_MASK, ULONG, BOOLEAN,
    TOKEN_INFORMATION_CLASS, TOKEN_QUERY, TOKEN_QUERY_SOURCE,
    TOKEN_USER, TOKEN_GROUPS, TOKEN_PRIVILEGES, TOKEN_OWNER, TOKEN_PRIMARY_GROUP,
    TOKEN_DEFAULT_DACL, TOKEN_SOURCE, TOKEN_TYPE, TOKEN_STATISTICS,
    TOKEN_MANDATORY_LABEL, TOKEN_ELEVATION, TOKEN_ELEVATION_TYPE,
    TOKEN_MANDATORY_POLICY, TOKEN_MANDATORY_POLICY_NO_WRITE_UP,
    SID, SID_AND_ATTRIBUTES, LUID_AND_ATTRIBUTES, ACL, SECURITY_IMPERSONATION_LEVEL,
};

use crate::ob::refcount::{ob_reference_object_by_handle, ob_dereference_object};
use super::tokenobj::se_token_object_type;
use super::token::TOKEN;
use crate::se::sid::rtl_length_sid;

// =============================================================================
// NtQueryInformationToken
// =============================================================================

/// NtQueryInformationToken - запрос информации о токене
///
/// # Параметры
/// - `token_handle` - хэндл токена с правами TOKEN_QUERY
/// - `token_information_class` - класс запрашиваемой информации
/// - `token_information` - выходной буфер
/// - `token_information_length` - размер буфера
/// - `return_length` - требуемый размер (выходной)
///
/// # Возвращает
/// - `STATUS_SUCCESS` - успех
/// - `STATUS_BUFFER_TOO_SMALL` - буфер слишком мал
/// - `STATUS_ACCESS_DENIED` - нет прав TOKEN_QUERY
/// - `STATUS_INVALID_HANDLE` - невалидный хэндл
/// - `STATUS_INVALID_INFO_CLASS` - неподдерживаемый класс
///
/// # Источник
/// ReactOS: ntoskrnl/se/token.c NtQueryInformationToken
pub fn nt_query_information_token(
    token_handle: PVOID,
    token_information_class: TOKEN_INFORMATION_CLASS,
    token_information: PVOID,
    token_information_length: ULONG,
    return_length: *mut ULONG,
) -> NTSTATUS {
    // Определяем требуемые права доступа
    let required_access = match token_information_class {
        TOKEN_INFORMATION_CLASS::TokenSource => TOKEN_QUERY_SOURCE,
        _ => TOKEN_QUERY,
    };

    // Получаем объект токена по хэндлу
    let mut token_object: PVOID = core::ptr::null_mut();
    let token_type = se_token_object_type();

    let status = unsafe {
        ob_reference_object_by_handle(
            token_handle,
            required_access,
            token_type,
            1, // UserMode - всегда проверяем права
            &mut token_object,
            core::ptr::null_mut(),
        )
    };

    if status != STATUS_SUCCESS {
        return status;
    }

    let token = token_object as *mut TOKEN;

    // Запрашиваем информацию
    let result = unsafe {
        sep_query_information_token(
            token,
            token_information_class,
            token_information,
            token_information_length,
            return_length,
        )
    };

    // Освобождаем ссылку на токен
    unsafe { ob_dereference_object(token_object) };

    result
}

/// SeQueryInformationToken - внутренняя функция запроса информации (KernelMode)
///
/// Используется драйверами без проверки прав на хэндл.
pub fn se_query_information_token(
    token: *mut TOKEN,
    token_information_class: TOKEN_INFORMATION_CLASS,
    token_information: PVOID,
    token_information_length: ULONG,
    return_length: *mut ULONG,
) -> NTSTATUS {
    if token.is_null() {
        return STATUS_INVALID_HANDLE;
    }

    unsafe {
        sep_query_information_token(
            token,
            token_information_class,
            token_information,
            token_information_length,
            return_length,
        )
    }
}

/// Внутренняя функция запроса информации о токене
unsafe fn sep_query_information_token(
    token: *mut TOKEN,
    token_information_class: TOKEN_INFORMATION_CLASS,
    token_information: PVOID,
    token_information_length: ULONG,
    return_length: *mut ULONG,
) -> NTSTATUS {
    use TOKEN_INFORMATION_CLASS::*;

    // Вычисляем требуемый размер и копируем данные
    match token_information_class {
        TokenUser => query_token_user(token, token_information, token_information_length, return_length),
        TokenGroups => query_token_groups(token, token_information, token_information_length, return_length),
        TokenPrivileges => query_token_privileges(token, token_information, token_information_length, return_length),
        TokenOwner => query_token_owner(token, token_information, token_information_length, return_length),
        TokenPrimaryGroup => query_token_primary_group(token, token_information, token_information_length, return_length),
        TokenDefaultDacl => query_token_default_dacl(token, token_information, token_information_length, return_length),
        TokenSource => query_token_source(token, token_information, token_information_length, return_length),
        TokenType => query_token_type(token, token_information, token_information_length, return_length),
        TokenImpersonationLevel => query_token_impersonation_level(token, token_information, token_information_length, return_length),
        TokenStatistics => query_token_statistics(token, token_information, token_information_length, return_length),
        TokenRestrictedSids => query_token_restricted_sids(token, token_information, token_information_length, return_length),
        TokenSessionId => query_token_session_id(token, token_information, token_information_length, return_length),
        TokenIntegrityLevel => query_token_integrity_level(token, token_information, token_information_length, return_length),
        TokenElevationType => query_token_elevation_type(token, token_information, token_information_length, return_length),
        TokenElevation => query_token_elevation(token, token_information, token_information_length, return_length),
        TokenHasRestrictions => query_token_has_restrictions(token, token_information, token_information_length, return_length),
        TokenMandatoryPolicy => query_token_mandatory_policy(token, token_information, token_information_length, return_length),
        TokenLinkedToken => query_token_linked_token(token, token_information, token_information_length, return_length),
        TokenVirtualizationAllowed => query_token_virtualization_allowed(token, token_information, token_information_length, return_length),
        TokenVirtualizationEnabled => query_token_virtualization_enabled(token, token_information, token_information_length, return_length),
        TokenUIAccess => query_token_ui_access(token, token_information, token_information_length, return_length),
        _ => STATUS_INVALID_INFO_CLASS,
    }
}

// =============================================================================
// Query handlers for each TOKEN_INFORMATION_CLASS
// =============================================================================

/// TokenUser - информация о пользователе
unsafe fn query_token_user(
    token: *mut TOKEN,
    buffer: PVOID,
    length: ULONG,
    return_length: *mut ULONG,
) -> NTSTATUS {
    let user_sid = (*token).user_sid;
    if user_sid.is_null() {
        return STATUS_INVALID_HANDLE;
    }

    let sid_length = rtl_length_sid(user_sid) as usize;
    let required = core::mem::size_of::<TOKEN_USER>() + sid_length;

    if !return_length.is_null() {
        *return_length = required as ULONG;
    }

    if length < required as ULONG {
        return STATUS_BUFFER_TOO_SMALL;
    }

    if buffer.is_null() {
        return STATUS_BUFFER_TOO_SMALL;
    }

    // Копируем структуру и SID
    let out = buffer as *mut TOKEN_USER;
    let sid_dest = (buffer as *mut u8).add(core::mem::size_of::<TOKEN_USER>());

    // SID копируется сразу после структуры
    core::ptr::copy_nonoverlapping(user_sid as *const u8, sid_dest, sid_length);
    
    // Указатель в структуре указывает на скопированный SID
    (*out).user.sid = sid_dest as *mut SID;
    (*out).user.attributes = 0; // User SID всегда имеет 0 attributes

    STATUS_SUCCESS
}

/// TokenGroups - группы токена
unsafe fn query_token_groups(
    token: *mut TOKEN,
    buffer: PVOID,
    length: ULONG,
    return_length: *mut ULONG,
) -> NTSTATUS {
    let group_count = (*token).group_count as usize;
    let groups = (*token).groups;

    // Вычисляем размер
    let header_size = core::mem::size_of::<ULONG>(); // group_count
    let array_size = group_count * core::mem::size_of::<SID_AND_ATTRIBUTES>();
    
    let mut sids_size = 0usize;
    if !groups.is_null() {
        for i in 0..group_count {
            let g = &*groups.add(i);
            if !g.sid.is_null() {
                sids_size += rtl_length_sid(g.sid) as usize;
            }
        }
    }

    let required = header_size + array_size + sids_size;

    if !return_length.is_null() {
        *return_length = required as ULONG;
    }

    if length < required as ULONG {
        return STATUS_BUFFER_TOO_SMALL;
    }

    if buffer.is_null() {
        return STATUS_BUFFER_TOO_SMALL;
    }

    // Копируем данные
    let out_count = buffer as *mut ULONG;
    *out_count = group_count as ULONG;

    let out_array = (buffer as *mut u8).add(header_size) as *mut SID_AND_ATTRIBUTES;
    let mut sid_dest = (buffer as *mut u8).add(header_size + array_size);

    if !groups.is_null() {
        for i in 0..group_count {
            let src = &*groups.add(i);
            let dst = &mut *out_array.add(i);

            dst.attributes = src.attributes;

            if !src.sid.is_null() {
                let sid_len = rtl_length_sid(src.sid) as usize;
                core::ptr::copy_nonoverlapping(src.sid as *const u8, sid_dest, sid_len);
                dst.sid = sid_dest as *mut SID;
                sid_dest = sid_dest.add(sid_len);
            } else {
                dst.sid = core::ptr::null_mut();
            }
        }
    }

    STATUS_SUCCESS
}

/// TokenPrivileges - привилегии токена
unsafe fn query_token_privileges(
    token: *mut TOKEN,
    buffer: PVOID,
    length: ULONG,
    return_length: *mut ULONG,
) -> NTSTATUS {
    let priv_count = (*token).privilege_count as usize;
    let privileges = (*token).privileges;

    let header_size = core::mem::size_of::<ULONG>(); // privilege_count
    let array_size = priv_count * core::mem::size_of::<LUID_AND_ATTRIBUTES>();
    let required = header_size + array_size;

    if !return_length.is_null() {
        *return_length = required as ULONG;
    }

    if length < required as ULONG {
        return STATUS_BUFFER_TOO_SMALL;
    }

    if buffer.is_null() {
        return STATUS_BUFFER_TOO_SMALL;
    }

    // Копируем данные
    let out_count = buffer as *mut ULONG;
    *out_count = priv_count as ULONG;

    let out_array = (buffer as *mut u8).add(header_size) as *mut LUID_AND_ATTRIBUTES;

    if !privileges.is_null() && priv_count > 0 {
        core::ptr::copy_nonoverlapping(
            privileges,
            out_array,
            priv_count,
        );
    }

    STATUS_SUCCESS
}

/// TokenOwner - владелец по умолчанию
unsafe fn query_token_owner(
    token: *mut TOKEN,
    buffer: PVOID,
    length: ULONG,
    return_length: *mut ULONG,
) -> NTSTATUS {
    let owner = (*token).owner;
    if owner.is_null() {
        // Если owner не установлен, используем user
        return query_token_owner_from_sid((*token).user_sid, buffer, length, return_length);
    }
    query_token_owner_from_sid(owner, buffer, length, return_length)
}

unsafe fn query_token_owner_from_sid(
    owner: *const SID,
    buffer: PVOID,
    length: ULONG,
    return_length: *mut ULONG,
) -> NTSTATUS {
    if owner.is_null() {
        return STATUS_INVALID_HANDLE;
    }

    let sid_length = rtl_length_sid(owner) as usize;
    let required = core::mem::size_of::<TOKEN_OWNER>() + sid_length;

    if !return_length.is_null() {
        *return_length = required as ULONG;
    }

    if length < required as ULONG {
        return STATUS_BUFFER_TOO_SMALL;
    }

    if buffer.is_null() {
        return STATUS_BUFFER_TOO_SMALL;
    }

    let out = buffer as *mut TOKEN_OWNER;
    let sid_dest = (buffer as *mut u8).add(core::mem::size_of::<TOKEN_OWNER>());

    core::ptr::copy_nonoverlapping(owner as *const u8, sid_dest, sid_length);
    (*out).owner = sid_dest as *mut SID;

    STATUS_SUCCESS
}

/// TokenPrimaryGroup - первичная группа
unsafe fn query_token_primary_group(
    token: *mut TOKEN,
    buffer: PVOID,
    length: ULONG,
    return_length: *mut ULONG,
) -> NTSTATUS {
    let primary_group = (*token).primary_group;
    if primary_group.is_null() {
        return STATUS_INVALID_HANDLE;
    }

    let sid_length = rtl_length_sid(primary_group) as usize;
    let required = core::mem::size_of::<TOKEN_PRIMARY_GROUP>() + sid_length;

    if !return_length.is_null() {
        *return_length = required as ULONG;
    }

    if length < required as ULONG {
        return STATUS_BUFFER_TOO_SMALL;
    }

    if buffer.is_null() {
        return STATUS_BUFFER_TOO_SMALL;
    }

    let out = buffer as *mut TOKEN_PRIMARY_GROUP;
    let sid_dest = (buffer as *mut u8).add(core::mem::size_of::<TOKEN_PRIMARY_GROUP>());

    core::ptr::copy_nonoverlapping(primary_group as *const u8, sid_dest, sid_length);
    (*out).primary_group = sid_dest as *mut SID;

    STATUS_SUCCESS
}

/// TokenDefaultDacl - DACL по умолчанию
unsafe fn query_token_default_dacl(
    token: *mut TOKEN,
    buffer: PVOID,
    length: ULONG,
    return_length: *mut ULONG,
) -> NTSTATUS {
    let default_dacl = (*token).default_dacl;
    
    let dacl_size = if !default_dacl.is_null() {
        (*default_dacl).acl_size as usize
    } else {
        0
    };

    let required = core::mem::size_of::<TOKEN_DEFAULT_DACL>() + dacl_size;

    if !return_length.is_null() {
        *return_length = required as ULONG;
    }

    if length < required as ULONG {
        return STATUS_BUFFER_TOO_SMALL;
    }

    if buffer.is_null() {
        return STATUS_BUFFER_TOO_SMALL;
    }

    let out = buffer as *mut TOKEN_DEFAULT_DACL;

    if !default_dacl.is_null() {
        let dacl_dest = (buffer as *mut u8).add(core::mem::size_of::<TOKEN_DEFAULT_DACL>());
        core::ptr::copy_nonoverlapping(default_dacl as *const u8, dacl_dest, dacl_size);
        (*out).default_dacl = dacl_dest as *mut ACL;
    } else {
        (*out).default_dacl = core::ptr::null_mut();
    }

    STATUS_SUCCESS
}

/// TokenSource - источник токена
unsafe fn query_token_source(
    token: *mut TOKEN,
    buffer: PVOID,
    length: ULONG,
    return_length: *mut ULONG,
) -> NTSTATUS {
    let required = core::mem::size_of::<TOKEN_SOURCE>();

    if !return_length.is_null() {
        *return_length = required as ULONG;
    }

    if length < required as ULONG {
        return STATUS_BUFFER_TOO_SMALL;
    }

    if buffer.is_null() {
        return STATUS_BUFFER_TOO_SMALL;
    }

    let out = buffer as *mut TOKEN_SOURCE;
    *out = (*token).token_source;

    STATUS_SUCCESS
}

/// TokenType - тип токена (Primary/Impersonation)
unsafe fn query_token_type(
    token: *mut TOKEN,
    buffer: PVOID,
    length: ULONG,
    return_length: *mut ULONG,
) -> NTSTATUS {
    let required = core::mem::size_of::<TOKEN_TYPE>();

    if !return_length.is_null() {
        *return_length = required as ULONG;
    }

    if length < required as ULONG {
        return STATUS_BUFFER_TOO_SMALL;
    }

    if buffer.is_null() {
        return STATUS_BUFFER_TOO_SMALL;
    }

    let out = buffer as *mut TOKEN_TYPE;
    *out = (*token).token_type;

    STATUS_SUCCESS
}

/// TokenImpersonationLevel - уровень impersonation
unsafe fn query_token_impersonation_level(
    token: *mut TOKEN,
    buffer: PVOID,
    length: ULONG,
    return_length: *mut ULONG,
) -> NTSTATUS {
    // Только для impersonation tokens
    if (*token).token_type != TOKEN_TYPE::TokenImpersonation {
        return crate::nt::STATUS_INVALID_INFO_CLASS;
    }

    let required = core::mem::size_of::<SECURITY_IMPERSONATION_LEVEL>();

    if !return_length.is_null() {
        *return_length = required as ULONG;
    }

    if length < required as ULONG {
        return STATUS_BUFFER_TOO_SMALL;
    }

    if buffer.is_null() {
        return STATUS_BUFFER_TOO_SMALL;
    }

    let out = buffer as *mut SECURITY_IMPERSONATION_LEVEL;
    *out = (*token).impersonation_level;

    STATUS_SUCCESS
}

/// TokenStatistics - статистика токена
unsafe fn query_token_statistics(
    token: *mut TOKEN,
    buffer: PVOID,
    length: ULONG,
    return_length: *mut ULONG,
) -> NTSTATUS {
    let required = core::mem::size_of::<TOKEN_STATISTICS>();

    if !return_length.is_null() {
        *return_length = required as ULONG;
    }

    if length < required as ULONG {
        return STATUS_BUFFER_TOO_SMALL;
    }

    if buffer.is_null() {
        return STATUS_BUFFER_TOO_SMALL;
    }

    let out = buffer as *mut TOKEN_STATISTICS;
    *out = (*token).get_statistics();

    STATUS_SUCCESS
}

/// TokenRestrictedSids - restricted SIDs
unsafe fn query_token_restricted_sids(
    token: *mut TOKEN,
    buffer: PVOID,
    length: ULONG,
    return_length: *mut ULONG,
) -> NTSTATUS {
    let count = (*token).restricted_sid_count as usize;
    let sids = (*token).restricted_sids;

    let header_size = core::mem::size_of::<ULONG>();
    let array_size = count * core::mem::size_of::<SID_AND_ATTRIBUTES>();
    
    let mut sids_size = 0usize;
    if !sids.is_null() {
        for i in 0..count {
            let s = &*sids.add(i);
            if !s.sid.is_null() {
                sids_size += rtl_length_sid(s.sid) as usize;
            }
        }
    }

    let required = header_size + array_size + sids_size;

    if !return_length.is_null() {
        *return_length = required as ULONG;
    }

    if length < required as ULONG {
        return STATUS_BUFFER_TOO_SMALL;
    }

    if buffer.is_null() {
        return STATUS_BUFFER_TOO_SMALL;
    }

    let out_count = buffer as *mut ULONG;
    *out_count = count as ULONG;

    let out_array = (buffer as *mut u8).add(header_size) as *mut SID_AND_ATTRIBUTES;
    let mut sid_dest = (buffer as *mut u8).add(header_size + array_size);

    if !sids.is_null() {
        for i in 0..count {
            let src = &*sids.add(i);
            let dst = &mut *out_array.add(i);

            dst.attributes = src.attributes;

            if !src.sid.is_null() {
                let sid_len = rtl_length_sid(src.sid) as usize;
                core::ptr::copy_nonoverlapping(src.sid as *const u8, sid_dest, sid_len);
                dst.sid = sid_dest as *mut SID;
                sid_dest = sid_dest.add(sid_len);
            } else {
                dst.sid = core::ptr::null_mut();
            }
        }
    }

    STATUS_SUCCESS
}

/// TokenSessionId - ID сессии
unsafe fn query_token_session_id(
    token: *mut TOKEN,
    buffer: PVOID,
    length: ULONG,
    return_length: *mut ULONG,
) -> NTSTATUS {
    let required = core::mem::size_of::<ULONG>();

    if !return_length.is_null() {
        *return_length = required as ULONG;
    }

    if length < required as ULONG {
        return STATUS_BUFFER_TOO_SMALL;
    }

    if buffer.is_null() {
        return STATUS_BUFFER_TOO_SMALL;
    }

    let out = buffer as *mut ULONG;
    *out = (*token).session_id;

    STATUS_SUCCESS
}

/// TokenIntegrityLevel - уровень целостности (MIC)
unsafe fn query_token_integrity_level(
    token: *mut TOKEN,
    buffer: PVOID,
    length: ULONG,
    return_length: *mut ULONG,
) -> NTSTATUS {
    let integrity_sid = (*token).integrity_level_sid;
    
    // Если нет integrity SID, создаём Medium по умолчанию
    let (sid_ptr, sid_length) = if !integrity_sid.is_null() {
        (integrity_sid, rtl_length_sid(integrity_sid) as usize)
    } else {
        // Используем Medium integrity как default
        let medium_sid = crate::se::sid::se_medium_mandatory_sid();
        (medium_sid as *mut SID, rtl_length_sid(medium_sid) as usize)
    };

    let required = core::mem::size_of::<TOKEN_MANDATORY_LABEL>() + sid_length;

    if !return_length.is_null() {
        *return_length = required as ULONG;
    }

    if length < required as ULONG {
        return STATUS_BUFFER_TOO_SMALL;
    }

    if buffer.is_null() {
        return STATUS_BUFFER_TOO_SMALL;
    }

    let out = buffer as *mut TOKEN_MANDATORY_LABEL;
    let sid_dest = (buffer as *mut u8).add(core::mem::size_of::<TOKEN_MANDATORY_LABEL>());

    core::ptr::copy_nonoverlapping(sid_ptr as *const u8, sid_dest, sid_length);
    (*out).label.sid = sid_dest as *mut SID;
    (*out).label.attributes = crate::nt::SE_GROUP_INTEGRITY | crate::nt::SE_GROUP_INTEGRITY_ENABLED;

    STATUS_SUCCESS
}

/// TokenElevationType - тип elevation (UAC)
unsafe fn query_token_elevation_type(
    token: *mut TOKEN,
    buffer: PVOID,
    length: ULONG,
    return_length: *mut ULONG,
) -> NTSTATUS {
    let required = core::mem::size_of::<TOKEN_ELEVATION_TYPE>();

    if !return_length.is_null() {
        *return_length = required as ULONG;
    }

    if length < required as ULONG {
        return STATUS_BUFFER_TOO_SMALL;
    }

    if buffer.is_null() {
        return STATUS_BUFFER_TOO_SMALL;
    }

    let out = buffer as *mut TOKEN_ELEVATION_TYPE;
    *out = (*token).elevation_type;

    STATUS_SUCCESS
}

/// TokenElevation - состояние elevation
unsafe fn query_token_elevation(
    token: *mut TOKEN,
    buffer: PVOID,
    length: ULONG,
    return_length: *mut ULONG,
) -> NTSTATUS {
    let required = core::mem::size_of::<TOKEN_ELEVATION>();

    if !return_length.is_null() {
        *return_length = required as ULONG;
    }

    if length < required as ULONG {
        return STATUS_BUFFER_TOO_SMALL;
    }

    if buffer.is_null() {
        return STATUS_BUFFER_TOO_SMALL;
    }

    let out = buffer as *mut TOKEN_ELEVATION;
    (*out).token_is_elevated = (*token).elevation;

    STATUS_SUCCESS
}

/// TokenHasRestrictions - есть ли ограничения
unsafe fn query_token_has_restrictions(
    token: *mut TOKEN,
    buffer: PVOID,
    length: ULONG,
    return_length: *mut ULONG,
) -> NTSTATUS {
    let required = core::mem::size_of::<ULONG>();

    if !return_length.is_null() {
        *return_length = required as ULONG;
    }

    if length < required as ULONG {
        return STATUS_BUFFER_TOO_SMALL;
    }

    if buffer.is_null() {
        return STATUS_BUFFER_TOO_SMALL;
    }

    let out = buffer as *mut ULONG;
    *out = if (*token).is_restricted() { 1 } else { 0 };

    STATUS_SUCCESS
}

/// TokenMandatoryPolicy - политика MIC
unsafe fn query_token_mandatory_policy(
    token: *mut TOKEN,
    buffer: PVOID,
    length: ULONG,
    return_length: *mut ULONG,
) -> NTSTATUS {
    let required = core::mem::size_of::<TOKEN_MANDATORY_POLICY>();

    if !return_length.is_null() {
        *return_length = required as ULONG;
    }

    if length < required as ULONG {
        return STATUS_BUFFER_TOO_SMALL;
    }

    if buffer.is_null() {
        return STATUS_BUFFER_TOO_SMALL;
    }

    let out = buffer as *mut TOKEN_MANDATORY_POLICY;
    // По умолчанию: NO_WRITE_UP policy
    (*out).policy = TOKEN_MANDATORY_POLICY_NO_WRITE_UP;

    STATUS_SUCCESS
}

/// TokenLinkedToken - связанный токен (UAC)
unsafe fn query_token_linked_token(
    token: *mut TOKEN,
    buffer: PVOID,
    length: ULONG,
    return_length: *mut ULONG,
) -> NTSTATUS {
    use core::sync::atomic::Ordering;

    // Требуемый размер - HANDLE (указатель)
    let required = core::mem::size_of::<PVOID>();

    if !return_length.is_null() {
        *return_length = required as ULONG;
    }

    if length < required as ULONG {
        return STATUS_BUFFER_TOO_SMALL;
    }

    if buffer.is_null() {
        return STATUS_BUFFER_TOO_SMALL;
    }

    let linked = (*token).linked_token.load(Ordering::Acquire);
    
    // Если есть linked token, нужно создать хэндл для него
    // Пока возвращаем NULL - полная реализация требует создания хэндла
    let out = buffer as *mut PVOID;
    *out = core::ptr::null_mut(); // TODO: создать хэндл на linked token

    STATUS_SUCCESS
}

/// TokenVirtualizationAllowed
unsafe fn query_token_virtualization_allowed(
    token: *mut TOKEN,
    buffer: PVOID,
    length: ULONG,
    return_length: *mut ULONG,
) -> NTSTATUS {
    use super::token::TOKEN_VIRTUALIZE_ALLOWED;
    use core::sync::atomic::Ordering;

    let required = core::mem::size_of::<ULONG>();

    if !return_length.is_null() {
        *return_length = required as ULONG;
    }

    if length < required as ULONG {
        return STATUS_BUFFER_TOO_SMALL;
    }

    if buffer.is_null() {
        return STATUS_BUFFER_TOO_SMALL;
    }

    let flags = (*token).token_flags.load(Ordering::Acquire);
    let out = buffer as *mut ULONG;
    *out = if (flags & TOKEN_VIRTUALIZE_ALLOWED) != 0 { 1 } else { 0 };

    STATUS_SUCCESS
}

/// TokenVirtualizationEnabled
unsafe fn query_token_virtualization_enabled(
    token: *mut TOKEN,
    buffer: PVOID,
    length: ULONG,
    return_length: *mut ULONG,
) -> NTSTATUS {
    use super::token::TOKEN_VIRTUALIZE_ENABLED;
    use core::sync::atomic::Ordering;

    let required = core::mem::size_of::<ULONG>();

    if !return_length.is_null() {
        *return_length = required as ULONG;
    }

    if length < required as ULONG {
        return STATUS_BUFFER_TOO_SMALL;
    }

    if buffer.is_null() {
        return STATUS_BUFFER_TOO_SMALL;
    }

    let flags = (*token).token_flags.load(Ordering::Acquire);
    let out = buffer as *mut ULONG;
    *out = if (flags & TOKEN_VIRTUALIZE_ENABLED) != 0 { 1 } else { 0 };

    STATUS_SUCCESS
}

/// TokenUIAccess
unsafe fn query_token_ui_access(
    token: *mut TOKEN,
    buffer: PVOID,
    length: ULONG,
    return_length: *mut ULONG,
) -> NTSTATUS {
    use super::token::TOKEN_UIACCESS;
    use core::sync::atomic::Ordering;

    let required = core::mem::size_of::<ULONG>();

    if !return_length.is_null() {
        *return_length = required as ULONG;
    }

    if length < required as ULONG {
        return STATUS_BUFFER_TOO_SMALL;
    }

    if buffer.is_null() {
        return STATUS_BUFFER_TOO_SMALL;
    }

    let flags = (*token).token_flags.load(Ordering::Acquire);
    let out = buffer as *mut ULONG;
    *out = if (flags & TOKEN_UIACCESS) != 0 { 1 } else { 0 };

    STATUS_SUCCESS
}

