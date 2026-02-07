//! Big Data - хранение больших значений
//!
//! Когда размер данных значения превышает ~16KB, данные разбиваются
//! на сегменты и хранятся в структуре Big Data.
//!
//! # Архитектура
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                  Big Data Header (signature: "db")          │
//! ├─────────────────────────────────────────────────────────────┤
//! │  signature: "db"              (2 bytes)                     │
//! │  segment_count                (2 bytes)                     │
//! │  segment_list_offset          (4 bytes)                     │
//! └─────────────────────────────────────────────────────────────┘
//!                              │
//!                              ▼
//! ┌─────────────────────────────────────────────────────────────┐
//! │                     Segment List                            │
//! ├─────────────────────────────────────────────────────────────┤
//! │  segment_offset[0]            (4 bytes)                     │
//! │  segment_offset[1]            (4 bytes)                     │
//! │  ...                                                        │
//! │  segment_offset[N-1]          (4 bytes)                     │
//! └─────────────────────────────────────────────────────────────┘
//!          │           │                      │
//!          ▼           ▼                      ▼
//!     ┌─────────┐ ┌─────────┐           ┌─────────┐
//!     │ Segment │ │ Segment │    ...    │ Segment │
//!     │ 16344B  │ │ 16344B  │           │ <=16344B│
//!     └─────────┘ └─────────┘           └─────────┘
//! ```
//!
//! # Размер сегмента
//!
//! Каждый сегмент (кроме последнего) содержит ровно 16344 байта данных.
//! Последний сегмент может быть меньше.
//!
//! Источники:
//! - Windows registry file format specification (Maxim Suhanov)

use core::cmp;
use core::mem;
use core::ops::Range;

use zerocopy::FromBytes;
use zerocopy::Immutable;
use zerocopy::IntoBytes;
use zerocopy::KnownLayout;
use zerocopy::Ref;
use zerocopy::U16;
use zerocopy::U32;
use zerocopy::Unaligned;
use zerocopy::byteorder::LittleEndian;

use crate::error::HiveError;
use crate::error::Result;
use crate::hive::byte_subrange;
use crate::key_node::cell_range_from_data;

// =============================================================================
// Константы
// =============================================================================

/// Максимальный размер данных в одном сегменте Big Data
pub const BIG_DATA_SEGMENT_SIZE: usize = 16344;

/// Сигнатура Big Data
const BIG_DATA_SIGNATURE: &[u8; 2] = b"db";

// =============================================================================
// On-Disk структуры
// =============================================================================

/// On-disk структура заголовка Big Data
#[derive(FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned)]
#[repr(packed)]
struct BigDataHeader {
    /// Сигнатура "db"
    signature: [u8; 2],
    /// Количество сегментов
    segment_count: U16<LittleEndian>,
    /// Смещение списка сегментов
    segment_list_offset: U32<LittleEndian>,
}

/// On-disk структура элемента списка сегментов
#[derive(FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned)]
#[repr(packed)]
struct BigDataListItem {
    /// Смещение сегмента данных
    segment_offset: U32<LittleEndian>,
}

// =============================================================================
// BigDataSlices - итератор по сегментам
// =============================================================================

/// Итератор по сегментам Big Data.
///
/// Каждый вызов `next()` возвращает слайс с данными очередного сегмента.
#[derive(Clone)]
pub struct BigDataSlices<'h> {
    data: &'h [u8],
    base_block_size: usize,
    /// Диапазон элементов списка сегментов
    list_items_range: Range<usize>,
    /// Оставшееся количество байт данных
    bytes_left: usize,
}

impl<'h> BigDataSlices<'h> {
    /// Создает итератор Big Data
    pub(crate) fn new(
        data: &'h [u8],
        base_block_size: usize,
        data_size: u32,
        data_size_field_offset: usize,
        header_cell_range: Range<usize>,
    ) -> Result<Self> {
        let data_size_usize = data_size as usize;

        // Проверяем заголовок
        let header_range = byte_subrange(&header_cell_range, mem::size_of::<BigDataHeader>())
            .ok_or_else(|| HiveError::InvalidHeaderSize {
                offset: base_block_size + header_cell_range.start,
                expected: mem::size_of::<BigDataHeader>(),
                actual: header_cell_range.len(),
            })?;

        let header = Ref::<&[u8], BigDataHeader>::from_bytes(&data[header_range.clone()]).unwrap();

        // Проверяем сигнатуру
        if &header.signature != BIG_DATA_SIGNATURE {
            return Err(HiveError::InvalidTwoByteSignature {
                offset: base_block_size + header_range.start,
                expected: BIG_DATA_SIGNATURE,
                actual: header.signature,
            });
        }

        // Проверяем количество сегментов
        let segment_count = header.segment_count.get();
        let max_data_size = segment_count as usize * BIG_DATA_SEGMENT_SIZE;
        if data_size_usize > max_data_size {
            return Err(HiveError::InvalidSizeField {
                offset: data_size_field_offset,
                expected: max_data_size,
                actual: data_size_usize,
            });
        }

        // Получаем список сегментов
        let segment_list_offset = header.segment_list_offset.get();
        let segment_list_cell_range =
            cell_range_from_data(data, base_block_size, segment_list_offset)?;

        let list_byte_count = segment_count as usize * mem::size_of::<BigDataListItem>();
        let list_items_range = byte_subrange(&segment_list_cell_range, list_byte_count)
            .ok_or_else(|| HiveError::InvalidSizeField {
                offset: base_block_size + header_range.start + 2, // offset of segment_count
                expected: list_byte_count,
                actual: segment_list_cell_range.len(),
            })?;

        Ok(Self {
            data,
            base_block_size,
            list_items_range,
            bytes_left: data_size_usize,
        })
    }
}

impl<'h> Iterator for BigDataSlices<'h> {
    type Item = Result<&'h [u8]>;

    fn next(&mut self) -> Option<Self::Item> {
        // Определяем размер данных для этого сегмента
        let bytes_to_return = cmp::min(self.bytes_left, BIG_DATA_SEGMENT_SIZE);
        if bytes_to_return == 0 {
            return None;
        }

        // Читаем смещение сегмента
        let item_size = mem::size_of::<BigDataListItem>();
        let item_range = byte_subrange(&self.list_items_range, item_size)?;
        self.list_items_range.start += item_size;

        let item = Ref::<&[u8], BigDataListItem>::from_bytes(&self.data[item_range]).unwrap();
        let segment_offset = item.segment_offset.get();

        // Уменьшаем оставшееся количество
        self.bytes_left -= bytes_to_return;

        // Получаем cell сегмента
        let cell_range = iter_try!(cell_range_from_data(
            self.data,
            self.base_block_size,
            segment_offset
        ));

        // Проверяем размер
        let data_range = iter_try!(byte_subrange(&cell_range, bytes_to_return).ok_or_else(|| {
            HiveError::InvalidDataSize {
                offset: self.base_block_size + cell_range.start,
                expected: bytes_to_return,
                actual: cell_range.len(),
            }
        }));

        Some(Ok(&self.data[data_range]))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let item_size = mem::size_of::<BigDataListItem>();
        let size = self.list_items_range.len() / item_size;
        (size, Some(size))
    }
}

impl<'h> ExactSizeIterator for BigDataSlices<'h> {}
