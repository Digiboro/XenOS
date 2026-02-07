//! ACPI Device Object Management
//!
//! Управление ACPI Device Objects (PDO) и связью с AML namespace.
//!
//! Каждое ACPI устройство в namespace (\\_SB.PCI0, \\_SB.LNKA, etc.)
//! может иметь соответствующий PDO в дереве PnP устройств.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::ptr;

use crate::acpi::aml::namespace::AmlName;
use crate::io::device::DEVICE_OBJECT;
use crate::io::pnpmgr::devnode::DEVICE_NODE;
use crate::nt::PVOID;

/// ACPI Device Extension
///
/// Дополнительные данные для ACPI PDO, хранящиеся в DeviceExtension.
/// Связывает PDO с соответствующим объектом в AML namespace.
#[repr(C)]
pub struct AcpiDeviceExtension {
    /// Путь в AML namespace (например, "\\_SB.PCI0")
    pub acpi_path: AmlName,

    /// Hardware ID (_HID)
    pub hardware_id: String,

    /// Compatible IDs (_CID), может быть пустым
    pub compatible_ids: Vec<String>,

    /// Unique ID (_UID), может быть пустым
    pub unique_id: String,

    /// Address (_ADR) для PCI-style адресации
    /// Для PCI: high word = device, low word = function
    pub address: Option<u64>,

    /// Device Status из _STA
    pub status: AcpiDeviceStatus,

    /// Указатель на devnode (back-reference)
    pub device_node: *mut DEVICE_NODE,

    /// Указатель на родительский ACPI PDO
    pub parent_device: *mut DEVICE_OBJECT,

    /// Флаги устройства
    pub flags: AcpiDeviceFlags,
}

impl AcpiDeviceExtension {
    /// Создает новое расширение для ACPI устройства
    pub fn new(acpi_path: AmlName, hardware_id: String) -> Self {
        Self {
            acpi_path,
            hardware_id,
            compatible_ids: Vec::new(),
            unique_id: String::new(),
            address: None,
            status: AcpiDeviceStatus::default(),
            device_node: ptr::null_mut(),
            parent_device: ptr::null_mut(),
            flags: AcpiDeviceFlags::empty(),
        }
    }

    /// Возвращает полный Instance Path
    pub fn instance_path(&self) -> String {
        let uid = if self.unique_id.is_empty() {
            None
        } else {
            Some(self.unique_id.as_str())
        };
        super::ids::format_instance_path(&self.hardware_id, uid)
    }

    /// Проверяет, является ли устройство PCI Host Bridge
    pub fn is_pci_bridge(&self) -> bool {
        super::ids::known_hids::is_pci_bridge(&self.hardware_id)
    }
}

/// Статус ACPI устройства (из _STA метода)
#[derive(Clone, Copy, Debug, Default)]
pub struct AcpiDeviceStatus {
    /// Устройство присутствует (bit 0)
    pub present: bool,
    /// Устройство включено (bit 1)
    pub enabled: bool,
    /// Показывать в UI (bit 2)
    pub show_in_ui: bool,
    /// Устройство функционирует (bit 3)
    pub functioning: bool,
    /// Батарея присутствует (bit 4, только для PNP0C0A)
    pub battery_present: bool,
}

impl AcpiDeviceStatus {
    /// Парсит статус из 64-bit значения _STA
    pub fn from_sta(sta: u64) -> Self {
        Self {
            present: (sta & 0x01) != 0,
            enabled: (sta & 0x02) != 0,
            show_in_ui: (sta & 0x04) != 0,
            functioning: (sta & 0x08) != 0,
            battery_present: (sta & 0x10) != 0,
        }
    }

    /// Устройство доступно для enumeration (present + enabled + functioning)
    pub fn is_available(&self) -> bool {
        self.present && self.enabled && self.functioning
    }

    /// Дефолтный статус, если _STA отсутствует
    /// По спецификации ACPI, если _STA отсутствует — устройство считается present & functional
    pub fn default_present() -> Self {
        Self {
            present: true,
            enabled: true,
            show_in_ui: true,
            functioning: true,
            battery_present: false,
        }
    }
}

bitflags::bitflags! {
    /// Флаги ACPI устройства
    #[derive(Clone, Copy, Debug, Default)]
    pub struct AcpiDeviceFlags: u32 {
        /// Устройство — bus (имеет дочерние устройства)
        const IS_BUS = 0x0001;
        /// Устройство — PCI Host Bridge
        const IS_PCI_BRIDGE = 0x0002;
        /// _INI был вызван
        const INI_CALLED = 0x0004;
        /// Ресурсы (_CRS) запрошены
        const RESOURCES_QUERIED = 0x0008;
        /// Устройство в процессе enumeration
        const ENUMERATING = 0x0010;
        /// Enumeration завершена
        const ENUMERATED = 0x0020;
        /// Устройство удалено (surprise removal)
        const REMOVED = 0x0040;
    }
}

/// Информация об ACPI устройстве для enumeration
#[derive(Clone, Debug)]
pub struct AcpiDeviceInfo {
    /// Путь в AML namespace
    pub acpi_path: AmlName,
    /// Hardware ID
    pub hardware_id: String,
    /// Compatible IDs
    pub compatible_ids: Vec<String>,
    /// Unique ID
    pub unique_id: String,
    /// Address (_ADR)
    pub address: Option<u64>,
    /// Status (_STA)
    pub status: AcpiDeviceStatus,
}

impl AcpiDeviceInfo {
    /// Создает информацию об устройстве с минимальными данными
    pub fn new(acpi_path: AmlName, hardware_id: String) -> Self {
        Self {
            acpi_path,
            hardware_id,
            compatible_ids: Vec::new(),
            unique_id: String::new(),
            address: None,
            status: AcpiDeviceStatus::default_present(),
        }
    }
}

/// Глобальный список ACPI PDO
///
/// Используется для отслеживания всех созданных ACPI устройств.
pub static mut ACPI_DEVICE_LIST_HEAD: crate::nt::LIST_ENTRY = crate::nt::LIST_ENTRY::new();

/// Спинлок для защиты списка ACPI устройств
pub static mut ACPI_DEVICE_LIST_LOCK: crate::ke::spinlock::KSPIN_LOCK =
    crate::ke::spinlock::KSPIN_LOCK::new();

