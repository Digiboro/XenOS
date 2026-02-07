//! Работа со строками в hive
//!
//! Строки в hive могут быть в двух форматах:
//! - Latin1 (ISO-8859-1) - 1 байт на символ, используется для ASCII-совместимых имен
//! - UTF-16LE - 2 байта на code unit, для Unicode имен
//!
//! Выбор формата определяется флагом KEY_COMP_NAME / VALUE_COMP_NAME.
//!
//! # Сравнение строк
//!
//! Windows реестр использует case-insensitive сравнение для имен ключей и значений.
//! Таблица преобразования к uppercase охватывает только Unicode BMP (первые 65535 символов).
//!
//! Источники:
//! - Windows SDK: ntrtl.h (RtlUpcaseUnicodeChar)

#[cfg(feature = "alloc")]
use alloc::string::String;
use core::cmp::Ordering;
use core::fmt;

// =============================================================================
// Таблица преобразования к uppercase
// =============================================================================

/// Таблица преобразования lowercase -> uppercase для Unicode BMP.
///
/// Windows использует эту таблицу для case-insensitive сравнения.
/// Сгенерирована из UnicodeData.txt.
static BMP_UPPERCASE_TABLE: &[(u16, u16)] = &[
    (0x61, 0x41), // a -> A
    (0x62, 0x42), // b -> B
    (0x63, 0x43), // c -> C
    (0x64, 0x44), // d -> D
    (0x65, 0x45), // e -> E
    (0x66, 0x46), // f -> F
    (0x67, 0x47), // g -> G
    (0x68, 0x48), // h -> H
    (0x69, 0x49), // i -> I
    (0x6a, 0x4a), // j -> J
    (0x6b, 0x4b), // k -> K
    (0x6c, 0x4c), // l -> L
    (0x6d, 0x4d), // m -> M
    (0x6e, 0x4e), // n -> N
    (0x6f, 0x4f), // o -> O
    (0x70, 0x50), // p -> P
    (0x71, 0x51), // q -> Q
    (0x72, 0x52), // r -> R
    (0x73, 0x53), // s -> S
    (0x74, 0x54), // t -> T
    (0x75, 0x55), // u -> U
    (0x76, 0x56), // v -> V
    (0x77, 0x57), // w -> W
    (0x78, 0x58), // x -> X
    (0x79, 0x59), // y -> Y
    (0x7a, 0x5a), // z -> Z
    // Расширенная латиница
    (0xe0, 0xc0),  // à -> À
    (0xe1, 0xc1),  // á -> Á
    (0xe2, 0xc2),  // â -> Â
    (0xe3, 0xc3),  // ã -> Ã
    (0xe4, 0xc4),  // ä -> Ä
    (0xe5, 0xc5),  // å -> Å
    (0xe6, 0xc6),  // æ -> Æ
    (0xe7, 0xc7),  // ç -> Ç
    (0xe8, 0xc8),  // è -> È
    (0xe9, 0xc9),  // é -> É
    (0xea, 0xca),  // ê -> Ê
    (0xeb, 0xcb),  // ë -> Ë
    (0xec, 0xcc),  // ì -> Ì
    (0xed, 0xcd),  // í -> Í
    (0xee, 0xce),  // î -> Î
    (0xef, 0xcf),  // ï -> Ï
    (0xf0, 0xd0),  // ð -> Ð
    (0xf1, 0xd1),  // ñ -> Ñ
    (0xf2, 0xd2),  // ò -> Ò
    (0xf3, 0xd3),  // ó -> Ó
    (0xf4, 0xd4),  // ô -> Ô
    (0xf5, 0xd5),  // õ -> Õ
    (0xf6, 0xd6),  // ö -> Ö
    (0xf8, 0xd8),  // ø -> Ø
    (0xf9, 0xd9),  // ù -> Ù
    (0xfa, 0xda),  // ú -> Ú
    (0xfb, 0xdb),  // û -> Û
    (0xfc, 0xdc),  // ü -> Ü
    (0xfd, 0xdd),  // ý -> Ý
    (0xfe, 0xde),  // þ -> Þ
    (0xff, 0x178), // ÿ -> Ÿ
    // Кириллица
    (0x430, 0x410), // а -> А
    (0x431, 0x411), // б -> Б
    (0x432, 0x412), // в -> В
    (0x433, 0x413), // г -> Г
    (0x434, 0x414), // д -> Д
    (0x435, 0x415), // е -> Е
    (0x436, 0x416), // ж -> Ж
    (0x437, 0x417), // з -> З
    (0x438, 0x418), // и -> И
    (0x439, 0x419), // й -> Й
    (0x43a, 0x41a), // к -> К
    (0x43b, 0x41b), // л -> Л
    (0x43c, 0x41c), // м -> М
    (0x43d, 0x41d), // н -> Н
    (0x43e, 0x41e), // о -> О
    (0x43f, 0x41f), // п -> П
    (0x440, 0x420), // р -> Р
    (0x441, 0x421), // с -> С
    (0x442, 0x422), // т -> Т
    (0x443, 0x423), // у -> У
    (0x444, 0x424), // ф -> Ф
    (0x445, 0x425), // х -> Х
    (0x446, 0x426), // ц -> Ц
    (0x447, 0x427), // ч -> Ч
    (0x448, 0x428), // ш -> Ш
    (0x449, 0x429), // щ -> Щ
    (0x44a, 0x42a), // ъ -> Ъ
    (0x44b, 0x42b), // ы -> Ы
    (0x44c, 0x42c), // ь -> Ь
    (0x44d, 0x42d), // э -> Э
    (0x44e, 0x42e), // ю -> Ю
    (0x44f, 0x42f), // я -> Я
    (0x451, 0x401), // ё -> Ё
    // Full-width латиница
    (0xff41, 0xff21), // ａ -> Ａ
    (0xff42, 0xff22),
    (0xff43, 0xff23),
    (0xff44, 0xff24),
    (0xff45, 0xff25),
    (0xff46, 0xff26),
    (0xff47, 0xff27),
    (0xff48, 0xff28),
    (0xff49, 0xff29),
    (0xff4a, 0xff2a),
    (0xff4b, 0xff2b),
    (0xff4c, 0xff2c),
    (0xff4d, 0xff2d),
    (0xff4e, 0xff2e),
    (0xff4f, 0xff2f),
    (0xff50, 0xff30),
    (0xff51, 0xff31),
    (0xff52, 0xff32),
    (0xff53, 0xff33),
    (0xff54, 0xff34),
    (0xff55, 0xff35),
    (0xff56, 0xff36),
    (0xff57, 0xff37),
    (0xff58, 0xff38),
    (0xff59, 0xff39),
    (0xff5a, 0xff3a), // ｚ -> Ｚ
];

/// Преобразует UTF-16 code unit к uppercase
fn utf16_to_uppercase(unit: u16) -> u16 {
    match BMP_UPPERCASE_TABLE.binary_search_by(|&(key, _)| key.cmp(&unit)) {
        Ok(index) => BMP_UPPERCASE_TABLE[index].1,
        Err(_) => unit,
    }
}

// =============================================================================
// HiveString
// =============================================================================

/// Zero-copy представление строки из hive.
///
/// Поддерживает два формата:
/// - Latin1 (ISO-8859-1): 1 байт на символ
/// - UTF-16LE: 2 байта на code unit
#[derive(Clone, Debug, Eq)]
pub enum HiveString<'h> {
    /// Latin1 строка (ASCII-совместимая)
    Latin1(&'h [u8]),
    /// UTF-16LE строка
    Utf16LE(&'h [u8]),
}

impl<'h> HiveString<'h> {
    /// Проверяет, пустая ли строка.
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Возвращает длину строки в байтах.
    ///
    /// Внимание: это не количество символов!
    pub const fn len(&self) -> usize {
        match self {
            Self::Latin1(bytes) => bytes.len(),
            Self::Utf16LE(bytes) => bytes.len(),
        }
    }

    /// Конвертирует в String, возвращая None при ошибке декодирования.
    #[cfg(feature = "alloc")]
    pub fn to_string_checked(&self) -> Option<String> {
        match self {
            Self::Latin1(bytes) => {
                let string: String = bytes.iter().map(|&b| b as char).collect();
                Some(string)
            },
            Self::Utf16LE(bytes) => {
                let mut string = String::new();
                for chunk in bytes.chunks_exact(2) {
                    let code_unit = u16::from_le_bytes([chunk[0], chunk[1]]);
                    if let Some(c) = char::from_u32(code_unit as u32) {
                        string.push(c);
                    } else {
                        return None;
                    }
                }
                Some(string)
            },
        }
    }

    /// Конвертирует в String, заменяя невалидные символы на U+FFFD.
    #[cfg(feature = "alloc")]
    pub fn to_string_lossy(&self) -> String {
        match self {
            Self::Latin1(bytes) => bytes.iter().map(|&b| b as char).collect(),
            Self::Utf16LE(bytes) => {
                let mut string = String::new();
                for chunk in bytes.chunks_exact(2) {
                    let code_unit = u16::from_le_bytes([chunk[0], chunk[1]]);
                    let c = char::from_u32(code_unit as u32).unwrap_or(char::REPLACEMENT_CHARACTER);
                    string.push(c);
                }
                string
            },
        }
    }

    /// Итератор по UTF-16 code units для Latin1 строки
    fn latin1_iter(&'h self) -> impl Iterator<Item = u16> + 'h {
        match self {
            Self::Latin1(bytes) => bytes.iter().map(|&b| b as u16),
            Self::Utf16LE(_) => panic!("latin1_iter вызван для Utf16LE"),
        }
    }

    /// Итератор по UTF-16 code units для UTF-16LE строки
    fn utf16le_iter(&'h self) -> impl Iterator<Item = u16> + 'h {
        match self {
            Self::Latin1(_) => panic!("utf16le_iter вызван для Latin1"),
            Self::Utf16LE(bytes) => bytes
                .chunks_exact(2)
                .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]])),
        }
    }

    /// Сравнивает два итератора UTF-16 code units case-insensitively
    fn cmp_iter<TI, OI>(mut this_iter: TI, mut other_iter: OI) -> Ordering
    where
        TI: Iterator<Item = u16>,
        OI: Iterator<Item = u16>,
    {
        loop {
            match (this_iter.next(), other_iter.next()) {
                (Some(this_unit), Some(other_unit)) => {
                    let this_upper = utf16_to_uppercase(this_unit);
                    let other_upper = utf16_to_uppercase(other_unit);

                    if this_upper != other_upper {
                        return this_upper.cmp(&other_upper);
                    }
                },
                (Some(_), None) => return Ordering::Greater,
                (None, Some(_)) => return Ordering::Less,
                (None, None) => return Ordering::Equal,
            }
        }
    }

    /// Сравнивает HiveString с &str
    fn cmp_with_str(&self, other: &str) -> Ordering {
        let other_iter = other.encode_utf16();

        match self {
            Self::Latin1(_) => Self::cmp_iter(self.latin1_iter(), other_iter),
            Self::Utf16LE(_) => Self::cmp_iter(self.utf16le_iter(), other_iter),
        }
    }
}

// =============================================================================
// Trait implementations
// =============================================================================

impl fmt::Display for HiveString<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Latin1(bytes) => {
                for &byte in *bytes {
                    write!(f, "{}", byte as char)?;
                }
            },
            Self::Utf16LE(bytes) => {
                for chunk in bytes.chunks_exact(2) {
                    let code_unit = u16::from_le_bytes([chunk[0], chunk[1]]);
                    let c = char::from_u32(code_unit as u32).unwrap_or(char::REPLACEMENT_CHARACTER);
                    write!(f, "{}", c)?;
                }
            },
        }
        Ok(())
    }
}

impl Ord for HiveString<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Latin1(_), Self::Latin1(_)) => {
                Self::cmp_iter(self.latin1_iter(), other.latin1_iter())
            },
            (Self::Latin1(_), Self::Utf16LE(_)) => {
                Self::cmp_iter(self.latin1_iter(), other.utf16le_iter())
            },
            (Self::Utf16LE(_), Self::Latin1(_)) => {
                Self::cmp_iter(self.utf16le_iter(), other.latin1_iter())
            },
            (Self::Utf16LE(_), Self::Utf16LE(_)) => {
                Self::cmp_iter(self.utf16le_iter(), other.utf16le_iter())
            },
        }
    }
}

impl PartialEq for HiveString<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl PartialEq<str> for HiveString<'_> {
    fn eq(&self, other: &str) -> bool {
        self.cmp_with_str(other) == Ordering::Equal
    }
}

impl PartialEq<&str> for HiveString<'_> {
    fn eq(&self, other: &&str) -> bool {
        self.cmp_with_str(other) == Ordering::Equal
    }
}

impl PartialOrd for HiveString<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialOrd<str> for HiveString<'_> {
    fn partial_cmp(&self, other: &str) -> Option<Ordering> {
        Some(self.cmp_with_str(other))
    }
}

impl PartialOrd<&str> for HiveString<'_> {
    fn partial_cmp(&self, other: &&str) -> Option<Ordering> {
        Some(self.cmp_with_str(other))
    }
}
