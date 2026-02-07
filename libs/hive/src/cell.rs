//! Работа с cells в hive
//!
//! Cell - это базовая единица хранения данных в hive. Каждая cell имеет заголовок
//! с размером и данные. Размер в заголовке:
//! - Отрицательный = cell аллоцирована (используется)
//! - Положительный = cell свободна
//!
//! # Архитектура
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                         Bin (4KB aligned)                   │
//! ├─────────────────────────────────────────────────────────────┤
//! │  BinHeader (32 bytes)                                       │
//! │  ┌─────────────────────────────────────────────────────┐    │
//! │  │ signature: "hbin"                                   │    │
//! │  │ offset_from_first_hbin                              │    │
//! │  │ bin_size                                            │    │
//! │  │ ...                                                 │    │
//! │  └─────────────────────────────────────────────────────┘    │
//! ├─────────────────────────────────────────────────────────────┤
//! │  Cell 1                                                     │
//! │  ┌─────────────────────────────────────────────────────┐    │
//! │  │ size: i32 (< 0 = allocated, > 0 = free)             │    │
//! │  │ data: [u8; abs(size) - 4]                           │    │
//! │  └─────────────────────────────────────────────────────┘    │
//! ├─────────────────────────────────────────────────────────────┤
//! │  Cell 2                                                     │
//! │  ┌─────────────────────────────────────────────────────┐    │
//! │  │ size: i32                                           │    │
//! │  │ data: [u8]                                          │    │
//! │  └─────────────────────────────────────────────────────┘    │
//! ├─────────────────────────────────────────────────────────────┤
//! │  ... more cells ...                                         │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Выравнивание
//!
//! - Размер cell всегда кратен 8 байтам
//! - Минимальный размер cell = 8 байт (4 байта заголовок + 4 байта данные)
//!
//! Источники:
//! - ReactOS: docs/ref/reactos/sdk/lib/cmlib/cmdata.h

use zerocopy::FromBytes;
use zerocopy::Immutable;
use zerocopy::IntoBytes;
use zerocopy::KnownLayout;
use zerocopy::U32;
use zerocopy::Unaligned;
use zerocopy::byteorder::LittleEndian;

// =============================================================================
// Константы
// =============================================================================

/// Минимальный размер cell (заголовок + минимум данных)
pub const CELL_MIN_SIZE: usize = 8;

/// Выравнивание размера cell
pub const CELL_ALIGNMENT: usize = 8;

/// Размер заголовка cell
pub const CELL_HEADER_SIZE: usize = 4;

/// Размер заголовка bin
pub const BIN_HEADER_SIZE: usize = 32;

/// Сигнатура bin
pub const BIN_SIGNATURE: &[u8; 4] = b"hbin";

// =============================================================================
// On-Disk структуры
// =============================================================================

/// On-disk структура заголовка bin
#[allow(dead_code)]
#[derive(FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned)]
#[repr(packed)]
pub struct BinHeader {
    /// Сигнатура "hbin"
    pub signature: [u8; 4],
    /// Смещение от первого hbin в hive
    pub offset_from_first_hbin: U32<LittleEndian>,
    /// Размер bin (включая заголовок)
    pub bin_size: U32<LittleEndian>,
    /// Reserved
    pub reserved1: U32<LittleEndian>,
    pub reserved2: U32<LittleEndian>,
    /// Timestamp
    pub timestamp: U32<LittleEndian>,
    pub timestamp_high: U32<LittleEndian>,
    /// Spare
    pub spare: U32<LittleEndian>,
}

// =============================================================================
// Cell Allocator (для feature = "write")
// =============================================================================

/// Аллокатор cells для операций записи.
///
/// Используется для:
/// - Аллокации новых cells
/// - Освобождения существующих cells
/// - Слияния соседних свободных cells
/// - Расширения hive при необходимости
#[cfg(feature = "write")]
pub struct CellAllocator {
    /// Список свободных cells (смещение, размер)
    free_cells: alloc::vec::Vec<(u32, u32)>,
    /// Текущий размер данных hive
    data_size: usize,
}

#[cfg(feature = "write")]
impl CellAllocator {
    /// Создает новый аллокатор, сканируя hive на предмет свободных cells.
    pub fn new(data: &[u8]) -> Self {
        let mut allocator = Self {
            free_cells: alloc::vec::Vec::new(),
            data_size: data.len(),
        };
        allocator.scan_free_cells(data);
        allocator
    }

    /// Сканирует данные hive и собирает информацию о свободных cells.
    fn scan_free_cells(&mut self, data: &[u8]) {
        let mut offset = 0usize;

        while offset < data.len() {
            // Проверяем, это bin header?
            if offset + BIN_HEADER_SIZE <= data.len() {
                let sig = &data[offset..offset + 4];
                if sig == BIN_SIGNATURE {
                    // Это bin header, пропускаем его
                    offset += BIN_HEADER_SIZE;
                    continue;
                }
            }

            // Читаем размер cell
            if offset + CELL_HEADER_SIZE > data.len() {
                break;
            }

            let size_bytes: [u8; 4] = data[offset..offset + 4].try_into().unwrap();
            let size = i32::from_le_bytes(size_bytes);

            if size == 0 {
                // Невалидная cell, пропускаем
                break;
            }

            let abs_size = size.unsigned_abs() as usize;

            if size > 0 {
                // Свободная cell
                self.free_cells.push((offset as u32, abs_size as u32));
            }

            offset += abs_size;
        }

        // Сортируем по размеру для best-fit аллокации
        self.free_cells.sort_by_key(|&(_, size)| size);
    }

    /// Аллоцирует cell заданного размера.
    ///
    /// Возвращает смещение аллоцированной cell или None если нет места.
    pub fn allocate(&mut self, required_size: usize) -> Option<u32> {
        // Выравниваем размер
        let aligned_size = align_cell_size(required_size + CELL_HEADER_SIZE);

        // Ищем подходящую свободную cell (best-fit)
        let index = self
            .free_cells
            .iter()
            .position(|&(_, size)| size as usize >= aligned_size)?;

        let (offset, free_size) = self.free_cells.remove(index);
        let remaining = free_size as usize - aligned_size;

        // Если осталось достаточно места, создаем новую свободную cell
        if remaining >= CELL_MIN_SIZE {
            let new_free_offset = offset as usize + aligned_size;
            self.free_cells
                .push((new_free_offset as u32, remaining as u32));
            self.free_cells.sort_by_key(|&(_, size)| size);
        }

        Some(offset)
    }

    /// Освобождает cell по заданному смещению.
    pub fn free(&mut self, offset: u32, size: u32) {
        self.free_cells.push((offset, size));
        // TODO: слияние соседних свободных cells
        self.free_cells.sort_by_key(|&(_, size)| size);
    }
}

// Заглушка для не-write режима
#[cfg(not(feature = "write"))]
pub struct CellAllocator;

// =============================================================================
// Вспомогательные функции
// =============================================================================

/// Выравнивает размер cell до кратного CELL_ALIGNMENT.
pub const fn align_cell_size(size: usize) -> usize {
    (size + CELL_ALIGNMENT - 1) & !(CELL_ALIGNMENT - 1)
}

/// Вычисляет размер cell для данных заданного размера.
pub const fn cell_size_for_data(data_size: usize) -> usize {
    align_cell_size(data_size + CELL_HEADER_SIZE)
}
