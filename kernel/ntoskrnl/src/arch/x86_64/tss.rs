//! TSS (Task State Segment) для x86_64
//!
//! Источники:
//! - NT5: ke/amd64/initkr.c (KiInitializeTss)
//! - ReactOS: ke/amd64/kiinit.c

#![allow(dead_code)]

/// TSS64 - Task State Segment для 64-bit mode
///
/// В 64-bit режиме TSS используется только для:
/// - RSP0-RSP2: стеки для переключения привилегий
/// - IST1-IST7: стеки для прерываний (Interrupt Stack Table)
/// - I/O permission bitmap
#[repr(C, packed)]
pub struct Tss64 {
    reserved0: u32,

    /// RSP0 - стек для ring 0 (при переходе из ring 3)
    pub rsp0: u64,
    /// RSP1 - стек для ring 1 (не используется в NT)
    pub rsp1: u64,
    /// RSP2 - стек для ring 2 (не используется в NT)
    pub rsp2: u64,

    reserved1: u64,

    /// IST1 - стек для NMI и Double Fault
    pub ist1: u64,
    /// IST2 - стек для Machine Check
    pub ist2: u64,
    /// IST3 - стек для Debug exceptions
    pub ist3: u64,
    /// IST4 - зарезервирован
    pub ist4: u64,
    /// IST5 - зарезервирован
    pub ist5: u64,
    /// IST6 - зарезервирован
    pub ist6: u64,
    /// IST7 - зарезервирован
    pub ist7: u64,

    reserved2: u64,
    reserved3: u16,

    /// Смещение до I/O permission bitmap
    pub iomap_base: u16,
}

impl Tss64 {
    /// Создает новый пустой TSS
    pub const fn new() -> Self {
        Self {
            reserved0: 0,
            rsp0: 0,
            rsp1: 0,
            rsp2: 0,
            reserved1: 0,
            ist1: 0,
            ist2: 0,
            ist3: 0,
            ist4: 0,
            ist5: 0,
            ist6: 0,
            ist7: 0,
            reserved2: 0,
            reserved3: 0,
            iomap_base: core::mem::size_of::<Self>() as u16,
        }
    }

    /// Устанавливает RSP0 (стек ядра для переключения из ring 3)
    pub fn set_rsp0(&mut self, stack_top: u64) {
        self.rsp0 = stack_top;
    }

    /// Устанавливает IST1 (для NMI и Double Fault)
    pub fn set_ist1(&mut self, stack_top: u64) {
        self.ist1 = stack_top;
    }

    /// Устанавливает IST2 (для Machine Check)
    pub fn set_ist2(&mut self, stack_top: u64) {
        self.ist2 = stack_top;
    }

    /// Устанавливает IST3 (для Debug)
    pub fn set_ist3(&mut self, stack_top: u64) {
        self.ist3 = stack_top;
    }
}

impl Default for Tss64 {
    fn default() -> Self {
        Self::new()
    }
}

/// Индексы IST в IDT
pub mod ist_index {
    /// Без IST (использует RSP0)
    pub const NONE: u8 = 0;
    /// IST1 для NMI и Double Fault
    pub const PANIC: u8 = 1;
    /// IST2 для Machine Check
    pub const MCA: u8 = 2;
    /// IST3 для Debug
    pub const DEBUG: u8 = 3;
}

/// Размер стека для IST (16KB)
pub const IST_STACK_SIZE: usize = 16 * 1024;

/// Размер стека ядра (32KB)
pub const KERNEL_STACK_SIZE: usize = 32 * 1024;

/// KTSS64 - расширенная структура TSS в NT (включает I/O bitmap)
#[repr(C)]
pub struct Ktss64 {
    pub tss: Tss64,
    /// I/O permission bitmap (8KB + 1 byte terminator)
    pub io_map: [u8; 8193],
}

impl Ktss64 {
    pub const fn new() -> Self {
        Self {
            tss: Tss64::new(),
            io_map: [0xFF; 8193], // Все порты запрещены по умолчанию
        }
    }

    /// Разрешает доступ к порту
    pub fn allow_port(&mut self, port: u16) {
        let byte_index = (port / 8) as usize;
        let bit_index = port % 8;
        if byte_index < self.io_map.len() - 1 {
            self.io_map[byte_index] &= !(1 << bit_index);
        }
    }

    /// Запрещает доступ к порту
    pub fn deny_port(&mut self, port: u16) {
        let byte_index = (port / 8) as usize;
        let bit_index = port % 8;
        if byte_index < self.io_map.len() - 1 {
            self.io_map[byte_index] |= 1 << bit_index;
        }
    }
}

impl Default for Ktss64 {
    fn default() -> Self {
        Self::new()
    }
}
