//! Trap/Exception Assembly Stubs (AMD64)
//!
//! Вариант A: NT/ReactOS-style trap stubs.
//! Задача: выровнять стек (16-byte) и вызвать Rust handler по Windows x64 ABI.
//! Для #GP/#PF на текущем этапе handler всегда bugcheck'ит и не возвращается,
//! поэтому stub не делает restore/iretq.

use core::arch::global_asm;

// Trap codes (UNEXPECTED_KERNEL_MODE_TRAP Param1)
const TRAP_GENERAL_PROTECTION: u32 = 13;

global_asm!(
    r#"
.global KiGeneralProtectionFault

KiGeneralProtectionFault:
    // Stack layout on entry (no CPL change, has error code):
    //   [rsp + 0]  = error code
    //   [rsp + 8]  = RIP
    //   [rsp + 16] = CS
    //   [rsp + 24] = RFLAGS
    //
    // interrupted_rsp = rsp + 32
    mov r11, rsp
    // Сохраняем RBP из прерванного контекста (CPU его не пушит)
    mov r10, rbp

    // Win64 ABI args:
    // RCX = trap_code (u32)
    // RDX = error_code (u64)
    // R8  = rip (u64)
    // R9  = interrupted_rsp (u64)
    mov ecx, {trap_code}
    mov rdx, [r11 + 0]
    mov r8,  [r11 + 8]
    lea r9,  [r11 + 32]

    // Ensure Win64 ABI stack alignment + shadow space for Rust callout.
    // В Win64 ABI перед `call` нужно иметь RSP ≡ 8 (mod 16),
    // и выделить shadow space (32 байта).
    // Мы не возвращаемся из bugcheck, поэтому можем выровнять RSP агрессивно.
    and rsp, -16
    sub rsp, 0x38
    // 5-й аргумент (rbp) передаём через стек: [rsp + 0x20]
    mov [rsp + 0x20], r10
    call KiTrapBugCheckWithError

1:
    hlt
    jmp 1b
"#,
    trap_code = const TRAP_GENERAL_PROTECTION,
);

global_asm!(
    r#"
.global KiPageFault

KiPageFault:
    // Stack layout on entry (no CPL change, has error code):
    //   [rsp + 0]  = error code
    //   [rsp + 8]  = RIP
    //   [rsp + 16] = CS
    //   [rsp + 24] = RFLAGS
    //
    // Сохраняем оригинальный RSP в r15 (callee-saved)
    push r15
    push r14
    push r13
    push r12
    push rbx
    push rbp
    
    mov r15, rsp
    add r15, 48              // r15 = original rsp (before pushes)
    
    // Читаем CR2 (fault address) сразу, пока не затёрли
    mov r14, cr2
    
    // Сохраняем error_code и rip
    mov r13, [r15 + 0]       // error_code
    mov r12, [r15 + 8]       // rip
    
    // Win64 ABI args для MmAccessFault:
    // RCX = fault_address (CR2)
    // RDX = error_code
    // R8  = trap_frame_rip
    mov rcx, r14
    mov rdx, r13
    mov r8, r12
    
    // Выравниваем стек и выделяем shadow space
    and rsp, -16
    sub rsp, 0x28
    
    // Вызываем MmAccessFault
    call MmAccessFault
    
    // Проверяем результат (EAX = NTSTATUS)
    // STATUS_SUCCESS = 0 -> fault обработан, retry инструкцию
    test eax, eax
    jnz .page_fault_failed
    
    // Fault обработан успешно — восстанавливаем регистры
    mov rsp, r15
    sub rsp, 48              // point back to saved regs
    
    pop rbp
    pop rbx
    pop r12
    pop r13
    pop r14
    pop r15
    
    // Снимаем error code и возвращаемся
    add rsp, 8
    iretq

.page_fault_failed:
    // Fault не обработан — вызываем bugcheck
    // Win64 ABI args для KiPageFaultBugCheck:
    // RCX = error_code
    // RDX = rip
    // R8  = interrupted_rsp
    mov rcx, r13             // error_code
    mov rdx, r12             // rip
    lea r8,  [r15 + 32]      // interrupted_rsp
    
    call KiPageFaultBugCheck
    
1:
    hlt
    jmp 1b
"#,
);

unsafe extern "C" {
    /// #GP stub (vector 13)
    pub fn KiGeneralProtectionFault();
    /// #PF stub (vector 14)
    pub fn KiPageFault();
}
