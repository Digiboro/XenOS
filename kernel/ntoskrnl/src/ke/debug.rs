//! Отладочный вывод в debugcon (порт 0xe9)
//!
//! Прямой вывод без блокировок для отладки на любом IRQL.
//! Используется для трассировки критических путей (таймеры, планировщик).

/// Прямой вывод строки в debugcon без блокировок
#[inline]
pub fn debug_raw(s: &str) {
    const DEBUGCON_PORT: u16 = 0xe9;
    for byte in s.as_bytes() {
        crate::hal::portio::write_port_uchar(DEBUGCON_PORT, *byte);
    }
}

/// Вывод числа в hex в debugcon
#[inline]
pub fn debug_hex(val: u64) {
    const HEX_CHARS: &[u8] = b"0123456789abcdef";
    const DEBUGCON_PORT: u16 = 0xe9;
    
    crate::hal::portio::write_port_uchar(DEBUGCON_PORT, b'0');
    crate::hal::portio::write_port_uchar(DEBUGCON_PORT, b'x');
    
    for i in (0..16).rev() {
        let nibble = ((val >> (i * 4)) & 0xf) as usize;
        crate::hal::portio::write_port_uchar(DEBUGCON_PORT, HEX_CHARS[nibble]);
    }
}

/// Вывод десятичного числа в debugcon
#[inline]
pub fn debug_dec(mut val: u64) {
    const DEBUGCON_PORT: u16 = 0xe9;
    
    if val == 0 {
        crate::hal::portio::write_port_uchar(DEBUGCON_PORT, b'0');
        return;
    }
    
    let mut buf = [0u8; 20];
    let mut i = 0;
    
    while val > 0 {
        buf[i] = b'0' + (val % 10) as u8;
        val /= 10;
        i += 1;
    }
    
    while i > 0 {
        i -= 1;
        crate::hal::portio::write_port_uchar(DEBUGCON_PORT, buf[i]);
    }
}

