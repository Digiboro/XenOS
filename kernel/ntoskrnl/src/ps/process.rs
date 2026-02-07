//! EPROCESS - Executive Process
//!
//! Структура процесса NT ядра.
//!
//! Источники:
//! - ReactOS: ps/process.c, include/ndk/pstypes.h

use core::sync::atomic::AtomicPtr;
use core::sync::atomic::AtomicU32;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;

use super::types::*;
use crate::ex::handle::HANDLE_TABLE;
use crate::ke::event::DISPATCHER_HEADER;
use crate::ke::event::KOBJECT_TYPE;
use crate::ke::mutex::EX_PUSH_LOCK;
use crate::ke::spinlock::KSPIN_LOCK;
use crate::ke::thread::KTHREAD;
use crate::nt::LARGE_INTEGER;
use crate::nt::LIST_ENTRY;
use crate::nt::PVOID;
use crate::nt::STATUS_SUCCESS;

// =============================================================================
// KPROCESS - Kernel Process
// =============================================================================

/// KPROCESS - kernel-mode часть процесса
#[repr(C)]
pub struct KPROCESS {
    /// Dispatcher header для ожидания
    pub header: DISPATCHER_HEADER,
    /// Список потоков в процессе (KTHREAD.ThreadListEntry)
    pub thread_list_head: LIST_ENTRY,
    /// Profile list
    pub profile_list_head: LIST_ENTRY,
    /// Directory table base (CR3)
    pub directory_table_base: u64,
    /// LDT descriptor (для WoW64)
    pub ldt_descriptor: [u8; 16],
    /// Int21 descriptor
    pub int21_descriptor: [u8; 16],
    /// IO permission map offset
    pub io_pm_offset: u16,
    /// Padding
    pub _reserved1: u16,
    /// Активные процессоры (affinity mask)
    pub active_processors: AtomicUsize,
    /// Kernel time
    pub kernel_time: AtomicU32,
    /// User time
    pub user_time: AtomicU32,
    /// Ready list head
    pub ready_list_head: LIST_ENTRY,
    /// Swap list entry
    pub swap_list_entry: LIST_ENTRY,
    /// Stack count
    pub stack_count: AtomicU32,
    /// Thread quantum
    pub thread_quantum: i8,
    /// Base priority
    pub base_priority: i8,
    /// State
    pub state: u8,
    /// Thread seed
    pub thread_seed: u8,
    /// Disable boost
    pub disable_boost: u8,
    /// Disable quantum
    pub disable_quantum: u8,
    /// Ideal node
    pub ideal_node: u8,
    /// Flags
    pub flags: u8,
    /// Foreground process flag (0 = background, 1 = foreground)
    /// Foreground процессы получают увеличенный quantum
    pub foreground: u8,
    /// Quantum reset value (quantum восстанавливается до этого значения)
    /// Для foreground = PspForegroundQuantum[2], для background = PspForegroundQuantum[0]
    pub quantum_reset: i8,
    /// Foreground boost - дополнительный boost для foreground процессов
    /// Применяется к потокам при пробуждении из ожидания
    pub foreground_boost: i8,
    /// Padding для выравнивания
    pub _process_pad: u8,
}

/// PsPrioritySeparation (минимальная база).
///
/// В NT это глобальная настройка (обычно 0..2/3), влияющая на boost потоков
/// foreground процессов при пробуждении из ожиданий.
///
/// На текущем этапе держим фиксированное значение, ориентируясь на ReactOS
/// (типично = 2).
pub const PS_PRIORITY_SEPARATION: i8 = 2;

impl KPROCESS {
    pub const fn new() -> Self {
        Self {
            header: DISPATCHER_HEADER::new(KOBJECT_TYPE::Process as u8, 0),
            thread_list_head: LIST_ENTRY::new(),
            profile_list_head: LIST_ENTRY::new(),
            directory_table_base: 0,
            ldt_descriptor: [0; 16],
            int21_descriptor: [0; 16],
            io_pm_offset: 0,
            _reserved1: 0,
            active_processors: AtomicUsize::new(0),
            kernel_time: AtomicU32::new(0),
            user_time: AtomicU32::new(0),
            ready_list_head: LIST_ENTRY::new(),
            swap_list_entry: LIST_ENTRY::new(),
            stack_count: AtomicU32::new(0),
            thread_quantum: 6, // Default quantum (PspForegroundQuantum[0])
            base_priority: 8,  // Normal priority
            state: 0,
            thread_seed: 0,
            disable_boost: 0,
            disable_quantum: 0,
            ideal_node: 0,
            flags: 0,
            foreground: 0,       // Background by default
            quantum_reset: 6,    // Default quantum reset (background)
            foreground_boost: 0, // No foreground boost by default
            _process_pad: 0,
        }
    }
}

// =============================================================================
// KPROCESS_STATE (ReactOS: include/ndk/ketypes.h)
// =============================================================================

/// KPROCESS_STATE - состояние процесса
///
/// Источник: ReactOS include/ndk/ketypes.h
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KPROCESS_STATE {
    ProcessInMemory = 0,
    ProcessOutOfMemory = 1,
    ProcessInTransition = 2,
}

// =============================================================================
// KeInitializeProcess (ReactOS: ke/procobj.c:115-190)
// =============================================================================

/// KeInitializeProcess - инициализация KPROCESS
///
/// Источник: ReactOS ke/procobj.c:115-190
///
/// # Параметры
/// - `process` - указатель на KPROCESS
/// - `priority` - базовый приоритет (0 для idle)
/// - `affinity` - маска процессоров (u64::MAX для всех CPU)
/// - `directory_table_base` - CR3 значение
/// - `enable` - AutoAlignment
pub unsafe fn ke_initialize_process(
    process: *mut KPROCESS,
    priority: i8,
    affinity: u64,
    directory_table_base: u64,
    enable: bool,
) {
    unsafe {
        if process.is_null() {
            return;
        }

        // 1. Dispatcher header
        (*process).header = DISPATCHER_HEADER::new(
            KOBJECT_TYPE::Process as u8,
            (core::mem::size_of::<KPROCESS>() / 4) as u8,
        );
        (*process).header.signal_state.store(0, Ordering::Release);
        LIST_ENTRY::init_head(&raw mut (*process).header.wait_list_head);

        // 2. Scheduler data
        (*process).active_processors.store(0, Ordering::Release);
        (*process).base_priority = priority;
        (*process).thread_quantum = 6; // Default quantum
        (*process).quantum_reset = 6; // Default, перезаписывается при необходимости
        (*process).directory_table_base = directory_table_base;

        // AutoAlignment флаг хранится в flags
        if enable {
            (*process).flags |= 0x01; // AutoAlignment bit
        }

        // 3. Affinity - храним в active_processors
        (*process)
            .active_processors
            .store(affinity as usize, Ordering::Release);

        // 4. Lists
        LIST_ENTRY::init_head(&raw mut (*process).thread_list_head);
        LIST_ENTRY::init_head(&raw mut (*process).profile_list_head);
        LIST_ENTRY::init_head(&raw mut (*process).ready_list_head);

        // 5. State = ProcessInMemory
        (*process).state = KPROCESS_STATE::ProcessInMemory as u8;

        // 6. Остальные поля
        (*process).kernel_time.store(0, Ordering::Release);
        (*process).user_time.store(0, Ordering::Release);
        (*process).stack_count.store(0, Ordering::Release);
        (*process).thread_seed = 0;
        (*process).disable_boost = 0;
        (*process).disable_quantum = 0;
        (*process).ideal_node = 0;
        (*process).foreground = 0;
        (*process).foreground_boost = 0;
    }
}

// =============================================================================
// Process Priority Mode (NT compatible)
// =============================================================================

/// PSPROCESSPRIORITYMODE - режим приоритета процесса
///
/// Используется в PsSetProcessPriorityByClass для установки
/// приоритета и quantum процесса.
///
/// Источник: ReactOS ntoskrnl/include/internal/ps_x.h
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PSPROCESSPRIORITYMODE {
    /// Background process - минимальный quantum
    PsProcessPriorityBackground = 0,
    /// Foreground process - увеличенный quantum (3x boost)
    PsProcessPriorityForeground = 1,
    /// SpinUp - при запуске процесса
    PsProcessPrioritySpinup = 2,
}

/// PsSetProcessPriorityByClass - устанавливает приоритет процесса по классу
///
/// Устанавливает базовый приоритет и quantum процесса на основе
/// его класса приоритета и режима (foreground/background).
///
/// # Параметры
/// - `process`: Указатель на EPROCESS
/// - `priority_mode`: Режим приоритета (Foreground/Background/Spinup)
///
/// # Источник
/// ReactOS ntoskrnl/ps/process.c PsSetProcessPriorityByClass
///
/// # Безопасность
/// Вызывающий должен гарантировать валидность указателя на процесс.
pub unsafe fn ps_set_process_priority_by_class(
    process: *mut EPROCESS,
    priority_mode: PSPROCESSPRIORITYMODE,
) {
    unsafe {
        use super::types::PROCESS_PRIORITY_CLASS_IDLE;
        use crate::ke::trap::PSP_FOREGROUND_QUANTUM;

        if process.is_null() {
            return;
        }

        let pcb = &mut (*process).pcb;

        // Определяем quantum index на основе режима
        // Index 0 = Background (quantum 6)
        // Index 1 = Normal/Spinup (quantum 12)
        // Index 2 = Foreground (quantum 18)
        let quantum_index = match priority_mode {
            PSPROCESSPRIORITYMODE::PsProcessPriorityBackground => 0,
            PSPROCESSPRIORITYMODE::PsProcessPriorityForeground => 2,
            PSPROCESSPRIORITYMODE::PsProcessPrioritySpinup => 1,
        };

        // Устанавливаем foreground флаг
        pcb.foreground = if priority_mode == PSPROCESSPRIORITYMODE::PsProcessPriorityForeground {
            1
        } else {
            0
        };

        // Foreground boost (priority separation) для AdjustUnwait.
        pcb.foreground_boost = if pcb.foreground != 0 {
            PS_PRIORITY_SEPARATION
        } else {
            0
        };

        // Устанавливаем quantum из таблицы PspForegroundQuantum
        // Idle процессы всегда получают минимальный quantum
        if (*process).priority_class == PROCESS_PRIORITY_CLASS_IDLE {
            pcb.quantum_reset = PSP_FOREGROUND_QUANTUM[0]; // 6
            pcb.thread_quantum = PSP_FOREGROUND_QUANTUM[0];
            pcb.foreground_boost = 0;
        } else {
            pcb.quantum_reset = PSP_FOREGROUND_QUANTUM[quantum_index];
            pcb.thread_quantum = PSP_FOREGROUND_QUANTUM[quantum_index];
        }
    }
}

// =============================================================================
// EPROCESS - Executive Process
// =============================================================================

/// EPROCESS - полная структура процесса
#[repr(C)]
pub struct EPROCESS {
    /// Kernel process (должен быть первым)
    pub pcb: KPROCESS,

    /// Process lock
    pub process_lock: EX_PUSH_LOCK,

    /// Время создания
    pub create_time: LARGE_INTEGER,
    /// Время выхода
    pub exit_time: LARGE_INTEGER,

    /// Run-down protect
    pub run_down_protect: AtomicU32,

    /// Unique process ID
    pub unique_process_id: AtomicUsize,

    /// Active process links
    pub active_process_links: LIST_ENTRY,

    /// Quota usage
    pub quota_usage: [usize; 3],
    /// Quota peak
    pub quota_peak: [usize; 3],
    /// Commit charge
    pub commit_charge: AtomicUsize,
    /// Peak virtual size
    pub peak_virtual_size: usize,
    /// Virtual size
    pub virtual_size: AtomicUsize,

    /// Session process links
    pub session_process_links: LIST_ENTRY,

    /// Debug port
    pub debug_port: PVOID,
    /// Exception port
    pub exception_port: PVOID,

    /// Object table (handles)
    pub object_table: AtomicPtr<HANDLE_TABLE>,

    /// Token
    pub token: AtomicPtr<core::ffi::c_void>, // EX_FAST_REF

    /// Working set page
    pub working_set_page: usize,

    /// Address creation lock
    pub address_creation_lock: EX_PUSH_LOCK,

    /// Hyper space lock
    pub hyper_space_lock: KSPIN_LOCK,

    /// Fork in progress (указатель на ETHREAD, приводится при использовании)
    pub fork_in_progress: AtomicPtr<core::ffi::c_void>,

    /// Hardware trigger
    pub hardware_trigger: u32,

    /// Physical VAD root
    pub phys_vad_root: PVOID, // MM_AVL_TABLE

    /// Clone root
    pub clone_root: PVOID,

    /// Number of private pages
    pub number_of_private_pages: AtomicUsize,
    /// Number of locked pages
    pub number_of_locked_pages: AtomicUsize,

    /// Win32 process
    pub win32_process: PVOID,

    /// Job
    pub job: PVOID,

    /// Section object
    pub section_object: PVOID,
    /// Section base address
    pub section_base_address: PVOID,

    /// Cookie
    pub cookie: AtomicU32,

    /// Working set watch
    pub working_set_watch: PVOID,

    /// Win32 window station
    pub win32_window_station: PVOID,

    /// Inherited from unique process id
    pub inherited_from_unique_process_id: usize,

    /// LDT information
    pub ldt_information: PVOID,

    /// VAD free hint
    pub vad_free_hint: PVOID,
    /// VDM objects
    pub vdm_objects: PVOID,

    /// Device map
    pub device_map: PVOID,

    /// Session ID
    pub session_id: u32,

    /// PEB
    pub peb: PVOID,

    /// Prefetch trace
    pub prefetch_trace: PVOID,

    /// Read operation count
    pub read_operation_count: LARGE_INTEGER,
    /// Write operation count
    pub write_operation_count: LARGE_INTEGER,
    /// Other operation count
    pub other_operation_count: LARGE_INTEGER,
    /// Read transfer count
    pub read_transfer_count: LARGE_INTEGER,
    /// Write transfer count
    pub write_transfer_count: LARGE_INTEGER,
    /// Other transfer count
    pub other_transfer_count: LARGE_INTEGER,

    /// Commit charge limit
    pub commit_charge_limit: usize,
    /// Commit charge peak
    pub commit_charge_peak: AtomicUsize,

    /// AweInfo
    pub awe_info: PVOID,

    /// SE audit process creation info
    pub se_audit_process_creation_info: PVOID,

    /// VAD root
    pub vad_root: PVOID, // MM_AVL_TABLE

    /// Thread list head
    pub thread_list_head: LIST_ENTRY,

    /// Active threads
    pub active_threads: AtomicU32,

    /// Image file pointer
    pub image_file_pointer: PVOID,

    /// Image file name
    pub image_file_name: [u8; 16],

    /// Lock count
    pub lock_count: AtomicU32,

    /// Process flags
    pub flags: AtomicU32,

    /// Exit status
    pub exit_status: AtomicU32,

    /// Priority class
    pub priority_class: u8,

    /// Padding
    pub _reserved: [u8; 3],

    /// SubSystem data
    pub sub_system_data: PVOID,

    /// Locked pages list
    pub locked_pages_list: PVOID,

    /// Security port
    pub security_port: PVOID,

    /// Wow64 process
    pub wow64_process: PVOID,
}

impl EPROCESS {
    pub const fn new() -> Self {
        Self {
            pcb: KPROCESS::new(),
            process_lock: EX_PUSH_LOCK::new(),
            create_time: LARGE_INTEGER::new(0),
            exit_time: LARGE_INTEGER::new(0),
            run_down_protect: AtomicU32::new(0),
            unique_process_id: AtomicUsize::new(0),
            active_process_links: LIST_ENTRY::new(),
            quota_usage: [0; 3],
            quota_peak: [0; 3],
            commit_charge: AtomicUsize::new(0),
            peak_virtual_size: 0,
            virtual_size: AtomicUsize::new(0),
            session_process_links: LIST_ENTRY::new(),
            debug_port: core::ptr::null_mut(),
            exception_port: core::ptr::null_mut(),
            object_table: AtomicPtr::new(core::ptr::null_mut()),
            token: AtomicPtr::new(core::ptr::null_mut()),
            working_set_page: 0,
            address_creation_lock: EX_PUSH_LOCK::new(),
            hyper_space_lock: KSPIN_LOCK::new(),
            fork_in_progress: AtomicPtr::new(core::ptr::null_mut()),
            hardware_trigger: 0,
            phys_vad_root: core::ptr::null_mut(),
            clone_root: core::ptr::null_mut(),
            number_of_private_pages: AtomicUsize::new(0),
            number_of_locked_pages: AtomicUsize::new(0),
            win32_process: core::ptr::null_mut(),
            job: core::ptr::null_mut(),
            section_object: core::ptr::null_mut(),
            section_base_address: core::ptr::null_mut(),
            cookie: AtomicU32::new(0),
            working_set_watch: core::ptr::null_mut(),
            win32_window_station: core::ptr::null_mut(),
            inherited_from_unique_process_id: 0,
            ldt_information: core::ptr::null_mut(),
            vad_free_hint: core::ptr::null_mut(),
            vdm_objects: core::ptr::null_mut(),
            device_map: core::ptr::null_mut(),
            session_id: 0,
            peb: core::ptr::null_mut(),
            prefetch_trace: core::ptr::null_mut(),
            read_operation_count: LARGE_INTEGER::new(0),
            write_operation_count: LARGE_INTEGER::new(0),
            other_operation_count: LARGE_INTEGER::new(0),
            read_transfer_count: LARGE_INTEGER::new(0),
            write_transfer_count: LARGE_INTEGER::new(0),
            other_transfer_count: LARGE_INTEGER::new(0),
            commit_charge_limit: 0,
            commit_charge_peak: AtomicUsize::new(0),
            awe_info: core::ptr::null_mut(),
            se_audit_process_creation_info: core::ptr::null_mut(),
            vad_root: core::ptr::null_mut(),
            thread_list_head: LIST_ENTRY::new(),
            active_threads: AtomicU32::new(0),
            image_file_pointer: core::ptr::null_mut(),
            image_file_name: [0; 16],
            lock_count: AtomicU32::new(0),
            flags: AtomicU32::new(0),
            exit_status: AtomicU32::new(0x103), // STATUS_PENDING
            priority_class: PROCESS_PRIORITY_CLASS_NORMAL,
            _reserved: [0; 3],
            sub_system_data: core::ptr::null_mut(),
            locked_pages_list: core::ptr::null_mut(),
            security_port: core::ptr::null_mut(),
            wow64_process: core::ptr::null_mut(),
        }
    }

    /// Возвращает Process ID
    #[inline]
    pub fn process_id(&self) -> usize {
        self.unique_process_id.load(Ordering::Acquire)
    }

    /// Устанавливает Process ID
    #[inline]
    pub fn set_process_id(&self, id: usize) {
        self.unique_process_id.store(id, Ordering::Release);
    }

    /// Возвращает количество активных потоков
    #[inline]
    pub fn active_thread_count(&self) -> u32 {
        self.active_threads.load(Ordering::Acquire)
    }

    /// Проверяет флаг
    #[inline]
    pub fn has_flag(&self, flag: u32) -> bool {
        (self.flags.load(Ordering::Acquire) & flag) != 0
    }

    /// Устанавливает флаг
    #[inline]
    pub fn set_flag(&self, flag: u32) {
        self.flags.fetch_or(flag, Ordering::AcqRel);
    }

    /// Снимает флаг
    #[inline]
    pub fn clear_flag(&self, flag: u32) {
        self.flags.fetch_and(!flag, Ordering::AcqRel);
    }

    /// Устанавливает имя образа
    pub fn set_image_name(&mut self, name: &[u8]) {
        let len = core::cmp::min(name.len(), 15);
        self.image_file_name[..len].copy_from_slice(&name[..len]);
        self.image_file_name[len] = 0;
    }

    /// Возвращает имя образа как строку
    pub fn image_name(&self) -> &[u8] {
        let end = self
            .image_file_name
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(16);
        &self.image_file_name[..end]
    }
}

// =============================================================================
// Global Process Variables
// =============================================================================

// NOTE: ETHREAD определён в ps/thread.rs (единственное место)
// Используйте super::thread::ETHREAD или crate::ps::ETHREAD

use core::cell::UnsafeCell;

#[repr(transparent)]
pub struct SyncUnsafeCell<T>(UnsafeCell<T>);
unsafe impl<T> Sync for SyncUnsafeCell<T> {}

impl<T> SyncUnsafeCell<T> {
    pub const fn new(value: T) -> Self {
        Self(UnsafeCell::new(value))
    }

    #[inline]
    pub fn get(&self) -> *mut T {
        self.0.get()
    }
}

/// Initial System Process
pub static PS_INITIAL_SYSTEM_PROCESS: AtomicPtr<EPROCESS> = AtomicPtr::new(core::ptr::null_mut());

/// Idle Process
pub static PS_IDLE_PROCESS: AtomicPtr<EPROCESS> = AtomicPtr::new(core::ptr::null_mut());

/// Active process list head
pub static PS_ACTIVE_PROCESS_HEAD: SyncUnsafeCell<LIST_ENTRY> =
    SyncUnsafeCell::new(LIST_ENTRY::new());

/// Active process lock
pub static PSP_ACTIVE_PROCESS_MUTEX: SyncUnsafeCell<EX_PUSH_LOCK> =
    SyncUnsafeCell::new(EX_PUSH_LOCK::new());

// =============================================================================
// Process Functions
// =============================================================================

/// Возвращает System Process
#[inline]
pub fn ps_get_initial_system_process() -> *mut EPROCESS {
    PS_INITIAL_SYSTEM_PROCESS.load(Ordering::Acquire)
}

/// Возвращает Idle Process
#[inline]
pub fn ps_idle_process() -> *mut EPROCESS {
    PS_IDLE_PROCESS.load(Ordering::Acquire)
}

/// PsGetCurrentProcess - возвращает текущий процесс
///
/// # Safety
/// Требует инициализированный PCR с текущим потоком
#[inline]
pub unsafe fn ps_get_current_process() -> *mut EPROCESS {
    unsafe {
        let thread = ps_get_current_thread();
        if thread.is_null() {
            return ps_get_initial_system_process();
        }
        // thread указывает на ETHREAD, первое поле которого - KTHREAD (tcb).
        // KTHREAD.process указывает на KPROCESS, который является первым полем EPROCESS.
        // Поэтому (KTHREAD.process as *mut EPROCESS) корректно.
        let kthread = thread as *mut KTHREAD;
        (*kthread).process as *mut EPROCESS
    }
}

/// PsGetCurrentThread - возвращает текущий поток
///
/// Возвращает указатель на ETHREAD (определён в ps/thread.rs).
/// Layout ETHREAD: первое поле - KTHREAD, поэтому адрес ETHREAD == адрес KTHREAD.
///
/// # Safety
/// Требует инициализированный PCR
#[inline]
pub unsafe fn ps_get_current_thread() -> PVOID {
    unsafe {
        use crate::arch::x86_64::pcr;
        let pcr = pcr::get_pcr();
        if pcr.is_null() {
            return core::ptr::null_mut();
        }
        // current_thread хранит PKTHREAD, но благодаря layout ETHREAD (tcb первым полем)
        // это одновременно и указатель на ETHREAD
        (*pcr).prcb.current_thread
    }
}

/// PsGetProcessId - возвращает ID процесса
#[inline]
pub fn ps_get_process_id(process: *const EPROCESS) -> usize {
    if process.is_null() {
        return 0;
    }
    unsafe { (*process).unique_process_id.load(Ordering::Acquire) }
}

/// PsGetProcessImageFileName - возвращает имя образа
pub fn ps_get_process_image_file_name(process: *const EPROCESS) -> &'static [u8] {
    if process.is_null() {
        return b"Unknown";
    }
    unsafe { (*process).image_name() }
}

/// Проверяет, является ли это системным процессом
#[inline]
pub fn ps_is_system_process(process: *const EPROCESS) -> bool {
    process == ps_get_initial_system_process()
}

// =============================================================================
// Process Creation
// =============================================================================

/// Параметры создания процесса
#[derive(Default)]
pub struct PSP_CREATE_PROCESS_PARAMS {
    /// Родительский процесс (NULL для kernel processes)
    pub parent_process: *mut EPROCESS,
    /// Имя образа (до 15 символов)
    pub image_name: [u8; 16],
    /// Класс приоритета
    pub priority_class: u8,
    /// Базовый приоритет
    pub base_priority: i8,
    /// Наследовать handles от родителя
    pub inherit_handles: bool,
}

impl PSP_CREATE_PROCESS_PARAMS {
    pub const fn new() -> Self {
        Self {
            parent_process: core::ptr::null_mut(),
            image_name: [0; 16],
            priority_class: super::types::PROCESS_PRIORITY_CLASS_NORMAL,
            base_priority: 8,
            inherit_handles: false,
        }
    }

    /// Устанавливает имя образа
    pub fn set_image_name(&mut self, name: &[u8]) {
        let len = core::cmp::min(name.len(), 15);
        self.image_name[..len].copy_from_slice(&name[..len]);
        self.image_name[len] = 0;
    }
}

/// psp_create_process - создаёт новый процесс
///
/// PspCreateProcess (NT internal name)
///
/// Создаёт EPROCESS, инициализирует базовые структуры и выделяет PID.
/// НЕ добавляет процесс в active list и CID table - это делает psp_insert_process.
///
/// # Arguments
/// * `params` - параметры создания процесса
/// * `process_out` - возвращает указатель на созданный EPROCESS
///
/// # Returns
/// NTSTATUS код результата
///
/// # Safety
/// Вызывающий должен гарантировать валидность parent_process (если не NULL)
pub unsafe fn psp_create_process(
    params: &PSP_CREATE_PROCESS_PARAMS,
    process_out: *mut *mut EPROCESS,
) -> crate::nt::NTSTATUS {
    unsafe {
        use core::sync::atomic::Ordering;

        use crate::ex::pool::POOL_TYPE;
        use crate::ex::pool::ex_allocate_pool_with_tag;
        use crate::nt::LIST_ENTRY;
        use crate::nt::PVOID;
        use crate::nt::STATUS_INSUFFICIENT_RESOURCES;
        use crate::nt::STATUS_SUCCESS;

        if process_out.is_null() {
            return crate::nt::STATUS_INVALID_PARAMETER;
        }

        // 1. Выделяем память для EPROCESS
        let process = ex_allocate_pool_with_tag(
            POOL_TYPE::NonPagedPool,
            core::mem::size_of::<EPROCESS>(),
            u32::from_le_bytes(*b"Proc"),
        ) as *mut EPROCESS;

        if process.is_null() {
            return STATUS_INSUFFICIENT_RESOURCES;
        }

        // 2. Инициализируем нулями и базовыми значениями
        core::ptr::write_bytes(process, 0, 1);
        *process = EPROCESS::new();

        // 3. Выделяем PID
        let pid = super::cid::psp_allocate_process_id();
        (*process).set_process_id(pid);

        // 4. Устанавливаем имя образа
        if params.image_name[0] != 0 {
            (*process)
                .image_file_name
                .copy_from_slice(&params.image_name);
        }

        // 5. Устанавливаем приоритет
        (*process).priority_class = params.priority_class;
        (*process).pcb.base_priority = params.base_priority;

        // 6. Инициализируем списки
        LIST_ENTRY::init_head(&raw mut (*process).pcb.thread_list_head);
        LIST_ENTRY::init_head(&raw mut (*process).thread_list_head);
        LIST_ENTRY::init_head(&raw mut (*process).active_process_links);

        // 7. Создаём handle table
        if let Some(handle_table) = crate::ex::handle::ex_create_handle_table(process as PVOID) {
            (*process)
                .object_table
                .store(handle_table, Ordering::Release);
        } else {
            // Не удалось создать handle table - освобождаем процесс
            crate::ex::pool::ex_free_pool_with_tag(process as PVOID, u32::from_le_bytes(*b"Proc"));
            return STATUS_INSUFFICIENT_RESOURCES;
        }

        // 8. Наследуем handles если есть родитель и флаг установлен
        if !params.parent_process.is_null() && params.inherit_handles {
            let status = super::init::psp_inherit_handles(process, params.parent_process);
            if status != STATUS_SUCCESS {
                crate::kd_print!(
                    "[PS] psp_create_process: psp_inherit_handles failed: 0x{:08X}\n",
                    status
                );
                // Продолжаем даже при ошибке наследования
            }

            // Запоминаем родительский PID
            (*process).inherited_from_unique_process_id = (*params.parent_process).process_id();
        }

        // 9. Устанавливаем время создания
        (*process).create_time =
            crate::nt::LARGE_INTEGER::new(crate::ke::time::ke_query_system_time() as i64);

        *process_out = process;
        STATUS_SUCCESS
    }
}

/// psp_insert_process - вставляет процесс в системные списки
///
/// PspInsertProcess (NT internal name)
///
/// Добавляет процесс в:
/// - Active process list (PS_ACTIVE_PROCESS_HEAD)
/// - CID table (PSP_CID_TABLE)
///
/// # Arguments
/// * `process` - указатель на EPROCESS
///
/// # Returns
/// NTSTATUS код результата
///
/// # Safety
/// - Процесс должен быть создан через psp_create_process
/// - Вызывать только один раз для каждого процесса
pub unsafe fn psp_insert_process(process: *mut EPROCESS) -> crate::nt::NTSTATUS {
    unsafe {
        use crate::nt::LIST_ENTRY;
        use crate::nt::STATUS_INVALID_PARAMETER;

        if process.is_null() {
            return STATUS_INVALID_PARAMETER;
        }

        // 1. Добавляем в active process list
        // TODO: Использовать PSP_ACTIVE_PROCESS_MUTEX для синхронизации
        let head = PS_ACTIVE_PROCESS_HEAD.get();

        // Insert at tail (новые процессы в конец списка)
        let links = &raw mut (*process).active_process_links;
        (*links).blink = (*head).blink;
        (*links).flink = head as *mut LIST_ENTRY;

        if !(*head).blink.is_null() {
            (*(*head).blink).flink = links;
        }
        (*head).blink = links;

        // Если список был пуст, устанавливаем flink
        if (*head).flink.is_null() || (*head).flink == head as *mut LIST_ENTRY {
            (*head).flink = links;
        }

        // 2. Вставляем в CID table
        let cid_status = super::cid::psp_create_process_cid(process);
        if cid_status != STATUS_SUCCESS {
            crate::kd_print!(
                "[PS] psp_insert_process: failed to insert into CID table: 0x{:08X}\n",
                cid_status
            );
            // Продолжаем даже при ошибке CID - процесс всё равно функционален
        }

        crate::kd_print!(
            "[PS] psp_insert_process: PID={} inserted into active list\n",
            (*process).process_id()
        );

        STATUS_SUCCESS
    }
}

/// psp_remove_process - удаляет процесс из системных списков
///
/// Удаляет процесс из active process list и CID table.
/// Вызывается из psp_exit_process при завершении процесса.
pub unsafe fn psp_remove_process(process: *mut EPROCESS) {
    unsafe {
        if process.is_null() {
            return;
        }

        let pid = (*process).process_id();

        // 1. Удаляем из active process list
        let links = &raw mut (*process).active_process_links;
        if !(*links).flink.is_null() && !(*links).blink.is_null() {
            // Проверяем что процесс в списке (не сам на себя указывает)
            if (*links).flink != links && (*links).blink != links {
                (*(*links).blink).flink = (*links).flink;
                (*(*links).flink).blink = (*links).blink;
            }
            (*links).flink = core::ptr::null_mut();
            (*links).blink = core::ptr::null_mut();
        }

        // 2. Удаляем из CID table
        if pid != 0 {
            // Idle process не в CID
            super::cid::psp_delete_process_cid(pid);
        }

        crate::kd_print!("[PS] psp_remove_process: PID={} removed from lists\n", pid);
    }
}

// =============================================================================
// Process Termination
// =============================================================================

/// NtTerminateProcess - завершает процесс по handle
///
/// Системный вызов для завершения процесса. Если handle = NtCurrentProcess (-1),
/// завершает текущий процесс. Иначе завершает процесс, на который указывает handle.
///
/// # Arguments
/// * `process_handle` - handle процесса (или NtCurrentProcess для текущего)
/// * `exit_status` - код завершения процесса
///
/// # Returns
/// * STATUS_SUCCESS - процесс успешно завершён
/// * STATUS_INVALID_HANDLE - невалидный handle
///
/// # Safety
/// - process_handle должен быть валидным handle с правом PROCESS_TERMINATE
///
/// Источники:
/// - MSDN: NtTerminateProcess
/// - ReactOS: ps/kill.c
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn NtTerminateProcess(
    process_handle: crate::nt::HANDLE,
    exit_status: crate::nt::NTSTATUS,
) -> crate::nt::NTSTATUS {
    unsafe {
        use core::sync::atomic::Ordering;

        use super::types::NT_CURRENT_PROCESS;
        use crate::nt::STATUS_INVALID_HANDLE;

        let process: *mut EPROCESS;

        // Проверка на NtCurrentProcess pseudo-handle
        if process_handle as isize == NT_CURRENT_PROCESS {
            process = ps_get_current_process();
        } else {
            // Для других handle нужно получить EPROCESS через ObReferenceObjectByHandle
            // TODO: Полная реализация с ObReferenceObjectByHandle
            // Пока поддерживаем только прямые указатели (kernel-mode)
            process = process_handle as *mut EPROCESS;
        }

        if process.is_null() {
            return STATUS_INVALID_HANDLE;
        }

        // Нельзя завершить System Process или Idle Process
        if process == PS_INITIAL_SYSTEM_PROCESS.load(Ordering::Acquire)
            || process == PS_IDLE_PROCESS.load(Ordering::Acquire)
        {
            crate::kd_print!("[PS] NtTerminateProcess: cannot terminate system/idle process\n");
            return crate::nt::STATUS_ACCESS_DENIED;
        }

        crate::kd_print!(
            "[PS] NtTerminateProcess: terminating PID={} with status=0x{:08X}\n",
            (*process).process_id(),
            exit_status
        );

        // Вызываем внутреннюю функцию завершения
        psp_exit_process(process, exit_status)
    }
}

/// psp_exit_process - внутренняя функция завершения процесса
///
/// PspExitProcess (NT internal name)
///
/// Выполняет cleanup процесса:
/// 1. Помечает процесс как exiting
/// 2. Завершает все потоки процесса
/// 3. Закрывает handle table
/// 4. Удаляет из active list и CID
///
/// # Arguments
/// * `process` - указатель на EPROCESS
/// * `exit_status` - код завершения
///
/// # Returns
/// NTSTATUS код результата
pub unsafe fn psp_exit_process(
    process: *mut EPROCESS,
    exit_status: crate::nt::NTSTATUS,
) -> crate::nt::NTSTATUS {
    unsafe {
        use core::sync::atomic::Ordering;

        use super::types::PS_PROCESS_FLAGS_PROCESS_EXITING;
        use crate::nt::LIST_ENTRY;
        use crate::nt::STATUS_SUCCESS;

        if process.is_null() {
            return crate::nt::STATUS_INVALID_PARAMETER;
        }

        // 1. Проверяем, не завершается ли процесс уже
        if (*process).has_flag(PS_PROCESS_FLAGS_PROCESS_EXITING) {
            return STATUS_SUCCESS; // Уже в процессе завершения
        }

        // 2. Помечаем процесс как завершающийся
        (*process).set_flag(PS_PROCESS_FLAGS_PROCESS_EXITING);
        (*process)
            .exit_status
            .store(exit_status as u32, Ordering::Release);

        // 3. Устанавливаем время выхода
        (*process).exit_time =
            crate::nt::LARGE_INTEGER::new(crate::ke::time::ke_query_system_time() as i64);

        // 4. Завершаем все потоки процесса
        // TODO: Итерация по thread_list_head и вызов psp_exit_thread для каждого
        // Пока просто помечаем потоки как terminated
        let thread_list_head = &raw mut (*process).pcb.thread_list_head;
        let mut entry = (*thread_list_head).flink;

        while !entry.is_null() && entry != thread_list_head as *mut LIST_ENTRY {
            // CONTAINING_RECORD: получаем KTHREAD из thread_list_entry
            let kthread_offset =
                core::mem::offset_of!(crate::ke::thread::KTHREAD, thread_list_entry);
            let kthread = (entry as *mut u8).sub(kthread_offset) as *mut crate::ke::thread::KTHREAD;

            // Помечаем поток как terminated
            (*kthread).set_state(crate::ke::thread::KTHREAD_STATE::Terminated);

            // Следующий поток
            entry = (*entry).flink;
        }

        // 5. Удаляем из системных списков
        psp_remove_process(process);

        // 6. Закрываем handle table
        let handle_table = (*process)
            .object_table
            .swap(core::ptr::null_mut(), Ordering::AcqRel);
        if !handle_table.is_null() {
            crate::ex::handle::ex_destroy_handle_table(handle_table);
        }

        crate::kd_print!(
            "[PS] psp_exit_process: PID={} terminated\n",
            (*process).process_id()
        );

        // 7. Если это текущий процесс - нужно переключиться
        let current = ps_get_current_process();
        if process == current {
            // Текущий процесс завершается - переключаемся на System Process
            // TODO: Полноценный механизм перехода
            crate::kd_print!(
                "[PS] psp_exit_process: current process is terminating, need switch\n"
            );
        }

        STATUS_SUCCESS
    }
}
