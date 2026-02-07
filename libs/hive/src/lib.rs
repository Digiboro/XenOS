//! Библиотека для работы с Windows Registry Hive файлами
//!
//! Hive файлы находятся в `C:\Windows\system32\config` и хранят данные
//! реестра Windows (SYSTEM, SOFTWARE, SAM, SECURITY и др.).
//!
//! Библиотека поддерживает формат hive начиная с Windows NT 4.0 до современных версий.
//! Ориентир реализации: Windows NT 6.1 (Windows 7).
//!
//! # Архитектура
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                    Hive File (REGF format)                      │
//! ├─────────────────────────────────────────────────────────────────┤
//! │  HiveBaseBlock (4096 bytes)                                     │
//! │  ┌──────────────────────────────────────────────────────────┐   │
//! │  │ signature: "regf"                                         │   │
//! │  │ sequence_numbers (primary/secondary)                      │   │
//! │  │ version (major.minor)                                     │   │
//! │  │ root_cell_offset                                          │   │
//! │  │ data_size, checksum                                       │   │
//! │  └──────────────────────────────────────────────────────────┘   │
//! ├─────────────────────────────────────────────────────────────────┤
//! │  Bins (содержат cells)                                          │
//! │  ┌──────────────────────────────────────────────────────────┐   │
//! │  │  Cell: size (i32, отрицательный = allocated) + data      │   │
//! │  │  ┌────────────────────────────────────────────────────┐  │   │
//! │  │  │ KeyNode (nk) - ключ реестра                        │  │   │
//! │  │  │ KeyValue (vk) - значение реестра                   │  │   │
//! │  │  │ SubkeysList (lf/lh/li/ri) - списки подключей       │  │   │
//! │  │  │ ValuesList - список значений                       │  │   │
//! │  │  │ BigData (db) - большие данные (>16KB)              │  │   │
//! │  │  └────────────────────────────────────────────────────┘  │   │
//! │  └──────────────────────────────────────────────────────────┘   │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Использование
//!
//! ## Чтение hive
//!
//! ```ignore
//! use hive::{Hive, Result};
//!
//! fn read_service_group_order(data: &[u8]) -> Result<()> {
//!     let hive = Hive::new(data)?;
//!     let root = hive.root_key_node()?;
//!     
//!     // Навигация по пути
//!     if let Some(key) = root.subpath("ControlSet001\\Control\\ServiceGroupOrder")? {
//!         // Чтение значения
//!         if let Some(value) = key.value("List")? {
//!             let data_type = value.data_type()?;
//!             let raw_data = value.data()?;
//!             // ...
//!         }
//!     }
//!     Ok(())
//! }
//! ```
//!
//! ## Создание hive (feature = "write")
//!
//! ```ignore
//! use hive::{HiveBuilder, Result};
//!
//! fn create_system_hive() -> Result<Vec<u8>> {
//!     let mut builder = HiveBuilder::new("SYSTEM")?;
//!     
//!     // Создание структуры ключей
//!     let control_set = builder.create_key("ControlSet001")?;
//!     let control = builder.create_subkey(&control_set, "Control")?;
//!     
//!     // Добавление значений
//!     builder.set_dword(&control, "CurrentControlSet", 1)?;
//!     
//!     // Сохранение
//!     builder.build()
//! }
//! ```
//!
//! # Features
//!
//! - `alloc` - включает поддержку динамической памяти (Vec, String)
//! - `write` - включает полную поддержку записи (создание ключей, значений)
//!
//! # Отступления от архитектуры Windows NT
//!
//! - Упрощенная валидация - не проверяются все поля заголовка
//! - Нет поддержки dirty pages и transaction log
//! - Нет поддержки volatile ключей (только очистка счетчика)
//! - Security descriptors читаются, но не валидируются
//!
//! Источники:
//! - MSDN (NT6.1 / Windows 7): приоритетный источник для API и структур
//! - Windows registry file format specification (Maxim Suhanov)
//! - ReactOS cmlib: docs/ref/reactos/sdk/lib/cmlib (источник для анализа)

#![cfg_attr(not(any(test, feature = "std")), no_std)]
#![forbid(unsafe_code)]

#[cfg(feature = "alloc")]
extern crate alloc;

// Для тестов используем std alloc
#[cfg(all(test, not(feature = "alloc")))]
extern crate alloc;

// =============================================================================
// Макросы
// =============================================================================

/// Макрос для упрощения обработки ошибок в итераторах.
/// Возвращает `Some(Err(e))` при ошибке.
macro_rules! iter_try {
    ($e:expr) => {
        match $e {
            Ok(x) => x,
            Err(e) => return Some(Err(e)),
        }
    };
}

// =============================================================================
// Модули
// =============================================================================

mod big_data;
mod cell;
mod error;
mod hive;
mod key_node;
mod key_value;
mod string;
mod subkeys;

// =============================================================================
// Публичный API
// =============================================================================

pub use crate::big_data::BigDataSlices;
pub use crate::error::HiveError;
pub use crate::error::Result;
pub use crate::hive::Hive;
pub use crate::hive::HiveMinorVersion;
pub use crate::key_node::KeyNode;
pub use crate::key_value::KeyValue;
pub use crate::key_value::KeyValueData;
pub use crate::key_value::KeyValueDataType;
pub use crate::string::HiveString;
pub use crate::subkeys::SubKeyNodes;

#[cfg(feature = "write")]
pub mod builder;

#[cfg(feature = "write")]
pub use crate::builder::HiveBuilder;
#[cfg(feature = "write")]
pub use crate::builder::KeyHandle;
