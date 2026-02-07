// =============================================================================
// KiSwitchToBootStack - переключение на boot stack
//
// Архитектура NT: эта функция переключает стек на InitialStack Idle Thread
// и вызывает ki_system_startup_boot_stack. Это позволяет:
// 1. Иметь guard page внизу boot stack
// 2. Освободить временный стек загрузчика
//
// Источники:
// - NT6.1: ke/amd64/trap.asm (KiSwitchToBootStack)
// - ReactOS: ntoskrnl/ke/amd64/trap.S
// =============================================================================

// Intel syntax (как в ctxswitch.S и ReactOS)
.intel_syntax noprefix

.text

// =============================================================================
// ki_switch_to_boot_stack
//
// Переключает стек на InitialStack и вызывает ki_system_startup_boot_stack.
//
// Параметры (Windows x64 ABI):
//   rcx = InitialStack (верхняя граница стека, выровненная)
//
// Эта функция никогда не возвращает.
// =============================================================================
.global ki_switch_to_boot_stack
ki_switch_to_boot_stack:
    // Переключаем стек на переданный InitialStack
    mov rsp, rcx

    // Выравниваем стек для Windows x64 ABI:
    // После call на входе в функцию должно быть RSP % 16 == 8
    // (return address уже на стеке).
    //
    // Расчёт:
    // 1. and rsp, -16  -> RSP % 16 == 0
    // 2. sub rsp, 0x20 -> RSP -= 32 (shadow space), RSP % 16 == 0
    // 3. call          -> RSP -= 8 (push return addr), RSP % 16 == 8 (правильно!)
    and rsp, -16
    sub rsp, 0x20    // Shadow space (32 bytes)

    // Вызываем ki_system_startup_boot_stack (Rust функция)
    // Эта функция никогда не возвращает
    call ki_system_startup_boot_stack

    // Если каким-то образом вернулись - останавливаем CPU
    cli
.Lhalt_loop:
    hlt
    jmp .Lhalt_loop
