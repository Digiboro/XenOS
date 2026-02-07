//! KTHREAD - Kernel Thread
//!
//! Источники:
//! - NT5: ke/thrdobj.c, inc/ke.h
//! - ReactOS: ke/thrdobj.c, include/ndk/ketypes.h

#![allow(dead_code)]
#![allow(non_camel_case_types)]

use core::sync::atomic::AtomicI32;
use core::sync::atomic::AtomicU8;
use core::sync::atomic::AtomicU64;
use core::sync::atomic::Ordering;

use super::dpc::KDPC;
use super::event::DISPATCHER_HEADER;
use super::event::KOBJECT_TYPE;
use super::spinlock::KSPIN_LOCK;
use super::timer::KTIMER;
use crate::nt::LIST_ENTRY;
use crate::nt::PVOID;
use crate::nt::SINGLE_LIST_ENTRY;
use crate::nt::UCHAR;
use crate::nt::ULONG;

/// KADJUST_REASON - причина корректировки приоритета
///
/// Используется в KiDeferredReadyThread для определения
/// типа boost при добавлении потока в ready queue.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KADJUST_REASON {
    /// Нет корректировки
    AdjustNone = 0,
    /// Unwait boost (после пробуждения из ожидания)
    AdjustUnwait = 1,
    /// Explicit boost (после release mutex и т.д.)
    AdjustBoost = 2,
}

/// Приоритеты потоков
pub mod priority {
    pub const LOW_PRIORITY: i8 = 0;
    pub const LOW_REALTIME_PRIORITY: i8 = 16;
    pub const HIGH_PRIORITY: i8 = 31;
    pub const MAXIMUM_PRIORITY: i8 = 32;
}

/// Состояния потока
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KTHREAD_STATE {
    Initialized = 0,
    Ready = 1,
    Running = 2,
    Standby = 3,
    Terminated = 4,
    Waiting = 5,
    Transition = 6,
    DeferredReady = 7,
    GateWait = 8,
}

/// Wait reason
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KWAIT_REASON {
    Executive = 0,
    FreePage = 1,
    PageIn = 2,
    PoolAllocation = 3,
    DelayExecution = 4,
    Suspended = 5,
    UserRequest = 6,
    WrExecutive = 7,
    WrFreePage = 8,
    WrPageIn = 9,
    WrPoolAllocation = 10,
    WrDelayExecution = 11,
    WrSuspended = 12,
    WrUserRequest = 13,
    WrQueue = 14,
    WrLpcReceive = 15,
    WrLpcReply = 16,
    WrVirtualMemory = 17,
    WrPageOut = 18,
    WrRendezvous = 19,
    Spare2 = 20,
    Spare3 = 21,
    Spare4 = 22,
    Spare5 = 23,
    WrCalloutStack = 24,
    WrKernel = 25,
    WrResource = 26,
    WrPushLock = 27,
    WrMutex = 28,
    WrQuantumEnd = 29,
    WrDispatchInt = 30,
    WrPreempted = 31,
    WrYieldExecution = 32,
    WrFastMutex = 33,
    WrGuardedMutex = 34,
    WrRundown = 35,
    MaximumWaitReason = 36,
}

/// KWAIT_BLOCK - блок ожидания
#[repr(C)]
pub struct KWAIT_BLOCK {
    /// Список блоков ожидания в объекте
    pub wait_list_entry: LIST_ENTRY,
    /// Поток ожидающий объект
    pub thread: *mut KTHREAD,
    /// Объект ожидания
    pub object: PVOID,
    /// Следующий wait block для multiple wait
    pub next_wait_block: *mut KWAIT_BLOCK,
    /// Тип ожидания
    pub wait_type: u16,
    /// Индекс блока
    pub block_state: u8,
    /// Wait key
    pub wait_key: u16,
}

impl KWAIT_BLOCK {
    pub const fn new() -> Self {
        Self {
            wait_list_entry: LIST_ENTRY::new(),
            thread: core::ptr::null_mut(),
            object: core::ptr::null_mut(),
            next_wait_block: core::ptr::null_mut(),
            wait_type: 0,
            block_state: 0,
            wait_key: 0,
        }
    }
}

impl Default for KWAIT_BLOCK {
    fn default() -> Self {
        Self::new()
    }
}

/// Максимальное количество объектов для WaitForMultipleObjects
pub const MAXIMUM_WAIT_OBJECTS: usize = 64;

/// THREAD_WAIT_OBJECTS - встроенные wait blocks
pub const THREAD_WAIT_OBJECTS: usize = 3;

/// KAPC_STATE - состояние APC
#[repr(C)]
pub struct KAPC_STATE {
    pub apc_list_head: [LIST_ENTRY; 2],
    pub process: PVOID,
    pub kernel_apc_in_progress: UCHAR,
    pub kernel_apc_pending: UCHAR,
    pub user_apc_pending: UCHAR,
}

impl KAPC_STATE {
    pub const fn new() -> Self {
        Self {
            apc_list_head: [LIST_ENTRY::new(); 2],
            process: core::ptr::null_mut(),
            kernel_apc_in_progress: 0,
            kernel_apc_pending: 0,
            user_apc_pending: 0,
        }
    }
}

impl Default for KAPC_STATE {
    fn default() -> Self {
        Self::new()
    }
}

/// KTHREAD - структура потока ядра
#[repr(C)]
pub struct KTHREAD {
    /// Dispatcher header (для синхронизации)
    pub header: DISPATCHER_HEADER,

    // ========== Wait state ==========
    /// Список мьютексов, принадлежащих потоку
    pub mutant_list_head: LIST_ENTRY,

    /// Начальный адрес потока
    pub initial_stack: PVOID,
    /// Предел стека
    pub stack_limit: PVOID,
    /// Указатель на стек ядра
    pub kernel_stack: PVOID,

    /// Спинлок потока
    pub thread_lock: KSPIN_LOCK,

    /// Сохраненный APC state
    pub apc_state: KAPC_STATE,

    // ========== Scheduling ==========
    /// Текущее состояние
    pub state: AtomicU8,
    /// Предыдущий режим (kernel/user)
    pub previous_mode: UCHAR,
    /// Alertable flag
    pub alertable: UCHAR,
    /// Basepriority saturation
    pub base_priority_saturation: UCHAR,

    /// Статус ожидания
    pub wait_status: AtomicI32,

    /// Wait blocks
    pub wait_block_list: *mut KWAIT_BLOCK,
    /// Встроенные wait blocks
    pub wait_block: [KWAIT_BLOCK; THREAD_WAIT_OBJECTS + 1],

    /// DPC для quantum end
    pub quantum_dpc: *mut KDPC,

    /// Quantum (в тиках)
    pub quantum: i8,
    /// Тип планирования
    pub scheduling_type: UCHAR,
    /// Wait IRQL
    pub wait_irql: UCHAR,
    /// Wait mode
    pub wait_mode: UCHAR,

    /// Текущий wait reason
    pub wait_reason: UCHAR,

    /// Wait time
    pub wait_time: ULONG,

    // ========== Priority ==========
    /// Базовый приоритет
    pub base_priority: i8,
    /// Текущий приоритет
    pub priority: i8,

    /// Priority decrement (от boost)
    pub priority_decrement: i8,

    /// Saturation
    pub saturation: UCHAR,

    // ========== Priority Boost/Adjust ==========
    /// Причина корректировки приоритета (при добавлении в ready queue)
    pub adjust_reason: KADJUST_REASON,

    /// Величина boost для adjust
    pub adjust_increment: i8,

    /// Disable boost flag
    pub disable_boost: u8,

    /// Quantum reset value (восстанавливается при истечении quantum)
    pub quantum_reset: i8,

    // ========== Preemption ==========
    /// Флаг что поток был вытеснен (для InsertHead vs InsertTail)
    pub preempted: u8,

    /// Idle swap block flag (для context switch)
    pub idle_swap_block: u8,

    // ========== APC ==========
    /// Kernel APC disable count
    pub kernel_apc_disable: i16,

    /// APC queue lock
    pub apc_queue_lock: AtomicU64,

    // ========== Context ==========
    /// TEB (Thread Environment Block)
    pub teb: PVOID,

    /// Kernel time (в тиках)
    pub kernel_time: ULONG,
    /// User time (в тиках)
    pub user_time: ULONG,

    // ========== Lists ==========
    /// Очередь готовности
    pub queue_list_entry: LIST_ENTRY,

    /// Wait list entry
    pub wait_list_entry: LIST_ENTRY,

    /// Thread list entry
    pub thread_list_entry: LIST_ENTRY,

    // ========== Timer ==========
    /// Таймер для DelayExecution и др.
    pub timer: KTIMER,
    /// Wait block для таймера
    pub timer_wait_block: KWAIT_BLOCK,

    // ========== Affinity ==========
    /// Affinity (маска процессоров)
    pub affinity: u64,

    /// Идеальный процессор
    pub ideal_processor: UCHAR,
    /// Номер процессора при последнем запуске
    pub next_processor: UCHAR,

    // ========== IDs ==========
    /// Thread ID
    pub thread_id: ULONG,
    /// Process ID
    pub process_id: ULONG,

    // ========== Process ==========
    /// Указатель на процесс
    pub process: PVOID, // PKPROCESS

    // ========== Counters ==========
    /// Количество переключений контекста
    pub context_switches: ULONG,

    // ========== Context Switch (ReactOS compatibility) ==========
    /// SwapBusy - флаг занятости при переключении контекста
    pub swap_busy: u8,

    /// NpxState - состояние FPU/SSE/AVX (0 для kernel mode, features для user mode)
    pub npx_state: u64,

    /// StateSaveArea - указатель на область сохранения XSAVE
    pub state_save_area: PVOID,

    /// SpecialApcDisable - счетчик отключения special APC
    pub special_apc_disable: i16,

    /// KernelStackResident - kernel stack находится в памяти (не swapped)
    /// В NT используется для tracking swapping состояния стека.
    /// 1 = резидентный, 0 = swapped out
    pub kernel_stack_resident: u8,

    /// SwapListEntry - используется для DeferredReadyList (single-list)
    /// (ReactOS: KTHREAD.SwapListEntry)
    pub swap_list_entry: SINGLE_LIST_ENTRY,
}

impl KTHREAD {
    pub const fn new() -> Self {
        Self {
            header: DISPATCHER_HEADER::new(KOBJECT_TYPE::Thread as UCHAR, 0),
            mutant_list_head: LIST_ENTRY::new(),
            initial_stack: core::ptr::null_mut(),
            stack_limit: core::ptr::null_mut(),
            kernel_stack: core::ptr::null_mut(),
            thread_lock: KSPIN_LOCK::new(),
            apc_state: KAPC_STATE::new(),
            state: AtomicU8::new(KTHREAD_STATE::Initialized as u8),
            previous_mode: 0,
            alertable: 0,
            base_priority_saturation: 0,
            wait_status: AtomicI32::new(0),
            wait_block_list: core::ptr::null_mut(),
            wait_block: [
                KWAIT_BLOCK::new(),
                KWAIT_BLOCK::new(),
                KWAIT_BLOCK::new(),
                KWAIT_BLOCK::new(),
            ],
            quantum_dpc: core::ptr::null_mut(),
            quantum: 0,
            scheduling_type: 0,
            wait_irql: 0,
            wait_mode: 0,
            wait_reason: 0,
            wait_time: 0,
            base_priority: 8,
            priority: 8,
            priority_decrement: 0,
            saturation: 0,
            adjust_reason: KADJUST_REASON::AdjustNone,
            adjust_increment: 0,
            disable_boost: 0,
            quantum_reset: 6, // Default quantum
            preempted: 0,
            idle_swap_block: 0,
            kernel_apc_disable: 0,
            apc_queue_lock: AtomicU64::new(0),
            teb: core::ptr::null_mut(),
            kernel_time: 0,
            user_time: 0,
            queue_list_entry: LIST_ENTRY::new(),
            wait_list_entry: LIST_ENTRY::new(),
            thread_list_entry: LIST_ENTRY::new(),
            timer: KTIMER::new(),
            timer_wait_block: KWAIT_BLOCK::new(),
            affinity: !0,
            ideal_processor: 0,
            next_processor: 0,
            thread_id: 0,
            process_id: 0,
            process: core::ptr::null_mut(),
            context_switches: 0,
            swap_busy: 0,
            npx_state: 0,
            state_save_area: core::ptr::null_mut(),
            special_apc_disable: 0,
            kernel_stack_resident: 1, // По умолчанию стек резидентный
            swap_list_entry: SINGLE_LIST_ENTRY::new(),
        }
    }

    /// Возвращает текущее состояние потока
    #[inline]
    pub fn get_state(&self) -> KTHREAD_STATE {
        let state = self.state.load(Ordering::Acquire);
        // Safety: state всегда валидный KTHREAD_STATE
        unsafe { core::mem::transmute(state) }
    }

    /// Устанавливает состояние потока с проверкой допустимости перехода
    #[inline]
    pub fn set_state(&self, new_state: KTHREAD_STATE) {
        self.state.store(new_state as u8, Ordering::Release);
    }
}

impl Default for KTHREAD {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// KeInitializeThread (ReactOS: ke/thrdobj.c:756-913)
// =============================================================================

/// KeInitializeThread - инициализация KTHREAD
///
/// Источник: ReactOS ke/thrdobj.c:891-913
///
/// # Параметры
/// - `process` - процесс-владелец
/// - `thread` - указатель на KTHREAD
/// - `system_routine` - системная routine (None для idle)
/// - `start_routine` - стартовая routine (None для idle)
/// - `start_context` - контекст (null для idle)
/// - `context` - user context (null для kernel threads)
/// - `teb` - TEB (null для kernel threads)
/// - `kernel_stack` - верхняя граница стека
/// - `stack_size` - размер стека
pub unsafe fn ke_initialize_thread(
    process: *mut crate::ps::process::KPROCESS,
    thread: *mut KTHREAD,
    _system_routine: Option<unsafe extern "win64" fn(PVOID)>,
    _start_routine: Option<unsafe extern "win64" fn(PVOID)>,
    _start_context: PVOID,
    _context: PVOID,
    _teb: PVOID,
    kernel_stack: PVOID,
    stack_size: usize,
) {
    unsafe {
        use core::sync::atomic::Ordering;

        use crate::ke::event::DISPATCHER_HEADER;
        use crate::ke::event::KOBJECT_TYPE;

        if thread.is_null() || process.is_null() {
            return;
        }

        // 1. Dispatcher header
        (*thread).header = DISPATCHER_HEADER::new(
            KOBJECT_TYPE::Thread as u8,
            (core::mem::size_of::<KTHREAD>() / 4) as u8,
        );
        (*thread).header.signal_state.store(0, Ordering::Release);
        LIST_ENTRY::init_head(&raw mut (*thread).header.wait_list_head);

        // 2. Lists
        LIST_ENTRY::init_head(&raw mut (*thread).mutant_list_head);
        LIST_ENTRY::init_head(&raw mut (*thread).apc_state.apc_list_head[0]);
        LIST_ENTRY::init_head(&raw mut (*thread).apc_state.apc_list_head[1]);

        // 3. Stack
        (*thread).initial_stack = kernel_stack;
        (*thread).stack_limit = (kernel_stack as usize - stack_size) as PVOID;
        (*thread).kernel_stack = kernel_stack;

        // 4. Process
        (*thread).apc_state.process = process as PVOID;
        (*thread).process = process as PVOID;

        // 5. Priority
        (*thread).priority = (*process).base_priority;
        (*thread).base_priority = (*process).base_priority;
        (*thread).priority_decrement = 0;
        (*thread).quantum = (*process).thread_quantum;
        (*thread).quantum_reset = (*process).quantum_reset;

        // 6. Affinity
        (*thread).affinity = (*process).active_processors.load(Ordering::Acquire) as u64;
        (*thread).ideal_processor = 0;
        (*thread).next_processor = 0;

        // 7. State = Initialized
        (*thread)
            .state
            .store(KTHREAD_STATE::Initialized as u8, Ordering::Release);

        // 8. Wait blocks
        for i in 0..4 {
            (*thread).wait_block[i].thread = thread;
        }
        (*thread).wait_block_list = core::ptr::null_mut();

        // 9. Остальные поля
        (*thread).wait_irql = 0;
        (*thread).wait_mode = 0;
        (*thread).wait_reason = 0;
        (*thread).wait_time = 0;
        (*thread).kernel_apc_disable = 0;
        (*thread).special_apc_disable = 0;
        (*thread).preempted = 0;
        (*thread).adjust_reason = KADJUST_REASON::AdjustNone;
        (*thread).adjust_increment = 0;

        // 10. TEB и прочее
        (*thread).teb = core::ptr::null_mut();
        (*thread).queue_list_entry = LIST_ENTRY::new();
        (*thread).thread_list_entry = LIST_ENTRY::new();

        // 11. IDs
        (*thread).thread_id = 0;
        (*thread).process_id = 0;

        // 12. Counters
        (*thread).context_switches = 0;
        (*thread).kernel_time = 0;
        (*thread).user_time = 0;
    }
}
