//! HiveBuilder - создание и модификация hive файлов
//!
//! Этот модуль доступен только с feature = "write".
//!
//! # Архитектура
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                         HiveBuilder                             │
//! ├─────────────────────────────────────────────────────────────────┤
//! │  data: Vec<u8>           - буфер данных hive                    │
//! │  allocator: CellAllocator - управление свободными cells         │
//! │  root_key_offset: u32    - смещение корневого ключа             │
//! └─────────────────────────────────────────────────────────────────┘
//!                              │
//!                              ▼
//! ┌─────────────────────────────────────────────────────────────────┐
//! │  [0..4096]     HiveBaseBlock                                    │
//! │  [4096..]      Data (bins с cells)                              │
//! │    [4096..4128]  BinHeader                                      │
//! │    [4128..]      Cells (KeyNode, KeyValue, lists, etc.)         │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Использование
//!
//! ```ignore
//! use hive::HiveBuilder;
//!
//! let mut builder = HiveBuilder::new("SYSTEM")?;
//! let root = builder.root_key();
//!
//! // Создание иерархии ключей
//! let control_set = builder.create_subkey(&root, "ControlSet001")?;
//! let control = builder.create_subkey(&control_set, "Control")?;
//!
//! // Добавление значений
//! builder.set_dword(&control, "CurrentControlSet", 1)?;
//! builder.set_string(&control, "SystemRoot", r"\SystemRoot")?;
//!
//! // Сборка финального hive
//! let hive_bytes = builder.build()?;
//! ```
//!
//! Источники:
//! - MSDN (NT6.1): Registry Hive Format
//! - ReactOS: docs/ref/reactos/sdk/lib/cmlib

use alloc::vec;
use alloc::vec::Vec;
use core::mem;

use zerocopy::FromBytes;
use zerocopy::Immutable;
use zerocopy::IntoBytes;
use zerocopy::KnownLayout;
use zerocopy::U16;
use zerocopy::U32;
use zerocopy::U64;
use zerocopy::Unaligned;
use zerocopy::byteorder::LittleEndian;

use crate::cell::CELL_HEADER_SIZE;
use crate::cell::CELL_MIN_SIZE;
use crate::cell::align_cell_size;
use crate::error::Result;
use crate::key_value::KeyValueDataType;

// =============================================================================
// Константы
// =============================================================================

/// Размер базового блока hive
const HIVE_BASE_BLOCK_SIZE: usize = 4096;

/// Размер bin по умолчанию (4KB)
const DEFAULT_BIN_SIZE: usize = 4096;

/// Сигнатура hive
const HIVE_SIGNATURE: [u8; 4] = *b"regf";

/// Сигнатура bin
const BIN_SIGNATURE: [u8; 4] = *b"hbin";

/// Сигнатура KeyNode
const KEY_NODE_SIGNATURE: [u8; 2] = *b"nk";

/// Сигнатура KeyValue
const KEY_VALUE_SIGNATURE: [u8; 2] = *b"vk";

/// Сигнатура Hash Leaf
const HASH_LEAF_SIGNATURE: [u8; 2] = *b"lh";

/// Флаг: корневой ключ
const KEY_HIVE_ENTRY: u16 = 0x0004;

/// Флаг: имя в ASCII
const KEY_COMP_NAME: u16 = 0x0020;

/// Флаг: имя значения в ASCII
const VALUE_COMP_NAME: u16 = 0x0001;

/// Данные хранятся в data_offset
const DATA_IN_OFFSET: u32 = 0x8000_0000;

// =============================================================================
// On-Disk структуры для записи
// =============================================================================

/// Базовый блок hive для записи
#[allow(dead_code)]
#[derive(FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned)]
#[repr(packed)]
struct HiveBaseBlock {
    signature: [u8; 4],
    primary_sequence_number: U32<LittleEndian>,
    secondary_sequence_number: U32<LittleEndian>,
    timestamp: U64<LittleEndian>,
    major_version: U32<LittleEndian>,
    minor_version: U32<LittleEndian>,
    file_type: U32<LittleEndian>,
    file_format: U32<LittleEndian>,
    root_cell_offset: U32<LittleEndian>,
    data_size: U32<LittleEndian>,
    clustering_factor: U32<LittleEndian>,
    file_name: [U16<LittleEndian>; 32],
    padding_1: [u8; 396],
    checksum: U32<LittleEndian>,
    padding_2: [u8; 3576],
    boot_type: U32<LittleEndian>,
    boot_recover: U32<LittleEndian>,
}

/// Заголовок KeyNode для записи
#[allow(dead_code)]
#[derive(FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned)]
#[repr(packed)]
struct KeyNodeHeader {
    signature: [u8; 2],
    flags: U16<LittleEndian>,
    timestamp: U64<LittleEndian>,
    spare: U32<LittleEndian>,
    parent: U32<LittleEndian>,
    subkey_count: U32<LittleEndian>,
    volatile_subkey_count: U32<LittleEndian>,
    subkeys_list_offset: U32<LittleEndian>,
    volatile_subkeys_list_offset: U32<LittleEndian>,
    values_count: U32<LittleEndian>,
    values_list_offset: U32<LittleEndian>,
    security_offset: U32<LittleEndian>,
    class_name_offset: U32<LittleEndian>,
    max_subkey_name: U32<LittleEndian>,
    max_subkey_class_name: U32<LittleEndian>,
    max_value_name: U32<LittleEndian>,
    max_value_data: U32<LittleEndian>,
    work_var: U32<LittleEndian>,
    key_name_length: U16<LittleEndian>,
    class_name_length: U16<LittleEndian>,
}

/// Заголовок KeyValue для записи
#[allow(dead_code)]
#[derive(FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned)]
#[repr(packed)]
struct KeyValueHeader {
    signature: [u8; 2],
    name_length: U16<LittleEndian>,
    data_size: U32<LittleEndian>,
    data_offset: U32<LittleEndian>,
    data_type: U32<LittleEndian>,
    flags: U16<LittleEndian>,
    spare: U16<LittleEndian>,
}

/// Заголовок списка подключей (Hash Leaf)
#[allow(dead_code)]
#[derive(FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned)]
#[repr(packed)]
struct SubkeysListHeader {
    signature: [u8; 2],
    count: U16<LittleEndian>,
}

/// Элемент Hash Leaf
#[allow(dead_code)]
#[derive(FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned)]
#[repr(packed)]
struct HashLeafItem {
    key_node_offset: U32<LittleEndian>,
    name_hash: U32<LittleEndian>,
}

// =============================================================================
// KeyHandle
// =============================================================================

/// Handle для ключа в builder.
///
/// Используется для ссылки на ключи при создании подключей и значений.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyHandle {
    /// Смещение ключа в данных (относительно начала data, не файла)
    pub(crate) offset: u32,
}

impl KeyHandle {
    /// Создает новый handle
    fn new(offset: u32) -> Self {
        Self { offset }
    }
}

// =============================================================================
// HiveBuilder
// =============================================================================

/// Builder для создания и модификации hive файлов.
pub struct HiveBuilder {
    /// Данные hive (включая базовый блок)
    data: Vec<u8>,
    /// Смещение корневого ключа
    root_key_offset: u32,
    /// Текущий sequence number
    sequence_number: u32,
}

impl HiveBuilder {
    /// Создает новый пустой hive с указанным именем.
    ///
    /// Имя используется только для информации в заголовке.
    pub fn new(name: &str) -> Result<Self> {
        let mut builder = Self {
            data: vec![0u8; HIVE_BASE_BLOCK_SIZE + DEFAULT_BIN_SIZE],
            root_key_offset: 0,
            sequence_number: 1,
        };

        // Инициализируем базовый блок
        builder.init_base_block(name);

        // Инициализируем первый bin
        builder.init_first_bin();

        // Создаем корневой ключ
        builder.create_root_key(name)?;

        Ok(builder)
    }

    /// Создает builder из существующего hive для модификации.
    ///
    /// Загружает существующий hive и позволяет добавлять новые ключи и значения.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        use crate::Hive;

        // Валидируем hive
        let hive = Hive::new(bytes)?;
        let root = hive.root_key_node()?;

        // Читаем root_key_offset из базового блока
        let root_key_offset = u32::from_le_bytes(bytes[36..40].try_into().unwrap());

        // Читаем sequence number
        let sequence_number = u32::from_le_bytes(bytes[4..8].try_into().unwrap());

        // Копируем данные
        let data = bytes.to_vec();

        Ok(Self {
            data,
            root_key_offset,
            sequence_number,
        })
    }

    /// Возвращает текущий размер hive в байтах.
    pub fn size(&self) -> usize {
        self.data.len()
    }

    /// Возвращает данные hive без финализации.
    ///
    /// Используйте этот метод для отладки. Для получения корректного hive
    /// используйте `build()`.
    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }

    /// Инициализирует базовый блок hive
    fn init_base_block(&mut self, name: &str) {
        // Заполняем заголовок
        self.data[0..4].copy_from_slice(&HIVE_SIGNATURE);

        // Sequence numbers
        self.write_u32(4, self.sequence_number);
        self.write_u32(8, self.sequence_number);

        // Timestamp (0 для простоты)
        self.write_u64(12, 0);

        // Version: 1.5 (Windows XP+)
        self.write_u32(20, 1); // major
        self.write_u32(24, 5); // minor

        // File type: Primary (0)
        self.write_u32(28, 0);

        // File format: Memory (1)
        self.write_u32(32, 1);

        // Root cell offset (будет установлен позже)
        self.write_u32(36, 0);

        // Data size
        self.write_u32(40, DEFAULT_BIN_SIZE as u32);

        // Clustering factor
        self.write_u32(44, 1);

        // File name (UTF-16LE, до 32 символов)
        let name_offset = 48;
        for (i, c) in name.encode_utf16().take(31).enumerate() {
            self.write_u16(name_offset + i * 2, c);
        }

        // Контрольная сумма будет вычислена в build()
    }

    /// Инициализирует первый bin
    fn init_first_bin(&mut self) {
        let bin_offset = HIVE_BASE_BLOCK_SIZE;

        // Сигнатура "hbin"
        self.data[bin_offset..bin_offset + 4].copy_from_slice(&BIN_SIGNATURE);

        // Смещение от первого hbin (0 для первого)
        self.write_u32(bin_offset + 4, 0);

        // Размер bin
        self.write_u32(bin_offset + 8, DEFAULT_BIN_SIZE as u32);

        // Reserved и timestamp - оставляем нулями

        // Создаем одну большую свободную cell после заголовка bin
        let cell_offset = bin_offset + 32; // После BinHeader
        let cell_size = DEFAULT_BIN_SIZE - 32;

        // Положительный размер = свободная cell
        self.write_i32(cell_offset, cell_size as i32);
    }

    /// Создает корневой ключ
    fn create_root_key(&mut self, name: &str) -> Result<()> {
        let key_offset = self.allocate_key_node(name, 0, KEY_HIVE_ENTRY | KEY_COMP_NAME)?;
        self.root_key_offset = key_offset;

        // Обновляем root_cell_offset в базовом блоке
        self.write_u32(36, key_offset);

        Ok(())
    }

    /// Возвращает handle корневого ключа.
    pub fn root_key(&self) -> KeyHandle {
        KeyHandle::new(self.root_key_offset)
    }

    /// Создает подключ.
    pub fn create_subkey(&mut self, parent: &KeyHandle, name: &str) -> Result<KeyHandle> {
        // Создаем новый KeyNode
        let key_offset = self.allocate_key_node(name, parent.offset, KEY_COMP_NAME)?;

        // Добавляем ключ в список подключей родителя
        self.add_subkey_to_parent(parent.offset, key_offset, name)?;

        Ok(KeyHandle::new(key_offset))
    }

    /// Аллоцирует и инициализирует KeyNode
    ///
    /// Возвращает cell_offset (смещение cell относительно начала hive data).
    /// Это смещение включает cell header.
    fn allocate_key_node(&mut self, name: &str, parent_offset: u32, flags: u16) -> Result<u32> {
        let name_bytes = name.as_bytes();
        let header_size = mem::size_of::<KeyNodeHeader>();
        let total_size = header_size + name_bytes.len();

        let cell_offset = self.allocate_cell(total_size)?;

        // Записываем KeyNodeHeader (после cell header)
        let header = KeyNodeHeader {
            signature: KEY_NODE_SIGNATURE,
            flags: U16::new(flags),
            timestamp: U64::new(0),
            spare: U32::new(0),
            parent: U32::new(parent_offset),
            subkey_count: U32::new(0),
            volatile_subkey_count: U32::new(0),
            subkeys_list_offset: U32::new(u32::MAX),
            volatile_subkeys_list_offset: U32::new(u32::MAX),
            values_count: U32::new(0),
            values_list_offset: U32::new(u32::MAX),
            security_offset: U32::new(u32::MAX),
            class_name_offset: U32::new(u32::MAX),
            max_subkey_name: U32::new(0),
            max_subkey_class_name: U32::new(0),
            max_value_name: U32::new(0),
            max_value_data: U32::new(0),
            work_var: U32::new(0),
            key_name_length: U16::new(name_bytes.len() as u16),
            class_name_length: U16::new(0),
        };

        // Абсолютное смещение данных в буфере (после cell header)
        let abs_data_offset = HIVE_BASE_BLOCK_SIZE + cell_offset as usize + CELL_HEADER_SIZE;
        let header_bytes = header.as_bytes();
        self.data[abs_data_offset..abs_data_offset + header_size].copy_from_slice(header_bytes);

        // Записываем имя
        let name_abs_offset = abs_data_offset + header_size;
        self.data[name_abs_offset..name_abs_offset + name_bytes.len()].copy_from_slice(name_bytes);

        // Возвращаем cell_offset (смещение начала cell относительно hive data)
        Ok(cell_offset)
    }

    /// Добавляет подключ в список подключей родителя
    ///
    /// parent_offset и child_offset - это cell_offset (включая cell header)
    fn add_subkey_to_parent(
        &mut self,
        parent_offset: u32,
        child_offset: u32,
        child_name: &str,
    ) -> Result<()> {
        // Абсолютное смещение данных KeyNode родителя (после cell header)
        let parent_data_abs = HIVE_BASE_BLOCK_SIZE + parent_offset as usize + CELL_HEADER_SIZE;

        // Смещения полей в KeyNodeHeader:
        // subkey_count: offset 20
        // subkeys_list_offset: offset 28
        // max_subkey_name: offset 52

        let subkeys_list_offset = self.read_u32(parent_data_abs + 28);
        let subkey_count = self.read_u32(parent_data_abs + 20);

        let name_hash = compute_name_hash(child_name);

        if subkeys_list_offset == u32::MAX {
            // Нет списка подключей, создаем новый
            let list_offset = self.create_subkeys_list(&[(child_offset, name_hash)])?;
            self.write_u32(parent_data_abs + 28, list_offset); // subkeys_list_offset
        } else {
            // Расширяем существующий список
            let new_list_offset =
                self.extend_subkeys_list(subkeys_list_offset, child_offset, name_hash)?;
            self.write_u32(parent_data_abs + 28, new_list_offset);
        }

        // Увеличиваем счетчик подключей
        self.write_u32(parent_data_abs + 20, subkey_count + 1);

        // Обновляем max_subkey_name если нужно
        let current_max = self.read_u32(parent_data_abs + 52);
        let name_len = child_name.len() as u32 * 2; // UTF-16
        if name_len > current_max {
            self.write_u32(parent_data_abs + 52, name_len);
        }

        Ok(())
    }

    /// Создает новый список подключей (Hash Leaf)
    ///
    /// Возвращает cell_offset (смещение cell относительно hive data).
    fn create_subkeys_list(&mut self, items: &[(u32, u32)]) -> Result<u32> {
        let header_size = mem::size_of::<SubkeysListHeader>();
        let item_size = mem::size_of::<HashLeafItem>();
        let total_size = header_size + items.len() * item_size;

        let cell_offset = self.allocate_cell(total_size)?;
        let abs_data_offset = HIVE_BASE_BLOCK_SIZE + cell_offset as usize + CELL_HEADER_SIZE;

        // Записываем заголовок
        self.data[abs_data_offset..abs_data_offset + 2].copy_from_slice(&HASH_LEAF_SIGNATURE);
        self.write_u16(abs_data_offset + 2, items.len() as u16);

        // Записываем элементы
        for (i, &(key_offset, hash)) in items.iter().enumerate() {
            let item_abs_offset = abs_data_offset + header_size + i * item_size;
            self.write_u32(item_abs_offset, key_offset);
            self.write_u32(item_abs_offset + 4, hash);
        }

        Ok(cell_offset)
    }

    /// Расширяет существующий список подключей
    ///
    /// list_offset - это cell_offset (включает cell header).
    /// Возвращает новый cell_offset списка.
    fn extend_subkeys_list(
        &mut self,
        list_offset: u32,
        new_key_offset: u32,
        name_hash: u32,
    ) -> Result<u32> {
        // Абсолютное смещение данных списка (после cell header)
        let list_data_abs = HIVE_BASE_BLOCK_SIZE + list_offset as usize + CELL_HEADER_SIZE;

        // Читаем текущий count
        let count = self.read_u16(list_data_abs + 2) as usize;

        // Собираем все существующие элементы + новый
        let header_size = mem::size_of::<SubkeysListHeader>();
        let item_size = mem::size_of::<HashLeafItem>();

        let mut items: Vec<(u32, u32)> = Vec::with_capacity(count + 1);
        for i in 0..count {
            let item_abs_offset = list_data_abs + header_size + i * item_size;
            let key_offset = self.read_u32(item_abs_offset);
            let hash = self.read_u32(item_abs_offset + 4);
            items.push((key_offset, hash));
        }
        items.push((new_key_offset, name_hash));

        // Сортируем по hash для бинарного поиска
        items.sort_by_key(|&(_, hash)| hash);

        // Освобождаем старую cell
        self.free_cell(list_offset);

        // Создаем новый список
        let new_list_offset = self.create_subkeys_list(&items)?;

        Ok(new_list_offset)
    }

    /// Устанавливает DWORD значение.
    pub fn set_dword(&mut self, key: &KeyHandle, name: &str, value: u32) -> Result<()> {
        let data = value.to_le_bytes();
        self.set_value_internal(key, name, KeyValueDataType::RegDWord, &data)
    }

    /// Устанавливает QWORD значение.
    pub fn set_qword(&mut self, key: &KeyHandle, name: &str, value: u64) -> Result<()> {
        let data = value.to_le_bytes();
        self.set_value_internal(key, name, KeyValueDataType::RegQWord, &data)
    }

    /// Устанавливает строковое значение (REG_SZ).
    pub fn set_string(&mut self, key: &KeyHandle, name: &str, value: &str) -> Result<()> {
        // Конвертируем в UTF-16LE с null-terminator
        let mut data: Vec<u8> = Vec::new();
        for c in value.encode_utf16() {
            data.extend_from_slice(&c.to_le_bytes());
        }
        // Null terminator
        data.extend_from_slice(&[0, 0]);

        self.set_value_internal(key, name, KeyValueDataType::RegSZ, &data)
    }

    /// Устанавливает expand string значение (REG_EXPAND_SZ).
    pub fn set_expand_string(&mut self, key: &KeyHandle, name: &str, value: &str) -> Result<()> {
        let mut data: Vec<u8> = Vec::new();
        for c in value.encode_utf16() {
            data.extend_from_slice(&c.to_le_bytes());
        }
        data.extend_from_slice(&[0, 0]);

        self.set_value_internal(key, name, KeyValueDataType::RegExpandSZ, &data)
    }

    /// Устанавливает бинарное значение.
    pub fn set_binary(&mut self, key: &KeyHandle, name: &str, value: &[u8]) -> Result<()> {
        self.set_value_internal(key, name, KeyValueDataType::RegBinary, value)
    }

    /// Устанавливает multi-string значение (REG_MULTI_SZ).
    pub fn set_multi_string(&mut self, key: &KeyHandle, name: &str, values: &[&str]) -> Result<()> {
        let mut data: Vec<u8> = Vec::new();

        for value in values {
            for c in value.encode_utf16() {
                data.extend_from_slice(&c.to_le_bytes());
            }
            // Null terminator после каждой строки
            data.extend_from_slice(&[0, 0]);
        }
        // Финальный null terminator
        data.extend_from_slice(&[0, 0]);

        self.set_value_internal(key, name, KeyValueDataType::RegMultiSZ, &data)
    }

    /// Устанавливает значение с произвольным типом и сырыми данными.
    ///
    /// Используется когда данные уже в нужном формате (например, UTF-16LE строки).
    pub fn set_raw(
        &mut self,
        key: &KeyHandle,
        name: &str,
        data_type: KeyValueDataType,
        data: &[u8],
    ) -> Result<()> {
        self.set_value_internal(key, name, data_type, data)
    }

    /// Внутренний метод установки значения
    fn set_value_internal(
        &mut self,
        key: &KeyHandle,
        name: &str,
        data_type: KeyValueDataType,
        data: &[u8],
    ) -> Result<()> {
        let value_offset = self.allocate_key_value(name, data_type, data)?;

        // Добавляем значение в список значений ключа
        self.add_value_to_key(key.offset, value_offset)?;

        Ok(())
    }

    /// Аллоцирует и инициализирует KeyValue
    ///
    /// Возвращает cell_offset (смещение cell относительно hive data).
    fn allocate_key_value(
        &mut self,
        name: &str,
        data_type: KeyValueDataType,
        data: &[u8],
    ) -> Result<u32> {
        let name_bytes = name.as_bytes();
        let header_size = mem::size_of::<KeyValueHeader>();

        // Определяем, помещаются ли данные в data_offset
        let data_in_offset = data.len() <= 4;

        let total_size = header_size + name_bytes.len();

        let cell_offset = self.allocate_cell(total_size)?;
        let abs_data_offset = HIVE_BASE_BLOCK_SIZE + cell_offset as usize + CELL_HEADER_SIZE;

        // Аллоцируем данные если не помещаются в offset
        let (stored_data_offset, stored_data_size) = if data_in_offset {
            // Данные прямо в поле data_offset
            let mut data_offset_value = 0u32;
            for (i, &byte) in data.iter().enumerate() {
                data_offset_value |= (byte as u32) << (i * 8);
            }
            (data_offset_value, (data.len() as u32) | DATA_IN_OFFSET)
        } else {
            // Аллоцируем отдельную cell для данных
            let data_cell_offset = self.allocate_cell(data.len())?;
            let data_cell_abs = HIVE_BASE_BLOCK_SIZE + data_cell_offset as usize + CELL_HEADER_SIZE;
            self.data[data_cell_abs..data_cell_abs + data.len()].copy_from_slice(data);
            // В hive формате data_offset указывает на cell, не на данные
            (data_cell_offset, data.len() as u32)
        };

        // Записываем KeyValueHeader
        let header = KeyValueHeader {
            signature: KEY_VALUE_SIGNATURE,
            name_length: U16::new(name_bytes.len() as u16),
            data_size: U32::new(stored_data_size),
            data_offset: U32::new(stored_data_offset),
            data_type: U32::new(data_type as u32),
            flags: U16::new(if name_bytes.is_ascii() {
                VALUE_COMP_NAME
            } else {
                0
            }),
            spare: U16::new(0),
        };

        let header_bytes = header.as_bytes();
        self.data[abs_data_offset..abs_data_offset + header_size].copy_from_slice(header_bytes);

        // Записываем имя
        let name_abs_offset = abs_data_offset + header_size;
        self.data[name_abs_offset..name_abs_offset + name_bytes.len()].copy_from_slice(name_bytes);

        // Возвращаем cell_offset
        Ok(cell_offset)
    }

    /// Добавляет значение в список значений ключа
    ///
    /// key_offset и value_offset - это cell_offset (включая cell header)
    fn add_value_to_key(&mut self, key_offset: u32, value_offset: u32) -> Result<()> {
        // Абсолютное смещение данных KeyNode (после cell header)
        let key_data_abs = HIVE_BASE_BLOCK_SIZE + key_offset as usize + CELL_HEADER_SIZE;

        // Смещения полей в KeyNodeHeader:
        // values_count: offset 36
        // values_list_offset: offset 40

        let values_list_offset = self.read_u32(key_data_abs + 40);
        let values_count = self.read_u32(key_data_abs + 36);

        if values_list_offset == u32::MAX {
            // Создаем новый список значений
            let list_offset = self.create_values_list(&[value_offset])?;
            self.write_u32(key_data_abs + 40, list_offset);
        } else {
            // Расширяем существующий список
            self.extend_values_list(key_offset, values_list_offset, value_offset)?;
        }

        // Увеличиваем счетчик значений
        self.write_u32(key_data_abs + 36, values_count + 1);

        Ok(())
    }

    /// Создает список значений
    ///
    /// Возвращает cell_offset (смещение cell относительно hive data).
    fn create_values_list(&mut self, value_offsets: &[u32]) -> Result<u32> {
        let total_size = value_offsets.len() * 4;

        let cell_offset = self.allocate_cell(total_size)?;
        let abs_data_offset = HIVE_BASE_BLOCK_SIZE + cell_offset as usize + CELL_HEADER_SIZE;

        for (i, &offset) in value_offsets.iter().enumerate() {
            self.write_u32(abs_data_offset + i * 4, offset);
        }

        Ok(cell_offset)
    }

    /// Расширяет список значений
    ///
    /// key_offset и list_offset - это cell_offset (включая cell header)
    fn extend_values_list(
        &mut self,
        key_offset: u32,
        list_offset: u32,
        new_value_offset: u32,
    ) -> Result<()> {
        // Абсолютное смещение данных KeyNode (после cell header)
        let key_data_abs = HIVE_BASE_BLOCK_SIZE + key_offset as usize + CELL_HEADER_SIZE;
        // values_count at offset 36
        let count = self.read_u32(key_data_abs + 36) as usize;

        // Абсолютное смещение данных списка (после cell header)
        let list_data_abs = HIVE_BASE_BLOCK_SIZE + list_offset as usize + CELL_HEADER_SIZE;
        let mut offsets: Vec<u32> = Vec::with_capacity(count + 1);
        for i in 0..count {
            offsets.push(self.read_u32(list_data_abs + i * 4));
        }
        offsets.push(new_value_offset);

        // Освобождаем старую cell
        self.free_cell(list_offset);

        // Создаем новый список
        let new_list_offset = self.create_values_list(&offsets)?;
        // values_list_offset at offset 40
        self.write_u32(key_data_abs + 40, new_list_offset);

        Ok(())
    }

    /// Аллоцирует cell заданного размера
    fn allocate_cell(&mut self, data_size: usize) -> Result<u32> {
        let required_size = align_cell_size(CELL_HEADER_SIZE + data_size);

        // Обходим все bins
        let mut bin_offset = HIVE_BASE_BLOCK_SIZE;

        while bin_offset < self.data.len() {
            // Проверяем сигнатуру bin
            if &self.data[bin_offset..bin_offset + 4] != b"hbin" {
                break;
            }

            // Читаем размер bin
            let bin_size = self.read_u32(bin_offset + 8) as usize;
            let bin_end = bin_offset + bin_size;

            // Обходим cells внутри bin
            let mut offset = bin_offset + 32; // После BinHeader

            while offset < bin_end && offset + 4 <= self.data.len() {
                let size = self.read_i32(offset);

                if size == 0 {
                    break;
                }

                let abs_size = size.unsigned_abs() as usize;

                if size > 0 && abs_size >= required_size {
                    // Нашли свободную cell достаточного размера
                    let remaining = abs_size - required_size;

                    // Помечаем как занятую (отрицательный размер)
                    self.write_i32(offset, -(required_size as i32));

                    // Если осталось место, создаем новую свободную cell
                    if remaining >= CELL_MIN_SIZE {
                        self.write_i32(offset + required_size, remaining as i32);
                    }

                    // Возвращаем смещение относительно начала данных
                    return Ok((offset - HIVE_BASE_BLOCK_SIZE) as u32);
                }

                offset += abs_size;
            }

            // Переходим к следующему bin
            bin_offset += bin_size;
        }

        // Нет свободного места - расширяем hive
        self.extend_hive(required_size)?;
        self.allocate_cell(data_size)
    }

    /// Освобождает cell
    fn free_cell(&mut self, cell_offset: u32) {
        let abs_offset = HIVE_BASE_BLOCK_SIZE + cell_offset as usize;
        let size = self.read_i32(abs_offset);

        if size < 0 {
            // Делаем положительным (свободная)
            self.write_i32(abs_offset, -size);
        }

        // TODO: объединение соседних свободных cells
    }

    /// Расширяет hive добавлением нового bin
    fn extend_hive(&mut self, min_size: usize) -> Result<()> {
        let new_bin_size =
            ((min_size + DEFAULT_BIN_SIZE - 1) / DEFAULT_BIN_SIZE) * DEFAULT_BIN_SIZE;
        let old_len = self.data.len();

        // Расширяем буфер
        self.data.resize(old_len + new_bin_size, 0);

        // Инициализируем новый bin
        let bin_offset = old_len;
        self.data[bin_offset..bin_offset + 4].copy_from_slice(&BIN_SIGNATURE);
        self.write_u32(bin_offset + 4, (bin_offset - HIVE_BASE_BLOCK_SIZE) as u32);
        self.write_u32(bin_offset + 8, new_bin_size as u32);

        // Создаем свободную cell
        let cell_offset = bin_offset + 32;
        let cell_size = new_bin_size - 32;
        self.write_i32(cell_offset, cell_size as i32);

        // Обновляем data_size в базовом блоке
        let new_data_size = self.data.len() - HIVE_BASE_BLOCK_SIZE;
        self.write_u32(40, new_data_size as u32);

        Ok(())
    }

    /// Собирает финальный hive.
    pub fn build(mut self) -> Result<Vec<u8>> {
        // Обновляем sequence numbers
        self.sequence_number += 1;
        self.write_u32(4, self.sequence_number);
        self.write_u32(8, self.sequence_number);

        // Вычисляем контрольную сумму
        let checksum = self.compute_checksum();
        self.write_u32(508, checksum);

        Ok(self.data)
    }

    /// Вычисляет контрольную сумму базового блока
    fn compute_checksum(&self) -> u32 {
        let mut checksum = 0u32;

        for i in (0..508).step_by(4) {
            let dword = self.read_u32(i);
            checksum ^= dword;
        }

        if checksum == 0 {
            checksum = 1;
        } else if checksum == u32::MAX {
            checksum = u32::MAX - 1;
        }

        checksum
    }

    // =========================================================================
    // Вспомогательные методы чтения/записи
    // =========================================================================

    fn read_u16(&self, offset: usize) -> u16 {
        u16::from_le_bytes(self.data[offset..offset + 2].try_into().unwrap())
    }

    fn read_u32(&self, offset: usize) -> u32 {
        u32::from_le_bytes(self.data[offset..offset + 4].try_into().unwrap())
    }

    fn read_i32(&self, offset: usize) -> i32 {
        i32::from_le_bytes(self.data[offset..offset + 4].try_into().unwrap())
    }

    fn write_u16(&mut self, offset: usize, value: u16) {
        self.data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn write_u32(&mut self, offset: usize, value: u32) {
        self.data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn write_i32(&mut self, offset: usize, value: i32) {
        self.data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn write_u64(&mut self, offset: usize, value: u64) {
        self.data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
}

// =============================================================================
// Вспомогательные функции
// =============================================================================

/// Вычисляет hash имени для Hash Leaf
fn compute_name_hash(name: &str) -> u32 {
    let mut hash = 0u32;

    for c in name.chars() {
        let upper = c.to_ascii_uppercase() as u32;
        hash = hash.wrapping_mul(37).wrapping_add(upper);
    }

    hash
}
