//! ACCESS_STATE - состояние запроса доступа
//!
//! ACCESS_STATE используется для передачи информации между OB и SE
//! при проверках доступа.
//!
//! Источники:
//! - ReactOS: ntoskrnl/se/accesschk.c

use crate::nt::{
    NTSTATUS, PVOID, STATUS_SUCCESS, ACCESS_MASK, ULONG, UCHAR, BOOLEAN,
    PRIVILEGE_SET, LUID,
};
use crate::ob::types::GENERIC_MAPPING;

use super::subject::SECURITY_SUBJECT_CONTEXT;

// =============================================================================
// AUX_ACCESS_DATA
// =============================================================================

/// AUX_ACCESS_DATA - вспомогательные данные для access check
#[repr(C)]
pub struct AUX_ACCESS_DATA {
    /// Privilege set использованный при access check
    pub privilege_set: *mut PRIVILEGE_SET,
    /// Generic mapping для объекта
    pub generic_mapping: GENERIC_MAPPING,
    /// Object type list для object-specific access checks
    pub object_type_list: PVOID,
    /// Количество элементов в object type list
    pub object_type_list_length: ULONG,
    /// Reserved
    pub reserved: ULONG,
}

impl AUX_ACCESS_DATA {
    pub const fn new() -> Self {
        Self {
            privilege_set: core::ptr::null_mut(),
            generic_mapping: GENERIC_MAPPING {
                generic_read: 0,
                generic_write: 0,
                generic_execute: 0,
                generic_all: 0,
            },
            object_type_list: core::ptr::null_mut(),
            object_type_list_length: 0,
            reserved: 0,
        }
    }
}

impl Default for AUX_ACCESS_DATA {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// ACCESS_STATE
// =============================================================================

/// ACCESS_STATE - состояние запроса доступа
///
/// Создается в начале операции open/create и передается через OB в SE
/// для проверки доступа.
#[repr(C)]
pub struct ACCESS_STATE {
    /// Текущий операционный ID (для аудита)
    pub operation_id: LUID,
    /// Флаг безопасности (был ли выполнен access check)
    pub security_evaluated: BOOLEAN,
    /// Флаг генерации аудита
    pub generate_audit: BOOLEAN,
    /// Флаг успешного аудита
    pub generate_on_close: BOOLEAN,
    /// Privilege use for audit
    pub privilege_used_for_access: BOOLEAN,
    /// Flags
    pub flags: ULONG,
    /// Remaining desired access (то, что ещё не проверено)
    pub remaining_desired_access: ACCESS_MASK,
    /// Previously granted access (уже полученные права от parent)
    pub previously_granted_access: ACCESS_MASK,
    /// Original desired access (изначальный запрос)
    pub original_desired_access: ACCESS_MASK,
    /// Subject security context
    pub subject_security_context: SECURITY_SUBJECT_CONTEXT,
    /// Security descriptor объекта (если известен)
    pub security_descriptor: PVOID,
    /// Auxiliary data
    pub aux_data: *mut AUX_ACCESS_DATA,
    /// Небольшой буфер для privilege set (чтобы избежать аллокации)
    pub privileges: PrivilegeSetBuffer,
    /// Audit privilege allocated flag
    pub audit_privilege: BOOLEAN,
    /// Object name for audit
    pub object_name: crate::nt::UNICODE_STRING,
    /// Object type name for audit
    pub object_type_name: crate::nt::UNICODE_STRING,
}

/// Встроенный буфер для небольшого privilege set
#[repr(C)]
pub struct PrivilegeSetBuffer {
    pub privilege_count: ULONG,
    pub control: ULONG,
    pub privilege: [crate::nt::LUID_AND_ATTRIBUTES; 3],
}

impl PrivilegeSetBuffer {
    pub const fn new() -> Self {
        Self {
            privilege_count: 0,
            control: 0,
            privilege: [
                crate::nt::LUID_AND_ATTRIBUTES::new(LUID::new(0, 0), 0),
                crate::nt::LUID_AND_ATTRIBUTES::new(LUID::new(0, 0), 0),
                crate::nt::LUID_AND_ATTRIBUTES::new(LUID::new(0, 0), 0),
            ],
        }
    }

    /// Возвращает указатель как PRIVILEGE_SET
    pub fn as_privilege_set(&mut self) -> *mut PRIVILEGE_SET {
        self as *mut Self as *mut PRIVILEGE_SET
    }
}

impl ACCESS_STATE {
    pub const fn new() -> Self {
        Self {
            operation_id: LUID::new(0, 0),
            security_evaluated: 0,
            generate_audit: 0,
            generate_on_close: 0,
            privilege_used_for_access: 0,
            flags: 0,
            remaining_desired_access: 0,
            previously_granted_access: 0,
            original_desired_access: 0,
            subject_security_context: SECURITY_SUBJECT_CONTEXT::new(),
            security_descriptor: core::ptr::null_mut(),
            aux_data: core::ptr::null_mut(),
            privileges: PrivilegeSetBuffer::new(),
            audit_privilege: 0,
            object_name: crate::nt::UNICODE_STRING::new(),
            object_type_name: crate::nt::UNICODE_STRING::new(),
        }
    }
}

impl Default for ACCESS_STATE {
    fn default() -> Self {
        Self::new()
    }
}

// ACCESS_STATE flags
pub const ACCESS_STATE_OBJECT_CREATED: ULONG = 0x0001;
pub const ACCESS_STATE_TRAVERSE_CHECK: ULONG = 0x0002;
pub const ACCESS_STATE_MAXIMUM_ALLOWED: ULONG = 0x0004;

// =============================================================================
// ACCESS_STATE functions
// =============================================================================

/// SeCreateAccessState - создает и инициализирует ACCESS_STATE
///
/// # Источник
/// ReactOS: ntoskrnl/se/accesschk.c SeCreateAccessState
pub fn se_create_access_state(
    access_state: *mut ACCESS_STATE,
    aux_data: *mut AUX_ACCESS_DATA,
    desired_access: ACCESS_MASK,
    generic_mapping: *const GENERIC_MAPPING,
) -> NTSTATUS {
    se_create_access_state_ex(
        core::ptr::null_mut(), // current thread
        core::ptr::null_mut(), // current process
        access_state,
        aux_data,
        desired_access,
        generic_mapping,
    )
}

/// SeCreateAccessStateEx - создает ACCESS_STATE для указанного потока/процесса
///
/// # Источник
/// ReactOS: ntoskrnl/se/accesschk.c SeCreateAccessStateEx
pub fn se_create_access_state_ex(
    thread: PVOID,
    process: PVOID,
    access_state: *mut ACCESS_STATE,
    aux_data: *mut AUX_ACCESS_DATA,
    desired_access: ACCESS_MASK,
    generic_mapping: *const GENERIC_MAPPING,
) -> NTSTATUS {
    use crate::nt::MAXIMUM_ALLOWED;
    use super::subject::se_capture_subject_context_ex;

    if access_state.is_null() {
        return crate::nt::STATUS_INVALID_PARAMETER;
    }

    unsafe {
        // Обнуляем структуру
        core::ptr::write_bytes(access_state, 0, 1);

        // Захватываем subject context
        se_capture_subject_context_ex(
            thread,
            process,
            &mut (*access_state).subject_security_context,
        );

        // Сохраняем desired access
        (*access_state).original_desired_access = desired_access;
        (*access_state).remaining_desired_access = desired_access;

        // Проверяем MAXIMUM_ALLOWED
        if (desired_access & MAXIMUM_ALLOWED) != 0 {
            (*access_state).flags |= ACCESS_STATE_MAXIMUM_ALLOWED;
        }

        // Настраиваем aux_data
        if !aux_data.is_null() {
            (*access_state).aux_data = aux_data;
            
            if !generic_mapping.is_null() {
                (*aux_data).generic_mapping = *generic_mapping;
            }
        }

        // Инициализируем встроенный privilege set
        (*access_state).privileges.privilege_count = 0;
        (*access_state).privileges.control = 0;
    }

    STATUS_SUCCESS
}

/// SeDeleteAccessState - освобождает ACCESS_STATE
///
/// # Источник
/// ReactOS: ntoskrnl/se/accesschk.c SeDeleteAccessState
pub fn se_delete_access_state(access_state: *mut ACCESS_STATE) {
    use super::subject::se_release_subject_context;

    if access_state.is_null() {
        return;
    }

    unsafe {
        // Освобождаем subject context
        se_release_subject_context(&mut (*access_state).subject_security_context);

        // Освобождаем privilege set если был аллоцирован отдельно
        // (не встроенный буфер)
        if (*access_state).audit_privilege != 0 {
            // TODO: освободить если был аллоцирован
        }
    }
}

/// SeSetAccessStateGenericMapping - устанавливает generic mapping в ACCESS_STATE
///
/// # Источник
/// ReactOS: ntoskrnl/se/accesschk.c SeSetAccessStateGenericMapping
pub fn se_set_access_state_generic_mapping(
    access_state: *mut ACCESS_STATE,
    generic_mapping: *const GENERIC_MAPPING,
) {
    if access_state.is_null() || generic_mapping.is_null() {
        return;
    }

    unsafe {
        if !(*access_state).aux_data.is_null() {
            (*(*access_state).aux_data).generic_mapping = *generic_mapping;
        }
    }
}

/// SeAppendPrivileges - добавляет привилегии к ACCESS_STATE
///
/// Используется для записи информации о привилегиях, использованных при access check.
///
/// # Источник
/// ReactOS: ntoskrnl/se/accesschk.c SeAppendPrivileges
pub fn se_append_privileges(
    access_state: *mut ACCESS_STATE,
    privileges: *const PRIVILEGE_SET,
) -> NTSTATUS {
    if access_state.is_null() || privileges.is_null() {
        return STATUS_SUCCESS;
    }

    unsafe {
        let src_count = (*privileges).privilege_count as usize;
        if src_count == 0 {
            return STATUS_SUCCESS;
        }

        // Пытаемся использовать встроенный буфер
        let current_count = (*access_state).privileges.privilege_count as usize;
        let available = 3 - current_count;

        if src_count <= available {
            // Помещается во встроенный буфер
            for i in 0..src_count {
                (*access_state).privileges.privilege[current_count + i] =
                    *(*privileges).privilege.as_ptr().add(i);
            }
            (*access_state).privileges.privilege_count += src_count as ULONG;
        } else {
            // TODO: аллоцировать отдельный буфер
            // Пока игнорируем лишние привилегии
        }
    }

    STATUS_SUCCESS
}

