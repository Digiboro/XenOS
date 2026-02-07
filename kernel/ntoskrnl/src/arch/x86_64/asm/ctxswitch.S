// =============================================================================
// ctxswitch.S - Context Switch для x86_64
//
// Низкоуровневый код переключения контекста между потоками.
// Реализация основана на Windows NT 6.1 (Windows 7) и ReactOS.
//
// ВАЖНО: Константы смещений должны совпадать с static assertions в context.rs!
// При изменении структур KTHREAD, KSWITCH_FRAME и т.д. нужно обновить
// соответствующие .set и проверить что сборка проходит.
//
// Источники:
// - ReactOS: ntoskrnl/ke/amd64/ctxswitch.S
// - NT5: ke/amd64/ctxswap.asm
// =============================================================================

// Intel syntax (как в оригинальном коде и ReactOS)
.intel_syntax noprefix

.text

.global ki_swap_context_internal
.global ki_thread_startup
.global ki_invalid_system_thread_startup_exit
.global ki_user_thread_startup_exit

// =============================================================================
// Константы смещений (ABI контракт - проверяется в context.rs)
// =============================================================================

// Смещения в KTHREAD (NT 6.1 compatible)
.set ThKernelStack,         0x38    // KTHREAD.kernel_stack
.set ThInitialStack,        0x28    // KTHREAD.initial_stack
.set ThState,               0x78    // KTHREAD.state

// Смещения в KPCR (для GS-relative доступа)
.set PcrIrql,               0x70    // KPCR.irql

// Смещения в KSWITCH_FRAME (256 байт, align 16)
// XMM non-volatile регистры (Windows x64 ABI: XMM6-XMM15)
.set SwXmm6,                0x00
.set SwXmm7,                0x10
.set SwXmm8,                0x20
.set SwXmm9,                0x30
.set SwXmm10,               0x40
.set SwXmm11,               0x50
.set SwXmm12,               0x60
.set SwXmm13,               0x70
.set SwXmm14,               0x80
.set SwXmm15,               0x90
// MXCSR и служебные поля
.set SwMxCsr,               0xA0
.set SwApcBypass,           0xA8
// Integer non-volatile регистры
.set SwRbx,                 0xB0
.set SwRdi,                 0xB8
.set SwRsi,                 0xC0
.set SwR12,                 0xC8
.set SwR13,                 0xD0
.set SwR14,                 0xD8
.set SwR15,                 0xE0
// RBP и return address
.set SwRbp,                 0xF0
.set SwReturn,              0xF8

// Смещения в KSTART_FRAME (относительно RSP после ret из KSWITCH_FRAME)
.set SfP1Home,              0x00    // StartRoutine
.set SfP2Home,              0x08    // StartContext
.set SfP4Home,              0x18    // SystemRoutine
.set SfReturn,              0x28    // Return address

// Размеры фреймов
.set KSWITCH_FRAME_LENGTH,  256     // 0x100

// Состояния потока
.set Running,               2

// =============================================================================
// ki_swap_context_internal - переключение контекста
//
// Windows x64 ABI:
//   CL  = WaitIrql
//   RDX = OldThread (PKTHREAD)
//   R8  = NewThread (PKTHREAD)
//
// Returns:
//   AL = 1 если kernel APC pending, иначе 0
// =============================================================================
ki_swap_context_internal:
    // Сохраняем RBP
    push rbp
    
    // Выделяем место под KSWITCH_FRAME
    // После push rbp и call на стеке уже 16 байт (RBP + return addr)
    // Выделяем остальное: 256 - 16 = 240 байт
    sub rsp, (KSWITCH_FRAME_LENGTH - 16)

    // Сохраняем XMM non-volatile регистры (Windows x64 ABI: XMM6-XMM15)
    movaps [rsp + SwXmm6], xmm6
    movaps [rsp + SwXmm7], xmm7
    movaps [rsp + SwXmm8], xmm8
    movaps [rsp + SwXmm9], xmm9
    movaps [rsp + SwXmm10], xmm10
    movaps [rsp + SwXmm11], xmm11
    movaps [rsp + SwXmm12], xmm12
    movaps [rsp + SwXmm13], xmm13
    movaps [rsp + SwXmm14], xmm14
    movaps [rsp + SwXmm15], xmm15
    
    // Сохраняем MXCSR
    stmxcsr [rsp + SwMxCsr]

    // Сохраняем integer non-volatile регистры
    mov [rsp + SwRbx], rbx
    mov [rsp + SwRdi], rdi
    mov [rsp + SwRsi], rsi
    mov [rsp + SwR12], r12
    mov [rsp + SwR13], r13
    mov [rsp + SwR14], r14
    mov [rsp + SwR15], r15

    // Сохраняем WaitIrql как ApcBypass
    mov [rsp + SwApcBypass], cl

    // =========================================================================
    // ПЕРЕКЛЮЧЕНИЕ СТЕКОВ
    // =========================================================================
    
    // Сохраняем RSP старого потока
    mov [rdx + ThKernelStack], rsp

    // Загружаем RSP нового потока
    mov rsp, [r8 + ThKernelStack]

    // Восстанавливаем ApcBypass из стека нового потока
    movzx ecx, byte ptr [rsp + SwApcBypass]

    // =========================================================================
    // ВЫЗОВ RUST ki_swap_context_resume
    // =========================================================================
    
    // Windows x64 ABI требует:
    // 1. 32 байта shadow space
    // 2. RSP % 16 == 0 перед call (после call RSP % 16 == 8)
    //
    // kernel_stack ВСЕГДА выровнен на 16 байт (требование XSAVE/movaps).
    // Поэтому используем фиксированный размер аллокации:
    // - sub rsp, 0x28 (40 байт) -> RSP % 16 == 8 (т.к. 0 - 40 = 8 mod 16)
    // - sub rsp, 0x08 -> RSP % 16 == 0 (выровнено для call)
    // - итого: sub rsp, 0x30 (48 байт)
    
    sub rsp, 0x30
    call ki_swap_context_resume
    add rsp, 0x30

    // Сохраняем результат (AL содержит APC pending флаг)
    movzx r9d, al

    // =========================================================================
    // ВОССТАНОВЛЕНИЕ РЕГИСТРОВ НОВОГО ПОТОКА
    // =========================================================================
    
    // Восстанавливаем MXCSR
    ldmxcsr [rsp + SwMxCsr]

    // Восстанавливаем XMM non-volatile регистры
    movaps xmm6, [rsp + SwXmm6]
    movaps xmm7, [rsp + SwXmm7]
    movaps xmm8, [rsp + SwXmm8]
    movaps xmm9, [rsp + SwXmm9]
    movaps xmm10, [rsp + SwXmm10]
    movaps xmm11, [rsp + SwXmm11]
    movaps xmm12, [rsp + SwXmm12]
    movaps xmm13, [rsp + SwXmm13]
    movaps xmm14, [rsp + SwXmm14]
    movaps xmm15, [rsp + SwXmm15]

    // Восстанавливаем integer non-volatile регистры
    mov rbx, [rsp + SwRbx]
    mov rdi, [rsp + SwRdi]
    mov rsi, [rsp + SwRsi]
    mov r12, [rsp + SwR12]
    mov r13, [rsp + SwR13]
    mov r14, [rsp + SwR14]
    mov r15, [rsp + SwR15]

    // Восстанавливаем результат
    mov eax, r9d

    // Восстанавливаем стек и RBP
    add rsp, (KSWITCH_FRAME_LENGTH - 16)
    pop rbp
    ret

// =============================================================================
// ki_thread_startup - точка входа для новых потоков
//
// При входе:
//   RSP указывает на начало KSTART_FRAME
//   KSTART_FRAME содержит параметры для SystemRoutine
// =============================================================================
ki_thread_startup:
    // Очищаем регистры для безопасности
    xor rbx, rbx
    xor rsi, rsi
    xor rdi, rdi
    xor rbp, rbp
    xor r10, r10
    xor r11, r11
    xor r12, r12
    xor r13, r13
    xor r14, r14
    xor r15, r15

    // Устанавливаем IRQL в APC_LEVEL
    // ВАЖНО: обновляем И CR8 (hardware), И PCR->irql (software cache)
    mov rax, 1              // APC_LEVEL
    mov cr8, rax
    mov byte ptr gs:[PcrIrql], al

    // Загружаем параметры из KSTART_FRAME
    // Windows x64 ABI: RCX = 1-й арг, RDX = 2-й арг
    mov rcx, [rsp + SfP1Home]   // StartRoutine
    mov rdx, [rsp + SfP2Home]   // StartContext
    mov rax, [rsp + SfP4Home]   // SystemRoutine

    // Включаем прерывания перед вызовом пользовательского кода
    sti

    // Если SystemRoutine == NULL, пропускаем вызов
    test rax, rax
    jz .Lthread_startup_exit

    // Выделяем shadow space для Win64 ABI
    // RSP выровнен на 16 (после ret из KSWITCH_FRAME)
    // sub rsp, 0x30 -> RSP % 16 == 0 (48 байт: 32 shadow + 16 padding)
    // call -> RSP % 16 == 8 (правильно для Win64 ABI)
    sub rsp, 0x30
    
    // Вызываем SystemRoutine(StartRoutine, StartContext)
    // Примечание: SystemRoutine (psp_system_thread_startup) никогда не возвращается,
    // но для корректности восстанавливаем стек
    call rax
    
    add rsp, 0x30

.Lthread_startup_exit:
    // Переходим по адресу возврата из KSTART_FRAME
    // RSP указывает на KSTART_FRAME, return address на смещении SfReturn (0x28)
    add rsp, SfReturn
    ret

// =============================================================================
// ki_invalid_system_thread_startup_exit - system thread вернулся (баг!)
// =============================================================================
ki_invalid_system_thread_startup_exit:
    // System thread не должен возвращаться из SystemRoutine
    // Это баг - вызываем assertion failure
    int 0x2C
    nop
    jmp ki_invalid_system_thread_startup_exit

// =============================================================================
// ki_user_thread_startup_exit - переход в user mode
//
// TODO: Реализовать полноценный переход через IRET
// Сейчас это заглушка
// =============================================================================
ki_user_thread_startup_exit:
    // Пока заглушка - ждем прерывание
    sti
.Luser_wait:
    hlt
    jmp .Luser_wait
