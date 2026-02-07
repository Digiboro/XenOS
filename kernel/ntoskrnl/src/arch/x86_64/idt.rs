//! IDT (Interrupt Descriptor Table) для x86_64
//!
//! Источники:
//! - NT5: ke/amd64/initkr.c (KiInitializeIdtEntry)
//! - ReactOS: ke/amd64/kiinit.c

#![allow(dead_code)]

use super::gdt::selector;
use super::tss::ist_index;

/// Количество записей в IDT
pub const IDT_ENTRIES: usize = 256;

/// Типы gate в IDT
mod gate_type {
    pub const INTERRUPT_GATE: u8 = 0xE; // Interrupt Gate (clears IF)
    pub const TRAP_GATE: u8 = 0xF; // Trap Gate (doesn't clear IF)
}

/// Gate дескриптор в IDT (16 байт для 64-bit)
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct IdtEntry {
    offset_low: u16,
    selector: u16,
    ist: u8,       // bits 0-2: IST index, bits 3-7: reserved
    type_attr: u8, // bits 0-3: gate type, bit 4: 0, bits 5-6: DPL, bit 7: present
    offset_mid: u16,
    offset_high: u32,
    reserved: u32,
}

impl IdtEntry {
    /// Создает пустой дескриптор
    pub const fn null() -> Self {
        Self {
            offset_low: 0,
            selector: 0,
            ist: 0,
            type_attr: 0,
            offset_mid: 0,
            offset_high: 0,
            reserved: 0,
        }
    }

    /// Создает interrupt gate
    ///
    /// # Arguments
    /// * `handler` - адрес обработчика
    /// * `selector` - селектор кода (обычно KGDT64_R0_CODE)
    /// * `ist` - индекс IST (0 = без IST)
    /// * `dpl` - уровень привилегий (0 = ring 0, 3 = ring 3)
    pub fn new_interrupt(handler: u64, code_selector: u16, ist: u8, dpl: u8) -> Self {
        Self {
            offset_low: handler as u16,
            selector: code_selector,
            ist: ist & 0x7,
            type_attr: (1 << 7) | ((dpl & 0x3) << 5) | gate_type::INTERRUPT_GATE,
            offset_mid: (handler >> 16) as u16,
            offset_high: (handler >> 32) as u32,
            reserved: 0,
        }
    }

    /// Создает trap gate (не очищает IF)
    pub fn new_trap(handler: u64, code_selector: u16, ist: u8, dpl: u8) -> Self {
        Self {
            offset_low: handler as u16,
            selector: code_selector,
            ist: ist & 0x7,
            type_attr: (1 << 7) | ((dpl & 0x3) << 5) | gate_type::TRAP_GATE,
            offset_mid: (handler >> 16) as u16,
            offset_high: (handler >> 32) as u32,
            reserved: 0,
        }
    }

    /// Устанавливает обработчик
    pub fn set_handler(&mut self, handler: u64) {
        self.offset_low = handler as u16;
        self.offset_mid = (handler >> 16) as u16;
        self.offset_high = (handler >> 32) as u32;
    }
}

/// IDT таблица
#[repr(C, align(16))]
pub struct Idt {
    entries: [IdtEntry; IDT_ENTRIES],
}

impl Idt {
    /// Создает пустую IDT
    pub const fn new() -> Self {
        Self {
            entries: [IdtEntry::null(); IDT_ENTRIES],
        }
    }

    /// Устанавливает обработчик исключения
    pub fn set_handler(&mut self, vector: u8, handler: u64, ist: u8, dpl: u8) {
        self.entries[vector as usize] =
            IdtEntry::new_interrupt(handler, selector::KGDT64_R0_CODE, ist, dpl);
    }

    /// Устанавливает trap (не очищает IF)
    pub fn set_trap(&mut self, vector: u8, handler: u64, ist: u8, dpl: u8) {
        self.entries[vector as usize] =
            IdtEntry::new_trap(handler, selector::KGDT64_R0_CODE, ist, dpl);
    }

    /// Получает запись IDT
    pub fn get(&self, vector: u8) -> &IdtEntry {
        &self.entries[vector as usize]
    }

    /// Получает изменяемую запись IDT
    pub fn get_mut(&mut self, vector: u8) -> &mut IdtEntry {
        &mut self.entries[vector as usize]
    }

    /// Возвращает указатель на IDT для IDTR
    pub fn pointer(&self) -> IdtPointer {
        IdtPointer {
            limit: (core::mem::size_of::<Self>() - 1) as u16,
            base: self as *const _ as u64,
        }
    }
}

impl Default for Idt {
    fn default() -> Self {
        Self::new()
    }
}

/// IDTR структура
#[repr(C, packed)]
pub struct IdtPointer {
    pub limit: u16,
    pub base: u64,
}

/// Загружает IDT
///
/// # Safety
/// IDT должна быть корректно инициализирована
pub unsafe fn load_idt(idt_ptr: &IdtPointer) {
    unsafe {
        core::arch::asm!(
            "lidt [{}]",
            in(reg) idt_ptr,
            options(nostack, preserves_flags)
        );
    }
}

/// Векторы исключений процессора
pub mod vector {
    /// #DE - Divide Error
    pub const DIVIDE_ERROR: u8 = 0;
    /// #DB - Debug Exception
    pub const DEBUG: u8 = 1;
    /// NMI - Non-Maskable Interrupt
    pub const NMI: u8 = 2;
    /// #BP - Breakpoint
    pub const BREAKPOINT: u8 = 3;
    /// #OF - Overflow
    pub const OVERFLOW: u8 = 4;
    /// #BR - BOUND Range Exceeded
    pub const BOUND_RANGE: u8 = 5;
    /// #UD - Invalid Opcode
    pub const INVALID_OPCODE: u8 = 6;
    /// #NM - Device Not Available (FPU)
    pub const DEVICE_NOT_AVAILABLE: u8 = 7;
    /// #DF - Double Fault
    pub const DOUBLE_FAULT: u8 = 8;
    /// Coprocessor Segment Overrun (legacy)
    pub const COPROCESSOR_SEGMENT: u8 = 9;
    /// #TS - Invalid TSS
    pub const INVALID_TSS: u8 = 10;
    /// #NP - Segment Not Present
    pub const SEGMENT_NOT_PRESENT: u8 = 11;
    /// #SS - Stack-Segment Fault
    pub const STACK_SEGMENT_FAULT: u8 = 12;
    /// #GP - General Protection Fault
    pub const GENERAL_PROTECTION: u8 = 13;
    /// #PF - Page Fault
    pub const PAGE_FAULT: u8 = 14;
    /// Reserved
    pub const RESERVED_15: u8 = 15;
    /// #MF - x87 FPU Floating-Point Error
    pub const X87_FPU_ERROR: u8 = 16;
    /// #AC - Alignment Check
    pub const ALIGNMENT_CHECK: u8 = 17;
    /// #MC - Machine Check
    pub const MACHINE_CHECK: u8 = 18;
    /// #XM/#XF - SIMD Floating-Point Exception
    pub const SIMD_FP: u8 = 19;
    /// #VE - Virtualization Exception
    pub const VIRTUALIZATION: u8 = 20;
    /// #CP - Control Protection Exception
    pub const CONTROL_PROTECTION: u8 = 21;

    // Зарезервированы: 22-31

    /// Первый вектор для внешних прерываний
    pub const IRQ_BASE: u8 = 32;

    /// Вектор APIC Timer
    pub const APIC_TIMER: u8 = 0xEF;
    /// Вектор APIC Error
    pub const APIC_ERROR: u8 = 0xFE;
    /// Вектор APIC Spurious
    pub const APIC_SPURIOUS: u8 = 0xFF;
}

/// Interrupt frame, сохраняемый процессором при прерывании
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct InterruptFrame {
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

/// Расширенный interrupt frame с error code
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct InterruptFrameWithError {
    pub error_code: u64,
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

/// Инициализирует базовые обработчики исключений
///
/// # Safety
/// Должен вызываться только при инициализации ядра
pub unsafe fn init_exception_handlers(idt: &mut Idt, handlers: &ExceptionHandlers) {
    use vector::*;

    // Исключения без error code
    idt.set_handler(DIVIDE_ERROR, handlers.divide_error, ist_index::NONE, 0);
    idt.set_trap(DEBUG, handlers.debug, ist_index::DEBUG, 0);
    idt.set_handler(NMI, handlers.nmi, ist_index::PANIC, 0);
    idt.set_trap(BREAKPOINT, handlers.breakpoint, ist_index::NONE, 3); // DPL=3 для int3
    idt.set_trap(OVERFLOW, handlers.overflow, ist_index::NONE, 3);
    idt.set_handler(BOUND_RANGE, handlers.bound_range, ist_index::NONE, 0);
    idt.set_handler(INVALID_OPCODE, handlers.invalid_opcode, ist_index::NONE, 0);
    idt.set_handler(
        DEVICE_NOT_AVAILABLE,
        handlers.device_not_available,
        ist_index::NONE,
        0,
    );

    // Double Fault использует IST1
    idt.set_handler(DOUBLE_FAULT, handlers.double_fault, ist_index::PANIC, 0);

    // Исключения с error code
    idt.set_handler(INVALID_TSS, handlers.invalid_tss, ist_index::NONE, 0);
    idt.set_handler(
        SEGMENT_NOT_PRESENT,
        handlers.segment_not_present,
        ist_index::NONE,
        0,
    );
    idt.set_handler(
        STACK_SEGMENT_FAULT,
        handlers.stack_segment_fault,
        ist_index::NONE,
        0,
    );
    idt.set_handler(
        GENERAL_PROTECTION,
        handlers.general_protection,
        ist_index::NONE,
        0,
    );
    idt.set_handler(PAGE_FAULT, handlers.page_fault, ist_index::NONE, 0);

    // FPU/SIMD исключения
    idt.set_handler(X87_FPU_ERROR, handlers.x87_fpu_error, ist_index::NONE, 0);
    idt.set_handler(
        ALIGNMENT_CHECK,
        handlers.alignment_check,
        ist_index::NONE,
        0,
    );
    idt.set_handler(MACHINE_CHECK, handlers.machine_check, ist_index::MCA, 0);
    idt.set_handler(SIMD_FP, handlers.simd_fp, ist_index::NONE, 0);
}

/// Структура с адресами обработчиков исключений
pub struct ExceptionHandlers {
    pub divide_error: u64,
    pub debug: u64,
    pub nmi: u64,
    pub breakpoint: u64,
    pub overflow: u64,
    pub bound_range: u64,
    pub invalid_opcode: u64,
    pub device_not_available: u64,
    pub double_fault: u64,
    pub invalid_tss: u64,
    pub segment_not_present: u64,
    pub stack_segment_fault: u64,
    pub general_protection: u64,
    pub page_fault: u64,
    pub x87_fpu_error: u64,
    pub alignment_check: u64,
    pub machine_check: u64,
    pub simd_fp: u64,
}

// =============================================================================
// APIC Interrupt Handlers Registration
// =============================================================================

/// Векторы APIC прерываний (из ReactOS apicp.h для AMD64)
pub mod apic_vector {
    /// APIC Timer vector (CLOCK_LEVEL)
    pub const APIC_TIMER: u8 = 0xD1;
    /// Clock IPI vector (для SMP)
    pub const CLOCK_IPI: u8 = 0xD2;
    /// Spurious interrupt vector
    pub const APIC_SPURIOUS: u8 = 0xDF;
    /// IPI vector
    pub const APIC_IPI: u8 = 0xE1;
    /// Error vector
    pub const APIC_ERROR: u8 = 0xE2;
}

/// Векторы software interrupts (APC/DPC)
pub mod swint_vector {
    /// APC software interrupt vector (APC_LEVEL)
    pub const APC_VECTOR: u8 = 0x1F;
    /// DPC/Dispatch software interrupt vector (DISPATCH_LEVEL)
    pub const DISPATCH_VECTOR: u8 = 0x2F;
}

/// Структура с адресами обработчиков APIC прерываний
pub struct ApicHandlers {
    /// APIC Timer handler (vector 0xD1)
    pub timer: u64,
    /// APIC Spurious handler (vector 0xDF)
    pub spurious: u64,
    /// APIC Error handler (vector 0xE2)
    pub error: u64,
}

/// Инициализирует обработчики APIC прерываний в IDT
///
/// # Safety
/// Должен вызываться при инициализации после init_exception_handlers
pub unsafe fn init_apic_handlers(idt: &mut Idt, handlers: &ApicHandlers) {
    use apic_vector::*;

    // APIC Timer - vector 0xD1, IST=0, DPL=0
    idt.set_handler(APIC_TIMER, handlers.timer, ist_index::NONE, 0);

    // APIC Spurious - vector 0xDF, IST=0, DPL=0
    idt.set_handler(APIC_SPURIOUS, handlers.spurious, ist_index::NONE, 0);

    // APIC Error - vector 0xE2, IST=0, DPL=0
    idt.set_handler(APIC_ERROR, handlers.error, ist_index::NONE, 0);
}

/// Структура с адресами обработчиков software interrupts
pub struct SwintHandlers {
    /// APC interrupt handler (vector 0x1F)
    pub apc: u64,
    /// DPC/Dispatch interrupt handler (vector 0x2F)
    pub dispatch: u64,
}

/// Инициализирует обработчики software interrupts (APC/DPC) в IDT
///
/// NT использует эти векторы для доставки APC и DPC через self-IPI.
///
/// # Safety
/// Должен вызываться при инициализации после init_exception_handlers
pub unsafe fn init_software_interrupt_handlers(idt: &mut Idt, handlers: &SwintHandlers) {
    use swint_vector::*;

    // APC - vector 0x1F, IST=0, DPL=0 (kernel only)
    idt.set_handler(APC_VECTOR, handlers.apc, ist_index::NONE, 0);

    // DPC/Dispatch - vector 0x2F, IST=0, DPL=0 (kernel only)
    idt.set_handler(DISPATCH_VECTOR, handlers.dispatch, ist_index::NONE, 0);
}

// =============================================================================
// Device Interrupt Handlers
// =============================================================================

/// Инициализирует обработчики device interrupts (IRQ 0-23) в IDT
///
/// Векторы 0x30-0x47 используются для device IRQs (ISA и PCI).
///
/// # Safety
/// Должен вызываться при инициализации после init_exception_handlers
pub unsafe fn init_device_interrupt_handlers(idt: &mut Idt, handlers: &crate::ke::trap::DeviceInterruptHandlers) {
    // Device IRQ vectors start at 0x30
    const DEVICE_VECTOR_BASE: u8 = 0x30;
    
    for (i, &handler) in handlers.handlers.iter().enumerate() {
        let vector = DEVICE_VECTOR_BASE + i as u8;
        idt.set_handler(vector, handler, ist_index::NONE, 0);
    }
}
