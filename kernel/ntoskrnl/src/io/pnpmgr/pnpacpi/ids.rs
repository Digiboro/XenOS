//! ACPI Device ID Parsing
//!
//! Утилиты для работы с ACPI идентификаторами устройств:
//! - _HID (Hardware ID)
//! - _CID (Compatible IDs)
//! - _UID (Unique ID)
//! - _ADR (Address)
//!
//! # Форматы ACPI HID
//!
//! 1. EISA ID (legacy): 7 символов, например "PNP0A03"
//!    Формат: XXXNNNN (3 буквы + 4 hex цифры)
//!    Результат: ACPI\PNP0A03
//!
//! 2. ACPI ID (modern): Произвольная строка, например "MSFT0001"
//!    Результат: ACPI\MSFT0001
//!
//! 3. Integer EISA ID: Упакованный 32-bit формат (legacy BIOS)
//!    Биты [0:15] = compressed vendor (3 буквы → 15 бит)
//!    Биты [16:31] = product ID (4 hex цифры → 16 бит)

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// Максимальная длина HardwareID
pub const ACPI_HID_MAX_LEN: usize = 64;

/// Преобразует EISA ID из 32-bit packed формата в строку
///
/// EISA ID — это 32-битное число, где:
/// - Биты 0-4: Первая буква - 'A' + 1
/// - Биты 5-9: Вторая буква - 'A' + 1
/// - Биты 10-14: Третья буква - 'A' + 1
/// - Биты 16-31: Product ID (в BCD формате)
///
/// Пример: 0x030AD041 → "PNP0A03"
pub fn eisa_id_to_string(eisa_id: u32) -> String {
    // EISA ID хранится в little-endian формате
    // Нужно сначала swap bytes для правильного декодирования
    let swapped = eisa_id.swap_bytes();

    let vendor_bytes = (swapped >> 16) as u16;
    let product_id = swapped as u16;

    // Декодируем 3-буквенный vendor code
    // Каждая буква закодирована 5 битами: значение 1-26 соответствует A-Z
    let c1 = ((vendor_bytes >> 10) & 0x1F) as u8;
    let c2 = ((vendor_bytes >> 5) & 0x1F) as u8;
    let c3 = (vendor_bytes & 0x1F) as u8;

    let char1 = (b'A' - 1 + c1) as char;
    let char2 = (b'A' - 1 + c2) as char;
    let char3 = (b'A' - 1 + c3) as char;

    // Product ID — 4 hex цифры
    let mut result = String::with_capacity(7);
    result.push(char1);
    result.push(char2);
    result.push(char3);

    // Добавляем product ID как 4 hex цифры
    for i in (0..4).rev() {
        let nibble = ((product_id >> (i * 4)) & 0xF) as u8;
        let hex_char = if nibble < 10 {
            (b'0' + nibble) as char
        } else {
            (b'A' + nibble - 10) as char
        };
        result.push(hex_char);
    }

    result
}

/// Проверяет, является ли строка валидным EISA/PNP ID
///
/// Валидный формат: XXXNNNN (3 буквы + 4 hex цифры)
pub fn is_valid_eisa_id(s: &str) -> bool {
    if s.len() != 7 {
        return false;
    }

    let bytes = s.as_bytes();

    // Первые 3 символа — заглавные буквы
    for i in 0..3 {
        if !bytes[i].is_ascii_uppercase() {
            return false;
        }
    }

    // Последние 4 символа — hex цифры
    for i in 3..7 {
        if !bytes[i].is_ascii_hexdigit() {
            return false;
        }
    }

    true
}

/// Формирует PnP Hardware ID из ACPI _HID
///
/// Добавляет префикс "ACPI\\" к HID.
pub fn format_hardware_id(hid: &str) -> String {
    let mut result = String::with_capacity(5 + hid.len());
    result.push_str("ACPI\\");
    result.push_str(hid);
    result
}

/// Формирует Instance ID из ACPI _UID
///
/// Если UID не указан, возвращает "0".
pub fn format_instance_id(uid: Option<&str>) -> String {
    match uid {
        Some(s) if !s.is_empty() => String::from(s),
        _ => String::from("0"),
    }
}

/// Формирует полный Instance Path для ACPI устройства
///
/// Формат: ACPI\{HID}\{UID}
/// Пример: ACPI\PNP0A03\0
pub fn format_instance_path(hid: &str, uid: Option<&str>) -> String {
    let uid_str = format_instance_id(uid);
    let mut result = String::with_capacity(6 + hid.len() + uid_str.len());
    result.push_str("ACPI\\");
    result.push_str(hid);
    result.push('\\');
    result.push_str(&uid_str);
    result
}

/// Формирует Compatible IDs из ACPI _CID
///
/// Возвращает список строк в формате "ACPI\XXXXX".
pub fn format_compatible_ids(cids: &[String]) -> Vec<String> {
    cids.iter()
        .map(|cid| {
            let mut result = String::with_capacity(5 + cid.len());
            result.push_str("ACPI\\");
            result.push_str(cid);
            result
        })
        .collect()
}

/// Известные ACPI HID для bus устройств
pub mod known_hids {
    /// PCI Host Bridge
    pub const PCI_HOST_BRIDGE: &str = "PNP0A03";

    /// PCI Express Host Bridge
    pub const PCIE_HOST_BRIDGE: &str = "PNP0A08";

    /// System Board
    pub const SYSTEM_BOARD: &str = "PNP0C01";

    /// Motherboard Resources
    pub const MOTHERBOARD_RESOURCES: &str = "PNP0C02";

    /// PCI Interrupt Link Device
    pub const PCI_INTERRUPT_LINK: &str = "PNP0C0F";

    /// HPET (High Precision Event Timer)
    pub const HPET: &str = "PNP0103";

    /// Real Time Clock
    pub const RTC: &str = "PNP0B00";

    /// System Timer
    pub const TIMER: &str = "PNP0100";

    /// DMA Controller
    pub const DMA_CONTROLLER: &str = "PNP0200";

    /// Programmable Interrupt Controller
    pub const PIC: &str = "PNP0000";

    /// ACPI Power Button
    pub const POWER_BUTTON: &str = "PNP0C0C";

    /// ACPI Sleep Button
    pub const SLEEP_BUTTON: &str = "PNP0C0E";

    /// ACPI Lid Device
    pub const LID: &str = "PNP0C0D";

    /// ACPI Processor
    pub const PROCESSOR: &str = "ACPI0007";

    /// Generic Container Device
    pub const CONTAINER: &str = "PNP0A05";

    /// Проверяет, является ли HID PCI Host Bridge
    pub fn is_pci_bridge(hid: &str) -> bool {
        hid == PCI_HOST_BRIDGE || hid == PCIE_HOST_BRIDGE
    }

    /// Проверяет, является ли HID системным ресурсом (не enumerable)
    pub fn is_system_resource(hid: &str) -> bool {
        matches!(
            hid,
            SYSTEM_BOARD | MOTHERBOARD_RESOURCES | PIC | DMA_CONTROLLER | RTC | TIMER
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_eisa_id_to_string() {
        // PNP0A03 in packed EISA format (little-endian)
        assert_eq!(eisa_id_to_string(0x030AD041), "PNP0A03");
    }

    #[test]
    fn test_is_valid_eisa_id() {
        assert!(is_valid_eisa_id("PNP0A03"));
        assert!(is_valid_eisa_id("PNP0A08"));
        assert!(!is_valid_eisa_id("PNP0A0")); // too short
        assert!(!is_valid_eisa_id("pnp0a03")); // lowercase
        assert!(!is_valid_eisa_id("1NP0A03")); // digit in vendor
    }

    #[test]
    fn test_format_hardware_id() {
        assert_eq!(format_hardware_id("PNP0A03"), "ACPI\\PNP0A03");
        assert_eq!(format_hardware_id("MSFT0001"), "ACPI\\MSFT0001");
    }

    #[test]
    fn test_format_instance_path() {
        assert_eq!(format_instance_path("PNP0A03", None), "ACPI\\PNP0A03\\0");
        assert_eq!(
            format_instance_path("PNP0A03", Some("1")),
            "ACPI\\PNP0A03\\1"
        );
    }
}

