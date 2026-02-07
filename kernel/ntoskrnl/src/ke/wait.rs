//! Wait (dispatcher objects) — переписано с нуля (база Этапа A)
//!
//! Публичные NT точки входа (x64):
//! - KeWaitForSingleObject
//! - KeWaitForMultipleObjects
//! - KeDelayExecutionThread
//!
//! На этом этапе реализуем минимально корректный протокол:
//! - работа со списками ожидания под `KI_DISPATCHER_LOCK`
//! - переход потока в Waiting и вызов `ki_swap_thread()` (который освобождает lock)

#![allow(non_camel_case_types)]

use core::sync::atomic::Ordering;

use crate::hal::irql::DISPATCH_LEVEL;
use crate::hal::irql::kf_lower_irql;
use crate::hal::irql::kf_raise_irql;
use crate::ke::event::DISPATCHER_HEADER;
use crate::ke::event::EVENT_SYNCHRONIZATION_OBJECT;
use crate::ke::event::SEMAPHORE_OBJECT;
use crate::ke::sched::KI_DISPATCHER_LOCK;
use crate::ke::sched::ki_query_low_tick_count;
use crate::ke::sched::ki_swap_thread;
use crate::ke::spinlock::APC_LEVEL;
use crate::ke::spinlock::ke_acquire_spin_lock_at_dpc_level;
use crate::ke::spinlock::ke_release_spin_lock_from_dpc_level;
use crate::ke::thread::KTHREAD;
use crate::ke::thread::KTHREAD_STATE;
use crate::ke::thread::KWAIT_BLOCK;
use crate::ke::thread::KWAIT_REASON;
use crate::ke::thread::MAXIMUM_WAIT_OBJECTS;
use crate::ke::thread::THREAD_WAIT_OBJECTS;
use crate::ke::timer::KTIMER;
use crate::nt::BOOLEAN;
use crate::nt::LARGE_INTEGER;
use crate::nt::LIST_ENTRY;
use crate::nt::PVOID;
use crate::nt::ntstatus::*;

pub const WAIT_ANY: u16 = 0;
pub const WAIT_ALL: u16 = 1;

pub const KERNEL_MODE: u8 = 0;
pub const USER_MODE: u8 = 1;

pub const TIMER_WAIT_BLOCK: usize = THREAD_WAIT_OBJECTS;

#[inline]
fn timeout_to_i64(timeout: *const LARGE_INTEGER) -> Option<i64> {
    if timeout.is_null() {
        None
    } else {
        Some(unsafe { (*timeout).quad_part })
    }
}

#[inline]
fn compute_deadline_interrupt(timeout_100ns: i64) -> u64 {
    let now = crate::ke::time::ke_query_interrupt_time();
    if timeout_100ns < 0 {
        now.saturating_add((-timeout_100ns) as u64)
    } else {
        // absolute SYSTEM_TIME -> interrupt time via base_time
        let base_time = unsafe { (*(&raw const crate::ke::time::SHARED_USER_DATA)).base_time };
        if timeout_100ns <= base_time {
            0
        } else {
            (timeout_100ns - base_time) as u64
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "win64" fn KeWaitForSingleObject(
    object: PVOID,
    wait_reason: u8,
    wait_mode: u8,
    alertable: BOOLEAN,
    timeout: *const LARGE_INTEGER,
) -> i32 {
    unsafe {
        ke_wait_for_single_object(
            object,
            wait_reason,
            wait_mode,
            alertable != 0,
            timeout_to_i64(timeout),
        )
    }
}

pub unsafe fn ke_wait_for_single_object(
    object: PVOID,
    wait_reason: u8,
    wait_mode: u8,
    alertable: bool,
    timeout: Option<i64>,
) -> NTSTATUS {
    unsafe {
        use crate::arch::x86_64::pcr::get_prcb;


        if object.is_null() {
            return STATUS_INVALID_PARAMETER;
        }

        let prcb = get_prcb();
        if prcb.is_null() {
            return STATUS_UNSUCCESSFUL;
        }
        let thread = (*prcb).current_thread as *mut KTHREAD;
        if thread.is_null() {
            return STATUS_UNSUCCESSFUL;
        }


        let object_hdr = object as *mut DISPATCHER_HEADER;
        let deadline = match timeout {
            None => None,
            Some(0) => {
                return STATUS_TIMEOUT;
            },
            Some(t) => Some(compute_deadline_interrupt(t)),
        };

        loop {
            let old_irql = kf_raise_irql(DISPATCH_LEVEL);
            (*thread).wait_irql = old_irql;

            ke_acquire_spin_lock_at_dpc_level(&KI_DISPATCHER_LOCK);

            // kernel APC pending: если можно — уйдем и попробуем снова
            if (*thread).apc_state.kernel_apc_pending != 0 && old_irql < APC_LEVEL {
                ke_release_spin_lock_from_dpc_level(&KI_DISPATCHER_LOCK);
                kf_lower_irql(old_irql);
                continue;
            }

            // fast path: object signaled
            if (*object_hdr).signal_state.load(Ordering::Acquire) > 0 {
                // side effects минимально: для sync objects decrement делается в signal path,
                // здесь пока оставляем как есть.
                ke_release_spin_lock_from_dpc_level(&KI_DISPATCHER_LOCK);
                kf_lower_irql(old_irql);
                return STATUS_SUCCESS;
            }

            // Формируем wait block (и таймерный, если нужен)
            let wb0 = &mut (*thread).wait_block[0];
            wb0.thread = thread;
            wb0.object = object;
            wb0.wait_key = 0;
            wb0.wait_type = WAIT_ANY;

            if let Some(deadline) = deadline {
                let now = crate::ke::time::ke_query_interrupt_time();
                if deadline <= now {
                    ke_release_spin_lock_from_dpc_level(&KI_DISPATCHER_LOCK);
                    kf_lower_irql(old_irql);
                    return STATUS_TIMEOUT;
                }
                let remaining = deadline - now;
                let interval = -(remaining as i64);

                // Thread timer для timeout
                let timer = &mut (*thread).timer;
                let timer_wb = &mut (*thread).wait_block[TIMER_WAIT_BLOCK];
                timer_wb.thread = thread;
                timer_wb.object = timer as *mut KTIMER as PVOID;
                // NT-семантика: timer wait block возвращает STATUS_TIMEOUT
                timer_wb.wait_key = STATUS_TIMEOUT as u16;
                timer_wb.wait_type = WAIT_ANY;

                wb0.next_wait_block = timer_wb as *mut KWAIT_BLOCK;
                timer_wb.next_wait_block = wb0 as *mut KWAIT_BLOCK;

                // Вставляем timer wait block в timer wait list
                LIST_ENTRY::insert_tail(
                    &mut (*timer).header.wait_list_head as *mut LIST_ENTRY,
                    &mut timer_wb.wait_list_entry as *mut LIST_ENTRY,
                );

                // Ставим таймер на оставшееся время (relative).
                crate::ke::timer::ke_set_timer_ex(timer, interval, 0, None);
            } else {
                wb0.next_wait_block = wb0 as *mut KWAIT_BLOCK;
            }

            (*thread).wait_block_list = wb0 as *mut KWAIT_BLOCK;

            // Вставляем в список объекта
            LIST_ENTRY::insert_tail(
                &mut (*object_hdr).wait_list_head as *mut LIST_ENTRY,
                &mut wb0.wait_list_entry as *mut LIST_ENTRY,
            );

            (*thread).wait_status.store(0, Ordering::Release);
            (*thread).alertable = if alertable { 1 } else { 0 };
            (*thread).wait_mode = wait_mode;
            (*thread).wait_reason = wait_reason;
            (*thread).wait_time = ki_query_low_tick_count();
            (*thread).set_state(KTHREAD_STATE::Waiting);

            // Switch: ki_swap_thread освободит dispatcher lock и вернет status
            let status = ki_swap_thread();

            if status != STATUS_KERNEL_APC {
                return status;
            }

            // Kernel APC: deadline уже зафиксирован, на следующей итерации
            // будет пересчитано оставшееся время и переустановлен timer.
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "win64" fn KeWaitForMultipleObjects(
    count: u32,
    objects: *const PVOID,
    wait_type: u16,
    wait_reason: u8,
    wait_mode: u8,
    alertable: BOOLEAN,
    timeout: *const LARGE_INTEGER,
    wait_block_array: *mut KWAIT_BLOCK,
) -> i32 {
    unsafe {
        ke_wait_for_multiple_objects(
            count,
            objects,
            wait_type,
            wait_reason,
            wait_mode,
            alertable != 0,
            timeout_to_i64(timeout),
            wait_block_array,
        )
    }
}

pub unsafe fn ke_wait_for_multiple_objects(
    count: u32,
    objects: *const PVOID,
    wait_type: u16,
    wait_reason: u8,
    wait_mode: u8,
    alertable: bool,
    timeout: Option<i64>,
    wait_block_array: *mut KWAIT_BLOCK,
) -> NTSTATUS {
    unsafe {
        use crate::arch::x86_64::pcr::get_prcb;

        if count == 0 || (count as usize) > MAXIMUM_WAIT_OBJECTS || objects.is_null() {
            return STATUS_INVALID_PARAMETER;
        }
        if wait_type != WAIT_ANY && wait_type != WAIT_ALL {
            return STATUS_INVALID_PARAMETER;
        }

        let prcb = get_prcb();
        if prcb.is_null() {
            return STATUS_UNSUCCESSFUL;
        }
        let thread = (*prcb).current_thread as *mut KTHREAD;
        if thread.is_null() {
            return STATUS_UNSUCCESSFUL;
        }

        // Выбираем массив wait blocks
        let wait_blocks: *mut KWAIT_BLOCK = if (count as usize) <= THREAD_WAIT_OBJECTS {
            &mut (*thread).wait_block[0] as *mut KWAIT_BLOCK
        } else {
            if wait_block_array.is_null() {
                return STATUS_INVALID_PARAMETER;
            }
            wait_block_array
        };

        let deadline = match timeout {
            None => None,
            Some(0) => return STATUS_TIMEOUT,
            Some(t) => Some(compute_deadline_interrupt(t)),
        };

        loop {
            let old_irql = kf_raise_irql(DISPATCH_LEVEL);
            (*thread).wait_irql = old_irql;

            ke_acquire_spin_lock_at_dpc_level(&KI_DISPATCHER_LOCK);

            // kernel APC pending: если можно — уйдем и попробуем снова
            if (*thread).apc_state.kernel_apc_pending != 0 && old_irql < APC_LEVEL {
                ke_release_spin_lock_from_dpc_level(&KI_DISPATCHER_LOCK);
                kf_lower_irql(old_irql);
                continue;
            }

            // Быстрая проверка удовлетворения
            let mut first_signaled: i32 = -1;
            let mut all_signaled = true;
            for i in 0..(count as usize) {
                let obj = *objects.add(i) as *mut DISPATCHER_HEADER;
                if obj.is_null() {
                    ke_release_spin_lock_from_dpc_level(&KI_DISPATCHER_LOCK);
                    kf_lower_irql(old_irql);
                    return STATUS_INVALID_PARAMETER;
                }
                let s = (*obj).signal_state.load(Ordering::Acquire);
                if s > 0 {
                    if first_signaled < 0 {
                        first_signaled = i as i32;
                    }
                } else {
                    all_signaled = false;
                }
            }

            if wait_type == WAIT_ANY && first_signaled >= 0 {
                let obj = *objects.add(first_signaled as usize) as *mut DISPATCHER_HEADER;
                ki_wait_satisfy_object(obj);
                ke_release_spin_lock_from_dpc_level(&KI_DISPATCHER_LOCK);
                kf_lower_irql(old_irql);
                return first_signaled; // STATUS_WAIT_0 + index (STATUS_WAIT_0 == 0)
            }

            if wait_type == WAIT_ALL && all_signaled {
                for i in 0..(count as usize) {
                    let obj = *objects.add(i) as *mut DISPATCHER_HEADER;
                    ki_wait_satisfy_object(obj);
                }
                ke_release_spin_lock_from_dpc_level(&KI_DISPATCHER_LOCK);
                kf_lower_irql(old_irql);
                return STATUS_SUCCESS;
            }

            // Инициализация wait blocks для объектов
            for i in 0..(count as usize) {
                let wb = &mut *wait_blocks.add(i);
                let obj = *objects.add(i) as *mut DISPATCHER_HEADER;
                wb.wait_list_entry = LIST_ENTRY::new();
                wb.thread = thread;
                wb.object = obj as PVOID;
                wb.wait_type = wait_type;
                wb.block_state = 0;
                wb.wait_key = i as u16; // STATUS_WAIT_0 + i
            }

            // Связываем циклический список
            for i in 0..(count as usize) {
                let wb = &mut *wait_blocks.add(i);
                let next = if i + 1 == count as usize {
                    wait_blocks
                } else {
                    wait_blocks.add(i + 1)
                };
                wb.next_wait_block = next;
            }

            // Таймерный wait block (если есть timeout)
            let head: *mut KWAIT_BLOCK = wait_blocks;
            if let Some(deadline) = deadline {
                let now = crate::ke::time::ke_query_interrupt_time();
                if deadline <= now {
                    ke_release_spin_lock_from_dpc_level(&KI_DISPATCHER_LOCK);
                    kf_lower_irql(old_irql);
                    return STATUS_TIMEOUT;
                }
                let remaining = deadline - now;
                let interval = -(remaining as i64);

                let timer = &mut (*thread).timer;
                let timer_wb = &mut (*thread).wait_block[TIMER_WAIT_BLOCK];
                timer_wb.wait_list_entry = LIST_ENTRY::new();
                timer_wb.thread = thread;
                timer_wb.object = timer as *mut KTIMER as PVOID;
                timer_wb.wait_type = WAIT_ANY; // timer всегда как WaitAny
                timer_wb.block_state = 0;
                timer_wb.wait_key = STATUS_TIMEOUT as u16;

                // Включаем таймер в циклический список: last->timer, timer->head
                let last = &mut *wait_blocks.add(count as usize - 1);
                timer_wb.next_wait_block = head;
                last.next_wait_block = timer_wb as *mut KWAIT_BLOCK;

                // Вставляем timer wait block в список таймера
                LIST_ENTRY::insert_tail(
                    &mut (*timer).header.wait_list_head as *mut LIST_ENTRY,
                    &mut timer_wb.wait_list_entry as *mut LIST_ENTRY,
                );

                crate::ke::timer::ke_set_timer_ex(timer, interval, 0, None);
            }

            (*thread).wait_block_list = head;

            // Вставляем все object wait blocks в списки объектов
            for i in 0..(count as usize) {
                let wb = &mut *wait_blocks.add(i);
                let obj = wb.object as *mut DISPATCHER_HEADER;
                LIST_ENTRY::insert_tail(
                    &mut (*obj).wait_list_head as *mut LIST_ENTRY,
                    &mut wb.wait_list_entry as *mut LIST_ENTRY,
                );
            }

            (*thread).wait_status.store(0, Ordering::Release);
            (*thread).alertable = if alertable { 1 } else { 0 };
            (*thread).wait_mode = wait_mode;
            (*thread).wait_reason = wait_reason;
            (*thread).wait_time = ki_query_low_tick_count();
            (*thread).set_state(KTHREAD_STATE::Waiting);

            let status = ki_swap_thread();
            if status != STATUS_KERNEL_APC {
                return status;
            }
            // Kernel APC: deadline фиксирован, оставшееся время пересчитается на следующей итерации.
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "win64" fn KeDelayExecutionThread(
    wait_mode: u8,
    alertable: BOOLEAN,
    interval: *const LARGE_INTEGER,
) -> i32 {
    unsafe {
        let interval = if interval.is_null() {
            0
        } else {
            (*interval).quad_part
        };
        ke_delay_execution_thread(wait_mode, alertable != 0, interval)
    }
}

pub unsafe fn ke_delay_execution_thread(wait_mode: u8, alertable: bool, interval: i64) -> NTSTATUS {
    unsafe {
        use crate::arch::x86_64::pcr::get_prcb;

        crate::ke::debug::debug_raw("[WAIT] KeDelayExecutionThread: interval=");
        crate::ke::debug::debug_hex(interval as u64);
        crate::ke::debug::debug_raw("\n");

        let _ = wait_mode;
        let prcb = get_prcb();
        if prcb.is_null() {
            return STATUS_UNSUCCESSFUL;
        }
        let thread = (*prcb).current_thread as *mut KTHREAD;
        if thread.is_null() {
            return STATUS_UNSUCCESSFUL;
        }

        if interval == 0 {
            return STATUS_SUCCESS;
        }

        let deadline = compute_deadline_interrupt(interval);

        crate::ke::debug::debug_raw("[WAIT] deadline=");
        crate::ke::debug::debug_hex(deadline);
        crate::ke::debug::debug_raw("\n");

        // Delay = ожидание только на thread timer.
        loop {
            crate::ke::debug::debug_raw("[WAIT] Loop iteration\n");
            let old_irql = kf_raise_irql(DISPATCH_LEVEL);
            (*thread).wait_irql = old_irql;

            ke_acquire_spin_lock_at_dpc_level(&KI_DISPATCHER_LOCK);

            if (*thread).apc_state.kernel_apc_pending != 0 && old_irql < APC_LEVEL {
                ke_release_spin_lock_from_dpc_level(&KI_DISPATCHER_LOCK);
                kf_lower_irql(old_irql);
                continue;
            }

            // Alertable/user APC (минимально): как и в single wait, пока не реализуем alert list —
            // оставляем только user_apc_pending.
            if alertable && (*thread).apc_state.user_apc_pending != 0 {
                ke_release_spin_lock_from_dpc_level(&KI_DISPATCHER_LOCK);
                kf_lower_irql(old_irql);
                return STATUS_USER_APC;
            }

            let timer = &mut (*thread).timer;
            let timer_wb = &mut (*thread).wait_block[TIMER_WAIT_BLOCK];
            timer_wb.thread = thread;
            timer_wb.object = timer as *mut KTIMER as PVOID;
            timer_wb.wait_key = STATUS_SUCCESS as u16; // Delay returns SUCCESS on expiry
            timer_wb.wait_type = WAIT_ANY;
            timer_wb.next_wait_block = timer_wb as *mut KWAIT_BLOCK;

            (*thread).wait_block_list = timer_wb as *mut KWAIT_BLOCK;

            LIST_ENTRY::insert_tail(
                &mut (*timer).header.wait_list_head as *mut LIST_ENTRY,
                &mut timer_wb.wait_list_entry as *mut LIST_ENTRY,
            );

            let now = crate::ke::time::ke_query_interrupt_time();
            crate::ke::debug::debug_raw("[WAIT] now=");
            crate::ke::debug::debug_hex(now);
            crate::ke::debug::debug_raw("\n");

            if deadline <= now {
                crate::ke::debug::debug_raw("[WAIT] Already expired, returning\n");
                ke_release_spin_lock_from_dpc_level(&KI_DISPATCHER_LOCK);
                kf_lower_irql(old_irql);
                return STATUS_SUCCESS;
            }
            let remaining = deadline - now;
            let rel = -(remaining as i64);
            
            crate::ke::debug::debug_raw("[WAIT] Setting timer, rel=");
            crate::ke::debug::debug_hex(rel as u64);
            crate::ke::debug::debug_raw("\n");
            
            crate::ke::timer::ke_set_timer_ex(timer, rel, 0, None);

            (*thread).wait_status.store(0, Ordering::Release);
            (*thread).alertable = if alertable { 1 } else { 0 };
            (*thread).wait_mode = wait_mode;
            (*thread).wait_reason = KWAIT_REASON::DelayExecution as u8;
            (*thread).wait_time = ki_query_low_tick_count();
            (*thread).set_state(KTHREAD_STATE::Waiting);

            crate::ke::debug::debug_raw("[WAIT] Calling ki_swap_thread\n");
            let status = ki_swap_thread();
            crate::ke::debug::debug_raw("[WAIT] Returned from ki_swap_thread, status=");
            crate::ke::debug::debug_hex(status as u64);
            crate::ke::debug::debug_raw("\n");

            if status != STATUS_KERNEL_APC {
                crate::ke::debug::debug_raw("[WAIT] Returning status\n");
                return status;
            }
            crate::ke::debug::debug_raw("[WAIT] Got KERNEL_APC, continuing loop\n");
        }
    }
}

#[inline]
fn ki_wait_satisfy_object(object: *mut DISPATCHER_HEADER) {
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
