//! RTL Unicode helpers
//!
//! Функции для работы с Unicode (case mapping, сравнение строк, свойства символов).
//! Используют NLS таблицы, загруженные из unicode.nls через LoaderBlock.
//!
//! ВАЖНО: Полноценные функции требуют инициализации NLS подсистемы.
//! До вызова `rtl_initialize_nls_from_loader_block()` доступен только ASCII fallback.

use crate::nt::UNICODE_STRING;

// =============================================================================
// NLS Runtime State
// =============================================================================

/// Флаг инициализации NLS подсистемы.
/// После успешной инициализации устанавливается в true.
static mut NLS_INITIALIZED: bool = false;

/// Указатель на таблицу UUP1 (simple upper mapping, BMP).
/// Таблица u16[65536], где table[ch] = toUpper(ch).
static mut NLS_UPCASE_TABLE: *const u16 = core::ptr::null();

/// Указатель на таблицу ULO1 (simple lower mapping, BMP).
/// Таблица u16[65536], где table[ch] = toLower(ch).
static mut NLS_LOWCASE_TABLE: *const u16 = core::ptr::null();

/// Указатель на таблицу PRP1 (свойства символов, BMP).
/// Таблица u16[65536] с битовыми масками свойств.
static mut NLS_PROPS_TABLE: *const u16 = core::ptr::null();

// =============================================================================
// Константы свойств символов (биты PRP1)
// =============================================================================

/// Символ является алфавитным (Alphabetic)
pub const CHAR_PROP_ALPHABETIC: u16 = 0x0001;

/// Символ является прописной буквой (Uppercase)
pub const CHAR_PROP_UPPERCASE: u16 = 0x0002;

/// Символ является строчной буквой (Lowercase)
pub const CHAR_PROP_LOWERCASE: u16 = 0x0004;

/// Символ является десятичной цифрой (Decimal Digit, Nd)
pub const CHAR_PROP_DECIMAL_DIGIT: u16 = 0x0008;

/// Символ является пробельным (White_Space)
pub const CHAR_PROP_WHITE_SPACE: u16 = 0x0010;

// =============================================================================
// NLS Initialization (вызывается из rtl/nls.rs)
// =============================================================================

/// Устанавливает указатели на NLS таблицы.
/// Вызывается из `rtl_initialize_nls_from_loader_block()` после парсинга XNLS.
///
/// # Safety
/// Указатели должны указывать на валидные таблицы размером 65536 элементов u16.
/// Вызывается только один раз при старте ядра до запуска планировщика.
pub unsafe fn nls_set_tables(upcase: *const u16, lowcase: *const u16, props: *const u16) {
    unsafe {
        NLS_UPCASE_TABLE = upcase;
        NLS_LOWCASE_TABLE = lowcase;
        NLS_PROPS_TABLE = props;
        NLS_INITIALIZED = true;
    }
}

/// Проверяет, инициализирована ли NLS подсистема.
#[inline]
pub fn nls_is_initialized() -> bool {
    unsafe { NLS_INITIALIZED }
}

// =============================================================================
// ASCII Fallback (используется до инициализации NLS)
// =============================================================================

/// Upcase одного UTF-16 code unit (ASCII fallback).
/// Работает только для ASCII диапазона 'a'..'z'.
#[inline]
pub const fn rtl_upcase_char_ascii(ch: u16) -> u16 {
    if ch >= b'a' as u16 && ch <= b'z' as u16 {
        ch - 32
    } else {
        ch
    }
}

/// Downcase одного UTF-16 code unit (ASCII fallback).
/// Работает только для ASCII диапазона 'A'..'Z'.
#[inline]
pub const fn rtl_downcase_char_ascii(ch: u16) -> u16 {
    if ch >= b'A' as u16 && ch <= b'Z' as u16 {
        ch + 32
    } else {
        ch
    }
}

// =============================================================================
// Unicode Case Mapping
// =============================================================================

/// RtlUpcaseUnicodeChar
///
/// Upcase одного UTF-16 code unit через NLS таблицу.
///
/// Если NLS не инициализирована - использует ASCII fallback.
/// После инициализации NLS - полная поддержка BMP (U+0000..U+FFFF).
#[inline]
pub fn rtl_upcase_unicode_char(ch: u16) -> u16 {
    unsafe {
        if NLS_INITIALIZED && !NLS_UPCASE_TABLE.is_null() {
            // Полный upcase через NLS таблицу
            *NLS_UPCASE_TABLE.add(ch as usize)
        } else {
            // ASCII fallback до инициализации NLS
            rtl_upcase_char_ascii(ch)
        }
    }
}

/// RtlDowncaseUnicodeChar
///
/// Downcase одного UTF-16 code unit через NLS таблицу.
///
/// Если NLS не инициализирована - использует ASCII fallback.
/// После инициализации NLS - полная поддержка BMP (U+0000..U+FFFF).
#[inline]
pub fn rtl_downcase_unicode_char(ch: u16) -> u16 {
    unsafe {
        if NLS_INITIALIZED && !NLS_LOWCASE_TABLE.is_null() {
            // Полный downcase через NLS таблицу
            *NLS_LOWCASE_TABLE.add(ch as usize)
        } else {
            // ASCII fallback до инициализации NLS
            rtl_downcase_char_ascii(ch)
        }
    }
}

// =============================================================================
// Unicode String Comparison
// =============================================================================

/// RtlCompareUnicodeString
///
/// Сравнивает две `UNICODE_STRING`.
///
/// Возвращает:
/// - < 0 если s1 < s2
/// - 0 если равны
/// - > 0 если s1 > s2
pub fn rtl_compare_unicode_string(
    s1: &UNICODE_STRING,
    s2: &UNICODE_STRING,
    case_insensitive: bool,
) -> i32 {
    let len1 = (s1.length / 2) as usize;
    let len2 = (s2.length / 2) as usize;
    let min_len = core::cmp::min(len1, len2);

    if s1.buffer.is_null() || s2.buffer.is_null() {
        // Сравнение по указателям как крайний случай (неаутентично, но безопасно).
        return (s1.buffer as usize).cmp(&(s2.buffer as usize)) as i32;
    }

    let a = unsafe { core::slice::from_raw_parts(s1.buffer, len1) };
    let b = unsafe { core::slice::from_raw_parts(s2.buffer, len2) };

    for i in 0..min_len {
        let mut c1 = a[i];
        let mut c2 = b[i];
        if case_insensitive {
            c1 = rtl_upcase_unicode_char(c1);
            c2 = rtl_upcase_unicode_char(c2);
        }
        if c1 != c2 {
            return if c1 < c2 { -1 } else { 1 };
        }
    }

    // Если общий префикс равен — сравниваем длины.
    if len1 == len2 {
        0
    } else if len1 < len2 {
        -1
    } else {
        1
    }
}

/// RtlEqualUnicodeString
///
/// Проверяет равенство двух `UNICODE_STRING`.
#[inline]
pub fn rtl_equal_unicode_string(
    s1: &UNICODE_STRING,
    s2: &UNICODE_STRING,
    case_insensitive: bool,
) -> bool {
    rtl_compare_unicode_string(s1, s2, case_insensitive) == 0
}

// =============================================================================
// Character Properties (через PRP1 таблицу)
// =============================================================================

/// Получает свойства символа из NLS таблицы PRP1.
/// Возвращает 0 если NLS не инициализирована.
#[inline]
fn get_char_props(ch: u16) -> u16 {
    unsafe {
        if NLS_INITIALIZED && !NLS_PROPS_TABLE.is_null() {
            *NLS_PROPS_TABLE.add(ch as usize)
        } else {
            0
        }
    }
}

/// Проверяет, является ли символ алфавитным (Alphabetic).
/// Включает буквы всех алфавитов Unicode.
#[inline]
pub fn rtl_is_alphabetic(ch: u16) -> bool {
    if !nls_is_initialized() {
        // ASCII fallback
        return (ch >= b'A' as u16 && ch <= b'Z' as u16)
            || (ch >= b'a' as u16 && ch <= b'z' as u16);
    }
    (get_char_props(ch) & CHAR_PROP_ALPHABETIC) != 0
}

/// Проверяет, является ли символ прописной буквой (Uppercase).
#[inline]
pub fn rtl_is_uppercase(ch: u16) -> bool {
    if !nls_is_initialized() {
        // ASCII fallback
        return ch >= b'A' as u16 && ch <= b'Z' as u16;
    }
    (get_char_props(ch) & CHAR_PROP_UPPERCASE) != 0
}

/// Проверяет, является ли символ строчной буквой (Lowercase).
#[inline]
pub fn rtl_is_lowercase(ch: u16) -> bool {
    if !nls_is_initialized() {
        // ASCII fallback
        return ch >= b'a' as u16 && ch <= b'z' as u16;
    }
    (get_char_props(ch) & CHAR_PROP_LOWERCASE) != 0
}

/// Проверяет, является ли символ десятичной цифрой (0-9).
/// В Unicode это категория Nd (Decimal Number).
#[inline]
pub fn rtl_is_decimal_digit(ch: u16) -> bool {
    if !nls_is_initialized() {
        // ASCII fallback
        return ch >= b'0' as u16 && ch <= b'9' as u16;
    }
    (get_char_props(ch) & CHAR_PROP_DECIMAL_DIGIT) != 0
}

/// Проверяет, является ли символ пробельным (White_Space).
/// Включает пробел, табуляцию, переводы строк и другие Unicode пробелы.
#[inline]
pub fn rtl_is_white_space(ch: u16) -> bool {
    if !nls_is_initialized() {
        // ASCII fallback: space, tab, CR, LF, VT, FF
        return ch == b' ' as u16
            || ch == b'\t' as u16
            || ch == b'\r' as u16
            || ch == b'\n' as u16
            || ch == 0x0B  // VT
            || ch == 0x0C; // FF
    }
    (get_char_props(ch) & CHAR_PROP_WHITE_SPACE) != 0
}

/// Проверяет, является ли символ буквой или цифрой (Alphanumeric).
#[inline]
pub fn rtl_is_alphanumeric(ch: u16) -> bool {
    rtl_is_alphabetic(ch) || rtl_is_decimal_digit(ch)
}
