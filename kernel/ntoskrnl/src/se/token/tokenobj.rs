//! Token Object Type
//!
//! Интеграция токенов с Object Manager.
//!
//! Источники:
//! - ReactOS: ntoskrnl/se/token.c

use core::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

use crate::nt::{
    NTSTATUS, PVOID, STATUS_SUCCESS, ULONG, ACCESS_MASK, UNICODE_STRING,
    TOKEN_ALL_ACCESS, TOKEN_READ, TOKEN_WRITE,
    READ_CONTROL,
};
use crate::ob::types::{OBJECT_TYPE, OBJECT_TYPE_INITIALIZER, GENERIC_MAPPING};

use super::token::TOKEN;

// =============================================================================
// Token Object Type
// =============================================================================

/// SeTokenObjectType - глобальный тип объекта Token
static SE_TOKEN_OBJECT_TYPE: AtomicPtr<OBJECT_TYPE> = AtomicPtr::new(core::ptr::null_mut());

/// Флаг инициализации типа Token
static TOKEN_TYPE_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Token generic mapping
pub const SE_TOKEN_MAPPING: GENERIC_MAPPING = GENERIC_MAPPING {
    generic_read: TOKEN_READ,
    generic_write: TOKEN_WRITE,
    generic_execute: READ_CONTROL | crate::nt::TOKEN_IMPERSONATE,
    generic_all: TOKEN_ALL_ACCESS,
};

/// Возвращает указатель на SeTokenObjectType
pub fn se_token_object_type() -> *mut OBJECT_TYPE {
    SE_TOKEN_OBJECT_TYPE.load(Ordering::Acquire)
}

/// Delete procedure для токенов
unsafe extern "win64" fn sep_token_delete_method(object: PVOID) {
    // object указывает на body токена (т.е. на TOKEN)
    if !object.is_null() {
        let token = object as *mut TOKEN;
        // Освобождаем связанный токен если есть
        let linked = (*token).linked_token.swap(core::ptr::null_mut(), Ordering::AcqRel);
        if !linked.is_null() {
            // TODO: dereference linked token через OB
        }
        // Основное освобождение происходит через OB pool free
        // sep_delete_token вызывать не нужно - OB сам освободит память
    }
}

/// Создает тип объекта Token
///
/// Вызывается из SeInitSystem Phase 0.
pub fn sep_create_token_object_type() -> NTSTATUS {
    use crate::ob::life::ob_create_object_type;
    use crate::ob::types::NON_PAGED_POOL;

    if TOKEN_TYPE_INITIALIZED.load(Ordering::Acquire) {
        return STATUS_SUCCESS;
    }

    // Имя типа "Token"
    static TOKEN_TYPE_NAME: [u16; 6] = [b'T' as u16, b'o' as u16, b'k' as u16, b'e' as u16, b'n' as u16, 0];
    
    let mut type_name = UNICODE_STRING {
        length: 10, // 5 chars * 2 bytes
        maximum_length: 12,
        buffer: TOKEN_TYPE_NAME.as_ptr() as *mut u16,
    };

    let mut type_info = OBJECT_TYPE_INITIALIZER::new();
    type_info.length = core::mem::size_of::<OBJECT_TYPE_INITIALIZER>() as u16;
    type_info.pool_type = NON_PAGED_POOL;
    type_info.valid_access_mask = TOKEN_ALL_ACCESS;
    type_info.generic_mapping = SE_TOKEN_MAPPING;
    type_info.default_non_paged_pool_charge = core::mem::size_of::<TOKEN>() as ULONG;
    type_info.security_required = true;
    type_info.delete_procedure = Some(sep_token_delete_method);

    let mut token_type: *mut OBJECT_TYPE = core::ptr::null_mut();
    
    let status = ob_create_object_type(
        &mut type_name,
        &type_info,
        core::ptr::null_mut(),
        &mut token_type,
    );

    if status == STATUS_SUCCESS {
        SE_TOKEN_OBJECT_TYPE.store(token_type, Ordering::Release);
        TOKEN_TYPE_INITIALIZED.store(true, Ordering::Release);
    }

    status
}

