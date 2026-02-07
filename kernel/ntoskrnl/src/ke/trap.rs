//! Обработчики исключений и прерываний
//!
//! Источники:
//! - NT5: ke/amd64/trap.asm, ke/amd64/except.c
//! - ReactOS: ke/amd64/trap.S, ke/amd64/except.c
//!            sdk/include/ndk/amd64/ketypes.h

#![allow(dead_code)]
#![allow(non_camel_case_types)]

use super::bugcheck::bugcheck_codes;
use super::bugcheck::ke_bug_check_ex;
use super::bugcheck::trap_codes;
use super::trap_asm;
use crate::arch::x86_64::cpu;
use crate::arch::x86_64::idt::InterruptFrame;
use crate::arch::x86_64::idt::InterruptFrameWithError;
use crate::arch::x86_64::xstate::M128A;

// =============================================================================
// KTRAP_FRAME - сохраненный контекст при исключении (ReactOS amd64)
// =============================================================================

/// KTRAP_FRAME - контекст процессора при trap/exception/interrupt
///
/// Структура соответствует ReactOS/Windows NT amd64.
/// Размер: 0x190 (400) байт
#[repr(C)]
#[derive(Clone, Copy)]
pub struct KTRAP_FRAME {
    // 0x00: Home space для параметров (Windows x64 ABI)
    pub p1_home: u64, // 0x00
    pub p2_home: u64, // 0x08
    pub p3_home: u64, // 0x10
    pub p4_home: u64, // 0x18
    pub p5: u64,      // 0x20

    // 0x28: Режим и состояние
    pub previous_mode: i8,    // 0x28 - KernelMode (0) / UserMode (1)
    pub previous_irql: u8,    // 0x29 - IRQL до trap
    pub fault_indicator: u8,  // 0x2A
    pub exception_active: u8, // 0x2B
    pub mxcsr: u32,           // 0x2C - SSE control/status register

    // 0x30: Volatile регистры (сохраняются всегда)
    pub rax: u64, // 0x30
    pub rcx: u64, // 0x38
    pub rdx: u64, // 0x40
    pub r8: u64,  // 0x48
    pub r9: u64,  // 0x50
    pub r10: u64, // 0x58
    pub r11: u64, // 0x60

    // 0x68: GS base (union с GsSwap)
    pub gs_base: u64, // 0x68 - также GsSwap

    // 0x70: XMM регистры (volatile)
    pub xmm0: M128A, // 0x70
    pub xmm1: M128A, // 0x80
    pub xmm2: M128A, // 0x90
    pub xmm3: M128A, // 0xA0
    pub xmm4: M128A, // 0xB0
    pub xmm5: M128A, // 0xC0

    // 0xD0: Fault/Context info (union)
    pub fault_address: u64, // 0xD0 - также ContextRecord, TimeStampCKCL

    // 0xD8: Debug регистры
    pub dr0: u64, // 0xD8
    pub dr1: u64, // 0xE0
    pub dr2: u64, // 0xE8
    pub dr3: u64, // 0xF0
    pub dr6: u64, // 0xF8
    pub dr7: u64, // 0x100

    // 0x108: Debug control (union)
    pub debug_control: u64,           // 0x108
    pub last_branch_to_rip: u64,      // 0x110
    pub last_branch_from_rip: u64,    // 0x118
    pub last_exception_to_rip: u64,   // 0x120
    pub last_exception_from_rip: u64, // 0x128

    // 0x130: Сегментные регистры
    pub seg_ds: u16, // 0x130
    pub seg_es: u16, // 0x132
    pub seg_fs: u16, // 0x134
    pub seg_gs: u16, // 0x136

    // 0x138: Связь с другими фреймами
    pub trap_frame: u64, // 0x138 - указатель на предыдущий trap frame

    // 0x140: Non-volatile регистры (сохраняются при user mode trap)
    pub rbx: u64, // 0x140
    pub rdi: u64, // 0x148
    pub rsi: u64, // 0x150
    pub rbp: u64, // 0x158

    // 0x160: Error code (union с ExceptionFrame, TimeStampKlog)
    pub error_code: u64, // 0x160

    // 0x168: CPU-сохраненные регистры (IRET frame)
    pub rip: u64,              // 0x168
    pub seg_cs: u16,           // 0x170
    pub fill0: u8,             // 0x172
    pub logging: u8,           // 0x173
    pub fill1: [u16; 2],       // 0x174
    pub eflags: u32,           // 0x178
    pub fill2: u32,            // 0x17C
    pub rsp: u64,              // 0x180
    pub seg_ss: u16,           // 0x188
    pub fill3: u16,            // 0x18A
    pub code_patch_cycle: i32, // 0x18C
}

/// Размер KTRAP_FRAME
pub const KTRAP_FRAME_LENGTH: usize = 0x190; // 400 байт

impl KTRAP_FRAME {
    pub const fn new() -> Self {
        Self {
            p1_home: 0,
            p2_home: 0,
            p3_home: 0,
            p4_home: 0,
            p5: 0,
            previous_mode: 0,
            previous_irql: 0,
            fault_indicator: 0,
            exception_active: 0,
            mxcsr: 0x1F80, // INITIAL_MXCSR
            rax: 0,
            rcx: 0,
            rdx: 0,
            r8: 0,
            r9: 0,
            r10: 0,
            r11: 0,
            gs_base: 0,
            xmm0: M128A::new(),
            xmm1: M128A::new(),
            xmm2: M128A::new(),
            xmm3: M128A::new(),
            xmm4: M128A::new(),
            xmm5: M128A::new(),
            fault_address: 0,
            dr0: 0,
            dr1: 0,
            dr2: 0,
            dr3: 0,
            dr6: 0,
            dr7: 0,
            debug_control: 0,
            last_branch_to_rip: 0,
            last_branch_from_rip: 0,
            last_exception_to_rip: 0,
            last_exception_from_rip: 0,
            seg_ds: 0,
            seg_es: 0,
            seg_fs: 0,
            seg_gs: 0,
            trap_frame: 0,
            rbx: 0,
            rdi: 0,
            rsi: 0,
            rbp: 0,
            error_code: 0,
            rip: 0,
            seg_cs: 0,
            fill0: 0,
            logging: 0,
            fill1: [0; 2],
            eflags: 0,
            fill2: 0,
            rsp: 0,
            seg_ss: 0,
            fill3: 0,
            code_patch_cycle: 0,
        }
    }

    /// Проверяет был ли trap из user mode
    #[inline]
    pub fn is_user_mode(&self) -> bool {
        self.previous_mode != 0
    }

    /// Возвращает exception frame если есть
    #[inline]
    pub fn exception_frame(&self) -> *mut KEXCEPTION_FRAME {
        self.error_code as *mut KEXCEPTION_FRAME
    }
}

impl Default for KTRAP_FRAME {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// KEXCEPTION_FRAME - non-volatile регистры при exception
// =============================================================================

/// KEXCEPTION_FRAME - сохранение non-volatile регистров
///
/// Используется при обработке исключений для сохранения
/// non-volatile регистров (callee-saved).
/// Размер: 0x140 (320) байт
#[repr(C)]
#[derive(Clone, Copy)]
pub struct KEXCEPTION_FRAME {
    // 0x00: Home space
    pub p1_home: u64,       // 0x00
    pub p2_home: u64,       // 0x08
    pub p3_home: u64,       // 0x10
    pub p4_home: u64,       // 0x18
    pub p5: u64,            // 0x20
    pub initial_stack: u64, // 0x28 (или Spare1 в Win8+)

    // 0x30: XMM регистры (non-volatile, XMM6-XMM15)
    pub xmm6: M128A,  // 0x30
    pub xmm7: M128A,  // 0x40
    pub xmm8: M128A,  // 0x50
    pub xmm9: M128A,  // 0x60
    pub xmm10: M128A, // 0x70
    pub xmm11: M128A, // 0x80
    pub xmm12: M128A, // 0x90
    pub xmm13: M128A, // 0xA0
    pub xmm14: M128A, // 0xB0
    pub xmm15: M128A, // 0xC0

    // 0xD0: Связи и буферы
    pub trap_frame: u64,    // 0xD0
    pub output_buffer: u64, // 0xD8
    pub output_length: u64, // 0xE0
    pub spare2: u64,        // 0xE8 (или CallbackStack до Win8)

    // 0xF0: MxCsr и non-volatile GPRs
    pub mxcsr: u64,          // 0xF0
    pub rbp: u64,            // 0xF8
    pub rbx: u64,            // 0x100
    pub rdi: u64,            // 0x108
    pub rsi: u64,            // 0x110
    pub r12: u64,            // 0x118
    pub r13: u64,            // 0x120
    pub r14: u64,            // 0x128
    pub r15: u64,            // 0x130
    pub return_address: u64, // 0x138
}

/// Размер KEXCEPTION_FRAME
pub const KEXCEPTION_FRAME_LENGTH: usize = 0x140; // 320 байт

impl KEXCEPTION_FRAME {
    pub const fn new() -> Self {
        Self {
            p1_home: 0,
            p2_home: 0,
            p3_home: 0,
            p4_home: 0,
            p5: 0,
            initial_stack: 0,
            xmm6: M128A::new(),
            xmm7: M128A::new(),
            xmm8: M128A::new(),
            xmm9: M128A::new(),
            xmm10: M128A::new(),
            xmm11: M128A::new(),
            xmm12: M128A::new(),
            xmm13: M128A::new(),
            xmm14: M128A::new(),
            xmm15: M128A::new(),
            trap_frame: 0,
            output_buffer: 0,
            output_length: 0,
            spare2: 0,
            mxcsr: 0x1F80,
            rbp: 0,
            rbx: 0,
            rdi: 0,
            rsi: 0,
            r12: 0,
            r13: 0,
            r14: 0,
            r15: 0,
            return_address: 0,
        }
    }
}

impl Default for KEXCEPTION_FRAME {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// MACHINE_FRAME - CPU-сохраненная часть при interrupt/exception
// =============================================================================

/// MACHINE_FRAME - регистры, автоматически сохраняемые CPU при interrupt
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MACHINE_FRAME {
    pub rip: u64,
    pub seg_cs: u16,
    pub fill1: [u16; 3],
    pub eflags: u32,
    pub fill2: u32,
    pub rsp: u64,
    pub seg_ss: u16,
    pub fill3: [u16; 3],
}

impl MACHINE_FRAME {
    pub const fn new() -> Self {
        Self {
            rip: 0,
            seg_cs: 0,
            fill1: [0; 3],
            eflags: 0,
            fill2: 0,
            rsp: 0,
            seg_ss: 0,
            fill3: [0; 3],
        }
    }
}

/// Page Fault error code bits
pub mod page_fault_error {
    /// Violation was caused by a not-present page
    pub const PRESENT: u64 = 1 << 0;
    /// Write access caused the fault
    pub const WRITE: u64 = 1 << 1;
    /// Fault occurred in user mode
    pub const USER: u64 = 1 << 2;
    /// Reserved bit was set in page table
    pub const RESERVED_WRITE: u64 = 1 << 3;
    /// Instruction fetch caused the fault
    pub const INSTRUCTION_FETCH: u64 = 1 << 4;
    /// Protection key violation
    pub const PROTECTION_KEY: u64 = 1 << 5;
    /// Shadow stack access fault
    pub const SHADOW_STACK: u64 = 1 << 6;
}

// =============================================================================
// Обработчики исключений
// =============================================================================

/// #DE - Divide Error
pub extern "x86-interrupt" fn divide_error_handler(frame: InterruptFrame) {
    ke_bug_check_ex(
        bugcheck_codes::UNEXPECTED_KERNEL_MODE_TRAP,
        trap_codes::DIVIDE_ERROR as usize,
        frame.rip as usize,
        frame.rsp as usize,
        0,
    );
}

/// #DB - Debug Exception
pub extern "x86-interrupt" fn debug_handler(frame: InterruptFrame) {
    // Debug exceptions могут быть нормальными (breakpoint)
    // Пока просто логируем
    let _ = frame;
}

/// NMI - Non-Maskable Interrupt
pub extern "x86-interrupt" fn nmi_handler(frame: InterruptFrame) {
    // NMI обычно указывает на аппаратную проблему
    ke_bug_check_ex(
        bugcheck_codes::UNEXPECTED_KERNEL_MODE_TRAP,
        trap_codes::NMI as usize,
        frame.rip as usize,
        frame.rsp as usize,
        0,
    );
}

/// #BP - Breakpoint
pub extern "x86-interrupt" fn breakpoint_handler(frame: InterruptFrame) {
    // Breakpoint - нормальное событие для отладчика
    let _ = frame;
    // TODO: Вызвать kernel debugger если активен
}

/// #OF - Overflow
pub extern "x86-interrupt" fn overflow_handler(frame: InterruptFrame) {
    ke_bug_check_ex(
        bugcheck_codes::UNEXPECTED_KERNEL_MODE_TRAP,
        trap_codes::OVERFLOW as usize,
        frame.rip as usize,
        frame.rsp as usize,
        0,
    );
}

/// #BR - Bound Range Exceeded
pub extern "x86-interrupt" fn bound_range_handler(frame: InterruptFrame) {
    ke_bug_check_ex(
        bugcheck_codes::UNEXPECTED_KERNEL_MODE_TRAP,
        trap_codes::BOUND_RANGE as usize,
        frame.rip as usize,
        frame.rsp as usize,
        0,
    );
}

/// #UD - Invalid Opcode
pub extern "x86-interrupt" fn invalid_opcode_handler(frame: InterruptFrame) {
    ke_bug_check_ex(
        bugcheck_codes::UNEXPECTED_KERNEL_MODE_TRAP,
        trap_codes::INVALID_OPCODE as usize,
        frame.rip as usize,
        frame.rsp as usize,
        0,
    );
}

/// #NM - Device Not Available (FPU)
pub extern "x86-interrupt" fn device_not_available_handler(frame: InterruptFrame) {
    // Это может быть нормально если используется lazy FPU switching
    let _ = frame;
    // TODO: Загрузить FPU контекст потока
}

/// #DF - Double Fault
pub extern "x86-interrupt" fn double_fault_handler(frame: InterruptFrameWithError) -> ! {
    ke_bug_check_ex(
        bugcheck_codes::UNEXPECTED_KERNEL_MODE_TRAP,
        trap_codes::DOUBLE_FAULT as usize,
        frame.error_code as usize,
        frame.rip as usize,
        frame.rsp as usize,
    );
}

/// #TS - Invalid TSS
pub extern "x86-interrupt" fn invalid_tss_handler(frame: InterruptFrameWithError) {
    ke_bug_check_ex(
        bugcheck_codes::UNEXPECTED_KERNEL_MODE_TRAP,
        trap_codes::INVALID_TSS as usize,
        frame.error_code as usize,
        frame.rip as usize,
        frame.rsp as usize,
    );
}

/// #NP - Segment Not Present
pub extern "x86-interrupt" fn segment_not_present_handler(frame: InterruptFrameWithError) {
    ke_bug_check_ex(
        bugcheck_codes::UNEXPECTED_KERNEL_MODE_TRAP,
        trap_codes::SEGMENT_NOT_PRESENT as usize,
        frame.error_code as usize,
        frame.rip as usize,
        frame.rsp as usize,
    );
}

/// #SS - Stack-Segment Fault
pub extern "x86-interrupt" fn stack_segment_fault_handler(frame: InterruptFrameWithError) {
    ke_bug_check_ex(
        bugcheck_codes::UNEXPECTED_KERNEL_MODE_TRAP,
        trap_codes::STACK_FAULT as usize,
        frame.error_code as usize,
        frame.rip as usize,
        frame.rsp as usize,
    );
}

/// #GP - General Protection Fault
pub extern "x86-interrupt" fn general_protection_handler(frame: InterruptFrameWithError) {
    ke_bug_check_ex(
        bugcheck_codes::UNEXPECTED_KERNEL_MODE_TRAP,
        trap_codes::GENERAL_PROTECTION as usize,
        frame.error_code as usize,
        frame.rip as usize,
        frame.rsp as usize,
    );
}

/// #PF - Page Fault
pub extern "x86-interrupt" fn page_fault_handler(frame: InterruptFrameWithError) {
    let fault_addr = cpu::read_cr2();

    let write = frame.error_code & page_fault_error::WRITE != 0;

    // TODO: Вызвать MmAccessFault для обработки
    // Пока вызываем KeBugCheckEx

    // PAGE_FAULT_IN_NONPAGED_AREA parameters:
    // Param1: Memory address that was referenced
    // Param2: Type of access (0=read, 1=write)
    // Param3: Address that referenced memory (RIP)
    // Param4: Type of page fault (error code)
    ke_bug_check_ex(
        bugcheck_codes::PAGE_FAULT_IN_NONPAGED_AREA,
        fault_addr as usize,
        if write { 1 } else { 0 },
        frame.rip as usize,
        frame.error_code as usize,
    );
}

// =============================================================================
// NT/ReactOS-style trap stubs callouts (Win64 ABI)
// =============================================================================

/// Общий bugcheck для исключений с error code (через ASM trap stub).
///
/// Win64 ABI:
/// - RCX = trap_code
/// - RDX = error_code
/// - R8  = rip
/// - R9  = interrupted_rsp
#[unsafe(no_mangle)]
pub extern "win64" fn KiTrapBugCheckWithError(
    trap_code: u32,
    error_code: u64,
    rip: u64,
    interrupted_rsp: u64,
    rbp: u64,
) -> ! {
    // Сохраняем контекст для BSOD (максимум информации с места сбоя).
    // ВАЖНО: это именно регистры прерванного контекста, а не текущего стека KeBugCheckEx.
    super::bugcheck::ki_bugcheck_set_trap_context(trap_code, error_code, rip, interrupted_rsp, rbp);
    ke_bug_check_ex(
        bugcheck_codes::UNEXPECTED_KERNEL_MODE_TRAP,
        trap_code as usize,
        error_code as usize,
        rip as usize,
        interrupted_rsp as usize,
    );
}

/// Bugcheck для page fault, чтобы увидеть первопричину (#PF) без вторичного #GP в Rust-прологе.
///
/// Win64 ABI:
/// - RCX = error_code
/// - RDX = rip
/// - R8  = interrupted_rsp
#[unsafe(no_mangle)]
pub extern "win64" fn KiPageFaultBugCheck(error_code: u64, rip: u64, _interrupted_rsp: u64) -> ! {
    let fault_addr = cpu::read_cr2();
    let write = (error_code & page_fault_error::WRITE) != 0;
    ke_bug_check_ex(
        bugcheck_codes::PAGE_FAULT_IN_NONPAGED_AREA,
        fault_addr as usize,
        if write { 1 } else { 0 },
        rip as usize,
        error_code as usize,
    );
}

/// #MF - x87 FPU Error
pub extern "x86-interrupt" fn x87_fpu_error_handler(frame: InterruptFrame) {
    ke_bug_check_ex(
        bugcheck_codes::UNEXPECTED_KERNEL_MODE_TRAP,
        trap_codes::X87_FPU_ERROR as usize,
        frame.rip as usize,
        frame.rsp as usize,
        0,
    );
}

/// #AC - Alignment Check
pub extern "x86-interrupt" fn alignment_check_handler(frame: InterruptFrameWithError) {
    ke_bug_check_ex(
        bugcheck_codes::UNEXPECTED_KERNEL_MODE_TRAP,
        trap_codes::ALIGNMENT_CHECK as usize,
        frame.error_code as usize,
        frame.rip as usize,
        frame.rsp as usize,
    );
}

/// #MC - Machine Check
pub extern "x86-interrupt" fn machine_check_handler(frame: InterruptFrame) -> ! {
    ke_bug_check_ex(
        bugcheck_codes::UNEXPECTED_KERNEL_MODE_TRAP,
        trap_codes::MACHINE_CHECK as usize,
        frame.rip as usize,
        frame.rsp as usize,
        0,
    );
}

/// #XM - SIMD Floating-Point Exception
pub extern "x86-interrupt" fn simd_fp_handler(frame: InterruptFrame) {
    ke_bug_check_ex(
        bugcheck_codes::UNEXPECTED_KERNEL_MODE_TRAP,
        trap_codes::SIMD_FP as usize,
        frame.rip as usize,
        frame.rsp as usize,
        0,
    );
}

// =============================================================================
// Прерывания
// =============================================================================

/// Обработчик spurious прерывания
pub extern "x86-interrupt" fn spurious_handler(_frame: InterruptFrame) {
    // Spurious interrupt - игнорируем
}

/// KeUpdateRunTime - обновляет счетчики времени выполнения
///
/// Вызывается из timer_handler на каждый тик.
/// Увеличивает user_time или kernel_time в зависимости от режима при прерывании.
/// Также обновляет dpc_time если активен DPC.
///
/// Источник: ReactOS ntoskrnl/ke/time.c KeUpdateRunTime
unsafe fn ke_update_run_time(user_mode: bool) {
    unsafe {
        use crate::arch::x86_64::pcr::get_prcb;

        let prcb = get_prcb();
        if prcb.is_null() {
            return;
        }

        let current = (*prcb).current_thread as *mut super::thread::KTHREAD;
        if current.is_null() {
            return;
        }

        // Проверяем выполняется ли DPC
        if (*prcb).dpc_routine_active != 0 {
            // DPC time - увеличиваем счетчик DPC времени
            (*prcb).dpc_time = (*prcb).dpc_time.wrapping_add(1);
        } else if user_mode {
            // User mode - увеличиваем user time потока и процессора
            (*current).user_time = (*current).user_time.wrapping_add(1);
            (*prcb).user_time = (*prcb).user_time.wrapping_add(1);
        } else {
            // Kernel mode - увеличиваем kernel time потока и процессора
            (*current).kernel_time = (*current).kernel_time.wrapping_add(1);
            (*prcb).kernel_time = (*prcb).kernel_time.wrapping_add(1);
        }
    }
}

/// PspForegroundQuantum - таблица квантов для foreground/background процессов
///
/// Индексы:
/// - 0: Background process (минимальный quantum)
/// - 1: Normal
/// - 2: Foreground process (максимальный quantum, 3x boost)
///
/// Значения для Workstation (Short Variable):
/// - Background: 6 (2 clock ticks)
/// - Normal: 12 (4 clock ticks)
/// - Foreground: 18 (6 clock ticks)
///
/// Источник: ReactOS ntoskrnl/ps/quota.c, Windows Internals
pub static PSP_FOREGROUND_QUANTUM: [i8; 3] = [6, 12, 18];

/// CLOCK_QUANTUM_DECREMENT - декремент quantum за тик таймера
/// В NT/ReactOS = 3
pub const CLOCK_QUANTUM_DECREMENT: i8 = 3;

/// KiQuantumTick - декремент quantum текущего потока
///
/// Вызывается из timer_handler на каждый тик таймера.
/// НЕ делает context switch напрямую - устанавливает флаг для отложенного switch.
///
/// Соответствует Windows XP/ReactOS поведению:
/// - Idle thread: только проверка ready_summary
/// - Real-time потоки (priority >= 16): НЕ теряют quantum
/// - Динамические потоки: декремент на CLOCK_QUANTUM_DECREMENT
unsafe fn ki_quantum_tick() {
    unsafe {
        use crate::arch::x86_64::pcr::get_prcb;
        use crate::ke::globals::LOW_REALTIME_PRIORITY;

        let prcb = get_prcb();
        if prcb.is_null() {
            return;
        }

        let current = (*prcb).current_thread as *mut super::thread::KTHREAD;
        if current.is_null() {
            return;
        }

        let is_idle = current == (*prcb).idle_thread as *mut super::thread::KTHREAD;

        // =========================================================================
        // IDLE THREAD: проверяем есть ли готовые потоки
        // =========================================================================
        if is_idle {
            if (*prcb).ready_summary != 0 {
                (*prcb).quantum_end = 1;
            }
            return;
        }

        // =========================================================================
        // REAL-TIME ПОТОКИ (priority >= 16): НЕ теряют quantum
        // =========================================================================
        if (*current).priority >= LOW_REALTIME_PRIORITY as i8 {
            return;
        }

        // =========================================================================
        // ДИНАМИЧЕСКИЕ ПОТОКИ: декремент quantum
        // =========================================================================
        if (*current).quantum > 0 {
            (*current).quantum -= CLOCK_QUANTUM_DECREMENT;

            if (*current).quantum <= 0 {
                // Quantum исчерпан - устанавливаем флаг
                (*prcb).quantum_end = 1;
            }
        }
    }
}

/// KiAdjustQuantumThread / KiQuantumEnd - обрабатывает конец кванта времени
///
/// Соответствует ReactOS KiAdjustQuantumThread (thrdschd.c:534-580)
/// и Windows XP KiQuantumEnd.
///
/// Выполняет:
/// 1. Reset quantum до quantum_reset
/// 2. Priority decay для динамических потоков
/// 3. Добавление текущего потока в ready queue
/// 4. Выбор следующего потока
///
/// # Returns
/// Указатель на следующий поток или NULL
pub unsafe fn ki_quantum_end() -> *mut super::thread::KTHREAD {
    unsafe {
        use crate::arch::x86_64::pcr::get_prcb;
        use crate::ke::globals::LOW_REALTIME_PRIORITY;
        use crate::ke::sched::THREAD_QUANTUM;
        use crate::ke::sched::ki_find_ready_thread;
        use crate::ke::sched::ki_ready_thread;
        use crate::ke::sched::ki_set_prcb_next_thread;
        use crate::nt::PVOID;

        // ВАЖНО: вызывается из `ke::sched::ki_dispatch_on_current_processor()` под
        // `KI_DISPATCHER_LOCK` и на DISPATCH_LEVEL. Поэтому тут не берём дополнительные
        // thread/prcb locks — это будет возвращено на Этапе C при полноценной SMP-логике.

        let prcb = get_prcb();
        if prcb.is_null() {
            return core::ptr::null_mut();
        }

        (*prcb).quantum_end = 0;

        let current = (*prcb).current_thread as *mut super::thread::KTHREAD;
        if current.is_null() {
            return core::ptr::null_mut();
        }

        // Если NextThread уже выбран (wake/ready) — не перетираем.
        if !(*prcb).next_thread.is_null() {
            return (*prcb).next_thread as *mut super::thread::KTHREAD;
        }

        // IDLE: если есть ready потоки — выбираем следующий.
        if current as PVOID == (*prcb).idle_thread {
            if (*prcb).ready_summary != 0 {
                if let Some(next) = ki_find_ready_thread((*prcb).number as u32, 0) {
                    let t = next.as_ptr();
                    (*t).set_state(super::thread::KTHREAD_STATE::Standby);
                    ki_set_prcb_next_thread(prcb, t);
                    return t;
                }
            }
            return core::ptr::null_mut();
        }

        // Reset quantum (минимальная база).
        // ВАЖНО: сюда мы попадаем при установленном `prcb.quantum_end`,
        // то есть чаще всего именно при истечении кванта.
        if (*current).quantum <= 0 {
            (*current).quantum = if (*current).quantum_reset > 0 {
                (*current).quantum_reset
            } else {
                THREAD_QUANTUM
            };
        }

        // Real-time: переключаемся только если есть более приоритетные ready потоки.
        if (*current).priority >= LOW_REALTIME_PRIORITY as i8 {
            let higher = (*current).priority as u32 + 1;
            if higher < 32 {
                let mask = !((1u32 << higher) - 1);
                if ((*prcb).ready_summary & mask) != 0 {
                    if let Some(next) = ki_find_ready_thread((*prcb).number as u32, higher as i32) {
                        let t = next.as_ptr();
                        (*t).set_state(super::thread::KTHREAD_STATE::Standby);
                        ki_set_prcb_next_thread(prcb, t);
                        return t;
                    }
                }
            }
            return core::ptr::null_mut();
        }

        // Dynamic: round-robin, но только если есть другие ready потоки.
        // Priority decay для boosted потоков (минимальная база).
        // Делается на конце кванта (даже если больше нет ready потоков).
        if (*current).priority_decrement > 0 {
            (*current).priority = crate::ke::priority::ki_compute_new_priority(current, 1);
        }

        // NT/ReactOS: динамический поток в конце кванта уступает CPU только если есть
        // ready-поток с приоритетом >= текущего (после decay).
        // Если есть только более низкие приоритеты — поток продолжает выполняться с reset quantum.
        let ready_summary = (*prcb).ready_summary;
        let cur_prio = (*current).priority as u32;
        let mask = if cur_prio == 0 {
            !0u32
        } else {
            !((1u32 << cur_prio) - 1)
        };
        if (ready_summary & mask) != 0 {
            ki_ready_thread(current);
            if let Some(next) = ki_find_ready_thread((*prcb).number as u32, cur_prio as i32) {
                let t = next.as_ptr();
                (*t).set_state(super::thread::KTHREAD_STATE::Standby);
                ki_set_prcb_next_thread(prcb, t);
                return t;
            }
        }

        core::ptr::null_mut()
    }
}

/// Возвращает адрес обработчика APIC таймера
///
/// Возвращает адрес ASM stub KiClockInterrupt из interrupt.S
pub fn get_apic_timer_handler() -> u64 {
    super::swint_asm::KiClockInterrupt as *const () as u64
}

// =============================================================================
// APIC Timer Handler
// =============================================================================

/// Счётчик clock interrupt для диагностики
static CLOCK_INT_COUNTER: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// KiClockInterruptHandler - обработчик прерывания APIC Timer
///
/// Вызывается из ASM stub KiClockInterrupt (interrupt.S).
/// IRQL уже поднят до CLOCK_LEVEL в ASM stub.
///
/// # Аргументы
/// * `user_mode` - был ли прерван user mode код (пока не используется,
///   ASM stub передаёт 0 для консервативности)
///
/// Источник: ReactOS hal/halx86/apic/apictimer.c
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn KiClockInterruptHandler(_user_mode: u8) {
    unsafe {
        // IRQL уже поднят в ASM stub через KiEnterHardwareInterrupt(CLOCK_LEVEL)

        // 1. Обновляем счетчики в APIC модуле
        crate::hal::apic_clock_interrupt();

        // 2. Получаем time increment
        let increment = crate::hal::apic_query_time_increment();

        // 3. KeUpdateSystemTime
        super::time::ke_update_system_time(increment);

        // 4. KeUpdateRunTime
        // TODO: получать user_mode из interrupt frame в ASM stub
        let user_mode_flag = _user_mode != 0;
        ke_update_run_time(user_mode_flag);

        // 5. Обновляем DPC request rate
        let prcb = crate::arch::x86_64::pcr::get_prcb();
        if !prcb.is_null() {
            let current_count = (*prcb).dpc_count;
            let last_count = (*prcb).dpc_last_count;
            (*prcb).dpc_request_rate = current_count.saturating_sub(last_count);
            (*prcb).dpc_last_count = current_count;
        }

        // 6. Quantum tick
        ki_quantum_tick();

        // 7. Отправляем EOI в APIC
        crate::hal::apic_send_eoi();

        // 8. Проверяем нужен ли dispatch
        if !prcb.is_null() {
            if (*prcb).quantum_end != 0 || (*prcb).dpc_requested != 0 {
                crate::hal::init::hal_request_dispatch();
            }
        }

        // IRQL будет понижен в ASM stub через KiExitHardwareInterrupt
    }
}

/// Заполняет структуру ExceptionHandlers адресами обработчиков
pub fn get_exception_handlers() -> crate::arch::x86_64::idt::ExceptionHandlers {
    crate::arch::x86_64::idt::ExceptionHandlers {
        divide_error: divide_error_handler as *const () as u64,
        debug: debug_handler as *const () as u64,
        nmi: nmi_handler as *const () as u64,
        breakpoint: breakpoint_handler as *const () as u64,
        overflow: overflow_handler as *const () as u64,
        bound_range: bound_range_handler as *const () as u64,
        invalid_opcode: invalid_opcode_handler as *const () as u64,
        device_not_available: device_not_available_handler as *const () as u64,
        double_fault: double_fault_handler as *const () as u64,
        invalid_tss: invalid_tss_handler as *const () as u64,
        segment_not_present: segment_not_present_handler as *const () as u64,
        stack_segment_fault: stack_segment_fault_handler as *const () as u64,
        // Вариант A: используем ASM trap stubs для #GP/#PF, чтобы гарантировать 16-byte alignment
        // и получить первопричину исключения (без вторичного #GP в прологе Rust handler-а).
        general_protection: trap_asm::KiGeneralProtectionFault as *const () as u64,
        page_fault: trap_asm::KiPageFault as *const () as u64,
        x87_fpu_error: x87_fpu_error_handler as *const () as u64,
        alignment_check: alignment_check_handler as *const () as u64,
        machine_check: machine_check_handler as *const () as u64,
        simd_fp: simd_fp_handler as *const () as u64,
    }
}

/// Заполняет структуру ApicHandlers адресами обработчиков APIC
///
/// Все обработчики - ASM stubs из interrupt.S, которые вызывают
/// соответствующие Rust handlers с корректным выравниванием стека.
pub fn get_apic_handlers() -> crate::arch::x86_64::idt::ApicHandlers {
    use super::swint_asm::KiClockInterrupt;
    use super::swint_asm::KiErrorInterrupt;
    use super::swint_asm::KiSpuriousInterrupt;

    crate::arch::x86_64::idt::ApicHandlers {
        timer: KiClockInterrupt as *const () as u64,
        spurious: KiSpuriousInterrupt as *const () as u64,
        error: KiErrorInterrupt as *const () as u64,
    }
}

/// Заполняет структуру SwintHandlers адресами обработчиков software interrupts
///
/// Используются для APC (vector 0x1F) и DPC (vector 0x2F) доставки через self-IPI.
pub fn get_swint_handlers() -> crate::arch::x86_64::idt::SwintHandlers {
    use super::swint_asm::KiApcInterrupt;
    use super::swint_asm::KiDispatchInterrupt;

    crate::arch::x86_64::idt::SwintHandlers {
        apc: KiApcInterrupt as *const () as u64,
        dispatch: KiDispatchInterrupt as *const () as u64,
    }
}

/// KiErrorInterruptHandler - обработчик ошибок APIC
///
/// Вызывается из ASM stub KiErrorInterrupt (interrupt.S).
/// EOI уже отправлен в ASM stub.
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn KiErrorInterruptHandler() {
    // Читаем и очищаем ESR (Error Status Register)
    let esr = crate::hal::apic_read(crate::hal::ApicRegister::Esr);
    crate::hal::apic_write(crate::hal::ApicRegister::Esr, 0);

    // Логируем ошибку (пока просто игнорируем)
    let _ = esr;

    // EOI уже отправлен в ASM stub
}

// =============================================================================
// Device Interrupt Handler
// =============================================================================

/// KiDeviceInterruptHandler - generic handler for device interrupts
///
/// Called from ASM stubs for device interrupt vectors (0x30+).
/// Dispatches to registered driver ISR via HAL.
///
/// # Arguments
/// * `vector` - interrupt vector number
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn KiDeviceInterruptHandler(vector: u8) {
    unsafe {
        // Begin interrupt - raise IRQL
        // Device IRQL is based on vector: higher vectors get lower IRQL
        let device_irql = crate::hal::irql::DEVICE_IRQL_BASE + (0x4F - vector).min(10);
        let mut old_irql: u8 = 0;
        let is_real = crate::hal::interrupt::hal_begin_system_interrupt(
            device_irql,
            vector,
            &mut old_irql,
        );
        
        if !is_real {
            // Spurious interrupt - don't process
            return;
        }
        
        // Dispatch to registered handler
        crate::hal::init::hal_dispatch_interrupt(vector);
        
        // End interrupt - send EOI and lower IRQL
        crate::hal::interrupt::hal_end_system_interrupt(old_irql, true);
    }
}

/// Device interrupt entry for specific vector
/// Macro generates entry points for each device IRQ vector
macro_rules! device_irq_handler {
    ($name:ident, $vector:expr) => {
        #[unsafe(no_mangle)]
        pub unsafe extern "x86-interrupt" fn $name(_frame: InterruptFrame) {
            unsafe {
                KiDeviceInterruptHandler($vector);
            }
        }
    };
}

// Generate handlers for ISA/PCI IRQ vectors (0x30-0x4F)
// These map to GSI 0-31 (vector = 0x30 + GSI)
device_irq_handler!(KiDeviceIrq0, 0x30);
device_irq_handler!(KiDeviceIrq1, 0x31);
device_irq_handler!(KiDeviceIrq2, 0x32);
device_irq_handler!(KiDeviceIrq3, 0x33);
device_irq_handler!(KiDeviceIrq4, 0x34);
device_irq_handler!(KiDeviceIrq5, 0x35);
device_irq_handler!(KiDeviceIrq6, 0x36);
device_irq_handler!(KiDeviceIrq7, 0x37);
device_irq_handler!(KiDeviceIrq8, 0x38);
device_irq_handler!(KiDeviceIrq9, 0x39);
device_irq_handler!(KiDeviceIrq10, 0x3A);
device_irq_handler!(KiDeviceIrq11, 0x3B);
device_irq_handler!(KiDeviceIrq12, 0x3C);
device_irq_handler!(KiDeviceIrq13, 0x3D);
device_irq_handler!(KiDeviceIrq14, 0x3E);
device_irq_handler!(KiDeviceIrq15, 0x3F);
// Higher IRQs (PCI typically uses 16+)
device_irq_handler!(KiDeviceIrq16, 0x40);
device_irq_handler!(KiDeviceIrq17, 0x41);
device_irq_handler!(KiDeviceIrq18, 0x42);
device_irq_handler!(KiDeviceIrq19, 0x43);
device_irq_handler!(KiDeviceIrq20, 0x44);
device_irq_handler!(KiDeviceIrq21, 0x45);
device_irq_handler!(KiDeviceIrq22, 0x46);
device_irq_handler!(KiDeviceIrq23, 0x47);

/// Structure containing device interrupt handler addresses
pub struct DeviceInterruptHandlers {
    pub handlers: [u64; 24],
}

/// Get device interrupt handler addresses for IDT setup
pub fn get_device_interrupt_handlers() -> DeviceInterruptHandlers {
    DeviceInterruptHandlers {
        handlers: [
            KiDeviceIrq0 as *const () as u64,
            KiDeviceIrq1 as *const () as u64,
            KiDeviceIrq2 as *const () as u64,
            KiDeviceIrq3 as *const () as u64,
            KiDeviceIrq4 as *const () as u64,
            KiDeviceIrq5 as *const () as u64,
            KiDeviceIrq6 as *const () as u64,
            KiDeviceIrq7 as *const () as u64,
            KiDeviceIrq8 as *const () as u64,
            KiDeviceIrq9 as *const () as u64,
            KiDeviceIrq10 as *const () as u64,
            KiDeviceIrq11 as *const () as u64,
            KiDeviceIrq12 as *const () as u64,
            KiDeviceIrq13 as *const () as u64,
            KiDeviceIrq14 as *const () as u64,
            KiDeviceIrq15 as *const () as u64,
            KiDeviceIrq16 as *const () as u64,
            KiDeviceIrq17 as *const () as u64,
            KiDeviceIrq18 as *const () as u64,
            KiDeviceIrq19 as *const () as u64,
            KiDeviceIrq20 as *const () as u64,
            KiDeviceIrq21 as *const () as u64,
            KiDeviceIrq22 as *const () as u64,
            KiDeviceIrq23 as *const () as u64,
        ],
    }
}
