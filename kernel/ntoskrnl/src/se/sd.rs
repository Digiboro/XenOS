//! SD - Security Descriptor
//!
//! Функции для работы с Security Descriptor: валидация, capture, query/set.
//!
//! Источники:
//! - ReactOS: ntoskrnl/se/sd.c
//! - ReactOS: lib/rtl/sd.c

use crate::ex::pool::{ex_allocate_pool_with_tag, ex_free_pool_with_tag, POOL_TYPE};
use crate::nt::{
    ACCESS_MASK, BOOLEAN, NTSTATUS, PVOID, STATUS_INVALID_SECURITY_DESCR, STATUS_SUCCESS,
    STATUS_BUFFER_TOO_SMALL, UCHAR, ULONG,
    ACL, SID, SECURITY_DESCRIPTOR, SECURITY_DESCRIPTOR_RELATIVE, SECURITY_DESCRIPTOR_CONTROL,
    SECURITY_INFORMATION, SE_SELF_RELATIVE, SE_DACL_PRESENT, SE_SACL_PRESENT,
    SE_OWNER_DEFAULTED, SE_GROUP_DEFAULTED, SE_DACL_DEFAULTED, SE_SACL_DEFAULTED,
    OWNER_SECURITY_INFORMATION, GROUP_SECURITY_INFORMATION,
    DACL_SECURITY_INFORMATION, SACL_SECURITY_INFORMATION,
};

use super::sid::{rtl_valid_sid, rtl_length_sid, rtl_copy_sid};
use super::acl::rtl_valid_acl;

/// Pool tag для Security Descriptor аллокаций
const TAG_SD: u32 = u32::from_le_bytes(*b"dSeS");

// =============================================================================
// Security Descriptor validation
// =============================================================================

/// RtlValidSecurityDescriptor - проверяет валидность SD
///
/// # Источник
/// ReactOS: lib/rtl/sd.c RtlValidSecurityDescriptor
pub fn rtl_valid_security_descriptor(sd: *const SECURITY_DESCRIPTOR) -> bool {
    if sd.is_null() {
        return false;
    }

    unsafe {
        // Проверяем revision
        if (*sd).revision != SECURITY_DESCRIPTOR::REVISION {
            return false;
        }

        // Для self-relative SD проверяем через другой путь
        if ((*sd).control & SE_SELF_RELATIVE) != 0 {
            return rtl_valid_relative_security_descriptor(
                sd as *const SECURITY_DESCRIPTOR_RELATIVE,
                core::mem::size_of::<SECURITY_DESCRIPTOR_RELATIVE>() as ULONG,
                0,
            );
        }

        // Проверяем Owner SID
        if !(*sd).owner.is_null() && !rtl_valid_sid((*sd).owner) {
            return false;
        }

        // Проверяем Group SID
        if !(*sd).group.is_null() && !rtl_valid_sid((*sd).group) {
            return false;
        }

        // Проверяем DACL
        if ((*sd).control & SE_DACL_PRESENT) != 0 && !(*sd).dacl.is_null() {
            if !rtl_valid_acl((*sd).dacl) {
                return false;
            }
        }

        // Проверяем SACL
        if ((*sd).control & SE_SACL_PRESENT) != 0 && !(*sd).sacl.is_null() {
            if !rtl_valid_acl((*sd).sacl) {
                return false;
            }
        }

        true
    }
}

/// RtlValidRelativeSecurityDescriptor - проверяет валидность self-relative SD
///
/// # Источник
/// ReactOS: lib/rtl/sd.c RtlValidRelativeSecurityDescriptor
pub fn rtl_valid_relative_security_descriptor(
    sd: *const SECURITY_DESCRIPTOR_RELATIVE,
    sd_length: ULONG,
    required_info: SECURITY_INFORMATION,
) -> bool {
    if sd.is_null() {
        return false;
    }

    let min_size = core::mem::size_of::<SECURITY_DESCRIPTOR_RELATIVE>() as ULONG;
    if sd_length < min_size {
        return false;
    }

    unsafe {
        // Проверяем revision
        if (*sd).revision != SECURITY_DESCRIPTOR::REVISION {
            return false;
        }

        // Должен быть self-relative
        if ((*sd).control & SE_SELF_RELATIVE) == 0 {
            return false;
        }

        let sd_base = sd as *const u8;

        // Проверяем Owner
        if (*sd).owner != 0 {
            if (*sd).owner as ULONG >= sd_length {
                return false;
            }
            let owner = sd_base.add((*sd).owner as usize) as *const SID;
            if !rtl_valid_sid(owner) {
                return false;
            }
            let owner_end = (*sd).owner + rtl_length_sid(owner);
            if owner_end > sd_length {
                return false;
            }
        } else if (required_info & OWNER_SECURITY_INFORMATION) != 0 {
            return false;
        }

        // Проверяем Group
        if (*sd).group != 0 {
            if (*sd).group as ULONG >= sd_length {
                return false;
            }
            let group = sd_base.add((*sd).group as usize) as *const SID;
            if !rtl_valid_sid(group) {
                return false;
            }
            let group_end = (*sd).group + rtl_length_sid(group);
            if group_end > sd_length {
                return false;
            }
        } else if (required_info & GROUP_SECURITY_INFORMATION) != 0 {
            return false;
        }

        // Проверяем DACL
        if (*sd).dacl != 0 {
            if (*sd).dacl as ULONG >= sd_length {
                return false;
            }
            let dacl = sd_base.add((*sd).dacl as usize) as *const ACL;
            if !rtl_valid_acl(dacl) {
                return false;
            }
            let dacl_end = (*sd).dacl + (*dacl).acl_size as ULONG;
            if dacl_end > sd_length {
                return false;
            }
        } else if (required_info & DACL_SECURITY_INFORMATION) != 0 {
            if ((*sd).control & SE_DACL_PRESENT) == 0 {
                return false;
            }
        }

        // Проверяем SACL
        if (*sd).sacl != 0 {
            if (*sd).sacl as ULONG >= sd_length {
                return false;
            }
            let sacl = sd_base.add((*sd).sacl as usize) as *const ACL;
            if !rtl_valid_acl(sacl) {
                return false;
            }
            let sacl_end = (*sd).sacl + (*sacl).acl_size as ULONG;
            if sacl_end > sd_length {
                return false;
            }
        } else if (required_info & SACL_SECURITY_INFORMATION) != 0 {
            if ((*sd).control & SE_SACL_PRESENT) == 0 {
                return false;
            }
        }

        true
    }
}

// =============================================================================
// Security Descriptor initialization
// =============================================================================

/// RtlCreateSecurityDescriptor - инициализирует пустой SD
///
/// # Источник
/// ReactOS: lib/rtl/sd.c RtlCreateSecurityDescriptor
pub fn rtl_create_security_descriptor(
    sd: *mut SECURITY_DESCRIPTOR,
    revision: ULONG,
) -> NTSTATUS {
    if sd.is_null() {
        return STATUS_INVALID_SECURITY_DESCR;
    }

    if revision != SECURITY_DESCRIPTOR::REVISION as ULONG {
        return STATUS_INVALID_SECURITY_DESCR;
    }

    unsafe {
        (*sd).revision = SECURITY_DESCRIPTOR::REVISION;
        (*sd).sbz1 = 0;
        (*sd).control = 0;
        (*sd).owner = core::ptr::null_mut();
        (*sd).group = core::ptr::null_mut();
        (*sd).sacl = core::ptr::null_mut();
        (*sd).dacl = core::ptr::null_mut();
    }

    STATUS_SUCCESS
}

/// RtlCreateSecurityDescriptorRelative - инициализирует пустой self-relative SD
pub fn rtl_create_security_descriptor_relative(
    sd: *mut SECURITY_DESCRIPTOR_RELATIVE,
    revision: ULONG,
) -> NTSTATUS {
    if sd.is_null() {
        return STATUS_INVALID_SECURITY_DESCR;
    }

    if revision != SECURITY_DESCRIPTOR::REVISION as ULONG {
        return STATUS_INVALID_SECURITY_DESCR;
    }

    unsafe {
        (*sd).revision = SECURITY_DESCRIPTOR::REVISION;
        (*sd).sbz1 = 0;
        (*sd).control = SE_SELF_RELATIVE;
        (*sd).owner = 0;
        (*sd).group = 0;
        (*sd).sacl = 0;
        (*sd).dacl = 0;
    }

    STATUS_SUCCESS
}

// =============================================================================
// Security Descriptor getters/setters
// =============================================================================

/// RtlGetOwnerSecurityDescriptor - получает Owner из SD
///
/// # Источник
/// ReactOS: lib/rtl/sd.c RtlGetOwnerSecurityDescriptor
pub fn rtl_get_owner_security_descriptor(
    sd: *const SECURITY_DESCRIPTOR,
    owner: *mut *mut SID,
    owner_defaulted: *mut BOOLEAN,
) -> NTSTATUS {
    if sd.is_null() || owner.is_null() {
        return STATUS_INVALID_SECURITY_DESCR;
    }

    unsafe {
        if ((*sd).control & SE_SELF_RELATIVE) != 0 {
            let rel_sd = sd as *const SECURITY_DESCRIPTOR_RELATIVE;
            if (*rel_sd).owner != 0 {
                *owner = (rel_sd as *const u8).add((*rel_sd).owner as usize) as *mut SID;
            } else {
                *owner = core::ptr::null_mut();
            }
        } else {
            *owner = (*sd).owner;
        }

        if !owner_defaulted.is_null() {
            *owner_defaulted = if ((*sd).control & SE_OWNER_DEFAULTED) != 0 { 1 } else { 0 };
        }
    }

    STATUS_SUCCESS
}

/// RtlSetOwnerSecurityDescriptor - устанавливает Owner в SD (только absolute)
///
/// # Источник
/// ReactOS: lib/rtl/sd.c RtlSetOwnerSecurityDescriptor
pub fn rtl_set_owner_security_descriptor(
    sd: *mut SECURITY_DESCRIPTOR,
    owner: *mut SID,
    owner_defaulted: BOOLEAN,
) -> NTSTATUS {
    if sd.is_null() {
        return STATUS_INVALID_SECURITY_DESCR;
    }

    unsafe {
        // Нельзя модифицировать self-relative SD
        if ((*sd).control & SE_SELF_RELATIVE) != 0 {
            return STATUS_INVALID_SECURITY_DESCR;
        }

        (*sd).owner = owner;
        
        if owner_defaulted != 0 {
            (*sd).control |= SE_OWNER_DEFAULTED;
        } else {
            (*sd).control &= !SE_OWNER_DEFAULTED;
        }
    }

    STATUS_SUCCESS
}

/// RtlGetGroupSecurityDescriptor - получает Group из SD
pub fn rtl_get_group_security_descriptor(
    sd: *const SECURITY_DESCRIPTOR,
    group: *mut *mut SID,
    group_defaulted: *mut BOOLEAN,
) -> NTSTATUS {
    if sd.is_null() || group.is_null() {
        return STATUS_INVALID_SECURITY_DESCR;
    }

    unsafe {
        if ((*sd).control & SE_SELF_RELATIVE) != 0 {
            let rel_sd = sd as *const SECURITY_DESCRIPTOR_RELATIVE;
            if (*rel_sd).group != 0 {
                *group = (rel_sd as *const u8).add((*rel_sd).group as usize) as *mut SID;
            } else {
                *group = core::ptr::null_mut();
            }
        } else {
            *group = (*sd).group;
        }

        if !group_defaulted.is_null() {
            *group_defaulted = if ((*sd).control & SE_GROUP_DEFAULTED) != 0 { 1 } else { 0 };
        }
    }

    STATUS_SUCCESS
}

/// RtlSetGroupSecurityDescriptor - устанавливает Group в SD
pub fn rtl_set_group_security_descriptor(
    sd: *mut SECURITY_DESCRIPTOR,
    group: *mut SID,
    group_defaulted: BOOLEAN,
) -> NTSTATUS {
    if sd.is_null() {
        return STATUS_INVALID_SECURITY_DESCR;
    }

    unsafe {
        if ((*sd).control & SE_SELF_RELATIVE) != 0 {
            return STATUS_INVALID_SECURITY_DESCR;
        }

        (*sd).group = group;
        
        if group_defaulted != 0 {
            (*sd).control |= SE_GROUP_DEFAULTED;
        } else {
            (*sd).control &= !SE_GROUP_DEFAULTED;
        }
    }

    STATUS_SUCCESS
}

/// RtlGetDaclSecurityDescriptor - получает DACL из SD
///
/// # Источник
/// ReactOS: lib/rtl/sd.c RtlGetDaclSecurityDescriptor
pub fn rtl_get_dacl_security_descriptor(
    sd: *const SECURITY_DESCRIPTOR,
    dacl_present: *mut BOOLEAN,
    dacl: *mut *mut ACL,
    dacl_defaulted: *mut BOOLEAN,
) -> NTSTATUS {
    if sd.is_null() {
        return STATUS_INVALID_SECURITY_DESCR;
    }

    unsafe {
        if !dacl_present.is_null() {
            *dacl_present = if ((*sd).control & SE_DACL_PRESENT) != 0 { 1 } else { 0 };
        }

        if !dacl.is_null() {
            if ((*sd).control & SE_DACL_PRESENT) != 0 {
                if ((*sd).control & SE_SELF_RELATIVE) != 0 {
                    let rel_sd = sd as *const SECURITY_DESCRIPTOR_RELATIVE;
                    if (*rel_sd).dacl != 0 {
                        *dacl = (rel_sd as *const u8).add((*rel_sd).dacl as usize) as *mut ACL;
                    } else {
                        *dacl = core::ptr::null_mut();
                    }
                } else {
                    *dacl = (*sd).dacl;
                }
            } else {
                *dacl = core::ptr::null_mut();
            }
        }

        if !dacl_defaulted.is_null() {
            *dacl_defaulted = if ((*sd).control & SE_DACL_DEFAULTED) != 0 { 1 } else { 0 };
        }
    }

    STATUS_SUCCESS
}

/// RtlSetDaclSecurityDescriptor - устанавливает DACL в SD
///
/// # Источник
/// ReactOS: lib/rtl/sd.c RtlSetDaclSecurityDescriptor
pub fn rtl_set_dacl_security_descriptor(
    sd: *mut SECURITY_DESCRIPTOR,
    dacl_present: BOOLEAN,
    dacl: *mut ACL,
    dacl_defaulted: BOOLEAN,
) -> NTSTATUS {
    if sd.is_null() {
        return STATUS_INVALID_SECURITY_DESCR;
    }

    unsafe {
        if ((*sd).control & SE_SELF_RELATIVE) != 0 {
            return STATUS_INVALID_SECURITY_DESCR;
        }

        if dacl_present != 0 {
            (*sd).control |= SE_DACL_PRESENT;
            (*sd).dacl = dacl;

            if dacl_defaulted != 0 {
                (*sd).control |= SE_DACL_DEFAULTED;
            } else {
                (*sd).control &= !SE_DACL_DEFAULTED;
            }
        } else {
            (*sd).control &= !SE_DACL_PRESENT;
            (*sd).control &= !SE_DACL_DEFAULTED;
            (*sd).dacl = core::ptr::null_mut();
        }
    }

    STATUS_SUCCESS
}

/// RtlGetSaclSecurityDescriptor - получает SACL из SD
pub fn rtl_get_sacl_security_descriptor(
    sd: *const SECURITY_DESCRIPTOR,
    sacl_present: *mut BOOLEAN,
    sacl: *mut *mut ACL,
    sacl_defaulted: *mut BOOLEAN,
) -> NTSTATUS {
    if sd.is_null() {
        return STATUS_INVALID_SECURITY_DESCR;
    }

    unsafe {
        if !sacl_present.is_null() {
            *sacl_present = if ((*sd).control & SE_SACL_PRESENT) != 0 { 1 } else { 0 };
        }

        if !sacl.is_null() {
            if ((*sd).control & SE_SACL_PRESENT) != 0 {
                if ((*sd).control & SE_SELF_RELATIVE) != 0 {
                    let rel_sd = sd as *const SECURITY_DESCRIPTOR_RELATIVE;
                    if (*rel_sd).sacl != 0 {
                        *sacl = (rel_sd as *const u8).add((*rel_sd).sacl as usize) as *mut ACL;
                    } else {
                        *sacl = core::ptr::null_mut();
                    }
                } else {
                    *sacl = (*sd).sacl;
                }
            } else {
                *sacl = core::ptr::null_mut();
            }
        }

        if !sacl_defaulted.is_null() {
            *sacl_defaulted = if ((*sd).control & SE_SACL_DEFAULTED) != 0 { 1 } else { 0 };
        }
    }

    STATUS_SUCCESS
}

/// RtlSetSaclSecurityDescriptor - устанавливает SACL в SD
pub fn rtl_set_sacl_security_descriptor(
    sd: *mut SECURITY_DESCRIPTOR,
    sacl_present: BOOLEAN,
    sacl: *mut ACL,
    sacl_defaulted: BOOLEAN,
) -> NTSTATUS {
    if sd.is_null() {
        return STATUS_INVALID_SECURITY_DESCR;
    }

    unsafe {
        if ((*sd).control & SE_SELF_RELATIVE) != 0 {
            return STATUS_INVALID_SECURITY_DESCR;
        }

        if sacl_present != 0 {
            (*sd).control |= SE_SACL_PRESENT;
            (*sd).sacl = sacl;

            if sacl_defaulted != 0 {
                (*sd).control |= SE_SACL_DEFAULTED;
            } else {
                (*sd).control &= !SE_SACL_DEFAULTED;
            }
        } else {
            (*sd).control &= !SE_SACL_PRESENT;
            (*sd).control &= !SE_SACL_DEFAULTED;
            (*sd).sacl = core::ptr::null_mut();
        }
    }

    STATUS_SUCCESS
}

/// RtlGetControlSecurityDescriptor - получает control flags из SD
pub fn rtl_get_control_security_descriptor(
    sd: *const SECURITY_DESCRIPTOR,
    control: *mut SECURITY_DESCRIPTOR_CONTROL,
    revision: *mut ULONG,
) -> NTSTATUS {
    if sd.is_null() || control.is_null() || revision.is_null() {
        return STATUS_INVALID_SECURITY_DESCR;
    }

    unsafe {
        *control = (*sd).control;
        *revision = (*sd).revision as ULONG;
    }

    STATUS_SUCCESS
}

// =============================================================================
// Security Descriptor length calculation
// =============================================================================

/// RtlLengthSecurityDescriptor - вычисляет длину SD
///
/// # Источник
/// ReactOS: lib/rtl/sd.c RtlLengthSecurityDescriptor
pub fn rtl_length_security_descriptor(sd: *const SECURITY_DESCRIPTOR) -> ULONG {
    if sd.is_null() {
        return 0;
    }

    unsafe {
        let mut length = if ((*sd).control & SE_SELF_RELATIVE) != 0 {
            core::mem::size_of::<SECURITY_DESCRIPTOR_RELATIVE>()
        } else {
            core::mem::size_of::<SECURITY_DESCRIPTOR>()
        };

        // Добавляем Owner
        let owner = sd_get_owner_ptr(sd);
        if !owner.is_null() {
            length += rtl_length_sid(owner) as usize;
        }

        // Добавляем Group
        let group = sd_get_group_ptr(sd);
        if !group.is_null() {
            length += rtl_length_sid(group) as usize;
        }

        // Добавляем DACL
        if ((*sd).control & SE_DACL_PRESENT) != 0 {
            let dacl = sd_get_dacl_ptr(sd);
            if !dacl.is_null() {
                length += (*dacl).acl_size as usize;
            }
        }

        // Добавляем SACL
        if ((*sd).control & SE_SACL_PRESENT) != 0 {
            let sacl = sd_get_sacl_ptr(sd);
            if !sacl.is_null() {
                length += (*sacl).acl_size as usize;
            }
        }

        length as ULONG
    }
}

// =============================================================================
// Helper functions
// =============================================================================

/// Получает указатель на Owner SID (универсально для absolute и self-relative)
pub fn sd_get_owner_ptr(sd: *const SECURITY_DESCRIPTOR) -> *const SID {
    if sd.is_null() {
        return core::ptr::null();
    }

    unsafe {
        if ((*sd).control & SE_SELF_RELATIVE) != 0 {
            let rel_sd = sd as *const SECURITY_DESCRIPTOR_RELATIVE;
            if (*rel_sd).owner != 0 {
                (rel_sd as *const u8).add((*rel_sd).owner as usize) as *const SID
            } else {
                core::ptr::null()
            }
        } else {
            (*sd).owner
        }
    }
}

/// Получает указатель на Group SID
pub fn sd_get_group_ptr(sd: *const SECURITY_DESCRIPTOR) -> *const SID {
    if sd.is_null() {
        return core::ptr::null();
    }

    unsafe {
        if ((*sd).control & SE_SELF_RELATIVE) != 0 {
            let rel_sd = sd as *const SECURITY_DESCRIPTOR_RELATIVE;
            if (*rel_sd).group != 0 {
                (rel_sd as *const u8).add((*rel_sd).group as usize) as *const SID
            } else {
                core::ptr::null()
            }
        } else {
            (*sd).group
        }
    }
}

/// Получает указатель на DACL
pub fn sd_get_dacl_ptr(sd: *const SECURITY_DESCRIPTOR) -> *const ACL {
    if sd.is_null() {
        return core::ptr::null();
    }

    unsafe {
        if ((*sd).control & SE_DACL_PRESENT) == 0 {
            return core::ptr::null();
        }

        if ((*sd).control & SE_SELF_RELATIVE) != 0 {
            let rel_sd = sd as *const SECURITY_DESCRIPTOR_RELATIVE;
            if (*rel_sd).dacl != 0 {
                (rel_sd as *const u8).add((*rel_sd).dacl as usize) as *const ACL
            } else {
                core::ptr::null()
            }
        } else {
            (*sd).dacl
        }
    }
}

/// Получает указатель на SACL
pub fn sd_get_sacl_ptr(sd: *const SECURITY_DESCRIPTOR) -> *const ACL {
    if sd.is_null() {
        return core::ptr::null();
    }

    unsafe {
        if ((*sd).control & SE_SACL_PRESENT) == 0 {
            return core::ptr::null();
        }

        if ((*sd).control & SE_SELF_RELATIVE) != 0 {
            let rel_sd = sd as *const SECURITY_DESCRIPTOR_RELATIVE;
            if (*rel_sd).sacl != 0 {
                (rel_sd as *const u8).add((*rel_sd).sacl as usize) as *const ACL
            } else {
                core::ptr::null()
            }
        } else {
            (*sd).sacl
        }
    }
}

/// SeValidSecurityDescriptor - валидация SD для SE
///
/// # Источник
/// ReactOS: ntoskrnl/se/sd.c SeValidSecurityDescriptor
pub fn se_valid_security_descriptor(length: ULONG, sd: *const SECURITY_DESCRIPTOR) -> bool {
    if sd.is_null() || length < core::mem::size_of::<SECURITY_DESCRIPTOR_RELATIVE>() as ULONG {
        return false;
    }

    // Для SE требуется self-relative формат
    unsafe {
        if ((*sd).control & SE_SELF_RELATIVE) == 0 {
            return false;
        }
    }

    rtl_valid_relative_security_descriptor(
        sd as *const SECURITY_DESCRIPTOR_RELATIVE,
        length,
        0,
    )
}

/// SeCaptureSecurityDescriptor - захватывает (копирует) SD
///
/// Создает копию SD в self-relative формате.
///
/// # Источник
/// ReactOS: ntoskrnl/se/sd.c SeCaptureSecurityDescriptor
pub fn se_capture_security_descriptor(
    original_sd: *const SECURITY_DESCRIPTOR,
    pool_type: POOL_TYPE,
    captured_sd: *mut *mut SECURITY_DESCRIPTOR,
) -> NTSTATUS {
    if original_sd.is_null() || captured_sd.is_null() {
        unsafe { *captured_sd = core::ptr::null_mut(); }
        return STATUS_SUCCESS; // NULL SD допустим
    }

    if !rtl_valid_security_descriptor(original_sd) {
        return STATUS_INVALID_SECURITY_DESCR;
    }

    // Вычисляем размер для self-relative копии
    let length = rtl_length_security_descriptor(original_sd) as usize;
    
    // Аллоцируем буфер
    let buffer = ex_allocate_pool_with_tag(pool_type, length, TAG_SD);
    if buffer.is_null() {
        return STATUS_INVALID_SECURITY_DESCR;
    }

    // Делаем self-relative копию
    let status = rtl_make_self_relative_sd(
        original_sd as *mut SECURITY_DESCRIPTOR,
        buffer as *mut SECURITY_DESCRIPTOR_RELATIVE,
        &mut (length as ULONG),
    );

    if status != STATUS_SUCCESS {
        ex_free_pool_with_tag(buffer, TAG_SD);
        unsafe { *captured_sd = core::ptr::null_mut(); }
        return status;
    }

    unsafe { *captured_sd = buffer as *mut SECURITY_DESCRIPTOR; }
    STATUS_SUCCESS
}

/// SeReleaseSecurityDescriptor - освобождает захваченный SD
pub fn se_release_security_descriptor(sd: *mut SECURITY_DESCRIPTOR) {
    if !sd.is_null() {
        ex_free_pool_with_tag(sd as PVOID, TAG_SD);
    }
}

/// RtlAbsoluteToSelfRelativeSD - конвертирует absolute SD в self-relative
///
/// # Источник
/// ReactOS: lib/rtl/sd.c RtlAbsoluteToSelfRelativeSD
pub fn rtl_absolute_to_self_relative_sd(
    absolute_sd: *const SECURITY_DESCRIPTOR,
    self_relative_sd: *mut SECURITY_DESCRIPTOR_RELATIVE,
    buffer_length: *mut ULONG,
) -> NTSTATUS {
    rtl_make_self_relative_sd(absolute_sd as *mut SECURITY_DESCRIPTOR, self_relative_sd, buffer_length)
}

/// RtlMakeSelfRelativeSD - создает self-relative копию SD
///
/// # Источник
/// ReactOS: lib/rtl/sd.c RtlMakeSelfRelativeSD
pub fn rtl_make_self_relative_sd(
    absolute_sd: *mut SECURITY_DESCRIPTOR,
    self_relative_sd: *mut SECURITY_DESCRIPTOR_RELATIVE,
    buffer_length: *mut ULONG,
) -> NTSTATUS {
    if absolute_sd.is_null() || buffer_length.is_null() {
        return STATUS_INVALID_SECURITY_DESCR;
    }

    unsafe {
        // Вычисляем требуемый размер
        let required_length = rtl_length_security_descriptor(absolute_sd);
        
        if *buffer_length < required_length {
            *buffer_length = required_length;
            return STATUS_BUFFER_TOO_SMALL;
        }

        if self_relative_sd.is_null() {
            return STATUS_INVALID_SECURITY_DESCR;
        }

        // Инициализируем заголовок
        (*self_relative_sd).revision = (*absolute_sd).revision;
        (*self_relative_sd).sbz1 = 0;
        (*self_relative_sd).control = (*absolute_sd).control | SE_SELF_RELATIVE;
        (*self_relative_sd).owner = 0;
        (*self_relative_sd).group = 0;
        (*self_relative_sd).sacl = 0;
        (*self_relative_sd).dacl = 0;

        let mut offset = core::mem::size_of::<SECURITY_DESCRIPTOR_RELATIVE>() as ULONG;
        let dest_base = self_relative_sd as *mut u8;

        // Копируем Owner
        let owner = sd_get_owner_ptr(absolute_sd);
        if !owner.is_null() {
            (*self_relative_sd).owner = offset;
            let owner_len = rtl_length_sid(owner);
            core::ptr::copy_nonoverlapping(
                owner as *const u8,
                dest_base.add(offset as usize),
                owner_len as usize,
            );
            offset += owner_len;
        }

        // Копируем Group
        let group = sd_get_group_ptr(absolute_sd);
        if !group.is_null() {
            (*self_relative_sd).group = offset;
            let group_len = rtl_length_sid(group);
            core::ptr::copy_nonoverlapping(
                group as *const u8,
                dest_base.add(offset as usize),
                group_len as usize,
            );
            offset += group_len;
        }

        // Копируем SACL
        if ((*absolute_sd).control & SE_SACL_PRESENT) != 0 {
            let sacl = sd_get_sacl_ptr(absolute_sd);
            if !sacl.is_null() {
                (*self_relative_sd).sacl = offset;
                let sacl_len = (*sacl).acl_size as ULONG;
                core::ptr::copy_nonoverlapping(
                    sacl as *const u8,
                    dest_base.add(offset as usize),
                    sacl_len as usize,
                );
                offset += sacl_len;
            }
        }

        // Копируем DACL
        if ((*absolute_sd).control & SE_DACL_PRESENT) != 0 {
            let dacl = sd_get_dacl_ptr(absolute_sd);
            if !dacl.is_null() {
                (*self_relative_sd).dacl = offset;
                let dacl_len = (*dacl).acl_size as ULONG;
                core::ptr::copy_nonoverlapping(
                    dacl as *const u8,
                    dest_base.add(offset as usize),
                    dacl_len as usize,
                );
            }
        }

        *buffer_length = required_length;
    }

    STATUS_SUCCESS
}

// =============================================================================
// SeAssignSecurity / SeDeassignSecurity
// =============================================================================

/// SeAssignSecurity - назначает Security Descriptor новому объекту
///
/// Создает SD для нового объекта, комбинируя:
/// - Parent SD (наследование)
/// - Explicit SD (указанный при создании)
/// - Token defaults (owner, primary group, default DACL)
///
/// # Источник
/// ReactOS: ntoskrnl/se/sd.c SeAssignSecurity
pub fn se_assign_security(
    parent_sd: *const SECURITY_DESCRIPTOR,
    explicit_sd: *const SECURITY_DESCRIPTOR,
    new_sd: *mut *mut SECURITY_DESCRIPTOR,
    is_directory_object: bool,
    subject_context: *mut super::subject::SECURITY_SUBJECT_CONTEXT,
    generic_mapping: *const crate::ob::types::GENERIC_MAPPING,
    pool_type: POOL_TYPE,
) -> NTSTATUS {
    se_assign_security_ex(
        parent_sd,
        explicit_sd,
        new_sd,
        core::ptr::null(), // object type GUID
        is_directory_object,
        0, // auto_inherit_flags
        subject_context,
        generic_mapping,
        pool_type,
    )
}

/// SeAssignSecurityEx - расширенная версия с поддержкой auto-inherit
///
/// # Источник
/// ReactOS: ntoskrnl/se/sd.c SeAssignSecurityEx
#[allow(clippy::too_many_arguments)]
pub fn se_assign_security_ex(
    parent_sd: *const SECURITY_DESCRIPTOR,
    explicit_sd: *const SECURITY_DESCRIPTOR,
    new_sd: *mut *mut SECURITY_DESCRIPTOR,
    _object_type: *const crate::nt::GUID,
    is_directory_object: bool,
    _auto_inherit_flags: ULONG,
    subject_context: *mut super::subject::SECURITY_SUBJECT_CONTEXT,
    generic_mapping: *const crate::ob::types::GENERIC_MAPPING,
    pool_type: POOL_TYPE,
) -> NTSTATUS {
    use crate::nt::{
        STATUS_NO_MEMORY, SE_DACL_AUTO_INHERITED, SE_SACL_AUTO_INHERITED,
    };
    use super::sid::{rtl_length_sid, rtl_copy_sid, se_local_system_sid};
    use super::token::TOKEN;

    if new_sd.is_null() {
        return crate::nt::STATUS_INVALID_PARAMETER;
    }

    unsafe {
        *new_sd = core::ptr::null_mut();

        // Получаем token для defaults
        let token = super::subject::se_get_effective_token(subject_context);
        
        // === Определяем Owner ===
        let owner_sid = sep_determine_owner(explicit_sd, subject_context, token);
        
        // === Определяем Group ===
        let group_sid = sep_determine_group(explicit_sd, subject_context, token);
        
        // === Определяем DACL ===
        let (dacl, dacl_present, dacl_defaulted) = sep_determine_dacl(
            parent_sd,
            explicit_sd,
            token,
            is_directory_object,
            generic_mapping,
        );
        
        // === Определяем SACL ===
        let (sacl, sacl_present, sacl_defaulted) = sep_determine_sacl(
            parent_sd,
            explicit_sd,
            token,
            is_directory_object,
        );

        // === Вычисляем размер self-relative SD ===
        let mut size = core::mem::size_of::<SECURITY_DESCRIPTOR_RELATIVE>();
        
        if !owner_sid.is_null() {
            size += rtl_length_sid(owner_sid) as usize;
        }
        if !group_sid.is_null() {
            size += rtl_length_sid(group_sid) as usize;
        }
        if dacl_present && !dacl.is_null() {
            size += (*dacl).acl_size as usize;
        }
        if sacl_present && !sacl.is_null() {
            size += (*sacl).acl_size as usize;
        }

        // === Аллоцируем память ===
        let buffer = ex_allocate_pool_with_tag(pool_type, size, TAG_SD);
        if buffer.is_null() {
            return STATUS_NO_MEMORY;
        }
        core::ptr::write_bytes(buffer, 0, size);

        let sd_rel = buffer as *mut SECURITY_DESCRIPTOR_RELATIVE;
        (*sd_rel).revision = SECURITY_DESCRIPTOR::REVISION;
        (*sd_rel).sbz1 = 0;
        (*sd_rel).control = SE_SELF_RELATIVE;

        let mut offset = core::mem::size_of::<SECURITY_DESCRIPTOR_RELATIVE>() as ULONG;
        let base = buffer as *mut u8;

        // === Копируем Owner ===
        if !owner_sid.is_null() {
            (*sd_rel).owner = offset;
            let owner_len = rtl_length_sid(owner_sid);
            core::ptr::copy_nonoverlapping(
                owner_sid as *const u8,
                base.add(offset as usize),
                owner_len as usize,
            );
            offset += owner_len;
        }

        // === Копируем Group ===
        if !group_sid.is_null() {
            (*sd_rel).group = offset;
            let group_len = rtl_length_sid(group_sid);
            core::ptr::copy_nonoverlapping(
                group_sid as *const u8,
                base.add(offset as usize),
                group_len as usize,
            );
            offset += group_len;
        }

        // === Копируем SACL ===
        if sacl_present {
            (*sd_rel).control |= SE_SACL_PRESENT;
            if sacl_defaulted {
                (*sd_rel).control |= SE_SACL_DEFAULTED;
            }
            if !sacl.is_null() {
                (*sd_rel).sacl = offset;
                let sacl_len = (*sacl).acl_size as ULONG;
                core::ptr::copy_nonoverlapping(
                    sacl as *const u8,
                    base.add(offset as usize),
                    sacl_len as usize,
                );
                offset += sacl_len;
            }
        }

        // === Копируем DACL ===
        if dacl_present {
            (*sd_rel).control |= SE_DACL_PRESENT;
            if dacl_defaulted {
                (*sd_rel).control |= SE_DACL_DEFAULTED;
            }
            if !dacl.is_null() {
                (*sd_rel).dacl = offset;
                let dacl_len = (*dacl).acl_size as ULONG;
                core::ptr::copy_nonoverlapping(
                    dacl as *const u8,
                    base.add(offset as usize),
                    dacl_len as usize,
                );
            }
        }

        *new_sd = sd_rel as *mut SECURITY_DESCRIPTOR;
    }

    STATUS_SUCCESS
}

/// SeDeassignSecurity - освобождает SD назначенный SeAssignSecurity
///
/// # Источник
/// ReactOS: ntoskrnl/se/sd.c SeDeassignSecurity
pub fn se_deassign_security(sd: *mut *mut SECURITY_DESCRIPTOR) -> NTSTATUS {
    if sd.is_null() {
        return STATUS_SUCCESS;
    }

    unsafe {
        if !(*sd).is_null() {
            ex_free_pool_with_tag(*sd as PVOID, TAG_SD);
            *sd = core::ptr::null_mut();
        }
    }

    STATUS_SUCCESS
}

// === Internal helpers ===

/// Определяет Owner для нового SD
unsafe fn sep_determine_owner(
    explicit_sd: *const SECURITY_DESCRIPTOR,
    _subject_context: *mut super::subject::SECURITY_SUBJECT_CONTEXT,
    token: *mut super::token::TOKEN,
) -> *const SID {
    // 1. Explicit SD owner
    if !explicit_sd.is_null() {
        let owner = sd_get_owner_ptr(explicit_sd);
        if !owner.is_null() {
            return owner;
        }
    }

    // 2. Token owner (default owner)
    if !token.is_null() {
        let tok = &*token;
        if !tok.owner.is_null() {
            return tok.owner;
        }
        // Fallback to user SID
        if !tok.user_sid.is_null() {
            return tok.user_sid;
        }
    }

    // 3. LocalSystem as ultimate fallback
    super::sid::se_local_system_sid()
}

/// Определяет Group для нового SD
unsafe fn sep_determine_group(
    explicit_sd: *const SECURITY_DESCRIPTOR,
    _subject_context: *mut super::subject::SECURITY_SUBJECT_CONTEXT,
    token: *mut super::token::TOKEN,
) -> *const SID {
    // 1. Explicit SD group
    if !explicit_sd.is_null() {
        let group = sd_get_group_ptr(explicit_sd);
        if !group.is_null() {
            return group;
        }
    }

    // 2. Token primary group
    if !token.is_null() {
        let tok = &*token;
        if !tok.primary_group.is_null() {
            return tok.primary_group;
        }
    }

    // 3. LocalSystem as fallback
    super::sid::se_local_system_sid()
}

/// Определяет DACL для нового SD
unsafe fn sep_determine_dacl(
    parent_sd: *const SECURITY_DESCRIPTOR,
    explicit_sd: *const SECURITY_DESCRIPTOR,
    token: *mut super::token::TOKEN,
    _is_directory: bool,
    _generic_mapping: *const crate::ob::types::GENERIC_MAPPING,
) -> (*const ACL, bool, bool) {
    // 1. Explicit DACL (if DACL_PRESENT)
    if !explicit_sd.is_null() {
        if ((*explicit_sd).control & SE_DACL_PRESENT) != 0 {
            let dacl = sd_get_dacl_ptr(explicit_sd);
            let defaulted = ((*explicit_sd).control & SE_DACL_DEFAULTED) != 0;
            return (dacl, true, defaulted);
        }
    }

    // 2. Inherited from parent (only inheritable ACEs)
    // TODO: implement proper ACL inheritance
    if !parent_sd.is_null() {
        if ((*parent_sd).control & SE_DACL_PRESENT) != 0 {
            let parent_dacl = sd_get_dacl_ptr(parent_sd);
            if !parent_dacl.is_null() && (*parent_dacl).ace_count > 0 {
                // Simplified: copy entire DACL (should filter for inheritable ACEs)
                return (parent_dacl, true, true);
            }
        }
    }

    // 3. Token default DACL
    if !token.is_null() {
        let tok = &*token;
        if !tok.default_dacl.is_null() {
            return (tok.default_dacl, true, true);
        }
    }

    // 4. No DACL (full access for everyone)
    (core::ptr::null(), true, true)
}

/// Определяет SACL для нового SD
unsafe fn sep_determine_sacl(
    parent_sd: *const SECURITY_DESCRIPTOR,
    explicit_sd: *const SECURITY_DESCRIPTOR,
    _token: *mut super::token::TOKEN,
    _is_directory: bool,
) -> (*const ACL, bool, bool) {
    // 1. Explicit SACL
    if !explicit_sd.is_null() {
        if ((*explicit_sd).control & SE_SACL_PRESENT) != 0 {
            let sacl = sd_get_sacl_ptr(explicit_sd);
            let defaulted = ((*explicit_sd).control & SE_SACL_DEFAULTED) != 0;
            return (sacl, true, defaulted);
        }
    }

    // 2. Inherited from parent
    if !parent_sd.is_null() {
        if ((*parent_sd).control & SE_SACL_PRESENT) != 0 {
            let parent_sacl = sd_get_sacl_ptr(parent_sd);
            if !parent_sacl.is_null() {
                return (parent_sacl, true, true);
            }
        }
    }

    // 3. No SACL
    (core::ptr::null(), false, false)
}

// =============================================================================
// SeQuerySecurityDescriptorInfo / SeSetSecurityDescriptorInfo
// =============================================================================

/// SeQuerySecurityDescriptorInfo - запрашивает части SD по SECURITY_INFORMATION mask
///
/// Копирует запрошенные части SD в выходной буфер в self-relative формате.
///
/// # Источник
/// ReactOS: ntoskrnl/se/sd.c SeQuerySecurityDescriptorInfo
pub fn se_query_security_descriptor_info(
    security_information: *const SECURITY_INFORMATION,
    security_descriptor: *mut SECURITY_DESCRIPTOR,
    length: *mut ULONG,
    object_sd: *mut *mut SECURITY_DESCRIPTOR,
) -> NTSTATUS {
    if security_information.is_null() || length.is_null() || object_sd.is_null() {
        return crate::nt::STATUS_INVALID_PARAMETER;
    }

    unsafe {
        let info = *security_information;
        let sd = *object_sd;

        if sd.is_null() {
            // Минимальный размер для пустого SD
            *length = core::mem::size_of::<SECURITY_DESCRIPTOR_RELATIVE>() as ULONG;
            return STATUS_BUFFER_TOO_SMALL;
        }

        // Вычисляем требуемый размер
        let mut required_size = core::mem::size_of::<SECURITY_DESCRIPTOR_RELATIVE>();

        let owner = if (info & OWNER_SECURITY_INFORMATION) != 0 {
            sd_get_owner_ptr(sd)
        } else {
            core::ptr::null()
        };
        if !owner.is_null() {
            required_size += super::sid::rtl_length_sid(owner) as usize;
        }

        let group = if (info & GROUP_SECURITY_INFORMATION) != 0 {
            sd_get_group_ptr(sd)
        } else {
            core::ptr::null()
        };
        if !group.is_null() {
            required_size += super::sid::rtl_length_sid(group) as usize;
        }

        let dacl = if (info & DACL_SECURITY_INFORMATION) != 0 && ((*sd).control & SE_DACL_PRESENT) != 0 {
            sd_get_dacl_ptr(sd)
        } else {
            core::ptr::null()
        };
        if !dacl.is_null() {
            required_size += (*dacl).acl_size as usize;
        }

        let sacl = if (info & SACL_SECURITY_INFORMATION) != 0 && ((*sd).control & SE_SACL_PRESENT) != 0 {
            sd_get_sacl_ptr(sd)
        } else {
            core::ptr::null()
        };
        if !sacl.is_null() {
            required_size += (*sacl).acl_size as usize;
        }

        // Проверяем размер буфера
        if *length < required_size as ULONG {
            *length = required_size as ULONG;
            return STATUS_BUFFER_TOO_SMALL;
        }

        if security_descriptor.is_null() {
            *length = required_size as ULONG;
            return STATUS_BUFFER_TOO_SMALL;
        }

        // Заполняем выходной SD
        let out_sd = security_descriptor as *mut SECURITY_DESCRIPTOR_RELATIVE;
        core::ptr::write_bytes(out_sd, 0, 1);
        
        (*out_sd).revision = SECURITY_DESCRIPTOR::REVISION;
        (*out_sd).control = SE_SELF_RELATIVE;

        let mut offset = core::mem::size_of::<SECURITY_DESCRIPTOR_RELATIVE>() as ULONG;
        let base = out_sd as *mut u8;

        // Копируем Owner
        if !owner.is_null() {
            (*out_sd).owner = offset;
            let len = super::sid::rtl_length_sid(owner);
            core::ptr::copy_nonoverlapping(owner as *const u8, base.add(offset as usize), len as usize);
            offset += len;
        }

        // Копируем Group
        if !group.is_null() {
            (*out_sd).group = offset;
            let len = super::sid::rtl_length_sid(group);
            core::ptr::copy_nonoverlapping(group as *const u8, base.add(offset as usize), len as usize);
            offset += len;
        }

        // Копируем DACL
        if (info & DACL_SECURITY_INFORMATION) != 0 {
            (*out_sd).control |= (*sd).control & (SE_DACL_PRESENT | SE_DACL_DEFAULTED);
            if !dacl.is_null() {
                (*out_sd).dacl = offset;
                let len = (*dacl).acl_size as ULONG;
                core::ptr::copy_nonoverlapping(dacl as *const u8, base.add(offset as usize), len as usize);
                offset += len;
            }
        }

        // Копируем SACL
        if (info & SACL_SECURITY_INFORMATION) != 0 {
            (*out_sd).control |= (*sd).control & (SE_SACL_PRESENT | SE_SACL_DEFAULTED);
            if !sacl.is_null() {
                (*out_sd).sacl = offset;
                let len = (*sacl).acl_size as ULONG;
                core::ptr::copy_nonoverlapping(sacl as *const u8, base.add(offset as usize), len as usize);
            }
        }

        *length = required_size as ULONG;
    }

    STATUS_SUCCESS
}

/// SeSetSecurityDescriptorInfo - модифицирует части SD
///
/// Обновляет указанные части object SD на основе modification SD.
///
/// # Источник
/// ReactOS: ntoskrnl/se/sd.c SeSetSecurityDescriptorInfo
pub fn se_set_security_descriptor_info(
    _object: PVOID,
    security_information: *const SECURITY_INFORMATION,
    modification_sd: *const SECURITY_DESCRIPTOR,
    object_sd: *mut *mut SECURITY_DESCRIPTOR,
    pool_type: POOL_TYPE,
    generic_mapping: *const crate::ob::types::GENERIC_MAPPING,
) -> NTSTATUS {
    se_set_security_descriptor_info_ex(
        _object,
        security_information,
        modification_sd,
        object_sd,
        0, // auto_inherit_flags
        pool_type,
        generic_mapping,
    )
}

/// SeSetSecurityDescriptorInfoEx - расширенная версия
#[allow(clippy::too_many_arguments)]
pub fn se_set_security_descriptor_info_ex(
    _object: PVOID,
    security_information: *const SECURITY_INFORMATION,
    modification_sd: *const SECURITY_DESCRIPTOR,
    object_sd: *mut *mut SECURITY_DESCRIPTOR,
    _auto_inherit_flags: ULONG,
    pool_type: POOL_TYPE,
    _generic_mapping: *const crate::ob::types::GENERIC_MAPPING,
) -> NTSTATUS {
    use crate::nt::STATUS_NO_MEMORY;

    if security_information.is_null() || modification_sd.is_null() || object_sd.is_null() {
        return crate::nt::STATUS_INVALID_PARAMETER;
    }

    unsafe {
        let info = *security_information;
        let old_sd = *object_sd;

        // Определяем компоненты нового SD
        let new_owner = if (info & OWNER_SECURITY_INFORMATION) != 0 {
            sd_get_owner_ptr(modification_sd)
        } else if !old_sd.is_null() {
            sd_get_owner_ptr(old_sd)
        } else {
            core::ptr::null()
        };

        let new_group = if (info & GROUP_SECURITY_INFORMATION) != 0 {
            sd_get_group_ptr(modification_sd)
        } else if !old_sd.is_null() {
            sd_get_group_ptr(old_sd)
        } else {
            core::ptr::null()
        };

        let (new_dacl, new_dacl_present) = if (info & DACL_SECURITY_INFORMATION) != 0 {
            if ((*modification_sd).control & SE_DACL_PRESENT) != 0 {
                (sd_get_dacl_ptr(modification_sd), true)
            } else {
                (core::ptr::null(), true) // Explicit null DACL
            }
        } else if !old_sd.is_null() && ((*old_sd).control & SE_DACL_PRESENT) != 0 {
            (sd_get_dacl_ptr(old_sd), true)
        } else {
            (core::ptr::null(), false)
        };

        let (new_sacl, new_sacl_present) = if (info & SACL_SECURITY_INFORMATION) != 0 {
            if ((*modification_sd).control & SE_SACL_PRESENT) != 0 {
                (sd_get_sacl_ptr(modification_sd), true)
            } else {
                (core::ptr::null(), true)
            }
        } else if !old_sd.is_null() && ((*old_sd).control & SE_SACL_PRESENT) != 0 {
            (sd_get_sacl_ptr(old_sd), true)
        } else {
            (core::ptr::null(), false)
        };

        // Вычисляем размер нового SD
        let mut size = core::mem::size_of::<SECURITY_DESCRIPTOR_RELATIVE>();
        if !new_owner.is_null() {
            size += super::sid::rtl_length_sid(new_owner) as usize;
        }
        if !new_group.is_null() {
            size += super::sid::rtl_length_sid(new_group) as usize;
        }
        if new_dacl_present && !new_dacl.is_null() {
            size += (*new_dacl).acl_size as usize;
        }
        if new_sacl_present && !new_sacl.is_null() {
            size += (*new_sacl).acl_size as usize;
        }

        // Аллоцируем новый SD
        let buffer = ex_allocate_pool_with_tag(pool_type, size, TAG_SD);
        if buffer.is_null() {
            return STATUS_NO_MEMORY;
        }
        core::ptr::write_bytes(buffer, 0, size);

        let sd_rel = buffer as *mut SECURITY_DESCRIPTOR_RELATIVE;
        (*sd_rel).revision = SECURITY_DESCRIPTOR::REVISION;
        (*sd_rel).control = SE_SELF_RELATIVE;

        let mut offset = core::mem::size_of::<SECURITY_DESCRIPTOR_RELATIVE>() as ULONG;
        let base = buffer as *mut u8;

        // Копируем Owner
        if !new_owner.is_null() {
            (*sd_rel).owner = offset;
            let len = super::sid::rtl_length_sid(new_owner);
            core::ptr::copy_nonoverlapping(new_owner as *const u8, base.add(offset as usize), len as usize);
            offset += len;
        }

        // Копируем Group
        if !new_group.is_null() {
            (*sd_rel).group = offset;
            let len = super::sid::rtl_length_sid(new_group);
            core::ptr::copy_nonoverlapping(new_group as *const u8, base.add(offset as usize), len as usize);
            offset += len;
        }

        // Копируем SACL
        if new_sacl_present {
            (*sd_rel).control |= SE_SACL_PRESENT;
            if !new_sacl.is_null() {
                (*sd_rel).sacl = offset;
                let len = (*new_sacl).acl_size as ULONG;
                core::ptr::copy_nonoverlapping(new_sacl as *const u8, base.add(offset as usize), len as usize);
                offset += len;
            }
        }

        // Копируем DACL
        if new_dacl_present {
            (*sd_rel).control |= SE_DACL_PRESENT;
            if !new_dacl.is_null() {
                (*sd_rel).dacl = offset;
                let len = (*new_dacl).acl_size as ULONG;
                core::ptr::copy_nonoverlapping(new_dacl as *const u8, base.add(offset as usize), len as usize);
            }
        }

        // Освобождаем старый SD
        if !old_sd.is_null() {
            ex_free_pool_with_tag(old_sd as PVOID, TAG_SD);
        }

        *object_sd = sd_rel as *mut SECURITY_DESCRIPTOR;
    }

    STATUS_SUCCESS
}

// =============================================================================
// ObpReleaseSecurityDescriptor - для OB
// =============================================================================

/// ObpReleaseSecurityDescriptor - освобождает SD объекта
///
/// Вызывается из OB при удалении объекта.
pub fn obp_release_security_descriptor(sd: PVOID) {
    if !sd.is_null() {
        ex_free_pool_with_tag(sd, TAG_SD);
    }
}

