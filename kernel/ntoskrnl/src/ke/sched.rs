//! Scheduler / Dispatcher core (переписано с нуля)
//!
//! На данном этапе (Этап A):
//! - фиксируем инварианты: dispatcher lock contract, dispatch path через SW interrupt
//! - обеспечиваем базовый ready queue + wait/unwait glue
//! - SMP учитываем в интерфейсах (IPI), но реализация пока минимальная

#![allow(non_camel_case_types)]

use core::ptr::NonNull;
use core::sync::atomic::AtomicU64;
use core::sync::atomic::Ordering;
use core::sync::atomic::fence;

use crate::arch::x86_64::pcr::KPRCB;
use crate::arch::x86_64::pcr::get_prcb;
use crate::ke::spinlock::KSPIN_LOCK;
use crate::ke::spinlock::ke_acquire_spin_lock_at_dpc_level;
use crate::ke::spinlock::ke_release_spin_lock_from_dpc_level;
use crate::ke::thread::KADJUST_REASON;
use crate::ke::thread::KTHREAD;
use crate::ke::thread::KTHREAD_STATE;
use crate::nt::LIST_ENTRY;
use crate::nt::PVOID;
use crate::nt::SINGLE_LIST_ENTRY;

// Используем общие функции отладки
use super::debug::{debug_raw, debug_hex, debug_dec};

// =============================================================================
// Константы
// =============================================================================

pub const THREAD_QUANTUM: i8 = 6;
pub const CLOCK_QUANTUM_DECREMENT: i8 = 3;
pub const WAIT_QUANTUM_DECREMENT: i8 = 1;

// =============================================================================
// Глобальные состояния
// =============================================================================

pub static KI_IDLE_SUMMARY: AtomicU64 = AtomicU64::new(0);
pub static KI_DISPATCHER_LOCK: KSPIN_LOCK = KSPIN_LOCK::new();

// =============================================================================
// SMP helpers (memory ordering scaffolding)
// =============================================================================

/// Установить `Prcb->NextThread` с release-упорядочиванием (для SMP/IPI).
///
/// На текущем этапе `NextThread` хранится как обычное поле (не atomic), но на x86_64
/// aligned pointer store/load атомарен. Барьер нужен, чтобы другой CPU не увидел IPI
/// "раньше" чем обновление `NextThread`.
#[inline]
pub(crate) unsafe fn ki_set_prcb_next_thread(prcb: *mut KPRCB, thread: *mut KTHREAD) {
    unsafe {
        (*prcb).next_thread = thread as PVOID;
        fence(Ordering::Release);
    }
}

/// Прочитать `Prcb->NextThread` с acquire-упорядочиванием (для SMP/IPI).
#[inline]
pub(crate) unsafe fn ki_get_prcb_next_thread(prcb: *mut KPRCB) -> *mut KTHREAD {
    unsafe {
        fence(Ordering::Acquire);
        (*prcb).next_thread as *mut KTHREAD
    }
}

#[inline]
pub fn ki_query_low_tick_count() -> u32 {
    crate::ke::time::ke_query_tick_count() as u32
}

pub unsafe fn ki_init_scheduler() {
    KI_IDLE_SUMMARY.store(0, Ordering::Release);
}

// =============================================================================
// DeferredReady (NT/ReactOS-style)
// =============================================================================

/// Вставляет поток в DeferredReady list текущего PRCB.
///
/// # Safety
/// - вызывается под `KI_DISPATCHER_LOCK`
/// - поток НЕ должен быть в ready queue (queue_list_entry пуст)
pub unsafe fn ki_insert_deferred_ready_list(thread: *mut KTHREAD) {
    unsafe {
        if thread.is_null() {
            return;
        }

        let prcb = get_prcb();
        if prcb.is_null() {
            // fallback: нет PRCB — делаем обычный ready
            ki_ready_thread(thread);
            return;
        }

        debug_assert!(
            (*thread).queue_list_entry.flink.is_null()
                && (*thread).queue_list_entry.blink.is_null(),
            "ki_insert_deferred_ready_list: thread already queued"
        );

        (*thread).set_state(KTHREAD_STATE::DeferredReady);

        // SINGLE_LIST push-front: Thread->SwapListEntry.Next = Head.Next; Head.Next = Thread
        let head = &raw mut (*prcb).deferred_ready_list_head as *mut SINGLE_LIST_ENTRY;
        (*thread).swap_list_entry.next = (*head).next;
        (*head).next = &raw mut (*thread).swap_list_entry as *mut SINGLE_LIST_ENTRY;
    }
}

/// Обрабатывает DeferredReady list и переносит потоки в обычные ready queues.
///
/// # Safety
/// - вызывается под `KI_DISPATCHER_LOCK` и на DISPATCH_LEVEL
unsafe fn ki_process_deferred_ready_list(prcb: *mut KPRCB) {
    unsafe {
        if prcb.is_null() {
            return;
        }

        debug_raw("[SCHED] Processing deferred ready list\n");

        // Забираем список целиком (как ReactOS: Head.Next -> local, Head.Next = NULL)
        let head = &raw mut (*prcb).deferred_ready_list_head as *mut SINGLE_LIST_ENTRY;
        let mut entry = (*head).next;
        (*head).next = core::ptr::null_mut();

        let mut count = 0;
        while !entry.is_null() {
            let thread = crate::containing_record!(entry, KTHREAD, swap_list_entry);
            entry = (*entry).next;
            (*thread).swap_list_entry.next = core::ptr::null_mut();

            debug_raw("[SCHED] Deferred ready thread=");
            debug_hex(thread as u64);
            debug_raw("\n");

            // Делаём поток готовым: корректировки priority/quantum выполняются тут (NT/ReactOS).
            ki_deferred_ready_thread(thread);
            count += 1;
        }

        debug_raw("[SCHED] Processed ");
        debug_dec(count);
        debug_raw(" deferred threads\n");
    }
}

/// KiDeferredReadyThread (NT/ReactOS pattern, минимальная база).
///
/// Здесь применяются AdjustBoost/AdjustUnwait (priority/quantum tweaks), после чего поток
/// добавляется в ready queues через `ki_ready_thread`.
///
/// # Safety
/// - вызывается под `KI_DISPATCHER_LOCK` и на DISPATCH_LEVEL
unsafe fn ki_deferred_ready_thread(thread: *mut KTHREAD) {
    unsafe {
        use crate::ke::globals::LOW_REALTIME_PRIORITY;
        use crate::ke::spinlock::ki_acquire_thread_lock;
        use crate::ke::spinlock::ki_release_thread_lock;
        use crate::nt::ntstatus::STATUS_KERNEL_APC;

        if thread.is_null() {
            return;
        }

        // DEBUG: отслеживаем изменения приоритета
        let old_prio = (*thread).priority;
        let old_base = (*thread).base_priority;

        debug_assert!(
            (*thread).get_state() == KTHREAD_STATE::DeferredReady,
            "ki_deferred_ready_thread: expected DeferredReady state"
        );

        let reason = (*thread).adjust_reason;
        let inc = (*thread).adjust_increment;

        if reason != KADJUST_REASON::AdjustNone {
            // ReactOS/NT: корректировки под thread lock
            unsafe { ki_acquire_thread_lock(thread) };

            match reason {
                KADJUST_REASON::AdjustBoost => {
                    // Boost: только для non-RT и если boost разрешён.
                    if (*thread).disable_boost == 0
                        && (*thread).priority < (LOW_REALTIME_PRIORITY as i8 - 3)
                        && inc > 0
                        && (*thread).priority <= inc
                    {
                        let mut new_prio = inc.saturating_add(1);
                        let max = LOW_REALTIME_PRIORITY as i8 - 3;
                        if new_prio > max {
                            new_prio = max;
                        }

                        // PriorityDecrement на величину boost (для последующего decay).
                        let delta = new_prio.saturating_sub((*thread).priority).max(0);
                        (*thread).priority_decrement =
                            (*thread).priority_decrement.saturating_add(delta);
                        (*thread).priority = new_prio;
                    }

                    // Quantum tweak: минимум 4, затем -1 (как в ReactOS)
                    if (*thread).quantum < 4 {
                        (*thread).quantum = 4;
                    }
                    (*thread).quantum = (*thread).quantum.saturating_sub(1);
                },
                KADJUST_REASON::AdjustUnwait => {
                    if (*thread).priority < LOW_REALTIME_PRIORITY as i8 {
                        // "time critical" — просто reset quantum
                        if (*thread).base_priority >= (LOW_REALTIME_PRIORITY as i8 - 2) {
                            (*thread).quantum = (*thread).quantum_reset;
                        } else {
                            // Если ранее не было decrement и есть increment — reset quantum
                            if (*thread).priority_decrement == 0 && inc > 0 {
                                (*thread).quantum = (*thread).quantum_reset;
                            }

                            // Wait code в ROS делает special-case для STATUS_KERNEL_APC
                            if (*thread).wait_status.load(Ordering::Acquire) != STATUS_KERNEL_APC {
                                (*thread).quantum = (*thread).quantum.saturating_sub(1);
                                if (*thread).quantum <= 0 {
                                    (*thread).quantum = (*thread).quantum_reset;
                                    (*thread).priority =
                                        crate::ke::priority::ki_compute_new_priority(thread, 1);
                                }
                            }

                            // Unwait boost (ReactOS/NT-паттерн):
                            // new = BasePriority + AdjustIncrement (+ PsPrioritySeparation для foreground процессов).
                            if (*thread).priority_decrement == 0
                                && (*thread).disable_boost == 0
                                && inc > 0
                            {
                                let base_boost = (*thread).base_priority.saturating_add(inc);

                                // Foreground separation: если процесс foreground — добавляем boost.
                                let mut fg_boost: i8 = 0;
                                let proc = (*thread).process as *mut crate::ps::process::KPROCESS;
                                if !proc.is_null() && unsafe { (*proc).foreground } != 0 {
                                    fg_boost = unsafe { (*proc).foreground_boost };
                                    if fg_boost == 0 {
                                        fg_boost = crate::ps::process::PS_PRIORITY_SEPARATION;
                                    }
                                }

                                let mut new_prio = base_boost.saturating_add(fg_boost);
                                if new_prio >= LOW_REALTIME_PRIORITY as i8 {
                                    new_prio = LOW_REALTIME_PRIORITY as i8 - 1;
                                }

                                if new_prio > (*thread).priority {
                                    // Если foreground boost поднял выше base+inc — ставим decrement для последующего decay.
                                    if fg_boost > 0 && new_prio > base_boost {
                                        (*thread).priority_decrement =
                                            new_prio.saturating_sub(base_boost);
                                    }
                                    (*thread).priority = new_prio;
                                }
                            }
                        }
                    } else {
                        // RT: просто reset quantum
                        (*thread).quantum = (*thread).quantum_reset;
                    }
                },
                _ => {},
            }

            // Сбрасываем корректировки (как минимум reason; increment тоже очищаем чтобы не "залипало")
            (*thread).adjust_reason = KADJUST_REASON::AdjustNone;
            (*thread).adjust_increment = 0;

            unsafe { ki_release_thread_lock(thread) };
        }

        // DEBUG: проверяем изменился ли приоритет
        let new_prio = (*thread).priority;
        let new_base = (*thread).base_priority;
        if new_prio != old_prio || new_base != old_base {
            debug_raw("[SCHED] KiDeferredReadyThread: thread=");
            debug_hex(thread as u64);
            debug_raw(" prio ");
            debug_dec(old_prio as u64);
            debug_raw("/");
            debug_dec(old_base as u64);
            debug_raw(" -> ");
            debug_dec(new_prio as u64);
            debug_raw("/");
            debug_dec(new_base as u64);
            debug_raw("\n");
        }

        ki_ready_thread(thread);
    }
}

// =============================================================================
// Ready queue helpers
// =============================================================================

#[inline]
unsafe fn set_prcb_ready_summary(prio: u32, prcb: *mut KPRCB) {
    unsafe {
        (*prcb).ready_summary |= 1 << prio;
    }
}

#[inline]
unsafe fn clear_prcb_ready_summary(prio: u32, prcb: *mut KPRCB) {
    unsafe {
        (*prcb).ready_summary &= !(1 << prio);
    }
}

/// Ищет ready thread на текущем CPU.
///
/// # Safety
/// - вызывается под `KI_DISPATCHER_LOCK`
pub unsafe fn ki_find_ready_thread(processor: u32, low_priority: i32) -> Option<NonNull<KTHREAD>> {
    unsafe {
        let prcb = crate::ke::init::ke_get_processor_prcb(processor as u8);
        if prcb.is_null() {
            return None;
        }

        let mask = if low_priority <= 0 {
            !0u32
        } else {
            !((1u32 << (low_priority as u32)) - 1)
        };
        let mut set = mask & (*prcb).ready_summary;
        
        // DEBUG: отключено - слишком много шума
        // debug_raw("[SCHED] KiFindReadyThread: proc=");
        // debug_hex(processor as u64);
        // debug_raw(" ready_summary=");
        // debug_hex((*prcb).ready_summary as u64);
        // debug_raw(" set=");
        // debug_hex(set as u64);
        // debug_raw("\n");
        
        if set == 0 {
            // DEBUG: отключено - слишком много шума
            // debug_raw("[SCHED] No ready threads\n");
            return None;
        }

        let lists = &mut (*prcb).dispatcher_ready_list_head;
        while set != 0 {
            let prio = 31 - set.leading_zeros() as usize;
            let head = &mut lists[prio] as *mut LIST_ENTRY;
            if !LIST_ENTRY::is_empty(head) {
                let entry = (*head).flink;
                // В NT ready queue использует отдельный list entry (Thread->QueueListEntry),
                // а WaitListEntry зарезервирован под ожидания/dispatcher.
                let thread = crate::containing_record!(entry, KTHREAD, queue_list_entry);
                LIST_ENTRY::remove_entry(&mut (*thread).queue_list_entry as *mut LIST_ENTRY);
                (*thread).queue_list_entry = LIST_ENTRY::new();
                if LIST_ENTRY::is_empty(head) {
                    clear_prcb_ready_summary(prio as u32, prcb);
                }
                (*thread).next_processor = processor as u8;
                return NonNull::new(thread);
            }
            set &= !(1 << prio);
        }

        None
    }
}

/// Выбор следующего потока: ready или idle.
///
/// KiSelectNextThread
///
/// # Safety
/// - вызывается под `KI_DISPATCHER_LOCK`
pub unsafe fn ki_select_next_thread(processor: u32) -> *mut KTHREAD {
    unsafe {
        debug_raw("[SCHED] KiSelectNextThread\n");

        if let Some(t) = ki_find_ready_thread(processor, 0) {
            debug_raw("[SCHED] Found ready thread=");
            debug_hex(t.as_ptr() as u64);
            debug_raw("\n");
            return t.as_ptr();
        }

        let prcb = crate::ke::init::ke_get_processor_prcb(processor as u8);
        if prcb.is_null() {
            debug_raw("[SCHED] No PRCB for processor\n");
            return core::ptr::null_mut();
        }

        // Отметим CPU как idle (бит для будущего SMP/idle selection)
        KI_IDLE_SUMMARY.fetch_or(1u64 << processor, Ordering::AcqRel);
        let idle = (*prcb).idle_thread as *mut KTHREAD;

        debug_raw("[SCHED] Selecting IDLE thread\n");
        idle
    }
}

/// Делает поток готовым к выполнению.
///
/// KiReadyThread
///
/// # Safety
/// - вызывается под `KI_DISPATCHER_LOCK`
pub unsafe fn ki_ready_thread(thread: *mut KTHREAD) {
    unsafe {
        if thread.is_null() {
            return;
        }

        let prio = (*thread).priority as usize;
        let base_prio = (*thread).base_priority as usize;

        debug_raw("[SCHED] KiReadyThread: thread=");
        debug_hex(thread as u64);
        debug_raw(" prio=");
        debug_hex(prio as u64);
        debug_raw(" base=");
        debug_hex(base_prio as u64);
        debug_raw("\n");

        // базовая метка времени ready
        (*thread).wait_time = ki_query_low_tick_count();

        let prcb = get_prcb();
        if prcb.is_null() {
            debug_raw("[SCHED] KiReadyThread: no PRCB\n");
            return;
        }


        // Поток не должен быть уже вставлен в ready queue.
        // (иначе будет corruption списка)
        debug_assert!(
            (*thread).queue_list_entry.flink.is_null()
                && (*thread).queue_list_entry.blink.is_null(),
            "ki_ready_thread: thread already queued (queue_list_entry not null)"
        );

        let state = (*thread).get_state();

        // Если текущий поток idle, можно назначить next_thread напрямую.
        // ВАЖНО: NextThread может уже быть выставлен (например, мы подряд создаём несколько
        // потоков до первого dispatch). В этом случае нельзя "терять" предыдущий Standby:
        // оставляем более приоритетный как NextThread, а второй кладём в ready queue.
        let cur = (*prcb).current_thread as *mut KTHREAD;
        if !cur.is_null() && cur as PVOID == (*prcb).idle_thread {
            debug_raw("[SCHED] Current is IDLE, checking next_thread\n");
            let existing_next = (*prcb).next_thread as *mut KTHREAD;
            if existing_next.is_null() {
                debug_raw("[SCHED] Setting as next_thread (Standby)\n");
                (*thread).set_state(KTHREAD_STATE::Standby);
                ki_set_prcb_next_thread(prcb, thread);
                return;
            }

            // NextThread уже занят: выбираем более приоритетный для Standby.
            if (*thread).priority > (*existing_next).priority {
                // Сбрасываем старый NextThread обратно в ready queue.
                let old_next_prio = (*existing_next).priority as usize;
                debug_assert!(
                    (*existing_next).queue_list_entry.flink.is_null()
                        && (*existing_next).queue_list_entry.blink.is_null(),
                    "ki_ready_thread: existing NextThread already queued"
                );
                (*existing_next).preempted = 0;
                (*existing_next).set_state(KTHREAD_STATE::Ready);
                let head =
                    &mut (*prcb).dispatcher_ready_list_head[old_next_prio] as *mut LIST_ENTRY;
                LIST_ENTRY::insert_tail(
                    head,
                    &mut (*existing_next).queue_list_entry as *mut LIST_ENTRY,
                );
                set_prcb_ready_summary(old_next_prio as u32, prcb);

                // Новый становится NextThread.
                (*thread).set_state(KTHREAD_STATE::Standby);
                ki_set_prcb_next_thread(prcb, thread);
            } else {
                // Новый поток просто ready.
                (*thread).set_state(KTHREAD_STATE::Ready);
                let head = &mut (*prcb).dispatcher_ready_list_head[prio] as *mut LIST_ENTRY;
                LIST_ENTRY::insert_tail(head, &mut (*thread).queue_list_entry as *mut LIST_ENTRY);
                set_prcb_ready_summary(prio as u32, prcb);
            }
            return;
        }

        // Preemption (минимально): если новый поток выше приоритета текущего —
        // ставим NextThread и просим DISPATCH.
        if !cur.is_null() && (*thread).priority > (*cur).priority {
            let existing_next = (*prcb).next_thread as *mut KTHREAD;
            if existing_next.is_null() {
                (*thread).set_state(KTHREAD_STATE::Standby);
                ki_set_prcb_next_thread(prcb, thread);
                // пометим текущий как вытесняемый (для политики requeue)
                (*cur).preempted = 1;
                ki_request_dispatch_interrupt((*prcb).number as u32);
                return;
            }

            // Уже есть NextThread — если новый выше, заменяем, а старый NextThread кладём в ready queue.
            if (*thread).priority > (*existing_next).priority {
                let old_next_prio = (*existing_next).priority as usize;
                debug_assert!(
                    (*existing_next).queue_list_entry.flink.is_null()
                        && (*existing_next).queue_list_entry.blink.is_null(),
                    "ki_ready_thread: existing NextThread already queued"
                );
                (*existing_next).preempted = 0;
                (*existing_next).set_state(KTHREAD_STATE::Ready);
                let head =
                    &mut (*prcb).dispatcher_ready_list_head[old_next_prio] as *mut LIST_ENTRY;
                LIST_ENTRY::insert_tail(
                    head,
                    &mut (*existing_next).queue_list_entry as *mut LIST_ENTRY,
                );
                set_prcb_ready_summary(old_next_prio as u32, prcb);

                (*thread).set_state(KTHREAD_STATE::Standby);
                ki_set_prcb_next_thread(prcb, thread);
                (*cur).preempted = 1;
                ki_request_dispatch_interrupt((*prcb).number as u32);
                return;
            }
        }

        // Иначе кладём в ready queue текущего CPU.
        (*thread).set_state(KTHREAD_STATE::Ready);

        let head = &mut (*prcb).dispatcher_ready_list_head[prio] as *mut LIST_ENTRY;
        if (*thread).preempted != 0 {
            (*thread).preempted = 0;
            LIST_ENTRY::insert_head(head, &mut (*thread).queue_list_entry as *mut LIST_ENTRY);
        } else {
            LIST_ENTRY::insert_tail(head, &mut (*thread).queue_list_entry as *mut LIST_ENTRY);
        }
        set_prcb_ready_summary(prio as u32, prcb);
    }
}

/// Запросить dispatch interrupt (локально/удаленно).
pub fn ki_request_dispatch_interrupt(processor: u32) {
    unsafe {
        let cur = get_prcb();
        if !cur.is_null() && (*cur).number as u32 == processor {
            crate::hal::swint::hal_request_software_interrupt(crate::hal::irql::DISPATCH_LEVEL);
        } else {
            crate::ke::ipi::ki_ipi_send(1u64 << processor, crate::ke::ipi::IPI_DPC);
        }
    }
}

/// Центральная часть dispatch на текущем CPU (вызывается из DPC interrupt handler).
///
/// KiDispatchOnCurrentProcessor
///
/// Счетчик вложенности для отладки reentrancy
static DISPATCH_DEPTH: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// # Safety
/// - вызывается на DISPATCH_LEVEL
pub unsafe fn ki_dispatch_on_current_processor() {
    unsafe {
        use core::sync::atomic::Ordering;
        
        let depth = DISPATCH_DEPTH.fetch_add(1, Ordering::SeqCst);
        if depth > 0 {
            DISPATCH_DEPTH.fetch_sub(1, Ordering::SeqCst);
            return;  // Предотвращаем reentrancy
        }
        
        debug_raw("[SCHED] === KiDispatchOnCurrentProcessor START ===\n");
        
        let prcb = get_prcb();
        if prcb.is_null() {
            debug_raw("[SCHED] No PRCB\n");
            DISPATCH_DEPTH.fetch_sub(1, Ordering::SeqCst);
            return;
        }

        // Quantum end (scheduler part живет в ke/trap.rs; мы только вызываем)
        if (*prcb).quantum_end != 0 {
            debug_raw("[SCHED] Quantum end detected\n");
            ke_acquire_spin_lock_at_dpc_level(&KI_DISPATCHER_LOCK);
            crate::ke::trap::ki_quantum_end();
            ke_release_spin_lock_from_dpc_level(&KI_DISPATCHER_LOCK);
        }

        // Дальнейшие манипуляции со state/ready queue делаем под dispatcher lock,
        // но перед реальным свитчем lock должен быть отпущен.
        ke_acquire_spin_lock_at_dpc_level(&KI_DISPATCHER_LOCK);

        // Обработаем DeferredReady list (если есть).
        if (*prcb).deferred_ready_list_head.next != core::ptr::null_mut() {
            ki_process_deferred_ready_list(prcb);
        }

        let mut next = ki_get_prcb_next_thread(prcb);

        debug_raw("[SCHED] next_thread=");
        debug_hex(next as u64);
        debug_raw(" ready_summary=");
        debug_hex((*prcb).ready_summary as u64);
        debug_raw("\n");

        // Если next_thread не установлен, пытаемся выбрать из ready queue
        if next.is_null() && (*prcb).ready_summary != 0 {
            debug_raw("[SCHED] Selecting from ready queue\n");
            next = ki_select_next_thread((*prcb).number as u32);
            if !next.is_null() {
                debug_raw("[SCHED] Selected thread=");
                debug_hex(next as u64);
                debug_raw("\n");
                ki_set_prcb_next_thread(prcb, next);
            }
        }

        if next.is_null() {
            debug_raw("[SCHED] No thread to dispatch\n");
            ke_release_spin_lock_from_dpc_level(&KI_DISPATCHER_LOCK);
            DISPATCH_DEPTH.fetch_sub(1, core::sync::atomic::Ordering::SeqCst);
            return;
        }

        let old = (*prcb).current_thread as *mut KTHREAD;
        if old.is_null() || old == next {
            debug_raw("[SCHED] Same thread or no old thread\n");
            (*prcb).next_thread = core::ptr::null_mut();
            ke_release_spin_lock_from_dpc_level(&KI_DISPATCHER_LOCK);
            DISPATCH_DEPTH.fetch_sub(1, core::sync::atomic::Ordering::SeqCst);
            return;
        }

        debug_raw("[SCHED] Switching: old=");
        debug_hex(old as u64);
        debug_raw(" -> next=");
        debug_hex(next as u64);
        debug_raw("\n");

        // Помечаем next как Standby (NT смысл NextThread).
        // Пропускаем если уже в Standby (например, установлен ki_quantum_end)
        let next_state = (*next).get_state();
        if next_state != KTHREAD_STATE::Standby {
            (*next).set_state(KTHREAD_STATE::Standby);
        }

        // Если переключаемся с idle — снимаем idle bit до свитча.
        if old as PVOID == (*prcb).idle_thread {
            KI_IDLE_SUMMARY.fetch_and(!(1u64 << (*prcb).number), Ordering::AcqRel);
        } else {
            // Если старый поток всё ещё Running и не был ре-очереден (например, при preemption),
            // переводим его в Ready и ставим в ready queue (в хвост).
            let st = (*old).get_state();
            if st == KTHREAD_STATE::Running {
                if (*old).queue_list_entry.flink.is_null() {
                    ki_ready_thread(old);
                } else {
                    (*old).set_state(KTHREAD_STATE::Ready);
                }
            }
        }

        // ВАЖНО: освобождаем lock перед свитчем.
        ke_release_spin_lock_from_dpc_level(&KI_DISPATCHER_LOCK);

        crate::arch::x86_64::context::ki_swap_context(crate::hal::irql::DISPATCH_LEVEL, old);
        
        DISPATCH_DEPTH.fetch_sub(1, core::sync::atomic::Ordering::SeqCst);
    }
}

// =============================================================================
// Wait/unwait glue (используется объектами синхронизации)
// =============================================================================

/// KiUnwaitThread
pub unsafe fn ki_unwait_thread(thread: *mut KTHREAD, wait_status: i32, increment: i8) {
    unsafe {
        if thread.is_null() {
            return;
        }

        // Удаляем wait blocks из списков объектов (реальная семантика будет уточняться на Этапе A3)
        ki_unlink_thread(thread);

        (*thread).wait_status.store(wait_status, Ordering::Release);

        if ((*thread).priority as i32) < crate::ke::globals::LOW_REALTIME_PRIORITY {
            (*thread).adjust_reason = KADJUST_REASON::AdjustUnwait;
            (*thread).adjust_increment = increment;
        }

        // NT/ReactOS: unwait обычно идёт через DeferredReady (обработка в dispatch path).
        ki_insert_deferred_ready_list(thread);
    }
}

fn ki_unlink_thread(thread: *mut KTHREAD) {
    if thread.is_null() {
        return;
    }

    unsafe {
        let wait_block_list = (*thread).wait_block_list;
        if !wait_block_list.is_null() {
            let mut wb = wait_block_list;
            loop {
                // Безопасно удаляем только если блок реально в списке.
                if !(*wb).wait_list_entry.flink.is_null() {
                    LIST_ENTRY::remove_entry(&mut (*wb).wait_list_entry as *mut LIST_ENTRY);
                    (*wb).wait_list_entry = LIST_ENTRY::new();
                }
                let next = (*wb).next_wait_block;
                if next.is_null() || next == wait_block_list {
                    break;
                }
                wb = next;
            }
        }
        (*thread).wait_block_list = core::ptr::null_mut();

        // В NT KiUnlinkThread также снимает thread timer, если он был вставлен.
        // Это нужно, чтобы timeout timer не срабатывал после того как ожидание уже завершилось.
        crate::ke::timer::ke_cancel_timer(&mut (*thread).timer);
    }
}

pub unsafe fn ki_wait_test(object: *mut crate::ke::event::DISPATCHER_HEADER, increment: i8) {
    unsafe {
        use crate::ke::thread::KWAIT_BLOCK;
        use crate::nt::ntstatus::STATUS_KERNEL_APC;
        use crate::nt::ntstatus::STATUS_TIMEOUT;

        if object.is_null() {
            return;
        }

        debug_raw("[SCHED] KiWaitTest: object=");
        debug_hex(object as u64);
        debug_raw(" signal=");
        debug_hex((*object).signal_state.load(Ordering::Acquire) as u64);
        debug_raw("\n");

        let wait_list = &mut (*object).wait_list_head as *mut LIST_ENTRY;

        let mut woken_count = 0;
        while (*object).signal_state.load(Ordering::Acquire) > 0 {
            // Защита от неинициализированного списка (flink == null)
            if (*wait_list).flink.is_null() {
                break;
            }
            if LIST_ENTRY::is_empty(wait_list) {
                break;
            }

            let entry = (*wait_list).flink;
            // Дополнительная проверка на случай повреждённого списка
            if entry.is_null() || entry == wait_list {
                break;
            }
            let wb = crate::containing_record!(entry, KWAIT_BLOCK, wait_list_entry);
            let thread = (*wb).thread;
            if thread.is_null() {
                LIST_ENTRY::remove_entry(entry);
                continue;
            }

            let wait_type = (*wb).wait_type;
            let key = (*wb).wait_key as i32;

            // WaitAny: удовлетворяем объект и пробуждаем.
            if wait_type == 0 {
                debug_raw("[SCHED] Waking thread=");
                debug_hex(thread as u64);
                debug_raw(" WaitAny\n");
                ki_wait_satisfy_any(object);
                ki_unwait_thread(thread, key, increment);
                woken_count += 1;
                continue;
            }

            // Для WaitAll (как в ReactOS/NT5): будим поток с STATUS_KERNEL_APC,
            // чтобы он заново проверил все объекты и выполнил "satisfy" атомарно
            // в KeWaitForMultipleObjects под dispatcher lock.
            //
            // Исключение: таймерный wait block у нас создаётся как WAIT_ANY, так что сюда
            // он не попадет. Если попадёт (на будущее) — пропускаем timeout напрямую.
            if key == STATUS_TIMEOUT {
                ki_unwait_thread(thread, key, increment);
            } else {
                debug_raw("[SCHED] Waking thread=");
                debug_hex(thread as u64);
                debug_raw(" WaitAll/APC\n");
                ki_unwait_thread(thread, STATUS_KERNEL_APC, 0);
                woken_count += 1;
            }
        }

        if woken_count > 0 {
            debug_raw("[SCHED] KiWaitTest woke ");
            debug_dec(woken_count);
            debug_raw(" threads\n");
        }
    }
}

fn ki_wait_satisfy_any(object: *mut crate::ke::event::DISPATCHER_HEADER) {
    use crate::ke::event::EVENT_SYNCHRONIZATION_OBJECT;
    use crate::ke::event::SEMAPHORE_OBJECT;

    if object.is_null() {
        return;
    }
    unsafe {
        let t = (*object).r#type;
        if t == EVENT_SYNCHRONIZATION_OBJECT || t == SEMAPHORE_OBJECT {
            (*object).signal_state.fetch_sub(1, Ordering::AcqRel);
        }
    }
}

// =============================================================================
// Публичные API (NT-style wrappers)
// =============================================================================

#[unsafe(no_mangle)]
pub unsafe extern "win64" fn KeReadyThread(thread: *mut KTHREAD) {
    unsafe {
        ke_ready_thread(thread);
    }
}

/// Внутренний wrapper (используется по проекту).
pub unsafe fn ke_ready_thread(thread: *mut KTHREAD) {
    unsafe {
        if thread.is_null() {
            return;
        }
        ke_acquire_spin_lock_at_dpc_level(&KI_DISPATCHER_LOCK);
        ki_ready_thread(thread);
        ke_release_spin_lock_from_dpc_level(&KI_DISPATCHER_LOCK);
    }
}

/// KiSwapThread - используется wait-кодом: выбирает следующий поток и делает свитч.
///
/// # Safety
/// - вызывается на DISPATCH_LEVEL с захваченным `KI_DISPATCHER_LOCK`
/// - освобождает `KI_DISPATCHER_LOCK` внутри
pub unsafe fn ki_swap_thread() -> i32 {
    unsafe {
        use crate::arch::x86_64::context::ki_swap_context;
        use crate::hal::irql::kf_lower_irql;

        debug_raw("[SCHED] KiSwapThread START\n");

        let prcb = get_prcb();

        if prcb.is_null() {
            debug_raw("[SCHED] KiSwapThread: no PRCB\n");
            ke_release_spin_lock_from_dpc_level(&KI_DISPATCHER_LOCK);
            return 0;
        }

        let old = (*prcb).current_thread as *mut KTHREAD;
        if old.is_null() {
            debug_raw("[SCHED] KiSwapThread: no current thread\n");
            ke_release_spin_lock_from_dpc_level(&KI_DISPATCHER_LOCK);
            return 0;
        }

        let wait_irql = (*old).wait_irql;

        // Выбор нового потока под dispatcher lock
        let new = if !(*prcb).next_thread.is_null() {
            (*prcb).next_thread as *mut KTHREAD
        } else {
            ki_select_next_thread((*prcb).number as u32)
        };

        debug_raw("[SCHED] KiSwapThread: old=");
        debug_hex(old as u64);
        debug_raw(" new=");
        debug_hex(new as u64);
        debug_raw("\n");

        if new.is_null() || new == old {
            debug_raw("[SCHED] KiSwapThread: no switch needed\n");
            ke_release_spin_lock_from_dpc_level(&KI_DISPATCHER_LOCK);
            kf_lower_irql(wait_irql);
            return (*old).wait_status.load(Ordering::Acquire);
        }


        // NextThread должен быть выставлен для KiSwapContext.
        (*new).set_state(KTHREAD_STATE::Standby);
        ki_set_prcb_next_thread(prcb, new);

        // Освобождаем dispatcher lock перед свитчем (как требует NT-паттерн выхода из dispatcher)
        ke_release_spin_lock_from_dpc_level(&KI_DISPATCHER_LOCK);

        debug_raw("[SCHED] KiSwapThread: calling ki_swap_context\n");
        // Реальный свитч. Возврат произойдёт позже, когда old будет снова запущен.
        let apc_pending = ki_swap_context(wait_irql, old);

        debug_raw("[SCHED] KiSwapThread: returned from ki_swap_context, apc_pending=");
        debug_hex(apc_pending as u64);
        debug_raw("\n");

        if apc_pending {
            debug_raw("[SCHED] KiSwapThread: delivering APC\n");
            crate::ke::apc::ki_deliver_apc(
                crate::ke::apc::KERNEL_MODE,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
            );
        }
        kf_lower_irql(wait_irql);

        let status = (*old).wait_status.load(Ordering::Acquire);

        debug_raw("[SCHED] KiSwapThread: returning status=");
        debug_hex(status as u64);
        debug_raw("\n");

        status
    }
}
