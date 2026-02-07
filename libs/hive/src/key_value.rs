//! Структура KeyValue - значение реестра
//!
//! KeyValue представляет значение, принадлежащее ключу реестра.
//! Каждое значение имеет:
//! - Имя (может быть пустым для значения по умолчанию)
//! - Тип данных (REG_SZ, REG_DWORD, etc.)
//! - Данные
//!
//! # Архитектура
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                   KeyValue (signature: "vk")                │
//! ├─────────────────────────────────────────────────────────────┤
//! │  signature: "vk"              (2 bytes)                     │
//! │  name_length                  (2 bytes)                     │
//! │  data_size                    (4 bytes)                     │
//! │    bit 31: data stored in data_offset field                 │
//! │  data_offset                  (4 bytes)                     │
//! │  data_type                    (4 bytes)                     │
//! │  flags                        (2 bytes)                     │
//! │  spare                        (2 bytes)                     │
//! │  value_name                   (variable, Latin1 or UTF-16)  │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Оптимизация малых данных
//!
//! Если данные помещаются в 4 байта (data_size <= 4 и бит 31 установлен),
//! они хранятся прямо в поле data_offset вместо отдельной cell.
//!
//! Источники:
//! - MSDN (NT6.1): KEY_VALUE_BASIC_INFORMATION, KEY_VALUE_FULL_INFORMATION
//! - ReactOS: docs/ref/reactos/sdk/lib/cmlib/cmdata.h (CM_KEY_VALUE)

use core::mem;
use core::ops::Range;

use bitflags::bitflags;
use zerocopy::FromBytes;
use zerocopy::Immutable;
use zerocopy::IntoBytes;
use zerocopy::KnownLayout;
use zerocopy::Ref;
use zerocopy::U16;
use zerocopy::U32;
use zerocopy::Unaligned;
use zerocopy::byteorder::LittleEndian;

use crate::big_data::BIG_DATA_SEGMENT_SIZE;
use crate::big_data::BigDataSlices;
use crate::error::HiveError;
use crate::error::Result;
use crate::hive::byte_subrange;
use crate::key_node::cell_range_from_data;
use crate::string::HiveString;

// =============================================================================
// Константы
// =============================================================================

/// Сигнатура KeyValue
const KEY_VALUE_SIGNATURE: &[u8; 2] = b"vk";

/// Бит в data_size, указывающий что данные хранятся в data_offset
const DATA_STORED_IN_DATA_OFFSET: u32 = 0x8000_0000;

// =============================================================================
// Типы данных значений
// =============================================================================

/// Типы данных значений реестра.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum KeyValueDataType {
    /// REG_NONE - нет определенного типа
    RegNone = 0x0000_0000,
    /// REG_SZ - null-terminated строка (UTF-16LE)
    RegSZ = 0x0000_0001,
    /// REG_EXPAND_SZ - строка с переменными окружения
    RegExpandSZ = 0x0000_0002,
    /// REG_BINARY - бинарные данные
    RegBinary = 0x0000_0003,
    /// REG_DWORD - 32-bit число (little-endian)
    RegDWord = 0x0000_0004,
    /// REG_DWORD_BIG_ENDIAN - 32-bit число (big-endian)
    RegDWordBigEndian = 0x0000_0005,
    /// REG_LINK - символическая ссылка
    RegLink = 0x0000_0006,
    /// REG_MULTI_SZ - массив null-terminated строк
    RegMultiSZ = 0x0000_0007,
    /// REG_RESOURCE_LIST - список ресурсов устройства
    RegResourceList = 0x0000_0008,
    /// REG_FULL_RESOURCE_DESCRIPTOR - описатель ресурсов
    RegFullResourceDescriptor = 0x0000_0009,
    /// REG_RESOURCE_REQUIREMENTS_LIST - требования к ресурсам
    RegResourceRequirementsList = 0x0000_000a,
    /// REG_QWORD - 64-bit число
    RegQWord = 0x0000_000b,
}

impl KeyValueDataType {
    /// Конвертация из u32
    pub fn from_u32(value: u32) -> Option<Self> {
        match value {
            0x0000_0000 => Some(Self::RegNone),
            0x0000_0001 => Some(Self::RegSZ),
            0x0000_0002 => Some(Self::RegExpandSZ),
            0x0000_0003 => Some(Self::RegBinary),
            0x0000_0004 => Some(Self::RegDWord),
            0x0000_0005 => Some(Self::RegDWordBigEndian),
            0x0000_0006 => Some(Self::RegLink),
            0x0000_0007 => Some(Self::RegMultiSZ),
            0x0000_0008 => Some(Self::RegResourceList),
            0x0000_0009 => Some(Self::RegFullResourceDescriptor),
            0x0000_000a => Some(Self::RegResourceRequirementsList),
            0x0000_000b => Some(Self::RegQWord),
            _ => None,
        }
    }
}

// =============================================================================
// Флаги KeyValue
// =============================================================================

bitflags! {
    /// Флаги значения реестра
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct KeyValueFlags: u16 {
        /// Имя в ASCII (Latin1) вместо UTF-16
        const VALUE_COMP_NAME = 0x0001;
    }
}

// =============================================================================
// On-Disk структуры
// =============================================================================

/// On-disk структура заголовка KeyValue
#[allow(dead_code)]
#[derive(FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned)]
#[repr(packed)]
struct KeyValueHeader {
    /// Сигнатура "vk"
    signature: [u8; 2],
    /// Длина имени значения
    name_length: U16<LittleEndian>,
    /// Размер данных (бит 31 = данные в data_offset)
    data_size: U32<LittleEndian>,
    /// Смещение данных или сами данные (если data_size & 0x80000000)
    data_offset: U32<LittleEndian>,
    /// Тип данных
    data_type: U32<LittleEndian>,
    /// Флаги
    flags: U16<LittleEndian>,
    /// Spare
    spare: U16<LittleEndian>,
}

/// Размер заголовка KeyValue
const KEY_VALUE_HEADER_SIZE: usize = mem::size_of::<KeyValueHeader>();

// =============================================================================
// KeyValueData
// =============================================================================

/// Zero-copy представление данных значения.
#[derive(Clone)]
pub enum KeyValueData<'h> {
    /// Малые данные (помещаются в одну cell)
    Small(&'h [u8]),
    /// Большие данные (требуют Big Data структуру)
    Big(BigDataSlices<'h>),
}

impl<'h> KeyValueData<'h> {
    /// Конвертирует данные в Vec<u8>
    #[cfg(feature = "alloc")]
    pub fn into_vec(self) -> Result<alloc::vec::Vec<u8>> {
        match self {
            Self::Small(data) => Ok(data.to_vec()),
            Self::Big(iter) => {
                let mut data = alloc::vec::Vec::new();
                for slice_result in iter {
                    let slice = slice_result?;
                    data.extend_from_slice(slice);
                }
                Ok(data)
            },
        }
    }
}

// =============================================================================
// KeyValue
// =============================================================================

/// Значение реестра.
#[derive(Clone)]
pub struct KeyValue<'h> {
    data: &'h [u8],
    base_block_size: usize,
    header_range: Range<usize>,
    data_range: Range<usize>,
}

impl<'h> KeyValue<'h> {
    /// Создает KeyValue из данных и диапазона cell
    pub(crate) fn from_data_and_range(
        data: &'h [u8],
        base_block_size: usize,
        cell_range: Range<usize>,
    ) -> Result<Self> {
        let header_range = byte_subrange(&cell_range, KEY_VALUE_HEADER_SIZE).ok_or_else(|| {
            HiveError::InvalidHeaderSize {
                offset: base_block_size + cell_range.start,
                expected: KEY_VALUE_HEADER_SIZE,
                actual: cell_range.len(),
            }
        })?;
        let data_range = header_range.end..cell_range.end;

        let key_value = Self {
            data,
            base_block_size,
            header_range,
            data_range,
        };
        key_value.validate_signature()?;

        Ok(key_value)
    }

    fn header(&self) -> Ref<&[u8], KeyValueHeader> {
        Ref::from_bytes(&self.data[self.header_range.clone()]).unwrap()
    }

    fn validate_signature(&self) -> Result<()> {
        let header = self.header();
        let signature = &header.signature;

        if signature == KEY_VALUE_SIGNATURE {
            Ok(())
        } else {
            Err(HiveError::InvalidTwoByteSignature {
                offset: self.base_block_size + self.header_range.start,
                expected: KEY_VALUE_SIGNATURE,
                actual: *signature,
            })
        }
    }

    /// Возвращает имя значения.
    ///
    /// Пустое имя означает значение по умолчанию (Default).
    pub fn name(&self) -> Result<HiveString<'h>> {
        let header = self.header();
        let flags = KeyValueFlags::from_bits_truncate(header.flags.get());
        let name_length = header.name_length.get() as usize;

        if name_length == 0 {
            // Значение по умолчанию
            return Ok(HiveString::Latin1(&[]));
        }

        let name_range = byte_subrange(&self.data_range, name_length).ok_or_else(|| {
            HiveError::InvalidSizeField {
                offset: self.base_block_size + self.data_range.start,
                expected: name_length,
                actual: self.data_range.len(),
            }
        })?;
        let name_bytes = &self.data[name_range];

        if flags.contains(KeyValueFlags::VALUE_COMP_NAME) {
            Ok(HiveString::Latin1(name_bytes))
        } else {
            Ok(HiveString::Utf16LE(name_bytes))
        }
    }

    /// Возвращает тип данных значения.
    pub fn data_type(&self) -> Result<KeyValueDataType> {
        let header = self.header();
        let data_type_code = header.data_type.get();

        KeyValueDataType::from_u32(data_type_code).ok_or_else(|| {
            HiveError::UnsupportedKeyValueDataType {
                offset: self.base_block_size + self.header_range.start + 12, // offset of data_type
                actual: data_type_code,
            }
        })
    }

    /// Возвращает размер данных в байтах.
    pub fn data_size(&self) -> u32 {
        let header = self.header();
        header.data_size.get() & !DATA_STORED_IN_DATA_OFFSET
    }

    /// Возвращает сырые данные значения.
    pub fn data(&self) -> Result<KeyValueData<'h>> {
        let header = self.header();

        let data_size_raw = header.data_size.get();
        let data_stored_in_offset = data_size_raw & DATA_STORED_IN_DATA_OFFSET != 0;
        let data_size = (data_size_raw & !DATA_STORED_IN_DATA_OFFSET) as usize;

        if data_stored_in_offset {
            // Данные хранятся прямо в поле data_offset
            if data_size > mem::size_of::<u32>() {
                return Err(HiveError::InvalidSizeField {
                    offset: self.base_block_size + self.header_range.start + 4, // offset of data_size
                    expected: mem::size_of::<u32>(),
                    actual: data_size,
                });
            }

            // Вычисляем смещение поля data_offset в данных
            let data_offset_field_offset = self.header_range.start + 8; // offset of data_offset in header
            let data_start = data_offset_field_offset;
            let data_end = data_start + data_size;

            Ok(KeyValueData::Small(&self.data[data_start..data_end]))
        } else if data_size <= BIG_DATA_SEGMENT_SIZE {
            // Данные в отдельной cell
            let cell_range =
                cell_range_from_data(self.data, self.base_block_size, header.data_offset.get())?;

            if cell_range.len() < data_size {
                return Err(HiveError::InvalidDataSize {
                    offset: self.base_block_size + cell_range.start,
                    expected: data_size,
                    actual: cell_range.len(),
                });
            }

            let data_start = cell_range.start;
            let data_end = data_start + data_size;

            Ok(KeyValueData::Small(&self.data[data_start..data_end]))
        } else {
            // Big Data
            let cell_range =
                cell_range_from_data(self.data, self.base_block_size, header.data_offset.get())?;

            let iter = BigDataSlices::new(
                self.data,
                self.base_block_size,
                data_size as u32,
                self.base_block_size + self.header_range.start + 4, // offset of data_size
                cell_range,
            )?;

            Ok(KeyValueData::Big(iter))
        }
    }

    /// Возвращает данные как DWORD (u32).
    ///
    /// Работает только для REG_DWORD и REG_DWORD_BIG_ENDIAN.
    pub fn dword_data(&self) -> Result<u32> {
        if let KeyValueData::Small(data) = self.data()? {
            if data.len() != mem::size_of::<u32>() {
                return Err(HiveError::InvalidDataSize {
                    offset: self.base_block_size + self.header_range.start + 4,
                    expected: mem::size_of::<u32>(),
                    actual: data.len(),
                });
            }

            match self.data_type()? {
                KeyValueDataType::RegDWord => Ok(u32::from_le_bytes(data.try_into().unwrap())),
                KeyValueDataType::RegDWordBigEndian => {
                    Ok(u32::from_be_bytes(data.try_into().unwrap()))
                },
                data_type => Err(HiveError::InvalidKeyValueDataType {
                    expected: &[
                        KeyValueDataType::RegDWord,
                        KeyValueDataType::RegDWordBigEndian,
                    ],
                    actual: data_type,
                }),
            }
        } else {
            Err(HiveError::InvalidDataSize {
                offset: self.base_block_size + self.header_range.start + 4,
                expected: mem::size_of::<u32>(),
                actual: self.data_size() as usize,
            })
        }
    }

    /// Возвращает данные как QWORD (u64).
    ///
    /// Работает только для REG_QWORD.
    pub fn qword_data(&self) -> Result<u64> {
        if let KeyValueData::Small(data) = self.data()? {
            if data.len() != mem::size_of::<u64>() {
                return Err(HiveError::InvalidDataSize {
                    offset: self.base_block_size + self.header_range.start + 4,
                    expected: mem::size_of::<u64>(),
                    actual: data.len(),
                });
            }

            match self.data_type()? {
                KeyValueDataType::RegQWord => Ok(u64::from_le_bytes(data.try_into().unwrap())),
                data_type => Err(HiveError::InvalidKeyValueDataType {
                    expected: &[KeyValueDataType::RegQWord],
                    actual: data_type,
                }),
            }
        } else {
            Err(HiveError::InvalidDataSize {
                offset: self.base_block_size + self.header_range.start + 4,
                expected: mem::size_of::<u64>(),
                actual: self.data_size() as usize,
            })
        }
    }

    /// Возвращает данные как строку.
    ///
    /// Работает для REG_SZ и REG_EXPAND_SZ.
    #[cfg(feature = "alloc")]
    pub fn string_data(&self) -> Result<alloc::string::String> {
        match self.data_type()? {
            KeyValueDataType::RegSZ | KeyValueDataType::RegExpandSZ => {},
            data_type => {
                return Err(HiveError::InvalidKeyValueDataType {
                    expected: &[KeyValueDataType::RegSZ, KeyValueDataType::RegExpandSZ],
                    actual: data_type,
                });
            },
        }

        let data = self.data()?;
        let bytes = match data {
            KeyValueData::Small(b) => b,
            KeyValueData::Big(_) => {
                // Для больших данных нужна аллокация
                let vec = data.into_vec()?;
                return utf16le_to_string(&vec);
            },
        };

        utf16le_to_string(bytes)
    }
}

/// Конвертирует UTF-16LE байты в String
#[cfg(feature = "alloc")]
fn utf16le_to_string(bytes: &[u8]) -> Result<alloc::string::String> {
    use alloc::string::String;

    let mut string = String::new();

    for chunk in bytes.chunks_exact(2) {
        let code_unit = u16::from_le_bytes([chunk[0], chunk[1]]);

        // Останавливаемся на NUL
        if code_unit == 0 {
            break;
        }

        // Декодируем UTF-16
        if let Some(c) = char::from_u32(code_unit as u32) {
            string.push(c);
        } else {
            // Surrogate pair - упрощенная обработка
            string.push(char::REPLACEMENT_CHARACTER);
        }
    }

    Ok(string)
}
