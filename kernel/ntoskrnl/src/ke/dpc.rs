//! DPC - Deferred Procedure Call (NT style, x64)
//!
//! Минимальная корректная база:
//! - постановка DPC в очередь текущего CPU под DpcLock
//! - запрос DISPATCH software interrupt
//! - обработка очереди DPC в `ki_retire_dpc_list`
//! - вход через `KiDispatchInterruptHandler` (вызывается ASM stub'ом на DISPATCH_LEVEL)

#![allow(non_camel_case_types)]

use super::spinlock::ke_acquire_spin_lock_at_dpc_level;
use super::spinlock::ke_release_spin_lock_from_dpc_level;
use crate::nt::BOOLEAN;
use crate::nt::LIST_ENTRY;
use crate::nt::PVOID;
use crate::nt::UCHAR;

pub type PKDEFERRED_ROUTINE = extern "win64" fn(
    dpc: *mut KDPC,
    deferred_context: PVOID,
    system_argument1: PVOID,
    system_argument2: PVOID,
);

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KDPC_IMPORTANCE {
    LowImportance = 0,
    MediumImportance = 1,
    HighImportance = 2,
    MediumHighImportance = 3,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KDPC_TYPE {
    Normal = 0x13,
    Threaded = 0x14,
}

#[repr(C)]
pub struct KDPC {
    pub r#type: UCHAR,
    pub importance: UCHAR,
    pub number: u16,
    pub dpc_list_entry: LIST_ENTRY,
    pub deferred_routine: Option<PKDEFERRED_ROUTINE>,
    pub deferred_context: PVOID,
    pub system_argument1: PVOID,
    pub system_argument2: PVOID,
    /// non-null => queued; хранит PRCB*
    pub dpc_data: PVOID,
}

impl KDPC {
    pub const fn new() -> Self {
        Self {
            r#type: KDPC_TYPE::Normal as UCHAR,
            importance: KDPC_IMPORTANCE::MediumImportance as UCHAR,
            number: 0,
            dpc_list_entry: LIST_ENTRY::new(),
            deferred_routine: None,
            deferred_context: core::ptr::null_mut(),
            system_argument1: core::ptr::null_mut(),
            system_argument2: core::ptr::null_mut(),
            dpc_data: core::ptr::null_mut(),
        }
    }

    #[inline]
    pub fn is_queued(&self) -> bool {
        !self.dpc_data.is_null()
    }
}

impl Default for KDPC {
    fn default() -> Self {
        Self::new()
    }
}

pub fn ke_initialize_dpc(
    dpc: &mut KDPC,
    deferred_routine: PKDEFERRED_ROUTINE,
    deferred_context: PVOID,
) {
    dpc.r#type = KDPC_TYPE::Normal as UCHAR;
    dpc.importance = KDPC_IMPORTANCE::MediumImportance as UCHAR;
    dpc.number = 0;
    dpc.deferred_routine = Some(deferred_routine);
    dpc.deferred_context = deferred_context;
    dpc.system_argument1 = core::ptr::null_mut();
    dpc.system_argument2 = core::ptr::null_mut();
    dpc.dpc_data = core::ptr::null_mut();
    dpc.dpc_list_entry = LIST_ENTRY::new();
}

pub fn ke_set_importance_dpc(dpc: &mut KDPC, importance: KDPC_IMPORTANCE) {
    dpc.importance = importance as UCHAR;
}

pub fn ke_set_target_processor_dpc(dpc: &mut KDPC, processor: u8) {
    dpc.number = processor as u16;
}

pub unsafe fn ke_insert_queue_dpc(
    dpc: &mut KDPC,
    system_argument1: PVOID,
    system_argument2: PVOID,
) -> bool {
    unsafe {
        if dpc.is_queued() {
            return false;
        }

        dpc.system_argument1 = system_argument1;
        dpc.system_argument2 = system_argument2;

        let prcb = crate::arch::x86_64::pcr::get_prcb();
        if prcb.is_null() {
            return false;
        }

        ke_acquire_spin_lock_at_dpc_level(&(*prcb).dpc_lock);

        dpc.dpc_data = prcb as PVOID;
        (*prcb).dpc_queue_depth = (*prcb).dpc_queue_depth.wrapping_add(1);
        (*prcb).dpc_requested = 1;

        let list_head = &mut (*prcb).dpc_list_head as *mut LIST_ENTRY;
        if dpc.importance >= KDPC_IMPORTANCE::HighImportance as UCHAR {
            LIST_ENTRY::insert_head(list_head, &mut dpc.dpc_list_entry as *mut LIST_ENTRY);
        } else {
            LIST_ENTRY::insert_tail(list_head, &mut dpc.dpc_list_entry as *mut LIST_ENTRY);
        }

        // Запрашиваем DISPATCH SW interrupt (если еще не запрошен).
        if (*prcb).dpc_interrupt_requested == 0 {
            (*prcb).dpc_interrupt_requested = 1;
            crate::hal::swint::hal_request_software_interrupt(crate::hal::irql::DISPATCH_LEVEL);
        }

        ke_release_spin_lock_from_dpc_level(&(*prcb).dpc_lock);

        true
    }
}

pub unsafe fn ke_remove_queue_dpc(dpc: &mut KDPC) -> bool {
    unsafe {
        if !dpc.is_queued() {
            return false;
        }

        let prcb = dpc.dpc_data as *mut crate::arch::x86_64::pcr::KPRCB;
        if prcb.is_null() {
            return false;
        }

        ke_acquire_spin_lock_at_dpc_level(&(*prcb).dpc_lock);
        LIST_ENTRY::remove_entry(&mut dpc.dpc_list_entry as *mut LIST_ENTRY);
        dpc.dpc_data = core::ptr::null_mut();
        (*prcb).dpc_queue_depth = (*prcb).dpc_queue_depth.saturating_sub(1);
        ke_release_spin_lock_from_dpc_level(&(*prcb).dpc_lock);
        true
    }
}

/// Обрабатывает DPC очередь текущего CPU.
///
/// # Safety
/// - должна выполняться на DISPATCH_LEVEL
pub unsafe fn ki_retire_dpc_list(prcb: *mut crate::arch::x86_64::pcr::KPRCB) {
    unsafe {
        if prcb.is_null() {
            return;
        }

        loop {
            ke_acquire_spin_lock_at_dpc_level(&(*prcb).dpc_lock);

            let list_head = &mut (*prcb).dpc_list_head as *mut LIST_ENTRY;
            if LIST_ENTRY::is_empty(list_head) {
                (*prcb).dpc_requested = 0;
                (*prcb).dpc_interrupt_requested = 0;
                (*prcb).dpc_queue_depth = 0;
                ke_release_spin_lock_from_dpc_level(&(*prcb).dpc_lock);
                break;
            }

            let entry = (*list_head).flink;
            LIST_ENTRY::remove_entry(entry);
            (*prcb).dpc_queue_depth = (*prcb).dpc_queue_depth.saturating_sub(1);

            let dpc = crate::containing_record!(entry, KDPC, dpc_list_entry);
            let routine = (*dpc).deferred_routine;
            let ctx = (*dpc).deferred_context;
            let a1 = (*dpc).system_argument1;
            let a2 = (*dpc).system_argument2;
            (*dpc).dpc_data = core::ptr::null_mut();

            ke_release_spin_lock_from_dpc_level(&(*prcb).dpc_lock);

            if let Some(r) = routine {
                r(dpc, ctx, a1, a2);
            }
        }
    }
}

/// Входная точка software interrupt vector 0x2F (вызывается из ASM stub).
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn KiDispatchInterruptHandler() {
    unsafe {
        // Мы уже на DISPATCH_LEVEL (поднят в ASM stub через KiEnterSoftwareInterrupt).
        // Очищаем оба флага: software interrupt request и pending dispatch.
        // Без очистки PENDING_DISPATCH возникает бесконечный цикл software interrupts,
        // так как kf_lower_irql при понижении IRQL проверяет hal_is_dispatch_pending().
        crate::hal::swint::hal_clear_software_interrupt(crate::hal::irql::DISPATCH_LEVEL);
        crate::hal::init::hal_clear_dispatch();

        ki_dispatch_interrupt();
    }
}

/// Центральный обработчик dispatch уровня:
/// 1) Timer expiration (отложенная обработка истекших таймеров)
/// 2) DPC
/// 3) попытка планирования/переключения (через ke::sched)
///
/// Порядок важен: таймеры обрабатываются первыми (ReactOS pattern),
/// так как они могут добавлять DPC в очередь.
pub unsafe fn ki_dispatch_interrupt() {
    unsafe {
        let prcb = crate::arch::x86_64::pcr::get_prcb();
        if prcb.is_null() {
            return;
        }

        // Timer expiration first (ReactOS pattern)
        // Обрабатываем истекшие таймеры если установлен timer_request
        unsafe {
            if (*prcb).timer_request != 0 {
                // Сохраняем timer_hand ДО обнуления (ki_timer_expiration читает из PRCB!)
                let _timer_hand = (*prcb).timer_hand;

                // Обрабатываем истекшие таймеры (читает timer_hand из PRCB)
                let current_time = crate::ke::time::ke_query_interrupt_time();
                crate::ke::timer::ki_timer_expiration(current_time);

                // Обнуляем ПОСЛЕ обработки
                (*prcb).timer_request = 0;
                (*prcb).timer_hand = 0;
            }
        }

        // DPC second
        if (*prcb).dpc_requested != 0 || (*prcb).dpc_queue_depth != 0 {
            ki_retire_dpc_list(prcb);
        }

        // Затем — scheduler dispatch (next_thread, quantum_end, и т.п.)
        crate::ke::sched::ki_dispatch_on_current_processor();
    }
}
