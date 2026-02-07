//! XenCore Kernel Entry Point
//!
//! Точка входа ядра NT-совместимой ОС XenOS.
//! Вызывается из winload.efi после подготовки LOADER_PARAMETER_BLOCK.
//!
//! # Архитектура инициализации (NT way)
//!
//! ```text
//! KiSystemStartup (entry point)
//!   ├── ki_early_cpu_init()         - CPU control registers, SSE
//!   ├── ki_init_kd()                - Kernel Debugger (serial)
//!   ├── ki_init_hhdm()              - HHDM для физ. памяти
//!   ├── ki_init_pool()              - Kernel memory pool
//!   ├── ki_init_bootvid()           - Boot video (INBV/BOOTVID)
//!   ├── ki_init_arch()              - GDT, TSS, IDT, PCR
//!   ├── ki_initialize_idle_process()- Idle Process (PID 0)
//!   ├── ki_initialize_idle_thread() - Idle Thread (TID 0)
//!   └── ki_switch_to_boot_stack()   - ASM: переключение на boot stack
//!         └── ki_system_startup_boot_stack()
//!               ├── ki_initialize_kernel()
//!               │     └── exp_initialize_executive() - Phase 0
//!               │           ├── HAL(0) + STI
//!               │           ├── KE, EX, MM, OB
//!               │           ├── PS(0) → создаёт Phase1 thread
//!               │           └── IO(0)
//!               └── ki_idle_loop()  - Scheduler entry (никогда не возвращает)
//! ```
//!
//! Phase 1 выполняется в системном потоке на PASSIVE_LEVEL:
//! ```text
//! phase1_initialization (ex/init.rs)
//!   ├── HAL MMIO + HAL(1)
//!   ├── OB, EX(1), MM(1)
//!   ├── SYSCALL MSR
//!   ├── IO(1), PS(1)
//!   └── mm_zero_page_thread() - поток не завершается, переиспользуется
//! ```

#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

use core::cell::UnsafeCell;
use core::panic::PanicInfo;

extern crate alloc;

// PE-экспорты в стиле Windows NT (для hal.dll и драйверов).
// Разбиты по подсистемам: exports/ke.rs, exports/mm.rs, etc.
mod exports;

use ntoskrnl::imports::bootvid::FramebufferInfo;
// NT-совместимый LoaderParameterBlock из ntldr crate
use ntldr::LOADER_PARAMETER_BLOCK;
use ntoskrnl::arch::x86_64::cpu;
use ntoskrnl::arch::x86_64::gdt;
use ntoskrnl::arch::x86_64::idt;
use ntoskrnl::arch::x86_64::pcr;
use ntoskrnl::arch::x86_64::tss;
use ntoskrnl::bootlog;
use ntoskrnl::ex;
use ntoskrnl::inbv;
use ntoskrnl::kd::BV_COLOR_LIGHT_GREEN;
use ntoskrnl::kd::dbg_print;
use ntoskrnl::kd::dbg_print_colored;
use ntoskrnl::kd::dbg_print_fail;
use ntoskrnl::kd::dbg_print_hex;
use ntoskrnl::kd::dbg_print_num;
use ntoskrnl::kd::dbg_print_ok;
use ntoskrnl::kd::dbg_print_size;
use ntoskrnl::kd::dbg_print_skip;
use ntoskrnl::kd::kd_init_system;
use ntoskrnl::ke;
use ntoskrnl::mm;

// =============================================================================
// KiSystemStartup sub-functions
// =============================================================================

/// Ранняя инициализация CPU: SSE, control registers.
/// Должна вызываться до любых операций с памятью/копированием.
unsafe fn ki_early_cpu_init() {
    unsafe {
        cpu::setup_control_registers();
    }
}

/// Инициализация Kernel Debugger (serial port).
unsafe fn ki_init_kd() {
    kd_init_system();
}

/// Инициализация HHDM в подсистемах.
unsafe fn ki_init_hhdm(lpb: &LOADER_PARAMETER_BLOCK) {
    unsafe {
        mm::init::mm_set_hhdm_offset(lpb.HhdmOffset);
        ntoskrnl::acpi::handler::init_hhdm_offset(lpb.HhdmOffset as usize);
    }
}

/// Инициализация kernel memory pool.
/// Возвращает (heap_start, heap_size) для диагностики.
unsafe fn ki_init_pool(lpb: &LOADER_PARAMETER_BLOCK) -> (usize, usize) {
    unsafe {
        // Pool уже выбран winload'ом: PoolBase/PoolSize
        // Конвертируем физический адрес в виртуальный через HHDM
        let pool_phys = lpb.PoolBase;
        let pool_size = lpb.PoolSize as usize;

        if pool_phys == 0 || pool_size == 0 {
            return (0, 0);
        }

        // Виртуальный адрес через HHDM
        let pool_virt = (lpb.HhdmOffset + pool_phys) as usize;

        ex::pool::exp_init_pool_system(pool_virt, pool_size);

        (pool_virt, pool_size)
    }
}

/// Инициализация boot video (INBV/BOOTVID).
///
/// Аргумент `sos_mode` включает режим /SOS (вывод лога загрузки на экран).
unsafe fn ki_init_bootvid(lpb: &LOADER_PARAMETER_BLOCK, sos_mode: bool) -> bool {
    if lpb.FramebufferInfo.Present == 0 {
        return false;
    }

    let fb_phys = lpb.FramebufferInfo.FrameBufferBase;
    let width = lpb.FramebufferInfo.HorizontalResolution as usize;
    let height = lpb.FramebufferInfo.VerticalResolution as usize;
    // PixelsPerScanLine * 4 bytes = pitch
    let pitch = (lpb.FramebufferInfo.PixelsPerScanLine as usize) * 4;
    let bpp = lpb.FramebufferInfo.BitsPerPixel as u8;

    if fb_phys == 0 || width == 0 || height == 0 {
        return false;
    }

    // Framebuffer доступен через HHDM
    let fb_virt = (lpb.HhdmOffset + fb_phys) as *mut u32;

    let fb_info = FramebufferInfo {
        address: fb_virt,
        width,
        height,
        pitch,
        bpp,
    };

    if inbv::inbv_driver_initialize(&fb_info) {
        // /SOS режим: вывод бутлога на экран (аналог NT /SOS)
        if sos_mode {
            bootlog::bootlog_enable(true);
            bootlog::bootlog_init();
            // Включаем KD screen провайдер для вывода dbg_print на экран
            inbv::inbv_enable_display_string(true);
        } else {
            // Обычный режим: INBV только для BSOD, вывод строк выключен
            inbv::inbv_enable_display_string(false);
        }
        true
    } else {
        false
    }
}

/// Архитектурная инициализация: GDT, TSS, IDT, PCR.
unsafe fn ki_init_arch() {
    unsafe {
        // TSS с IST стеками
        let tss = &mut *BSP_TSS.get();
        let ist1_top = IST1_STACK.get() as u64 + tss::IST_STACK_SIZE as u64;
        let ist2_top = IST2_STACK.get() as u64 + tss::IST_STACK_SIZE as u64;
        tss.set_ist1(ist1_top);
        tss.set_ist2(ist2_top);

        // GDT
        let gdt = &mut *BSP_GDT.get();
        gdt.set_tss(BSP_TSS.get() as u64);
        let gdt_ptr = gdt.pointer();
        gdt::load_gdt(&gdt_ptr);
        gdt::reload_segments(gdt::selector::KGDT64_R0_CODE, gdt::selector::KGDT64_R0_DATA);
        gdt::load_tss(gdt::selector::KGDT64_SYS_TSS);

        // PCR/PRCB
        let pcr = &mut *BSP_PCR.get();
        pcr.init(0);

        // IDT с обработчиками
        let idt_table = &mut *IDT.get();
        pcr.idt_base = idt_table as *mut _ as *mut idt::IdtEntry;

        let handlers = ke::trap::get_exception_handlers();
        idt::init_exception_handlers(idt_table, &handlers);

        let apic_handlers = ke::trap::get_apic_handlers();
        idt::init_apic_handlers(idt_table, &apic_handlers);

        let swint_handlers = ke::trap::get_swint_handlers();
        idt::init_software_interrupt_handlers(idt_table, &swint_handlers);

        // Device interrupt handlers for IRQ 0-23 (vectors 0x30-0x47)
        let device_handlers = ke::trap::get_device_interrupt_handlers();
        idt::init_device_interrupt_handlers(idt_table, &device_handlers);

        let idt_ptr = idt_table.pointer();
        idt::load_idt(&idt_ptr);

        // GS base для PCR
        cpu::setup_gs_base(pcr);

        // Начальный IRQL = HIGH_LEVEL, прерывания выключены
        ntoskrnl::hal::irql::disable_interrupts();
        ntoskrnl::hal::irql::hal_set_irql(ntoskrnl::hal::irql::HIGH_LEVEL);
    }
}

// =============================================================================
// Global Allocator
// =============================================================================

#[global_allocator]
static ALLOCATOR: ntoskrnl::ex::pool::KernelAllocator = ntoskrnl::ex::pool::KernelAllocator;

// =============================================================================
// Per-CPU Data wrapper
// =============================================================================

#[repr(transparent)]
struct SyncUnsafeCell<T>(UnsafeCell<T>);
unsafe impl<T> Sync for SyncUnsafeCell<T> {}

impl<T> SyncUnsafeCell<T> {
    const fn new(value: T) -> Self {
        Self(UnsafeCell::new(value))
    }

    #[inline]
    fn get(&self) -> *mut T {
        self.0.get()
    }
}

// =============================================================================
// Per-CPU Data (BSP)
// =============================================================================

/// GDT для BSP
static BSP_GDT: SyncUnsafeCell<gdt::Gdt> = SyncUnsafeCell::new(gdt::Gdt::new());

/// TSS для BSP
static BSP_TSS: SyncUnsafeCell<tss::Tss64> = SyncUnsafeCell::new(tss::Tss64::new());

/// IDT (общая)
static IDT: SyncUnsafeCell<idt::Idt> = SyncUnsafeCell::new(idt::Idt::new());

/// PCR для BSP
static BSP_PCR: SyncUnsafeCell<pcr::KPCR> = SyncUnsafeCell::new(pcr::KPCR::new());

/// Стеки для IST
static IST1_STACK: SyncUnsafeCell<[u8; tss::IST_STACK_SIZE]> =
    SyncUnsafeCell::new([0; tss::IST_STACK_SIZE]);
static IST2_STACK: SyncUnsafeCell<[u8; tss::IST_STACK_SIZE]> =
    SyncUnsafeCell::new([0; tss::IST_STACK_SIZE]);

// =============================================================================
// Kernel Entry Point (KiSystemStartup определён выше)
// =============================================================================

// Единственная точка входа: KiSystemStartup (вызывается из winload.efi)

// =============================================================================
// Вывод по serial в стиле WinNT должен идти через KD (DbgPrint/KdPrint).
// Прямая запись в COM-порты из точки входа не используется.

/// NT-совместимая точка входа ядра (Windows NT `KiSystemStartup`).
///
/// Entry point для PE формата. Вызывается из winload.efi после подготовки
/// LOADER_PARAMETER_BLOCK и ExitBootServices.
///
/// # Архитектура
///
/// winload.efi уже настроил:
/// - Page tables с HHDM (0xFFFF800000000000) и kernel mapping (0xFFFFF80000000000)
/// - LOADER_PARAMETER_BLOCK с информацией о системе
/// - ExitBootServices вызван, UEFI Boot Services недоступны
#[unsafe(no_mangle)]
pub extern "win64" fn KiSystemStartup(loader_block: *const LOADER_PARAMETER_BLOCK) -> ! {
    unsafe {
        // =====================================================================
        // Фаза -1: Критическая ранняя инициализация (до любого вывода)
        // =====================================================================

        // CPU control registers (SSE, etc.) - до любых операций с памятью!
        ki_early_cpu_init();

        // Проверка LoaderBlock
        if loader_block.is_null() {
            // Ранний вывод делаем через KD, без прямой записи в COM.
            kd_init_system();
            dbg_print("[KERNEL ENTRY] ERROR: LoaderBlock is NULL!\n");
            hcf(); // Halt and Catch Fire
        }
        let lpb = &*loader_block;

        // HHDM должен быть настроен первым для доступа к памяти
        ki_init_hhdm(lpb);

        // Сохраняем ACPI RSDP для последующей конфигурации KD (SPCR).
        ntoskrnl::kd::data::kd_set_acpi_rsdp_physical(lpb.AcpiTablePhysical);

        // KD (serial) для диагностики
        ki_init_kd();

        // Парсим Load Options для /DEBUG и /SOS
        let (_debug_mode, sos_mode) = ntoskrnl::kd::data::kd_init_from_load_options(
            lpb.LoadOptions,
            lpb.LoadOptionsLength,
        );

        // =====================================================================
        // Boot video (INBV/BOOTVID) - инициализируем РАНЬШЕ остального вывода,
        // чтобы при /SOS весь лог шёл на экран с самого начала
        // =====================================================================
        let vid_ok = ki_init_bootvid(lpb, sos_mode);

        // =====================================================================
        // Фаза 0: Ранняя инициализация
        // =====================================================================

        // Banner
        dbg_print("\n");
        dbg_print("========================================\n");
        dbg_print("  XenOS Kernel v0.0.1-dev\n");
        dbg_print("  NT-compatible Operating System\n");
        dbg_print("========================================\n");
        dbg_print("\n");

        // Информация о LoaderBlock
        dbg_print("[LOADER_PARAMETER_BLOCK]\n");
        dbg_print("  Version:     ");
        dbg_print_num(lpb.OsMajorVersion as u64);
        dbg_print(".");
        dbg_print_num(lpb.OsMinorVersion as u64);
        dbg_print("\n");
        dbg_print("  Kernel:      0x");
        dbg_print_hex(lpb.KernelBase);
        dbg_print(" (");
        dbg_print_size(lpb.KernelSize as u64);
        dbg_print(")\n");
        dbg_print("  HHDM:        0x");
        dbg_print_hex(lpb.HhdmOffset);
        dbg_print("\n");
        if lpb.AcpiTablePhysical != 0 {
            dbg_print("  ACPI RSDP:   0x");
            dbg_print_hex(lpb.AcpiTablePhysical);
            dbg_print("\n");
        }
        // Framebuffer info
        if vid_ok {
            dbg_print("  Framebuffer: 0x");
            dbg_print_hex(lpb.FramebufferInfo.FrameBufferBase);
            dbg_print(" (");
            dbg_print_num(lpb.FramebufferInfo.HorizontalResolution as u64);
            dbg_print("x");
            dbg_print_num(lpb.FramebufferInfo.VerticalResolution as u64);
            dbg_print(")\n");
        }
        dbg_print("\n");

        // Pool initialization
        dbg_print("[POOL] Initializing kernel memory pool... ");
        let (heap_start, heap_size) = ki_init_pool(lpb);
        if heap_start != 0 {
            dbg_print_ok();
            dbg_print("\n");
            dbg_print("       Heap start: 0x");
            dbg_print_hex(heap_start as u64);
            dbg_print("\n");
            dbg_print("       Heap size:  ");
            dbg_print_size(heap_size as u64);
            dbg_print("\n");
        } else {
            dbg_print_fail();
            dbg_print("\n");
        }

        // Architecture init (GDT, TSS, IDT, PCR)
        dbg_print("[ARCH] Initializing GDT, TSS, IDT, PCR... ");
        ki_init_arch();
        dbg_print_ok();
        dbg_print("\n");

        dbg_print("\n");
        dbg_print_colored("[PHASE 0]", BV_COLOR_LIGHT_GREEN);
        dbg_print(" Subsystem Initialization\n");
        dbg_print(
            "--------------------------------------------------------------------------------\n",
        );

        // Устанавливаем глобальный указатель на LoaderBlock (для MM и др. подсистем)
        ke::globals::ke_set_loader_block(lpb);

        // =====================================================================
        // NT архитектура: Idle Process/Thread создаются ДО Executive init
        // =====================================================================
        dbg_print("[KE] Initializing Idle Process... ");
        ke::idle::ki_initialize_idle_process();
        dbg_print_ok();
        dbg_print("\n");

        dbg_print("[KE] Initializing Idle Thread... ");
        let prcb = &raw mut (*BSP_PCR.get()).prcb;
        if ke::idle::ki_initialize_idle_thread(prcb, 0) {
            dbg_print_ok();
            dbg_print("\n");
            dbg_print("       Idle Thread: 0x");
            dbg_print_hex((*prcb).idle_thread as u64);
            dbg_print(" (priority=HIGH, will be lowered after Executive init)\n");
        } else {
            dbg_print_fail();
            dbg_print("\n");
            hcf();
        }

        // =====================================================================
        // Режим тестирования ядра (feature test-kernel)
        // Запускаем ДО переключения на boot stack
        // =====================================================================
        #[cfg(feature = "test-kernel")]
        {
            // Временно используем старую логику для тестов
            ntoskrnl::hal::irql::hal_set_irql(ntoskrnl::hal::irql::APC_LEVEL);
            ki_phase0_init(lpb);
            ke::idle::ki_set_idle_thread_priority(prcb);

            dbg_print("\n");
            dbg_print("========================================\n");
            dbg_print("  KERNEL TEST MODE\n");
            dbg_print("========================================\n");

            // Запуск тестов - функция не возвращается, завершает QEMU
            ntoskrnl::test::run_all_test_suites(ntoskrnl::test::get_all_tests());
        }

        // =====================================================================
        // NT архитектура: KiSwitchToBootStack
        //
        // После создания Idle Process/Thread переключаемся на boot stack
        // и продолжаем инициализацию в ki_system_startup_boot_stack.
        //
        // Порядок вызовов:
        // KiSwitchToBootStack (ASM)
        //   └── ki_system_startup_boot_stack (Rust)
        //         └── ki_initialize_kernel
        //               └── exp_initialize_executive (Phase 0)
        //                     └── ps_init_system(0) создаёт Phase 1 поток
        //         └── ki_idle_loop (никогда не возвращает)
        // =====================================================================

        // Получаем верхнюю границу стека Idle Thread
        let initial_stack = ke::idle::ki_get_idle_stack_top();

        // Переключаемся на boot stack и продолжаем инициализацию
        // Эта функция никогда не возвращает
        ke::init::ki_switch_to_boot_stack(initial_stack);
    }
}

fn hcf() -> ! {
    loop {
        unsafe {
            core::arch::asm!("hlt");
        }
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    use ntoskrnl::ke::bugcheck::bugcheck_codes;
    use ntoskrnl::ke::bugcheck::ke_bug_check_ex;

    unsafe {
        // Выключаем прерывания сразу
        core::arch::asm!("cli");

        // Выводим краткую информацию о panic перед вызовом KeBugCheckEx
        // Инициализируем KD (если ещё не был инициализирован)
        kd_init_system();
        dbg_print("\n*** Rust panic! ***\n");

        // Location
        if let Some(location) = info.location() {
            dbg_print("Location: ");
            dbg_print(location.file());
            dbg_print(":");
            dbg_print_num(location.line() as u64);
            dbg_print(":");
            dbg_print_num(location.column() as u64);
            dbg_print("\n");
        }

        // Message
        dbg_print("Message: ");
        struct PanicWriter;
        impl core::fmt::Write for PanicWriter {
            fn write_str(&mut self, s: &str) -> core::fmt::Result {
                dbg_print(s);
                Ok(())
            }
        }
        use core::fmt::Write;
        let _ = write!(PanicWriter, "{}", info.message());
        dbg_print("\n\n");

        // Получаем RIP из return address на стеке (приблизительно)
        let rip: u64;
        core::arch::asm!(
            "lea {}, [rip]",
            out(reg) rip,
            options(nomem, nostack)
        );

        // Вызываем KeBugCheckEx для полного дампа
        // KMODE_EXCEPTION_NOT_HANDLED:
        // Param1: Exception code (используем 0xDEAD для panic)
        // Param2: Address where exception occurred
        // Param3: 0
        // Param4: 0
        ke_bug_check_ex(
            bugcheck_codes::KMODE_EXCEPTION_NOT_HANDLED,
            0xDEAD_0000, // Специальный код для Rust panic
            rip as usize,
            0,
            0,
        );
    }
}
