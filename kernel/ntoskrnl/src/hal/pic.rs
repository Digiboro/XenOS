//! 8259 PIC - Programmable Interrupt Controller
//!
//! Управление Legacy PIC для x86.
//! В современных системах заменен на APIC, но нужен для совместимости.
//!
//! Источники:
//! - ReactOS: hal/halx86/generic/pic.c

use super::portio::io_delay;
use super::portio::read_port_uchar;
use super::portio::write_port_uchar;

// =============================================================================
// PIC Ports
// =============================================================================

/// Master PIC (PIC1) - управляет IRQ 0-7
pub const PIC1_COMMAND: u16 = 0x20;
pub const PIC1_DATA: u16 = 0x21;

/// Slave PIC (PIC2) - управляет IRQ 8-15
pub const PIC2_COMMAND: u16 = 0xA0;
pub const PIC2_DATA: u16 = 0xA1;

// =============================================================================
// PIC Commands
// =============================================================================

/// ICW1 flags
pub const ICW1_INIT: u8 = 0x10; // Initialization
pub const ICW1_ICW4: u8 = 0x01; // ICW4 needed
pub const ICW1_SINGLE: u8 = 0x02; // Single mode (vs cascade)
pub const ICW1_INTERVAL4: u8 = 0x04; // Call address interval 4 (vs 8)
pub const ICW1_LEVEL: u8 = 0x08; // Level triggered (vs edge)

/// ICW4 flags
pub const ICW4_8086: u8 = 0x01; // 8086/88 mode (vs MCS-80/85)
pub const ICW4_AUTO: u8 = 0x02; // Auto EOI
pub const ICW4_BUF_SLAVE: u8 = 0x08; // Buffered mode (slave)
pub const ICW4_BUF_MASTER: u8 = 0x0C; // Buffered mode (master)
pub const ICW4_SFNM: u8 = 0x10; // Special fully nested mode

/// OCW2 commands
pub const OCW2_EOI: u8 = 0x20; // Non-specific EOI
pub const OCW2_SPECIFIC_EOI: u8 = 0x60; // Specific EOI (+ IRQ number)

/// OCW3 commands
pub const OCW3_READ_IRR: u8 = 0x0A; // Read IRR (Interrupt Request Register)
pub const OCW3_READ_ISR: u8 = 0x0B; // Read ISR (In-Service Register)

// =============================================================================
// Interrupt Vector Mapping
// =============================================================================

/// Base vector for Master PIC (IRQ 0-7 -> vectors 0x20-0x27)
pub const PIC1_VECTOR_BASE: u8 = 0x20;

/// Base vector for Slave PIC (IRQ 8-15 -> vectors 0x28-0x2F)
pub const PIC2_VECTOR_BASE: u8 = 0x28;

/// IRQ для cascade (slave подключен к IRQ2 master)
pub const PIC_CASCADE_IRQ: u8 = 2;

// =============================================================================
// PIC State
// =============================================================================

use core::sync::atomic::AtomicU8;
use core::sync::atomic::Ordering;

/// Текущая маска прерываний Master PIC
static PIC1_MASK: AtomicU8 = AtomicU8::new(0xFF);

/// Текущая маска прерываний Slave PIC
static PIC2_MASK: AtomicU8 = AtomicU8::new(0xFF);

/// PIC инициализирован
static PIC_INITIALIZED: AtomicU8 = AtomicU8::new(0);

// =============================================================================
// PIC Initialization
// =============================================================================

/// Инициализирует 8259 PIC
///
/// Настраивает оба PIC в cascade режиме с ремаппингом векторов.
/// После инициализации все IRQ замаскированы (отключены).
pub fn halp_initialize_legacy_pics() {
    // Сохраняем текущие маски
    let _mask1 = read_port_uchar(PIC1_DATA);
    let _mask2 = read_port_uchar(PIC2_DATA);

    // ICW1: начало инициализации в cascade режиме
    write_port_uchar(PIC1_COMMAND, ICW1_INIT | ICW1_ICW4);
    io_delay();
    write_port_uchar(PIC2_COMMAND, ICW1_INIT | ICW1_ICW4);
    io_delay();

    // ICW2: ремаппинг векторов
    // Master PIC: IRQ 0-7 -> 0x20-0x27
    write_port_uchar(PIC1_DATA, PIC1_VECTOR_BASE);
    io_delay();
    // Slave PIC: IRQ 8-15 -> 0x28-0x2F
    write_port_uchar(PIC2_DATA, PIC2_VECTOR_BASE);
    io_delay();

    // ICW3: настройка cascade
    // Master: slave подключен к IRQ2 (bit 2 = 1)
    write_port_uchar(PIC1_DATA, 1 << PIC_CASCADE_IRQ);
    io_delay();
    // Slave: cascade identity = 2
    write_port_uchar(PIC2_DATA, PIC_CASCADE_IRQ);
    io_delay();

    // ICW4: режим 8086
    write_port_uchar(PIC1_DATA, ICW4_8086);
    io_delay();
    write_port_uchar(PIC2_DATA, ICW4_8086);
    io_delay();

    // Маскируем все прерывания
    write_port_uchar(PIC1_DATA, 0xFF);
    write_port_uchar(PIC2_DATA, 0xFF);

    // Сохраняем маски
    PIC1_MASK.store(0xFF, Ordering::Release);
    PIC2_MASK.store(0xFF, Ordering::Release);

    PIC_INITIALIZED.store(1, Ordering::Release);
}

/// Отключает Legacy PIC (при использовании APIC)
pub fn halp_disable_legacy_pics() {
    // Маскируем все прерывания
    write_port_uchar(PIC1_DATA, 0xFF);
    write_port_uchar(PIC2_DATA, 0xFF);

    PIC1_MASK.store(0xFF, Ordering::Release);
    PIC2_MASK.store(0xFF, Ordering::Release);
}

// =============================================================================
// IRQ Management
// =============================================================================

/// Разрешает указанный IRQ
pub fn hal_enable_irq(irq: u8) {
    if irq < 8 {
        // Master PIC
        let mask = PIC1_MASK.load(Ordering::Acquire) & !(1 << irq);
        PIC1_MASK.store(mask, Ordering::Release);
        write_port_uchar(PIC1_DATA, mask);
    } else if irq < 16 {
        // Slave PIC
        let slave_irq = irq - 8;
        let mask = PIC2_MASK.load(Ordering::Acquire) & !(1 << slave_irq);
        PIC2_MASK.store(mask, Ordering::Release);
        write_port_uchar(PIC2_DATA, mask);

        // Разрешаем cascade IRQ на master
        let mask1 = PIC1_MASK.load(Ordering::Acquire) & !(1 << PIC_CASCADE_IRQ);
        PIC1_MASK.store(mask1, Ordering::Release);
        write_port_uchar(PIC1_DATA, mask1);
    }
}

/// Запрещает указанный IRQ
pub fn hal_disable_irq(irq: u8) {
    if irq < 8 {
        // Master PIC
        let mask = PIC1_MASK.load(Ordering::Acquire) | (1 << irq);
        PIC1_MASK.store(mask, Ordering::Release);
        write_port_uchar(PIC1_DATA, mask);
    } else if irq < 16 {
        // Slave PIC
        let slave_irq = irq - 8;
        let mask = PIC2_MASK.load(Ordering::Acquire) | (1 << slave_irq);
        PIC2_MASK.store(mask, Ordering::Release);
        write_port_uchar(PIC2_DATA, mask);
    }
}

/// Отправляет EOI (End of Interrupt) для указанного IRQ
pub fn hal_end_of_interrupt(irq: u8) {
    if irq >= 8 {
        // Для slave PIC - сначала EOI на slave
        write_port_uchar(PIC2_COMMAND, OCW2_EOI);
    }
    // EOI на master (всегда)
    write_port_uchar(PIC1_COMMAND, OCW2_EOI);
}

/// Отправляет specific EOI для указанного IRQ
pub fn hal_specific_eoi(irq: u8) {
    if irq >= 8 {
        // Для slave PIC
        write_port_uchar(PIC2_COMMAND, OCW2_SPECIFIC_EOI | (irq - 8));
        write_port_uchar(PIC1_COMMAND, OCW2_SPECIFIC_EOI | PIC_CASCADE_IRQ);
    } else {
        write_port_uchar(PIC1_COMMAND, OCW2_SPECIFIC_EOI | irq);
    }
}

// =============================================================================
// PIC Status
// =============================================================================

/// Читает IRR (Interrupt Request Register) - ожидающие прерывания
pub fn hal_read_irr() -> u16 {
    write_port_uchar(PIC1_COMMAND, OCW3_READ_IRR);
    write_port_uchar(PIC2_COMMAND, OCW3_READ_IRR);

    let irr1 = read_port_uchar(PIC1_COMMAND) as u16;
    let irr2 = read_port_uchar(PIC2_COMMAND) as u16;

    irr1 | (irr2 << 8)
}

/// Читает ISR (In-Service Register) - обрабатываемые прерывания
pub fn hal_read_isr() -> u16 {
    write_port_uchar(PIC1_COMMAND, OCW3_READ_ISR);
    write_port_uchar(PIC2_COMMAND, OCW3_READ_ISR);

    let isr1 = read_port_uchar(PIC1_COMMAND) as u16;
    let isr2 = read_port_uchar(PIC2_COMMAND) as u16;

    isr1 | (isr2 << 8)
}

/// Проверяет, является ли прерывание spurious (ложным)
///
/// IRQ7 и IRQ15 могут генерироваться ложно из-за особенностей PIC.
pub fn hal_is_spurious_irq(irq: u8) -> bool {
    if irq == 7 {
        // Проверяем ISR master PIC
        write_port_uchar(PIC1_COMMAND, OCW3_READ_ISR);
        let isr = read_port_uchar(PIC1_COMMAND);
        return (isr & 0x80) == 0;
    } else if irq == 15 {
        // Проверяем ISR slave PIC
        write_port_uchar(PIC2_COMMAND, OCW3_READ_ISR);
        let isr = read_port_uchar(PIC2_COMMAND);
        if (isr & 0x80) == 0 {
            // Spurious от slave - нужен EOI на master
            write_port_uchar(PIC1_COMMAND, OCW2_EOI);
            return true;
        }
    }
    false
}

// =============================================================================
// Vector Conversion
// =============================================================================

/// Конвертирует IRQ в interrupt vector
#[inline]
pub fn irq_to_vector(irq: u8) -> u8 {
    if irq < 8 {
        PIC1_VECTOR_BASE + irq
    } else {
        PIC2_VECTOR_BASE + (irq - 8)
    }
}

/// Конвертирует interrupt vector в IRQ
#[inline]
pub fn vector_to_irq(vector: u8) -> Option<u8> {
    if vector >= PIC1_VECTOR_BASE && vector < PIC1_VECTOR_BASE + 8 {
        Some(vector - PIC1_VECTOR_BASE)
    } else if vector >= PIC2_VECTOR_BASE && vector < PIC2_VECTOR_BASE + 8 {
        Some(vector - PIC2_VECTOR_BASE + 8)
    } else {
        None
    }
}

/// Проверяет инициализирован ли PIC
#[inline]
pub fn is_pic_initialized() -> bool {
    PIC_INITIALIZED.load(Ordering::Acquire) != 0
}
