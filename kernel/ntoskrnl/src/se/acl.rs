//! ACL - Access Control List
//!
//! Функции для работы с ACL и ACE: валидация, итерация, создание.
//!
//! Источники:
//! - ReactOS: ntoskrnl/se/acl.c
//! - ReactOS: lib/rtl/acl.c

use crate::nt::{
    NTSTATUS, PVOID, STATUS_INVALID_ACL, STATUS_SUCCESS, UCHAR, ULONG, USHORT,
    ACL, ACE_HEADER, ACCESS_ALLOWED_ACE, ACCESS_DENIED_ACE, SYSTEM_AUDIT_ACE,
    SYSTEM_MANDATORY_LABEL_ACE, ACCESS_ALLOWED_ACE_TYPE, ACCESS_DENIED_ACE_TYPE,
    SYSTEM_AUDIT_ACE_TYPE, SYSTEM_MANDATORY_LABEL_ACE_TYPE, SID, ACCESS_MASK,
};

use super::sid::rtl_valid_sid;

// =============================================================================
// ACL validation
// =============================================================================

/// RtlValidAcl - проверяет валидность ACL
///
/// Проверяет структурную корректность ACL и всех ACE внутри.
///
/// # Источник
/// ReactOS: lib/rtl/acl.c RtlValidAcl
pub fn rtl_valid_acl(acl: *const ACL) -> bool {
    if acl.is_null() {
        return false;
    }

    unsafe {
        // Проверяем revision
        if (*acl).acl_revision < ACL::REVISION || (*acl).acl_revision > ACL::REVISION_DS {
            return false;
        }

        // Минимальный размер ACL
        if ((*acl).acl_size as usize) < core::mem::size_of::<ACL>() {
            return false;
        }

        // Итерируемся по ACE и проверяем каждый
        let acl_end = (acl as *const u8).add((*acl).acl_size as usize);
        let mut ace_ptr = (acl as *const u8).add(core::mem::size_of::<ACL>());
        
        for _ in 0..(*acl).ace_count {
            // Проверяем, что ACE_HEADER помещается
            if ace_ptr.add(core::mem::size_of::<ACE_HEADER>()) > acl_end {
                return false;
            }

            let ace_header = ace_ptr as *const ACE_HEADER;
            let ace_size = (*ace_header).ace_size as usize;

            // Проверяем минимальный размер ACE
            if ace_size < core::mem::size_of::<ACE_HEADER>() {
                return false;
            }

            // Проверяем, что ACE помещается в ACL
            if ace_ptr.add(ace_size) > acl_end {
                return false;
            }

            ace_ptr = ace_ptr.add(ace_size);
        }

        true
    }
}

/// RtlCreateAcl - инициализирует ACL
///
/// # Источник
/// ReactOS: lib/rtl/acl.c RtlCreateAcl
pub fn rtl_create_acl(acl: *mut ACL, acl_length: ULONG, acl_revision: ULONG) -> NTSTATUS {
    if acl.is_null() {
        return STATUS_INVALID_ACL;
    }

    if acl_length < core::mem::size_of::<ACL>() as ULONG {
        return STATUS_INVALID_ACL;
    }

    // Проверяем revision
    let revision = acl_revision as UCHAR;
    if revision < ACL::REVISION || revision > ACL::REVISION_DS {
        return STATUS_INVALID_ACL;
    }

    unsafe {
        (*acl).acl_revision = revision;
        (*acl).sbz1 = 0;
        (*acl).acl_size = acl_length as USHORT;
        (*acl).ace_count = 0;
        (*acl).sbz2 = 0;
    }

    STATUS_SUCCESS
}

/// RtlFirstFreeAce - находит первый свободный байт в ACL
///
/// # Источник
/// ReactOS: lib/rtl/acl.c RtlFirstFreeAce
pub fn rtl_first_free_ace(acl: *const ACL, first_free: *mut *mut ACE_HEADER) -> bool {
    if acl.is_null() || first_free.is_null() {
        return false;
    }

    unsafe {
        let acl_end = (acl as *const u8).add((*acl).acl_size as usize);
        let mut ace_ptr = (acl as *const u8).add(core::mem::size_of::<ACL>());

        for _ in 0..(*acl).ace_count {
            if ace_ptr >= acl_end {
                *first_free = core::ptr::null_mut();
                return false;
            }

            let ace_header = ace_ptr as *const ACE_HEADER;
            ace_ptr = ace_ptr.add((*ace_header).ace_size as usize);
        }

        if ace_ptr > acl_end {
            *first_free = core::ptr::null_mut();
            return false;
        }

        *first_free = ace_ptr as *mut ACE_HEADER;
        true
    }
}

// =============================================================================
// ACE iteration
// =============================================================================

/// RtlGetAce - получает ACE по индексу
///
/// # Источник
/// ReactOS: lib/rtl/acl.c RtlGetAce
pub fn rtl_get_ace(acl: *const ACL, ace_index: ULONG, ace: *mut *mut ACE_HEADER) -> NTSTATUS {
    if acl.is_null() || ace.is_null() {
        return STATUS_INVALID_ACL;
    }

    unsafe {
        if ace_index >= (*acl).ace_count as ULONG {
            return STATUS_INVALID_ACL;
        }

        let acl_end = (acl as *const u8).add((*acl).acl_size as usize);
        let mut ace_ptr = (acl as *const u8).add(core::mem::size_of::<ACL>());

        for i in 0..=ace_index {
            if ace_ptr >= acl_end {
                return STATUS_INVALID_ACL;
            }

            if i == ace_index {
                *ace = ace_ptr as *mut ACE_HEADER;
                return STATUS_SUCCESS;
            }

            let ace_header = ace_ptr as *const ACE_HEADER;
            ace_ptr = ace_ptr.add((*ace_header).ace_size as usize);
        }

        STATUS_INVALID_ACL
    }
}

/// Итератор по ACE в ACL
pub struct AceIterator {
    current: *const u8,
    end: *const u8,
    remaining: u16,
}

impl AceIterator {
    /// Создает итератор для ACL
    pub fn new(acl: *const ACL) -> Option<Self> {
        if acl.is_null() {
            return None;
        }

        unsafe {
            Some(Self {
                current: (acl as *const u8).add(core::mem::size_of::<ACL>()),
                end: (acl as *const u8).add((*acl).acl_size as usize),
                remaining: (*acl).ace_count,
            })
        }
    }
}

impl Iterator for AceIterator {
    type Item = *const ACE_HEADER;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 || self.current >= self.end {
            return None;
        }

        unsafe {
            let ace = self.current as *const ACE_HEADER;
            let ace_size = (*ace).ace_size as usize;

            if self.current.add(ace_size) > self.end {
                return None;
            }

            self.current = self.current.add(ace_size);
            self.remaining -= 1;

            Some(ace)
        }
    }
}

// =============================================================================
// ACE helpers
// =============================================================================

/// Получает SID из ACE (для standard ACE типов: ALLOW/DENY/AUDIT/MANDATORY_LABEL)
///
/// # Safety
/// ACE должен быть валидным и иметь поддерживаемый тип
pub unsafe fn ace_get_sid(ace: *const ACE_HEADER) -> *const SID {
    if ace.is_null() {
        return core::ptr::null();
    }

    let ace_type = (*ace).ace_type;

    match ace_type {
        ACCESS_ALLOWED_ACE_TYPE => {
            let typed_ace = ace as *const ACCESS_ALLOWED_ACE;
            &(*typed_ace).sid_start as *const ULONG as *const SID
        }
        ACCESS_DENIED_ACE_TYPE => {
            let typed_ace = ace as *const ACCESS_DENIED_ACE;
            &(*typed_ace).sid_start as *const ULONG as *const SID
        }
        SYSTEM_AUDIT_ACE_TYPE => {
            let typed_ace = ace as *const SYSTEM_AUDIT_ACE;
            &(*typed_ace).sid_start as *const ULONG as *const SID
        }
        SYSTEM_MANDATORY_LABEL_ACE_TYPE => {
            let typed_ace = ace as *const SYSTEM_MANDATORY_LABEL_ACE;
            &(*typed_ace).sid_start as *const ULONG as *const SID
        }
        _ => core::ptr::null(),
    }
}

/// Получает ACCESS_MASK из ACE
///
/// # Safety
/// ACE должен быть валидным и иметь поддерживаемый тип
pub unsafe fn ace_get_mask(ace: *const ACE_HEADER) -> ACCESS_MASK {
    if ace.is_null() {
        return 0;
    }

    let ace_type = (*ace).ace_type;

    match ace_type {
        ACCESS_ALLOWED_ACE_TYPE => {
            let typed_ace = ace as *const ACCESS_ALLOWED_ACE;
            (*typed_ace).mask
        }
        ACCESS_DENIED_ACE_TYPE => {
            let typed_ace = ace as *const ACCESS_DENIED_ACE;
            (*typed_ace).mask
        }
        SYSTEM_AUDIT_ACE_TYPE => {
            let typed_ace = ace as *const SYSTEM_AUDIT_ACE;
            (*typed_ace).mask
        }
        SYSTEM_MANDATORY_LABEL_ACE_TYPE => {
            let typed_ace = ace as *const SYSTEM_MANDATORY_LABEL_ACE;
            (*typed_ace).mask
        }
        _ => 0,
    }
}

/// Проверяет, является ли ACE deny ACE
#[inline]
pub fn ace_is_deny(ace: *const ACE_HEADER) -> bool {
    if ace.is_null() {
        return false;
    }

    unsafe { (*ace).ace_type == ACCESS_DENIED_ACE_TYPE }
}

/// Проверяет, является ли ACE allow ACE
#[inline]
pub fn ace_is_allow(ace: *const ACE_HEADER) -> bool {
    if ace.is_null() {
        return false;
    }

    unsafe { (*ace).ace_type == ACCESS_ALLOWED_ACE_TYPE }
}

/// Проверяет, является ли ACE audit ACE
#[inline]
pub fn ace_is_audit(ace: *const ACE_HEADER) -> bool {
    if ace.is_null() {
        return false;
    }

    unsafe { (*ace).ace_type == SYSTEM_AUDIT_ACE_TYPE }
}

/// Проверяет, является ли ACE mandatory label ACE (MIC)
#[inline]
pub fn ace_is_mandatory_label(ace: *const ACE_HEADER) -> bool {
    if ace.is_null() {
        return false;
    }

    unsafe { (*ace).ace_type == SYSTEM_MANDATORY_LABEL_ACE_TYPE }
}

/// Вычисляет размер ACE для стандартного типа (с учетом SID)
pub fn ace_size_for_sid(sid: *const SID) -> USHORT {
    if sid.is_null() {
        return 0;
    }

    // sizeof(ACE_HEADER) + sizeof(ACCESS_MASK) + SID_length - sizeof(ULONG)
    // (минус ULONG потому что sid_start уже включен в структуру)
    let sid_length = unsafe { 
        super::sid::rtl_length_sid(sid) 
    };
    
    let size = core::mem::size_of::<ACE_HEADER>() 
        + core::mem::size_of::<ACCESS_MASK>() 
        + sid_length as usize 
        - core::mem::size_of::<ULONG>();
    
    size as USHORT
}

/// RtlAddAccessAllowedAce - добавляет ACCESS_ALLOWED_ACE в ACL
///
/// # Источник
/// ReactOS: lib/rtl/acl.c RtlAddAccessAllowedAce
pub fn rtl_add_access_allowed_ace(
    acl: *mut ACL,
    ace_revision: ULONG,
    access_mask: ACCESS_MASK,
    sid: *const SID,
) -> NTSTATUS {
    rtl_add_access_allowed_ace_ex(acl, ace_revision, 0, access_mask, sid)
}

/// RtlAddAccessAllowedAceEx - добавляет ACCESS_ALLOWED_ACE с флагами
///
/// # Источник
/// ReactOS: lib/rtl/acl.c RtlAddAccessAllowedAceEx
pub fn rtl_add_access_allowed_ace_ex(
    acl: *mut ACL,
    ace_revision: ULONG,
    ace_flags: ULONG,
    access_mask: ACCESS_MASK,
    sid: *const SID,
) -> NTSTATUS {
    if acl.is_null() || sid.is_null() || !rtl_valid_sid(sid) {
        return STATUS_INVALID_ACL;
    }

    let ace_size = ace_size_for_sid(sid);
    if ace_size == 0 {
        return STATUS_INVALID_ACL;
    }

    unsafe {
        // Находим место для нового ACE
        let mut first_free: *mut ACE_HEADER = core::ptr::null_mut();
        if !rtl_first_free_ace(acl, &mut first_free) {
            return STATUS_INVALID_ACL;
        }

        // Проверяем, что есть место
        let acl_end = (acl as *const u8).add((*acl).acl_size as usize);
        let ace_end = (first_free as *const u8).add(ace_size as usize);
        if ace_end > acl_end {
            return STATUS_INVALID_ACL;
        }

        // Заполняем ACE
        let ace = first_free as *mut ACCESS_ALLOWED_ACE;
        (*ace).header.ace_type = ACCESS_ALLOWED_ACE_TYPE;
        (*ace).header.ace_flags = ace_flags as UCHAR;
        (*ace).header.ace_size = ace_size;
        (*ace).mask = access_mask;

        // Копируем SID
        let sid_dest = &mut (*ace).sid_start as *mut ULONG as *mut u8;
        let sid_length = super::sid::rtl_length_sid(sid) as usize;
        core::ptr::copy_nonoverlapping(sid as *const u8, sid_dest, sid_length);

        // Обновляем revision ACL если нужно
        if ace_revision as UCHAR > (*acl).acl_revision {
            (*acl).acl_revision = ace_revision as UCHAR;
        }

        (*acl).ace_count += 1;
    }

    STATUS_SUCCESS
}

/// RtlAddAccessDeniedAce - добавляет ACCESS_DENIED_ACE в ACL
///
/// # Источник
/// ReactOS: lib/rtl/acl.c RtlAddAccessDeniedAce
pub fn rtl_add_access_denied_ace(
    acl: *mut ACL,
    ace_revision: ULONG,
    access_mask: ACCESS_MASK,
    sid: *const SID,
) -> NTSTATUS {
    rtl_add_access_denied_ace_ex(acl, ace_revision, 0, access_mask, sid)
}

/// RtlAddAccessDeniedAceEx - добавляет ACCESS_DENIED_ACE с флагами
///
/// # Источник
/// ReactOS: lib/rtl/acl.c RtlAddAccessDeniedAceEx
pub fn rtl_add_access_denied_ace_ex(
    acl: *mut ACL,
    ace_revision: ULONG,
    ace_flags: ULONG,
    access_mask: ACCESS_MASK,
    sid: *const SID,
) -> NTSTATUS {
    if acl.is_null() || sid.is_null() || !rtl_valid_sid(sid) {
        return STATUS_INVALID_ACL;
    }

    let ace_size = ace_size_for_sid(sid);
    if ace_size == 0 {
        return STATUS_INVALID_ACL;
    }

    unsafe {
        let mut first_free: *mut ACE_HEADER = core::ptr::null_mut();
        if !rtl_first_free_ace(acl, &mut first_free) {
            return STATUS_INVALID_ACL;
        }

        let acl_end = (acl as *const u8).add((*acl).acl_size as usize);
        let ace_end = (first_free as *const u8).add(ace_size as usize);
        if ace_end > acl_end {
            return STATUS_INVALID_ACL;
        }

        let ace = first_free as *mut ACCESS_DENIED_ACE;
        (*ace).header.ace_type = ACCESS_DENIED_ACE_TYPE;
        (*ace).header.ace_flags = ace_flags as UCHAR;
        (*ace).header.ace_size = ace_size;
        (*ace).mask = access_mask;

        let sid_dest = &mut (*ace).sid_start as *mut ULONG as *mut u8;
        let sid_length = super::sid::rtl_length_sid(sid) as usize;
        core::ptr::copy_nonoverlapping(sid as *const u8, sid_dest, sid_length);

        if ace_revision as UCHAR > (*acl).acl_revision {
            (*acl).acl_revision = ace_revision as UCHAR;
        }

        (*acl).ace_count += 1;
    }

    STATUS_SUCCESS
}

