//! ACPI Namespace Enumeration
//!
//! Перебор ACPI namespace для обнаружения устройств и создания PDO.
//!
//! # Процесс enumeration
//!
//! 1. Traverse namespace начиная с \\_SB (System Bus)
//! 2. Для каждого Device/Processor/ThermalZone:
//!    - Проверить _STA (если отсутствует, устройство считается present)
//!    - Получить _HID, _CID, _UID, _ADR
//!    - Создать PDO и devnode
//! 3. Рекурсивно обработать дочерние устройства
//!
//! # Особые случаи
//!
//! - PCI Host Bridge (PNP0A03/PNP0A08): дочерние устройства обрабатываются PCI bus driver
//! - Processor: имеет _UID вместо _HID
//! - ThermalZone: специальная обработка

extern crate alloc;

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use core::ptr;
use core::str::FromStr;

use crate::acpi::aml::namespace::{AmlName, NamespaceLevelKind};
use crate::acpi::aml::object::Object;
use crate::acpi::aml::Interpreter;
use crate::acpi::Handler;
use crate::io::pnpmgr::devnode::{pip_allocate_device_node, pi_insert_dev_node, DEVICE_NODE};
use crate::io::pnpmgr::state::{PnpDevnodeState, DNF_ENUMERATED};
use crate::kd::dbg_print;
use crate::nt::ntstatus::*;
use crate::nt::NTSTATUS;

use super::device::{AcpiDeviceFlags, AcpiDeviceInfo, AcpiDeviceStatus};
use super::ids::{eisa_id_to_string, format_instance_path};

/// Результат enumeration ACPI namespace
pub struct AcpiEnumerationResult {
    /// Список обнаруженных устройств
    pub devices: Vec<AcpiDeviceInfo>,
    /// Количество PCI Host Bridges
    pub pci_bridge_count: u32,
    /// Ошибки при enumeration (non-fatal)
    pub errors: Vec<String>,
}

impl AcpiEnumerationResult {
    pub fn new() -> Self {
        Self {
            devices: Vec::new(),
            pci_bridge_count: 0,
            errors: Vec::new(),
        }
    }
}

impl Default for AcpiEnumerationResult {
    fn default() -> Self {
        Self::new()
    }
}

/// Enumerate ACPI namespace и собрать информацию об устройствах
///
/// Эта функция не создает PDO — только собирает информацию.
/// Создание PDO выполняется в `acpi_create_device_nodes`.
pub fn acpi_enumerate_namespace<H: Handler>(
    interpreter: &Interpreter<H>,
) -> AcpiEnumerationResult {
    let mut result = AcpiEnumerationResult::new();

    dbg_print("   [ACPI] Enumerating ACPI namespace...\n");

    // Клонируем namespace для traverse
    // TODO: Это неоптимально, но необходимо из-за borrowing rules
    let mut namespace = interpreter.namespace.lock().clone();

    let traverse_result = namespace.traverse(|path, level| {
        // Обрабатываем только устройства
        match level.kind {
            NamespaceLevelKind::Device
            | NamespaceLevelKind::Processor
            | NamespaceLevelKind::ThermalZone => {
                // Пытаемся получить информацию об устройстве
                match get_device_info(interpreter, path, level.kind) {
                    Ok(Some(info)) => {
                        if super::ids::known_hids::is_pci_bridge(&info.hardware_id) {
                            result.pci_bridge_count += 1;
                        }
                        result.devices.push(info);
                    }
                    Ok(None) => {
                        // Устройство не present или не нужно создавать PDO
                    }
                    Err(e) => {
                        let error_msg =
                            alloc::format!("Error enumerating {}: {:?}", path, e);
                        result.errors.push(error_msg);
                    }
                }
                // Продолжаем traverse дочерних
                Ok(true)
            }
            NamespaceLevelKind::Scope => {
                // Scope — просто контейнер, продолжаем traverse
                Ok(true)
            }
            _ => {
                // MethodLocals, etc — пропускаем
                Ok(false)
            }
        }
    });

    if let Err(e) = traverse_result {
        let error_msg = alloc::format!("Namespace traverse error: {:?}", e);
        result.errors.push(error_msg);
    }

    dbg_print("   [ACPI] Found ");
    crate::kd::dbg_print_num(result.devices.len() as u64);
    dbg_print(" ACPI devices, ");
    crate::kd::dbg_print_num(result.pci_bridge_count as u64);
    dbg_print(" PCI bridges\n");

    result
}

/// Получить информацию об устройстве из AML namespace
fn get_device_info<H: Handler>(
    interpreter: &Interpreter<H>,
    path: &AmlName,
    kind: NamespaceLevelKind,
) -> Result<Option<AcpiDeviceInfo>, crate::acpi::aml::AmlError> {
    // 1. Проверяем _STA
    let status = get_device_status(interpreter, path)?;
    if !status.is_available() {
        return Ok(None);
    }

    // 2. Получаем _HID
    let hardware_id = match kind {
        NamespaceLevelKind::Processor => {
            // Processor использует ACPI0007 по умолчанию
            String::from("ACPI0007")
        }
        NamespaceLevelKind::ThermalZone => {
            // ThermalZone использует специальный ID
            String::from("THERMALZONE")
        }
        _ => {
            // Device — получаем _HID
            match get_hid(interpreter, path)? {
                Some(hid) => hid,
                None => {
                    // Устройство без _HID — проверяем _ADR
                    // Если есть _ADR, это PCI-style устройство (адрес вместо ID)
                    // Для таких устройств мы не создаем ACPI PDO — их обработает PCI bus
                    return Ok(None);
                }
            }
        }
    };

    let mut info = AcpiDeviceInfo::new(path.clone(), hardware_id);
    info.status = status;

    // 3. Получаем _CID (опционально)
    info.compatible_ids = get_cid(interpreter, path)?;

    // 4. Получаем _UID (опционально)
    info.unique_id = get_uid(interpreter, path)?;

    // 5. Получаем _ADR (опционально)
    info.address = get_adr(interpreter, path)?;

    Ok(Some(info))
}

/// Получить _STA устройства
fn get_device_status<H: Handler>(
    interpreter: &Interpreter<H>,
    path: &AmlName,
) -> Result<AcpiDeviceStatus, crate::acpi::aml::AmlError> {
    let sta_path = AmlName::from_str("_STA").unwrap().resolve(path)?;

    match interpreter.evaluate_if_present(sta_path, alloc::vec![])? {
        Some(result) => {
            if let Object::Integer(value) = &*result {
                Ok(AcpiDeviceStatus::from_sta(*value))
            } else {
                // _STA вернул не Integer — считаем устройство present
                Ok(AcpiDeviceStatus::default_present())
            }
        }
        None => {
            // _STA отсутствует — устройство считается present по умолчанию
            Ok(AcpiDeviceStatus::default_present())
        }
    }
}

/// Получить _HID устройства
fn get_hid<H: Handler>(
    interpreter: &Interpreter<H>,
    path: &AmlName,
) -> Result<Option<String>, crate::acpi::aml::AmlError> {
    let hid_path = AmlName::from_str("_HID").unwrap().resolve(path)?;

    match interpreter.evaluate_if_present(hid_path, alloc::vec![])? {
        Some(result) => {
            let hid = match &*result {
                Object::Integer(value) => {
                    // EISA ID в packed формате
                    eisa_id_to_string(*value as u32)
                }
                Object::String(s) => s.clone(),
                _ => return Ok(None),
            };
            Ok(Some(hid))
        }
        None => Ok(None),
    }
}

/// Получить _CID устройства (может быть Package или одиночное значение)
fn get_cid<H: Handler>(
    interpreter: &Interpreter<H>,
    path: &AmlName,
) -> Result<Vec<String>, crate::acpi::aml::AmlError> {
    let cid_path = AmlName::from_str("_CID").unwrap().resolve(path)?;

    match interpreter.evaluate_if_present(cid_path, alloc::vec![])? {
        Some(result) => {
            let mut cids = Vec::new();

            match &*result {
                Object::Integer(value) => {
                    // Одиночный EISA ID
                    cids.push(eisa_id_to_string(*value as u32));
                }
                Object::String(s) => {
                    // Одиночная строка
                    cids.push(s.clone());
                }
                Object::Package(elements) => {
                    // Package из ID
                    for elem in elements {
                        match &**elem {
                            Object::Integer(value) => {
                                cids.push(eisa_id_to_string(*value as u32));
                            }
                            Object::String(s) => {
                                cids.push(s.clone());
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }

            Ok(cids)
        }
        None => Ok(Vec::new()),
    }
}

/// Получить _UID устройства
fn get_uid<H: Handler>(
    interpreter: &Interpreter<H>,
    path: &AmlName,
) -> Result<String, crate::acpi::aml::AmlError> {
    let uid_path = AmlName::from_str("_UID").unwrap().resolve(path)?;

    match interpreter.evaluate_if_present(uid_path, alloc::vec![])? {
        Some(result) => {
            let uid = match &*result {
                Object::Integer(value) => {
                    // Numeric UID
                    alloc::format!("{}", value)
                }
                Object::String(s) => s.clone(),
                _ => String::new(),
            };
            Ok(uid)
        }
        None => Ok(String::new()),
    }
}

/// Получить _ADR устройства
fn get_adr<H: Handler>(
    interpreter: &Interpreter<H>,
    path: &AmlName,
) -> Result<Option<u64>, crate::acpi::aml::AmlError> {
    let adr_path = AmlName::from_str("_ADR").unwrap().resolve(path)?;

    match interpreter.evaluate_if_present(adr_path, alloc::vec![])? {
        Some(result) => {
            if let Object::Integer(value) = &*result {
                Ok(Some(*value))
            } else {
                Ok(None)
            }
        }
        None => Ok(None),
    }
}

/// Создает devnode для ACPI устройств
///
/// Вызывается после enumeration для создания дерева устройств.
/// Также вызывает AddDevice для драйверов найденных по Hardware ID.
pub unsafe fn acpi_create_device_nodes(
    devices: &[AcpiDeviceInfo],
    parent_node: *mut DEVICE_NODE,
) -> NTSTATUS {
    use super::super::drvload::pnp_process_new_device;

    if parent_node.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    dbg_print("   [ACPI] Creating device nodes for ACPI devices...\n");

    let mut created = 0u32;
    let mut drivers_loaded = 0u32;

    for device in devices {
        // Создаём PDO для устройства
        let pdo = acpi_create_pdo_for_device(device);

        // Выделяем devnode с PDO
        let node = pip_allocate_device_node(pdo);
        if node.is_null() {
            dbg_print("   [ACPI] Failed to allocate device node\n");
            continue;
        }

        // Устанавливаем Instance Path (ACPI\HID\UID или ACPI\HID\0)
        set_device_node_instance_path(node, device);

        // Устанавливаем флаги
        (*node).set_flag(DNF_ENUMERATED);

        // Вставляем в дерево
        pi_insert_dev_node(node, parent_node);

        // Устанавливаем состояние
        super::super::devnode::pi_set_dev_node_state(node, PnpDevnodeState::Initialized);

        created += 1;

        // Пробуем найти драйвер и вызвать AddDevice
        if !pdo.is_null() {
            let hardware_id = alloc::format!("ACPI\\{}", device.hardware_id);
            let status = pnp_process_new_device(&hardware_id, pdo);
            if status >= 0 {
                // Проверяем, был ли создан FDO (AttachedDevice != NULL)
                if !(*pdo).attached_device.is_null() {
                    drivers_loaded += 1;
                    // Помечаем что драйвер добавлен
                    (*node).set_flag(super::super::state::DNF_ADDED);
                }
            }
        }
    }

    dbg_print("   [ACPI] Created ");
    crate::kd::dbg_print_num(created as u64);
    dbg_print(" device nodes, ");
    crate::kd::dbg_print_num(drivers_loaded as u64);
    dbg_print(" drivers loaded\n");

    STATUS_SUCCESS
}

/// Устанавливает Instance Path для devnode
///
/// Формат: ACPI\HID\UID (например: ACPI\PNP0A03\0)
unsafe fn set_device_node_instance_path(node: *mut DEVICE_NODE, device: &AcpiDeviceInfo) {
    use crate::ex::pool::{ex_allocate_pool_with_tag, POOL_TYPE};

    if node.is_null() {
        return;
    }

    // Формируем Instance Path: ACPI\HID\UID
    let uid = if device.unique_id.is_empty() {
        "0"
    } else {
        &device.unique_id
    };

    let path = alloc::format!("ACPI\\{}\\{}", device.hardware_id, uid);

    // Конвертируем в UTF-16
    let utf16_len = path.len();
    let buffer_size = (utf16_len + 1) * 2; // +1 для null terminator

    let buffer = ex_allocate_pool_with_tag(
        POOL_TYPE::NonPagedPool,
        buffer_size,
        u32::from_le_bytes(*b"InPa"),
    ) as *mut u16;

    if buffer.is_null() {
        return;
    }

    // Копируем как UTF-16 (ASCII only для простоты)
    for (i, ch) in path.bytes().enumerate() {
        *buffer.add(i) = ch as u16;
    }
    *buffer.add(utf16_len) = 0; // null terminator

    // Устанавливаем в devnode
    (*node).instance_path.buffer = buffer;
    (*node).instance_path.length = (utf16_len * 2) as u16;
    (*node).instance_path.maximum_length = buffer_size as u16;
}

/// Создаёт PDO для ACPI устройства
///
/// PDO — Physical Device Object, представляет физическое устройство.
/// Создаётся bus enumerator'ом (ACPI driver).
unsafe fn acpi_create_pdo_for_device(device: &AcpiDeviceInfo) -> crate::io::types::PDEVICE_OBJECT {
    use crate::ex::pool::{ex_allocate_pool_with_tag, POOL_TYPE};
    use crate::io::device::{DEVICE_OBJECT, DEVOBJ_EXTENSION};
    use crate::io::types::{DO_BUS_ENUMERATED_DEVICE, FILE_DEVICE_ACPI};
    use crate::nt::{CSHORT, USHORT};
    use super::driver::acpi_get_internal_driver;

    // Выделяем DEVICE_OBJECT (extension будет храниться отдельно)
    let obj_size = DEVICE_OBJECT::SIZE;

    let device_ptr = ex_allocate_pool_with_tag(
        POOL_TYPE::NonPagedPool,
        obj_size,
        u32::from_le_bytes(*b"AcPd"),
    );

    if device_ptr.is_null() {
        return ptr::null_mut();
    }

    // Зануляем
    core::ptr::write_bytes(device_ptr, 0, obj_size);

    let pdo = device_ptr as *mut DEVICE_OBJECT;

    // Инициализируем DEVICE_OBJECT
    (*pdo).r#type = crate::io::types::IO_TYPE_DEVICE as CSHORT;
    (*pdo).size = obj_size as USHORT;
    (*pdo).device_type = FILE_DEVICE_ACPI;
    (*pdo).flags = DO_BUS_ENUMERATED_DEVICE;
    (*pdo).reference_count = 1;
    (*pdo).stack_size = 1; // PDO имеет stack_size = 1
    
    // Устанавливаем internal ACPI driver для обработки IRP
    (*pdo).driver_object = acpi_get_internal_driver();

    // Выделяем и инициализируем DEVOBJ_EXTENSION (для device_node)
    let ext_size = core::mem::size_of::<DEVOBJ_EXTENSION>();
    let ext_ptr = ex_allocate_pool_with_tag(
        POOL_TYPE::NonPagedPool,
        ext_size,
        u32::from_le_bytes(*b"AcEx"),
    );
    
    if !ext_ptr.is_null() {
        core::ptr::write_bytes(ext_ptr, 0, ext_size);
        let ext = ext_ptr as *mut DEVOBJ_EXTENSION;
        (*ext).device_object = pdo;
        (*pdo).device_object_extension = ext;
    }

    pdo
}

/// Вызывает _INI для всех ACPI устройств
///
/// Должно вызываться после создания devnode, но до StartDevice.
pub fn acpi_call_device_ini<H: Handler>(
    interpreter: &Interpreter<H>,
    devices: &[AcpiDeviceInfo],
) {
    dbg_print("   [ACPI] Calling _INI for ACPI devices...\n");

    let mut initialized = 0u32;

    for device in devices {
        let ini_path = match AmlName::from_str("_INI")
            .ok()
            .and_then(|p| p.resolve(&device.acpi_path).ok())
        {
            Some(p) => p,
            None => continue,
        };

        match interpreter.evaluate_if_present(ini_path, alloc::vec![]) {
            Ok(Some(_)) => {
                initialized += 1;
            }
            Ok(None) => {
                // _INI отсутствует — это нормально
            }
            Err(e) => {
                // Ошибка при вызове _INI — логируем, но продолжаем
                dbg_print("   [ACPI] _INI error for ");
                // TODO: print device path
                dbg_print("\n");
            }
        }
    }

    dbg_print("   [ACPI] Initialized ");
    crate::kd::dbg_print_num(initialized as u64);
    dbg_print(" devices via _INI\n");
}

