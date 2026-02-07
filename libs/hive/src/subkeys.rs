//! Списки подключей (Subkeys Lists)
//!
//! Подключи хранятся в различных структурах для оптимизации поиска:
//!
//! # Типы списков
//!
//! - **Index Leaf (li)** - простой список смещений, Windows NT 3.x
//! - **Fast Leaf (lf)** - смещение + hint (первые 4 символа имени), Windows NT 4+
//! - **Hash Leaf (lh)** - смещение + hash имени, Windows XP+
//! - **Index Root (ri)** - индекс для больших списков, ссылается на другие листья
//!
//! # Архитектура
//!
//! ```text
//!                     ┌─────────────────┐
//!                     │   Index Root    │ (ri) - для > ~500 ключей
//!                     │    count: N     │
//!                     └────────┬────────┘
//!                              │
//!          ┌───────────────────┼───────────────────┐
//!          ▼                   ▼                   ▼
//! ┌─────────────────┐ ┌─────────────────┐ ┌─────────────────┐
//! │   Fast Leaf     │ │   Hash Leaf     │ │   Index Leaf    │
//! │   (lf) NT4+     │ │   (lh) XP+      │ │   (li)          │
//! │ offset+hint[4]  │ │ offset+hash[4]  │ │    offset       │
//! └─────────────────┘ └─────────────────┘ └─────────────────┘
//! ```
//!
//! Подключи отсортированы по имени для бинарного поиска.
//!
//! Источники:
//! - ReactOS: docs/ref/reactos/sdk/lib/cmlib/cmindex.c

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
use crate::key_node::KeyNode;
use crate::key_node::cell_range_from_data;

// =============================================================================
// Константы
// =============================================================================

/// Сигнатура Index Leaf
const INDEX_LEAF_SIGNATURE: &[u8; 2] = b"li";
/// Сигнатура Fast Leaf
const FAST_LEAF_SIGNATURE: &[u8; 2] = b"lf";
/// Сигнатура Hash Leaf
const HASH_LEAF_SIGNATURE: &[u8; 2] = b"lh";
/// Сигнатура Index Root
const INDEX_ROOT_SIGNATURE: &[u8; 2] = b"ri";

// =============================================================================
// On-Disk структуры
// =============================================================================

/// Общий заголовок для всех типов списков подключей
#[derive(FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned)]
#[repr(packed)]
pub(crate) struct SubkeysListHeader {
    /// Сигнатура (li, lf, lh, ri)
    pub signature: [u8; 2],
    /// Количество элементов
    pub count: U16<LittleEndian>,
}

/// Элемент Index Leaf (li)
#[derive(FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned)]
#[repr(packed)]
struct IndexLeafItem {
    /// Смещение KeyNode
    key_node_offset: U32<LittleEndian>,
}

/// Элемент Fast Leaf (lf)
#[derive(FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned)]
#[repr(packed)]
struct FastLeafItem {
    /// Смещение KeyNode
    key_node_offset: U32<LittleEndian>,
    /// Hint - первые 4 символа имени
    name_hint: [u8; 4],
}

/// Элемент Hash Leaf (lh)
#[derive(FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned)]
#[repr(packed)]
struct HashLeafItem {
    /// Смещение KeyNode
    key_node_offset: U32<LittleEndian>,
    /// Hash имени ключа
    name_hash: [u8; 4],
}

/// Элемент Index Root (ri)
#[derive(FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned)]
#[repr(packed)]
struct IndexRootItem {
    /// Смещение списка подключей (leaf)
    subkeys_list_offset: U32<LittleEndian>,
}

// =============================================================================
// Типы списков
// =============================================================================

/// Тип списка подключей
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubkeysListType {
    /// Index Leaf (li) - простой список
    IndexLeaf,
    /// Fast Leaf (lf) - с hint
    FastLeaf,
    /// Hash Leaf (lh) - с hash
    HashLeaf,
    /// Index Root (ri) - индекс
    IndexRoot,
}

impl SubkeysListType {
    /// Определяет тип по сигнатуре
    pub fn from_signature(sig: &[u8; 2]) -> Option<Self> {
        match sig {
            b"li" => Some(Self::IndexLeaf),
            b"lf" => Some(Self::FastLeaf),
            b"lh" => Some(Self::HashLeaf),
            b"ri" => Some(Self::IndexRoot),
            _ => None,
        }
    }

    /// Размер элемента для данного типа
    pub const fn item_size(&self) -> usize {
        match self {
            Self::IndexLeaf => mem::size_of::<IndexLeafItem>(),
            Self::FastLeaf => mem::size_of::<FastLeafItem>(),
            Self::HashLeaf => mem::size_of::<HashLeafItem>(),
            Self::IndexRoot => mem::size_of::<IndexRootItem>(),
        }
    }
}

// =============================================================================
// SubKeyNodes - итератор по подключам
// =============================================================================

/// Итератор по подключам ключа.
///
/// Объединяет все типы списков подключей в единый интерфейс.
#[derive(Clone)]
pub enum SubKeyNodes<'h> {
    /// Leaf (lf, lh, li)
    Leaf(LeafKeyNodes<'h>),
    /// Index Root (ri)
    IndexRoot(IndexRootKeyNodes<'h>),
}

impl<'h> SubKeyNodes<'h> {
    /// Создает итератор из данных и диапазона cell
    pub(crate) fn new(
        data: &'h [u8],
        base_block_size: usize,
        cell_range: Range<usize>,
    ) -> Result<Self> {
        // Читаем заголовок
        let header_range = byte_subrange(&cell_range, mem::size_of::<SubkeysListHeader>())
            .ok_or_else(|| HiveError::InvalidHeaderSize {
                offset: base_block_size + cell_range.start,
                expected: mem::size_of::<SubkeysListHeader>(),
                actual: cell_range.len(),
            })?;

        let header =
            Ref::<&[u8], SubkeysListHeader>::from_bytes(&data[header_range.clone()]).unwrap();
        let signature = header.signature;
        let count = header.count.get();
        let data_range = header_range.end..cell_range.end;

        let list_type = SubkeysListType::from_signature(&signature).ok_or_else(|| {
            HiveError::InvalidTwoByteSignature {
                offset: base_block_size + header_range.start,
                expected: b"li|lf|lh|ri",
                actual: signature,
            }
        })?;

        match list_type {
            SubkeysListType::IndexRoot => {
                let iter = IndexRootKeyNodes::new(data, base_block_size, count, data_range)?;
                Ok(Self::IndexRoot(iter))
            },
            _ => {
                let iter = LeafKeyNodes::new(data, base_block_size, count, list_type, data_range)?;
                Ok(Self::Leaf(iter))
            },
        }
    }
}

impl<'h> Iterator for SubKeyNodes<'h> {
    type Item = Result<KeyNode<'h>>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Leaf(iter) => iter.next(),
            Self::IndexRoot(iter) => iter.next(),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            Self::Leaf(iter) => iter.size_hint(),
            Self::IndexRoot(iter) => iter.size_hint(),
        }
    }
}

// =============================================================================
// LeafKeyNodes - итератор по leaf (lf, lh, li)
// =============================================================================

/// Итератор по подключам из leaf структуры.
#[derive(Clone)]
pub struct LeafKeyNodes<'h> {
    data: &'h [u8],
    base_block_size: usize,
    items_range: Range<usize>,
    list_type: SubkeysListType,
}

impl<'h> LeafKeyNodes<'h> {
    pub(crate) fn new(
        data: &'h [u8],
        base_block_size: usize,
        count: u16,
        list_type: SubkeysListType,
        data_range: Range<usize>,
    ) -> Result<Self> {
        let byte_count = count as usize * list_type.item_size();

        let items_range =
            byte_subrange(&data_range, byte_count).ok_or_else(|| HiveError::InvalidSizeField {
                offset: base_block_size + data_range.start,
                expected: byte_count,
                actual: data_range.len(),
            })?;

        Ok(Self {
            data,
            base_block_size,
            items_range,
            list_type,
        })
    }

    /// Читает смещение KeyNode из текущего элемента
    fn read_key_node_offset(&self, item_range: Range<usize>) -> u32 {
        // Все типы элементов имеют key_node_offset как первое поле
        let item = Ref::<&[u8], IndexLeafItem>::from_bytes(
            &self.data[item_range.start..item_range.start + 4],
        )
        .unwrap();
        item.key_node_offset.get()
    }
}

impl<'h> Iterator for LeafKeyNodes<'h> {
    type Item = Result<KeyNode<'h>>;

    fn next(&mut self) -> Option<Self::Item> {
        let item_size = self.list_type.item_size();
        let item_range = byte_subrange(&self.items_range, item_size)?;
        self.items_range.start += item_size;

        let offset = self.read_key_node_offset(item_range);
        let key_node = iter_try!(KeyNode::from_data_and_offset(
            self.data,
            self.base_block_size,
            offset
        ));

        Some(Ok(key_node))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let size = self.items_range.len() / self.list_type.item_size();
        (size, Some(size))
    }
}

impl<'h> ExactSizeIterator for LeafKeyNodes<'h> {}

// =============================================================================
// IndexRootKeyNodes - итератор по index root (ri)
// =============================================================================

/// Итератор по подключам из Index Root.
///
/// Index Root содержит ссылки на листья, которые в свою очередь
/// содержат ссылки на KeyNode.
#[derive(Clone)]
pub struct IndexRootKeyNodes<'h> {
    data: &'h [u8],
    base_block_size: usize,
    /// Диапазон элементов Index Root
    root_items_range: Range<usize>,
    /// Текущий итератор по leaf (если есть)
    current_leaf: Option<LeafKeyNodes<'h>>,
}

impl<'h> IndexRootKeyNodes<'h> {
    pub(crate) fn new(
        data: &'h [u8],
        base_block_size: usize,
        count: u16,
        data_range: Range<usize>,
    ) -> Result<Self> {
        let byte_count = count as usize * mem::size_of::<IndexRootItem>();

        let items_range =
            byte_subrange(&data_range, byte_count).ok_or_else(|| HiveError::InvalidSizeField {
                offset: base_block_size + data_range.start,
                expected: byte_count,
                actual: data_range.len(),
            })?;

        Ok(Self {
            data,
            base_block_size,
            root_items_range: items_range,
            current_leaf: None,
        })
    }

    /// Загружает следующий leaf из Index Root
    fn load_next_leaf(&mut self) -> Option<Result<()>> {
        let item_size = mem::size_of::<IndexRootItem>();
        let item_range = byte_subrange(&self.root_items_range, item_size)?;
        self.root_items_range.start += item_size;

        let item = Ref::<&[u8], IndexRootItem>::from_bytes(&self.data[item_range]).unwrap();
        let subkeys_list_offset = item.subkeys_list_offset.get();

        // Загружаем cell списка подключей
        let cell_range = iter_try!(cell_range_from_data(
            self.data,
            self.base_block_size,
            subkeys_list_offset
        ));

        // Читаем заголовок leaf
        let header_range = iter_try!(
            byte_subrange(&cell_range, mem::size_of::<SubkeysListHeader>()).ok_or_else(|| {
                HiveError::InvalidHeaderSize {
                    offset: self.base_block_size + cell_range.start,
                    expected: mem::size_of::<SubkeysListHeader>(),
                    actual: cell_range.len(),
                }
            })
        );

        let header =
            Ref::<&[u8], SubkeysListHeader>::from_bytes(&self.data[header_range.clone()]).unwrap();
        let signature = header.signature;
        let count = header.count.get();
        let data_range = header_range.end..cell_range.end;

        let list_type = iter_try!(SubkeysListType::from_signature(&signature).ok_or_else(|| {
            HiveError::InvalidTwoByteSignature {
                offset: self.base_block_size + header_range.start,
                expected: b"li|lf|lh",
                actual: signature,
            }
        }));

        // Index Root не может содержать другой Index Root
        if list_type == SubkeysListType::IndexRoot {
            return Some(Err(HiveError::InvalidTwoByteSignature {
                offset: self.base_block_size + header_range.start,
                expected: b"li|lf|lh",
                actual: signature,
            }));
        }

        let leaf = iter_try!(LeafKeyNodes::new(
            self.data,
            self.base_block_size,
            count,
            list_type,
            data_range
        ));
        self.current_leaf = Some(leaf);

        Some(Ok(()))
    }
}

impl<'h> Iterator for IndexRootKeyNodes<'h> {
    type Item = Result<KeyNode<'h>>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            // Пытаемся получить следующий элемент из текущего leaf
            if let Some(ref mut leaf) = self.current_leaf {
                if let Some(result) = leaf.next() {
                    return Some(result);
                }
            }

            // Текущий leaf исчерпан, загружаем следующий
            match self.load_next_leaf() {
                Some(Ok(())) => continue,
                Some(Err(e)) => return Some(Err(e)),
                None => return None,
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        // Для Index Root точный размер неизвестен без полного обхода
        (0, None)
    }
}
