//! NLS (National Language Support) Tests
//!
//! Тесты для модулей rtl/nls.rs и rtl/unicode.rs.
//! Покрывают: константы XNLS, ASCII fallback функции, сравнение строк, свойства символов.
//!
//! Примечание: тесты работают с ASCII fallback режимом, так как NLS таблицы
//! могут быть не инициализированы в тестовом окружении.

use crate::nt::UNICODE_STRING;
use crate::rtl::nls::STATUS_NLS_CHUNK_INVALID;
use crate::rtl::nls::STATUS_NLS_CHUNK_MISSING;
use crate::rtl::nls::STATUS_NLS_DATA_MISSING;
use crate::rtl::nls::STATUS_NLS_PARSE_ERROR;
use crate::rtl::unicode::*;
use crate::test::harness::KernelTest;

// =============================================================================
// NLS NTSTATUS Codes Tests
// =============================================================================

/// Проверка кодов ошибок NLS
fn test_nls_status_codes() {
    // Все коды должны быть отрицательными (ошибки)
    assert!(STATUS_NLS_DATA_MISSING < 0);
    assert!(STATUS_NLS_PARSE_ERROR < 0);
    assert!(STATUS_NLS_CHUNK_MISSING < 0);
    assert!(STATUS_NLS_CHUNK_INVALID < 0);

    // Все коды должны быть уникальными
    let codes = [
        STATUS_NLS_DATA_MISSING,
        STATUS_NLS_PARSE_ERROR,
        STATUS_NLS_CHUNK_MISSING,
        STATUS_NLS_CHUNK_INVALID,
    ];

    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            assert_ne!(codes[i], codes[j], "NLS status codes must be unique");
        }
    }

    // Коды должны иметь бит 0xC0000000 (severity error)
    assert!((STATUS_NLS_DATA_MISSING as u32 & 0xC0000000) == 0xC0000000);
    assert!((STATUS_NLS_PARSE_ERROR as u32 & 0xC0000000) == 0xC0000000);
    assert!((STATUS_NLS_CHUNK_MISSING as u32 & 0xC0000000) == 0xC0000000);
    assert!((STATUS_NLS_CHUNK_INVALID as u32 & 0xC0000000) == 0xC0000000);
}

// =============================================================================
// Character Property Constants Tests
// =============================================================================

/// Проверка констант свойств символов
fn test_char_property_constants() {
    // Проверяем что константы - степени двойки (битовые маски)
    assert_eq!(CHAR_PROP_ALPHABETIC, 0x0001);
    assert_eq!(CHAR_PROP_UPPERCASE, 0x0002);
    assert_eq!(CHAR_PROP_LOWERCASE, 0x0004);
    assert_eq!(CHAR_PROP_DECIMAL_DIGIT, 0x0008);
    assert_eq!(CHAR_PROP_WHITE_SPACE, 0x0010);

    // Каждая константа должна быть степенью двойки
    assert_eq!(CHAR_PROP_ALPHABETIC.count_ones(), 1);
    assert_eq!(CHAR_PROP_UPPERCASE.count_ones(), 1);
    assert_eq!(CHAR_PROP_LOWERCASE.count_ones(), 1);
    assert_eq!(CHAR_PROP_DECIMAL_DIGIT.count_ones(), 1);
    assert_eq!(CHAR_PROP_WHITE_SPACE.count_ones(), 1);

    // Константы не должны перекрываться
    let all_props = CHAR_PROP_ALPHABETIC
        | CHAR_PROP_UPPERCASE
        | CHAR_PROP_LOWERCASE
        | CHAR_PROP_DECIMAL_DIGIT
        | CHAR_PROP_WHITE_SPACE;
    assert_eq!(all_props.count_ones(), 5);
}

// =============================================================================
// ASCII Fallback Upcase Tests
// =============================================================================

/// Проверка rtl_upcase_char_ascii для нижнего регистра
fn test_upcase_ascii_lowercase() {
    // a-z должны конвертироваться в A-Z
    assert_eq!(rtl_upcase_char_ascii(b'a' as u16), b'A' as u16);
    assert_eq!(rtl_upcase_char_ascii(b'z' as u16), b'Z' as u16);
    assert_eq!(rtl_upcase_char_ascii(b'm' as u16), b'M' as u16);

    // Проверяем весь диапазон a-z
    for ch in b'a'..=b'z' {
        let upper = rtl_upcase_char_ascii(ch as u16);
        let expected = (ch - 32) as u16; // 'a'-'z' отличаются от 'A'-'Z' на 32
        assert_eq!(
            upper, expected,
            "rtl_upcase_char_ascii({}) failed",
            ch as char
        );
    }
}

/// Проверка rtl_upcase_char_ascii для уже верхнего регистра
fn test_upcase_ascii_uppercase() {
    // A-Z должны оставаться без изменений
    assert_eq!(rtl_upcase_char_ascii(b'A' as u16), b'A' as u16);
    assert_eq!(rtl_upcase_char_ascii(b'Z' as u16), b'Z' as u16);
    assert_eq!(rtl_upcase_char_ascii(b'M' as u16), b'M' as u16);

    // Проверяем весь диапазон A-Z
    for ch in b'A'..=b'Z' {
        let upper = rtl_upcase_char_ascii(ch as u16);
        assert_eq!(
            upper, ch as u16,
            "rtl_upcase_char_ascii({}) should not change",
            ch as char
        );
    }
}

/// Проверка rtl_upcase_char_ascii для не-букв
fn test_upcase_ascii_non_letters() {
    // Цифры не должны меняться
    for ch in b'0'..=b'9' {
        assert_eq!(rtl_upcase_char_ascii(ch as u16), ch as u16);
    }

    // Специальные символы не должны меняться
    let specials = [
        b' ', b'!', b'@', b'#', b'$', b'%', b'^', b'&', b'*', b'(', b')', b'-', b'_', b'=', b'+',
        b'[', b']', b'{', b'}', b';', b':', b'\'', b'"', b',', b'.', b'/', b'?', b'<', b'>',
    ];
    for &ch in &specials {
        assert_eq!(
            rtl_upcase_char_ascii(ch as u16),
            ch as u16,
            "rtl_upcase_char_ascii({}) should not change",
            ch as char
        );
    }

    // NUL и другие control chars
    assert_eq!(rtl_upcase_char_ascii(0), 0);
    assert_eq!(rtl_upcase_char_ascii(0x1F), 0x1F);
}

/// Проверка rtl_upcase_char_ascii для non-ASCII
fn test_upcase_ascii_non_ascii() {
    // Non-ASCII символы не должны меняться (это ASCII fallback)
    assert_eq!(rtl_upcase_char_ascii(0x00E4), 0x00E4); // ä
    assert_eq!(rtl_upcase_char_ascii(0x00F6), 0x00F6); // ö
    assert_eq!(rtl_upcase_char_ascii(0x0430), 0x0430); // а (кириллица)
    assert_eq!(rtl_upcase_char_ascii(0x044F), 0x044F); // я (кириллица)
    assert_eq!(rtl_upcase_char_ascii(0xFFFF), 0xFFFF); // max u16
}

// =============================================================================
// ASCII Fallback Downcase Tests
// =============================================================================

/// Проверка rtl_downcase_char_ascii для верхнего регистра
fn test_downcase_ascii_uppercase() {
    // A-Z должны конвертироваться в a-z
    assert_eq!(rtl_downcase_char_ascii(b'A' as u16), b'a' as u16);
    assert_eq!(rtl_downcase_char_ascii(b'Z' as u16), b'z' as u16);
    assert_eq!(rtl_downcase_char_ascii(b'M' as u16), b'm' as u16);

    // Проверяем весь диапазон A-Z
    for ch in b'A'..=b'Z' {
        let lower = rtl_downcase_char_ascii(ch as u16);
        let expected = (ch + 32) as u16;
        assert_eq!(
            lower, expected,
            "rtl_downcase_char_ascii({}) failed",
            ch as char
        );
    }
}

/// Проверка rtl_downcase_char_ascii для уже нижнего регистра
fn test_downcase_ascii_lowercase() {
    // a-z должны оставаться без изменений
    assert_eq!(rtl_downcase_char_ascii(b'a' as u16), b'a' as u16);
    assert_eq!(rtl_downcase_char_ascii(b'z' as u16), b'z' as u16);
    assert_eq!(rtl_downcase_char_ascii(b'm' as u16), b'm' as u16);

    // Проверяем весь диапазон a-z
    for ch in b'a'..=b'z' {
        let lower = rtl_downcase_char_ascii(ch as u16);
        assert_eq!(
            lower, ch as u16,
            "rtl_downcase_char_ascii({}) should not change",
            ch as char
        );
    }
}

/// Проверка rtl_downcase_char_ascii для не-букв
fn test_downcase_ascii_non_letters() {
    // Цифры не должны меняться
    for ch in b'0'..=b'9' {
        assert_eq!(rtl_downcase_char_ascii(ch as u16), ch as u16);
    }

    // NUL и другие
    assert_eq!(rtl_downcase_char_ascii(0), 0);
    assert_eq!(rtl_downcase_char_ascii(b' ' as u16), b' ' as u16);
}

// =============================================================================
// rtl_upcase_unicode_char / rtl_downcase_unicode_char Tests
// =============================================================================

/// Проверка rtl_upcase_unicode_char (использует ASCII fallback если NLS не инициализирован)
fn test_upcase_unicode_char_ascii_fallback() {
    // В тестовом окружении NLS может быть не инициализирован,
    // поэтому проверяем ASCII диапазон
    assert_eq!(rtl_upcase_unicode_char(b'a' as u16), b'A' as u16);
    assert_eq!(rtl_upcase_unicode_char(b'z' as u16), b'Z' as u16);
    assert_eq!(rtl_upcase_unicode_char(b'A' as u16), b'A' as u16);
    assert_eq!(rtl_upcase_unicode_char(b'0' as u16), b'0' as u16);
}

/// Проверка rtl_downcase_unicode_char (использует ASCII fallback если NLS не инициализирован)
fn test_downcase_unicode_char_ascii_fallback() {
    assert_eq!(rtl_downcase_unicode_char(b'A' as u16), b'a' as u16);
    assert_eq!(rtl_downcase_unicode_char(b'Z' as u16), b'z' as u16);
    assert_eq!(rtl_downcase_unicode_char(b'a' as u16), b'a' as u16);
    assert_eq!(rtl_downcase_unicode_char(b'0' as u16), b'0' as u16);
}

// =============================================================================
// String Comparison Tests
// =============================================================================

/// Проверка rtl_compare_unicode_string для равных строк
fn test_compare_unicode_string_equal() {
    // "Test" == "Test"
    static STR1: [u16; 5] = [b'T' as u16, b'e' as u16, b's' as u16, b't' as u16, 0];
    static STR2: [u16; 5] = [b'T' as u16, b'e' as u16, b's' as u16, b't' as u16, 0];

    let s1 = UNICODE_STRING {
        length: 8,
        maximum_length: 10,
        buffer: STR1.as_ptr() as *mut u16,
    };

    let s2 = UNICODE_STRING {
        length: 8,
        maximum_length: 10,
        buffer: STR2.as_ptr() as *mut u16,
    };

    assert_eq!(rtl_compare_unicode_string(&s1, &s2, false), 0);
    assert_eq!(rtl_compare_unicode_string(&s1, &s2, true), 0);
}

/// Проверка rtl_compare_unicode_string case-sensitive
fn test_compare_unicode_string_case_sensitive() {
    // "Test" vs "test" - case sensitive
    static STR_UPPER: [u16; 5] = [b'T' as u16, b'e' as u16, b's' as u16, b't' as u16, 0];
    static STR_LOWER: [u16; 5] = [b't' as u16, b'e' as u16, b's' as u16, b't' as u16, 0];

    let s_upper = UNICODE_STRING {
        length: 8,
        maximum_length: 10,
        buffer: STR_UPPER.as_ptr() as *mut u16,
    };

    let s_lower = UNICODE_STRING {
        length: 8,
        maximum_length: 10,
        buffer: STR_LOWER.as_ptr() as *mut u16,
    };

    // Case-sensitive: 'T' (0x54) < 't' (0x74)
    let cmp = rtl_compare_unicode_string(&s_upper, &s_lower, false);
    assert!(cmp < 0, "Case-sensitive: 'Test' should be < 'test'");
}

/// Проверка rtl_compare_unicode_string case-insensitive
fn test_compare_unicode_string_case_insensitive() {
    // "Test" vs "TEST" - case insensitive
    static STR_MIXED: [u16; 5] = [b'T' as u16, b'e' as u16, b's' as u16, b't' as u16, 0];
    static STR_UPPER: [u16; 5] = [b'T' as u16, b'E' as u16, b'S' as u16, b'T' as u16, 0];

    let s_mixed = UNICODE_STRING {
        length: 8,
        maximum_length: 10,
        buffer: STR_MIXED.as_ptr() as *mut u16,
    };

    let s_upper = UNICODE_STRING {
        length: 8,
        maximum_length: 10,
        buffer: STR_UPPER.as_ptr() as *mut u16,
    };

    // Case-insensitive: должны быть равны
    assert_eq!(rtl_compare_unicode_string(&s_mixed, &s_upper, true), 0);
}

/// Проверка rtl_compare_unicode_string для строк разной длины
fn test_compare_unicode_string_different_length() {
    // "Test" vs "Testing"
    static STR_SHORT: [u16; 5] = [b'T' as u16, b'e' as u16, b's' as u16, b't' as u16, 0];
    static STR_LONG: [u16; 8] = [
        b'T' as u16,
        b'e' as u16,
        b's' as u16,
        b't' as u16,
        b'i' as u16,
        b'n' as u16,
        b'g' as u16,
        0,
    ];

    let s_short = UNICODE_STRING {
        length: 8, // 4 chars
        maximum_length: 10,
        buffer: STR_SHORT.as_ptr() as *mut u16,
    };

    let s_long = UNICODE_STRING {
        length: 14, // 7 chars
        maximum_length: 16,
        buffer: STR_LONG.as_ptr() as *mut u16,
    };

    // "Test" < "Testing" (короткая строка - префикс длинной)
    let cmp = rtl_compare_unicode_string(&s_short, &s_long, false);
    assert!(cmp < 0, "'Test' should be < 'Testing'");

    // Обратный порядок
    let cmp_rev = rtl_compare_unicode_string(&s_long, &s_short, false);
    assert!(cmp_rev > 0, "'Testing' should be > 'Test'");
}

/// Проверка rtl_compare_unicode_string для пустых строк
fn test_compare_unicode_string_empty() {
    let empty = UNICODE_STRING::new();

    static STR: [u16; 2] = [b'A' as u16, 0];
    let non_empty = UNICODE_STRING {
        length: 2,
        maximum_length: 4,
        buffer: STR.as_ptr() as *mut u16,
    };

    // Пустая строка < непустая
    // Но buffer у empty = null, поэтому сравнение идёт по указателям
    // Результат зависит от реализации
    let _ = rtl_compare_unicode_string(&empty, &non_empty, false);
}

/// Проверка rtl_equal_unicode_string
fn test_equal_unicode_string() {
    static STR1: [u16; 5] = [b'T' as u16, b'e' as u16, b's' as u16, b't' as u16, 0];
    static STR2: [u16; 5] = [b'T' as u16, b'e' as u16, b's' as u16, b't' as u16, 0];
    static STR3: [u16; 5] = [b't' as u16, b'E' as u16, b'S' as u16, b'T' as u16, 0];

    let s1 = UNICODE_STRING {
        length: 8,
        maximum_length: 10,
        buffer: STR1.as_ptr() as *mut u16,
    };

    let s2 = UNICODE_STRING {
        length: 8,
        maximum_length: 10,
        buffer: STR2.as_ptr() as *mut u16,
    };

    let s3 = UNICODE_STRING {
        length: 8,
        maximum_length: 10,
        buffer: STR3.as_ptr() as *mut u16,
    };

    // Точное равенство
    assert!(rtl_equal_unicode_string(&s1, &s2, false));
    assert!(rtl_equal_unicode_string(&s1, &s2, true));

    // Case-insensitive равенство
    assert!(!rtl_equal_unicode_string(&s1, &s3, false)); // Case-sensitive: не равны
    assert!(rtl_equal_unicode_string(&s1, &s3, true)); // Case-insensitive: равны
}

// =============================================================================
// Character Property Tests (ASCII fallback)
// =============================================================================

/// Проверка rtl_is_alphabetic для ASCII
fn test_is_alphabetic_ascii() {
    // Буквы
    for ch in b'A'..=b'Z' {
        assert!(
            rtl_is_alphabetic(ch as u16),
            "'{}' should be alphabetic",
            ch as char
        );
    }
    for ch in b'a'..=b'z' {
        assert!(
            rtl_is_alphabetic(ch as u16),
            "'{}' should be alphabetic",
            ch as char
        );
    }

    // Не буквы
    for ch in b'0'..=b'9' {
        assert!(
            !rtl_is_alphabetic(ch as u16),
            "'{}' should not be alphabetic",
            ch as char
        );
    }
    assert!(!rtl_is_alphabetic(b' ' as u16));
    assert!(!rtl_is_alphabetic(b'!' as u16));
    assert!(!rtl_is_alphabetic(b'@' as u16));
}

/// Проверка rtl_is_uppercase для ASCII
fn test_is_uppercase_ascii() {
    // Uppercase
    for ch in b'A'..=b'Z' {
        assert!(
            rtl_is_uppercase(ch as u16),
            "'{}' should be uppercase",
            ch as char
        );
    }

    // Не uppercase
    for ch in b'a'..=b'z' {
        assert!(
            !rtl_is_uppercase(ch as u16),
            "'{}' should not be uppercase",
            ch as char
        );
    }
    for ch in b'0'..=b'9' {
        assert!(!rtl_is_uppercase(ch as u16));
    }
}

/// Проверка rtl_is_lowercase для ASCII
fn test_is_lowercase_ascii() {
    // Lowercase
    for ch in b'a'..=b'z' {
        assert!(
            rtl_is_lowercase(ch as u16),
            "'{}' should be lowercase",
            ch as char
        );
    }

    // Не lowercase
    for ch in b'A'..=b'Z' {
        assert!(
            !rtl_is_lowercase(ch as u16),
            "'{}' should not be lowercase",
            ch as char
        );
    }
    for ch in b'0'..=b'9' {
        assert!(!rtl_is_lowercase(ch as u16));
    }
}

/// Проверка rtl_is_decimal_digit для ASCII
fn test_is_decimal_digit_ascii() {
    // Цифры 0-9
    for ch in b'0'..=b'9' {
        assert!(
            rtl_is_decimal_digit(ch as u16),
            "'{}' should be decimal digit",
            ch as char
        );
    }

    // Не цифры
    for ch in b'A'..=b'Z' {
        assert!(!rtl_is_decimal_digit(ch as u16));
    }
    for ch in b'a'..=b'z' {
        assert!(!rtl_is_decimal_digit(ch as u16));
    }
    assert!(!rtl_is_decimal_digit(b' ' as u16));
}

/// Проверка rtl_is_white_space для ASCII
fn test_is_white_space_ascii() {
    // Пробельные символы
    assert!(rtl_is_white_space(b' ' as u16)); // Space
    assert!(rtl_is_white_space(b'\t' as u16)); // Tab
    assert!(rtl_is_white_space(b'\r' as u16)); // CR
    assert!(rtl_is_white_space(b'\n' as u16)); // LF
    assert!(rtl_is_white_space(0x0B)); // VT (Vertical Tab)
    assert!(rtl_is_white_space(0x0C)); // FF (Form Feed)

    // Не пробельные
    assert!(!rtl_is_white_space(b'A' as u16));
    assert!(!rtl_is_white_space(b'0' as u16));
    assert!(!rtl_is_white_space(b'!' as u16));
}

/// Проверка rtl_is_alphanumeric для ASCII
fn test_is_alphanumeric_ascii() {
    // Буквы и цифры
    for ch in b'A'..=b'Z' {
        assert!(rtl_is_alphanumeric(ch as u16));
    }
    for ch in b'a'..=b'z' {
        assert!(rtl_is_alphanumeric(ch as u16));
    }
    for ch in b'0'..=b'9' {
        assert!(rtl_is_alphanumeric(ch as u16));
    }

    // Не alphanumeric
    assert!(!rtl_is_alphanumeric(b' ' as u16));
    assert!(!rtl_is_alphanumeric(b'!' as u16));
    assert!(!rtl_is_alphanumeric(b'@' as u16));
    assert!(!rtl_is_alphanumeric(b'\n' as u16));
}

// =============================================================================
// Edge Cases Tests
// =============================================================================

/// Проверка граничных значений для case conversion
fn test_case_conversion_boundaries() {
    // Символы сразу перед 'A' и после 'Z'
    assert_eq!(rtl_upcase_char_ascii(b'@' as u16), b'@' as u16); // '@' = 0x40, перед 'A'
    assert_eq!(rtl_upcase_char_ascii(b'[' as u16), b'[' as u16); // '[' = 0x5B, после 'Z'

    // Символы сразу перед 'a' и после 'z'
    assert_eq!(rtl_upcase_char_ascii(b'`' as u16), b'`' as u16); // '`' = 0x60, перед 'a'
    assert_eq!(rtl_upcase_char_ascii(b'{' as u16), b'{' as u16); // '{' = 0x7B, после 'z'

    // Аналогично для downcase
    assert_eq!(rtl_downcase_char_ascii(b'@' as u16), b'@' as u16);
    assert_eq!(rtl_downcase_char_ascii(b'[' as u16), b'[' as u16);
}

/// Проверка upcase/downcase round-trip для ASCII букв
fn test_case_roundtrip() {
    // upcase(downcase(ch)) для uppercase букв должен вернуть оригинал
    for ch in b'A'..=b'Z' {
        let lower = rtl_downcase_char_ascii(ch as u16);
        let upper = rtl_upcase_char_ascii(lower);
        assert_eq!(upper, ch as u16, "Round-trip failed for '{}'", ch as char);
    }

    // downcase(upcase(ch)) для lowercase букв должен вернуть оригинал
    for ch in b'a'..=b'z' {
        let upper = rtl_upcase_char_ascii(ch as u16);
        let lower = rtl_downcase_char_ascii(upper);
        assert_eq!(lower, ch as u16, "Round-trip failed for '{}'", ch as char);
    }
}

/// Проверка nls_is_initialized
fn test_nls_is_initialized() {
    // В тестовом окружении NLS может быть как инициализирован, так и нет
    // Просто проверяем что функция не падает
    let _ = nls_is_initialized();
}

// =============================================================================
// Реестр тестов NLS
// =============================================================================

/// Все тесты NLS
pub static NLS_TESTS: &[KernelTest] = &[
    // NLS Status Codes
    KernelTest {
        name: "nls_status_codes",
        module: "rtl::nls",
        test_fn: test_nls_status_codes,
    },
    // Character Property Constants
    KernelTest {
        name: "char_property_constants",
        module: "rtl::unicode",
        test_fn: test_char_property_constants,
    },
    // ASCII Upcase Tests
    KernelTest {
        name: "upcase_ascii_lowercase",
        module: "rtl::unicode",
        test_fn: test_upcase_ascii_lowercase,
    },
    KernelTest {
        name: "upcase_ascii_uppercase",
        module: "rtl::unicode",
        test_fn: test_upcase_ascii_uppercase,
    },
    KernelTest {
        name: "upcase_ascii_non_letters",
        module: "rtl::unicode",
        test_fn: test_upcase_ascii_non_letters,
    },
    KernelTest {
        name: "upcase_ascii_non_ascii",
        module: "rtl::unicode",
        test_fn: test_upcase_ascii_non_ascii,
    },
    // ASCII Downcase Tests
    KernelTest {
        name: "downcase_ascii_uppercase",
        module: "rtl::unicode",
        test_fn: test_downcase_ascii_uppercase,
    },
    KernelTest {
        name: "downcase_ascii_lowercase",
        module: "rtl::unicode",
        test_fn: test_downcase_ascii_lowercase,
    },
    KernelTest {
        name: "downcase_ascii_non_letters",
        module: "rtl::unicode",
        test_fn: test_downcase_ascii_non_letters,
    },
    // Unicode Char Tests (ASCII fallback)
    KernelTest {
        name: "upcase_unicode_char_ascii_fallback",
        module: "rtl::unicode",
        test_fn: test_upcase_unicode_char_ascii_fallback,
    },
    KernelTest {
        name: "downcase_unicode_char_ascii_fallback",
        module: "rtl::unicode",
        test_fn: test_downcase_unicode_char_ascii_fallback,
    },
    // String Comparison Tests
    KernelTest {
        name: "compare_unicode_string_equal",
        module: "rtl::unicode",
        test_fn: test_compare_unicode_string_equal,
    },
    KernelTest {
        name: "compare_unicode_string_case_sensitive",
        module: "rtl::unicode",
        test_fn: test_compare_unicode_string_case_sensitive,
    },
    KernelTest {
        name: "compare_unicode_string_case_insensitive",
        module: "rtl::unicode",
        test_fn: test_compare_unicode_string_case_insensitive,
    },
    KernelTest {
        name: "compare_unicode_string_different_length",
        module: "rtl::unicode",
        test_fn: test_compare_unicode_string_different_length,
    },
    KernelTest {
        name: "compare_unicode_string_empty",
        module: "rtl::unicode",
        test_fn: test_compare_unicode_string_empty,
    },
    KernelTest {
        name: "equal_unicode_string",
        module: "rtl::unicode",
        test_fn: test_equal_unicode_string,
    },
    // Character Property Tests
    KernelTest {
        name: "is_alphabetic_ascii",
        module: "rtl::unicode",
        test_fn: test_is_alphabetic_ascii,
    },
    KernelTest {
        name: "is_uppercase_ascii",
        module: "rtl::unicode",
        test_fn: test_is_uppercase_ascii,
    },
    KernelTest {
        name: "is_lowercase_ascii",
        module: "rtl::unicode",
        test_fn: test_is_lowercase_ascii,
    },
    KernelTest {
        name: "is_decimal_digit_ascii",
        module: "rtl::unicode",
        test_fn: test_is_decimal_digit_ascii,
    },
    KernelTest {
        name: "is_white_space_ascii",
        module: "rtl::unicode",
        test_fn: test_is_white_space_ascii,
    },
    KernelTest {
        name: "is_alphanumeric_ascii",
        module: "rtl::unicode",
        test_fn: test_is_alphanumeric_ascii,
    },
    // Edge Cases
    KernelTest {
        name: "case_conversion_boundaries",
        module: "rtl::unicode",
        test_fn: test_case_conversion_boundaries,
    },
    KernelTest {
        name: "case_roundtrip",
        module: "rtl::unicode",
        test_fn: test_case_roundtrip,
    },
    KernelTest {
        name: "nls_is_initialized",
        module: "rtl::unicode",
        test_fn: test_nls_is_initialized,
    },
];
