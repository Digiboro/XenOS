//! Debug output для PCI driver через DbgPrint

use crate::types::*;

/// Выводит строку через DbgPrint
pub unsafe fn debug_print(s: &str) {
    // Создаём C string
    let mut buf = [0u8; 256];
    let len = s.len().min(255);
    for (i, &b) in s.as_bytes().iter().take(len).enumerate() {
        buf[i] = b;
    }
    buf[len] = 0; // null terminator
    
    DbgPrint(buf.as_ptr());
}

/// Выводит число в hex
pub unsafe fn debug_print_hex(value: u64) {
    const HEX_CHARS: &[u8] = b"0123456789ABCDEF";
    let mut buf = [0u8; 18]; // "0x" + 16 hex digits + null
    
    buf[0] = b'0';
    buf[1] = b'x';
    
    for i in 0..16 {
        let nibble = ((value >> (60 - i * 4)) & 0xF) as usize;
        buf[i + 2] = HEX_CHARS[nibble];
    }
    buf[17] = 0;
    
    DbgPrint(buf.as_ptr());
}

