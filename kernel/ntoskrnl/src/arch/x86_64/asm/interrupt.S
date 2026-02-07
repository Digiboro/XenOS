// =============================================================================
// interrupt.S - Interrupt Stubs для x86_64
//
// ASM stubs для software и hardware interrupts.
// Обеспечивают корректное выравнивание стека (16 байт) перед вызовом
// Rust handlers по Windows x64 ABI.
//
// Задача stubs:
// - Сохранить volatile регистры (Windows x64 ABI)
// - Динамически выровнять стек на 16 байт
// - Поднять IRQL через KiEnterSoftwareInterrupt / KiEnterHardwareInterrupt
// - Вызвать Rust handler
// - Восстановить IRQL через KiExitSoftwareInterrupt / KiExitHardwareInterrupt
// - Восстановить регистры и выполнить iretq
//
// Источники:
// - ReactOS: hal/halx86/apic/apictimer.c, ke/i386/traphdlr.c
// - NT5: ke/amd64/trap.asm
// =============================================================================

.intel_syntax noprefix

.text

// =============================================================================
// Константы
// =============================================================================

// Local APIC MMIO адреса (HAL fixed mapping)
.set LOCAL_APIC_BASE,       0xFFFFFFFFFFFE0000
.set APIC_EOI_OFFSET,       0x0B0

// IRQL уровни
.set APC_LEVEL,             1
.set DISPATCH_LEVEL,        2
.set CLOCK_LEVEL,           13

// =============================================================================
// Макрос для сохранения volatile регистров
// =============================================================================
.macro SAVE_VOLATILE_REGS
    push rbx
    push rax
    push rcx
    push rdx
    push r8
    push r9
    push r10
    push r11
.endm

// =============================================================================
// Макрос для восстановления volatile регистров
// =============================================================================
.macro RESTORE_VOLATILE_REGS
    pop r11
    pop r10
    pop r9
    pop r8
    pop rdx
    pop rcx
    pop rax
    pop rbx
.endm

// =============================================================================
// Макрос для выравнивания стека на 16 байт + shadow space
// После выполнения: BL = 1 если добавлено 8 байт для выравнивания
// =============================================================================
.macro ALIGN_STACK_AND_SHADOW
    xor ebx, ebx
    sub rsp, 0x20           // Shadow space (32 байта)
    test rsp, 0xF           // Проверяем выравнивание
    jz 1f
    sub rsp, 0x8            // Дополнительные 8 байт если нужно
    mov bl, 1
1:
.endm

// =============================================================================
// Макрос для восстановления стека после ALIGN_STACK_AND_SHADOW
// =============================================================================
.macro RESTORE_STACK_ALIGNMENT
    test bl, bl
    jz 1f
    add rsp, 0x8
1:
    add rsp, 0x20
.endm

// =============================================================================
// Макрос для отправки EOI в Local APIC
// =============================================================================
.macro SEND_APIC_EOI
    mov rax, LOCAL_APIC_BASE
    mov dword ptr [rax + APIC_EOI_OFFSET], 0
.endm

// =============================================================================
// KiApcInterrupt - APC Software Interrupt (vector 0x1F)
//
// Вызывается через self-IPI для доставки APC.
// =============================================================================
.global KiApcInterrupt

KiApcInterrupt:
    // Сохраняем volatile регистры
    // RBX используем как scratch:
    // - BL: флаг дополнительного выравнивания
    // - BH: OldIrql
    SAVE_VOLATILE_REGS
    ALIGN_STACK_AND_SHADOW

    // Отправляем EOI в APIC
    SEND_APIC_EOI

    // KiEnterSoftwareInterrupt(APC_LEVEL) -> AL = OldIrql
    mov ecx, APC_LEVEL
    call KiEnterSoftwareInterrupt
    mov bh, al

    // Вызываем handler
    call KiApcInterruptHandler

    // KiExitSoftwareInterrupt(OldIrql)
    movzx ecx, bh
    call KiExitSoftwareInterrupt

    RESTORE_STACK_ALIGNMENT
    RESTORE_VOLATILE_REGS
    iretq

// =============================================================================
// KiDispatchInterrupt - DPC/Dispatch Software Interrupt (vector 0x2F)
//
// Вызывается через self-IPI для обработки DPC очереди и переключения потоков.
// =============================================================================
.global KiDispatchInterrupt

KiDispatchInterrupt:
    SAVE_VOLATILE_REGS
    ALIGN_STACK_AND_SHADOW

    SEND_APIC_EOI

    // KiEnterSoftwareInterrupt(DISPATCH_LEVEL) -> AL = OldIrql
    mov ecx, DISPATCH_LEVEL
    call KiEnterSoftwareInterrupt
    mov bh, al

    call KiDispatchInterruptHandler

    // KiExitSoftwareInterrupt(OldIrql)
    movzx ecx, bh
    call KiExitSoftwareInterrupt

    RESTORE_STACK_ALIGNMENT
    RESTORE_VOLATILE_REGS
    iretq

// =============================================================================
// KiClockInterrupt - APIC Timer Hardware Interrupt (vector 0xD1)
//
// Обработчик прерывания таймера APIC. Вызывается на каждый tick.
// В отличие от software interrupts, EOI отправляется в handler'е
// после обработки (чтобы избежать nested interrupts на том же уровне).
// =============================================================================
.global KiClockInterrupt

KiClockInterrupt:
    SAVE_VOLATILE_REGS
    ALIGN_STACK_AND_SHADOW

    // НЕ отправляем EOI здесь - handler сам отправит после обработки

    // KiEnterHardwareInterrupt(CLOCK_LEVEL) -> AL = OldIrql
    mov ecx, CLOCK_LEVEL
    call KiEnterHardwareInterrupt
    mov bh, al

    // Передаём user_mode флаг в RCX (первый аргумент Win64)
    // Interrupt frame на стеке: после наших push'ей нужно вычислить смещение
    // Stack layout после SAVE_VOLATILE_REGS (8 regs * 8 = 64 байт) + alignment:
    // [rsp + offset] содержит CS из interrupt frame
    // Но проще получить CS из сохранённого места или передать 0 (kernel mode assumed)
    // TODO: корректно определять user_mode из interrupt frame
    xor ecx, ecx            // user_mode = 0 (kernel mode, conservative)

    call KiClockInterruptHandler

    // KiExitHardwareInterrupt(OldIrql)
    movzx ecx, bh
    call KiExitHardwareInterrupt

    RESTORE_STACK_ALIGNMENT
    RESTORE_VOLATILE_REGS
    iretq

// =============================================================================
// KiErrorInterrupt - APIC Error Interrupt (vector 0xE2)
//
// Обработчик ошибок APIC.
// =============================================================================
.global KiErrorInterrupt

KiErrorInterrupt:
    SAVE_VOLATILE_REGS
    ALIGN_STACK_AND_SHADOW

    SEND_APIC_EOI

    call KiErrorInterruptHandler

    RESTORE_STACK_ALIGNMENT
    RESTORE_VOLATILE_REGS
    iretq

// =============================================================================
// KiSpuriousInterrupt - APIC Spurious Interrupt (vector 0xDF)
//
// Spurious interrupt - просто игнорируем, НЕ отправляем EOI.
// =============================================================================
.global KiSpuriousInterrupt

KiSpuriousInterrupt:
    // Spurious interrupt - ничего не делаем, НЕ отправляем EOI
    iretq
