//! Software/Hardware Interrupt ASM Stubs - Rust declarations
//!
//! Фактический ASM код находится в arch/x86_64/asm/interrupt.S
//!
//! Этот модуль предоставляет extern declarations для ASM функций,
//! которые используются как обработчики прерываний в IDT.
//!
//! # Stubs
//!
//! - `KiApcInterrupt` - APC software interrupt (vector 0x1F)
//! - `KiDispatchInterrupt` - DPC/Dispatch software interrupt (vector 0x2F)
//! - `KiClockInterrupt` - APIC Timer hardware interrupt (vector 0xD1)
//! - `KiErrorInterrupt` - APIC Error interrupt (vector 0xE2)
//! - `KiSpuriousInterrupt` - APIC Spurious interrupt (vector 0xDF)
//!
//! # Архитектура
//!
//! ```text
//! Interrupt Entry (CPU)
//!        │
//!        ▼
//! ┌─────────────────────────┐
//! │  ASM Stub (interrupt.S) │
//! │  - Save volatile regs   │
//! │  - Align stack (16)     │
//! │  - Send EOI (if needed) │
//! │  - Raise IRQL           │
//! └────────────┬────────────┘
//!              │
//!              ▼
//! ┌─────────────────────────┐
//! │  Rust Handler           │
//! │  (extern "win64")       │
//! │  - Business logic       │
//! └────────────┬────────────┘
//!              │
//!              ▼
//! ┌─────────────────────────┐
//! │  ASM Stub (interrupt.S) │
//! │  - Lower IRQL           │
//! │  - Restore regs         │
//! │  - IRETQ                │
//! └─────────────────────────┘
//! ```
//!
//! Источники:
//! - ReactOS: hal/halx86/apic/apictimer.c
//! - NT5: ke/amd64/trap.asm

// Импорт ASM функций из interrupt.S (собирается через build.rs)
unsafe extern "C" {
    /// KiApcInterrupt - APC software interrupt stub
    ///
    /// Вызывается на векторе 0x1F через self-IPI.
    /// Обрабатывает очередь APC текущего потока.
    pub fn KiApcInterrupt();

    /// KiDispatchInterrupt - DPC/Dispatch software interrupt stub
    ///
    /// Вызывается на векторе 0x2F через self-IPI.
    /// Обрабатывает DPC очередь и выполняет переключение потоков.
    pub fn KiDispatchInterrupt();

    /// KiClockInterrupt - APIC Timer hardware interrupt stub
    ///
    /// Вызывается на векторе 0xD1 от Local APIC Timer.
    /// Обновляет системное время и выполняет quantum tick.
    pub fn KiClockInterrupt();

    /// KiErrorInterrupt - APIC Error interrupt stub
    ///
    /// Вызывается на векторе 0xE2 при ошибках APIC.
    pub fn KiErrorInterrupt();

    /// KiSpuriousInterrupt - APIC Spurious interrupt stub
    ///
    /// Вызывается на векторе 0xDF. Игнорируется без EOI.
    pub fn KiSpuriousInterrupt();
}
