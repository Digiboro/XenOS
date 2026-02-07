//! Структура KeyNode - ключ реестра
//!
//! KeyNode представляет ключ в реестре Windows. Каждый ключ имеет:
//! - Имя
//! - Опционально: подключи (subkeys)
//! - Опционально: значения (values)
//! - Метаданные (timestamp, флаги, security descriptor)
//!
//! # Архитектура
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    KeyNode (signature: "nk")                │
//! ├─────────────────────────────────────────────────────────────┤
//! │  signature: "nk"              (2 bytes)                     │
//! │  flags                        (2 bytes)                     │
//! │  timestamp                    (8 bytes, FILETIME)           │
//! │  spare                        (4 bytes)                     │
//! │  parent_offset                (4 bytes)                     │
//! │  subkey_count                 (4 bytes)                     │
//! │  volatile_subkey_count        (4 bytes)                     │
//! │  subkeys_list_offset          (4 bytes)                     │
//! │  volatile_subkeys_list_offset (4 bytes)                     │
//! │  values_count                 (4 bytes)                     │
//! │  values_list_offset           (4 bytes)                     │
//! │  security_offset              (4 bytes)                     │
//! │  class_name_offset            (4 bytes)                     │
//! │  max_subkey_name_length       (4 bytes)                     │
//! │  max_subkey_class_name_length (4 bytes)                     │
//! │  max_value_name_length        (4 bytes)                     │
//! │  max_value_data_length        (4 bytes)                     │
//! │  work_var                     (4 bytes)                     │
//! │  key_name_length              (2 bytes)                     │
//! │  class_name_length            (2 bytes)                     │
//! │  key_name                     (variable, Latin1 or UTF-16)  │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! Источники:
//! - MSDN (NT6.1): KEY_NODE_INFORMATION
//! - ReactOS: docs/ref/reactos/sdk/lib/cmlib/cmdata.h (CM_KEY_NODE)

use core::ops::Range;

use bitflags::bitflags;
use zerocopy::FromBytes;
use zerocopy::Immutable;
use zerocopy::IntoBytes;
use zerocopy::KnownLayout;
use zerocopy::Ref;
use zerocopy::U16;
use zerocopy::U32;
use zerocopy::U64;
use zerocopy::Unaligned;
use zerocopy::byteorder::LittleEndian;

use crate::error::HiveError;
use crate::error::Result;
use crate::hive::Hive;
use crate::hive::byte_subrange;
use crate::key_value::KeyValue;
use crate::string::HiveString;
use crate::subkeys::SubKeyNodes;

// =============================================================================
// Константы
// =============================================================================

/// Сигнатура KeyNode
const KEY_NODE_SIGNATURE: &[u8; 2] = b"nk";

// =============================================================================
// Флаги KeyNode
// =============================================================================

bitflags! {
    /// Флаги ключа реестра
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct KeyNodeFlags: u16 {
        /// Volatile ключ (не сохраняется на диск)
        const KEY_IS_VOLATILE = 0x0001;
        /// Точка монтирования другого hive
        const KEY_HIVE_EXIT = 0x0002;
        /// Корневой ключ hive
        const KEY_HIVE_ENTRY = 0x0004;
        /// Ключ нельзя удалить
        const KEY_NO_DELETE = 0x0008;
        /// Символическая ссылка
        const KEY_SYM_LINK = 0x0010;
        /// Имя в ASCII (Latin1) вместо UTF-16
        const KEY_COMP_NAME = 0x0020;
        /// Predefined handle
        const KEY_PREDEF_HANDLE = 0x0040;
        /// Ключ был виртуализирован
        const KEY_VIRT_MIRRORED = 0x0080;
        /// Виртуальный ключ
        const KEY_VIRT_TARGET = 0x0100;
        /// Часть virtual store path
        const KEY_VIRTUAL_STORE = 0x0200;
    }
}

// =============================================================================
// On-Disk структуры
// =============================================================================

/// On-disk структура заголовка KeyNode
#[allow(dead_code)]
#[derive(FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned)]
#[repr(packed)]
pub(crate) struct KeyNodeHeader {
    /// Сигнатура "nk"
    pub signature: [u8; 2],
    /// Флаги
    pub flags: U16<LittleEndian>,
    /// Timestamp последней записи
    pub timestamp: U64<LittleEndian>,
    /// Spare (не используется)
    pub spare: U32<LittleEndian>,
    /// Смещение родительского ключа
    pub parent: U32<LittleEndian>,
    /// Количество подключей
    pub subkey_count: U32<LittleEndian>,
    /// Количество volatile подключей
    pub volatile_subkey_count: U32<LittleEndian>,
    /// Смещение списка подключей
    pub subkeys_list_offset: U32<LittleEndian>,
    /// Смещение списка volatile подключей
    pub volatile_subkeys_list_offset: U32<LittleEndian>,
    /// Количество значений
    pub values_count: U32<LittleEndian>,
    /// Смещение списка значений
    pub values_list_offset: U32<LittleEndian>,
    /// Смещение security descriptor
    pub security_offset: U32<LittleEndian>,
    /// Смещение class name
    pub class_name_offset: U32<LittleEndian>,
    /// Максимальная длина имени подключа
    pub max_subkey_name: U32<LittleEndian>,
    /// Максимальная длина class name подключа
    pub max_subkey_class_name: U32<LittleEndian>,
    /// Максимальная длина имени значения
    pub max_value_name: U32<LittleEndian>,
    /// Максимальная длина данных значения
    pub max_value_data: U32<LittleEndian>,
    /// Work variable (не используется)
    pub work_var: U32<LittleEndian>,
    /// Длина имени ключа
    pub key_name_length: U16<LittleEndian>,
    /// Длина class name
    pub class_name_length: U16<LittleEndian>,
}

/// Размер заголовка KeyNode
pub(crate) const KEY_NODE_HEADER_SIZE: usize = core::mem::size_of::<KeyNodeHeader>();

// =============================================================================
// KeyNode
// =============================================================================

/// Ключ реестра.
///
/// Предоставляет доступ к имени, подключам и значениям.
#[derive(Clone)]
pub struct KeyNode<'h> {
    /// Данные hive
    data: &'h [u8],
    /// Смещение базового блока (для вычисления абсолютных смещений)
    base_block_size: usize,
    /// Диапазон заголовка в данных
    header_range: Range<usize>,
    /// Диапазон данных после заголовка
    data_range: Range<usize>,
}

impl<'h> KeyNode<'h> {
    /// Создает KeyNode из диапазона cell
    pub(crate) fn from_cell_range(hive: &Hive<'h>, cell_range: Range<usize>) -> Result<Self> {
        let header_range = byte_subrange(&cell_range, KEY_NODE_HEADER_SIZE).ok_or_else(|| {
            HiveError::InvalidHeaderSize {
                offset: hive.offset_of_data_offset(cell_range.start),
                expected: KEY_NODE_HEADER_SIZE,
                actual: cell_range.len(),
            }
        })?;
        let data_range = header_range.end..cell_range.end;

        let key_node = Self {
            data: hive.data,
            base_block_size: crate::hive::HIVE_BASE_BLOCK_SIZE,
            header_range,
            data_range,
        };
        key_node.validate_signature()?;

        Ok(key_node)
    }

    /// Создает KeyNode из смещения в subkeys list
    pub(crate) fn from_subkey_offset(hive: &Hive<'h>, offset: u32) -> Result<Self> {
        let cell_range = hive.cell_range_from_data_offset(offset)?;
        Self::from_cell_range(hive, cell_range)
    }

    /// Создает KeyNode напрямую из данных и смещения
    pub(crate) fn from_data_and_offset(
        data: &'h [u8],
        base_block_size: usize,
        offset: u32,
    ) -> Result<Self> {
        let cell_range = cell_range_from_data(data, base_block_size, offset)?;
        let header_range = byte_subrange(&cell_range, KEY_NODE_HEADER_SIZE).ok_or_else(|| {
            HiveError::InvalidHeaderSize {
                offset: base_block_size + cell_range.start,
                expected: KEY_NODE_HEADER_SIZE,
                actual: cell_range.len(),
            }
        })?;
        let data_range = header_range.end..cell_range.end;

        let key_node = Self {
            data,
            base_block_size,
            header_range,
            data_range,
        };
        key_node.validate_signature()?;

        Ok(key_node)
    }

    fn header(&self) -> Ref<&[u8], KeyNodeHeader> {
        Ref::from_bytes(&self.data[self.header_range.clone()]).unwrap()
    }

    fn validate_signature(&self) -> Result<()> {
        let header = self.header();
        let signature = &header.signature;

        if signature == KEY_NODE_SIGNATURE {
            Ok(())
        } else {
            Err(HiveError::InvalidTwoByteSignature {
                offset: self.base_block_size + self.header_range.start,
                expected: KEY_NODE_SIGNATURE,
                actual: *signature,
            })
        }
    }

    /// Возвращает имя ключа.
    pub fn name(&self) -> Result<HiveString<'h>> {
        let header = self.header();
        let flags = KeyNodeFlags::from_bits_truncate(header.flags.get());
        let key_name_length = header.key_name_length.get() as usize;

        let name_range = byte_subrange(&self.data_range, key_name_length).ok_or_else(|| {
            HiveError::InvalidSizeField {
                offset: self.base_block_size + self.data_range.start,
                expected: key_name_length,
                actual: self.data_range.len(),
            }
        })?;
        let name_bytes = &self.data[name_range];

        if flags.contains(KeyNodeFlags::KEY_COMP_NAME) {
            Ok(HiveString::Latin1(name_bytes))
        } else {
            Ok(HiveString::Utf16LE(name_bytes))
        }
    }

    /// Возвращает флаги ключа.
    pub fn flags(&self) -> KeyNodeFlags {
        let header = self.header();
        KeyNodeFlags::from_bits_truncate(header.flags.get())
    }

    /// Возвращает timestamp последней записи (FILETIME).
    pub fn timestamp(&self) -> u64 {
        self.header().timestamp.get()
    }

    /// Возвращает class name ключа (если есть).
    pub fn class_name(&self) -> Option<Result<HiveString<'h>>> {
        let header = self.header();
        let class_name_offset = header.class_name_offset.get();

        if class_name_offset == u32::MAX {
            return None;
        }

        let class_name_length = header.class_name_length.get() as usize;
        let cell_range = iter_try!(cell_range_from_data(
            self.data,
            self.base_block_size,
            class_name_offset
        ));

        let class_name_range = iter_try!(byte_subrange(&cell_range, class_name_length).ok_or_else(
            || HiveError::InvalidSizeField {
                offset: self.base_block_size + cell_range.start,
                expected: class_name_length,
                actual: cell_range.len(),
            }
        ));

        let class_name_bytes = &self.data[class_name_range];
        // Class name всегда UTF-16LE
        Some(Ok(HiveString::Utf16LE(class_name_bytes)))
    }

    /// Ищет подключ по имени.
    ///
    /// Подключи отсортированы, но без alloc бинарный поиск затруднен,
    /// поэтому используется линейный поиск.
    pub fn subkey(&self, name: &str) -> Option<Result<KeyNode<'h>>> {
        let subkeys = iter_try!(self.subkeys()?);

        for subkey_result in subkeys {
            let subkey = iter_try!(subkey_result);
            let subkey_name = iter_try!(subkey.name());
            if subkey_name == name {
                return Some(Ok(subkey));
            }
        }

        None
    }

    /// Возвращает итератор по подключам.
    pub fn subkeys(&self) -> Option<Result<SubKeyNodes<'h>>> {
        let header = self.header();
        let subkeys_list_offset = header.subkeys_list_offset.get();

        if subkeys_list_offset == u32::MAX {
            return None;
        }

        let cell_range = iter_try!(cell_range_from_data(
            self.data,
            self.base_block_size,
            subkeys_list_offset
        ));
        Some(SubKeyNodes::new(
            self.data,
            self.base_block_size,
            cell_range,
        ))
    }

    /// Навигация по пути к подключу.
    ///
    /// Путь состоит из компонентов, разделенных обратным слешем.
    /// Пустые компоненты игнорируются.
    pub fn subpath(&self, path: &str) -> Option<Result<KeyNode<'h>>> {
        let mut current = self.clone();

        for component in path.split('\\') {
            if component.is_empty() {
                continue;
            }
            current = iter_try!(current.subkey(component)?);
        }

        Some(Ok(current))
    }

    /// Ищет значение по имени.
    pub fn value(&self, name: &str) -> Option<Result<KeyValue<'h>>> {
        let values = iter_try!(self.values()?);

        // Значения не отсортированы, линейный поиск
        for value_result in values {
            let value = iter_try!(value_result);
            let value_name = iter_try!(value.name());
            if value_name == name {
                return Some(Ok(value));
            }
        }

        None
    }

    /// Возвращает итератор по значениям.
    pub fn values(&self) -> Option<Result<KeyValues<'h>>> {
        let header = self.header();
        let values_list_offset = header.values_list_offset.get();

        if values_list_offset == u32::MAX {
            return None;
        }

        let cell_range = iter_try!(cell_range_from_data(
            self.data,
            self.base_block_size,
            values_list_offset
        ));
        let count = header.values_count.get();
        let count_field_offset = self.base_block_size + self.header_range.start + 64; // offset of values_count

        Some(KeyValues::new(
            self.data,
            self.base_block_size,
            count,
            count_field_offset,
            cell_range,
        ))
    }
}

// =============================================================================
// Вспомогательные функции
// =============================================================================

use core::mem;

use crate::hive::CellHeader;

/// Получает диапазон cell из данных по смещению.
pub(crate) fn cell_range_from_data(
    data: &[u8],
    base_block_size: usize,
    data_offset: u32,
) -> Result<Range<usize>> {
    assert!(data_offset != u32::MAX);

    let data_offset = data_offset as usize;

    // Получаем заголовок cell
    let remaining_range = data_offset..data.len();
    let header_range =
        byte_subrange(&remaining_range, mem::size_of::<CellHeader>()).ok_or_else(|| {
            HiveError::InvalidHeaderSize {
                offset: base_block_size + data_offset,
                expected: mem::size_of::<CellHeader>(),
                actual: remaining_range.len(),
            }
        })?;
    let cell_data_offset = header_range.end;

    // Читаем заголовок
    let header = Ref::<&[u8], CellHeader>::from_bytes(&data[header_range]).unwrap();
    let cell_size = header.size.get();

    // Cell с size > 0 не аллоцирована
    if cell_size > 0 {
        return Err(HiveError::UnallocatedCell {
            offset: base_block_size + data_offset,
            size: cell_size,
        });
    }
    let cell_size = cell_size.unsigned_abs() as usize;

    // Размер cell должен быть кратен 8 байтам
    let expected_alignment = 8;
    if cell_size % expected_alignment != 0 {
        return Err(HiveError::InvalidSizeFieldAlignment {
            offset: base_block_size + data_offset,
            size: cell_size,
            expected_alignment,
        });
    }

    // Получаем диапазон данных cell (без заголовка)
    let remaining_range = cell_data_offset..data.len();
    let cell_data_size = cell_size - mem::size_of::<CellHeader>();
    let cell_data_range = byte_subrange(&remaining_range, cell_data_size).ok_or_else(|| {
        HiveError::InvalidSizeField {
            offset: base_block_size + data_offset,
            expected: cell_data_size,
            actual: remaining_range.len(),
        }
    })?;

    Ok(cell_data_range)
}

// =============================================================================
// KeyValues iterator
// =============================================================================

/// On-disk структура элемента списка значений
#[derive(FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned)]
#[repr(packed)]
struct KeyValuesListItem {
    key_value_offset: U32<LittleEndian>,
}

/// Итератор по значениям ключа.
#[derive(Clone)]
pub struct KeyValues<'h> {
    data: &'h [u8],
    base_block_size: usize,
    items_range: Range<usize>,
}

impl<'h> KeyValues<'h> {
    pub(crate) fn new(
        data: &'h [u8],
        base_block_size: usize,
        count: u32,
        count_field_offset: usize,
        cell_range: Range<usize>,
    ) -> Result<Self> {
        let byte_count = count as usize * core::mem::size_of::<KeyValuesListItem>();

        let items_range =
            byte_subrange(&cell_range, byte_count).ok_or_else(|| HiveError::InvalidSizeField {
                offset: count_field_offset,
                expected: byte_count,
                actual: cell_range.len(),
            })?;

        Ok(Self {
            data,
            base_block_size,
            items_range,
        })
    }
}

impl<'h> Iterator for KeyValues<'h> {
    type Item = Result<KeyValue<'h>>;

    fn next(&mut self) -> Option<Self::Item> {
        let item_size = core::mem::size_of::<KeyValuesListItem>();
        let item_range = byte_subrange(&self.items_range, item_size)?;
        self.items_range.start += item_size;

        let item = Ref::<&[u8], KeyValuesListItem>::from_bytes(&self.data[item_range]).unwrap();
        let offset = item.key_value_offset.get();

        let cell_range = iter_try!(cell_range_from_data(
            self.data,
            self.base_block_size,
            offset
        ));
        let value = iter_try!(KeyValue::from_data_and_range(
            self.data,
            self.base_block_size,
            cell_range
        ));

        Some(Ok(value))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let size = self.items_range.len() / core::mem::size_of::<KeyValuesListItem>();
        (size, Some(size))
    }
}

impl<'h> ExactSizeIterator for KeyValues<'h> {}
