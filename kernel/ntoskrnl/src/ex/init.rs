//! Инициализация Executive subsystem
//!
//! Executive - центральная подсистема ядра NT, координирующая инициализацию
//! всех остальных подсистем.
//!
//! # Архитектура
//!
//! ```text
//! KiSystemStartup (main.rs)
//!       │
//!       └── KiSwitchToBootStack()
//!             └── KiSystemStartupBootStack()
//!                   └── KiInitializeKernel()
//!                         │
//!                         └── ExpInitializeExecutive (этот модуль)
//!                               │
//!                               ├─► exp_is_loader_valid()
//!                               ├─► exp_set_initialization_phase(0)
//!                               ├─► HAL Phase 0 → STI
//!                               ├─► KE, EX, MM, OB
//!                               └─► PS (создает Phase1 thread) → IO
//! ```
//!
//! # Фазы инициализации
//!
//! - Phase 0: Ранняя инициализация (подсистемы, типы объектов, System Process)
//! - Phase 1: Основная инициализация (таймеры, APIC, драйверы)
//! - Phase 2: Инициализация завершена
//!
//! Источники:
//! - MSDN (NT6.1 / Windows 7): Executive initialization
//! - NT5: ex/init.c, init/init.c
//! - ReactOS: ntoskrnl/ex/init.c:925-1342

#![allow(dead_code)]

use core::sync::atomic::AtomicU32;
use core::sync::atomic::Ordering;

use crate::ke::init::GlobalData;
use crate::ke::mutex::FAST_MUTEX;
use crate::ke::spinlock::KSPIN_LOCK;
use crate::nt::NTSTATUS;
use crate::nt::STATUS_SUCCESS;

// =============================================================================
// Глобальные переменные Executive
// =============================================================================

/// ExpInitializationPhase - глобальная фаза инициализации Executive
///
/// Значения:
/// - 0: Phase 0 (ранняя инициализация)
/// - 1: Phase 1 выполняется
/// - 2: Phase 1 завершена
///
/// NT: ex/init.c (ExpInitializationPhase)
static EXP_INITIALIZATION_PHASE: AtomicU32 = AtomicU32::new(0);

/// ExpEnvironmentLock - защищает системные переменные окружения
static EXP_ENVIRONMENT_LOCK: GlobalData<FAST_MUTEX> = GlobalData::new(FAST_MUTEX::new());

/// ExpResourceSpinLock - спинлок для ERESOURCE
static EXP_RESOURCE_SPIN_LOCK: KSPIN_LOCK = KSPIN_LOCK::new();

/// Флаг инициализации Phase 0
static mut PHASE0_INITIALIZED: bool = false;

/// Флаг инициализации Phase 1
static mut PHASE1_INITIALIZED: bool = false;

// =============================================================================
// ExpInitializationPhase API
// =============================================================================

/// Получить текущую фазу инициализации Executive
///
/// # Возвращает
/// - 0: Phase 0 (ранняя инициализация)
/// - 1: Phase 1 выполняется
/// - 2: Phase 1 завершена
#[inline]
pub fn exp_get_initialization_phase() -> u32 {
    EXP_INITIALIZATION_PHASE.load(Ordering::Acquire)
}

/// Установить фазу инициализации Executive
#[inline]
pub fn exp_set_initialization_phase(phase: u32) {
    EXP_INITIALIZATION_PHASE.store(phase, Ordering::Release);
}

// =============================================================================
// Валидация LoaderBlock
// =============================================================================

/// ExpIsLoaderValid
///
/// Валидация LoaderBlock перед инициализацией.
/// В NT проверяет совместимость HAL и ядра.
///
/// Источники:
/// - NT: ex/init.c (ExpIsLoaderValid)
pub unsafe fn exp_is_loader_valid(loader_block: *const ntldr::LOADER_PARAMETER_BLOCK) -> bool {
    unsafe {
        if loader_block.is_null() {
            return false;
        }

        let lpb = &*loader_block;

        // Проверка версии
        if lpb.OsMajorVersion == 0 {
            crate::kd::dbg_print("[EX] exp_is_loader_valid: OsMajorVersion is 0\n");
            return false;
        }

        // Проверка HHDM
        if lpb.HhdmOffset == 0 {
            crate::kd::dbg_print("[EX] exp_is_loader_valid: HhdmOffset is 0\n");
            return false;
        }

        // Проверка kernel base
        if lpb.KernelBase == 0 {
            crate::kd::dbg_print("[EX] exp_is_loader_valid: KernelBase is 0\n");
            return false;
        }

        // TODO: Проверка совместимости HAL версии
        // TODO: Проверка NLS данных

        true
    }
}

// =============================================================================
// Handle Tables инициализация
// =============================================================================

/// ExpInitializeHandleTables
///
/// Инициализация глобальных структур для handle tables.
/// Вызывается из ExpInitializeExecutive до ObInitSystem.
///
/// NT: ex/handle.c (ExpInitializeHandleTables)
pub unsafe fn exp_initialize_handle_tables() {
    // TODO: Инициализация lookaside list для handle table entries
    // ExpHandleTableEntryLookasideList

    // TODO: Инициализация спинлока HandleTableListLock

    // TODO: Инициализация списка всех handle tables
    // HandleTableListHead

    crate::kd::dbg_print("       exp_initialize_handle_tables()... ");
    crate::kd::dbg_print_ok();
    crate::kd::dbg_print("\n");
}

// =============================================================================
// Инициализация
// =============================================================================

/// ExpInitSystemPhase0 - фаза 0 инициализации Executive
///
/// Вызывается из ExpInitializeExecutive до создания потоков
///
/// # Safety
/// Должен вызываться один раз на boot processor
pub unsafe fn exp_init_system_phase0() -> NTSTATUS {
    unsafe {
        // Уже инициализировано?
        if PHASE0_INITIALIZED {
            return STATUS_SUCCESS;
        }

        // Инициализируем Environment Lock
        crate::ke::mutex::ex_initialize_fast_mutex(&mut *EXP_ENVIRONMENT_LOCK.get());

        // Pool уже инициализирован на этом этапе (настроен загрузчиком)
        // Можем использовать ExAllocatePool

        // Инициализируем work queues
        super::work::exp_init_work_queues();

        PHASE0_INITIALIZED = true;
    }

    STATUS_SUCCESS
}

/// ExpInitSystemPhase1 - фаза 1 инициализации Executive
///
/// Вызывается после создания начальных потоков
///
/// # Safety
/// Должен вызываться после Phase 0
pub unsafe fn exp_init_system_phase1() -> NTSTATUS {
    unsafe {
        if PHASE1_INITIALIZED {
            return STATUS_SUCCESS;
        }

        if !PHASE0_INITIALIZED {
            return crate::nt::STATUS_UNSUCCESSFUL;
        }

        // Инициализируем worker threads
        // TODO: Создать рабочие потоки для каждой очереди

        // ExpInitializeEventImplementation - события уже в ke/event.rs

        // ExpInitializeMutantImplementation - мьютексы уже в ke/mutex.rs

        // ExpInitializeSemaphoreImplementation - семафоры уже в ke/semaphore.rs

        // ExpInitializeTimerImplementation - таймеры уже в ke/timer.rs

        // ExpInitializeCallbacks
        // TODO: Callback registration

        // ExpUuidInitialization
        // TODO: UUID генератор

        // ExpInitializeKeyedEventImplementation
        // TODO: Keyed events

        PHASE1_INITIALIZED = true;
    }

    STATUS_SUCCESS
}

/// ExpInitializeExecutive
///
/// Главная функция Phase 0 инициализации Executive.
/// Вызывается из KiInitializeKernel (в XenOS - из ki_phase0_init временно).
///
/// IRQL: APC_LEVEL при входе
///
/// Выполняет:
/// 1. Валидация LoaderBlock
/// 2. Установка ExpInitializationPhase = 0
/// 3. Инициализация подсистем в NT порядке
/// 4. Создание System Process и Phase 1 потока (внутри PsInitSystem)
///
/// # Arguments
/// * `cpu` - номер процессора (0 для BSP)
/// * `loader_block` - указатель на LOADER_PARAMETER_BLOCK
///
/// Источники:
/// - NT6.1: ex/init.c (ExpInitializeExecutive)
/// - ReactOS: ntoskrnl/ex/init.c:925-1342
pub unsafe fn exp_initialize_executive(
    cpu: u32,
    loader_block: *const ntldr::LOADER_PARAMETER_BLOCK,
) -> NTSTATUS {
    unsafe {
        // =========================================================================
        // 1. Валидация LoaderBlock
        // =========================================================================
        if !exp_is_loader_valid(loader_block) {
            crate::ke::bugcheck::ke_bug_check_ex(
                crate::ke::bugcheck::bugcheck_codes::MISMATCHED_HAL,
                0,
                0,
                0,
                0,
            );
        }

        let lpb = &*loader_block;

        // =========================================================================
        // 2. Multi-CPU check (для будущей SMP поддержки)
        // =========================================================================
        if cpu != 0 {
            // Для AP (Application Processors) - только HAL init
            crate::hal::hal_init_system(0);
            return STATUS_SUCCESS;
        }

        // =========================================================================
        // 3. Устанавливаем фазу инициализации
        // =========================================================================
        exp_set_initialization_phase(0);

        // =========================================================================
        // 4. NLS ранняя инициализация (до HAL)
        // =========================================================================
        crate::kd::dbg_print("[NLS] Initializing NLS from LoaderBlock... ");
        match crate::rtl::nls::rtl_initialize_nls_from_loader_block(lpb) {
            Ok(()) => {
                crate::kd::dbg_print("\n");
            },
            Err(status) => {
                crate::kd::dbg_print_fail();
                crate::kd::dbg_print("\n");
                crate::kd::dbg_print("[NLS] FATAL: NLS initialization failed\n");
                crate::ke::bugcheck::ke_bug_check_ex(
                    crate::ke::bugcheck::bugcheck_codes::NLS_DATA_ERROR,
                    status as usize,
                    lpb.UnicodeNlsBase as usize,
                    lpb.UnicodeNlsSize as usize,
                    0,
                );
            },
        }

        // =========================================================================
        // 5. HAL Phase 0
        // =========================================================================
        crate::kd::dbg_print("[HAL] Hardware Abstraction Layer Phase 0... ");
        if crate::hal::hal_init_system(0) {
            crate::kd::dbg_print_ok();
            crate::kd::dbg_print("\n");
            crate::kd::dbg_print("       PIC: 8259 initialized\n");
        } else {
            crate::kd::dbg_print_fail();
            crate::kd::dbg_print("\n");
            crate::ke::bugcheck::ke_bug_check(
                crate::ke::bugcheck::bugcheck_codes::HAL_INITIALIZATION_FAILED,
            );
        }

        // =========================================================================
        // 6. *** STI - включаем прерывания! (NT way) ***
        // В NT прерывания включаются сразу после HAL Phase 0
        // =========================================================================
        crate::kd::dbg_print("[KE] Enabling interrupts (STI)... ");
        crate::hal::irql::enable_interrupts();
        crate::kd::dbg_print_ok();
        crate::kd::dbg_print("\n");

        // =========================================================================
        // 7. KE инициализация
        // =========================================================================
        crate::kd::dbg_print("[KE] Initializing Kernel Executive...\n");

        crate::kd::dbg_print("       ki_init_system()... ");
        crate::ke::init::ki_init_system();
        crate::kd::dbg_print_ok();
        crate::kd::dbg_print("\n");

        crate::kd::dbg_print("       ki_init_scheduler()... ");
        crate::ke::sched::ki_init_scheduler();
        crate::kd::dbg_print_ok();
        crate::kd::dbg_print("\n");

        crate::kd::dbg_print("       ki_init_spin_locks()... ");
        crate::ke::init::ki_init_spin_locks();
        crate::kd::dbg_print_ok();
        crate::kd::dbg_print("\n");

        crate::kd::dbg_print("       ki_initialize_system_service_table()... ");
        crate::ke::syscall::ki_initialize_system_service_table();
        crate::kd::dbg_print_ok();
        crate::kd::dbg_print("\n");

        // Register BSP
        crate::kd::dbg_print("[KE] Registering Boot Processor... ");
        let prcb = crate::arch::x86_64::pcr::get_prcb();
        crate::ke::init::ki_register_processor(0, prcb);
        crate::kd::dbg_print_ok();
        crate::kd::dbg_print("\n");

        // =========================================================================
        // 8. EX Phase 0
        // =========================================================================
        crate::kd::dbg_print("[EX] Executive Phase 0 initialization... ");
        let ex_status = exp_init_system_phase0();
        if crate::nt::nt_success(ex_status) {
            crate::kd::dbg_print_ok();
            crate::kd::dbg_print("\n");
        } else {
            crate::kd::dbg_print_fail();
            crate::kd::dbg_print("\n");
            return ex_status;
        }

        // =========================================================================
        // 9. MM Phase 0
        // =========================================================================
        crate::kd::dbg_print("[MM] Memory Manager Phase 0... ");
        if crate::mm::mm_init_system(0) {
            crate::kd::dbg_print_ok();
            crate::kd::dbg_print("\n");
        } else {
            crate::kd::dbg_print_fail();
            crate::kd::dbg_print("\n");
        }

        // Self-referencing PML4
        crate::kd::dbg_print("[MM] Initializing self-referencing PML4... ");
        if crate::mm::init::mm_init_self_mapping() {
            crate::kd::dbg_print_ok();
            crate::kd::dbg_print("\n");
            crate::kd::dbg_print("       PML4[493] = self-reference entry (PXI_SELF)\n");
        } else {
            crate::kd::dbg_print_fail();
            crate::kd::dbg_print("\n");
        }

        // Hyperspace (требует self-referencing PML4)
        // Нужно для zero page thread и временных маппингов
        crate::mm::init::mm_init_hyperspace_after_selfmap();

        // =========================================================================
        // 10. Handle Tables
        // =========================================================================
        exp_initialize_handle_tables();

        // =========================================================================
        // 11. OB Phase 0
        // =========================================================================
        crate::kd::dbg_print("[OB] Object Manager Phase 0... ");
        if crate::ob::ob_init_system() {
            crate::kd::dbg_print_ok();
            crate::kd::dbg_print("\n");
            crate::kd::dbg_print("       Created types (Phase 0): Type, Directory, SymbolicLink\n");
        } else {
            crate::kd::dbg_print_fail();
            crate::kd::dbg_print("\n");
        }

        // =========================================================================
        // 12. SE Phase 0 - Security Reference Monitor
        // =========================================================================
        crate::kd::dbg_print("[SE] Security Manager Phase 0... ");
        if crate::se::se_init_system(0) {
            crate::kd::dbg_print_ok();
            crate::kd::dbg_print("\n");
            crate::kd::dbg_print("       Created types: Token\n");
            crate::kd::dbg_print("       Created: System token, well-known SIDs\n");
        } else {
            crate::kd::dbg_print_fail();
            crate::kd::dbg_print("\n");
        }

        // =========================================================================
        // 13. PS Phase 0 (+ Phase1 thread создается внутри!)
        // =========================================================================
        crate::kd::dbg_print("[PS] Process Manager Phase 0... ");
        if crate::ps::ps_init_system(0) {
            crate::kd::dbg_print_ok();
            crate::kd::dbg_print("\n");
            crate::kd::dbg_print("       Created types: Process, Thread\n");
            crate::kd::dbg_print("       Created: Idle Process (PID 0), System Process (PID 4)\n");
            crate::kd::dbg_print("       Created: Phase1 initialization thread\n");
        } else {
            crate::kd::dbg_print_fail();
            crate::kd::dbg_print("\n");
        }

        // =========================================================================
        // 13. IO Phase 0
        // =========================================================================
        crate::kd::dbg_print("[IO] I/O Manager Phase 0... ");
        if crate::io::io_init_system(0) {
            crate::kd::dbg_print_ok();
            crate::kd::dbg_print("\n");
            crate::kd::dbg_print("       Created types: Device, Driver, File\n");
        } else {
            crate::kd::dbg_print_fail();
            crate::kd::dbg_print("\n");
        }

        STATUS_SUCCESS
    }
}

/// Проверяет завершена ли Phase 0
pub fn exp_phase0_complete() -> bool {
    unsafe { PHASE0_INITIALIZED }
}

/// Проверяет завершена ли Phase 1
pub fn exp_phase1_complete() -> bool {
    unsafe { PHASE1_INITIALIZED }
}

// =============================================================================
// Resource спинлок
// =============================================================================

/// Получает Resource spinlock
#[inline]
pub fn exp_acquire_resource_lock() -> u8 {
    crate::ke::spinlock::ke_acquire_spin_lock(&EXP_RESOURCE_SPIN_LOCK)
}

/// Освобождает Resource spinlock
#[inline]
pub fn exp_release_resource_lock(old_irql: u8) {
    crate::ke::spinlock::ke_release_spin_lock(&EXP_RESOURCE_SPIN_LOCK, old_irql);
}

// =============================================================================
// Phase 1 Initialization
// =============================================================================

/// Phase1Initialization
///
/// Главная функция Phase 1 инициализации. Вызывается как start_routine
/// системного потока, созданного в Phase 0.
///
/// IRQL понижен до PASSIVE_LEVEL в `psp_system_thread_startup` (NT way).
/// После завершения инициализации поток превращается в MmZeroPageThread.
///
/// # Arguments
/// * `context` - указатель на LOADER_PARAMETER_BLOCK
///
/// # Safety
/// - Вызывается только один раз
/// - context должен быть валидным указателем на LPB
///
/// Источники:
/// - NT6.1: ex/init.c (Phase1Initialization)
/// - ReactOS: ntoskrnl/ex/init.c:925-960
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn phase1_initialization(context: crate::nt::PVOID) {
    unsafe {
        // IRQL = PASSIVE_LEVEL (понижен в psp_system_thread_startup)

        let lpb = context as *const ntldr::LOADER_PARAMETER_BLOCK;
        if lpb.is_null() {
            crate::kd::dbg_print("[PHASE 1] ERROR: context (LPB) is NULL\n");
            crate::ke::bugcheck::ke_bug_check(0x7B); // INACCESSIBLE_BOOT_DEVICE
        }

        // Вызываем основную логику инициализации
        phase1_initialization_discard(&*lpb);

        // Вывод завершения Phase 1
        crate::kd::dbg_print("\n");
        crate::kd::dbg_print_colored("[PHASE 1]", crate::kd::BV_COLOR_LIGHT_GREEN);
        crate::kd::dbg_print(" Complete\n");

        crate::kd::dbg_print("\n");
        crate::kd::dbg_print(
            "================================================================================\n",
        );
        crate::kd::dbg_print_colored("[KERNEL]", crate::kd::BV_COLOR_LIGHT_CYAN);
        crate::kd::dbg_print(" XenOS kernel initialized successfully!\n");
        crate::kd::dbg_print(
            "================================================================================\n",
        );

        // В NT поток Phase 1 не завершается, а превращается в MmZeroPageThread.
        // Это специальный системный поток, который обнуляет свободные страницы памяти.
        crate::mm::mm_zero_page_thread(); // Никогда не возвращает
    }
}

/// Phase1InitializationDiscard
///
/// Основная логика Phase 1 инициализации (discardable секция в NT).
/// Инициализирует подсистемы в NT-подобном порядке.
///
/// Порядок инициализации (адаптированный под XenOS):
/// 1. ExpInitializationPhase = 1
/// 2. HAL Phase 1 (APIC, таймеры)
/// 3. Object Manager Phase 1
/// 4. Executive Phase 1 (worker threads)
/// 5. Memory Manager Phase 1
/// 6. SYSCALL/SYSRET
/// 7. I/O Manager Phase 1
/// 8. Process Manager Phase 1
/// 9. ExpInitializationPhase = 2
///
/// Источники:
/// - NT6.1: ex/init.c (Phase1InitializationDiscard)
/// - ReactOS: ntoskrnl/ex/init.c:968-1342
unsafe fn phase1_initialization_discard(lpb: &ntldr::LOADER_PARAMETER_BLOCK) {
    unsafe {
        use crate::kd::BV_COLOR_LIGHT_GREEN;
        use crate::kd::dbg_print;
        use crate::kd::dbg_print_colored;
        use crate::kd::dbg_print_fail;
        use crate::kd::dbg_print_hex;
        use crate::kd::dbg_print_num;
        use crate::kd::dbg_print_ok;
        use crate::kd::dbg_print_skip;

        dbg_print("\n");
        dbg_print_colored("[PHASE 1]", BV_COLOR_LIGHT_GREEN);
        dbg_print(" Main Initialization (at PASSIVE_LEVEL)\n");
        dbg_print(
            "--------------------------------------------------------------------------------\n",
        );

        // =========================================================================
        // 1. Устанавливаем фазу инициализации
        // =========================================================================
        exp_set_initialization_phase(1);

        // =========================================================================
        // 2. HAL MMIO region (static page tables for APIC/IOAPIC)
        // ВАЖНО: должен быть до HAL Phase 1!
        // =========================================================================
        dbg_print("[MM] Initializing HAL MMIO region... ");
        if crate::mm::hal_mmio::mm_init_hal_mmio() {
            dbg_print_ok();
            dbg_print("\n");
            dbg_print("       HAL_LOCAL_APIC_BASE = 0xFFFFFFFFFFFE0000\n");
            dbg_print("       HAL_IOAPIC_BASE     = 0xFFFFFFFFFFFE1000\n");
        } else {
            dbg_print_fail();
            dbg_print("\n");
            dbg_print("       WARNING: HAL MMIO init failed, HAL Phase 1 may fail\n");
        }

        // =========================================================================
        // 3. HAL Phase 1 (APIC, таймеры)
        // =========================================================================
        dbg_print("[HAL] HAL Phase 1 (APIC, timers)... ");
        if crate::hal::hal_init_system(1) {
            dbg_print_ok();
            dbg_print("\n");
        } else {
            dbg_print_fail();
            dbg_print("\n");
        }

        // =========================================================================
        // TODO: Boot Video (InbvDriverInitialize)
        // TODO: Power Subsystem Phase 0 (PoInitSystem)
        // TODO: SMP (KeStartAllProcessors)
        // =========================================================================

        // =========================================================================
        // System Time (HalQueryRealTimeClock → ke_initialize_time)
        // =========================================================================
        dbg_print("[TIME] Reading RTC and initializing system time... ");
        if let Some(tf) = crate::ke::time::ke_initialize_time() {
            dbg_print_ok();
            dbg_print("\n");
            dbg_print("       RTC Time: ");
            let mut time_buf = [0u8; 32];
            dbg_print(crate::hal::rtc::format_time_fields(&tf, &mut time_buf));
            dbg_print(" UTC\n");
        } else {
            dbg_print_skip();
            dbg_print(" (RTC read failed, using fallback)\n");
        }

        // =========================================================================
        // 4. Object Manager Phase 1
        // =========================================================================
        dbg_print("[OB] Object Manager Phase 1... ");
        if crate::ob::ob_init_system() {
            dbg_print_ok();
            dbg_print("\n");
            dbg_print("       Namespace: created \\, \\ObjectTypes, \\KernelObjects\n");
            dbg_print(
                "       Namespace: created \\Device, \\Driver, \\FileSystem, \\??, \\GLOBAL??\n",
            );
            dbg_print("       ObjectTypes: populated from OBP_OBJECT_TYPES\n");
        } else {
            dbg_print_fail();
            dbg_print("\n");
        }

        // =========================================================================
        // 5. Executive Phase 1 (worker threads, callbacks)
        // =========================================================================
        dbg_print("[EX] Executive Phase 1... ");
        let ex_status = exp_init_system_phase1();
        if crate::nt::nt_success(ex_status) {
            dbg_print_ok();
            dbg_print("\n");
        } else {
            dbg_print_fail();
            dbg_print("\n");
        }

        // =========================================================================
        // TODO: Kernel Phase 1 (KeInitSystem)
        // TODO: Debugger Phase 1 (KdInitSystem)
        // TODO: Security Phase 1 (SeInitSystem)
        // =========================================================================

        // =========================================================================
        // 6. Memory Manager Phase 1
        // =========================================================================
        dbg_print("[MM] Memory Manager Phase 1... ");
        if crate::mm::mm_init_system(1) {
            dbg_print_ok();
            dbg_print("\n");
            dbg_print("       Available pages: ");
            dbg_print_num(crate::mm::mm_get_available_pages() as u64);
            dbg_print("\n");
        } else {
            dbg_print_fail();
            dbg_print("\n");
        }

        // =========================================================================
        // TODO: NLS (ExpInitNls)
        // TODO: Cache Manager (CcInitializeCacheManager)
        // TODO: Registry Phase 1 (CmInitSystem1)
        // TODO: FS Runtime (FsRtlInitSystem)
        // TODO: PnP Manager (PpInitSystem)
        // TODO: LPC (LpcInitSystem)
        // =========================================================================

        // =========================================================================
        // 7. SYSCALL/SYSRET MSR
        // =========================================================================
        dbg_print("[KE] Initializing SYSCALL (MSR: LSTAR, STAR, FMASK)... ");
        crate::ke::syscall::ki_initialize_syscall();
        dbg_print_ok();
        dbg_print("\n");

        // =========================================================================
        // 8. I/O Manager Phase 1
        // =========================================================================
        dbg_print("[IO] I/O Manager Phase 1... ");
        if crate::io::io_init_system(1) {
            dbg_print_ok();
            dbg_print("\n");
        } else {
            dbg_print_fail();
            dbg_print("\n");
        }

        // =========================================================================
        // TODO: MM Phase 2 (MmInitSystem(2))
        // TODO: Power Phase 1 (PoInitSystem(1))
        // =========================================================================

        // =========================================================================
        // 9. Process Manager Phase 1
        // =========================================================================
        dbg_print("[PS] Process Manager Phase 1... ");
        if crate::ps::ps_init_system(1) {
            dbg_print_ok();
            dbg_print("\n");
        } else {
            dbg_print_fail();
            dbg_print("\n");
        }

        // =========================================================================
        // 9.1. PS Test Threads (feature ps-test)
        // =========================================================================
        #[cfg(feature = "ps-test")]
        {
            dbg_print("[PS] Starting test threads (ps-test feature)...\n");
            crate::ps::test::ps_start_test_threads();
        }

        // =========================================================================
        // 10. ACPI Manager initialization
        // =========================================================================
        // 10a. Early init (TablesReady) — для таблиц SPCR/MCFG/MADT/HPET
        dbg_print("[ACPI] ACPI Manager init_early... ");
        if lpb.AcpiTablePhysical != 0 {
            let status = crate::acpi::acpi_init_early(lpb.AcpiTablePhysical as usize);
            if status >= 0 {
                dbg_print_ok();
                dbg_print("\n");
                dbg_print("       RSDP: 0x");
                dbg_print_hex(lpb.AcpiTablePhysical);
                dbg_print("\n");
            } else {
                dbg_print_fail();
                dbg_print("\n");
            }
        } else {
            dbg_print_skip();
            dbg_print(" (no RSDP)\n");
        }

        // 10b. Full init (PlatformReady + AmlReady) — для PnP enumeration
        if crate::acpi::acpi_is_ready(crate::acpi::AcpiReadyLevel::TablesReady) {
            dbg_print("[ACPI] ACPI Manager init_full... ");
            let status = crate::acpi::acpi_init_full();
            if status >= 0 {
                dbg_print_ok();
                dbg_print("\n");
            } else {
                dbg_print_fail();
                dbg_print(" (Interpreter may be unavailable)\n");
            }
        }

        // =========================================================================
        // 11. PnP Phase 2 (device enumeration)
        // =========================================================================
        // Теперь когда ACPI AmlReady, запускаем enumeration дерева устройств
        dbg_print("[PnP] PnP Manager Phase 2... ");
        crate::io::pnpmgr::pnp_init2();
        dbg_print_ok();
        dbg_print("\n");

        // =========================================================================
        // TODO: Free Loader Block (MmFreeLoaderBlock)
        // TODO: Security RM Phase 1 (SeRmInitPhase1)
        // TODO: Start SMSS (ExpLoadInitialProcess)
        // =========================================================================

        // =========================================================================
        // 12. Завершение Phase 1
        // =========================================================================
        exp_set_initialization_phase(2);
    }
}
