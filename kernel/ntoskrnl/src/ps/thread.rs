//! ETHREAD - Executive Thread
//!
//! Структура потока NT ядра.
//!
//! Источники:
//! - ReactOS: ps/thread.c, include/ndk/pstypes.h

use core::sync::atomic::AtomicI32;
use core::sync::atomic::AtomicPtr;
use core::sync::atomic::AtomicU32;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;

use super::process::EPROCESS;
use super::types::*;
use crate::ke::semaphore::KSEMAPHORE;
use crate::ke::thread::KTHREAD;
use crate::nt::LARGE_INTEGER;
use crate::nt::LIST_ENTRY;
use crate::nt::NTSTATUS;
use crate::nt::PVOID;
use crate::nt::STATUS_SUCCESS;

// =============================================================================
// ETHREAD - Executive Thread
// =============================================================================

/// ETHREAD - структура потока Executive уровня
///
/// Содержит KTHREAD как первое поле (layout как в NT), что позволяет
/// преобразовывать PKTHREAD <-> PETHREAD через pointer cast.
///
/// В NT 6.1 ETHREAD содержит ~700 байт полей для LPC, APC, impersonation,
/// IRP tracking и других подсистем. Текущая реализация содержит базовый набор.
#[repr(C)]
pub struct ETHREAD {
    /// Kernel thread (должен быть первым)
    pub tcb: KTHREAD,

    /// Время создания
    pub create_time: LARGE_INTEGER,

    /// Exit time / LPC reply chain
    pub exit_time: LARGE_INTEGER,

    /// Exit status
    pub exit_status: AtomicI32,

    /// Post block list
    pub post_block_list: LIST_ENTRY,

    /// Termination port
    pub termination_port: PVOID,

    /// Active timer list lock
    pub active_timer_list_lock: AtomicUsize,

    /// Active timer list head
    pub active_timer_list_head: LIST_ENTRY,

    /// Client ID
    pub cid: CLIENT_ID,

    /// LPC reply message / Keyed wait semaphore
    pub lpc_reply_semaphore: KSEMAPHORE,

    /// LPC reply message
    pub lpc_reply_message: PVOID,

    /// LPC reply message ID
    pub lpc_reply_message_id: u32,

    /// Impersonation info
    pub impersonation_info: PVOID,

    /// IRP list
    pub irp_list: LIST_ENTRY,

    /// Top level IRP
    pub top_level_irp: usize,

    /// Device to verify
    pub device_to_verify: PVOID,

    /// Thread's process
    pub thread_process: AtomicPtr<EPROCESS>,

    /// Start address
    pub start_address: PVOID,

    /// Win32 start address
    pub win32_start_address: PVOID,

    /// Thread list entry (in process)
    pub thread_list_entry: LIST_ENTRY,

    /// Run-down protect
    pub run_down_protect: AtomicU32,

    /// Thread lock
    pub thread_lock: AtomicUsize, // EX_PUSH_LOCK

    /// LPC receive message ID
    pub lpc_receive_message_id: u32,

    /// LPC waiting on port
    pub lpc_waiting_on_port: PVOID,

    /// Cross thread flags
    pub cross_thread_flags: AtomicU32,

    /// Same thread APC flags  
    pub same_thread_apc_flags: AtomicU32,

    /// Hard error disabled
    pub hard_errors_are_disabled: u8,

    /// Padding
    pub _reserved: [u8; 3],
}

impl ETHREAD {
    pub const fn new() -> Self {
        Self {
            tcb: KTHREAD::new(),
            create_time: LARGE_INTEGER::new(0),
            exit_time: LARGE_INTEGER::new(0),
            exit_status: AtomicI32::new(0x103), // STATUS_PENDING
            post_block_list: LIST_ENTRY::new(),
            termination_port: core::ptr::null_mut(),
            active_timer_list_lock: AtomicUsize::new(0),
            active_timer_list_head: LIST_ENTRY::new(),
            cid: CLIENT_ID::new(),
            lpc_reply_semaphore: KSEMAPHORE::new(),
            lpc_reply_message: core::ptr::null_mut(),
            lpc_reply_message_id: 0,
            impersonation_info: core::ptr::null_mut(),
            irp_list: LIST_ENTRY::new(),
            top_level_irp: 0,
            device_to_verify: core::ptr::null_mut(),
            thread_process: AtomicPtr::new(core::ptr::null_mut()),
            start_address: core::ptr::null_mut(),
            win32_start_address: core::ptr::null_mut(),
            thread_list_entry: LIST_ENTRY::new(),
            run_down_protect: AtomicU32::new(0),
            thread_lock: AtomicUsize::new(0),
            lpc_receive_message_id: 0,
            lpc_waiting_on_port: core::ptr::null_mut(),
            cross_thread_flags: AtomicU32::new(0),
            same_thread_apc_flags: AtomicU32::new(0),
            hard_errors_are_disabled: 0,
            _reserved: [0; 3],
        }
    }

    /// Возвращает Thread ID
    #[inline]
    pub fn thread_id(&self) -> usize {
        self.cid.thread_id()
    }

    /// Возвращает Process ID
    #[inline]
    pub fn process_id(&self) -> usize {
        self.cid.process_id()
    }

    /// Возвращает процесс потока
    #[inline]
    pub fn process(&self) -> *mut EPROCESS {
        self.thread_process.load(Ordering::Acquire)
    }

    /// Устанавливает процесс потока
    #[inline]
    pub fn set_process(&self, process: *mut EPROCESS) {
        self.thread_process.store(process, Ordering::Release);
    }

    /// Проверяет cross-thread флаг
    #[inline]
    pub fn has_cross_flag(&self, flag: u32) -> bool {
        (self.cross_thread_flags.load(Ordering::Acquire) & flag) != 0
    }

    /// Устанавливает cross-thread флаг
    #[inline]
    pub fn set_cross_flag(&self, flag: u32) {
        self.cross_thread_flags.fetch_or(flag, Ordering::AcqRel);
    }

    /// Проверяет, завершен ли поток
    #[inline]
    pub fn is_terminated(&self) -> bool {
        self.has_cross_flag(PS_CROSS_THREAD_FLAGS_TERMINATED)
    }

    /// Проверяет, системный ли это поток
    #[inline]
    pub fn is_system_thread(&self) -> bool {
        self.has_cross_flag(PS_CROSS_THREAD_FLAGS_SYSTEM)
    }
}

// =============================================================================
// Global Thread Variables
// =============================================================================

/// Текущий процессор ID (для per-CPU idle threads)
pub static PS_IDLE_THREAD: AtomicPtr<ETHREAD> = AtomicPtr::new(core::ptr::null_mut());

// =============================================================================
// Thread Functions
// =============================================================================

/// PsGetCurrentThreadId - возвращает ID текущего потока
///
/// Возвращает реальный Thread ID из ETHREAD.cid.unique_thread.
/// В NT этот ID используется для идентификации потока в CID table.
///
/// # Safety
/// Требует инициализированный PCR с текущим потоком
pub unsafe fn ps_get_current_thread_id() -> usize {
    unsafe {
        let thread = super::process::ps_get_current_thread();
        if thread.is_null() {
            return 0;
        }
        // thread указывает на ETHREAD, приводим и получаем TID из cid
        let ethread = thread as *const ETHREAD;
        (*ethread).cid.thread_id()
    }
}

/// PsGetCurrentProcessId - возвращает ID текущего процесса
pub unsafe fn ps_get_current_process_id() -> usize {
    unsafe {
        let process = super::process::ps_get_current_process();
        if process.is_null() {
            return 0;
        }
        (*process).process_id()
    }
}

/// PsGetThreadId - возвращает ID потока
pub fn ps_get_thread_id(thread: *const ETHREAD) -> usize {
    if thread.is_null() {
        return 0;
    }
    unsafe { (*thread).thread_id() }
}

/// PsGetThreadProcessId - возвращает Process ID потока
pub fn ps_get_thread_process_id(thread: *const ETHREAD) -> usize {
    if thread.is_null() {
        return 0;
    }
    unsafe { (*thread).process_id() }
}

/// PsGetThreadProcess - возвращает процесс потока
pub fn ps_get_thread_process(thread: *const ETHREAD) -> *mut EPROCESS {
    if thread.is_null() {
        return core::ptr::null_mut();
    }
    unsafe { (*thread).process() }
}

/// PsIsSystemThread - проверяет, системный ли поток
pub fn ps_is_system_thread(thread: *const ETHREAD) -> bool {
    if thread.is_null() {
        return false;
    }
    unsafe { (*thread).is_system_thread() }
}

/// PsIsThreadTerminating - проверяет, завершается ли поток
pub fn ps_is_thread_terminating(thread: *const ETHREAD) -> bool {
    if thread.is_null() {
        return true;
    }
    unsafe { (*thread).is_terminated() }
}

// =============================================================================
// Thread Creation
// =============================================================================

/// Размер стека для kernel thread.
///
/// NOTE: форматный debug-лог (scheduler trace) в Rust может потреблять десятки КБ стека.
/// Это приводит к overflow/почти-overflow и порче соседних аллокаций (часто рядом лежит ETHREAD/KTHREAD).
///
/// В debug сборках даём больше запас, в release можно оставить меньше.
#[cfg(debug_assertions)]
pub const KERNEL_STACK_SIZE: usize = 0x10000; // 64KB

#[cfg(not(debug_assertions))]
pub const KERNEL_STACK_SIZE: usize = 0x8000; // 32KB

/// PsCreateSystemThread - создание системного потока
///
/// # Arguments
/// * `thread_handle` - возвращает handle потока (может быть NULL)
/// * `desired_access` - желаемые права доступа
/// * `object_attributes` - атрибуты объекта (может быть NULL)
/// * `process_handle` - handle процесса (NULL = System Process)
/// * `client_id` - возвращает CLIENT_ID (может быть NULL)
/// * `start_routine` - функция потока
/// * `start_context` - параметр для функции
///
/// # Returns
/// NTSTATUS
pub unsafe fn ps_create_system_thread(
    thread_handle: *mut crate::nt::HANDLE,
    desired_access: u32,
    _object_attributes: PVOID,
    process_handle: crate::nt::HANDLE,
    client_id: *mut CLIENT_ID,
    start_routine: PKSTART_ROUTINE,
    start_context: PVOID,
) -> NTSTATUS {
    unsafe {
        use super::init::{PS_PROCESS_TYPE, PS_THREAD_TYPE};
        use crate::arch::x86_64::context::ki_initialize_context_thread;
        use crate::arch::x86_64::context::psp_system_thread_startup;
        use crate::ex::pool::POOL_TYPE;
        use crate::ex::pool::ex_allocate_pool_with_tag;
        use crate::ke::sched::KI_DISPATCHER_LOCK;
        use crate::ke::sched::ki_ready_thread;
        use crate::ke::spinlock::ke_acquire_spin_lock;
        use crate::ke::spinlock::ke_release_spin_lock;
        use crate::ob::life::{ob_create_object, ob_insert_object};
        use crate::ob::refcount::ob_reference_object_by_handle;

        // Определяем процесс
        let process = if process_handle == 0 || process_handle == -1isize as usize {
            // Используем System Process
            super::process::PS_INITIAL_SYSTEM_PROCESS.load(Ordering::Acquire)
        } else {
            // Получаем процесс по handle через Object Manager
            let mut process_ptr: PVOID = core::ptr::null_mut();
            let process_type = PS_PROCESS_TYPE.load(Ordering::Acquire);

            let status = ob_reference_object_by_handle(
                process_handle as PVOID,
                0, // PROCESS_CREATE_THREAD
                process_type,
                0, // KernelMode
                &mut process_ptr,
                core::ptr::null_mut(),
            );

            if status != STATUS_SUCCESS {
                return status;
            }

            process_ptr as *mut EPROCESS
        };

        if process.is_null() {
            return crate::nt::STATUS_PROCESS_IS_TERMINATING;
        }

        // Создаём объект ETHREAD через Object Manager
        // Это обеспечивает корректный OBJECT_HEADER с счётчиками
        let thread_type = PS_THREAD_TYPE.load(Ordering::Acquire);
        if thread_type.is_null() {
            return crate::nt::STATUS_OBJECT_TYPE_MISMATCH;
        }

        let mut thread_ptr: PVOID = core::ptr::null_mut();
        let status = ob_create_object(
            0, // KernelMode
            thread_type,
            core::ptr::null(), // ObjectAttributes
            0, // KernelMode
            core::ptr::null_mut(), // ParseContext
            core::mem::size_of::<ETHREAD>(),
            0, // PagedPoolCharge
            0, // NonPagedPoolCharge
            &mut thread_ptr,
        );

        if status != STATUS_SUCCESS {
            return status;
        }

        let thread = thread_ptr as *mut ETHREAD;

        // Инициализируем базовые поля
        *thread = ETHREAD::new();

        // Выделяем kernel stack
        let stack = ex_allocate_pool_with_tag(
            POOL_TYPE::NonPagedPool,
            KERNEL_STACK_SIZE,
            u32::from_le_bytes(*b"Kstk"),
        );

        if stack.is_null() {
            crate::ex::pool::ex_free_pool_with_tag(thread as PVOID, u32::from_le_bytes(*b"Thrd"));
            return crate::nt::STATUS_INSUFFICIENT_RESOURCES;
        }

        // Initial stack указывает на верх стека (стек растет вниз)
        let initial_stack = (stack as usize + KERNEL_STACK_SIZE) as PVOID;

        unsafe {
            // Инициализируем KTHREAD
            let kthread = &raw mut (*thread).tcb;

            // Stack
            (*kthread).initial_stack = initial_stack;
            (*kthread).stack_limit = stack;
            (*kthread).kernel_stack = initial_stack; // Будет обновлено ki_initialize_context_thread

            // Process
            (*kthread).process = &raw mut (*process).pcb as PVOID;
            // (*kthread).apc_state.process = &raw mut (*process).pcb as PVOID;

            // // Инициализируем APC списки (критически важно!)
            // LIST_ENTRY::init_head(&raw mut (*kthread).apc_state.apc_list_head[0]);
            // LIST_ENTRY::init_head(&raw mut (*kthread).apc_state.apc_list_head[1]);
            // LIST_ENTRY::init_head(&raw mut (*kthread).mutant_list_head);

            (*kthread).affinity = (*process).pcb.active_processors.load(Ordering::Acquire) as u64;
            if (*kthread).affinity == 0 {
                (*kthread).affinity = 1; // По умолчанию процессор 0
            }
            (*kthread).base_priority = (*process).pcb.base_priority;
            (*kthread).priority = (*process).pcb.base_priority;
            (*kthread).quantum = 6; // Default quantum
            (*kthread).quantum_reset = 6;

            // Ideal processor (для лучшего распределения нагрузки)
            (*kthread).ideal_processor = 0;
            (*kthread).next_processor = 0;

            // Состояние: KTHREAD::new() уже устанавливает Initialized
            // set_state не вызываем — это не переход состояния, а инициализация

            // Инициализируем таймер потока (критически важно для wait_list!)
            crate::ke::timer::ke_initialize_timer(&mut (*kthread).timer);

            // Client ID - используем единую функцию из ps/cid.rs
            let tid = super::cid::psp_allocate_thread_id();
            (*thread).cid.unique_process =
                (*process).unique_process_id.load(Ordering::Acquire) as PVOID;
            (*thread).cid.unique_thread = tid as PVOID;
            (*kthread).thread_id = tid as u32;
            (*kthread).process_id = (*thread).cid.unique_process as u32;

            // Thread process
            (*thread).thread_process.store(process, Ordering::Release);

            // Вставляем поток в CID table для корректной работы PsLookupThreadByThreadId
            let cid_status = super::cid::psp_create_thread_cid(thread);
            if cid_status != STATUS_SUCCESS {
                crate::kd_print!(
                    "[PS] Warning: failed to insert thread into CID table: 0x{:08X}\n",
                    cid_status
                );
            }

            // Start address
            (*thread).start_address = start_routine
                .map(|f| f as *const ())
                .unwrap_or(core::ptr::null()) as PVOID;

            // System thread flag
            (*thread).set_cross_flag(PS_CROSS_THREAD_FLAGS_SYSTEM);

            // Инициализируем контекст для переключения
            // При первом context switch на этот поток управление перейдет в ki_thread_startup,
            // который вызовет psp_system_thread_startup(start_routine, start_context)
            ki_initialize_context_thread(
                kthread,
                psp_system_thread_startup,
                start_routine,
                start_context,
            );

            // Добавляем в список потоков процесса
            let thread_list = &raw mut (*process).pcb.thread_list_head;
            LIST_ENTRY::insert_tail(thread_list, &raw mut (*kthread).thread_list_entry);

            // Возвращаем CLIENT_ID если запрошено
            if !client_id.is_null() {
                (*client_id).unique_process = (*thread).cid.unique_process;
                (*client_id).unique_thread = (*thread).cid.unique_thread;
            }

            // Создаём handle через Object Manager если запрошено
            if !thread_handle.is_null() {
                let mut handle: PVOID = core::ptr::null_mut();
                let status = ob_insert_object(
                    thread as PVOID,
                    core::ptr::null_mut(), // access_state
                    desired_access,
                    0,                     // object_pointer_bias
                    core::ptr::null_mut(), // new_object
                    &mut handle,
                );

                if status == STATUS_SUCCESS {
                    *thread_handle = handle as crate::nt::HANDLE;
                } else {
                    // Если не удалось создать handle, используем указатель как pseudo-handle
                    *thread_handle = thread as crate::nt::HANDLE;
                }
            }

            // =========================================================================
            // Добавляем поток в ready queue
            // =========================================================================
            // Захватываем dispatcher lock и добавляем поток в очередь готовых.
            // После этого планировщик сможет выбрать этот поток для выполнения.
            let irql = ke_acquire_spin_lock(&KI_DISPATCHER_LOCK);
            ki_ready_thread(kthread);
            ke_release_spin_lock(&KI_DISPATCHER_LOCK, irql);
        }

        STATUS_SUCCESS
    }
}

// NOTE: Thread ID выделяется через ps/cid.rs::psp_allocate_thread_id()
// Дублирующий NEXT_THREAD_ID удалён в рамках этапа 0.1 (нормализация PID/TID)

// =============================================================================
// Thread Creation Helpers
// =============================================================================

/// Тип стартовой рутины потока
/// Используем Windows x64 ABI для совместимости с NT
pub type PKSTART_ROUTINE = Option<unsafe extern "win64" fn(PVOID)>;

/// Инициализирует KTHREAD часть
pub unsafe fn ke_initialize_thread(
    thread: *mut KTHREAD,
    kernel_stack: PVOID,
    _system_routine: Option<unsafe extern "win64" fn(PVOID)>,
    _start_routine: PKSTART_ROUTINE,
    _start_context: PVOID,
    _context: PVOID,
    teb: PVOID,
    process: *mut EPROCESS,
) {
    unsafe {
        if thread.is_null() {
            return;
        }

        // Базовая инициализация
        (*thread).header.signal_state.store(0, Ordering::Release);

        // Устанавливаем stack
        (*thread).kernel_stack = kernel_stack;
        (*thread).initial_stack = kernel_stack;
        (*thread).stack_limit = (kernel_stack as usize).wrapping_sub(KERNEL_STACK_SIZE) as PVOID;

        // Устанавливаем процесс
        if !process.is_null() {
            (*thread).process = &raw mut (*process).pcb as PVOID;
            (*thread).affinity = (*process).pcb.active_processors.load(Ordering::Acquire) as u64;
            (*thread).base_priority = (*process).pcb.base_priority;
            (*thread).priority = (*process).pcb.base_priority;
        }

        // TEB
        (*thread).teb = teb;

        // Состояние: предполагается что thread уже инициализирован через KTHREAD::new()
        // с состоянием Initialized. Если нужен reset — использовать прямое присваивание.
    }
}

// =============================================================================
// Thread Termination
// =============================================================================

/// PsTerminateSystemThread - завершает текущий системный поток
///
/// Эта функция завершает текущий системный поток с указанным статусом.
/// Поток должен быть системным (созданным через PsCreateSystemThread).
/// Функция НЕ возвращает управление.
///
/// # Arguments
/// * `exit_status` - код завершения потока
///
/// # Returns
/// Никогда не возвращает управление (noreturn)
///
/// # Safety
/// - Должна вызываться только из системного потока
/// - Поток не должен удерживать блокировки
///
/// Источники:
/// - MSDN: PsTerminateSystemThread
/// - ReactOS: ps/kill.c
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn PsTerminateSystemThread(exit_status: crate::nt::NTSTATUS) -> ! {
    unsafe { psp_exit_thread(exit_status) }
}

/// NtTerminateThread - завершает поток по handle
///
/// Системный вызов для завершения потока. Если handle = NtCurrentThread (-2),
/// завершает текущий поток. Иначе завершает поток, на который указывает handle.
///
/// # Arguments
/// * `thread_handle` - handle потока (или NtCurrentThread для текущего)
/// * `exit_status` - код завершения потока
///
/// # Returns
/// * STATUS_SUCCESS - поток успешно завершён (для внешнего потока)
/// * Никогда не возвращает если thread_handle = NtCurrentThread
///
/// # Safety
/// - thread_handle должен быть валидным handle с правом THREAD_TERMINATE
/// - Для user-mode вызовов выполняется проверка доступа
///
/// Источники:
/// - MSDN: NtTerminateThread
/// - ReactOS: ps/kill.c
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn NtTerminateThread(
    thread_handle: crate::nt::HANDLE,
    exit_status: crate::nt::NTSTATUS,
) -> crate::nt::NTSTATUS {
    unsafe {
        use super::types::NT_CURRENT_THREAD;
        use crate::nt::STATUS_INVALID_HANDLE;
        use crate::nt::STATUS_SUCCESS;

        // Проверка на NtCurrentThread pseudo-handle
        if thread_handle as isize == NT_CURRENT_THREAD {
            // Завершаем текущий поток - не возвращаемся
            psp_exit_thread(exit_status);
            // Недостижимо, но компилятор требует возврат
        }

        // Для других handle нужно получить ETHREAD через ObReferenceObjectByHandle
        // TODO: Полная реализация с ObReferenceObjectByHandle
        //
        // Текущая минимальная реализация:
        // 1. Проверяем, является ли handle указателем на ETHREAD (kernel-only hack)
        // 2. Если да - помечаем поток для завершения

        // Пока только поддерживаем pseudo-handle и прямые указатели (kernel-mode)
        let thread_ptr = thread_handle as *mut ETHREAD;
        if thread_ptr.is_null() {
            return STATUS_INVALID_HANDLE;
        }

        // Проверяем что это не текущий поток (уже обработано выше)
        let current = super::process::ps_get_current_thread() as *mut ETHREAD;
        if thread_ptr == current {
            // Текущий поток - завершаем через psp_exit_thread
            psp_exit_thread(exit_status);
        }

        // Для внешнего потока: помечаем его для завершения
        // В полной реализации NT это вызывает APC для завершения в контексте потока
        crate::kd_print!(
            "[PS] NtTerminateThread: terminating external thread TID={} with status=0x{:08X}\n",
            (*thread_ptr).cid.thread_id(),
            exit_status
        );

        // Устанавливаем exit status и флаг terminated
        (*thread_ptr)
            .exit_status
            .store(exit_status as i32, core::sync::atomic::Ordering::Release);
        (*thread_ptr).set_cross_flag(super::types::PS_CROSS_THREAD_FLAGS_TERMINATED);

        // TODO: Полная реализация должна:
        // 1. Поставить APC в очередь потока для выполнения exit path
        // 2. Пробудить поток если он в ожидании
        // 3. Дождаться завершения или вернуть сразу (зависит от семантики)
        //
        // Пока просто помечаем - поток завершится при следующем возврате из ядра
        // или при проверке флага terminated

        STATUS_SUCCESS
    }
}

/// NtCreateThread - создаёт новый поток в указанном процессе
///
/// Минимальная реализация системного вызова для создания потока.
/// В текущей версии поддерживается только создание kernel-mode потоков.
///
/// # Arguments
/// * `thread_handle` - возвращает handle созданного потока
/// * `desired_access` - желаемые права доступа
/// * `object_attributes` - атрибуты объекта (может быть NULL)
/// * `process_handle` - handle процесса-владельца (NtCurrentProcess для текущего)
/// * `client_id` - возвращает CLIENT_ID (может быть NULL)
/// * `thread_context` - начальный контекст потока (CONTEXT)
/// * `initial_teb` - начальный TEB
/// * `create_suspended` - создать в приостановленном состоянии
///
/// # Returns
/// NTSTATUS код результата
///
/// Источники:
/// - MSDN: NtCreateThread
/// - ReactOS: ps/thread.c
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn NtCreateThread(
    thread_handle: *mut crate::nt::HANDLE,
    _desired_access: u32,
    _object_attributes: crate::nt::PVOID,
    process_handle: crate::nt::HANDLE,
    _client_id: *mut super::types::CLIENT_ID,
    _thread_context: crate::nt::PVOID, // PCONTEXT - пока не используем
    _initial_teb: crate::nt::PVOID,    // PINITIAL_TEB - пока не используем
    _create_suspended: u8,
) -> crate::nt::NTSTATUS {
    unsafe {
        use super::process::EPROCESS;
        use super::types::NT_CURRENT_PROCESS;
        use crate::nt::STATUS_INVALID_PARAMETER;
        use crate::nt::STATUS_NOT_IMPLEMENTED;

        // Текущая минимальная реализация:
        // - Поддерживаем только создание system threads
        // - thread_context и initial_teb игнорируются
        // - create_suspended не поддерживается

        crate::kd_print!(
            "[PS] NtCreateThread: creating thread in process handle=0x{:X}\n",
            process_handle
        );

        // Определяем процесс
        let process: *mut EPROCESS;
        if process_handle as isize == NT_CURRENT_PROCESS || process_handle == 0 {
            process = super::process::ps_get_current_process();
        } else {
            // TODO: ObReferenceObjectByHandle для получения процесса
            // Пока считаем handle прямым указателем
            process = process_handle as *mut EPROCESS;
        }

        if process.is_null() {
            return STATUS_INVALID_PARAMETER;
        }

        // Для минимальной реализации используем ps_create_system_thread
        // который создаёт kernel-mode поток
        //
        // TODO: Полная реализация должна:
        // 1. Создавать user-mode поток с указанным контекстом
        // 2. Поддерживать create_suspended
        // 3. Инициализировать TEB

        // Пока возвращаем NOT_IMPLEMENTED для user-mode вызовов
        // и только kernel-mode вызовы обрабатываются через PsCreateSystemThread

        if thread_handle.is_null() {
            return STATUS_INVALID_PARAMETER;
        }

        // NOTE: Это заглушка - полная реализация требует:
        // - Парсинг CONTEXT для начального состояния регистров
        // - Создание TEB в user space
        // - Поддержка suspended state

        crate::kd_print!(
            "[PS] NtCreateThread: full implementation pending, use PsCreateSystemThread for kernel threads\n"
        );

        STATUS_NOT_IMPLEMENTED
    }
}

/// psp_exit_thread - внутренняя функция завершения потока
///
/// PspExitThread (NT internal name)
///
/// Выполняет cleanup потока и переключает контекст на следующий поток.
/// Никогда не возвращает управление.
///
/// Порядок операций (упрощённый для текущей реализации):
/// 1. Помечаем поток как terminated
/// 2. Снимаем из scheduler списков (wait lists, timers, ready queues)
/// 3. Удаляем из CID table
/// 4. Уведомляем процесс (декремент active_threads)
/// 5. Переключаем контекст на следующий поток
///
/// # Safety
/// - Вызывается в контексте текущего потока
/// - Поток не должен быть в критической секции
pub unsafe fn psp_exit_thread(exit_status: crate::nt::NTSTATUS) -> ! {
    unsafe {
        use core::sync::atomic::Ordering;

        use crate::ke::sched::KI_DISPATCHER_LOCK;
        use crate::ke::sched::ki_swap_thread;
        use crate::ke::spinlock::ke_acquire_spin_lock;
        use crate::ke::thread::KTHREAD_STATE;

        // Получаем текущий поток
        let thread_ptr = super::process::ps_get_current_thread();
        if thread_ptr.is_null() {
            // Это критическая ошибка - не можем завершить несуществующий поток
            crate::ke::bugcheck::ke_bug_check_ex(
                crate::ke::bugcheck::bugcheck_codes::CRITICAL_PROCESS_DIED,
                0xDEAD0001, // Signature: psp_exit_thread called with null thread
                0,
                0,
                0,
            );
        }

        let thread = thread_ptr as *mut ETHREAD;
        let kthread = &raw mut (*thread).tcb;

        crate::kd_print!(
            "[PS] psp_exit_thread: TID={} exit_status=0x{:08X}\n",
            (*thread).cid.thread_id(),
            exit_status
        );

        // 1. Устанавливаем exit status
        (*thread)
            .exit_status
            .store(exit_status as i32, Ordering::Release);

        // 2. Помечаем поток как terminated (cross thread flag)
        (*thread).set_cross_flag(super::types::PS_CROSS_THREAD_FLAGS_TERMINATED);

        // 3. Удаляем из CID table
        let tid = (*thread).cid.thread_id();
        if tid != 0 {
            super::cid::psp_delete_thread_cid(tid);
        }

        // 4. Удаляем из списка потоков процесса
        let process = (*thread).thread_process.load(Ordering::Acquire);
        if !process.is_null() {
            let thread_list = &raw mut (*kthread).thread_list_entry;
            // Безопасное удаление из двусвязного списка
            if !(*thread_list).flink.is_null() && !(*thread_list).blink.is_null() {
                if (*thread_list).flink != thread_list as *mut _
                    && (*thread_list).blink != thread_list as *mut _
                {
                    (*(*thread_list).blink).flink = (*thread_list).flink;
                    (*(*thread_list).flink).blink = (*thread_list).blink;
                    (*thread_list).flink = core::ptr::null_mut();
                    (*thread_list).blink = core::ptr::null_mut();
                }
            }

            // Декремент active_threads
            let old_count = (*process).active_threads.fetch_sub(1, Ordering::AcqRel);

            // Если это был последний поток процесса - запускаем exit процесса
            if old_count == 1 {
                crate::kd_print!(
                    "[PS] psp_exit_thread: last thread in process PID={}, triggering process exit\n",
                    (*process).process_id()
                );
                super::process::psp_exit_process(process, exit_status);
            }
        }

        // 5. Захватываем dispatcher lock и помечаем поток как Terminated
        let _irql = ke_acquire_spin_lock(&KI_DISPATCHER_LOCK);

        // Устанавливаем состояние Terminated
        (*kthread).set_state(KTHREAD_STATE::Terminated);

        // 6. Снимаем из wait списков если был в ожидании
        // (текущий поток не может быть в wait списке, но очищаем для consistency)
        let wait_list = &raw mut (*kthread).wait_list_entry;
        if !(*wait_list).flink.is_null() && (*wait_list).flink != wait_list as *mut _ {
            (*(*wait_list).blink).flink = (*wait_list).flink;
            (*(*wait_list).flink).blink = (*wait_list).blink;
            (*wait_list).flink = core::ptr::null_mut();
            (*wait_list).blink = core::ptr::null_mut();
        }

        // 7. Сигнализируем объект-поток (для waiters)
        (*kthread).header.signal_state.store(1, Ordering::Release);
        // Пробуждаем потоки, ожидающие завершения этого потока
        crate::ke::sched::ki_wait_test(&mut (*kthread).header, 0);

        // 8. Переключаемся на следующий поток
        //
        // NT-подобный инвариант: поток после завершения не должен продолжать выполняться.
        // `ki_swap_thread` ожидает, что dispatcher lock захвачен на DISPATCH_LEVEL и
        // освободит его внутри (как часть “выхода из dispatcher”).
        crate::kd_print!("[PS] psp_exit_thread: calling ki_swap_thread (no return)\n");

        // ki_swap_thread не принимает аргументов, использует текущий IRQL из PCR.
        let _status = ki_swap_thread();

        // Если мы вернулись — это критическая ошибка планировщика/завершения потока.
        crate::ke::bugcheck::ke_bug_check_ex(
            crate::ke::bugcheck::bugcheck_codes::CRITICAL_PROCESS_DIED,
            0xDEAD0002, // Signature: ki_swap_thread returned in psp_exit_thread
            (*thread).cid.thread_id() as usize,
            exit_status as usize,
            _status as usize,
        );
    }
}
