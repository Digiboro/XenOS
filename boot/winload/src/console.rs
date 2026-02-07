//! Консольный вывод через UEFI stdout
//!
//! Функции для печати текста в UEFI console.

/// Печать строки в UEFI console (stdout).
pub fn print(s: &str) {
    uefi::system::with_stdout(|stdout| {
        // UEFI требует UTF-16, конвертируем посимвольно
        for c in s.chars() {
            let mut buf = [0u16; 2];
            let encoded = c.encode_utf16(&mut buf);
            // Null-terminate для UEFI
            let mut out = [0u16; 3];
            out[0] = encoded[0];
            if encoded.len() > 1 {
                out[1] = encoded[1];
            }
            // Используем низкоуровневый вывод
            let _ =
                stdout.output_string(unsafe { uefi::CStr16::from_u16_with_nul_unchecked(&out) });
        }
    });
}

/// Печать строки с переводом строки.
pub fn println(s: &str) {
    print(s);
    print("\r\n");
}

/// Печать hex числа (16 цифр).
pub fn print_hex(val: u64) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    print("0x");
    for i in (0..16).rev() {
        let digit = ((val >> (i * 4)) & 0xF) as usize;
        let c = HEX[digit] as char;
        let mut buf = [0u16; 2];
        buf[0] = c as u16;
        uefi::system::with_stdout(|stdout| {
            let _ =
                stdout.output_string(unsafe { uefi::CStr16::from_u16_with_nul_unchecked(&buf) });
        });
    }
}

/// Печать десятичного числа.
pub fn print_num(val: u64) {
    if val == 0 {
        print("0");
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = 0;
    let mut n = val;
    while n > 0 {
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        i += 1;
    }
    // Выводим в обратном порядке
    for j in (0..i).rev() {
        let c = buf[j] as char;
        let mut out = [0u16; 2];
        out[0] = c as u16;
        uefi::system::with_stdout(|stdout| {
            let _ =
                stdout.output_string(unsafe { uefi::CStr16::from_u16_with_nul_unchecked(&out) });
        });
    }
}
