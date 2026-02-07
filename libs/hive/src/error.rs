//! Типы ошибок библиотеки hive
//!
//! Все ошибки содержат информацию о смещении в файле для диагностики.

use core::fmt;

use crate::key_value::KeyValueDataType;

/// Результат операции с hive
pub type Result<T, E = HiveError> = core::result::Result<T, E>;

/// Ошибки при работе с hive файлами
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HiveError {
    /// Неверная контрольная сумма в базовом блоке
    InvalidChecksum {
        /// Ожидаемая контрольная сумма
        expected: u32,
        /// Фактическая контрольная сумма
        actual: u32,
    },

    /// Неверный размер данных
    InvalidDataSize {
        /// Смещение в файле
        offset: usize,
        /// Ожидаемый размер
        expected: usize,
        /// Фактический размер
        actual: usize,
    },

    /// Неверная 4-байтовая сигнатура
    InvalidFourByteSignature {
        /// Смещение в файле
        offset: usize,
        /// Ожидаемая сигнатура
        expected: &'static [u8; 4],
        /// Фактическая сигнатура
        actual: [u8; 4],
    },

    /// Неверная 2-байтовая сигнатура
    InvalidTwoByteSignature {
        /// Смещение в файле
        offset: usize,
        /// Ожидаемая сигнатура (может быть несколько вариантов через |)
        expected: &'static [u8],
        /// Фактическая сигнатура
        actual: [u8; 2],
    },

    /// Недостаточный размер заголовка структуры
    InvalidHeaderSize {
        /// Смещение в файле
        offset: usize,
        /// Ожидаемый размер
        expected: usize,
        /// Фактический размер
        actual: usize,
    },

    /// Неверный тип данных значения
    InvalidKeyValueDataType {
        /// Ожидаемые типы
        expected: &'static [KeyValueDataType],
        /// Фактический тип
        actual: KeyValueDataType,
    },

    /// Неверное поле размера
    InvalidSizeField {
        /// Смещение поля в файле
        offset: usize,
        /// Ожидаемый размер (минимум)
        expected: usize,
        /// Фактический размер
        actual: usize,
    },

    /// Неверное выравнивание поля размера
    InvalidSizeFieldAlignment {
        /// Смещение поля в файле
        offset: usize,
        /// Размер
        size: usize,
        /// Ожидаемое выравнивание
        expected_alignment: usize,
    },

    /// Несовпадение sequence numbers (hive не был корректно закрыт)
    SequenceNumberMismatch {
        /// Primary sequence number
        primary: u32,
        /// Secondary sequence number
        secondary: u32,
    },

    /// Cell не аллоцирована (size > 0)
    UnallocatedCell {
        /// Смещение cell в файле
        offset: usize,
        /// Размер cell
        size: i32,
    },

    /// Неподдерживаемый clustering factor
    UnsupportedClusteringFactor {
        /// Ожидаемое значение
        expected: u32,
        /// Фактическое значение
        actual: u32,
    },

    /// Неподдерживаемый формат файла
    UnsupportedFileFormat {
        /// Ожидаемое значение
        expected: u32,
        /// Фактическое значение
        actual: u32,
    },

    /// Неподдерживаемый тип файла
    UnsupportedFileType {
        /// Ожидаемое значение
        expected: u32,
        /// Фактическое значение
        actual: u32,
    },

    /// Неподдерживаемый тип данных значения
    UnsupportedKeyValueDataType {
        /// Смещение в файле
        offset: usize,
        /// Код типа
        actual: u32,
    },

    /// Неподдерживаемая версия hive
    UnsupportedVersion {
        /// Major версия
        major: u32,
        /// Minor версия
        minor: u32,
    },

    /// Недостаточно места для аллокации (для write feature)
    #[cfg(feature = "write")]
    OutOfSpace {
        /// Требуемый размер
        required: usize,
        /// Доступный размер
        available: usize,
    },

    /// Ключ не найден (для write feature)
    #[cfg(feature = "write")]
    KeyNotFound,

    /// Значение не найдено (для write feature)
    #[cfg(feature = "write")]
    ValueNotFound,
}

impl fmt::Display for HiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidChecksum { expected, actual } => {
                write!(
                    f,
                    "Неверная контрольная сумма: ожидалось {expected:#x}, получено {actual:#x}"
                )
            },
            Self::InvalidDataSize {
                offset,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "Неверный размер данных по смещению {offset:#x}: ожидалось {expected} байт, доступно {actual}"
                )
            },
            Self::InvalidFourByteSignature {
                offset,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "Неверная сигнатура по смещению {offset:#x}: ожидалось {:?}, получено {:?}",
                    expected, actual
                )
            },
            Self::InvalidTwoByteSignature {
                offset,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "Неверная сигнатура по смещению {offset:#x}: ожидалось {:?}, получено {:?}",
                    expected, actual
                )
            },
            Self::InvalidHeaderSize {
                offset,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "Недостаточный размер заголовка по смещению {offset:#x}: требуется {expected} байт, доступно {actual}"
                )
            },
            Self::InvalidKeyValueDataType { expected, actual } => {
                write!(
                    f,
                    "Неверный тип данных значения: ожидалось {expected:?}, получено {actual:?}"
                )
            },
            Self::InvalidSizeField {
                offset,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "Неверное поле размера по смещению {offset:#x}: указано {expected} байт, доступно {actual}"
                )
            },
            Self::InvalidSizeFieldAlignment {
                offset,
                size,
                expected_alignment,
            } => {
                write!(
                    f,
                    "Неверное выравнивание по смещению {offset:#x}: размер {size} не кратен {expected_alignment}"
                )
            },
            Self::SequenceNumberMismatch { primary, secondary } => {
                write!(
                    f,
                    "Несовпадение sequence numbers: primary={primary}, secondary={secondary}"
                )
            },
            Self::UnallocatedCell { offset, size } => {
                write!(
                    f,
                    "Cell по смещению {offset:#x} не аллоцирована (size={size})"
                )
            },
            Self::UnsupportedClusteringFactor { expected, actual } => {
                write!(
                    f,
                    "Неподдерживаемый clustering factor: ожидалось {expected}, получено {actual}"
                )
            },
            Self::UnsupportedFileFormat { expected, actual } => {
                write!(
                    f,
                    "Неподдерживаемый формат файла: ожидалось {expected}, получено {actual}"
                )
            },
            Self::UnsupportedFileType { expected, actual } => {
                write!(
                    f,
                    "Неподдерживаемый тип файла: ожидалось {expected}, получено {actual}"
                )
            },
            Self::UnsupportedKeyValueDataType { offset, actual } => {
                write!(
                    f,
                    "Неподдерживаемый тип данных по смещению {offset:#x}: код {actual:#x}"
                )
            },
            Self::UnsupportedVersion { major, minor } => {
                write!(f, "Неподдерживаемая версия hive: {major}.{minor}")
            },
            #[cfg(feature = "write")]
            Self::OutOfSpace {
                required,
                available,
            } => {
                write!(
                    f,
                    "Недостаточно места: требуется {required} байт, доступно {available}"
                )
            },
            #[cfg(feature = "write")]
            Self::KeyNotFound => write!(f, "Ключ не найден"),
            #[cfg(feature = "write")]
            Self::ValueNotFound => write!(f, "Значение не найдено"),
        }
    }
}

// Реализуем std::error::Error для HiveError когда доступна std
#[cfg(feature = "std")]
impl std::error::Error for HiveError {}
