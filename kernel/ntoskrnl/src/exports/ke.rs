//! Kernel Executive экспорты (`Ke*`)
//!
//! Экспорты подсистемы KE: bugcheck, синхронизация, IRQL, планировщик и т.д.

use ntoskrnl::nt::ULONG_PTR;

// =============================================================================
// Bugcheck
// =============================================================================

/// KeBugCheckEx
///
/// Вызывает критическую ошибку (bugcheck/BSOD) с указанными параметрами.
/// Функция не возвращает управление.
///
/// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/nf-wdm-kebugcheckex
nt_export_fn!(
    "KeBugCheckEx" => KeBugCheckEx(
        bug_check_code: u32,
        bug_check_parameter1: ULONG_PTR,
        bug_check_parameter2: ULONG_PTR,
        bug_check_parameter3: ULONG_PTR,
        bug_check_parameter4: ULONG_PTR,
    ) -> ! = ntoskrnl::ke::bugcheck::ke_bug_check_ex
);

// =============================================================================
// Execution Control
// =============================================================================

/// KeStallExecutionProcessor - точная busy-wait задержка
///
/// Используется для коротких задержек при инициализации hardware.
/// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-kestallexecutionprocessor
#[unsafe(export_name = "KeStallExecutionProcessor")]
pub unsafe extern "win64" fn KeStallExecutionProcessor(microseconds: u32) {
    ntoskrnl::hal::pit::ke_stall_execution_processor(microseconds);
}

// =============================================================================
// DPC (Deferred Procedure Call)
// =============================================================================

use ntoskrnl::ke::dpc::{KDPC, PKDEFERRED_ROUTINE, ke_initialize_dpc, ke_insert_queue_dpc, ke_remove_queue_dpc};
use ntoskrnl::nt::{PVOID, BOOLEAN, TRUE, FALSE};

/// KeInitializeDpc - инициализирует DPC объект
#[unsafe(export_name = "KeInitializeDpc")]
pub unsafe extern "win64" fn KeInitializeDpc(
    dpc: *mut KDPC,
    deferred_routine: PKDEFERRED_ROUTINE,
    deferred_context: PVOID,
) {
    if dpc.is_null() {
        return;
    }
    ke_initialize_dpc(&mut *dpc, deferred_routine, deferred_context);
}

/// KeInsertQueueDpc - ставит DPC в очередь
#[unsafe(export_name = "KeInsertQueueDpc")]
pub unsafe extern "win64" fn KeInsertQueueDpc(
    dpc: *mut KDPC,
    system_argument1: PVOID,
    system_argument2: PVOID,
) -> BOOLEAN {
    if dpc.is_null() {
        return FALSE;
    }
    if ke_insert_queue_dpc(&mut *dpc, system_argument1, system_argument2) {
        TRUE
    } else {
        FALSE
    }
}

/// KeRemoveQueueDpc - удаляет DPC из очереди
#[unsafe(export_name = "KeRemoveQueueDpc")]
pub unsafe extern "win64" fn KeRemoveQueueDpc(dpc: *mut KDPC) -> BOOLEAN {
    if dpc.is_null() {
        return FALSE;
    }
    if ke_remove_queue_dpc(&mut *dpc) {
        TRUE
    } else {
        FALSE
    }
}

/// KeSetImportanceDpc - устанавливает важность DPC
#[unsafe(export_name = "KeSetImportanceDpc")]
pub unsafe extern "win64" fn KeSetImportanceDpc(dpc: *mut KDPC, importance: u32) {
    use ntoskrnl::ke::dpc::{ke_set_importance_dpc, KDPC_IMPORTANCE};
    if dpc.is_null() {
        return;
    }
    let imp = match importance {
        0 => KDPC_IMPORTANCE::LowImportance,
        1 => KDPC_IMPORTANCE::MediumImportance,
        2 => KDPC_IMPORTANCE::HighImportance,
        _ => KDPC_IMPORTANCE::MediumImportance,
    };
    ke_set_importance_dpc(&mut *dpc, imp);
}

/// KeSetTargetProcessorDpc - устанавливает целевой процессор для DPC
#[unsafe(export_name = "KeSetTargetProcessorDpc")]
pub unsafe extern "win64" fn KeSetTargetProcessorDpc(dpc: *mut KDPC, number: i8) {
    use ntoskrnl::ke::dpc::ke_set_target_processor_dpc;
    if dpc.is_null() {
        return;
    }
    ke_set_target_processor_dpc(&mut *dpc, number as u8);
}

// =============================================================================
// Timer
// =============================================================================

use ntoskrnl::ke::timer::{KTIMER, TIMER_TYPE, ke_initialize_timer, ke_initialize_timer_ex, ke_set_timer_ex, ke_cancel_timer};

/// KeInitializeTimer - инициализирует таймер (NotificationTimer)
#[unsafe(export_name = "KeInitializeTimer")]
pub unsafe extern "win64" fn KeInitializeTimer(timer: *mut KTIMER) {
    if timer.is_null() {
        return;
    }
    ke_initialize_timer(&mut *timer);
}

/// KeInitializeTimerEx - инициализирует таймер с указанием типа
#[unsafe(export_name = "KeInitializeTimerEx")]
pub unsafe extern "win64" fn KeInitializeTimerEx(timer: *mut KTIMER, timer_type: u32) {
    if timer.is_null() {
        return;
    }
    let tt = if timer_type == 0 {
        TIMER_TYPE::NotificationTimer
    } else {
        TIMER_TYPE::SynchronizationTimer
    };
    ke_initialize_timer_ex(&mut *timer, tt);
}

/// KeSetTimer - устанавливает таймер
#[unsafe(export_name = "KeSetTimer")]
pub unsafe extern "win64" fn KeSetTimer(
    timer: *mut KTIMER,
    due_time: i64,
    dpc: *mut KDPC,
) -> BOOLEAN {
    if timer.is_null() {
        return FALSE;
    }
    let dpc_opt = if dpc.is_null() { None } else { Some(&mut *dpc) };
    if ke_set_timer_ex(&mut *timer, due_time, 0, dpc_opt) {
        TRUE
    } else {
        FALSE
    }
}

/// KeSetTimerEx - устанавливает таймер с периодом
#[unsafe(export_name = "KeSetTimerEx")]
pub unsafe extern "win64" fn KeSetTimerEx(
    timer: *mut KTIMER,
    due_time: i64,
    period: i32,
    dpc: *mut KDPC,
) -> BOOLEAN {
    if timer.is_null() {
        return FALSE;
    }
    let dpc_opt = if dpc.is_null() { None } else { Some(&mut *dpc) };
    if ke_set_timer_ex(&mut *timer, due_time, period, dpc_opt) {
        TRUE
    } else {
        FALSE
    }
}

/// KeCancelTimer - отменяет таймер
#[unsafe(export_name = "KeCancelTimer")]
pub unsafe extern "win64" fn KeCancelTimer(timer: *mut KTIMER) -> BOOLEAN {
    if timer.is_null() {
        return FALSE;
    }
    if ke_cancel_timer(&mut *timer) {
        TRUE
    } else {
        FALSE
    }
}

// =============================================================================
// Event (KEVENT)
// =============================================================================

use ntoskrnl::ke::event::{KEVENT, ke_initialize_event, ke_set_event, EVENT_TYPE};
use ntoskrnl::nt::LONG;

/// KeInitializeEvent - инициализирует объект события
///
/// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/nf-wdm-keinitializeevent
#[unsafe(export_name = "KeInitializeEvent")]
pub unsafe extern "win64" fn KeInitializeEvent(
    event: *mut KEVENT,
    event_type: u32,
    initial_state: BOOLEAN,
) {
    if event.is_null() {
        return;
    }
    
    let event_type_enum = if event_type == 0 {
        EVENT_TYPE::NotificationEvent
    } else {
        EVENT_TYPE::SynchronizationEvent
    };
    
    ke_initialize_event(&mut *event, event_type_enum, initial_state != 0);
}

/// KeSetEvent - устанавливает событие в signaled состояние
///
/// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/nf-wdm-kesetevent
#[unsafe(export_name = "KeSetEvent")]
pub unsafe extern "win64" fn KeSetEvent(
    event: *mut KEVENT,
    increment: LONG,
    wait: BOOLEAN,
) -> LONG {
    if event.is_null() {
        return 0;
    }
    ke_set_event(&*event, increment, wait != 0)
}