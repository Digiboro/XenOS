//! I/O Port Access Functions
//!
//! Функции для чтения и записи портов ввода-вывода x86.
//!
//! Источники:
//! - ReactOS: hal/halx86/generic/portio.c

// =============================================================================
// Port Read Functions
// =============================================================================

/// READ_PORT_UCHAR - читает байт из порта
#[inline]
pub fn read_port_uchar(port: u16) -> u8 {
    let value: u8;
    unsafe {
        core::arch::asm!(
            "in al, dx",
            in("dx") port,
            out("al") value,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}

/// READ_PORT_USHORT - читает слово из порта
#[inline]
pub fn read_port_ushort(port: u16) -> u16 {
    let value: u16;
    unsafe {
        core::arch::asm!(
            "in ax, dx",
            in("dx") port,
            out("ax") value,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}

/// READ_PORT_ULONG - читает двойное слово из порта
#[inline]
pub fn read_port_ulong(port: u16) -> u32 {
    let value: u32;
    unsafe {
        core::arch::asm!(
            "in eax, dx",
            in("dx") port,
            out("eax") value,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}

// =============================================================================
// Port Write Functions
// =============================================================================

/// WRITE_PORT_UCHAR - пишет байт в порт
#[inline]
pub fn write_port_uchar(port: u16, value: u8) {
    unsafe {
        core::arch::asm!(
            "out dx, al",
            in("dx") port,
            in("al") value,
            options(nomem, nostack, preserves_flags)
        );
    }
}

/// WRITE_PORT_USHORT - пишет слово в порт
#[inline]
pub fn write_port_ushort(port: u16, value: u16) {
    unsafe {
        core::arch::asm!(
            "out dx, ax",
            in("dx") port,
            in("ax") value,
            options(nomem, nostack, preserves_flags)
        );
    }
}

/// WRITE_PORT_ULONG - пишет двойное слово в порт
#[inline]
pub fn write_port_ulong(port: u16, value: u32) {
    unsafe {
        core::arch::asm!(
            "out dx, eax",
            in("dx") port,
            in("eax") value,
            options(nomem, nostack, preserves_flags)
        );
    }
}

// =============================================================================
// Port Buffer Functions
// =============================================================================

/// READ_PORT_BUFFER_UCHAR - читает буфер байтов из порта
#[inline]
pub fn read_port_buffer_uchar(port: u16, buffer: &mut [u8]) {
    unsafe {
        core::arch::asm!(
            "rep insb",
            in("dx") port,
            inout("rdi") buffer.as_mut_ptr() => _,
            inout("rcx") buffer.len() => _,
            options(nostack, preserves_flags)
        );
    }
}

/// WRITE_PORT_BUFFER_UCHAR - пишет буфер байтов в порт
#[inline]
pub fn write_port_buffer_uchar(port: u16, buffer: &[u8]) {
    unsafe {
        core::arch::asm!(
            "rep outsb",
            in("dx") port,
            inout("rsi") buffer.as_ptr() => _,
            inout("rcx") buffer.len() => _,
            options(nostack, preserves_flags)
        );
    }
}

// =============================================================================
// I/O Delay
// =============================================================================

/// Короткая задержка для медленных устройств (PIC, PIT, etc.)
#[inline]
pub fn io_delay() {
    // Запись в порт 0x80 - традиционный способ создания задержки
    // Порт 0x80 используется для POST codes и безопасен для записи
    write_port_uchar(0x80, 0);
}

/// Множественная задержка
#[inline]
pub fn io_delay_count(count: u32) {
    for _ in 0..count {
        io_delay();
    }
}

// =============================================================================
// Convenience Macros
// =============================================================================

/// Читает байт из порта с задержкой
#[inline]
pub fn read_port_uchar_slow(port: u16) -> u8 {
    let value = read_port_uchar(port);
    io_delay();
    value
}

/// Пишет байт в порт с задержкой
#[inline]
pub fn write_port_uchar_slow(port: u16, value: u8) {
    write_port_uchar(port, value);
    io_delay();
}
