//! Модуль для работы с boot-start драйверами
//!
//! Реализует парсинг SYSTEM hive и извлечение списка драйверов для загрузки
//! в соответствии с архитектурой Windows NT 6.1.
//!
//! # Архитектура
//!
//! ```text
//! SYSTEM Hive
//! ├── Select
//! │   └── Default -> 1 (CurrentControlSet)
//! └── ControlSet001
//!     ├── Control
//!     │   └── ServiceGroupOrder
//!     │       └── List: REG_MULTI_SZ (порядок групп)
//!     └── Services
//!         ├── XenTest
//!         │   ├── Start: 0 (SERVICE_BOOT_START)
//!         │   ├── Type: 1 (SERVICE_KERNEL_DRIVER)
//!         │   ├── ErrorControl: 1
//!         │   ├── ImagePath: \SystemRoot\System32\drivers\test.dll
//!         │   ├── Group: (опционально)
//!         │   └── Tag: (опционально)
//!         └── ...
//! ```
//!
//! Источники:
//! - MSDN (NT6.1): Service Registry Keys
//! - ReactOS: docs/ref/reactos/boot/freeldr/freeldr/windows/registry.c

extern crate alloc;

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

use hive::Hive;
use hive::KeyNode;
use hive::KeyValueDataType;

// =============================================================================
// Константы
// =============================================================================

/// SERVICE_BOOT_START - драйвер загружается при старте системы
const SERVICE_BOOT_START: u32 = 0;

/// SERVICE_KERNEL_DRIVER - драйвер ядра
const SERVICE_KERNEL_DRIVER: u32 = 1;

/// SERVICE_FILE_SYSTEM_DRIVER - драйвер файловой системы
const SERVICE_FILE_SYSTEM_DRIVER: u32 = 2;

// =============================================================================
// Структуры данных
// =============================================================================

/// Информация о boot-start драйвере
#[derive(Clone)]
pub struct BootDriverEntry {
    /// Имя сервиса (ключ в Services)
    pub name: String,
    /// ImagePath (путь к .sys/.dll файлу)
    pub image_path: Option<String>,
    /// Start type (0 = BOOT_START)
    pub start: u32,
    /// Service type
    pub service_type: u32,
    /// Error control
    pub error_control: u32,
    /// Group name (для сортировки)
    pub group: Option<String>,
    /// Tag (для сортировки внутри группы)
    pub tag: u32,
}

// =============================================================================
// Основные функции
// =============================================================================

/// Определяет CurrentControlSet из Select\Default
///
/// Возвращает номер ControlSet (обычно 1 или 2)
pub fn get_current_control_set(hive: &Hive) -> Option<u32> {
    let root = hive.root_key_node().ok()?;
    let select_key = root.subkey("Select")?.ok()?;

    // Сначала пробуем Default, затем Current
    if let Some(Ok(value)) = select_key.value("Default") {
        if let Ok(dword) = value.dword_data() {
            return Some(dword);
        }
    }

    if let Some(Ok(value)) = select_key.value("Current") {
        if let Ok(dword) = value.dword_data() {
            return Some(dword);
        }
    }

    None
}

/// Получает список boot-start драйверов из SYSTEM hive
///
/// Согласно NT 6.1:
/// - Ищем в ControlSet00N\Services
/// - Start = 0 (SERVICE_BOOT_START)
/// - Type = 1 (SERVICE_KERNEL_DRIVER) или 2 (SERVICE_FILE_SYSTEM_DRIVER)
/// - Обязательно наличие ImagePath
pub fn get_boot_drivers(hive: &Hive, control_set: u32) -> Vec<BootDriverEntry> {
    let mut drivers = Vec::new();

    let root = match hive.root_key_node() {
        Ok(r) => r,
        Err(_) => return drivers,
    };

    // Формируем путь к Services
    let cs_path = alloc::format!("ControlSet{:03}\\Services", control_set);

    let services_key = match root.subpath(&cs_path) {
        Some(Ok(k)) => k,
        _ => return drivers,
    };

    // Итерируем по subkeys (сервисам)
    let subkeys = match services_key.subkeys() {
        Some(Ok(s)) => s,
        _ => return drivers,
    };

    for service_result in subkeys {
        let service_key = match service_result {
            Ok(k) => k,
            Err(_) => continue,
        };

        // Читаем имя сервиса
        let name = match service_key.name() {
            Ok(n) => n.to_string(),
            Err(_) => continue,
        };

        // Читаем Start (обязательно)
        let start = match read_dword(&service_key, "Start") {
            Some(v) => v,
            None => continue,
        };

        // Проверяем: Start = 0 (BOOT_START)
        if start != SERVICE_BOOT_START {
            continue;
        }

        // Читаем Type (обязательно)
        let service_type = match read_dword(&service_key, "Type") {
            Some(v) => v,
            None => continue,
        };

        // Проверяем: Type = 1 или 2 (kernel driver или fs driver)
        if service_type != SERVICE_KERNEL_DRIVER && service_type != SERVICE_FILE_SYSTEM_DRIVER {
            continue;
        }

        // Читаем ImagePath (обязательно для NT 6.1)
        let image_path = read_string(&service_key, "ImagePath");
        if image_path.is_none() {
            continue;
        }

        // Читаем остальные поля
        let error_control = read_dword(&service_key, "ErrorControl").unwrap_or(1);
        let group = read_string(&service_key, "Group");
        let tag = read_dword(&service_key, "Tag").unwrap_or(0);

        drivers.push(BootDriverEntry {
            name,
            image_path,
            start,
            service_type,
            error_control,
            group,
            tag,
        });
    }

    drivers
}

/// Сортирует драйверы по ServiceGroupOrder и Tag
///
/// Порядок:
/// 1. Группы сортируются по ServiceGroupOrder\List
/// 2. Внутри группы - по Tag
/// 3. Драйверы без группы - в конец
pub fn sort_boot_drivers(hive: &Hive, control_set: u32, drivers: &mut [BootDriverEntry]) {
    // Получаем порядок групп
    let group_order = get_service_group_order(hive, control_set);

    drivers.sort_by(|a, b| {
        // Сначала сортируем по группе
        let a_group_idx = a
            .group
            .as_ref()
            .and_then(|g| group_order.iter().position(|x| x.eq_ignore_ascii_case(g)))
            .unwrap_or(usize::MAX);
        let b_group_idx = b
            .group
            .as_ref()
            .and_then(|g| group_order.iter().position(|x| x.eq_ignore_ascii_case(g)))
            .unwrap_or(usize::MAX);

        match a_group_idx.cmp(&b_group_idx) {
            core::cmp::Ordering::Equal => {
                // Внутри группы - по Tag
                a.tag.cmp(&b.tag)
            },
            other => other,
        }
    });
}

/// Получает ServiceGroupOrder\List
fn get_service_group_order(hive: &Hive, control_set: u32) -> Vec<String> {
    let mut groups = Vec::new();

    let root = match hive.root_key_node() {
        Ok(r) => r,
        Err(_) => return groups,
    };

    let path = alloc::format!("ControlSet{:03}\\Control\\ServiceGroupOrder", control_set);

    let key = match root.subpath(&path) {
        Some(Ok(k)) => k,
        _ => return groups,
    };

    let value = match key.value("List") {
        Some(Ok(v)) => v,
        _ => return groups,
    };

    // Проверяем тип - должен быть REG_MULTI_SZ
    if let Ok(KeyValueDataType::RegMultiSZ) = value.data_type() {
        if let Ok(data) = value.data() {
            if let hive::KeyValueData::Small(bytes) = data {
                groups = parse_multi_sz(bytes);
            }
        }
    }

    groups
}

// =============================================================================
// Вспомогательные функции
// =============================================================================

/// Читает DWORD значение из ключа
fn read_dword(key: &KeyNode, name: &str) -> Option<u32> {
    let value = key.value(name)?.ok()?;
    value.dword_data().ok()
}

/// Читает строковое значение из ключа (REG_SZ, REG_EXPAND_SZ)
fn read_string(key: &KeyNode, name: &str) -> Option<String> {
    let value = key.value(name)?.ok()?;

    let data_type = value.data_type().ok()?;
    match data_type {
        KeyValueDataType::RegSZ | KeyValueDataType::RegExpandSZ => {
            let data = value.data().ok()?;
            if let hive::KeyValueData::Small(bytes) = data {
                Some(utf16le_to_string(bytes))
            } else {
                None
            }
        },
        _ => None,
    }
}

/// Парсит REG_MULTI_SZ (массив null-terminated UTF-16LE строк)
fn parse_multi_sz(bytes: &[u8]) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = Vec::new();

    for chunk in bytes.chunks_exact(2) {
        let code_unit = u16::from_le_bytes([chunk[0], chunk[1]]);

        if code_unit == 0 {
            if !current.is_empty() {
                result.push(utf16_chars_to_string(&current));
                current.clear();
            } else {
                // Двойной NUL - конец списка
                break;
            }
        } else {
            current.push(code_unit);
        }
    }

    result
}

/// Конвертирует UTF-16LE байты в String
fn utf16le_to_string(bytes: &[u8]) -> String {
    let mut chars = Vec::new();

    for chunk in bytes.chunks_exact(2) {
        let code_unit = u16::from_le_bytes([chunk[0], chunk[1]]);
        if code_unit == 0 {
            break;
        }
        chars.push(code_unit);
    }

    utf16_chars_to_string(&chars)
}

/// Конвертирует массив UTF-16 code units в String
fn utf16_chars_to_string(chars: &[u16]) -> String {
    chars
        .iter()
        .filter_map(|&c| char::from_u32(c as u32))
        .collect()
}
