//! SeAccessCheck - проверка доступа
//!
//! Основная функция SE для проверки прав доступа к объектам.
//!
//! Источники:
//! - ReactOS: ntoskrnl/se/accesschk.c

use crate::nt::{
    NTSTATUS, PVOID, STATUS_SUCCESS, STATUS_ACCESS_DENIED, ACCESS_MASK, ULONG, BOOLEAN,
    PRIVILEGE_SET, SECURITY_DESCRIPTOR,
    GENERIC_READ, GENERIC_WRITE, GENERIC_EXECUTE, GENERIC_ALL,
    MAXIMUM_ALLOWED, ACCESS_SYSTEM_SECURITY, DELETE, READ_CONTROL, WRITE_DAC, WRITE_OWNER,
    SE_DACL_PRESENT, SE_SACL_PRESENT, ACE_HEADER,
    ACCESS_ALLOWED_ACE_TYPE, ACCESS_DENIED_ACE_TYPE, SYSTEM_MANDATORY_LABEL_ACE_TYPE,
    SYSTEM_MANDATORY_LABEL_ACE, SYSTEM_MANDATORY_LABEL_NO_WRITE_UP,
    SYSTEM_MANDATORY_LABEL_NO_READ_UP, SYSTEM_MANDATORY_LABEL_NO_EXECUTE_UP,
    SECURITY_MANDATORY_MEDIUM_RID,
};
use crate::ob::types::GENERIC_MAPPING;

use super::token::TOKEN;
use super::subject::SECURITY_SUBJECT_CONTEXT;
use super::access_state::ACCESS_STATE;
use super::sd::{sd_get_owner_ptr, sd_get_dacl_ptr};
use super::acl::{AceIterator, ace_get_sid, ace_get_mask, ace_is_allow, ace_is_deny};
use super::sid::{rtl_equal_sid, sep_sid_in_token};
use super::priv_::{sep_token_has_privilege, SE_SECURITY_LUID, SE_TAKE_OWNERSHIP_LUID};

// =============================================================================
// Generic mapping
// =============================================================================

/// RtlMapGenericMask - преобразует generic rights в specific
///
/// Заменяет GENERIC_READ/WRITE/EXECUTE/ALL на соответствующие specific права
/// из generic_mapping.
///
/// # Источник
/// ReactOS: lib/rtl/access.c RtlMapGenericMask
pub fn rtl_map_generic_mask(access_mask: *mut ACCESS_MASK, generic_mapping: *const GENERIC_MAPPING) {
    if access_mask.is_null() || generic_mapping.is_null() {
        return;
    }

    unsafe {
        let mut mask = *access_mask;

        if (mask & GENERIC_READ) != 0 {
            mask &= !GENERIC_READ;
            mask |= (*generic_mapping).generic_read;
        }

        if (mask & GENERIC_WRITE) != 0 {
            mask &= !GENERIC_WRITE;
            mask |= (*generic_mapping).generic_write;
        }

        if (mask & GENERIC_EXECUTE) != 0 {
            mask &= !GENERIC_EXECUTE;
            mask |= (*generic_mapping).generic_execute;
        }

        if (mask & GENERIC_ALL) != 0 {
            mask &= !GENERIC_ALL;
            mask |= (*generic_mapping).generic_all;
        }

        *access_mask = mask;
    }
}

// =============================================================================
// SeAccessCheck
// =============================================================================

/// SeAccessCheck - основная функция проверки доступа
///
/// Проверяет, имеет ли субъект (представленный subject_context) запрошенные
/// права доступа к объекту (представленному security_descriptor).
///
/// # Параметры
/// - `security_descriptor` - SD объекта
/// - `subject_context` - security context субъекта
/// - `subject_context_locked` - был ли уже заблокирован subject_context
/// - `desired_access` - запрашиваемые права
/// - `previously_granted_access` - уже полученные права (от parent)
/// - `privileges` - выходной параметр: использованные привилегии
/// - `generic_mapping` - маппинг generic → specific прав
/// - `access_mode` - KernelMode (0) / UserMode (1)
/// - `granted_access` - выходной параметр: полученные права
/// - `access_status` - выходной параметр: статус проверки
///
/// # Возвращает
/// true если доступ разрешен, false если запрещен
///
/// # Источник
/// ReactOS: ntoskrnl/se/accesschk.c SeAccessCheck
#[allow(clippy::too_many_arguments)]
pub fn se_access_check(
    security_descriptor: *const SECURITY_DESCRIPTOR,
    subject_context: *mut SECURITY_SUBJECT_CONTEXT,
    subject_context_locked: BOOLEAN,
    desired_access: ACCESS_MASK,
    previously_granted_access: ACCESS_MASK,
    privileges: *mut *mut PRIVILEGE_SET,
    generic_mapping: *const GENERIC_MAPPING,
    access_mode: i32, // KPROCESSOR_MODE
    granted_access: *mut ACCESS_MASK,
    access_status: *mut NTSTATUS,
) -> bool {
    // Инициализируем выходные параметры
    unsafe {
        if !granted_access.is_null() {
            *granted_access = 0;
        }
        if !access_status.is_null() {
            *access_status = STATUS_ACCESS_DENIED;
        }
        if !privileges.is_null() {
            *privileges = core::ptr::null_mut();
        }
    }

    // Проверяем параметры
    if security_descriptor.is_null() || subject_context.is_null() {
        return false;
    }

    // Получаем effective token
    let token = super::subject::se_get_effective_token(subject_context);
    if token.is_null() {
        return false;
    }

    // Маппим generic права
    let mut remaining_access = desired_access;
    if !generic_mapping.is_null() {
        rtl_map_generic_mask(&mut remaining_access, generic_mapping);
    }

    // Добавляем previously granted access
    let mut current_granted = previously_granted_access;

    // Убираем из remaining то, что уже есть
    remaining_access &= !current_granted;

    // Если ничего не осталось проверять - успех
    if remaining_access == 0 {
        unsafe {
            if !granted_access.is_null() {
                *granted_access = current_granted;
            }
            if !access_status.is_null() {
                *access_status = STATUS_SUCCESS;
            }
        }
        return true;
    }

    // Блокируем subject context если нужно
    if subject_context_locked == 0 {
        super::subject::se_lock_subject_context(subject_context);
    }

    // Выполняем основную проверку
    let result = sep_access_check(
        security_descriptor,
        token,
        remaining_access,
        &mut current_granted,
        privileges,
        generic_mapping,
        access_mode,
    );

    // Разблокируем если мы блокировали
    if subject_context_locked == 0 {
        super::subject::se_unlock_subject_context(subject_context);
    }

    unsafe {
        if !granted_access.is_null() {
            *granted_access = current_granted;
        }
        if !access_status.is_null() {
            *access_status = if result { STATUS_SUCCESS } else { STATUS_ACCESS_DENIED };
        }
    }

    result
}

/// SepAccessCheck - внутренняя функция проверки доступа
///
/// Реализует алгоритм проверки DACL + MIC (Win7).
fn sep_access_check(
    sd: *const SECURITY_DESCRIPTOR,
    token: *mut TOKEN,
    desired_access: ACCESS_MASK,
    granted_access: *mut ACCESS_MASK,
    privileges: *mut *mut PRIVILEGE_SET,
    generic_mapping: *const GENERIC_MAPPING,
    access_mode: i32,
) -> bool {
    unsafe {
        let mut remaining = desired_access;
        let mut granted = *granted_access;

        // === Шаг 0: MIC (Mandatory Integrity Control) - Win7 ===
        // Проверяем integrity level токена против объекта
        if !sep_mandatory_access_check(sd, token, remaining) {
            // Генерируем audit event для отказа
            sep_check_audit(sd, token, remaining, false);
            *granted_access = granted;
            return false;
        }

        // === Шаг 1: ACCESS_SYSTEM_SECURITY требует SeSecurityPrivilege ===
        if (remaining & ACCESS_SYSTEM_SECURITY) != 0 {
            if sep_token_has_privilege(
                (*token).privileges,
                (*token).privilege_count,
                &SE_SECURITY_LUID,
            ) {
                granted |= ACCESS_SYSTEM_SECURITY;
                remaining &= !ACCESS_SYSTEM_SECURITY;
                // TODO: записать использованную привилегию
            } else {
                return false; // Нет привилегии - сразу отказ
            }
        }

        // === Шаг 2: WRITE_OWNER - проверяем SeTakeOwnershipPrivilege ===
        if (remaining & WRITE_OWNER) != 0 {
            if sep_token_has_privilege(
                (*token).privileges,
                (*token).privilege_count,
                &SE_TAKE_OWNERSHIP_LUID,
            ) {
                granted |= WRITE_OWNER;
                remaining &= !WRITE_OWNER;
            }
        }

        // === Шаг 3: Owner имеет неявные права ===
        let owner_sid = sd_get_owner_ptr(sd);
        let is_owner = !owner_sid.is_null() && sep_is_token_owner(token, owner_sid);

        if is_owner {
            // Owner имеет READ_CONTROL и WRITE_DAC неявно
            if (remaining & READ_CONTROL) != 0 {
                granted |= READ_CONTROL;
                remaining &= !READ_CONTROL;
            }
            if (remaining & WRITE_DAC) != 0 {
                granted |= WRITE_DAC;
                remaining &= !WRITE_DAC;
            }
        }

        // Если осталось только то, что уже granted - успех
        if remaining == 0 {
            *granted_access = granted;
            return true;
        }

        // === Шаг 4: Проверка DACL ===
        let control = (*sd).control;

        // Если DACL_PRESENT не установлен - доступ разрешен (null DACL = full access)
        if (control & SE_DACL_PRESENT) == 0 {
            granted |= remaining;
            *granted_access = granted;
            return true;
        }

        let dacl = sd_get_dacl_ptr(sd);

        // Если DACL = null (present но пустой указатель) - полный доступ
        if dacl.is_null() {
            granted |= remaining;
            *granted_access = granted;
            return true;
        }

        // Если DACL пустой (0 ACE) - нет доступа
        if (*dacl).ace_count == 0 {
            *granted_access = granted;
            return remaining == 0;
        }

        // === Шаг 5: Обход ACE в DACL ===
        // Первая проверка: обычные SIDs токена
        let normal_result = sep_check_dacl_access(
            dacl, token, remaining, &mut granted, generic_mapping, false,
        );

        if !normal_result {
            *granted_access = granted;
            return false;
        }

        // === Шаг 6: Restricted tokens - двойная проверка (Win7) ===
        // Если токен restricted, нужно проверить ещё раз с restricted SIDs
        if (*token).is_restricted() && (*token).restricted_sid_count > 0 {
            let mut restricted_granted: ACCESS_MASK = 0;
            let restricted_remaining = desired_access;
            
            let restricted_result = sep_check_dacl_access(
                dacl, token, restricted_remaining, &mut restricted_granted, generic_mapping, true,
            );

            if !restricted_result {
                *granted_access = granted;
                return false;
            }

            // Результат = пересечение обычных и restricted прав
            granted &= restricted_granted;
        }

        // === Шаг 7: Audit (SACL processing) ===
        // Генерируем audit event если требуется
        sep_check_audit(sd, token, desired_access, true);

        *granted_access = granted;
        true
    }
}

/// SepCheckAudit - проверяет SACL и генерирует audit events
///
/// Вызывается в конце SeAccessCheck для обработки SYSTEM_AUDIT_ACE.
unsafe fn sep_check_audit(
    sd: *const SECURITY_DESCRIPTOR,
    token: *mut TOKEN,
    desired_access: ACCESS_MASK,
    access_granted: bool,
) {
    use super::audit::{se_is_auditing_enabled, se_examine_sacl};
    use super::sd::sd_get_sacl_ptr;

    // Если аудит глобально отключен - пропускаем
    if !se_is_auditing_enabled() {
        return;
    }

    // Проверяем наличие SACL
    if unsafe { ((*sd).control & SE_SACL_PRESENT) == 0 } {
        return;
    }

    let sacl = sd_get_sacl_ptr(sd);
    if sacl.is_null() {
        return;
    }

    // Проверяем SACL для генерации audit events
    let mut generate_audit: crate::nt::BOOLEAN = 0;
    let mut generate_alarm: crate::nt::BOOLEAN = 0;

    se_examine_sacl(
        sacl,
        token,
        desired_access,
        if access_granted { 1 } else { 0 },
        &mut generate_audit,
        &mut generate_alarm,
    );

    // TODO: если generate_audit != 0, сгенерировать audit event
    // Это будет реализовано при интеграции с LSA
}

/// Проверка DACL с указанным набором SIDs
///
/// # Параметры
/// - `use_restricted` - использовать restricted SIDs вместо обычных
unsafe fn sep_check_dacl_access(
    dacl: *const crate::nt::ACL,
    token: *mut TOKEN,
    mut remaining: ACCESS_MASK,
    granted: &mut ACCESS_MASK,
    generic_mapping: *const GENERIC_MAPPING,
    use_restricted: bool,
) -> bool {
    // Алгоритм Win7:
    // 1. Deny ACE обрабатываются первыми (если matching SID)
    // 2. Allow ACE накапливают права
    // 3. Порядок ACE в DACL важен

    if let Some(iter) = AceIterator::new(dacl) {
        for ace in iter {
            let ace_type = unsafe { (*ace).ace_type };

            match ace_type {
                ACCESS_DENIED_ACE_TYPE => {
                    // Deny ACE
                    let ace_sid = unsafe { ace_get_sid(ace) };
                    if !ace_sid.is_null() && sep_sid_matches_token_ex(token, ace_sid, use_restricted) {
                        let ace_mask = unsafe { ace_get_mask(ace) };
                        // Если какие-то из запрошенных прав запрещены - отказ
                        if (remaining & ace_mask) != 0 {
                            return false;
                        }
                    }
                }
                ACCESS_ALLOWED_ACE_TYPE => {
                    // Allow ACE
                    let ace_sid = unsafe { ace_get_sid(ace) };
                    if !ace_sid.is_null() && sep_sid_matches_token_ex(token, ace_sid, use_restricted) {
                        let mut ace_mask = unsafe { ace_get_mask(ace) };
                        
                        // Маппим generic права в ACE
                        if !generic_mapping.is_null() {
                            rtl_map_generic_mask(&mut ace_mask, generic_mapping);
                        }

                        // Добавляем разрешенные права
                        *granted |= ace_mask & remaining;
                        remaining &= !ace_mask;

                        // Если все права получены - успех
                        if remaining == 0 {
                            return true;
                        }
                    }
                }
                _ => {
                    // TODO: Другие типы ACE (audit, object ACE) - пока пропускаем
                }
            }
        }
    }

    // После обхода всех ACE
    remaining == 0
}

/// Проверяет, является ли user токена владельцем
fn sep_is_token_owner(token: *mut TOKEN, owner_sid: *const crate::nt::SID) -> bool {
    if token.is_null() || owner_sid.is_null() {
        return false;
    }

    unsafe {
        // Сравниваем с user SID токена
        if !(*token).user_sid.is_null() && rtl_equal_sid((*token).user_sid, owner_sid) {
            return true;
        }

        // Также проверяем owner SID токена (если установлен)
        if !(*token).owner.is_null() && rtl_equal_sid((*token).owner, owner_sid) {
            return true;
        }

        false
    }
}

/// Проверяет, соответствует ли SID токену (user или одна из групп)
fn sep_sid_matches_token(token: *mut TOKEN, sid: *const crate::nt::SID) -> bool {
    sep_sid_matches_token_ex(token, sid, false)
}

/// Проверяет, соответствует ли SID токену (расширенная версия)
///
/// # Параметры
/// - `use_restricted` - если true, проверяем только restricted SIDs
fn sep_sid_matches_token_ex(token: *mut TOKEN, sid: *const crate::nt::SID, use_restricted: bool) -> bool {
    if token.is_null() || sid.is_null() {
        return false;
    }

    unsafe {
        if use_restricted {
            // Для restricted проверки - только restricted SIDs
            if sep_sid_in_token((*token).restricted_sids, (*token).restricted_sid_count, sid, false) {
                return true;
            }
        } else {
            // Обычная проверка: user + groups
            // Проверяем user SID
            if !(*token).user_sid.is_null() && rtl_equal_sid((*token).user_sid, sid) {
                return true;
            }

            // Проверяем группы (только enabled, не deny-only)
            if sep_sid_in_token((*token).groups, (*token).group_count, sid, false) {
                return true;
            }
        }

        false
    }
}

// =============================================================================
// MIC (Mandatory Integrity Control) - Win7
// =============================================================================

/// Получает mandatory label из SACL объекта
///
/// Возвращает (integrity_level, policy_mask) или (MEDIUM, NO_WRITE_UP) если нет label
unsafe fn sep_get_object_integrity(sd: *const SECURITY_DESCRIPTOR) -> (ULONG, ACCESS_MASK) {
    unsafe {
        use super::sd::sd_get_sacl_ptr;
        use super::acl::AceIterator;
        use super::sid::rtl_get_rid;

        // Default: Medium integrity, no-write-up policy
        let default_level = SECURITY_MANDATORY_MEDIUM_RID;
        let default_policy = SYSTEM_MANDATORY_LABEL_NO_WRITE_UP;

        if sd.is_null() {
            return (default_level, default_policy);
        }

        // Проверяем наличие SACL
        if ((*sd).control & SE_SACL_PRESENT) == 0 {
            return (default_level, default_policy);
        }

        let sacl = sd_get_sacl_ptr(sd);
        if sacl.is_null() || (*sacl).ace_count == 0 {
            return (default_level, default_policy);
        }

        // Ищем SYSTEM_MANDATORY_LABEL_ACE в SACL
        if let Some(iter) = AceIterator::new(sacl) {
            for ace in iter {
                if (*ace).ace_type == SYSTEM_MANDATORY_LABEL_ACE_TYPE {
                    let label_ace = ace as *const SYSTEM_MANDATORY_LABEL_ACE;
                    let policy = (*label_ace).mask;
                    
                    // SID начинается после поля sid_start
                    let sid_ptr = &(*label_ace).sid_start as *const ULONG as *const crate::nt::SID;
                    let integrity_level = rtl_get_rid(sid_ptr);
                    
                    return (integrity_level, policy);
                }
            }
        }

        (default_level, default_policy)
    }
}

/// SepMandatoryAccessCheck - проверка MIC (Win7)
///
/// Реализует "no write-up" и другие mandatory policies.
/// Возвращает false если доступ запрещён по MIC.
///
/// # Win7 семантика
/// - Если integrity level токена < integrity level объекта:
///   - NO_WRITE_UP: запрещён WRITE/DELETE/APPEND доступ
///   - NO_READ_UP: запрещён READ доступ
///   - NO_EXECUTE_UP: запрещён EXECUTE доступ
unsafe fn sep_mandatory_access_check(
    sd: *const SECURITY_DESCRIPTOR,
    token: *mut TOKEN,
    desired_access: ACCESS_MASK,
) -> bool {
    unsafe {
        // Получаем integrity level токена
        let token_integrity = (*token).integrity_level_index;

        // Получаем integrity level и policy объекта
        let (object_integrity, policy) = sep_get_object_integrity(sd);

    // Если token integrity >= object integrity, MIC не блокирует
    if token_integrity >= object_integrity {
        return true;
    }

    // Token integrity < Object integrity - применяем policy

    // NO_WRITE_UP: блокируем write-подобные операции
    if (policy & SYSTEM_MANDATORY_LABEL_NO_WRITE_UP) != 0 {
        // Write operations: WRITE_DAC, WRITE_OWNER, DELETE, APPEND, WRITE
        const WRITE_OPERATIONS: ACCESS_MASK = WRITE_DAC | WRITE_OWNER | DELETE
            | 0x0002    // FILE_WRITE_DATA / KEY_SET_VALUE
            | 0x0004    // FILE_APPEND_DATA / KEY_CREATE_SUB_KEY
            | 0x0100;   // FILE_WRITE_ATTRIBUTES
        
        if (desired_access & WRITE_OPERATIONS) != 0 {
            return false;
        }
    }

    // NO_READ_UP: блокируем read операции
    if (policy & SYSTEM_MANDATORY_LABEL_NO_READ_UP) != 0 {
        const READ_OPERATIONS: ACCESS_MASK = READ_CONTROL
            | 0x0001    // FILE_READ_DATA / KEY_QUERY_VALUE
            | 0x0080;   // FILE_READ_ATTRIBUTES
        
        if (desired_access & READ_OPERATIONS) != 0 {
            return false;
        }
    }

        // NO_EXECUTE_UP: блокируем execute операции
        if (policy & SYSTEM_MANDATORY_LABEL_NO_EXECUTE_UP) != 0 {
            const EXECUTE_OPERATIONS: ACCESS_MASK = 0x0020; // FILE_EXECUTE
            
            if (desired_access & EXECUTE_OPERATIONS) != 0 {
                return false;
            }
        }

        true
    }
}

// =============================================================================
// SeAccessCheckFromState
// =============================================================================

/// SeAccessCheckFromState - проверка доступа из ACCESS_STATE
///
/// Удобная обертка для использования из OB.
pub fn se_access_check_from_state(
    access_state: *mut ACCESS_STATE,
    security_descriptor: *const SECURITY_DESCRIPTOR,
    desired_access: ACCESS_MASK,
    access_mode: i32,
    granted_access: *mut ACCESS_MASK,
) -> NTSTATUS {
    if access_state.is_null() {
        return STATUS_ACCESS_DENIED;
    }

    unsafe {
        let mut privileges: *mut PRIVILEGE_SET = core::ptr::null_mut();
        let mut status = STATUS_ACCESS_DENIED;

        let generic_mapping = if !(*access_state).aux_data.is_null() {
            &(*(*access_state).aux_data).generic_mapping as *const GENERIC_MAPPING
        } else {
            core::ptr::null()
        };

        let result = se_access_check(
            security_descriptor,
            &mut (*access_state).subject_security_context,
            0, // not locked
            desired_access,
            (*access_state).previously_granted_access,
            &mut privileges,
            generic_mapping,
            access_mode,
            granted_access,
            &mut status,
        );

        if result {
            // Обновляем access state
            if !granted_access.is_null() {
                (*access_state).previously_granted_access = *granted_access;
            }
            (*access_state).remaining_desired_access &= !*granted_access;
        }

        status
    }
}

// =============================================================================
// SeFastTraverseCheck (для NTFS / быстрая проверка)
// =============================================================================

/// SeFastTraverseCheck - быстрая проверка traverse access
///
/// Оптимизированная проверка для директорий (FILE_TRAVERSE).
/// Возвращает true если traverse разрешен без полной проверки DACL.
pub fn se_fast_traverse_check(
    security_descriptor: *const SECURITY_DESCRIPTOR,
    access_state: *const ACCESS_STATE,
    _traverse_access: ACCESS_MASK,
    access_mode: i32,
) -> bool {
    // Для KernelMode всегда разрешаем traverse
    if access_mode == 0 {
        return true;
    }

    // Если нет SD - разрешаем
    if security_descriptor.is_null() {
        return true;
    }

    // Проверяем bypass traverse checking privilege
    if !access_state.is_null() {
        unsafe {
            let token = super::subject::se_get_effective_token(
                &(*access_state).subject_security_context as *const _ as *mut _,
            );
            if !token.is_null() {
                let flags = (*token).token_flags.load(core::sync::atomic::Ordering::Acquire);
                if (flags & super::token::TOKEN_HAS_TRAVERSE_PRIVILEGE) != 0 {
                    return true;
                }
            }
        }
    }

    // Нужна полная проверка
    false
}

