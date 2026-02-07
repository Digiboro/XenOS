//! Priority management (ReactOS compatible)
//!
//! Реализация по образцу ReactOS ntoskrnl/ke/thrdobj.c:
//! - KeSetPriorityThread - публичная функция с захватом locks
//! - KiSetPriorityThread - внутренняя функция без locks
//!
//! Источники:
//! - ReactOS: ke/thrdobj.c (KeSetPriorityThread, KiSetPriorityThread)
//! - NT6.1: ke/thrdobj.c

#![allow(non_camel_case_types)]

use crate::arch::x86_64::pcr::get_prcb;
use crate::hal::irql::DISPATCH_LEVEL;
use crate::hal::irql::kf_lower_irql;
use crate::hal::irql::kf_raise_irql;
use crate::ke::thread::priority::HIGH_PRIORITY;
use crate::ke::thread::priority::LOW_PRIORITY;
use crate::ke::thread::priority::LOW_REALTIME_PRIORITY;
use crate::ke::sched::KI_DISPATCHER_LOCK;
use crate::ke::sched::ki_find_ready_thread;
use crate::ke::sched::ki_insert_deferred_ready_list;
use crate::ke::sched::ki_request_dispatch_interrupt;
use crate::ke::sched::ki_set_prcb_next_thread;
use crate::ke::spinlock::ke_acquire_spin_lock_at_dpc_level;
use crate::ke::spinlock::ke_release_spin_lock_from_dpc_level;
use crate::ke::spinlock::ki_acquire_prcb_lock_at_dpc_level;
use crate::ke::spinlock::ki_acquire_thread_lock;
use crate::ke::spinlock::ki_release_prcb_lock_from_dpc_level;
use crate::ke::spinlock::ki_release_thread_lock;
use crate::ke::thread::KTHREAD;
use crate::ke::thread::KTHREAD_STATE;
use crate::nt::LIST_ENTRY;

// =============================================================================
// KiComputeNewPriority
// =============================================================================

/// KiComputeNewPriority - вычисляет новый приоритет с учетом decay
///
/// Возвращает min(current - adjustment, base) с учетом decrement.
///
/// Источник: ReactOS ke/thrdobj.c
pub unsafe fn ki_compute_new_priority(thread: *mut KTHREAD, adjustment: i32) -> i8 {
    unsafe {
        if thread.is_null() {
            return 8;
        }

        let p = (*thread).priority as i32;
        let base = (*thread).base_priority as i32;

        // Real-time потоки не decay'ятся
        if p >= LOW_REALTIME_PRIORITY as i32 {
            return p as i8;
        }

        let dec = (*thread).priority_decrement as i32;
        if dec > 0 {
            let new_dec = (dec - adjustment).max(0);
            (*thread).priority_decrement = new_dec as i8;
            return (p - adjustment).max(base) as i8;
        }

        if p > base {
            return (p - adjustment).max(base) as i8;
        }
        base as i8
    }
}

// =============================================================================
// KiSetPriorityThread (внутренняя функция)
// =============================================================================

/// KiSetPriorityThread - внутренняя функция установки приоритета
///
/// ВАЖНО: Вызывается БЕЗ dispatcher lock! Сама захватывает PRCB lock при необходимости.
/// Предполагается что thread lock уже захвачен вызывающим кодом.
///
/// Обрабатывает разные состояния потока:
/// - Ready: убирает из очереди, обновляет приоритет, вставляет в deferred ready list
/// - Standby: обновляет приоритет, ищет preemption если понизился
/// - Running: обновляет приоритет, ищет preemption и посылает IPI если нужно
/// - DeferredReady: просто обновляет приоритет (будет обработан позже)
/// - Другие: просто обновляет приоритет
///
/// Источник: ReactOS ke/thrdobj.c:KiSetPriorityThread
pub unsafe fn ki_set_priority_thread(thread: *mut KTHREAD, priority: i8) {
    unsafe {
        debug_assert!(priority >= LOW_PRIORITY && priority <= HIGH_PRIORITY);

        if thread.is_null() {
            return;
        }

        // Проверяем что приоритет изменился
        if (*thread).priority == priority {
            return;
        }

        // Цикл обработки состояний (может потребоваться повтор при race condition)
        loop {
            let state = (*thread).get_state();

            match state {
                KTHREAD_STATE::Ready => {
                    // Получаем PRCB для потока и захватываем его lock
                    let processor = (*thread).next_processor;
                    let prcb = crate::ke::init::ke_get_processor_prcb(processor);

                    if prcb.is_null() {
                        // Нет PRCB - просто обновляем приоритет
                        (*thread).priority = priority;
                        break;
                    }

                    ki_acquire_prcb_lock_at_dpc_level(prcb);

                    // Проверяем что поток все еще Ready и на этом CPU
                    if (*thread).get_state() == KTHREAD_STATE::Ready
                        && (*thread).next_processor == (*prcb).number as u8
                    {
                        let old_prio = (*thread).priority as usize;

                        // Убираем из текущей очереди
                        if !(*thread).queue_list_entry.flink.is_null() {
                            if LIST_ENTRY::remove_entry(
                                &mut (*thread).queue_list_entry as *mut LIST_ENTRY,
                            ) {
                                // Обновляем ready summary если очередь опустела
                                (*prcb).ready_summary &= !(1u32 << old_prio);
                            }
                            (*thread).queue_list_entry = LIST_ENTRY::new();
                        }

                        // Обновляем приоритет
                        (*thread).priority = priority;

                        // Вставляем в deferred ready list
                        ki_insert_deferred_ready_list(thread);

                        ki_release_prcb_lock_from_dpc_level(prcb);
                    } else {
                        // Состояние изменилось - освобождаем lock и пробуем снова
                        ki_release_prcb_lock_from_dpc_level(prcb);
                        continue;
                    }
                }

                KTHREAD_STATE::Standby => {
                    // Получаем PRCB для потока и захватываем его lock
                    let processor = (*thread).next_processor;
                    let prcb = crate::ke::init::ke_get_processor_prcb(processor);

                    if prcb.is_null() {
                        (*thread).priority = priority;
                        break;
                    }

                    ki_acquire_prcb_lock_at_dpc_level(prcb);

                    // Проверяем что мы все еще next thread
                    if (*prcb).next_thread as *mut KTHREAD == thread {
                        let old_priority = (*thread).priority;
                        (*thread).priority = priority;

                        // Если приоритет понизился - ищем новый поток
                        if priority < old_priority {
                            if let Some(new_thread) =
                                ki_find_ready_thread(processor as u32, priority as i32 + 1)
                            {
                                // Нашли новый поток - ставим его на standby
                                let new_ptr = new_thread.as_ptr();
                                (*new_ptr).set_state(KTHREAD_STATE::Standby);
                                (*prcb).next_thread = new_ptr as *mut _;

                                // Текущий поток в deferred ready list
                                ki_insert_deferred_ready_list(thread);
                            }
                        }

                        ki_release_prcb_lock_from_dpc_level(prcb);
                    } else {
                        // Мы больше не next thread - освобождаем lock и пробуем снова
                        ki_release_prcb_lock_from_dpc_level(prcb);
                        continue;
                    }
                }

                KTHREAD_STATE::Running => {
                    // Получаем PRCB для потока и захватываем его lock
                    let processor = (*thread).next_processor;
                    let prcb = crate::ke::init::ke_get_processor_prcb(processor);

                    if prcb.is_null() {
                        (*thread).priority = priority;
                        break;
                    }

                    ki_acquire_prcb_lock_at_dpc_level(prcb);

                    // Проверяем что мы все еще current thread
                    if (*prcb).current_thread as *mut KTHREAD == thread {
                        let old_priority = (*thread).priority;
                        (*thread).priority = priority;

                        // Если приоритет понизился и нет pending next thread - ищем preemption
                        let mut request_interrupt = false;
                        if priority < old_priority && (*prcb).next_thread.is_null() {
                            if let Some(new_thread) =
                                ki_find_ready_thread(processor as u32, priority as i32 + 1)
                            {
                                // Нашли новый поток - ставим его на standby
                                let new_ptr = new_thread.as_ptr();
                                (*new_ptr).set_state(KTHREAD_STATE::Standby);
                                (*prcb).next_thread = new_ptr as *mut _;

                                // Нужен dispatch interrupt
                                request_interrupt = true;
                            }
                        }

                        ki_release_prcb_lock_from_dpc_level(prcb);

                        // Посылаем IPI если нужно и мы на другом CPU
                        if request_interrupt {
                            let current_cpu: u8 = {
                                let current_prcb = get_prcb();
                                if current_prcb.is_null() {
                                    0u8
                                } else {
                                    (*current_prcb).number
                                }
                            };

                            if current_cpu != processor {
                                // TODO: KiIpiSend для IPI на другой CPU
                                // Пока просто запрашиваем dispatch interrupt
                                ki_request_dispatch_interrupt(processor as u32);
                            } else {
                                ki_request_dispatch_interrupt(processor as u32);
                            }
                        }
                    } else {
                        // Мы больше не current thread - освобождаем lock и пробуем снова
                        ki_release_prcb_lock_from_dpc_level(prcb);
                        continue;
                    }
                }

                KTHREAD_STATE::DeferredReady => {
                    // Уже в deferred ready list - просто обновляем приоритет
                    // Он будет применен когда поток будет обработан
                    (*thread).priority = priority;
                }

                _ => {
                    // Waiting/Transition/Initialized/Terminated: просто обновляем приоритет
                    (*thread).priority = priority;
                }
            }

            // Состояние обработано успешно - выходим из цикла
            break;
        }
    }
}

// =============================================================================
// KeSetPriorityThread (публичная функция)
// =============================================================================

/// KeSetPriorityThread - устанавливает приоритет потока
///
/// Публичная функция с полным захватом locks.
/// НЕ изменяет BasePriority - для этого есть KeSetBasePriorityThread.
///
/// # Arguments
/// * `thread` - указатель на KTHREAD
/// * `priority` - новый приоритет (0-31)
///
/// # Returns
/// Старый приоритет потока
///
/// Источник: ReactOS ke/thrdobj.c:KeSetPriorityThread
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn KeSetPriorityThread(thread: *mut KTHREAD, priority: i8) -> i8 {
    unsafe { ke_set_priority_thread(thread, priority) }
}

/// ke_set_priority_thread - внутренняя версия KeSetPriorityThread
///
/// Публичная функция для использования внутри ядра (snake_case).
pub unsafe fn ke_set_priority_thread(thread: *mut KTHREAD, priority: i8) -> i8 {
    unsafe {
        if thread.is_null() {
            return 0;
        }

        debug_assert!(priority >= LOW_PRIORITY && priority <= HIGH_PRIORITY);

        // Захватываем dispatcher lock
        let old_irql = kf_raise_irql(DISPATCH_LEVEL);
        ke_acquire_spin_lock_at_dpc_level(&KI_DISPATCHER_LOCK);

        // Захватываем thread lock
        ki_acquire_thread_lock(thread);

        // Сохраняем старый приоритет и сбрасываем decrement
        let old_priority = (*thread).priority;
        (*thread).priority_decrement = 0;

        // Проверяем что приоритет изменился
        if priority != (*thread).priority {
            // Сбрасываем quantum
            (*thread).quantum = (*thread).quantum_reset;

            // Нормализация: если BasePriority != 0 и Priority == 0 → Priority = 1
            // Это защита от случайного понижения до 0 (который зарезервирован для idle)
            let mut new_priority = priority;
            if (*thread).base_priority != 0 && new_priority == 0 {
                new_priority = 1;
            }

            // Вызываем внутреннюю функцию
            ki_set_priority_thread(thread, new_priority);
        }

        // Освобождаем thread lock
        ki_release_thread_lock(thread);

        // Освобождаем dispatcher lock
        ke_release_spin_lock_from_dpc_level(&KI_DISPATCHER_LOCK);
        kf_lower_irql(old_irql);

        old_priority
    }
}

// =============================================================================
// KeSetBasePriorityThread
// =============================================================================

/// KeSetBasePriorityThread - устанавливает базовый приоритет потока
///
/// Изменяет BasePriority потока и пересчитывает Priority.
/// Используется для постоянного изменения приоритета (в отличие от boost).
///
/// # Arguments
/// * `thread` - указатель на KTHREAD
/// * `increment` - смещение от базового приоритета процесса
///
/// # Returns
/// Старый базовый приоритет потока (смещение)
///
/// Источник: ReactOS ke/thrdobj.c:KeSetBasePriorityThread
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn KeSetBasePriorityThread(thread: *mut KTHREAD, increment: i32) -> i32 {
    unsafe { ke_set_base_priority_thread(thread, increment) }
}

/// ke_set_base_priority_thread - внутренняя версия KeSetBasePriorityThread
pub unsafe fn ke_set_base_priority_thread(thread: *mut KTHREAD, increment: i32) -> i32 {
    unsafe {
        use crate::ps::process::KPROCESS;

        if thread.is_null() {
            return 0;
        }

        // Захватываем dispatcher lock
        let old_irql = kf_raise_irql(DISPATCH_LEVEL);
        ke_acquire_spin_lock_at_dpc_level(&KI_DISPATCHER_LOCK);

        // Захватываем thread lock
        ki_acquire_thread_lock(thread);

        // Получаем процесс
        let process = (*thread).process as *mut KPROCESS;
        if process.is_null() {
            ki_release_thread_lock(thread);
            ke_release_spin_lock_from_dpc_level(&KI_DISPATCHER_LOCK);
            kf_lower_irql(old_irql);
            return 0;
        }

        // Вычисляем старое смещение
        let process_base = (*process).base_priority;
        let old_base = (*thread).base_priority;
        let old_increment = (old_base - process_base) as i32;

        // Вычисляем новый базовый приоритет
        let mut new_base = process_base.saturating_add(increment as i8);

        // Ограничиваем диапазоном
        if new_base > HIGH_PRIORITY {
            new_base = HIGH_PRIORITY;
        }
        if new_base < LOW_PRIORITY {
            new_base = LOW_PRIORITY;
        }

        // Устанавливаем новый базовый приоритет
        (*thread).base_priority = new_base;

        // Сбрасываем boost
        (*thread).priority_decrement = 0;

        // Вычисляем новый текущий приоритет
        // Если текущий приоритет был ниже или равен старому base - используем новый base
        // Иначе сохраняем boost
        let new_priority = if (*thread).priority <= old_base {
            new_base
        } else {
            // Сохраняем boost относительно нового base
            let boost = (*thread).priority - old_base;
            let boosted = new_base.saturating_add(boost);
            if boosted > HIGH_PRIORITY {
                HIGH_PRIORITY
            } else {
                boosted
            }
        };

        // Устанавливаем новый приоритет через KiSetPriorityThread
        if new_priority != (*thread).priority {
            ki_set_priority_thread(thread, new_priority);
        }

        // Освобождаем thread lock
        ki_release_thread_lock(thread);

        // Освобождаем dispatcher lock
        ke_release_spin_lock_from_dpc_level(&KI_DISPATCHER_LOCK);
        kf_lower_irql(old_irql);

        old_increment
    }
}
