//! Subject Context - контекст субъекта безопасности
//!
//! SECURITY_SUBJECT_CONTEXT хранит security context текущего потока/процесса
//! для использования в проверках доступа.
//!
//! Источники:
//! - ReactOS: ntoskrnl/se/subject.c

use core::sync::atomic::Ordering;

use crate::nt::{PVOID, UCHAR};
use crate::se::token::TOKEN;

// =============================================================================
// SECURITY_SUBJECT_CONTEXT
// =============================================================================

/// SECURITY_SUBJECT_CONTEXT - контекст субъекта для access check
///
/// Захватывает primary token процесса и impersonation token потока (если есть).
#[repr(C)]
pub struct SECURITY_SUBJECT_CONTEXT {
    /// Client token (impersonation token потока, если есть)
    pub client_token: *mut TOKEN,
    /// Impersonation level клиентского токена
    pub impersonation_level: u32, // SECURITY_IMPERSONATION_LEVEL as u32
    /// Primary token процесса
    pub primary_token: *mut TOKEN,
    /// Указатель на процесс (для reference counting)
    pub process_audit_id: PVOID,
}

impl SECURITY_SUBJECT_CONTEXT {
    pub const fn new() -> Self {
        Self {
            client_token: core::ptr::null_mut(),
            impersonation_level: 0,
            primary_token: core::ptr::null_mut(),
            process_audit_id: core::ptr::null_mut(),
        }
    }
}

impl Default for SECURITY_SUBJECT_CONTEXT {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Subject Context functions
// =============================================================================

/// SeCaptureSubjectContext - захватывает security context текущего потока
///
/// Получает primary token процесса и impersonation token потока (если установлен).
/// После использования должен быть освобожден через SeReleaseSubjectContext.
///
/// # Источник
/// ReactOS: ntoskrnl/se/subject.c SeCaptureSubjectContext
pub fn se_capture_subject_context(subject_context: *mut SECURITY_SUBJECT_CONTEXT) {
    if subject_context.is_null() {
        return;
    }

    se_capture_subject_context_ex(
        core::ptr::null_mut(), // current thread
        core::ptr::null_mut(), // current process
        subject_context,
    );
}

/// SeCaptureSubjectContextEx - захватывает subject context для указанного потока/процесса
///
/// # Источник
/// ReactOS: ntoskrnl/se/subject.c SeCaptureSubjectContextEx
pub fn se_capture_subject_context_ex(
    thread: PVOID,  // PETHREAD, null = current
    process: PVOID, // PEPROCESS, null = current
    subject_context: *mut SECURITY_SUBJECT_CONTEXT,
) {
    use crate::ps::process::{ps_get_current_process, ps_get_current_thread, EPROCESS};
    use crate::ps::thread::ETHREAD;
    use crate::ps::types::PS_CROSS_THREAD_FLAGS_IMPERSONATING;

    if subject_context.is_null() {
        return;
    }

    unsafe {
        // Получаем процесс
        let proc = if process.is_null() {
            ps_get_current_process()
        } else {
            process as *mut EPROCESS
        };

        // Получаем поток
        let thr = if thread.is_null() {
            ps_get_current_thread() as *mut ETHREAD
        } else {
            thread as *mut ETHREAD
        };

        // Primary token из процесса
        (*subject_context).primary_token = core::ptr::null_mut();
        (*subject_context).client_token = core::ptr::null_mut();
        (*subject_context).impersonation_level = 0;
        (*subject_context).process_audit_id = proc as PVOID;

        if !proc.is_null() {
            // Получаем primary token из EPROCESS.token
            let token_ptr = (*proc).token.load(Ordering::Acquire);
            if !token_ptr.is_null() {
                // TODO: proper EX_FAST_REF handling
                // Пока просто используем указатель напрямую
                (*subject_context).primary_token = token_ptr as *mut TOKEN;
            }
        }

        // Impersonation token из потока
        if !thr.is_null() {
            let cross_flags = (*thr).cross_thread_flags.load(Ordering::Acquire);
            if (cross_flags & PS_CROSS_THREAD_FLAGS_IMPERSONATING) != 0 {
                // Поток выполняет impersonation
                // TODO: получить impersonation token из ETHREAD.impersonation_info
                // Пока оставляем null
            }
        }

        // TODO: reference tokens properly
    }
}

/// SeReleaseSubjectContext - освобождает захваченный subject context
///
/// # Источник
/// ReactOS: ntoskrnl/se/subject.c SeReleaseSubjectContext
pub fn se_release_subject_context(subject_context: *mut SECURITY_SUBJECT_CONTEXT) {
    if subject_context.is_null() {
        return;
    }

    unsafe {
        // TODO: dereference tokens properly
        (*subject_context).client_token = core::ptr::null_mut();
        (*subject_context).primary_token = core::ptr::null_mut();
        (*subject_context).process_audit_id = core::ptr::null_mut();
    }
}

/// SeLockSubjectContext - блокирует subject context
///
/// Предотвращает изменение токенов во время access check.
///
/// # Источник
/// ReactOS: ntoskrnl/se/subject.c SeLockSubjectContext
pub fn se_lock_subject_context(subject_context: *mut SECURITY_SUBJECT_CONTEXT) {
    // В текущей реализации lock не требуется, т.к. мы не делаем
    // reference counting на токены (пока что)
    let _ = subject_context;
}

/// SeUnlockSubjectContext - разблокирует subject context
///
/// # Источник
/// ReactOS: ntoskrnl/se/subject.c SeUnlockSubjectContext
pub fn se_unlock_subject_context(subject_context: *mut SECURITY_SUBJECT_CONTEXT) {
    let _ = subject_context;
}

/// SeGetEffectiveToken - получает effective token из subject context
///
/// Возвращает impersonation token если есть, иначе primary token.
pub fn se_get_effective_token(subject_context: *const SECURITY_SUBJECT_CONTEXT) -> *mut TOKEN {
    if subject_context.is_null() {
        return core::ptr::null_mut();
    }

    unsafe {
        if !(*subject_context).client_token.is_null() {
            (*subject_context).client_token
        } else {
            (*subject_context).primary_token
        }
    }
}

/// Проверяет, выполняется ли impersonation в subject context
pub fn se_is_impersonating(subject_context: *const SECURITY_SUBJECT_CONTEXT) -> bool {
    if subject_context.is_null() {
        return false;
    }

    unsafe { !(*subject_context).client_token.is_null() }
}

