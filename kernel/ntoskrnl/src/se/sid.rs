//! SID - Security Identifier
//!
//! Функции для работы с SID: валидация, сравнение, копирование, well-known SIDs.
//!
//! Источники:
//! - ReactOS: ntoskrnl/se/sid.c
//! - ReactOS: lib/rtl/sid.c

use core::sync::atomic::{AtomicBool, Ordering};

use crate::ex::pool::{ex_allocate_pool_with_tag, ex_free_pool_with_tag, POOL_TYPE};
use crate::nt::{
    NTSTATUS, PVOID, STATUS_INVALID_SID, STATUS_SUCCESS, UCHAR, ULONG,
    SID, SID_IDENTIFIER_AUTHORITY, SID_AND_ATTRIBUTES,
    SECURITY_NT_AUTHORITY, SECURITY_WORLD_SID_AUTHORITY,
    SECURITY_NULL_SID_AUTHORITY, SECURITY_MANDATORY_LABEL_AUTHORITY,
    SECURITY_LOCAL_SYSTEM_RID, SECURITY_ANONYMOUS_LOGON_RID,
    SECURITY_AUTHENTICATED_USER_RID, SECURITY_BUILTIN_DOMAIN_RID,
    DOMAIN_ALIAS_RID_ADMINS, SECURITY_WORLD_RID,
    SECURITY_MANDATORY_UNTRUSTED_RID, SECURITY_MANDATORY_LOW_RID,
    SECURITY_MANDATORY_MEDIUM_RID, SECURITY_MANDATORY_HIGH_RID,
    SECURITY_MANDATORY_SYSTEM_RID, SECURITY_LOCAL_SERVICE_RID,
    SECURITY_NETWORK_SERVICE_RID,
};

/// Pool tag для SID аллокаций
const TAG_SID: u32 = u32::from_le_bytes(*b"diSe");

// =============================================================================
// Well-known SIDs (глобальные, создаются при инициализации)
// =============================================================================

/// Флаг инициализации well-known SIDs
static WELL_KNOWN_SIDS_INITIALIZED: AtomicBool = AtomicBool::new(false);

// Статические буферы для well-known SIDs
// Формат: [revision, sub_authority_count, authority[6], sub_authorities...]

/// S-1-0-0 (Null SID)
static SE_NULL_SID_BUFFER: [u8; 12] = [
    1, 1, // revision=1, sub_auth_count=1
    0, 0, 0, 0, 0, 0, // NULL authority
    0, 0, 0, 0, // sub_authority[0] = 0
];

/// S-1-1-0 (Everyone / World)
static SE_WORLD_SID_BUFFER: [u8; 12] = [
    1, 1,
    0, 0, 0, 0, 0, 1, // WORLD authority
    0, 0, 0, 0, // sub_authority[0] = 0
];

/// S-1-5-7 (Anonymous Logon)
static SE_ANONYMOUS_LOGON_SID_BUFFER: [u8; 12] = [
    1, 1,
    0, 0, 0, 0, 0, 5, // NT authority
    7, 0, 0, 0, // sub_authority[0] = 7
];

/// S-1-5-11 (Authenticated Users)
static SE_AUTHENTICATED_USERS_SID_BUFFER: [u8; 12] = [
    1, 1,
    0, 0, 0, 0, 0, 5, // NT authority
    11, 0, 0, 0, // sub_authority[0] = 11
];

/// S-1-5-18 (Local System)
static SE_LOCAL_SYSTEM_SID_BUFFER: [u8; 12] = [
    1, 1,
    0, 0, 0, 0, 0, 5, // NT authority
    18, 0, 0, 0, // sub_authority[0] = 18
];

/// S-1-5-19 (Local Service)
static SE_LOCAL_SERVICE_SID_BUFFER: [u8; 12] = [
    1, 1,
    0, 0, 0, 0, 0, 5, // NT authority
    19, 0, 0, 0, // sub_authority[0] = 19
];

/// S-1-5-20 (Network Service)
static SE_NETWORK_SERVICE_SID_BUFFER: [u8; 12] = [
    1, 1,
    0, 0, 0, 0, 0, 5, // NT authority
    20, 0, 0, 0, // sub_authority[0] = 20
];

/// S-1-5-32-544 (Builtin Administrators)
static SE_ALIASED_ADMINS_SID_BUFFER: [u8; 16] = [
    1, 2, // revision=1, sub_auth_count=2
    0, 0, 0, 0, 0, 5, // NT authority
    32, 0, 0, 0, // sub_authority[0] = 32 (BUILTIN)
    0x20, 0x02, 0, 0, // sub_authority[1] = 544 (ADMINS)
];

// Integrity level SIDs (Win7 MIC)

/// S-1-16-0 (Untrusted Mandatory Level)
static SE_UNTRUSTED_MANDATORY_SID_BUFFER: [u8; 12] = [
    1, 1,
    0, 0, 0, 0, 0, 16, // Mandatory Label authority
    0, 0, 0, 0, // sub_authority[0] = 0
];

/// S-1-16-4096 (Low Mandatory Level)
static SE_LOW_MANDATORY_SID_BUFFER: [u8; 12] = [
    1, 1,
    0, 0, 0, 0, 0, 16,
    0, 0x10, 0, 0, // sub_authority[0] = 0x1000 = 4096
];

/// S-1-16-8192 (Medium Mandatory Level)
static SE_MEDIUM_MANDATORY_SID_BUFFER: [u8; 12] = [
    1, 1,
    0, 0, 0, 0, 0, 16,
    0, 0x20, 0, 0, // sub_authority[0] = 0x2000 = 8192
];

/// S-1-16-12288 (High Mandatory Level)
static SE_HIGH_MANDATORY_SID_BUFFER: [u8; 12] = [
    1, 1,
    0, 0, 0, 0, 0, 16,
    0, 0x30, 0, 0, // sub_authority[0] = 0x3000 = 12288
];

/// S-1-16-16384 (System Mandatory Level)
static SE_SYSTEM_MANDATORY_SID_BUFFER: [u8; 12] = [
    1, 1,
    0, 0, 0, 0, 0, 16,
    0, 0x40, 0, 0, // sub_authority[0] = 0x4000 = 16384
];

// =============================================================================
// Public well-known SID accessors
// =============================================================================

/// Возвращает указатель на Null SID (S-1-0-0)
#[inline]
pub fn se_null_sid() -> *const SID {
    SE_NULL_SID_BUFFER.as_ptr() as *const SID
}

/// Возвращает указатель на World/Everyone SID (S-1-1-0)
#[inline]
pub fn se_world_sid() -> *const SID {
    SE_WORLD_SID_BUFFER.as_ptr() as *const SID
}

/// Возвращает указатель на Anonymous Logon SID (S-1-5-7)
#[inline]
pub fn se_anonymous_logon_sid() -> *const SID {
    SE_ANONYMOUS_LOGON_SID_BUFFER.as_ptr() as *const SID
}

/// Возвращает указатель на Authenticated Users SID (S-1-5-11)
#[inline]
pub fn se_authenticated_users_sid() -> *const SID {
    SE_AUTHENTICATED_USERS_SID_BUFFER.as_ptr() as *const SID
}

/// Возвращает указатель на Local System SID (S-1-5-18)
#[inline]
pub fn se_local_system_sid() -> *const SID {
    SE_LOCAL_SYSTEM_SID_BUFFER.as_ptr() as *const SID
}

/// Возвращает указатель на Local Service SID (S-1-5-19)
#[inline]
pub fn se_local_service_sid() -> *const SID {
    SE_LOCAL_SERVICE_SID_BUFFER.as_ptr() as *const SID
}

/// Возвращает указатель на Network Service SID (S-1-5-20)
#[inline]
pub fn se_network_service_sid() -> *const SID {
    SE_NETWORK_SERVICE_SID_BUFFER.as_ptr() as *const SID
}

/// Возвращает указатель на Builtin Administrators SID (S-1-5-32-544)
#[inline]
pub fn se_aliased_admins_sid() -> *const SID {
    SE_ALIASED_ADMINS_SID_BUFFER.as_ptr() as *const SID
}

// Integrity level SIDs

/// Возвращает указатель на Untrusted Mandatory Level SID (S-1-16-0)
#[inline]
pub fn se_untrusted_mandatory_sid() -> *const SID {
    SE_UNTRUSTED_MANDATORY_SID_BUFFER.as_ptr() as *const SID
}

/// Возвращает указатель на Low Mandatory Level SID (S-1-16-4096)
#[inline]
pub fn se_low_mandatory_sid() -> *const SID {
    SE_LOW_MANDATORY_SID_BUFFER.as_ptr() as *const SID
}

/// Возвращает указатель на Medium Mandatory Level SID (S-1-16-8192)
#[inline]
pub fn se_medium_mandatory_sid() -> *const SID {
    SE_MEDIUM_MANDATORY_SID_BUFFER.as_ptr() as *const SID
}

/// Возвращает указатель на High Mandatory Level SID (S-1-16-12288)
#[inline]
pub fn se_high_mandatory_sid() -> *const SID {
    SE_HIGH_MANDATORY_SID_BUFFER.as_ptr() as *const SID
}

/// Возвращает указатель на System Mandatory Level SID (S-1-16-16384)
#[inline]
pub fn se_system_mandatory_sid() -> *const SID {
    SE_SYSTEM_MANDATORY_SID_BUFFER.as_ptr() as *const SID
}

// =============================================================================
// SID validation and utility functions
// =============================================================================

/// RtlValidSid - проверяет валидность SID
///
/// # Источник
/// ReactOS: lib/rtl/sid.c RtlValidSid
///
/// # Безопасность
/// Проверяет только структурную корректность, не делает probe для UserMode.
pub fn rtl_valid_sid(sid: *const SID) -> bool {
    if sid.is_null() {
        return false;
    }

    unsafe {
        // Проверяем revision
        if (*sid).revision != SID::REVISION {
            return false;
        }

        // Проверяем количество sub-authorities
        if (*sid).sub_authority_count as usize > SID::MAX_SUB_AUTHORITIES {
            return false;
        }

        true
    }
}

/// RtlLengthRequiredSid - возвращает требуемый размер SID
///
/// # Источник
/// ReactOS: lib/rtl/sid.c RtlLengthRequiredSid
#[inline]
pub const fn rtl_length_required_sid(sub_authority_count: ULONG) -> ULONG {
    SID::length_required(sub_authority_count as usize) as ULONG
}

/// RtlLengthSid - возвращает длину SID
///
/// # Источник
/// ReactOS: lib/rtl/sid.c RtlLengthSid
pub fn rtl_length_sid(sid: *const SID) -> ULONG {
    if sid.is_null() {
        return 0;
    }

    unsafe { SID::length_required((*sid).sub_authority_count as usize) as ULONG }
}

/// RtlEqualSid - сравнивает два SID
///
/// # Источник
/// ReactOS: lib/rtl/sid.c RtlEqualSid
pub fn rtl_equal_sid(sid1: *const SID, sid2: *const SID) -> bool {
    if sid1.is_null() || sid2.is_null() {
        return false;
    }

    unsafe {
        // Сравниваем revision
        if (*sid1).revision != (*sid2).revision {
            return false;
        }

        // Сравниваем количество sub-authorities
        if (*sid1).sub_authority_count != (*sid2).sub_authority_count {
            return false;
        }

        // Сравниваем authority
        if (*sid1).identifier_authority.value != (*sid2).identifier_authority.value {
            return false;
        }

        // Сравниваем sub-authorities
        let count = (*sid1).sub_authority_count as usize;
        if count > 0 {
            let sa1 = core::slice::from_raw_parts(&(*sid1).sub_authority[0], count);
            let sa2 = core::slice::from_raw_parts(&(*sid2).sub_authority[0], count);
            if sa1 != sa2 {
                return false;
            }
        }

        true
    }
}

/// RtlEqualPrefixSid - сравнивает префиксы двух SID (без последнего RID)
///
/// # Источник
/// ReactOS: lib/rtl/sid.c RtlEqualPrefixSid
pub fn rtl_equal_prefix_sid(sid1: *const SID, sid2: *const SID) -> bool {
    if sid1.is_null() || sid2.is_null() {
        return false;
    }

    unsafe {
        // Сравниваем revision
        if (*sid1).revision != (*sid2).revision {
            return false;
        }

        // Сравниваем количество sub-authorities
        if (*sid1).sub_authority_count != (*sid2).sub_authority_count {
            return false;
        }

        // Сравниваем authority
        if (*sid1).identifier_authority.value != (*sid2).identifier_authority.value {
            return false;
        }

        // Сравниваем sub-authorities кроме последнего
        let count = (*sid1).sub_authority_count as usize;
        if count > 1 {
            let sa1 = core::slice::from_raw_parts(&(*sid1).sub_authority[0], count - 1);
            let sa2 = core::slice::from_raw_parts(&(*sid2).sub_authority[0], count - 1);
            if sa1 != sa2 {
                return false;
            }
        }

        true
    }
}

/// RtlInitializeSid - инициализирует SID
///
/// # Источник
/// ReactOS: lib/rtl/sid.c RtlInitializeSid
pub fn rtl_initialize_sid(
    sid: *mut SID,
    identifier_authority: *const SID_IDENTIFIER_AUTHORITY,
    sub_authority_count: UCHAR,
) -> NTSTATUS {
    if sid.is_null() || identifier_authority.is_null() {
        return STATUS_INVALID_SID;
    }

    if sub_authority_count as usize > SID::MAX_SUB_AUTHORITIES {
        return STATUS_INVALID_SID;
    }

    unsafe {
        (*sid).revision = SID::REVISION;
        (*sid).sub_authority_count = sub_authority_count;
        (*sid).identifier_authority = *identifier_authority;

        // Обнуляем sub-authorities
        let sa_ptr = &mut (*sid).sub_authority[0] as *mut ULONG;
        for i in 0..sub_authority_count as usize {
            *sa_ptr.add(i) = 0;
        }
    }

    STATUS_SUCCESS
}

/// RtlSubAuthoritySid - возвращает указатель на sub-authority по индексу
///
/// # Источник
/// ReactOS: lib/rtl/sid.c RtlSubAuthoritySid
pub fn rtl_sub_authority_sid(sid: *mut SID, sub_authority_index: ULONG) -> *mut ULONG {
    if sid.is_null() {
        return core::ptr::null_mut();
    }

    unsafe {
        let sa_ptr = &mut (*sid).sub_authority[0] as *mut ULONG;
        sa_ptr.add(sub_authority_index as usize)
    }
}

/// RtlSubAuthorityCountSid - возвращает указатель на счетчик sub-authorities
///
/// # Источник
/// ReactOS: lib/rtl/sid.c RtlSubAuthorityCountSid
pub fn rtl_sub_authority_count_sid(sid: *mut SID) -> *mut UCHAR {
    if sid.is_null() {
        return core::ptr::null_mut();
    }

    unsafe { &mut (*sid).sub_authority_count as *mut UCHAR }
}

/// RtlIdentifierAuthoritySid - возвращает указатель на identifier authority
///
/// # Источник
/// ReactOS: lib/rtl/sid.c RtlIdentifierAuthoritySid
pub fn rtl_identifier_authority_sid(sid: *mut SID) -> *mut SID_IDENTIFIER_AUTHORITY {
    if sid.is_null() {
        return core::ptr::null_mut();
    }

    unsafe { &mut (*sid).identifier_authority as *mut SID_IDENTIFIER_AUTHORITY }
}

/// RtlCopySid - копирует SID
///
/// # Источник
/// ReactOS: lib/rtl/sid.c RtlCopySid
pub fn rtl_copy_sid(destination_length: ULONG, destination: *mut SID, source: *const SID) -> NTSTATUS {
    if destination.is_null() || source.is_null() {
        return STATUS_INVALID_SID;
    }

    let source_length = rtl_length_sid(source);
    if destination_length < source_length {
        return STATUS_INVALID_SID;
    }

    unsafe {
        core::ptr::copy_nonoverlapping(source as *const u8, destination as *mut u8, source_length as usize);
    }

    STATUS_SUCCESS
}

/// SepSidInToken - проверяет, содержит ли токен указанный SID
///
/// Внутренняя функция SE для проверки членства в группе.
///
/// # Параметры
/// - `sid_array` - массив SID_AND_ATTRIBUTES из токена
/// - `sid_count` - количество элементов в массиве
/// - `sid` - искомый SID
/// - `deny_only` - если true, ищет только deny-only группы
///
/// # Возвращает
/// true если SID найден (и соответствует deny_only требованию)
pub fn sep_sid_in_token(
    sid_array: *const SID_AND_ATTRIBUTES,
    sid_count: ULONG,
    sid: *const SID,
    deny_only: bool,
) -> bool {
    use crate::nt::{SE_GROUP_ENABLED, SE_GROUP_USE_FOR_DENY_ONLY};

    if sid_array.is_null() || sid.is_null() || sid_count == 0 {
        return false;
    }

    unsafe {
        for i in 0..sid_count as usize {
            let entry = &*sid_array.add(i);

            // Проверяем атрибуты
            if deny_only {
                // Ищем deny-only группы
                if (entry.attributes & SE_GROUP_USE_FOR_DENY_ONLY) == 0 {
                    continue;
                }
            } else {
                // Ищем enabled группы (не deny-only)
                if (entry.attributes & SE_GROUP_ENABLED) == 0 {
                    continue;
                }
                if (entry.attributes & SE_GROUP_USE_FOR_DENY_ONLY) != 0 {
                    continue;
                }
            }

            // Сравниваем SID
            if rtl_equal_sid(entry.sid, sid) {
                return true;
            }
        }
    }

    false
}

/// Аллоцирует и копирует SID
///
/// # Параметры
/// - `source` - исходный SID
/// - `pool_type` - тип пула для аллокации
///
/// # Возвращает
/// Указатель на копию SID или null при ошибке
pub fn se_capture_sid(source: *const SID, pool_type: POOL_TYPE) -> *mut SID {
    if source.is_null() || !rtl_valid_sid(source) {
        return core::ptr::null_mut();
    }

    let length = rtl_length_sid(source) as usize;
    let dest = ex_allocate_pool_with_tag(pool_type, length, TAG_SID);
    if dest.is_null() {
        return core::ptr::null_mut();
    }

    unsafe {
        core::ptr::copy_nonoverlapping(source as *const u8, dest as *mut u8, length);
    }

    dest as *mut SID
}

/// Освобождает SID, выделенный через se_capture_sid
pub fn se_release_sid(sid: *mut SID) {
    if !sid.is_null() {
        ex_free_pool_with_tag(sid as PVOID, TAG_SID);
    }
}

/// Возвращает RID (последний sub-authority) из SID
pub fn rtl_get_rid(sid: *const SID) -> ULONG {
    if sid.is_null() {
        return 0;
    }

    unsafe {
        let count = (*sid).sub_authority_count as usize;
        if count == 0 {
            return 0;
        }

        let sa_ptr = &(*sid).sub_authority[0] as *const ULONG;
        *sa_ptr.add(count - 1)
    }
}

/// Проверяет, является ли SID integrity level SID
pub fn sep_is_integrity_sid(sid: *const SID) -> bool {
    if sid.is_null() || !rtl_valid_sid(sid) {
        return false;
    }

    unsafe {
        // Проверяем authority = SECURITY_MANDATORY_LABEL_AUTHORITY
        if (*sid).identifier_authority.value != SECURITY_MANDATORY_LABEL_AUTHORITY.value {
            return false;
        }

        // Должен быть ровно 1 sub-authority
        (*sid).sub_authority_count == 1
    }
}

/// Извлекает integrity level из integrity SID
///
/// # Возвращает
/// Integrity level (SECURITY_MANDATORY_*_RID) или 0 если SID не является integrity SID
pub fn sep_get_integrity_level(sid: *const SID) -> ULONG {
    if !sep_is_integrity_sid(sid) {
        return 0;
    }

    rtl_get_rid(sid)
}

