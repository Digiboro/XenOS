//! APC (Asynchronous Procedure Call) - асинхронные вызовы процедур
//!
//! Источники:
//! - NT5: ke/apc.c
//! - ReactOS: ke/apc.c

#![allow(dead_code)]

use core::sync::atomic::AtomicU64;
use core::sync::atomic::Ordering;

use crate::arch::x86_64::pcr;
use crate::hal::APC_LEVEL;
use crate::hal::PASSIVE_LEVEL;
use crate::hal::kf_lower_irql;
use crate::hal::kf_raise_irql;
use crate::ke::thread::KTHREAD;
use crate::nt::LIST_ENTRY;
use crate::nt::PVOID;

/// KPROCESSOR_MODE
pub const KERNEL_MODE: u8 = 0;
pub const USER_MODE: u8 = 1;

/// KAPC - структура APC
#[repr(C)]
pub struct KAPC {
    /// Type
    pub type_: u8,
    /// Spare byte
    pub spare_byte0: u8,
    /// Size
    pub size: u8,
    /// Spare byte
    pub spare_byte1: u8,
    /// Spare long
    pub spare_long0: u32,
    /// Thread
    pub thread: *mut KTHREAD,
    /// APC list entry
    pub apc_list_entry: LIST_ENTRY,
    /// Kernel routine
    pub kernel_routine: Option<PKKERNEL_ROUTINE>,
    /// Rundown routine
    pub rundown_routine: Option<PKRUNDOWN_ROUTINE>,
    /// Normal routine
    pub normal_routine: Option<PKNORMAL_ROUTINE>,
    /// Normal context
    pub normal_context: PVOID,
    /// System argument 1
    pub system_argument1: PVOID,
    /// System argument 2
    pub system_argument2: PVOID,
    /// APC state index
    pub apc_state_index: i8,
    /// APC mode
    pub apc_mode: i8,
    /// Inserted flag
    pub inserted: u8,
}

impl KAPC {
    pub const fn new() -> Self {
        Self {
            type_: 0,
            spare_byte0: 0,
            size: core::mem::size_of::<Self>() as u8,
            spare_byte1: 0,
            spare_long0: 0,
            thread: core::ptr::null_mut(),
            apc_list_entry: LIST_ENTRY::new(),
            kernel_routine: None,
            rundown_routine: None,
            normal_routine: None,
            normal_context: core::ptr::null_mut(),
            system_argument1: core::ptr::null_mut(),
            system_argument2: core::ptr::null_mut(),
            apc_state_index: 0,
            apc_mode: 0,
            inserted: 0,
        }
    }
}

/// Kernel routine type
#[allow(non_camel_case_types)]
pub type PKKERNEL_ROUTINE = unsafe extern "win64" fn(
    apc: *mut KAPC,
    normal_routine: *mut Option<PKNORMAL_ROUTINE>,
    normal_context: *mut PVOID,
    system_argument1: *mut PVOID,
    system_argument2: *mut PVOID,
);

/// Normal routine type
#[allow(non_camel_case_types)]
pub type PKNORMAL_ROUTINE = unsafe extern "win64" fn(
    normal_context: PVOID,
    system_argument1: PVOID,
    system_argument2: PVOID,
);

/// Rundown routine type
#[allow(non_camel_case_types)]
pub type PKRUNDOWN_ROUTINE = unsafe extern "win64" fn(apc: *mut KAPC);

// =============================================================================
// KiApcInterruptHandler - interrupt handler для vector 0x1F
// =============================================================================

/// KiApcInterruptHandler - обработчик APC interrupt (vector 0x1F)
///
/// Вызывается из KiApcInterrupt assembly stub.
/// Сбрасывает флаг pending и вызывает ki_deliver_apc.
///
/// # Safety
/// Вызывается из interrupt context
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn KiApcInterruptHandler() {
    unsafe {
        let prcb = pcr::get_prcb();
        if prcb.is_null() {
            return;
        }

        // Сбрасываем флаг pending APC interrupt
        (*prcb).apc_interrupt_requested = 0;

        // Вызываем ki_deliver_apc для обработки pending APCs
        // previous_mode = KERNEL_MODE для interrupt context
        ki_deliver_apc(KERNEL_MODE, core::ptr::null_mut(), core::ptr::null_mut());
    }
}

// =============================================================================
// KiDeliverApc - доставка APC
// =============================================================================

/// KiDeliverApc - доставка APC текущему потоку
///
/// Соответствует ReactOS KiDeliverApc (ke/apc.c)
///
/// # Arguments
/// * `previous_mode` - режим при входе в kernel
/// * `exception_frame` - фрейм исключения (для user mode APC)
/// * `trap_frame` - trap frame (для user mode APC)
///
/// # Safety
/// Должен вызываться на APC_LEVEL
pub unsafe fn ki_deliver_apc(previous_mode: u8, _exception_frame: PVOID, _trap_frame: PVOID) {
    unsafe {
        let prcb = pcr::get_prcb();
        if prcb.is_null() {
            return;
        }

        let thread = (*prcb).current_thread as *mut KTHREAD;
        if thread.is_null() {
            return;
        }

        // =========================================================================
        // ДОСТАВКА KERNEL APC
        // =========================================================================

        // Повышаем IRQL до APC_LEVEL
        let old_irql = kf_raise_irql(APC_LEVEL);

        // Обрабатываем kernel APC queue
        while (*thread).apc_state.kernel_apc_pending != 0 {
            // Захватываем APC lock
            ki_acquire_apc_lock(thread);

            let apc_list =
                &mut (*thread).apc_state.apc_list_head[KERNEL_MODE as usize] as *mut LIST_ENTRY;

            if LIST_ENTRY::is_empty(apc_list) {
                (*thread).apc_state.kernel_apc_pending = 0;
                ki_release_apc_lock(thread);
                break;
            }

            // Извлекаем APC из списка
            let entry = (*apc_list).flink;
            let apc = crate::containing_record!(entry, KAPC, apc_list_entry);

            // Проверяем что APC inserted
            if (*apc).inserted == 0 {
                ki_release_apc_lock(thread);
                continue;
            }

            // Удаляем из списка
            LIST_ENTRY::remove_entry(entry);
            (*apc).inserted = 0;

            // Если список пуст - сбрасываем pending
            if LIST_ENTRY::is_empty(apc_list) {
                (*thread).apc_state.kernel_apc_pending = 0;
            }

            ki_release_apc_lock(thread);

            // Понижаем IRQL для вызова APC routine
            kf_lower_irql(PASSIVE_LEVEL);

            // Вызываем kernel routine
            if let Some(kernel_routine) = (*apc).kernel_routine {
                let mut normal_routine = (*apc).normal_routine;
                let mut normal_context = (*apc).normal_context;
                let mut system_argument1 = (*apc).system_argument1;
                let mut system_argument2 = (*apc).system_argument2;

                kernel_routine(
                    apc,
                    &mut normal_routine,
                    &mut normal_context,
                    &mut system_argument1,
                    &mut system_argument2,
                );

                // Если есть normal routine - вызываем
                if let Some(normal) = normal_routine {
                    // Проверяем что kernel APC не disabled
                    if (*thread).kernel_apc_disable == 0 {
                        normal(normal_context, system_argument1, system_argument2);
                    }
                }
            }

            // Возвращаем IRQL
            kf_raise_irql(APC_LEVEL);
        }

        // =========================================================================
        // ДОСТАВКА USER APC (если previous_mode == UserMode)
        // =========================================================================

        if previous_mode == USER_MODE
            && (*thread).apc_state.user_apc_pending != 0
            && (*thread).special_apc_disable == 0
        {
            // TODO: Реализовать user mode APC delivery
            // Это требует модификации trap frame для возврата через APC dispatcher
        }

        // Восстанавливаем IRQL
        kf_lower_irql(old_irql);
    }
}

/// APC queue lock (простая реализация через thread field)
static APC_LOCK: AtomicU64 = AtomicU64::new(0);

/// Захват APC lock
unsafe fn ki_acquire_apc_lock(_thread: *mut KTHREAD) {
    loop {
        if APC_LOCK
            .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            break;
        }
        crate::arch::x86_64::cpu::yield_processor();
    }
}

/// Освобождение APC lock
unsafe fn ki_release_apc_lock(_thread: *mut KTHREAD) {
    APC_LOCK.store(0, Ordering::Release);
}

/// KeInitializeApc - инициализация APC
pub unsafe fn ke_initialize_apc(
    apc: *mut KAPC,
    thread: *mut KTHREAD,
    apc_state_index: i8,
    kernel_routine: Option<PKKERNEL_ROUTINE>,
    rundown_routine: Option<PKRUNDOWN_ROUTINE>,
    normal_routine: Option<PKNORMAL_ROUTINE>,
    apc_mode: i8,
    normal_context: PVOID,
) {
    unsafe {
        if apc.is_null() {
            return;
        }

        (*apc).type_ = 0x12; // ApcObject
        (*apc).size = core::mem::size_of::<KAPC>() as u8;
        (*apc).thread = thread;
        (*apc).apc_state_index = apc_state_index;
        (*apc).kernel_routine = kernel_routine;
        (*apc).rundown_routine = rundown_routine;
        (*apc).normal_routine = normal_routine;
        (*apc).apc_mode = apc_mode;
        (*apc).normal_context = normal_context;
        (*apc).system_argument1 = core::ptr::null_mut();
        (*apc).system_argument2 = core::ptr::null_mut();
        (*apc).inserted = 0;
    }
}

/// KeInsertQueueApc - вставка APC в очередь потока
pub unsafe fn ke_insert_queue_apc(
    apc: *mut KAPC,
    system_argument1: PVOID,
    system_argument2: PVOID,
    _increment: i8,
) -> bool {
    unsafe {
        if apc.is_null() {
            return false;
        }

        let thread = (*apc).thread;
        if thread.is_null() {
            return false;
        }

        // Уже вставлен?
        if (*apc).inserted != 0 {
            return false;
        }

        ki_acquire_apc_lock(thread);

        (*apc).system_argument1 = system_argument1;
        (*apc).system_argument2 = system_argument2;
        (*apc).inserted = 1;

        let mode = (*apc).apc_mode as usize;
        if mode > 1 {
            ki_release_apc_lock(thread);
            return false;
        }

        let apc_list = &mut (*thread).apc_state.apc_list_head[mode] as *mut LIST_ENTRY;
        LIST_ENTRY::insert_tail(apc_list, &mut (*apc).apc_list_entry as *mut LIST_ENTRY);

        // Устанавливаем pending flag
        if mode == KERNEL_MODE as usize {
            (*thread).apc_state.kernel_apc_pending = 1;
        } else {
            (*thread).apc_state.user_apc_pending = 1;
        }

        ki_release_apc_lock(thread);

        true
    }
}
