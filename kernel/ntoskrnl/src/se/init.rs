//! SE Initialization - инициализация подсистемы безопасности
//!
//! Источники:
//! - ReactOS: ntoskrnl/se/semgr.c

use core::sync::atomic::{AtomicU32, Ordering};

use crate::dbg_print;
use crate::nt::{NTSTATUS, STATUS_SUCCESS};

use super::token::sep_create_system_token;
use super::token::sep_create_token_object_type;

// =============================================================================
// SE Initialization state
// =============================================================================

/// Фаза инициализации SE
static SE_INIT_PHASE: AtomicU32 = AtomicU32::new(0);

/// Глобальный System token
static mut SE_SYSTEM_TOKEN: *mut super::token::TOKEN = core::ptr::null_mut();

// =============================================================================
// SeInitSystem
// =============================================================================

/// SeInitSystem - инициализация SE подсистемы
///
/// Вызывается из ExpInitializeExecutive в соответствующих фазах.
///
/// # Фазы
/// - Phase 0: Создание типа объекта Token, well-known SIDs, System token
/// - Phase 1: Дополнительная инициализация (если нужна)
///
/// # Источник
/// ReactOS: ntoskrnl/se/semgr.c SeInitSystem
pub fn se_init_system(phase: u32) -> bool {
    match phase {
        0 => se_init_phase0(),
        1 => se_init_phase1(),
        _ => {
            dbg_print!("[SE] SeInitSystem: unknown phase {}\n", phase);
            false
        }
    }
}

/// Phase 0 инициализации SE
fn se_init_phase0() -> bool {
    dbg_print!("[SE] SeInitSystem Phase 0 starting...\n");

    // 1. Создаем тип объекта Token
    let status = sep_create_token_object_type();
    if status != STATUS_SUCCESS {
        dbg_print!("[SE] Failed to create Token object type: 0x{:08X}\n", status as u32);
        return false;
    }
    dbg_print!("[SE]   Token object type created\n");

    // 2. Создаем System token
    let system_token = sep_create_system_token();
    if system_token.is_null() {
        dbg_print!("[SE] Failed to create System token\n");
        return false;
    }
    
    unsafe {
        SE_SYSTEM_TOKEN = system_token;
    }
    dbg_print!("[SE]   System token created\n");

    // 3. Well-known SIDs уже инициализированы статически в sid.rs

    SE_INIT_PHASE.store(1, Ordering::Release);
    dbg_print!("[SE] SeInitSystem Phase 0 completed successfully\n");
    true
}

/// Phase 1 инициализации SE
fn se_init_phase1() -> bool {
    dbg_print!("[SE] SeInitSystem Phase 1 starting...\n");

    // Фаза 1 пока минимальна - основная работа в фазе 0
    // Здесь можно добавить:
    // - Настройку глобальных политик аудита
    // - Инициализацию LSA callbacks
    // - Загрузку настроек безопасности из реестра

    SE_INIT_PHASE.store(2, Ordering::Release);
    dbg_print!("[SE] SeInitSystem Phase 1 completed successfully\n");
    true
}

// =============================================================================
// System token access
// =============================================================================

/// Возвращает System token
///
/// Используется для назначения токена System process.
pub fn se_get_system_token() -> *mut super::token::TOKEN {
    unsafe { SE_SYSTEM_TOKEN }
}

/// SepAssignPrimaryTokenToProcess - назначает primary token процессу
///
/// Для System process вызывается с System token.
pub fn sep_assign_primary_token_to_process(
    process: *mut crate::ps::process::EPROCESS,
    token: *mut super::token::TOKEN,
) {
    if process.is_null() || token.is_null() {
        return;
    }

    unsafe {
        // TODO: proper reference counting и EX_FAST_REF handling
        // Пока просто устанавливаем указатель
        (*process).token.store(token as *mut core::ffi::c_void, Ordering::Release);
    }
}

/// Назначает System token для System process
///
/// Вызывается из PsInitSystem после создания System process.
pub fn se_assign_system_token_to_system_process(process: *mut crate::ps::process::EPROCESS) {
    let token = se_get_system_token();
    if !token.is_null() {
        sep_assign_primary_token_to_process(process, token);
        dbg_print!("[SE] System token assigned to System process\n");
    }
}

// =============================================================================
// SE state queries
// =============================================================================

/// Возвращает текущую фазу инициализации SE
pub fn se_get_init_phase() -> u32 {
    SE_INIT_PHASE.load(Ordering::Acquire)
}

/// Проверяет, завершена ли инициализация SE
pub fn se_is_initialized() -> bool {
    SE_INIT_PHASE.load(Ordering::Acquire) >= 1
}

