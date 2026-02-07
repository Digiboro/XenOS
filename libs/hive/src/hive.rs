//! Основная структура Hive
//!
//! Hive представляет весь файл реестра и является точкой входа для работы с данными.
//!
//! # Архитектура
//!
//! ```text
//! ┌────────────────────────────────────────────────────────────┐
//! │                    HiveBaseBlock (4096 bytes)              │
//! ├────────────────────────────────────────────────────────────┤
//! │  signature: "regf"         (4 bytes)                       │
//! │  primary_sequence_number   (4 bytes)                       │
//! │  secondary_sequence_number (4 bytes)                       │
//! │  timestamp                 (8 bytes)                       │
//! │  major_version             (4 bytes) = 1                   │
//! │  minor_version             (4 bytes) = 3..6                │
//! │  file_type                 (4 bytes) = 0 (Primary)         │
//! │  file_format               (4 bytes) = 1 (Memory)          │
//! │  root_cell_offset          (4 bytes)                       │
//! │  data_size                 (4 bytes)                       │
//! │  clustering_factor         (4 bytes) = 1                   │
//! │  file_name                 (64 bytes, UTF-16LE)            │
//! │  ...padding...                                             │
//! │  checksum                  (4 bytes, XOR-32)               │
//! │  ...padding...                                             │
//! └────────────────────────────────────────────────────────────┘
//! ```
//!
//! Источники:
//! - MSDN (NT6.1): Registry Hive Format
//! - ReactOS: docs/ref/reactos/sdk/lib/cmlib/cmhive.h

use core::mem;
use core::ops::Range;

use zerocopy::FromBytes;
use zerocopy::I32;
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
use crate::key_node::KeyNode;

// =============================================================================
// Константы
// =============================================================================

/// Размер базового блока hive
pub const HIVE_BASE_BLOCK_SIZE: usize = 4096;

/// Сигнатура hive файла
const HIVE_SIGNATURE: &[u8; 4] = b"regf";

// =============================================================================
// On-Disk структуры
// =============================================================================

/// Заголовок cell
#[derive(FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned)]
#[repr(packed)]
pub struct CellHeader {
    /// Размер cell (отрицательный = allocated, положительный = free)
    pub size: I32<LittleEndian>,
}

/// Известные minor версии hive
///
/// Используйте [`HiveMinorVersion::from_u32`] для проверки версии.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[repr(u32)]
pub enum HiveMinorVersion {
    /// Windows NT 3.1 Beta
    WindowsNT3_1Beta = 0,
    /// Windows NT 3.1
    WindowsNT3_1 = 1,
    /// Windows NT 3.5
    WindowsNT3_5 = 2,
    /// Windows NT 4.0 (минимально поддерживаемая)
    WindowsNT4 = 3,
    /// Windows XP Beta
    WindowsXPBeta = 4,
    /// Windows XP / 2003
    WindowsXP = 5,
    /// Windows Vista и новее
    WindowsVista = 6,
}

impl HiveMinorVersion {
    /// Конвертация из u32, возвращает None для неизвестных версий
    pub fn from_u32(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::WindowsNT3_1Beta),
            1 => Some(Self::WindowsNT3_1),
            2 => Some(Self::WindowsNT3_5),
            3 => Some(Self::WindowsNT4),
            4 => Some(Self::WindowsXPBeta),
            5 => Some(Self::WindowsXP),
            6 => Some(Self::WindowsVista),
            _ => None,
        }
    }
}

/// Типы hive файлов
#[allow(dead_code)]
#[repr(u32)]
enum HiveFileType {
    /// Primary hive (основной файл)
    Primary = 0,
    /// Log file
    Log = 1,
    /// External
    External = 2,
}

/// Форматы hive файлов
#[repr(u32)]
enum HiveFileFormat {
    /// Memory mapped формат
    Memory = 1,
}

/// On-disk структура базового блока hive
#[allow(dead_code)]
#[derive(FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned)]
#[repr(packed)]
struct HiveBaseBlock {
    /// Сигнатура "regf"
    signature: [u8; 4],
    /// Primary sequence number (инкрементируется при каждой записи)
    primary_sequence_number: U32<LittleEndian>,
    /// Secondary sequence number (должен совпадать с primary после успешной записи)
    secondary_sequence_number: U32<LittleEndian>,
    /// Timestamp последней записи (FILETIME)
    timestamp: U64<LittleEndian>,
    /// Major версия (всегда 1)
    major_version: U32<LittleEndian>,
    /// Minor версия (см. HiveMinorVersion)
    minor_version: U32<LittleEndian>,
    /// Тип файла (см. HiveFileType)
    file_type: U32<LittleEndian>,
    /// Формат файла (см. HiveFileFormat)
    file_format: U32<LittleEndian>,
    /// Смещение корневой cell относительно начала данных
    root_cell_offset: U32<LittleEndian>,
    /// Размер данных (без базового блока)
    data_size: U32<LittleEndian>,
    /// Clustering factor (всегда 1)
    clustering_factor: U32<LittleEndian>,
    /// Имя файла (UTF-16LE, 32 символа)
    file_name: [U16<LittleEndian>; 32],
    /// Padding до checksum
    padding_1: [u8; 396],
    /// Контрольная сумма (XOR-32 первых 508 байт)
    checksum: U32<LittleEndian>,
    /// Padding до конца блока
    padding_2: [u8; 3576],
    /// Тип загрузки (Boot type)
    boot_type: U32<LittleEndian>,
    /// Флаг восстановления (Boot recover)
    boot_recover: U32<LittleEndian>,
}

// =============================================================================
// Основная структура Hive
// =============================================================================

/// Корневая структура, описывающая registry hive.
///
/// `Hive` работает с любым byte slice, реализующим необходимые трейты,
/// что позволяет использовать его как с обычными слайсами (`&[u8]`),
/// так и с мутабельными (`&mut [u8]`).
pub struct Hive<'h> {
    /// Ссылка на базовый блок
    base_block: Ref<&'h [u8], HiveBaseBlock>,
    /// Данные hive (после базового блока)
    pub(crate) data: &'h [u8],
}

impl<'h> Hive<'h> {
    /// Создает новый `Hive` из byte slice.
    ///
    /// Выполняет базовую валидацию и отклоняет невалидные hive файлы.
    ///
    /// Используйте [`Hive::without_validation`] если нужно работать с
    /// частично поврежденными hive файлами.
    pub fn new(bytes: &'h [u8]) -> Result<Self> {
        let hive = Self::without_validation(bytes)?;
        hive.validate()?;
        Ok(hive)
    }

    /// Создает новый `Hive` без валидации заголовка.
    ///
    /// Полезно для работы с hive файлами, которые не были корректно
    /// закрыты (например, после hibernation с несовпадающими sequence numbers).
    ///
    /// Валидацию можно выполнить позже через [`Hive::validate`].
    pub fn without_validation(bytes: &'h [u8]) -> Result<Self> {
        let length = bytes.len();
        let (base_block, data) =
            Ref::from_prefix(bytes).map_err(|_| HiveError::InvalidHeaderSize {
                offset: 0,
                expected: mem::size_of::<HiveBaseBlock>(),
                actual: length,
            })?;

        Ok(Self { base_block, data })
    }

    /// Возвращает диапазон байт cell по её смещению в данных.
    ///
    /// Cell size хранится в начале cell:
    /// - Отрицательный размер = cell аллоцирована
    /// - Положительный размер = cell свободна
    pub(crate) fn cell_range_from_data_offset(&self, data_offset: u32) -> Result<Range<usize>> {
        // Только валидные смещения
        assert!(data_offset != u32::MAX);

        let data_offset = data_offset as usize;

        // Получаем заголовок cell
        let remaining_range = data_offset..self.data.len();
        let header_range = byte_subrange(&remaining_range, mem::size_of::<CellHeader>())
            .ok_or_else(|| HiveError::InvalidHeaderSize {
                offset: self.offset_of_data_offset(data_offset),
                expected: mem::size_of::<CellHeader>(),
                actual: remaining_range.len(),
            })?;
        let cell_data_offset = header_range.end;

        // Читаем заголовок
        let header = Ref::<&[u8], CellHeader>::from_bytes(&self.data[header_range]).unwrap();
        let cell_size = header.size.get();

        // Cell с size > 0 не аллоцирована
        if cell_size > 0 {
            return Err(HiveError::UnallocatedCell {
                offset: self.offset_of_data_offset(data_offset),
                size: cell_size,
            });
        }
        let cell_size = cell_size.unsigned_abs() as usize;

        // Размер cell должен быть кратен 8 байтам
        let expected_alignment = 8;
        if cell_size % expected_alignment != 0 {
            return Err(HiveError::InvalidSizeFieldAlignment {
                offset: self.offset_of_field(&header.size),
                size: cell_size,
                expected_alignment,
            });
        }

        // Получаем диапазон данных cell (без заголовка)
        let remaining_range = cell_data_offset..self.data.len();
        let cell_data_size = cell_size - mem::size_of::<CellHeader>();
        let cell_data_range = byte_subrange(&remaining_range, cell_data_size).ok_or_else(|| {
            HiveError::InvalidSizeField {
                offset: self.offset_of_field(&header.size),
                expected: cell_data_size,
                actual: remaining_range.len(),
            }
        })?;

        Ok(cell_data_range)
    }

    /// Вычисляет абсолютное смещение поля от начала hive файла.
    ///
    /// Используется для отчетов об ошибках.
    pub(crate) fn offset_of_field<T>(&self, field: &T) -> usize {
        let field_address = field as *const T as usize;
        let base_address = Ref::bytes(&self.base_block).as_ptr() as usize;

        assert!(field_address > base_address);
        field_address - base_address
    }

    /// Вычисляет абсолютное смещение data offset от начала hive файла.
    pub(crate) fn offset_of_data_offset(&self, data_offset: usize) -> usize {
        data_offset + mem::size_of::<HiveBaseBlock>()
    }

    /// Возвращает major версию hive.
    ///
    /// Единственное известное значение - 1.
    pub fn major_version(&self) -> u32 {
        self.base_block.major_version.get()
    }

    /// Возвращает minor версию hive.
    ///
    /// Используйте [`HiveMinorVersion::from_u32`] для проверки известных версий.
    pub fn minor_version(&self) -> u32 {
        self.base_block.minor_version.get()
    }

    /// Возвращает корневой [`KeyNode`] hive.
    pub fn root_key_node(&self) -> Result<KeyNode<'h>> {
        let root_cell_offset = self.base_block.root_cell_offset.get();
        let cell_range = self.cell_range_from_data_offset(root_cell_offset)?;
        KeyNode::from_cell_range(self, cell_range)
    }

    /// Выполняет валидацию заголовка hive.
    ///
    /// Если hive был открыт через [`Hive::new`], валидация уже выполнена.
    /// Эта функция нужна только для hive, открытых через [`Hive::without_validation`].
    pub fn validate(&self) -> Result<()> {
        self.validate_signature()?;
        self.validate_sequence_numbers()?;
        self.validate_version()?;
        self.validate_file_type()?;
        self.validate_file_format()?;
        self.validate_data_size()?;
        self.validate_clustering_factor()?;
        self.validate_checksum()?;
        Ok(())
    }

    fn validate_checksum(&self) -> Result<()> {
        // Вычисляем XOR-32 всех байт до поля checksum
        let checksum_offset = 508; // offset_of!(HiveBaseBlock, checksum)

        let mut calculated_checksum = 0u32;
        for dword_bytes in
            Ref::bytes(&self.base_block)[..checksum_offset].chunks(mem::size_of::<u32>())
        {
            let dword = u32::from_le_bytes(dword_bytes.try_into().unwrap());
            calculated_checksum ^= dword;
        }

        // Специальные случаи
        if calculated_checksum == 0 {
            calculated_checksum = 1;
        } else if calculated_checksum == u32::MAX {
            calculated_checksum = u32::MAX - 1;
        }

        let checksum = self.base_block.checksum.get();
        if checksum == calculated_checksum {
            Ok(())
        } else {
            Err(HiveError::InvalidChecksum {
                expected: checksum,
                actual: calculated_checksum,
            })
        }
    }

    fn validate_clustering_factor(&self) -> Result<()> {
        let clustering_factor = self.base_block.clustering_factor.get();
        let expected = 1;

        if clustering_factor == expected {
            Ok(())
        } else {
            Err(HiveError::UnsupportedClusteringFactor {
                expected,
                actual: clustering_factor,
            })
        }
    }

    fn validate_data_size(&self) -> Result<()> {
        let data_size = self.base_block.data_size.get() as usize;
        let expected_alignment = 4096;

        // Размер данных должен быть кратен 4096 байтам
        if data_size % expected_alignment != 0 {
            return Err(HiveError::InvalidSizeFieldAlignment {
                offset: self.offset_of_field(&self.base_block.data_size),
                size: data_size,
                expected_alignment,
            });
        }

        // Размер не должен превышать доступные данные
        if data_size > self.data.len() {
            return Err(HiveError::InvalidSizeField {
                offset: self.offset_of_field(&self.base_block.data_size),
                expected: data_size,
                actual: self.data.len(),
            });
        }

        Ok(())
    }

    fn validate_file_format(&self) -> Result<()> {
        let file_format = self.base_block.file_format.get();
        let expected = HiveFileFormat::Memory as u32;

        if file_format == expected {
            Ok(())
        } else {
            Err(HiveError::UnsupportedFileFormat {
                expected,
                actual: file_format,
            })
        }
    }

    fn validate_file_type(&self) -> Result<()> {
        let file_type = self.base_block.file_type.get();
        let expected = HiveFileType::Primary as u32;

        if file_type == expected {
            Ok(())
        } else {
            Err(HiveError::UnsupportedFileType {
                expected,
                actual: file_type,
            })
        }
    }

    fn validate_sequence_numbers(&self) -> Result<()> {
        let primary = self.base_block.primary_sequence_number.get();
        let secondary = self.base_block.secondary_sequence_number.get();

        if primary == secondary {
            Ok(())
        } else {
            Err(HiveError::SequenceNumberMismatch { primary, secondary })
        }
    }

    fn validate_signature(&self) -> Result<()> {
        let signature = &self.base_block.signature;

        if signature == HIVE_SIGNATURE {
            Ok(())
        } else {
            Err(HiveError::InvalidFourByteSignature {
                offset: self.offset_of_field(signature),
                expected: HIVE_SIGNATURE,
                actual: *signature,
            })
        }
    }

    fn validate_version(&self) -> Result<()> {
        let major = self.major_version();
        let minor = self.minor_version();

        // Поддерживаем версии начиная с NT4 (minor >= 3)
        if major == 1 && minor >= HiveMinorVersion::WindowsNT4 as u32 {
            Ok(())
        } else {
            Err(HiveError::UnsupportedVersion { major, minor })
        }
    }
}

// =============================================================================
// Мутабельная версия Hive (для clear_volatile_subkeys и записи)
// =============================================================================

/// Мутабельная версия Hive для операций модификации.
#[cfg(feature = "write")]
pub struct HiveMut<'h> {
    /// Ссылка на базовый блок
    base_block: Ref<&'h mut [u8], HiveBaseBlock>,
    /// Данные hive (после базового блока)
    pub(crate) data: &'h mut [u8],
}

#[cfg(feature = "write")]
impl<'h> HiveMut<'h> {
    /// Создает мутабельный `HiveMut` без валидации.
    ///
    /// Для валидации сначала создайте immutable `Hive` и вызовите `validate()`.
    pub fn without_validation(bytes: &'h mut [u8]) -> Result<Self> {
        let length = bytes.len();
        let (base_block, data) =
            Ref::from_prefix(bytes).map_err(|_| HiveError::InvalidHeaderSize {
                offset: 0,
                expected: mem::size_of::<HiveBaseBlock>(),
                actual: length,
            })?;

        Ok(Self { base_block, data })
    }

    /// Очищает поле `volatile_subkey_count` у всех ключей рекурсивно.
    ///
    /// Это необходимо сделать перед передачей hive ядру NT при загрузке.
    /// См. <https://github.com/reactos/reactos/pull/1883>
    pub fn clear_volatile_subkeys(&mut self) -> Result<()> {
        // TODO: реализовать рекурсивный обход и очистку
        // Требуется KeyNodeMut
        Ok(())
    }
}

// =============================================================================
// Вспомогательные функции
// =============================================================================

/// Возвращает поддиапазон заданного диапазона.
///
/// Выполняет проверки на переполнение и границы.
pub(crate) fn byte_subrange(range: &Range<usize>, byte_count: usize) -> Option<Range<usize>> {
    let subrange_end = range.start.checked_add(byte_count)?;

    if subrange_end > range.end {
        return None;
    }

    Some(range.start..subrange_end)
}
