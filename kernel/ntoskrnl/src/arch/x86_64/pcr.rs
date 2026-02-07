//! KPCR и KPRCB - Processor Control Region и Block
//!
//! Источники:
//! - NT5: inc/amd64.h, ke/amd64/initkr.c
//! - ReactOS: include/ndk/amd64/ketypes.h

#![allow(dead_code)]
#![allow(non_camel_case_types)]

use crate::ke::spinlock::KIRQL;
use crate::ke::spinlock::KSPIN_LOCK;
use crate::ke::spinlock::KSPIN_LOCK_QUEUE;
use crate::nt::LIST_ENTRY;
use crate::nt::PVOID;
use crate::nt::SINGLE_LIST_ENTRY;
use crate::nt::UCHAR;
use crate::nt::ULONG;
use crate::nt::USHORT;

/// NT_TIB - Thread Information Block (начало TEB/PCR)
#[repr(C)]
pub struct NT_TIB {
    /// Указатель на начало списка SEH
    pub exception_list: PVOID,
    /// Верхняя граница стека
    pub stack_base: PVOID,
    /// Нижняя граница стека
    pub stack_limit: PVOID,
    /// SubSystemTib
    pub sub_system_tib: PVOID,
    /// Fiber data или Version
    pub fiber_data: PVOID,
    /// Arbitrary user pointer
    pub arbitrary_user_pointer: PVOID,
    /// Указатель на себя
    pub self_ptr: *mut NT_TIB,
}

impl NT_TIB {
    pub const fn new() -> Self {
        Self {
            exception_list: core::ptr::null_mut(),
            stack_base: core::ptr::null_mut(),
            stack_limit: core::ptr::null_mut(),
            sub_system_tib: core::ptr::null_mut(),
            fiber_data: core::ptr::null_mut(),
            arbitrary_user_pointer: core::ptr::null_mut(),
            self_ptr: core::ptr::null_mut(),
        }
    }
}

impl Default for NT_TIB {
    fn default() -> Self {
        Self::new()
    }
}

/// KPRCB - Kernel Processor Control Block
///
/// Содержит per-processor данные ядра
#[repr(C)]
pub struct KPRCB {
    // ========== Базовые поля ==========
    /// Минорная версия
    pub minor_version: USHORT,
    /// Мажорная версия
    pub major_version: USHORT,

    /// Текущий поток
    pub current_thread: PVOID, // PKTHREAD
    /// Следующий поток для переключения
    pub next_thread: PVOID, // PKTHREAD
    /// Idle поток
    pub idle_thread: PVOID, // PKTHREAD

    /// Номер процессора
    pub number: UCHAR,
    /// Зарезервировано
    pub reserved: UCHAR,
    /// Build type
    pub build_type: USHORT,

    /// Маска принадлежности
    pub set_member: usize,

    /// CPU type
    pub cpu_type: UCHAR,
    /// CPU ID flag
    pub cpu_id: UCHAR,
    /// CPU step
    pub cpu_step: USHORT,

    /// Halted flag
    pub halted: UCHAR,
    /// CI enabled
    pub ci_enabled: UCHAR,

    /// Processor state
    pub processor_state: UCHAR,
    /// Reserved
    pub reserved2: UCHAR,

    // ========== Queued Spinlocks ==========
    /// Очереди для queued spinlocks
    pub lock_queue: [KSPIN_LOCK_QUEUE; 16],

    // ========== DPC ==========
    /// Спинлок для DPC
    pub dpc_lock: KSPIN_LOCK,

    /// Список DPC
    pub dpc_list_head: LIST_ENTRY,

    /// DPC stack
    pub dpc_stack: PVOID,

    /// Глубина очереди DPC
    pub dpc_queue_depth: ULONG,

    /// DPC count
    pub dpc_count: ULONG,

    /// DPC requested flag
    pub dpc_requested: ULONG,

    /// DPC routine active
    pub dpc_routine_active: ULONG,

    /// DPC interrupt already requested (avoid duplicate requests)
    pub dpc_interrupt_requested: ULONG,

    /// APC interrupt already requested (avoid duplicate requests)
    /// NT: используется для software interrupt delivery
    pub apc_interrupt_requested: ULONG,

    /// Maximum DPC queue depth before forcing interrupt (NT default: 4)
    pub maximum_dpc_queue_depth: ULONG,

    /// Minimum DPC request rate threshold (NT default: 3)
    pub minimum_dpc_rate: ULONG,

    /// Current DPC request rate (DPCs per tick, calculated in timer)
    pub dpc_request_rate: ULONG,

    /// IPI request summary (битовая маска типов IPI)
    pub ipi_request_summary: ULONG,

    /// IPI freeze flag (для debugger freeze)
    pub ipi_freeze_flag: ULONG,

    /// IPI worker routine (для IPI_PACKET_READY)
    pub ipi_worker_routine: Option<unsafe extern "win64" fn(PVOID, PVOID, PVOID)>,

    /// IPI current packet arguments (для IPI_PACKET_READY)
    pub ipi_current_packet: [PVOID; 3],

    /// Signal done - PRCB отправителя IPI (для IPI_PACKET_READY acknowledgment)
    pub signal_done: PVOID,

    /// DPC last count
    pub dpc_last_count: ULONG,

    /// Quantum end DPC
    pub quantum_end: ULONG,

    // ========== Прерывания ==========
    /// Текущий IRQL
    pub current_irql: KIRQL,

    /// Timer expiration request (0 = нет, 1 = есть истекшие таймеры)
    /// Используется для отложенной обработки таймеров на DISPATCH_LEVEL
    pub timer_request: u32,

    /// Индекс в таблице таймеров с истекшими таймерами
    /// Используется вместе с timer_request для обработки на DISPATCH_LEVEL
    pub timer_hand: u32,

    /// Количество прерываний
    pub interrupt_count: ULONG,

    // ========== Idle ==========
    /// Idle count
    pub idle_count: ULONG,

    // ========== Context Switches ==========
    /// Количество переключений контекста
    pub context_switches: ULONG,

    // ========== Планировщик ==========
    /// Ready summary (bitmap приоритетов)
    pub ready_summary: ULONG,

    /// Dispatcher ready list heads (по приоритетам)
    pub dispatcher_ready_list_head: [LIST_ENTRY; 32],

    // ========== Vendor ==========
    /// Vendor string
    pub vendor_string: [UCHAR; 13],

    /// Initial APIC ID
    pub initial_apic_id: UCHAR,

    /// Logical processors per physical
    pub logical_processors_per_physical: UCHAR,

    // ========== Feature Bits ==========
    /// Feature bits
    pub feature_bits: ULONG,

    // ========== Время ==========
    /// Kernel time
    pub kernel_time: ULONG,

    /// User time
    pub user_time: ULONG,

    /// DPC time
    pub dpc_time: ULONG,

    /// Interrupt time
    pub interrupt_time: ULONG,

    // ========== Parent node ==========
    /// Parent NUMA node
    pub parent_node: PVOID, // PKNODE

    // ========== Context Switch (ReactOS compatibility) ==========
    /// RspBase - базовый адрес стека ring 0 (копия TSS.Rsp0)
    pub rsp_base: u64,

    // ========== Scheduler Locks (для SMP и планировщика) ==========
    /// PRCB lock - блокировка для защиты PRCB (ready queues и т.д.)
    pub prcb_lock: KSPIN_LOCK,

    /// Context swap lock - блокировка при переключении контекста (SMP)
    pub context_swap_lock: KSPIN_LOCK,

    /// Idle schedule flag - процессор в режиме idle scheduling
    pub idle_schedule: u8,

    /// MultiThreadProcessorSet - маска всех логических процессоров физического ядра (SMT/HT)
    /// Для процессоров без HT это просто (1 << number)
    /// Для HT процессоров это маска всех siblings
    pub multi_thread_processor_set: u64,

    /// MultiThreadSetBusy - хотя бы один логический процессор ядра занят
    pub multi_thread_set_busy: u8,

    /// DeferredReadyListHead - single-list для отложенно готовых потоков
    /// (ReactOS: KPRCB.DeferredReadyListHead)
    pub deferred_ready_list_head: SINGLE_LIST_ENTRY,

    /// Padding для выравнивания
    pub _sched_pad: [u8; 7],
}

impl KPRCB {
    pub const fn new() -> Self {
        Self {
            minor_version: 0,
            major_version: 0,
            current_thread: core::ptr::null_mut(),
            next_thread: core::ptr::null_mut(),
            idle_thread: core::ptr::null_mut(),
            number: 0,
            reserved: 0,
            build_type: 0,
            set_member: 0,
            cpu_type: 0,
            cpu_id: 0,
            cpu_step: 0,
            halted: 0,
            ci_enabled: 0,
            processor_state: 0,
            reserved2: 0,
            lock_queue: [KSPIN_LOCK_QUEUE::new(); 16],
            dpc_lock: KSPIN_LOCK::new(),
            dpc_list_head: LIST_ENTRY::new(),
            dpc_stack: core::ptr::null_mut(),
            dpc_queue_depth: 0,
            dpc_count: 0,
            dpc_requested: 0,
            dpc_routine_active: 0,
            dpc_interrupt_requested: 0,
            apc_interrupt_requested: 0,
            maximum_dpc_queue_depth: 4, // NT default
            minimum_dpc_rate: 3,        // NT default
            dpc_request_rate: 0,
            ipi_request_summary: 0,
            ipi_freeze_flag: 0,
            ipi_worker_routine: None,
            ipi_current_packet: [core::ptr::null_mut(); 3],
            signal_done: core::ptr::null_mut(),
            dpc_last_count: 0,
            quantum_end: 0,
            current_irql: 0,
            timer_request: 0,
            timer_hand: 0,
            interrupt_count: 0,
            idle_count: 0,
            context_switches: 0,
            ready_summary: 0,
            dispatcher_ready_list_head: [LIST_ENTRY::new(); 32],
            vendor_string: [0; 13],
            initial_apic_id: 0,
            logical_processors_per_physical: 1,
            feature_bits: 0,
            kernel_time: 0,
            user_time: 0,
            dpc_time: 0,
            interrupt_time: 0,
            parent_node: core::ptr::null_mut(),
            rsp_base: 0,
            prcb_lock: KSPIN_LOCK::new(),
            context_swap_lock: KSPIN_LOCK::new(),
            idle_schedule: 0,
            multi_thread_processor_set: 0, // Инициализируется при старте процессора
            multi_thread_set_busy: 0,
            deferred_ready_list_head: SINGLE_LIST_ENTRY::new(),
            _sched_pad: [0; 7],
        }
    }
}

impl Default for KPRCB {
    fn default() -> Self {
        Self::new()
    }
}

/// Версии KPCR/KPRCB
pub const PCR_MAJOR_VERSION: USHORT = 1;
pub const PCR_MINOR_VERSION: USHORT = 1;
pub const PRCB_MAJOR_VERSION: USHORT = 1;
pub const PRCB_MINOR_VERSION: USHORT = 1;

/// KPCR - Kernel Processor Control Region
///
/// Доступна через GS:[0] в kernel mode
#[repr(C)]
pub struct KPCR {
    // ========== NT_TIB совместимость ==========
    pub nt_tib: NT_TIB,

    // ========== PCR поля ==========
    /// Указатель на себя
    pub self_pcr: *mut KPCR,

    /// Указатель на PRCB
    pub current_prcb: *mut KPRCB,

    /// Lock queue владелец
    pub lock_array: PVOID,

    /// Used field
    pub used: PVOID,

    // ========== IDT/GDT/TSS ==========
    /// IDT base
    pub idt_base: *mut super::idt::IdtEntry,

    /// TSS base (для обновления RSP0 при context switch)
    pub tss_base: *mut super::tss::Tss64,

    /// Unused
    pub unused: u64,

    // ========== Текущий IRQL ==========
    /// Текущий IRQL
    pub irql: KIRQL,

    // ========== Номер процессора ==========
    /// Secondary cache size
    pub secondary_cache_size: ULONG,

    /// Номер процессора
    pub number: UCHAR,

    // ========== Версии ==========
    /// Major version
    pub major_version: UCHAR,
    /// Minor version  
    pub minor_version: UCHAR,

    // ========== Stall factor ==========
    /// Stall scale factor
    pub stall_scale_factor: ULONG,

    // ========== Unused ==========
    /// Spare
    pub spare: [PVOID; 3],

    // ========== Kernel debug ==========
    /// Kernel debugger active
    pub kd_version_block: PVOID,

    // ========== Reserved ==========
    pub kd_secondary_version_block: PVOID,

    // ========== PRCB встроен в конец ==========
    pub prcb: KPRCB,
}

impl KPCR {
    pub const fn new() -> Self {
        Self {
            nt_tib: NT_TIB::new(),
            self_pcr: core::ptr::null_mut(),
            current_prcb: core::ptr::null_mut(),
            lock_array: core::ptr::null_mut(),
            used: core::ptr::null_mut(),
            idt_base: core::ptr::null_mut(),
            tss_base: core::ptr::null_mut(),
            unused: 0,
            irql: 0,
            secondary_cache_size: 0,
            number: 0,
            major_version: PCR_MAJOR_VERSION as UCHAR,
            minor_version: PCR_MINOR_VERSION as UCHAR,
            stall_scale_factor: 0,
            spare: [core::ptr::null_mut(); 3],
            kd_version_block: core::ptr::null_mut(),
            kd_secondary_version_block: core::ptr::null_mut(),
            prcb: KPRCB::new(),
        }
    }

    /// Инициализирует PCR
    pub fn init(&mut self, processor_number: UCHAR) {
        self.self_pcr = self as *mut KPCR;
        self.current_prcb = &mut self.prcb as *mut KPRCB;
        self.number = processor_number;
        self.major_version = PCR_MAJOR_VERSION as UCHAR;
        self.minor_version = PCR_MINOR_VERSION as UCHAR;

        self.prcb.number = processor_number;
        self.prcb.major_version = PRCB_MAJOR_VERSION;
        self.prcb.minor_version = PRCB_MINOR_VERSION;
        self.prcb.set_member = 1 << processor_number;

        // Инициализируем DPC list
        unsafe {
            LIST_ENTRY::init_head(&mut self.prcb.dpc_list_head as *mut LIST_ENTRY);

            // Инициализируем dispatcher ready lists
            for list in &mut self.prcb.dispatcher_ready_list_head {
                LIST_ENTRY::init_head(list as *mut LIST_ENTRY);
            }
        }

        // NT_TIB self pointer
        self.nt_tib.self_ptr = &mut self.nt_tib as *mut NT_TIB;
    }
}

impl Default for KPCR {
    fn default() -> Self {
        Self::new()
    }
}

/// Получает указатель на текущий PCR через GS
///
/// # Safety
/// GS должен быть корректно настроен на PCR
#[inline]
pub unsafe fn get_pcr() -> *mut KPCR {
    let pcr: *mut KPCR;
    unsafe {
        core::arch::asm!(
            "mov {}, gs:[{offset}]",
            out(reg) pcr,
            offset = const(core::mem::offset_of!(KPCR, self_pcr)),
            options(nostack, preserves_flags, readonly)
        );
    }
    pcr
}

/// Получает указатель на текущий PRCB через GS
///
/// # Safety
/// GS должен быть корректно настроен на PCR
#[inline]
pub unsafe fn get_prcb() -> *mut KPRCB {
    let prcb: *mut KPRCB;
    unsafe {
        core::arch::asm!(
            "mov {}, gs:[{offset}]",
            out(reg) prcb,
            offset = const(core::mem::offset_of!(KPCR, current_prcb)),
            options(nostack, preserves_flags, readonly)
        );
    }
    prcb
}

/// Получает номер текущего процессора
///
/// # Safety
/// GS должен быть корректно настроен на PCR
#[inline]
pub unsafe fn get_processor_number() -> UCHAR {
    let number: u8;
    unsafe {
        core::arch::asm!(
            "mov {}, gs:[{offset}]",
            out(reg_byte) number,
            offset = const(core::mem::offset_of!(KPCR, number)),
            options(nostack, preserves_flags, readonly)
        );
    }
    number
}

/// Получает текущий IRQL
///
/// # Safety
/// GS должен быть корректно настроен на PCR
#[inline]
pub unsafe fn get_current_irql() -> KIRQL {
    let irql: u8;
    unsafe {
        core::arch::asm!(
            "mov {}, gs:[{offset}]",
            out(reg_byte) irql,
            offset = const(core::mem::offset_of!(KPCR, irql)),
            options(nostack, preserves_flags, readonly)
        );
    }
    irql
}

/// Устанавливает текущий IRQL
///
/// # Safety
/// GS должен быть корректно настроен на PCR
#[inline]
pub unsafe fn set_current_irql(irql: KIRQL) {
    unsafe {
        core::arch::asm!(
            "mov gs:[{offset}], {irql}",
            offset = const(core::mem::offset_of!(KPCR, irql)),
            irql = in(reg_byte) irql,
            options(nostack, preserves_flags)
        );
    }
}
