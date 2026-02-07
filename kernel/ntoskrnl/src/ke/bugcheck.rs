//! KeBugCheck / KeBugCheckEx - обработка критических ошибок ядра
//!
//! Источники:
//! - NT5: ke/bugcheck.c, ke/amd64/bugcheck.c
//! - ReactOS: ke/bug.c

#![allow(dead_code)]

use core::sync::atomic::AtomicI32;
use core::sync::atomic::Ordering;

use crate::nt::ULONG_PTR;

/// Аналог `KeBugCheckCount` из NT: защита от повторного вывода/обработки bugcheck.
static KE_BUGCHECK_COUNT: AtomicI32 = AtomicI32::new(1);

// =============================================================================
// Crash Context (для расширенного вывода на BSOD)
// =============================================================================

/// Сохранённый контекст аварии (минимальный набор для stack-walk).
///
/// В NT подобную роль выполняют параметры bugcheck + сохранённые trap/exception кадры.
/// В XenOS мы дополнительно сохраняем RBP, чтобы делать RBP-chain stack walk.
#[repr(C)]
struct BUGCHECK_TRAP_CONTEXT {
    trap_code: u32,
    _pad: u32,
    error_code: u64,
    rip: u64,
    rsp: u64,
    rbp: u64,
}

/// Признак, что контекст установлен.
static BUGCHECK_TRAP_CONTEXT_VALID: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

/// Глобальный сохранённый контекст (одна авария = один BSOD).
static mut BUGCHECK_TRAP_CONTEXT: BUGCHECK_TRAP_CONTEXT = BUGCHECK_TRAP_CONTEXT {
    trap_code: 0,
    _pad: 0,
    error_code: 0,
    rip: 0,
    rsp: 0,
    rbp: 0,
};

/// Сохраняет контекст trap/exception для расширенного BSOD.
///
/// Используется trap stubs/handlers до вызова `KeBugCheckEx`.
///
/// # Safety
/// - Должно вызываться только в аварийном пути (bugcheck), без гонок “нормального” исполнения.
pub(crate) fn ki_bugcheck_set_trap_context(
    trap_code: u32,
    error_code: u64,
    rip: u64,
    rsp: u64,
    rbp: u64,
) {
    unsafe {
        BUGCHECK_TRAP_CONTEXT.trap_code = trap_code;
        BUGCHECK_TRAP_CONTEXT.error_code = error_code;
        BUGCHECK_TRAP_CONTEXT.rip = rip;
        BUGCHECK_TRAP_CONTEXT.rsp = rsp;
        BUGCHECK_TRAP_CONTEXT.rbp = rbp;
        // Публикуем "валидность" последней, чтобы читатель видел целостный набор.
        core::sync::atomic::compiler_fence(Ordering::SeqCst);
        BUGCHECK_TRAP_CONTEXT_VALID.store(true, Ordering::SeqCst);
    }
}

// =============================================================================
// BugCheck коды (из NT bugcodes.h)
// =============================================================================

/// Коды критических ошибок (BugCheck codes)
pub mod bugcheck_codes {
    /// APC_INDEX_MISMATCH - несоответствие APC индекса
    pub const APC_INDEX_MISMATCH: u32 = 0x01;

    /// IRQL_NOT_LESS_OR_EQUAL - доступ к памяти на повышенном IRQL
    pub const IRQL_NOT_LESS_OR_EQUAL: u32 = 0x0A;

    /// IRQL_NOT_GREATER_OR_EQUAL
    pub const IRQL_NOT_GREATER_OR_EQUAL: u32 = 0x09;

    /// NO_USER_MODE_CONTEXT - нет контекста user mode
    pub const NO_USER_MODE_CONTEXT: u32 = 0x0E;

    /// SPIN_LOCK_ALREADY_OWNED - спинлок уже захвачен
    pub const SPIN_LOCK_ALREADY_OWNED: u32 = 0x0F;

    /// SPIN_LOCK_NOT_OWNED - спинлок не был захвачен
    pub const SPIN_LOCK_NOT_OWNED: u32 = 0x10;

    /// KMODE_EXCEPTION_NOT_HANDLED - необработанное исключение в kernel mode
    pub const KMODE_EXCEPTION_NOT_HANDLED: u32 = 0x1E;

    /// KERNEL_APC_PENDING_DURING_EXIT - APC pending при выходе
    pub const KERNEL_APC_PENDING_DURING_EXIT: u32 = 0x20;

    /// PAGE_FAULT_IN_NONPAGED_AREA - page fault в невыгружаемой памяти
    pub const PAGE_FAULT_IN_NONPAGED_AREA: u32 = 0x50;

    /// PROCESS_HAS_LOCKED_PAGES - процесс имеет заблокированные страницы
    pub const PROCESS_HAS_LOCKED_PAGES: u32 = 0x76;

    /// UNEXPECTED_KERNEL_MODE_TRAP - неожиданный trap в kernel mode
    pub const UNEXPECTED_KERNEL_MODE_TRAP: u32 = 0x7F;

    /// KERNEL_MODE_EXCEPTION_NOT_HANDLED
    pub const KERNEL_MODE_EXCEPTION_NOT_HANDLED: u32 = 0x8E;

    /// CRITICAL_PROCESS_DIED - критический процесс завершился
    pub const CRITICAL_PROCESS_DIED: u32 = 0xEF;

    /// MANUALLY_INITIATED_CRASH - ручной вызов crash
    pub const MANUALLY_INITIATED_CRASH: u32 = 0xE2;

    /// KERNEL_DATA_INPAGE_ERROR - ошибка чтения данных ядра
    pub const KERNEL_DATA_INPAGE_ERROR: u32 = 0x7A;

    /// KERNEL_STACK_INPAGE_ERROR - ошибка чтения стека ядра
    pub const KERNEL_STACK_INPAGE_ERROR: u32 = 0x77;

    /// INVALID_PROCESS_ATTACH_ATTEMPT - неверная попытка присоединения к процессу
    pub const INVALID_PROCESS_ATTACH_ATTEMPT: u32 = 0x05;

    /// INVALID_PROCESS_DETACH_ATTEMPT - неверная попытка отсоединения от процессу
    pub const INVALID_PROCESS_DETACH_ATTEMPT: u32 = 0x06;

    /// DRIVER_IRQL_NOT_LESS_OR_EQUAL - драйвер обратился к памяти на высоком IRQL
    pub const DRIVER_IRQL_NOT_LESS_OR_EQUAL: u32 = 0xD1;

    /// DRIVER_POWER_STATE_FAILURE - ошибка состояния питания драйвера
    pub const DRIVER_POWER_STATE_FAILURE: u32 = 0x9F;

    /// INTERNAL_POWER_ERROR - внутренняя ошибка питания
    pub const INTERNAL_POWER_ERROR: u32 = 0xA0;

    /// PFN_LIST_CORRUPT - повреждение PFN списка
    pub const PFN_LIST_CORRUPT: u32 = 0x4E;

    /// MEMORY_MANAGEMENT - ошибка управления памятью
    pub const MEMORY_MANAGEMENT: u32 = 0x1A;

    /// BAD_POOL_HEADER - поврежден заголовок пула
    pub const BAD_POOL_HEADER: u32 = 0x19;

    /// BAD_POOL_CALLER - неверный вызывающий пула
    pub const BAD_POOL_CALLER: u32 = 0xC2;

    /// SYSTEM_SERVICE_EXCEPTION - исключение в системном сервисе
    pub const SYSTEM_SERVICE_EXCEPTION: u32 = 0x3B;

    /// SYSTEM_THREAD_EXCEPTION_NOT_HANDLED - необработанное исключение системного потока
    pub const SYSTEM_THREAD_EXCEPTION_NOT_HANDLED: u32 = 0x7E;

    /// ATTEMPTED_WRITE_TO_READONLY_MEMORY - попытка записи в readonly память
    pub const ATTEMPTED_WRITE_TO_READONLY_MEMORY: u32 = 0xBE;

    /// KERNEL_SECURITY_CHECK_FAILURE - сбой проверки безопасности ядра
    pub const KERNEL_SECURITY_CHECK_FAILURE: u32 = 0x139;

    /// ATTEMPTED_SWITCH_FROM_DPC - попытка переключения контекста из DPC
    pub const ATTEMPTED_SWITCH_FROM_DPC: u32 = 0xB8;

    /// NLS_DATA_ERROR - ошибка инициализации NLS данных
    /// Param1: NTSTATUS код ошибки
    /// Param2: Адрес NLS данных (или 0)
    /// Param3: Размер NLS данных (или 0)
    /// Param4: Зарезервировано
    pub const NLS_DATA_ERROR: u32 = 0x400;

    /// MISMATCHED_HAL - несовместимость HAL и ядра
    /// Возникает когда версия HAL не соответствует версии ядра
    /// или LoaderBlock невалиден
    pub const MISMATCHED_HAL: u32 = 0x79;

    /// HAL_INITIALIZATION_FAILED - сбой инициализации HAL
    /// Возникает когда hal_init_system(0) возвращает ошибку
    pub const HAL_INITIALIZATION_FAILED: u32 = 0x5C;

    /// PHASE0_INITIALIZATION_FAILED - сбой Phase 0 инициализации
    /// Возникает когда ExpInitializeExecutive возвращает ошибку
    /// Param1: NTSTATUS код ошибки
    pub const PHASE0_INITIALIZATION_FAILED: u32 = 0x31;
}

// =============================================================================
// Trap коды для UNEXPECTED_KERNEL_MODE_TRAP
// =============================================================================

/// Коды trap для Parameter1 при UNEXPECTED_KERNEL_MODE_TRAP
pub mod trap_codes {
    pub const DIVIDE_ERROR: u32 = 0;
    pub const DEBUG: u32 = 1;
    pub const NMI: u32 = 2;
    pub const BREAKPOINT: u32 = 3;
    pub const OVERFLOW: u32 = 4;
    pub const BOUND_RANGE: u32 = 5;
    pub const INVALID_OPCODE: u32 = 6;
    pub const DEVICE_NOT_AVAILABLE: u32 = 7;
    pub const DOUBLE_FAULT: u32 = 8;
    pub const INVALID_TSS: u32 = 10;
    pub const SEGMENT_NOT_PRESENT: u32 = 11;
    pub const STACK_FAULT: u32 = 12;
    pub const GENERAL_PROTECTION: u32 = 13;
    pub const PAGE_FAULT: u32 = 14;
    pub const X87_FPU_ERROR: u32 = 16;
    pub const ALIGNMENT_CHECK: u32 = 17;
    pub const MACHINE_CHECK: u32 = 18;
    pub const SIMD_FP: u32 = 19;
}

// =============================================================================
// KeBugCheck / KeBugCheckEx
// =============================================================================

/// KeBugCheck - вызов критической ошибки без параметров
///
/// Эквивалент KeBugCheckEx(code, 0, 0, 0, 0)
#[inline(never)]
pub fn ke_bug_check(bug_check_code: u32) -> ! {
    ke_bug_check_ex(bug_check_code, 0, 0, 0, 0)
}

/// KeBugCheckEx - вызов критической ошибки с параметрами
///
/// Выводит информацию о сбое и останавливает систему.
///
/// # Arguments
/// * `bug_check_code` - код ошибки из bugcheck_codes
/// * `param1` - параметр 1 (зависит от кода)
/// * `param2` - параметр 2 (зависит от кода)
/// * `param3` - параметр 3 (зависит от кода)
/// * `param4` - параметр 4 (зависит от кода)
#[inline(never)]
pub fn ke_bug_check_ex(
    bug_check_code: u32,
    param1: ULONG_PTR,
    param2: ULONG_PTR,
    param3: ULONG_PTR,
    param4: ULONG_PTR,
) -> ! {
    unsafe {
        // Отключаем прерывания немедленно
        core::arch::asm!("cli", options(nomem, nostack));

        // Вызываем внутреннюю функцию вывода
        ki_bug_check_dispatch(bug_check_code, param1, param2, param3, param4);
    }
}

/// Внутренняя функция вывода BugCheck
unsafe fn ki_bug_check_dispatch(
    bug_check_code: u32,
    param1: ULONG_PTR,
    param2: ULONG_PTR,
    param3: ULONG_PTR,
    param4: ULONG_PTR,
) -> ! {
    unsafe {
        use crate::hal::pit::ke_stall_execution_processor;
        use crate::inbv;
        use crate::kd;
        use crate::ke::bugcheck_callbacks::ki_do_bugcheck_callbacks;
        use crate::ke::bugtext::bugcheck_message_id;
        use crate::ke::bugtext::ke_get_bug_message_text;

        // NT: pre-BSOD DbgPrint "Fatal System Error" (только если debugger enabled).
        if bug_check_code != bugcheck_codes::MANUALLY_INITIATED_CRASH && kd::kd_debugger_enabled() {
            let p1 = param1 as *const core::ffi::c_void;
            let p2 = param2 as *const core::ffi::c_void;
            let p3 = param3 as *const core::ffi::c_void;
            let p4 = param4 as *const core::ffi::c_void;
            kd::dbg_print_args(format_args!(
                "\n*** Fatal System Error: 0x{0:08X}\n                       (0x{1:016X},0x{2:016X},0x{3:016X},0x{4:016X})\n\n",
                bug_check_code, p1 as usize, p2 as usize, p3 as usize, p4 as usize
            ));

            // В NT/ReactOS при подключенном debugger здесь выполняется KiBugCheckDebugBreak(DBG_STATUS_BUGCHECK_FIRST).
            if !kd::kd_debugger_not_present() {
                kd::DbgBreakPointWithStatus(kd::dbg_status::DBG_STATUS_BUGCHECK_FIRST);
            }
        }

        // Не пытаемся выводить BSOD больше одного раза (аналог KeBugCheckCount).
        if KE_BUGCHECK_COUNT.fetch_sub(1, Ordering::AcqRel) != 1 {
            loop {
                core::arch::asm!("hlt", options(nomem, nostack));
            }
        }

        // MP freeze: замораживаем остальные CPU и даем время "flush caches" (~1s).
        if crate::ke::ke_number_processors() > 1 {
            crate::ke::ki_ipi_freeze_all();
            ke_stall_execution_processor(1_000_000);
        }

        // Bugcheck callbacks (как в NT/ReactOS): выполняем после freeze.
        unsafe { ki_do_bugcheck_callbacks() };

        // --- BSOD display (INBV/BOOTVID) ---
        if inbv::inbv_is_boot_driver_installed() {
            // NOTE(отступление): в NT5 координаты фиксированы 640x480.
            // В XenOS (UEFI framebuffer) используем реальный размер framebuffer,
            // чтобы BSOD занимал весь экран.
            let width = inbv::inbv_get_fb_width();
            let height = inbv::inbv_get_fb_height();

            inbv::inbv_acquire_display_ownership();
            let _ = inbv::inbv_reset_display();
            // XenOS: фирменный "фиолетовый BSOD" как политика оформления.
            // NB: палитра `BV_COLOR_*` остаётся NT-совместимой; для кастомного цвета используем прямой BGRA32.
            const XENOS_BSOD_BGRA: u32 = 0xFF4B2381;
            inbv::inbv_solid_color_fill_bgra(0, 0, width, height, XENOS_BSOD_BGRA);
            // Чтобы не было "чёрных прямоугольников" под буквами: синхронизируем back color текста с фоном.
            inbv::inbv_set_text_background_bgra(XENOS_BSOD_BGRA);
            inbv::inbv_set_text_color(crate::imports::bootvid::BV_COLOR_WHITE);
            inbv::inbv_enable_display_string(true);
            inbv::inbv_set_scroll_region(0, 0, width, height);

            inbv::inbv_display_string("\n");
            if let Some(s) = ke_get_bug_message_text(bugcheck_message_id::BUGCHECK_MESSAGE_INTRO) {
                inbv::inbv_display_string(s);
            }
            inbv::inbv_display_string("\n\n");

            // В XP при generic PSS message выводится имя bugcheck (KeGetBugMessageText(code)).
            inbv::inbv_display_string(get_bugcheck_name(bug_check_code));
            inbv::inbv_display_string("\n\n");

            if let Some(s) = ke_get_bug_message_text(bugcheck_message_id::PSS_MESSAGE_INTRO) {
                inbv::inbv_display_string(s);
            }
            inbv::inbv_display_string("\n\n");
            if let Some(s) = ke_get_bug_message_text(bugcheck_message_id::BUGCODE_PSS_MESSAGE) {
                inbv::inbv_display_string(s);
            }
            inbv::inbv_display_string("\n\n");

            if let Some(s) = ke_get_bug_message_text(bugcheck_message_id::BUGCHECK_TECH_INFO) {
                inbv::inbv_display_string(s);
            }

            // STOP line (XenOS format; intentionally not byte-identical to NT).
            let stop = bsod_format_stop_line(bug_check_code, param1, param2, param3, param4);
            inbv::inbv_display_string(stop);

            // Расширенный блок: попытка вывести максимум контекста и call stack.
            // ВАЖНО: не делаем ничего, что может спровоцировать page fault/alloc.
            bsod_display_extended_context(bug_check_code, param1, param2, param3, param4);
        }

        // Останов системы.
        loop {
            core::arch::asm!("hlt", options(nomem, nostack));
        }
    }
}

/// Выводит расширенную информацию на BSOD: trap name, регистры, stack dump и call stack.
fn bsod_display_extended_context(
    code: u32,
    p1: ULONG_PTR,
    p2: ULONG_PTR,
    p3: ULONG_PTR,
    p4: ULONG_PTR,
) {
    use crate::inbv;

    inbv::inbv_display_string("\nCrash context:\n");

    // Для 0x7F пытаемся вывести имя trap и раскладку параметров.
    if code == bugcheck_codes::UNEXPECTED_KERNEL_MODE_TRAP {
        let trap = p1 as u32;
        inbv::inbv_display_string("Trap: ");
        inbv::inbv_display_string(get_trap_name(trap));
        inbv::inbv_display_string("\n");
    }

    // Если контекст установлен — используем его. Иначе делаем best-effort из параметров.
    let mut rip = 0u64;
    let mut rsp = 0u64;
    let mut rbp = 0u64;
    let mut have_ctx = false;

    if BUGCHECK_TRAP_CONTEXT_VALID.load(Ordering::SeqCst) {
        unsafe {
            rip = BUGCHECK_TRAP_CONTEXT.rip;
            rsp = BUGCHECK_TRAP_CONTEXT.rsp;
            rbp = BUGCHECK_TRAP_CONTEXT.rbp;
            have_ctx = true;
        }
    }

    if !have_ctx {
        // Fallback эвристика: для некоторых bugcheck параметры часто содержат RIP/RSP.
        // - 0x7F: либо (p2=rip,p3=rsp) либо (p3=rip,p4=rsp) при наличии error_code.
        if code == bugcheck_codes::UNEXPECTED_KERNEL_MODE_TRAP {
            let trap = p1 as u32;
            let has_error_code = matches!(
                trap,
                trap_codes::DOUBLE_FAULT
                    | trap_codes::INVALID_TSS
                    | trap_codes::SEGMENT_NOT_PRESENT
                    | trap_codes::STACK_FAULT
                    | trap_codes::GENERAL_PROTECTION
                    | trap_codes::PAGE_FAULT
                    | trap_codes::ALIGNMENT_CHECK
            );
            if has_error_code {
                rip = p3 as u64;
                rsp = p4 as u64;
            } else {
                rip = p2 as u64;
                rsp = p3 as u64;
            }
        } else if code == bugcheck_codes::PAGE_FAULT_IN_NONPAGED_AREA {
            // 0x50: Param3 = RIP (как в комментарии trap.rs)
            rip = p3 as u64;
            rsp = 0;
        }
    }

    if rip != 0 {
        inbv::inbv_display_string(bsod_format_kv_u64("RIP", rip));
    }
    if rsp != 0 {
        inbv::inbv_display_string(bsod_format_kv_u64("RSP", rsp));
    }
    if rbp != 0 {
        inbv::inbv_display_string(bsod_format_kv_u64("RBP", rbp));
    }

    // Показываем bounds стека текущего потока (если доступны).
    if let Some((stack_limit, stack_base, current_thread)) = bsod_get_current_stack_bounds() {
        inbv::inbv_display_string(bsod_format_kv_u64("CurrentThread", current_thread));
        inbv::inbv_display_string(bsod_format_kv_u64("StackLimit", stack_limit));
        inbv::inbv_display_string(bsod_format_kv_u64("StackBase", stack_base));

        // Stack dump (несколько qword от RSP).
        if rsp != 0 {
            inbv::inbv_display_string("\nStack dump (RSP):\n");
            bsod_dump_stack_qwords(rsp, stack_limit, stack_base, 24);
        }

        // Call stack (RBP chain).
        if rbp != 0 {
            inbv::inbv_display_string("\nCall stack (RBP chain):\n");
            bsod_walk_rbp_chain(rbp, stack_limit, stack_base, 32);
        }
    }
}

/// Возвращает (StackLimit, StackBase, CurrentThread) для текущего CPU, если доступно.
fn bsod_get_current_stack_bounds() -> Option<(u64, u64, u64)> {
    use crate::arch::x86_64::pcr::get_prcb;

    // Пытаемся получить текущий поток через PRCB.
    let prcb = unsafe { get_prcb() };
    if prcb.is_null() {
        return None;
    }
    let current = unsafe { (*prcb).current_thread as *mut crate::ke::thread::KTHREAD };
    if current.is_null() {
        return None;
    }
    let stack_limit = unsafe { (*current).stack_limit as u64 };
    let stack_base = unsafe { (*current).initial_stack as u64 };
    if stack_limit == 0 || stack_base == 0 || stack_limit >= stack_base {
        return None;
    }
    Some((stack_limit, stack_base, current as u64))
}

/// Проверяет, что адрес выглядит как “канонический” x86_64.
#[inline]
fn is_canonical_addr(a: u64) -> bool {
    // Канонический адрес: биты 63..48 = знак бита 47.
    let sign = (a >> 47) & 1;
    let top = a >> 48;
    if sign == 0 { top == 0 } else { top == 0xFFFF }
}

/// Печатает N qword из стека начиная с RSP (best-effort, без page fault защиты).
fn bsod_dump_stack_qwords(rsp: u64, stack_limit: u64, stack_base: u64, count: usize) {
    use crate::inbv;
    if !is_canonical_addr(rsp) {
        inbv::inbv_display_string("RSP is non-canonical, skip dump.\n");
        return;
    }
    if rsp < stack_limit || rsp >= stack_base {
        inbv::inbv_display_string("RSP is out of current stack bounds, skip dump.\n");
        return;
    }
    let mut addr = rsp;
    for i in 0..count {
        if addr + 8 > stack_base {
            break;
        }
        // В аварийном пути читаем напрямую: если это упадёт — всё равно bugcheck.
        let v = unsafe { (addr as *const u64).read_volatile() };
        inbv::inbv_display_string(bsod_format_stack_qword(i, addr, v));
        addr = addr.wrapping_add(8);
    }
}

/// Делает walk по цепочке frame pointers (RBP) и печатает return addresses.
fn bsod_walk_rbp_chain(rbp: u64, stack_limit: u64, stack_base: u64, max_frames: usize) {
    use crate::inbv;

    if !is_canonical_addr(rbp) {
        inbv::inbv_display_string("RBP is non-canonical, skip walk.\n");
        return;
    }
    if rbp < stack_limit || rbp + 16 > stack_base {
        inbv::inbv_display_string("RBP is out of current stack bounds, skip walk.\n");
        return;
    }

    let mut cur = rbp;
    for i in 0..max_frames {
        if cur < stack_limit || cur + 16 > stack_base {
            break;
        }
        if (cur & 0xF) != 0 {
            // Для стабильности: RBP обычно 16-byte aligned в Win64 ABI.
            break;
        }
        let next = unsafe { (cur as *const u64).read_volatile() };
        let ret = unsafe { (cur.wrapping_add(8) as *const u64).read_volatile() };
        inbv::inbv_display_string(bsod_format_frame(i, ret));

        // Цепочка должна идти “вверх” стека.
        if next <= cur || next >= stack_base {
            break;
        }
        if !is_canonical_addr(next) || !is_canonical_addr(ret) {
            break;
        }
        cur = next;
    }
}

/// Форматирует строку "KEY: 0x....\n" (без аллокаций).
fn bsod_format_kv_u64(key: &'static str, v: u64) -> &'static str {
    static mut LINE: [u8; 96] = [0; 96];
    const CAP: usize = 96;

    struct W {
        buf: *mut u8,
        cap: usize,
        len: usize,
    }
    impl W {
        fn push(&mut self, b: u8) {
            if self.len < self.cap {
                unsafe { self.buf.add(self.len).write(b) };
                self.len += 1;
            }
        }
        fn push_str(&mut self, s: &str) {
            for &b in s.as_bytes() {
                self.push(b);
            }
        }
        fn push_hex_fixed(&mut self, v: u64, digits: usize) {
            const HEX: &[u8; 16] = b"0123456789ABCDEF";
            for i in (0..digits).rev() {
                let nibble = ((v >> (i * 4)) & 0xF) as usize;
                self.push(HEX[nibble]);
            }
        }
    }

    unsafe {
        let p = core::ptr::addr_of_mut!(LINE).cast::<u8>();
        core::ptr::write_bytes(p, 0, CAP);
        let mut w = W {
            buf: p,
            cap: CAP,
            len: 0,
        };
        w.push_str(key);
        w.push_str(": 0x");
        w.push_hex_fixed(v, 16);
        w.push_str("\n");
        let bytes: &'static [u8] = core::slice::from_raw_parts(p, w.len);
        core::str::from_utf8_unchecked(bytes)
    }
}

/// Форматирует строку stack dump: "[addr] = value\n".
fn bsod_format_stack_qword(idx: usize, addr: u64, v: u64) -> &'static str {
    static mut LINE: [u8; 128] = [0; 128];
    const CAP: usize = 128;

    struct W {
        buf: *mut u8,
        cap: usize,
        len: usize,
    }
    impl W {
        fn push(&mut self, b: u8) {
            if self.len < self.cap {
                unsafe { self.buf.add(self.len).write(b) };
                self.len += 1;
            }
        }
        fn push_str(&mut self, s: &str) {
            for &b in s.as_bytes() {
                self.push(b);
            }
        }
        fn push_dec_2(&mut self, v: usize) {
            let d0 = (v / 10) % 10;
            let d1 = v % 10;
            self.push(b'0' + d0 as u8);
            self.push(b'0' + d1 as u8);
        }
        fn push_hex_fixed(&mut self, v: u64, digits: usize) {
            const HEX: &[u8; 16] = b"0123456789ABCDEF";
            for i in (0..digits).rev() {
                let nibble = ((v >> (i * 4)) & 0xF) as usize;
                self.push(HEX[nibble]);
            }
        }
    }

    unsafe {
        let p = core::ptr::addr_of_mut!(LINE).cast::<u8>();
        core::ptr::write_bytes(p, 0, CAP);
        let mut w = W {
            buf: p,
            cap: CAP,
            len: 0,
        };
        w.push(b'[');
        w.push_dec_2(idx.min(99));
        w.push_str("] 0x");
        w.push_hex_fixed(addr, 16);
        w.push_str(": 0x");
        w.push_hex_fixed(v, 16);
        w.push_str("\n");
        let bytes: &'static [u8] = core::slice::from_raw_parts(p, w.len);
        core::str::from_utf8_unchecked(bytes)
    }
}

/// Форматирует одну строку call stack: "#xx 0xADDR\n".
fn bsod_format_frame(idx: usize, ret: u64) -> &'static str {
    static mut LINE: [u8; 64] = [0; 64];
    const CAP: usize = 64;

    struct W {
        buf: *mut u8,
        cap: usize,
        len: usize,
    }
    impl W {
        fn push(&mut self, b: u8) {
            if self.len < self.cap {
                unsafe { self.buf.add(self.len).write(b) };
                self.len += 1;
            }
        }
        fn push_str(&mut self, s: &str) {
            for &b in s.as_bytes() {
                self.push(b);
            }
        }
        fn push_dec_2(&mut self, v: usize) {
            let d0 = (v / 10) % 10;
            let d1 = v % 10;
            self.push(b'0' + d0 as u8);
            self.push(b'0' + d1 as u8);
        }
        fn push_hex_fixed(&mut self, v: u64, digits: usize) {
            const HEX: &[u8; 16] = b"0123456789ABCDEF";
            for i in (0..digits).rev() {
                let nibble = ((v >> (i * 4)) & 0xF) as usize;
                self.push(HEX[nibble]);
            }
        }
    }

    unsafe {
        let p = core::ptr::addr_of_mut!(LINE).cast::<u8>();
        core::ptr::write_bytes(p, 0, CAP);
        let mut w = W {
            buf: p,
            cap: CAP,
            len: 0,
        };
        w.push(b'#');
        w.push_dec_2(idx.min(99));
        w.push_str(" 0x");
        w.push_hex_fixed(ret, 16);
        w.push_str("\n");
        let bytes: &'static [u8] = core::slice::from_raw_parts(p, w.len);
        core::str::from_utf8_unchecked(bytes)
    }
}

/// Формирует STOP line как в NT5, без аллокаций.
fn bsod_format_stop_line(
    code: u32,
    p1: ULONG_PTR,
    p2: ULONG_PTR,
    p3: ULONG_PTR,
    p4: ULONG_PTR,
) -> &'static str {
    // Используем статический буфер только для "последней" строки.
    // Bugcheck останавливает систему и не возвращается, поэтому это безопасно.
    static mut STOP_LINE: [u8; 128] = [0; 128];
    const STOP_LINE_CAP: usize = 128;

    struct W {
        buf: *mut u8,
        cap: usize,
        len: usize,
    }
    impl W {
        fn push(&mut self, b: u8) {
            if self.len < self.cap {
                unsafe {
                    self.buf.add(self.len).write(b);
                }
                self.len += 1;
            }
        }
        fn push_str(&mut self, s: &str) {
            for &b in s.as_bytes() {
                self.push(b);
            }
        }
        fn push_hex_fixed(&mut self, v: u64, digits: usize) {
            const HEX: &[u8; 16] = b"0123456789ABCDEF";
            // Печатаем фиксированную ширину, старшие нули сохраняем.
            for i in (0..digits).rev() {
                let nibble = ((v >> (i * 4)) & 0xF) as usize;
                self.push(HEX[nibble]);
            }
        }
    }

    unsafe {
        let buf_ptr = core::ptr::addr_of_mut!(STOP_LINE).cast::<u8>();
        core::ptr::write_bytes(buf_ptr, 0, STOP_LINE_CAP);
        let mut w = W {
            buf: buf_ptr,
            cap: STOP_LINE_CAP,
            len: 0,
        };

        w.push_str("\n\n*** XENOS STOP: 0x");
        w.push_hex_fixed(code as u64, 8);
        w.push_str(" (0x");
        w.push_hex_fixed(p1 as u64, 16);
        w.push_str(",0x");
        w.push_hex_fixed(p2 as u64, 16);
        w.push_str(",0x");
        w.push_hex_fixed(p3 as u64, 16);
        w.push_str(",0x");
        w.push_hex_fixed(p4 as u64, 16);
        w.push_str(")\n\n");

        // SAFETY: пишем только ASCII
        let bytes: &'static [u8] = core::slice::from_raw_parts(buf_ptr, w.len);
        core::str::from_utf8_unchecked(bytes)
    }
}

/// Возвращает название ошибки по коду
fn get_bugcheck_name(code: u32) -> &'static str {
    use bugcheck_codes::*;
    match code {
        APC_INDEX_MISMATCH => "APC_INDEX_MISMATCH",
        IRQL_NOT_LESS_OR_EQUAL => "IRQL_NOT_LESS_OR_EQUAL",
        IRQL_NOT_GREATER_OR_EQUAL => "IRQL_NOT_GREATER_OR_EQUAL",
        KMODE_EXCEPTION_NOT_HANDLED => "KMODE_EXCEPTION_NOT_HANDLED",
        PAGE_FAULT_IN_NONPAGED_AREA => "PAGE_FAULT_IN_NONPAGED_AREA",
        UNEXPECTED_KERNEL_MODE_TRAP => "UNEXPECTED_KERNEL_MODE_TRAP",
        KERNEL_MODE_EXCEPTION_NOT_HANDLED => "KERNEL_MODE_EXCEPTION_NOT_HANDLED",
        MANUALLY_INITIATED_CRASH => "MANUALLY_INITIATED_CRASH",
        SPIN_LOCK_ALREADY_OWNED => "SPIN_LOCK_ALREADY_OWNED",
        SPIN_LOCK_NOT_OWNED => "SPIN_LOCK_NOT_OWNED",
        BAD_POOL_HEADER => "BAD_POOL_HEADER",
        BAD_POOL_CALLER => "BAD_POOL_CALLER",
        MEMORY_MANAGEMENT => "MEMORY_MANAGEMENT",
        PFN_LIST_CORRUPT => "PFN_LIST_CORRUPT",
        DRIVER_IRQL_NOT_LESS_OR_EQUAL => "DRIVER_IRQL_NOT_LESS_OR_EQUAL",
        SYSTEM_SERVICE_EXCEPTION => "SYSTEM_SERVICE_EXCEPTION",
        SYSTEM_THREAD_EXCEPTION_NOT_HANDLED => "SYSTEM_THREAD_EXCEPTION_NOT_HANDLED",
        CRITICAL_PROCESS_DIED => "CRITICAL_PROCESS_DIED",
        KERNEL_SECURITY_CHECK_FAILURE => "KERNEL_SECURITY_CHECK_FAILURE",
        ATTEMPTED_SWITCH_FROM_DPC => "ATTEMPTED_SWITCH_FROM_DPC",
        _ => "UNKNOWN_BUGCHECK",
    }
}

/// Возвращает название trap по номеру
fn get_trap_name(trap: u32) -> &'static str {
    use trap_codes::*;
    match trap {
        DIVIDE_ERROR => "Divide Error",
        DEBUG => "Debug",
        NMI => "NMI",
        BREAKPOINT => "Breakpoint",
        OVERFLOW => "Overflow",
        BOUND_RANGE => "Bound Range",
        INVALID_OPCODE => "Invalid Opcode",
        DEVICE_NOT_AVAILABLE => "Device Not Available",
        DOUBLE_FAULT => "Double Fault",
        INVALID_TSS => "Invalid TSS",
        SEGMENT_NOT_PRESENT => "Segment Not Present",
        STACK_FAULT => "Stack Fault",
        GENERAL_PROTECTION => "General Protection",
        PAGE_FAULT => "Page Fault",
        X87_FPU_ERROR => "x87 FPU Error",
        ALIGNMENT_CHECK => "Alignment Check",
        MACHINE_CHECK => "Machine Check",
        SIMD_FP => "SIMD Floating Point",
        _ => "Unknown",
    }
}
