//! PnP Driver Loading
//!
//! Загрузка драйверов по Hardware ID для PnP устройств.
//!
//! # Архитектура
//!
//! ```text
//!              ┌────────────────┐
//!              │ ACPI enumerator│
//!              │ находит PNP0A03│
//!              └───────┬────────┘
//!                      │ Hardware ID
//!                      ▼
//!              ┌────────────────┐
//!              │ PnP Driver DB  │
//!              │ (boot drivers) │
//!              └───────┬────────┘
//!                      │ найден pci.sys
//!                      ▼
//!              ┌────────────────┐
//!              │ AddDevice()    │
//!              │ pci!PciAddDevice│
//!              └───────┬────────┘
//!                      │
//!                      ▼
//!              ┌────────────────┐
//!              │ FDO создан и   │
//!              │ присоединён    │
//!              └────────────────┘
//! ```
//!
//! Источники:
//! - ReactOS: ntoskrnl/io/pnpmgr/pnpload.c
//! - Windows 7: ntos/io/pnpmgr/

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::ptr;

use crate::io::driver::DRIVER_OBJECT;
use crate::io::types::PDEVICE_OBJECT;
use crate::kd::dbg_print;
use crate::nt::ntstatus::*;
use crate::nt::{NTSTATUS, UNICODE_STRING};

// =============================================================================
// Driver Database Entry
// =============================================================================

/// Запись в базе данных драйверов
///
/// Связывает Hardware ID с драйвером
#[derive(Clone)]
pub struct DriverDatabaseEntry {
    /// Hardware IDs, которые обрабатывает драйвер (null-separated)
    pub hardware_ids: Vec<String>,
    /// Указатель на DRIVER_OBJECT
    pub driver_object: *mut DRIVER_OBJECT,
    /// Имя драйвера
    pub driver_name: String,
}

impl DriverDatabaseEntry {
    pub fn new(driver_name: &str, driver_object: *mut DRIVER_OBJECT) -> Self {
        Self {
            hardware_ids: Vec::new(),
            driver_object,
            driver_name: String::from(driver_name),
        }
    }

    /// Добавляет Hardware ID
    pub fn add_hardware_id(&mut self, hid: &str) {
        self.hardware_ids.push(String::from(hid));
    }

    /// Проверяет, обрабатывает ли драйвер данный Hardware ID
    pub fn matches_hardware_id(&self, hid: &str) -> bool {
        for h in &self.hardware_ids {
            if h.eq_ignore_ascii_case(hid) {
                return true;
            }
        }
        false
    }
}

// =============================================================================
// Driver Database
// =============================================================================

/// База данных зарегистрированных драйверов
static mut DRIVER_DATABASE: Vec<DriverDatabaseEntry> = Vec::new();

/// Регистрирует драйвер для обработки указанных Hardware IDs
///
/// Вызывается при загрузке boot drivers.
pub unsafe fn pnp_register_driver_for_hardware_ids(
    driver_name: &str,
    driver_object: *mut DRIVER_OBJECT,
    hardware_ids: &[&str],
) {
    let mut entry = DriverDatabaseEntry::new(driver_name, driver_object);
    for hid in hardware_ids {
        entry.add_hardware_id(hid);
    }

    dbg_print("   [PnP] Registered driver ");
    dbg_print(driver_name);
    dbg_print(" for HIDs: ");
    for (i, hid) in hardware_ids.iter().enumerate() {
        if i > 0 {
            dbg_print(", ");
        }
        dbg_print(hid);
    }
    dbg_print("\n");

    // SAFETY: Мы в однопоточной фазе инициализации
    unsafe {
        let db = &raw mut DRIVER_DATABASE;
        (*db).push(entry);
    }
}

/// Регистрирует filter driver (без hardware IDs)
///
/// Filter drivers не привязаны к конкретным hardware IDs,
/// они attaches к существующему device stack.
pub unsafe fn pnp_register_filter_driver(
    driver_name: &str,
    driver_object: *mut DRIVER_OBJECT,
) {
    let entry = DriverDatabaseEntry::new(driver_name, driver_object);

    dbg_print("   [PnP] Registered filter driver: ");
    dbg_print(driver_name);
    dbg_print("\n");

    // SAFETY: Мы в однопоточной фазе инициализации
    unsafe {
        let db = &raw mut DRIVER_DATABASE;
        (*db).push(entry);
    }
}

/// Находит драйвер по Hardware ID
///
/// # Arguments
/// * `hardware_id` - Hardware ID устройства (например, "ACPI\\PNP0A03")
///
/// # Returns
/// Указатель на DRIVER_OBJECT или NULL
pub unsafe fn pnp_find_driver_for_hardware_id(hardware_id: &str) -> *mut DRIVER_OBJECT {
    // SAFETY: Поиск в базе драйверов
    // Ищем точное совпадение с полным Hardware ID (включая префикс)
    unsafe {
        let db = &raw const DRIVER_DATABASE;
        for entry in &(*db) {
            if entry.matches_hardware_id(hardware_id) {
                return entry.driver_object;
            }
        }
    }

    ptr::null_mut()
}

// =============================================================================
// Built-in Driver Mappings
// =============================================================================

/// Известные Hardware IDs для PCI bus driver
pub const PCI_HARDWARE_IDS: &[&str] = &[
    "ACPI\\PNP0A03", // PCI Host Bridge
    "ACPI\\PNP0A08", // PCI Express Host Bridge
];

/// Известные Hardware IDs для ACPI bus driver
pub const ACPI_HARDWARE_IDS: &[&str] = &[
    "PNP0C0C", // ACPI Power Button
    "PNP0C0D", // ACPI Lid Device
    "PNP0C0E", // ACPI Sleep Button
];

/// Известные Hardware IDs для AHCI driver
pub const AHCI_HARDWARE_IDS: &[&str] = &[
    "PCI\\CC_010601",           // AHCI SATA Controller (Class Code)
    "PCI\\VEN_8086&DEV_2922",   // Intel ICH9 AHCI
    "PCI\\VEN_8086&DEV_2923",   // Intel ICH9 AHCI (variant)
    "PCI\\VEN_8086&DEV_2681",   // Intel ICH6 AHCI
    "PCI\\VEN_8086&DEV_27C1",   // Intel ICH7 AHCI
    "PCI\\VEN_8086&DEV_27C5",   // Intel ICH7 AHCI (variant)
    "PCI\\VEN_8086&DEV_2821",   // Intel ICH8 AHCI
    "PCI\\VEN_8086&DEV_2829",   // Intel ICH8 AHCI (variant)
];

/// Известные Hardware IDs для Disk driver (Storage Class)
pub const DISK_HARDWARE_IDS: &[&str] = &[
    "SCSI\\Disk",               // Generic SCSI Disk
    "SCSI\\DiskQEMU_HARDDISK",  // QEMU SATA Disk
];

/// Hardware IDs для Volume Manager (Partition PDOs)
pub const PARTITION_HARDWARE_IDS: &[&str] = &[
    "STORAGE\\Partition",
];

/// Регистрирует встроенные маппинги драйверов
///
/// Вызывается после загрузки boot drivers.
pub unsafe fn pnp_register_builtin_drivers() {
    dbg_print("   [PnP] Registering built-in driver mappings...\n");

    // TODO: Найти загруженные драйверы и зарегистрировать их
    // Пока это заглушка — реальная реализация будет искать
    // драйверы в LoaderBlock и регистрировать их по известным HID

    dbg_print("   [PnP] Built-in driver mappings registered\n");
}

// =============================================================================
// AddDevice
// =============================================================================

/// Вызывает AddDevice для драйвера и PDO
///
/// Создаёт FDO и присоединяет к device stack.
///
/// # Arguments
/// * `driver_object` - Драйвер для вызова
/// * `pdo` - Physical Device Object (созданный bus enumerator)
///
/// # Returns
/// STATUS_SUCCESS или код ошибки
pub unsafe fn pnp_call_add_device(
    driver_object: *mut DRIVER_OBJECT,
    pdo: PDEVICE_OBJECT,
) -> NTSTATUS {
    if driver_object.is_null() || pdo.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    // Получаем AddDevice callback
    let driver_ext = (*driver_object).driver_extension;
    if driver_ext.is_null() {
        dbg_print("   [PnP] Driver has no extension\n");
        return STATUS_INVALID_DEVICE_REQUEST;
    }

    let add_device = (*driver_ext).add_device;
    if add_device.is_none() {
        dbg_print("   [PnP] Driver has no AddDevice\n");
        return STATUS_INVALID_DEVICE_REQUEST;
    }

    // Вызываем AddDevice
    let add_device_fn = add_device.unwrap();
    let status = add_device_fn(driver_object, pdo);

    if status >= 0 {
        dbg_print("   [PnP] AddDevice succeeded\n");
    } else {
        dbg_print("   [PnP] AddDevice failed\n");
    }

    status
}

// =============================================================================
// PnP Device Start
// =============================================================================

/// Запускает устройство через IRP_MN_START_DEVICE
///
/// Отправляет IRP_MN_START_DEVICE на device stack.
/// Использует boot configuration (QUERY_RESOURCES) для ресурсов.
pub unsafe fn pnp_start_device(device_object: PDEVICE_OBJECT) -> NTSTATUS {
    use crate::io::irp::{io_allocate_irp, io_free_irp, io_get_next_irp_stack_location, io_set_next_irp_stack_location};
    use crate::io::pnp::{IRP_MN_START_DEVICE, IRP_MN_QUERY_RESOURCES};
    use crate::io::types::IRP_MJ_PNP;
    use crate::nt::PVOID;

    if device_object.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    // Получаем верхний объект в стеке (FDO или filter)
    let mut top_device = device_object;
    while !(*top_device).attached_device.is_null() {
        top_device = (*top_device).attached_device;
    }

    dbg_print("   [PnP] Starting device...\n");

    // Сначала запрашиваем boot resources (QUERY_RESOURCES) у PDO
    // Это даёт нам текущие значения BAR и interrupt
    let boot_resources = pnp_query_boot_resources(device_object);
    
    if !boot_resources.is_null() {
        dbg_print("   [PnP] Got boot resources from PDO\n");
    }

    // Аллоцируем IRP для START_DEVICE
    let stack_size = (*top_device).stack_size;
    let irp = io_allocate_irp(stack_size as i8, false);
    if irp.is_null() {
        return STATUS_INSUFFICIENT_RESOURCES;
    }

    // Настраиваем IRP
    (*irp).io_status.status = STATUS_NOT_SUPPORTED;
    (*irp).io_status.information = 0;

    let stack = io_get_next_irp_stack_location(irp);
    if !stack.is_null() {
        (*stack).major_function = IRP_MJ_PNP;
        (*stack).minor_function = IRP_MN_START_DEVICE;
        (*stack).device_object = top_device;
        
        // Передаём boot resources как allocated resources
        // В реальной системе здесь была бы арбитрация, но для boot config
        // мы используем то, что уже настроено BIOS/firmware
        (*stack).parameters.start_device.allocated_resources = boot_resources as PVOID;
        (*stack).parameters.start_device.allocated_resources_translated = boot_resources as PVOID;
    }
    io_set_next_irp_stack_location(irp);

    // Синхронный вызов драйвера
    let driver = (*top_device).driver_object;
    let status = if !driver.is_null() {
        if let Some(func) = (*driver).major_function[IRP_MJ_PNP as usize] {
            func(top_device, irp)
        } else {
            STATUS_NOT_SUPPORTED
        }
    } else {
        STATUS_NOT_SUPPORTED
    };

    if status >= 0 {
        dbg_print("   [PnP] Device started OK\n");
    } else {
        dbg_print("   [PnP] Device start FAILED\n");
    }

    // Не освобождаем boot_resources - они могут использоваться драйвером
    io_free_irp(irp);
    status
}

/// Запрашивает boot resources у PDO через IRP_MN_QUERY_RESOURCES
unsafe fn pnp_query_boot_resources(pdo: PDEVICE_OBJECT) -> *mut u8 {
    use crate::io::irp::{io_allocate_irp, io_free_irp, io_get_next_irp_stack_location, io_set_next_irp_stack_location};
    use crate::io::pnp::IRP_MN_QUERY_RESOURCES;
    use crate::io::types::IRP_MJ_PNP;

    if pdo.is_null() {
        return core::ptr::null_mut();
    }

    // QUERY_RESOURCES идёт напрямую к PDO (не через стек)
    let stack_size = (*pdo).stack_size;
    let irp = io_allocate_irp(stack_size as i8, false);
    if irp.is_null() {
        return core::ptr::null_mut();
    }

    (*irp).io_status.status = STATUS_NOT_SUPPORTED;
    (*irp).io_status.information = 0;

    let stack = io_get_next_irp_stack_location(irp);
    if !stack.is_null() {
        (*stack).major_function = IRP_MJ_PNP;
        (*stack).minor_function = IRP_MN_QUERY_RESOURCES;
        (*stack).device_object = pdo;
    }
    io_set_next_irp_stack_location(irp);

    // Вызываем драйвер PDO напрямую
    let driver = (*pdo).driver_object;
    let status = if !driver.is_null() {
        if let Some(func) = (*driver).major_function[IRP_MJ_PNP as usize] {
            func(pdo, irp)
        } else {
            STATUS_NOT_SUPPORTED
        }
    } else {
        STATUS_NOT_SUPPORTED
    };

    let resources = if status >= 0 && (*irp).io_status.information != 0 {
        (*irp).io_status.information as *mut u8
    } else {
        core::ptr::null_mut()
    };

    io_free_irp(irp);
    resources
}

// =============================================================================
// Integration with ACPI enumeration
// =============================================================================

/// Обрабатывает обнаруженное устройство
///
/// Вызывается после создания PDO для устройства.
/// Ищет драйвер и вызывает AddDevice если найден.
/// Также вызывает AddDevice для зарегистрированных filters.
/// Проверяет состояние devnode чтобы избежать повторной обработки.
pub unsafe fn pnp_process_new_device(
    hardware_id: &str,
    pdo: PDEVICE_OBJECT,
) -> NTSTATUS {
    use super::state::{PnpDevnodeState, DNF_ADDED};
    
    dbg_print("   [PnP] Processing new device: ");
    dbg_print(hardware_id);
    dbg_print("\n");

    // Получаем devnode для проверки состояния
    let devnode = if !pdo.is_null() && !(*pdo).device_object_extension.is_null() {
        (*(*pdo).device_object_extension).device_node as *mut super::devnode::DEVICE_NODE
    } else {
        core::ptr::null_mut()
    };

    // Проверяем, не обработано ли уже устройство
    if !devnode.is_null() {
        let state = (*devnode).state;
        let flags = (*devnode).flags;
        
        // Если AddDevice уже был вызван (DNF_ADDED) или устройство уже стартовано - пропускаем
        if (flags & DNF_ADDED) != 0 || state.is_started() {
            return STATUS_SUCCESS;
        }
    }

    // Ищем основной драйвер (class driver)
    let driver = pnp_find_driver_for_hardware_id(hardware_id);

    if driver.is_null() {
        // Нет драйвера — устройство останется без FDO
        // Это нормально для устройств без драйвера
        dbg_print("   [PnP] No driver found for ");
        dbg_print(hardware_id);
        dbg_print("\n");
        return STATUS_SUCCESS;
    }

    // Вызываем AddDevice для основного драйвера
    let status = pnp_call_add_device(driver, pdo);
    if status < 0 {
        return status;
    }

    // Вызываем AddDevice для filters (если есть)
    // Filters attaches на top of stack и перехватывают IRPs
    pnp_call_device_filters(hardware_id, pdo);

    // Помечаем devnode как обработанный
    if !devnode.is_null() {
        (*devnode).set_flag(DNF_ADDED);
    }

    // Запускаем устройство
    let status = pnp_start_device(pdo);
    if status < 0 {
        return status;
    }

    // После START запрашиваем BusRelations (для bus drivers)
    // Это приведёт к enumeration дочерних устройств
    pnp_query_bus_relations(pdo)
}

/// Device class filters registry
/// Maps class name to filter driver name
static mut DEVICE_CLASS_FILTERS: [Option<(&'static str, &'static str)>; 8] = [
    Some(("SCSI\\Disk", "partmgr")),      // partmgr filters SCSI\Disk
    None, None, None, None, None, None, None,
];

/// Вызывает AddDevice для зарегистрированных filters
///
/// Filters attach на top of device stack после class driver.
#[allow(static_mut_refs)]
unsafe fn pnp_call_device_filters(hardware_id: &str, pdo: PDEVICE_OBJECT) {
    dbg_print("   [PnP] Checking filters for: ");
    dbg_print(hardware_id);
    dbg_print("\n");
    
    // Проверяем generic HID без instance ID
    let generic_id = if let Some(pos) = hardware_id.find('\\') {
        // Берём только первую часть до backslash для класса
        let class_end = hardware_id[pos+1..].find('\\')
            .map(|p| pos + 1 + p)
            .unwrap_or(hardware_id.len());
        &hardware_id[..class_end]
    } else {
        hardware_id
    };
    
    dbg_print("   [PnP] Generic ID: ");
    dbg_print(generic_id);
    dbg_print("\n");
    
    // Ищем filters для этого класса
    for filter_entry in DEVICE_CLASS_FILTERS.iter() {
        if let Some((class_id, filter_name)) = filter_entry {
            // Проверяем совпадение класса
            let matches = generic_id.starts_with(*class_id) || hardware_id.starts_with(*class_id);
            
            if matches {
                dbg_print("   [PnP] Filter match: ");
                dbg_print(filter_name);
                dbg_print(" for ");
                dbg_print(hardware_id);
                dbg_print("\n");
                
                // Ищем filter driver
                if let Some(filter_driver) = pnp_find_driver_by_name(filter_name) {
                    dbg_print("   [PnP] Calling filter AddDevice: ");
                    dbg_print(filter_name);
                    dbg_print("\n");
                    
                    let status = pnp_call_add_device(filter_driver, pdo);
                    if status >= 0 {
                        dbg_print("   [PnP] Filter AddDevice succeeded\n");
                    } else {
                        dbg_print("   [PnP] Filter AddDevice failed\n");
                    }
                } else {
                    dbg_print("   [PnP] WARNING: Filter driver not found: ");
                    dbg_print(filter_name);
                    dbg_print("\n");
                }
            }
        }
    }
}

/// Находит загруженный драйвер по имени
#[allow(static_mut_refs)]
unsafe fn pnp_find_driver_by_name(driver_name: &str) -> Option<*mut DRIVER_OBJECT> {
    // Ищем среди зарегистрированных драйверов
    let search_name = driver_name.to_lowercase();
    
    for entry in DRIVER_DATABASE.iter() {
        // Сравниваем имена - driver_name содержит полное имя типа "pci", "disk" и т.д.
        let entry_name = entry.driver_name.to_lowercase();
        
        if entry_name.contains(&search_name) || search_name.contains(&entry_name) {
            return Some(entry.driver_object);
        }
    }
    None
}

/// Запрашивает BusRelations у устройства для получения дочерних PDO.
pub unsafe fn pnp_query_bus_relations(pdo: PDEVICE_OBJECT) -> NTSTATUS {
    use crate::io::irp::{io_allocate_irp, io_free_irp, io_get_next_irp_stack_location, io_set_next_irp_stack_location};
    use crate::io::pnp::{IRP_MN_QUERY_DEVICE_RELATIONS, DEVICE_RELATIONS, DEVICE_RELATION_TYPE};
    use crate::io::types::IRP_MJ_PNP;
    use super::devnode::{pip_allocate_device_node, pi_insert_dev_node, IOP_ROOT_DEVICE_NODE, pi_set_dev_node_state, pi_set_dev_node_instance_path};
    use super::state::{PnpDevnodeState, DNF_ENUMERATED};

    if pdo.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    // Получаем верхний объект (FDO)
    let mut top_device = pdo;
    while !(*top_device).attached_device.is_null() {
        top_device = (*top_device).attached_device;
    }

    dbg_print("   [PnP] Querying BusRelations...\n");

    let stack_size = (*top_device).stack_size;
    let irp = io_allocate_irp(stack_size as i8, false);
    if irp.is_null() {
        return STATUS_INSUFFICIENT_RESOURCES;
    }

    (*irp).io_status.status = STATUS_NOT_SUPPORTED;
    (*irp).io_status.information = 0;

    let stack = io_get_next_irp_stack_location(irp);
    if !stack.is_null() {
        (*stack).major_function = IRP_MJ_PNP;
        (*stack).minor_function = IRP_MN_QUERY_DEVICE_RELATIONS;
        (*stack).parameters.query_device_relations.relation_type = DEVICE_RELATION_TYPE::BusRelations;
        (*stack).device_object = top_device;
    }
    io_set_next_irp_stack_location(irp);

    // Вызываем драйвер
    let driver = (*top_device).driver_object;
    let status = if !driver.is_null() {
        if let Some(func) = (*driver).major_function[IRP_MJ_PNP as usize] {
            func(top_device, irp)
        } else {
            STATUS_NOT_SUPPORTED
        }
    } else {
        STATUS_NOT_SUPPORTED
    };

    if status >= 0 && (*irp).io_status.information != 0 {
        let relations = (*irp).io_status.information as *const DEVICE_RELATIONS;
        let count = (*relations).count;

        dbg_print("   [PnP] BusRelations: ");
        crate::kd::dbg_print_num(count as u64);
        dbg_print(" child device(s)\n");

        // Получаем parent devnode (для PDO)
        let parent_node = if !(*pdo).device_object_extension.is_null() {
            (*(*pdo).device_object_extension).device_node as *mut super::devnode::DEVICE_NODE
        } else {
            IOP_ROOT_DEVICE_NODE
        };

        // Обрабатываем каждый дочерний PDO
        for i in 0..count as usize {
            let child_pdo = *(*relations).objects.as_ptr().add(i);
            if child_pdo.is_null() {
                continue;
            }

            // Проверяем, есть ли уже devnode для этого PDO
            let existing_node = if !(*child_pdo).device_object_extension.is_null() {
                let ext = (*child_pdo).device_object_extension;
                // Проверяем что extension тоже в kernel space
                if (ext as usize) < 0xFFFF800000000000 {
                    core::ptr::null_mut()
                } else {
                    (*ext).device_node as *mut super::devnode::DEVICE_NODE
                }
            } else {
                core::ptr::null_mut()
            };

            if !existing_node.is_null() {
                // Devnode уже существует - помечаем как enumerated и пропускаем создание
                (*existing_node).set_flag(DNF_ENUMERATED);
                continue;
            }

            // Создаём devnode для нового PDO
            let child_node = pip_allocate_device_node(child_pdo);
            if child_node.is_null() {
                continue;
            }

            // Запрашиваем Hardware ID для формирования instance path
            let hw_id = pnp_query_device_id(child_pdo);
            let instance_path = if !hw_id.is_empty() {
                alloc::format!("{}\\{}", hw_id, i)
            } else {
                // Fallback: используем bus/device/function address
                alloc::format!("PCI\\UNKNOWN\\{}", i)
            };
            pi_set_dev_node_instance_path(child_node, &instance_path);

            (*child_node).set_flag(DNF_ENUMERATED);
            pi_insert_dev_node(child_node, parent_node);
            pi_set_dev_node_state(child_node, PnpDevnodeState::Initialized);

            dbg_print("   [PnP] Created child devnode: ");
            if !hw_id.is_empty() {
                dbg_print(&hw_id);
            }
            dbg_print("\n");

            // Рекурсивно обрабатываем новое устройство
            if !hw_id.is_empty() {
                let _ = pnp_process_new_device(&hw_id, child_pdo);
            }
        }
    } else if status >= 0 {
        dbg_print("   [PnP] BusRelations: no children\n");
    }

    io_free_irp(irp);
    STATUS_SUCCESS
}

/// Запрашивает Hardware ID у PDO через IRP_MN_QUERY_ID
unsafe fn pnp_query_device_id(pdo: PDEVICE_OBJECT) -> alloc::string::String {
    use crate::io::irp::{io_allocate_irp, io_free_irp, io_get_next_irp_stack_location, io_set_next_irp_stack_location};
    use crate::io::pnp::IRP_MN_QUERY_ID;
    use crate::io::types::IRP_MJ_PNP;
    use alloc::string::String;
    // BUS_QUERY_DEVICE_ID = 0
    const BUS_QUERY_DEVICE_ID: u32 = 0;

    if pdo.is_null() {
        return String::new();
    }

    let stack_size = (*pdo).stack_size;
    let irp = io_allocate_irp(stack_size as i8, false);
    if irp.is_null() {
        return String::new();
    }

    (*irp).io_status.status = STATUS_NOT_SUPPORTED;
    (*irp).io_status.information = 0;

    let stack = io_get_next_irp_stack_location(irp);
    if !stack.is_null() {
        (*stack).major_function = IRP_MJ_PNP;
        (*stack).minor_function = IRP_MN_QUERY_ID;
        (*stack).parameters.query_id.id_type = BUS_QUERY_DEVICE_ID;
        (*stack).device_object = pdo;
    }
    io_set_next_irp_stack_location(irp);

    // Вызываем драйвер PDO
    let driver = (*pdo).driver_object;
    let status = if !driver.is_null() {
        if let Some(func) = (*driver).major_function[IRP_MJ_PNP as usize] {
            func(pdo, irp)
        } else {
            STATUS_NOT_SUPPORTED
        }
    } else {
        STATUS_NOT_SUPPORTED
    };

    let result = if status >= 0 && (*irp).io_status.information != 0 {
        // information содержит указатель на null-terminated Unicode string
        let ptr = (*irp).io_status.information as *const u16;
        let mut len = 0usize;
        while *ptr.add(len) != 0 && len < 256 {
            len += 1;
        }
        let slice = core::slice::from_raw_parts(ptr, len);
        String::from_utf16_lossy(slice)
    } else {
        String::new()
    };

    io_free_irp(irp);
    result
}
