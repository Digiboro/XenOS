//! Инициализация Kernel subsystem
//!
//! Источники:
//! - NT5: ke/krnlinit.c
//! - ReactOS: ke/krnlinit.c

#![allow(dead_code)]

use core::cell::UnsafeCell;
use core::sync::atomic::AtomicU8;
use core::sync::atomic::AtomicU32;
use core::sync::atomic::AtomicU64;
use core::sync::atomic::Ordering;

use super::dpc::KDPC;
use super::spinlock::KSPIN_LOCK;
use crate::nt::LIST_ENTRY;

// =============================================================================
// Глобальные спинлоки системы
// =============================================================================

/// Размер таблицы таймеров (используем значение из timer.rs)
const TIMER_TABLE_SIZE: usize = super::timer::TIMER_TABLE_SIZE;

/// KiTimerTableLock - массив спинлоков для таблицы таймеров (per-bucket)
pub static TIMER_TABLE_LOCK: [KSPIN_LOCK; TIMER_TABLE_SIZE] =
    [const { KSPIN_LOCK::new() }; TIMER_TABLE_SIZE];

/// KiProfileLock - защищает профилирование
pub static PROFILE_LOCK: KSPIN_LOCK = KSPIN_LOCK::new();

/// KiFreezeLock - используется при остановке системы
pub static FREEZE_LOCK: KSPIN_LOCK = KSPIN_LOCK::new();

// =============================================================================
// Глобальные списки (используем UnsafeCell для interior mutability)
// =============================================================================

/// Wrapper для глобальных данных
#[repr(transparent)]
pub struct GlobalData<T>(UnsafeCell<T>);

unsafe impl<T> Sync for GlobalData<T> {}

impl<T> GlobalData<T> {
    pub const fn new(value: T) -> Self {
        Self(UnsafeCell::new(value))
    }

    /// # Safety
    /// Вызывающий должен гарантировать синхронизацию
    #[inline]
    pub unsafe fn get(&self) -> *mut T {
        self.0.get()
    }
}

/// KiTimerTableListHead - таблица списков таймеров
///
/// ReactOS: KTIMER_TABLE_ENTRY KiTimerTableListHead[TIMER_TABLE_SIZE]
///
/// Каждая запись содержит:
/// - entry: список таймеров в bucket'е
/// - time: минимальное время таймера в bucket'е (для быстрой проверки)
pub static TIMER_TABLE_LIST_HEAD: GlobalData<[super::timer::KTIMER_TABLE_ENTRY; TIMER_TABLE_SIZE]> =
    GlobalData::new([const { super::timer::KTIMER_TABLE_ENTRY::new() }; TIMER_TABLE_SIZE]);

/// KiBugCheckCallbackListHead - список callback'ов для bugcheck
static BUGCHECK_CALLBACK_LIST_HEAD: GlobalData<LIST_ENTRY> = GlobalData::new(LIST_ENTRY::new());

/// Возвращает указатель на глобальный список bugcheck callbacks (KeBugcheckCallbackListHead).
///
/// # Safety
/// Вызывающий должен обеспечить синхронизацию (обычно через IRQL=HIGH_LEVEL).
pub(crate) unsafe fn ki_bugcheck_callback_list_head() -> *mut LIST_ENTRY {
    unsafe { BUGCHECK_CALLBACK_LIST_HEAD.get() }
}

/// KiProfileListHead - список профилей
static PROFILE_LIST_HEAD: GlobalData<LIST_ENTRY> = GlobalData::new(LIST_ENTRY::new());

/// KiProfileSourceListHead - список источников профилирования
static PROFILE_SOURCE_LIST_HEAD: GlobalData<LIST_ENTRY> = GlobalData::new(LIST_ENTRY::new());

// =============================================================================
// Глобальные переменные
// =============================================================================

/// Максимальное количество процессоров
pub const MAX_PROCESSORS: usize = 64;

/// KiProcessorBlock - массив указателей на KPRCB для каждого процессора
static PROCESSOR_BLOCK: GlobalData<[*mut crate::arch::x86_64::pcr::KPRCB; MAX_PROCESSORS]> =
    GlobalData::new([core::ptr::null_mut(); MAX_PROCESSORS]);

/// KeActiveProcessors - битовая маска активных процессоров
static ACTIVE_PROCESSORS: AtomicU64 = AtomicU64::new(0);

/// KeNumberProcessors - количество активных процессоров
static NUMBER_PROCESSORS: AtomicU8 = AtomicU8::new(0);

/// KeMaximumIncrement - максимальный инкремент времени (в 100ns единицах)
static MAXIMUM_INCREMENT: AtomicU32 = AtomicU32::new(100000); // 10ms по умолчанию

/// KeMinimumIncrement - минимальный инкремент времени
static MINIMUM_INCREMENT: AtomicU32 = AtomicU32::new(10000); // 1ms

/// KeTimeIncrement - текущий инкремент времени
static TIME_INCREMENT: AtomicU32 = AtomicU32::new(100000);

/// KiDpcTimeout - таймаут для DPC (в тиках)
pub static DPC_TIMEOUT: AtomicU32 = AtomicU32::new(110);

/// KiTimerExpirationDpc - DPC для обработки истекших таймеров
static TIMER_EXPIRATION_DPC: GlobalData<KDPC> = GlobalData::new(KDPC::new());

// =============================================================================
// Функции инициализации
// =============================================================================

/// KiInitSystem - инициализация системных структур ядра
///
/// Вызывается на boot processor в Phase 0
///
/// # Safety
/// Должен вызываться только один раз при старте системы
pub unsafe fn ki_init_system() {
    unsafe {
        // Инициализируем списки bugcheck callbacks
        LIST_ENTRY::init_head(BUGCHECK_CALLBACK_LIST_HEAD.get());

        // Инициализируем таблицу таймеров
        let table = &mut *TIMER_TABLE_LIST_HEAD.get();
        for table_entry in table.iter_mut() {
            LIST_ENTRY::init_head(&mut table_entry.entry as *mut LIST_ENTRY);
            table_entry.time = u64::MAX; // bucket пуст
        }

        // Инициализируем списки профилирования
        LIST_ENTRY::init_head(PROFILE_LIST_HEAD.get());
        LIST_ENTRY::init_head(PROFILE_SOURCE_LIST_HEAD.get());

        // Инициализируем Timer Expiration DPC
        super::dpc::ke_initialize_dpc(
            &mut *TIMER_EXPIRATION_DPC.get(),
            ki_timer_expiration_dpc,
            core::ptr::null_mut(),
        );
    }
}

/// KiInitSpinLocks - инициализация глобальных спинлоков
///
/// # Safety
/// Должен вызываться только при инициализации
pub unsafe fn ki_init_spin_locks() {
    // Глобальные спинлоки уже инициализированы статически
    // Эта функция может быть расширена для per-processor спинлоков
}

/// Callback для Timer Expiration DPC
extern "win64" fn ki_timer_expiration_dpc(
    _dpc: *mut KDPC,
    _deferred_context: *mut core::ffi::c_void,
    _system_argument1: *mut core::ffi::c_void,
    _system_argument2: *mut core::ffi::c_void,
) {
    // TODO: Обработка истекших таймеров
    // Это будет реализовано в Фазе 1.4
}

/// Регистрирует процессор в системе
///
/// # Safety
/// PRCB должен быть валиден
pub unsafe fn ki_register_processor(
    processor_number: u8,
    prcb: *mut crate::arch::x86_64::pcr::KPRCB,
) {
    unsafe {
        if (processor_number as usize) < MAX_PROCESSORS {
            let block = &mut *PROCESSOR_BLOCK.get();
            block[processor_number as usize] = prcb;
            ACTIVE_PROCESSORS.fetch_or(1 << processor_number, Ordering::SeqCst);

            let current = NUMBER_PROCESSORS.load(Ordering::SeqCst);
            if processor_number >= current {
                NUMBER_PROCESSORS.store(processor_number + 1, Ordering::SeqCst);
            }
        }
    }
}

/// Возвращает количество активных процессоров
pub fn ke_number_processors() -> u8 {
    NUMBER_PROCESSORS.load(Ordering::SeqCst)
}

/// Возвращает битовую маску активных процессоров
pub fn ke_active_processors() -> u64 {
    ACTIVE_PROCESSORS.load(Ordering::SeqCst)
}

/// Возвращает указатель на PRCB процессора
///
/// # Safety
/// Номер процессора должен быть валиден
pub unsafe fn ke_get_processor_prcb(processor_number: u8) -> *mut crate::arch::x86_64::pcr::KPRCB {
    unsafe {
        if (processor_number as usize) < MAX_PROCESSORS {
            let block = &*PROCESSOR_BLOCK.get();
            block[processor_number as usize]
        } else {
            core::ptr::null_mut()
        }
    }
}

/// Возвращает текущий инкремент времени
pub fn ke_query_time_increment() -> u32 {
    TIME_INCREMENT.load(Ordering::SeqCst)
}

/// Устанавливает инкремент времени
pub fn ke_set_time_increment(increment: u32) {
    let min = MINIMUM_INCREMENT.load(Ordering::SeqCst);
    let max = MAXIMUM_INCREMENT.load(Ordering::SeqCst);
    TIME_INCREMENT.store(increment.clamp(min, max), Ordering::SeqCst);
}

/// Возвращает максимальный инкремент времени
pub fn ke_query_maximum_increment() -> u32 {
    MAXIMUM_INCREMENT.load(Ordering::SeqCst)
}

// =============================================================================
// KiSwitchToBootStack / KiSystemStartupBootStack / KiInitializeKernel
// =============================================================================

// Внешняя ASM функция для переключения стека
unsafe extern "win64" {
    /// KiSwitchToBootStack (ASM)
    ///
    /// Переключает стек на InitialStack и вызывает ki_system_startup_boot_stack.
    /// Никогда не возвращает.
    ///
    /// # Arguments
    /// * `initial_stack` - верхняя граница стека (выровненная)
    pub fn ki_switch_to_boot_stack(initial_stack: *mut u8) -> !;
}

/// KiSystemStartupBootStack
///
/// Вызывается из KiSwitchToBootStack (ASM) после переключения на boot stack.
/// Выполняет инициализацию ядра и входит в idle loop.
///
/// Порядок действий:
/// 1. Вызывает ki_initialize_kernel (инициализация подсистем)
/// 2. Устанавливает приоритет Idle Thread = 0
/// 3. Включает прерывания (STI)
/// 4. Понижает IRQL до DISPATCH_LEVEL
/// 5. Входит в ki_idle_loop (никогда не возвращает)
///
/// Источники:
/// - NT6.1: ke/amd64/kiinit.c (KiSystemStartupBootStack)
/// - ReactOS: ntoskrnl/ke/amd64/kiinit.c:685-710
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ki_system_startup_boot_stack() -> ! {
    unsafe {
        use super::idle::ki_idle_loop;
        use crate::hal::irql::DISPATCH_LEVEL;
        use crate::hal::irql::kf_lower_irql;

        // Инициализация ядра (Phase 0)
        ki_initialize_kernel();

        // Получаем текущий поток (Idle Thread)
        let prcb = crate::arch::x86_64::pcr::get_prcb();
        let thread = (*prcb).current_thread as *mut super::thread::KTHREAD;

        // Устанавливаем приоритет 0 (только для idle thread)
        (*thread).priority = 0;

        // Прерывания уже включены в exp_initialize_executive после HAL(0)
        // Поэтому здесь STI не нужен

        // Понижаем IRQL до DISPATCH_LEVEL перед входом в idle loop
        kf_lower_irql(DISPATCH_LEVEL);

        // Устанавливаем WaitIrql для idle thread
        (*thread).wait_irql = DISPATCH_LEVEL;

        crate::kd::dbg_print("\n[KERNEL] Starting scheduler: entering KiIdleLoop...\n");

        // Вход в idle loop (никогда не возвращает)
        ki_idle_loop();
    }
}

/// KiInitializeKernel
///
/// Главная функция инициализации ядра.
/// Вызывается из ki_system_startup_boot_stack.
///
/// Выполняет:
/// 1. Понижает IRQL до APC_LEVEL
/// 2. Вызывает ExpInitializeExecutive (Phase 0 всех подсистем)
/// 3. Поднимает IRQL до DISPATCH_LEVEL
/// 4. Устанавливает приоритет Idle Thread через ki_set_idle_thread_priority
/// 5. Поднимает IRQL до HIGH_LEVEL
///
/// Источники:
/// - NT6.1: ke/amd64/kiinit.c (KiInitializeKernel)
/// - ReactOS: ntoskrnl/ke/amd64/kiinit.c:433-638
pub unsafe fn ki_initialize_kernel() {
    unsafe {
        use super::globals::ke_get_loader_block;
        use crate::hal::irql::APC_LEVEL;
        use crate::hal::irql::DISPATCH_LEVEL;
        use crate::hal::irql::HIGH_LEVEL;
        use crate::hal::irql::hal_set_irql;
        use crate::hal::irql::kf_raise_irql;
        use crate::nt::STATUS_SUCCESS;

        let prcb = crate::arch::x86_64::pcr::get_prcb();
        let cpu = (*prcb).number as u32;

        // Понижаем до APC_LEVEL для инициализации подсистем
        hal_set_irql(APC_LEVEL);

        // Вызываем ExpInitializeExecutive (Phase 0 всех подсистем)
        let loader_block = ke_get_loader_block();
        let status = crate::ex::exp_initialize_executive(cpu, loader_block);
        if status != STATUS_SUCCESS {
            crate::ke::bugcheck::ke_bug_check_ex(
                crate::ke::bugcheck::bugcheck_codes::PHASE0_INITIALIZATION_FAILED,
                status as usize,
                0,
                0,
                0,
            );
        }

        // Поднимаем до DISPATCH_LEVEL
        let _old_irql = kf_raise_irql(DISPATCH_LEVEL);

        // Устанавливаем приоритет Idle Thread до 0 + KiIdleSummary
        super::idle::ki_set_idle_thread_priority(prcb);

        // Поднимаем до HIGH_LEVEL перед возвратом
        let _old_irql = kf_raise_irql(HIGH_LEVEL);
    }
}
