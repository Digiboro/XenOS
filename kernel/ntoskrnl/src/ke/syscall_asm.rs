//! Syscall Entry Point Assembly (AMD64)
//!
//! ASM stub для обработки инструкции SYSCALL.
//!
//! # Архитектура входа через SYSCALL
//!
//! При выполнении SYSCALL процессор:
//! 1. Сохраняет RIP в RCX
//! 2. Сохраняет RFLAGS в R11
//! 3. Загружает CS из STAR[47:32]
//! 4. Загружает SS = CS + 8
//! 5. Загружает RIP из LSTAR
//! 6. Маскирует RFLAGS по FMASK
//!
//! # Соглашение о регистрах
//!
//! При входе в KiSystemCall64:
//! - RAX = номер системного вызова
//! - RCX = return address (сохранён процессором)
//! - R11 = RFLAGS (сохранён процессором)
//! - R10 = первый аргумент (перенесён из RCX в user stub)
//! - RDX = второй аргумент
//! - R8  = третий аргумент
//! - R9  = четвёртый аргумент
//! - User stack содержит аргументы 5+
//!
//! Источники:
//! - NT6.1: ke/amd64/trap.asm (KiSystemCall64)
//! - ReactOS: ke/amd64/trap.S
//! - AMD64 Architecture Manual

use core::arch::global_asm;

// Смещения в KPCR/KPRCB для доступа через GS
// Эти значения должны соответствовать структурам в arch/x86_64/pcr.rs

/// Смещение KPCR.current_prcb
const KPCR_CURRENT_PRCB: usize =
    core::mem::offset_of!(crate::arch::x86_64::pcr::KPCR, current_prcb);

/// Смещение KPRCB.current_thread
const KPRCB_CURRENT_THREAD: usize =
    core::mem::offset_of!(crate::arch::x86_64::pcr::KPRCB, current_thread);

/// Смещение KPRCB.rsp_base (kernel stack)
const KPRCB_RSP_BASE: usize = core::mem::offset_of!(crate::arch::x86_64::pcr::KPRCB, rsp_base);

/// Смещение KTHREAD.previous_mode
const KTHREAD_PREVIOUS_MODE: usize =
    core::mem::offset_of!(crate::ke::thread::KTHREAD, previous_mode);

/// Смещение KTHREAD.kernel_stack
const KTHREAD_KERNEL_STACK: usize = core::mem::offset_of!(crate::ke::thread::KTHREAD, kernel_stack);

// =============================================================================
// KiSystemCall64 - основной entrypoint для SYSCALL (64-bit)
// =============================================================================

global_asm!(
    r#"
.global KiSystemCall64

// =============================================================================
// KiSystemCall64
//
// Точка входа для инструкции SYSCALL из 64-bit user mode.
//
// На входе:
//   RAX = номер системного вызова
//   RCX = return address (user RIP, сохранён CPU)
//   R11 = user RFLAGS (сохранён CPU)
//   R10 = arg1 (перенесён из RCX в user stub)
//   RDX = arg2
//   R8  = arg3
//   R9  = arg4
//   RSP = user stack (args 5+ находятся по [rsp+0x28] в user space)
//
// На выходе:
//   RAX = NTSTATUS результат
//   Восстановлены: RCX, R11, RSP
//   Возврат через SYSRET
// =============================================================================

KiSystemCall64:
    // =========================================================================
    // Шаг 1: swapgs — переключаемся на kernel GS (KPCR)
    // =========================================================================
    swapgs
    
    // =========================================================================
    // Шаг 2: Сохраняем user RSP и переключаемся на kernel stack
    // =========================================================================
    // Сохраняем user RSP во временный регистр
    mov     gs:[{kpcr_user_rsp_scratch}], rsp
    
    // Загружаем kernel RSP из KPRCB.rsp_base (или KTHREAD.kernel_stack)
    mov     rsp, gs:[{kpcr_current_prcb}]
    mov     rsp, [rsp + {kprcb_rsp_base}]
    
    // =========================================================================
    // Шаг 3: Выделяем место и сохраняем volatile регистры
    // =========================================================================
    // Резервируем пространство для SYSCALL frame
    // Структура на стеке (сверху вниз):
    //   [rsp + 0x58] = user RSP
    //   [rsp + 0x50] = user RFLAGS (R11)
    //   [rsp + 0x48] = user RIP (RCX)
    //   [rsp + 0x40] = RAX (service number, сохраняем для отладки)
    //   [rsp + 0x38] = R10 (arg1)
    //   [rsp + 0x30] = RDX (arg2)
    //   [rsp + 0x28] = R8 (arg3)
    //   [rsp + 0x20] = R9 (arg4)
    //   [rsp + 0x00] = shadow space (32 bytes для Win64 ABI)
    
    sub     rsp, 0x60
    
    // Сохраняем user context
    mov     [rsp + 0x48], rcx        // user RIP
    mov     [rsp + 0x50], r11        // user RFLAGS
    
    // Сохраняем user RSP (был во временном месте)
    push    rax                       // временно сохраняем RAX
    mov     rax, gs:[{kpcr_user_rsp_scratch}]
    mov     [rsp + 0x60], rax         // user RSP (+8 из-за push)
    pop     rax
    
    // Сохраняем аргументы и service number
    mov     [rsp + 0x40], rax        // service number
    mov     [rsp + 0x38], r10        // arg1
    mov     [rsp + 0x30], rdx        // arg2
    mov     [rsp + 0x28], r8         // arg3
    mov     [rsp + 0x20], r9         // arg4
    
    // =========================================================================
    // Шаг 4: Устанавливаем PreviousMode = UserMode
    // =========================================================================
    mov     rbx, gs:[{kpcr_current_prcb}]
    mov     rbx, [rbx + {kprcb_current_thread}]
    mov     byte ptr [rbx + {kthread_previous_mode}], 1  // UserMode = 1
    
    // =========================================================================
    // Шаг 5: Вызываем диспетчер ki_system_service_dispatch
    // =========================================================================
    // Win64 ABI: RCX, RDX, R8, R9 = аргументы
    // Наш диспетчер: ki_system_service_dispatch(service_id, arg1, arg2, arg3, arg4)
    // Но у нас только 4 регистра, а нужно 5 аргументов...
    // Решение: service_id в ECX (32-bit), остальные сдвигаются
    
    mov     ecx, eax                 // service_id (32-bit)
    mov     rdx, r10                 // arg1
    mov     r8, [rsp + 0x30]         // arg2 (сохранённый RDX)
    mov     r9, [rsp + 0x28]         // arg3 (сохранённый R8)
    // arg4 передаётся на стеке (уже в shadow space + 0x20)
    mov     rax, [rsp + 0x20]        // arg4 (сохранённый R9)
    mov     [rsp + 0x20], rax        // копируем в правильное место shadow space
    
    call    ki_system_service_dispatch
    
    // RAX теперь содержит NTSTATUS результат
    
    // =========================================================================
    // Шаг 6: Восстанавливаем PreviousMode = KernelMode (опционально)
    // =========================================================================
    // В NT это обычно не делается — PreviousMode остаётся для следующего syscall
    // Но мы сбрасываем для безопасности
    mov     rbx, gs:[{kpcr_current_prcb}]
    mov     rbx, [rbx + {kprcb_current_thread}]
    mov     byte ptr [rbx + {kthread_previous_mode}], 0  // KernelMode = 0
    
    // =========================================================================
    // Шаг 7: Восстанавливаем user context и возвращаемся через SYSRET
    // =========================================================================
    mov     rcx, [rsp + 0x48]        // user RIP -> RCX (для SYSRET)
    mov     r11, [rsp + 0x50]        // user RFLAGS -> R11 (для SYSRET)
    mov     rsp, [rsp + 0x58]        // user RSP
    
    // swapgs — возвращаемся к user GS
    swapgs
    
    // SYSRET: RIP <- RCX, RFLAGS <- R11, CS <- STAR[63:48]+16, SS <- STAR[63:48]+8
    sysretq

// =============================================================================
// Временное хранилище для user RSP
// Это поле должно быть добавлено в KPCR или использовать существующее место
// Пока используем spare[0] в KPCR
// =============================================================================
"#,
    kpcr_current_prcb = const KPCR_CURRENT_PRCB,
    kprcb_current_thread = const KPRCB_CURRENT_THREAD,
    kprcb_rsp_base = const KPRCB_RSP_BASE,
    kthread_previous_mode = const KTHREAD_PREVIOUS_MODE,
    // Временное место для user RSP — используем spare[0] в KPCR
    // KPCR.spare[0] = offset 0xB8 (после kd_secondary_version_block)
    kpcr_user_rsp_scratch = const core::mem::offset_of!(crate::arch::x86_64::pcr::KPCR, spare),
);

unsafe extern "C" {
    /// KiSystemCall64 — точка входа SYSCALL для 64-bit user mode
    pub fn KiSystemCall64();
}

/// Возвращает адрес KiSystemCall64 для установки в IA32_LSTAR
pub fn get_syscall_entry_point() -> u64 {
    KiSystemCall64 as *const () as u64
}
