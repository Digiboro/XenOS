//! Idle Process и Idle Thread
//!
//! Реализация Idle Process и Idle Thread в соответствии с ReactOS.
//!
//! # Архитектура
//!
//! ```text
//! KiSystemStartup
//!     │
//!     ├── ki_initialize_idle_process()  ← ДО Executive init
//!     ├── ki_initialize_idle_thread()
//!     │
//!     └── ki_phase0_init()              ← Executive init
//!         └── psp_init_phase0()
//!             └── set_image_name("Idle") ← только имя
//!
//! ki_idle_loop() - основной цикл:
//! ┌─────────────────────────────────────────────────────────┐
//! │  loop {                                                  │
//! │      _enable();           // STI                         │
//! │      YieldProcessor();    // PAUSE x2                    │
//! │      _disable();          // CLI                         │
//! │                                                          │
//! │      if (has_work) {                                     │
//! │          ki_dispatch_interrupt();                        │
//! │      } else if (next_thread) {                           │
//! │          ki_swap_context(APC_LEVEL, old_thread);         │
//! │      } else {                                            │
//! │          sti; hlt;  // Idle sleep                        │
//! │      }                                                   │
//! │  }                                                       │
//! └─────────────────────────────────────────────────────────┘
//! ```
//!
//! # Отступления и упрощения
//!
//! - PowerState.IdleFunction не реализован, используется HLT
//! - Один Idle Thread на BSP (AP поддержка требует отдельной реализации)
//!
//! Источники:
//! - ReactOS: ke/krnlinit.c, ke/i386/kiinit.c, ke/procobj.c, ke/thrdobj.c
//! - ReactOS: ke/i386/thrdini.c (KiIdleLoop)
//! - MSDN: Windows Internals

#![allow(dead_code)]

use core::mem::MaybeUninit;
use core::sync::atomic::AtomicBool;
use core::sync::atomic::Ordering;

use crate::arch::x86_64::pcr::KPRCB;
use crate::arch::x86_64::pcr::get_prcb;
use crate::ke::thread::KTHREAD;
use crate::ke::thread::KTHREAD_STATE;
use crate::ke::thread::ke_initialize_thread;
use crate::ke::thread::priority::HIGH_PRIORITY;
use crate::nt::LIST_ENTRY;
use crate::nt::PVOID;
use crate::ps::process::EPROCESS;
use crate::ps::process::PS_IDLE_PROCESS;
use crate::ps::process::ke_initialize_process;
use crate::ps::thread::ETHREAD;

// =============================================================================
// Константы
// =============================================================================

/// Размер стека для Idle Thread (64KB debug, 32KB release)
#[cfg(debug_assertions)]
pub const IDLE_STACK_SIZE: usize = 0x10000;

#[cfg(not(debug_assertions))]
pub const IDLE_STACK_SIZE: usize = 0x8000;

// =============================================================================
// Статические структуры (ReactOS: ke/krnlinit.c:40-41)
// =============================================================================

/// KiInitialProcess - статический Idle Process
///
/// ReactOS: EPROCESS KiInitialProcess (krnlinit.c:40)
static mut KI_INITIAL_PROCESS: MaybeUninit<EPROCESS> = MaybeUninit::uninit();

/// KiInitialThread - статический Idle Thread для BSP
///
/// ReactOS: ETHREAD KiInitialThread (krnlinit.c:41)
static mut KI_INITIAL_THREAD: MaybeUninit<ETHREAD> = MaybeUninit::uninit();

/// Стек для BSP Idle Thread (выровнен по 16 байт)
#[repr(align(16))]
struct AlignedStack([u8; IDLE_STACK_SIZE]);

static mut BSP_IDLE_STACK: AlignedStack = AlignedStack([0; IDLE_STACK_SIZE]);

/// Флаг инициализации Idle Process
static IDLE_PROCESS_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Флаг инициализации Idle Thread
static IDLE_THREAD_INITIALIZED: AtomicBool = AtomicBool::new(false);

// =============================================================================
// Accessor функции
// =============================================================================

/// Возвращает указатель на KiInitialProcess
#[inline]
pub unsafe fn ki_get_initial_process() -> *mut EPROCESS {
    // Используем &raw mut для избежания создания mutable reference
    (&raw mut KI_INITIAL_PROCESS).cast::<EPROCESS>()
}

/// Возвращает указатель на KiInitialThread
#[inline]
pub unsafe fn ki_get_initial_thread() -> *mut ETHREAD {
    // Используем &raw mut для избежания создания mutable reference
    (&raw mut KI_INITIAL_THREAD).cast::<ETHREAD>()
}

/// Возвращает указатель на BSP Idle Stack (верхняя граница)
#[inline]
pub unsafe fn ki_get_idle_stack_top() -> *mut u8 {
    unsafe {
        // Используем &raw mut для избежания создания mutable reference
        let stack_base = (&raw mut BSP_IDLE_STACK).cast::<u8>();
        stack_base.add(IDLE_STACK_SIZE)
    }
}

// =============================================================================
// ki_initialize_idle_process (ReactOS: ke/i386/kiinit.c:500-520)
// =============================================================================

/// ki_initialize_idle_process - инициализация Idle Process
///
/// Источник: ReactOS ke/i386/kiinit.c:500-520
///
/// Должен вызываться ДО Executive init (psp_init_phase0).
pub unsafe fn ki_initialize_idle_process() {
    unsafe {
        if IDLE_PROCESS_INITIALIZED.load(Ordering::Acquire) {
            return;
        }

        let process = ki_get_initial_process();

        // 1. Обнуляем структуру
        core::ptr::write_bytes(process, 0, 1);

        // 2. Инициализируем KPROCESS
        // ReactOS: KeInitializeProcess(&KiInitialProcess.Pcb, 0, MAXULONG_PTR, &DirectoryTableBase, FALSE)
        ke_initialize_process(
            &raw mut (*process).pcb,
            0,        // Priority = 0 (idle)
            u64::MAX, // Affinity = all CPUs
            0,        // DirectoryTableBase = 0 (kernel space)
            false,    // AutoAlignment = FALSE
        );

        // 3. QuantumReset = MAXCHAR (127) - ReactOS: kiinit.c:513
        (*process).pcb.quantum_reset = i8::MAX; // 127

        // 4. Регистрируем как PsIdleProcess
        PS_IDLE_PROCESS.store(process, Ordering::Release);

        // 5. Инициализируем EPROCESS поля
        (*process).unique_process_id.store(0, Ordering::Release); // PID = 0
        (*process).priority_class = crate::ps::types::PROCESS_PRIORITY_CLASS_IDLE;

        // 6. Active process links (будет добавлен в список позже в psp_init_phase0)
        LIST_ENTRY::init_head(&raw mut (*process).active_process_links);

        IDLE_PROCESS_INITIALIZED.store(true, Ordering::Release);

        crate::kd::dbg_print("[KE/IDLE] Idle Process initialized (static)\n");
    }
}

// =============================================================================
// ki_initialize_idle_thread (ReactOS: ke/i386/kiinit.c:530-560)
// =============================================================================

/// ki_initialize_idle_thread - инициализация Idle Thread для BSP
///
/// Источник: ReactOS ke/i386/kiinit.c:530-560
///
/// # Параметры
/// - `prcb` - указатель на KPRCB текущего процессора
/// - `processor_number` - номер процессора (0 для BSP)
///
/// # Возвращает
/// true если успешно
pub unsafe fn ki_initialize_idle_thread(prcb: *mut KPRCB, processor_number: u8) -> bool {
    unsafe {
        if prcb.is_null() {
            return false;
        }

        if !IDLE_PROCESS_INITIALIZED.load(Ordering::Acquire) {
            crate::kd::dbg_print("[KE/IDLE] ERROR: Idle Process not initialized!\n");
            return false;
        }

        // Для BSP используем статическую структуру
        // Для AP нужна динамическая аллокация (не реализовано)
        if processor_number != 0 {
            crate::kd::dbg_print("[KE/IDLE] WARNING: AP idle thread not implemented\n");
            return false;
        }

        if IDLE_THREAD_INITIALIZED.load(Ordering::Acquire) {
            return true;
        }

        let thread = ki_get_initial_thread();
        let idle_process = PS_IDLE_PROCESS.load(Ordering::Acquire);
        let stack_top = ki_get_idle_stack_top();

        // 1. Обнуляем структуру
        core::ptr::write_bytes(thread, 0, 1);

        // 2. Инициализируем KTHREAD через KeInitializeThread
        ke_initialize_thread(
            &raw mut (*idle_process).pcb,
            &raw mut (*thread).tcb,
            None,                  // SystemRoutine = NULL
            None,                  // StartRoutine = NULL
            core::ptr::null_mut(), // StartContext = NULL
            core::ptr::null_mut(), // Context = NULL
            core::ptr::null_mut(), // Teb = NULL
            stack_top as PVOID,    // KernelStack
            IDLE_STACK_SIZE,       // StackSize
        );

        // 3. Manual overrides (ReactOS: kiinit.c:548-555)
        let kthread = &raw mut (*thread).tcb;

        // NextProcessor = ProcessorNumber
        (*kthread).next_processor = processor_number;

        // Priority = HIGH_PRIORITY (временно, потом понизим до 0)
        (*kthread).priority = HIGH_PRIORITY;

        // State = Running (bypass normal scheduling)
        (*kthread)
            .state
            .store(KTHREAD_STATE::Running as u8, Ordering::Release);

        // Affinity = только этот процессор (XenOS: u64, не AtomicU64)
        (*kthread).affinity = 1u64 << processor_number;

        // WaitIrql = DISPATCH_LEVEL
        (*kthread).wait_irql = crate::hal::irql::DISPATCH_LEVEL;

        // 4. ETHREAD поля
        (*thread)
            .thread_process
            .store(idle_process, Ordering::Release);
        (*thread).cid.unique_process = 0 as PVOID; // PID = 0
        (*thread).cid.unique_thread = 0 as PVOID; // TID = 0

        // 5. Добавляем в список потоков процесса
        let process_thread_list = &raw mut (*idle_process).pcb.thread_list_head;
        let thread_entry = &raw mut (*kthread).thread_list_entry;
        LIST_ENTRY::insert_tail(process_thread_list, thread_entry);

        // 6. Устанавливаем в PRCB
        (*prcb).current_thread = kthread as PVOID;
        (*prcb).next_thread = core::ptr::null_mut();
        (*prcb).idle_thread = kthread as PVOID;

        IDLE_THREAD_INITIALIZED.store(true, Ordering::Release);

        crate::kd::dbg_print("[KE/IDLE] Idle Thread initialized for CPU ");
        crate::kd::dbg_print_num(processor_number as u64);
        crate::kd::dbg_print(" (static ETHREAD, priority=HIGH)\n");

        true
    }
}

// =============================================================================
// ki_set_idle_thread_priority (ReactOS: ke/i386/kiinit.c:628)
// =============================================================================

/// ki_set_idle_thread_priority - понижает приоритет Idle Thread до 0
///
/// KiInitializeKernel: KeSetPriorityThread(IdleThread, 0) + KiIdleSummary
///
/// Понижает приоритет Idle Thread до 0 и добавляет процессор в KiIdleSummary.
/// Это позволяет Phase1 потоку получить CPU.
///
/// Источник: NT ke/i386/kiinit.c (KiInitializeKernel:624-628)
///
/// Вызывается ПОСЛЕ Executive init (psp_init_phase0).
pub unsafe fn ki_set_idle_thread_priority(prcb: *mut KPRCB) {
    unsafe {
        use core::sync::atomic::Ordering;

        if prcb.is_null() {
            return;
        }

        let idle_thread = (*prcb).idle_thread as *mut KTHREAD;
        if idle_thread.is_null() {
            return;
        }

        // NT: KeSetPriorityThread(IdleThread, 0)
        (*idle_thread).priority = 0;
        (*idle_thread).base_priority = 0;

        // NT way: установка KiIdleSummary
        // Добавляем процессор в idle summary если нет pending потока
        // TODO: PRCB lock (ke_acquire_prcb_lock / ke_release_prcb_lock)
        if (*prcb).next_thread.is_null() {
            let number = (*prcb).number as u32;
            crate::ke::sched::KI_IDLE_SUMMARY.fetch_or(1u64 << number, Ordering::Release);
        }

        crate::kd::dbg_print("[KE/IDLE] Idle Thread priority set to 0, KiIdleSummary updated\n");
    }
}

// =============================================================================
// ki_idle_loop (ReactOS: ke/i386/thrdini.c:260-350)
// =============================================================================

/// ki_idle_loop - основной цикл Idle Thread
///
/// Источник: ReactOS ke/i386/thrdini.c:260-350
///
/// # Отличия от текущего ki_idle_loop:
/// - STI/CLI в каждой итерации
/// - YieldProcessor x2 после STI
/// - Проверка timer_request
/// - KiSwapContext при наличии next_thread
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ki_idle_loop() -> ! {
    unsafe {
        use core::arch::asm;

        use crate::hal::irql::APC_LEVEL;

        // Одноразовый лог
        static ENTERED: AtomicBool = AtomicBool::new(false);
        if !ENTERED.swap(true, Ordering::AcqRel) {
            crate::kd::dbg_print("[KE/IDLE] Entered ki_idle_loop\n");
        }

        loop {
            // 1. Enable interrupts (ReactOS: _enable())
            asm!("sti", options(nomem, nostack, preserves_flags));

            // 2. YieldProcessor x2 (ReactOS: YieldProcessor())
            crate::arch::x86_64::cpu::yield_processor();
            crate::arch::x86_64::cpu::yield_processor();

            // 3. Disable interrupts (ReactOS: _disable())
            asm!("cli", options(nomem, nostack, preserves_flags));

            let prcb = get_prcb();
            if prcb.is_null() {
                // Если нет PRCB - просто sleep
                asm!("sti; hlt", options(nomem, nostack, preserves_flags));
                continue;
            }

            // 4. Check for DPC work
            if (*prcb).dpc_queue_depth > 0 || (*prcb).dpc_requested != 0 {
                crate::ke::dpc::ki_dispatch_interrupt();
                continue;
            }

            // 5. Check for timer request (отложенная обработка таймеров)
            if (*prcb).timer_request != 0 {
                crate::ke::dpc::ki_dispatch_interrupt();
                continue;
            }

            // 6. Check for deferred ready list
            if !(*prcb).deferred_ready_list_head.next.is_null() {
                crate::ke::dpc::ki_dispatch_interrupt();
                continue;
            }

            // 7. Check for scheduled thread
            // NT/ReactOS pattern: ki_swap_context сам управляет next_thread/current_thread
            // Не обнуляем next_thread и не меняем current_thread здесь - это делает ki_swap_context
            if !(*prcb).next_thread.is_null() {
                let old_thread = (*prcb).current_thread as *mut KTHREAD;

                // KiSwapContext выполнит:
                // - Обнуление next_thread
                // - Смену current_thread
                // - Установку состояния Running для нового потока
                // - Переключение контекста
                crate::arch::x86_64::context::ki_swap_context(APC_LEVEL, old_thread);
                continue;
            }

            // 7.5. Проверяем ready_summary - если есть потоки в ready queue, выбираем и переключаемся
            // Это нужно, т.к. ki_ready_thread может добавить поток в ready queue без установки next_thread
            // (например, если приоритет нового потока <= приоритету текущего)
            if (*prcb).ready_summary != 0 {
                // Захватываем dispatcher lock для выбора потока
                crate::ke::spinlock::ke_acquire_spin_lock_at_dpc_level(
                    &crate::ke::sched::KI_DISPATCHER_LOCK,
                );

                if let Some(next) = crate::ke::sched::ki_find_ready_thread((*prcb).number as u32, 0)
                {
                    let next_ptr = next.as_ptr();
                    (*next_ptr).set_state(KTHREAD_STATE::Standby);
                    crate::ke::sched::ki_set_prcb_next_thread(prcb, next_ptr);

                    crate::ke::spinlock::ke_release_spin_lock_from_dpc_level(
                        &crate::ke::sched::KI_DISPATCHER_LOCK,
                    );

                    let old_thread = (*prcb).current_thread as *mut KTHREAD;
                    crate::arch::x86_64::context::ki_swap_context(APC_LEVEL, old_thread);
                    continue;
                }

                crate::ke::spinlock::ke_release_spin_lock_from_dpc_level(
                    &crate::ke::sched::KI_DISPATCHER_LOCK,
                );
            }

            // 8. Idle function (ReactOS: Prcb->PowerState.IdleFunction)
            // Пока используем простой HLT
            asm!("sti; hlt", options(nomem, nostack, preserves_flags));
        }
    }
}
