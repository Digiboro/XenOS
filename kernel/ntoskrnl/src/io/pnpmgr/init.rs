//! PnP Manager Initialization
//!
//! Инициализация PnP Manager происходит в Phase 1 I/O init после загрузки
//! boot драйверов.
//!
//! Источники:
//! - ReactOS: ntoskrnl/io/pnpmgr/pnpinit.c, ntoskrnl/io/pnpmgr/pnpmgr.c
//! - Windows 7: ntos/io/pnpmgr/

use crate::kd::dbg_print;
use crate::nt::NTSTATUS;
use crate::nt::ntstatus::*;

use super::devaction::{pi_init_device_action_queue, PI_ENUMERATION_FINISHED};
use super::devnode::{
    pip_allocate_device_node, pi_set_dev_node_state, IOP_DEVICE_TREE_LOCK, IOP_ROOT_DEVICE_NODE,
};
use super::state::{PnpDevnodeState, DNF_ENUMERATED, DNF_MADEUP};

// =============================================================================
// Глобальные переменные PnP
// =============================================================================

/// PnP инициализирован
static mut PNP_INITIALIZED: bool = false;

/// Boot драйверы загружены
pub static mut PNP_BOOT_DRIVERS_LOADED: bool = false;

/// Boot драйверы инициализированы
pub static mut PNP_BOOT_DRIVERS_INITIALIZED: bool = false;

/// Интерфейс по умолчанию
pub static mut PNP_DEFAULT_INTERFACE_TYPE: u32 = 1; // Isa

// =============================================================================
// IopInitializePlugPlayServices
// =============================================================================

/// IopInitializePlugPlayServices — главная точка инициализации PnP
///
/// Вызывается из IoInitSystem(Phase 1) после загрузки boot драйверов.
///
/// # Действия:
/// 1. Инициализация глобальных структур (локи, очереди)
/// 2. Создание root devnode
/// 3. Инициализация арбитров ресурсов
/// 4. Инициализация очереди событий PnP
///
/// # Returns
/// STATUS_SUCCESS или код ошибки
pub unsafe fn iop_initialize_plug_play_services() -> NTSTATUS {
    dbg_print("   [PnP] Initializing Plug and Play services...\n");

    // 1. Инициализация очереди действий
    pi_init_device_action_queue();

    // 2. Инициализация арбитров
    let status = iop_initialize_arbiters();
    if status < 0 {
        dbg_print("   [PnP] Failed to initialize arbiters\n");
        return status;
    }

    // 3. Инициализация internal ACPI driver (для обработки IRP на ACPI PDO)
    let status = super::pnpacpi::acpi_init_internal_driver();
    if status < 0 {
        dbg_print("   [PnP] Failed to initialize internal ACPI driver\n");
        return status;
    }

    // 4. Создание root devnode
    let status = iop_create_root_device_node();
    if status < 0 {
        dbg_print("   [PnP] Failed to create root device node\n");
        return status;
    }

    // 5. Инициализация очереди событий PnP
    let status = iop_init_plug_play_events();
    if status < 0 {
        dbg_print("   [PnP] Failed to initialize PnP events\n");
        return status;
    }

    // Помечаем root devnode как Started
    if !IOP_ROOT_DEVICE_NODE.is_null() {
        pi_set_dev_node_state(IOP_ROOT_DEVICE_NODE, PnpDevnodeState::Started);
    }

    PNP_INITIALIZED = true;
    dbg_print("   [PnP] Plug and Play services initialized\n");

    STATUS_SUCCESS
}

/// PpInitSystem — вторичная инициализация PnP (после boot)
///
/// Вызывается позже для завершения инициализации PnP.
pub unsafe fn pp_init_system() -> bool {
    if !PNP_INITIALIZED {
        return false;
    }

    // Дополнительная инициализация выполняется в pnp_init2()
    true
}

/// PnpInit2 — третья фаза инициализации
///
/// Вызывается после завершения фазы 1.
/// Создаёт PnP worker thread и запускает enumeration дерева устройств.
pub unsafe fn pnp_init2() {
    if !PNP_INITIALIZED {
        return;
    }

    dbg_print("   [PnP] Phase 2 initialization\n");

    // Создаём worker thread для асинхронной обработки PnP событий
    let status = super::devaction::pi_create_worker_thread();
    if status < 0 {
        dbg_print("   [PnP] Warning: failed to create worker thread, using sync mode\n");
    }

    // Запускаем enumeration дерева устройств
    dbg_print("   [PnP] Starting device tree enumeration...\n");
    let status = super::devaction::pi_queue_device_action(
        IOP_ROOT_DEVICE_NODE,
        super::devaction::DeviceAction::EnumDeviceTree,
        0,
    );

    if status < 0 {
        dbg_print("   [PnP] Failed to queue enumeration action\n");
    }

    // Ожидаем завершения первичной enumeration (для boot)
    // Worker thread сигнализирует PI_ENUMERATION_LOCK когда очередь пуста
    super::devaction::pi_wait_for_enumeration_idle();

    use core::sync::atomic::Ordering;
    PI_ENUMERATION_FINISHED.store(true, Ordering::SeqCst);

    dbg_print("   [PnP] Phase 2 initialization complete\n");

    // Выводим дерево устройств в лог
    super::devnode::iop_dump_device_tree();
    
    // Тестовое монтирование томов (только при trace-io)
    #[cfg(feature = "trace-io")]
    iop_test_volume_mount();
}

/// Тестовое монтирование первого найденного тома
///
/// Ищет устройство HarddiskVolume1 в директории \Device и пытается смонтировать.
/// HarddiskVolume1 соответствует partition 1 (первая реальная партиция с FAT32).
/// HarddiskVolume0 это partition 0 (whole disk raw access).
#[cfg(feature = "trace-io")]
unsafe fn iop_test_volume_mount() {
    use crate::nt::ntdef::UNICODE_STRING;
    use super::super::vpb::iop_mount_volume;
    use crate::ob::dir::{obp_lookup_entry_directory, obp_release_lookup_context, OBP_LOOKUP_CONTEXT};
    
    dbg_print("\n   [IO] Testing volume mount...\n");
    
    // Получаем директорию \Device
    let device_dir = super::super::init::iop_get_device_directory();
    if device_dir.is_null() {
        dbg_print("   [IO] ERROR: \\Device directory not found\n");
        return;
    }
    
    // Ищем HarddiskVolume1 (partition 1, которая содержит FAT32)
    let volume_name: [u16; 16] = [
        b'H' as u16, b'a' as u16, b'r' as u16, b'd' as u16, b'd' as u16, b'i' as u16, b's' as u16, b'k' as u16,
        b'V' as u16, b'o' as u16, b'l' as u16, b'u' as u16, b'm' as u16, b'e' as u16,
        b'1' as u16, 0u16,
    ];
    
    let name = UNICODE_STRING {
        length: 30, // 15 chars * 2 bytes
        maximum_length: 32,
        buffer: volume_name.as_ptr() as *mut u16,
    };
    
    dbg_print("   [IO] Looking up HarddiskVolume1 in \\Device...\n");
    
    let mut context = OBP_LOOKUP_CONTEXT::new();
    let found = obp_lookup_entry_directory(device_dir, &name, 0x40, false, &mut context);
    
    if !found || context.object.is_null() {
        dbg_print("   [IO] HarddiskVolume1 not found in \\Device directory!\n");
        obp_release_lookup_context(&mut context);
        return;
    }
    
    let device_obj = context.object;
    dbg_print("   [IO] Found device at 0x");
    crate::kd::dbg_print_hex(device_obj as u64);
    dbg_print("\n");
    
    // Освобождаем lock на директорию
    obp_release_lookup_context(&mut context);
    
    // Проверяем что это DEVICE_OBJECT
    let device = device_obj as *mut super::super::device::DEVICE_OBJECT;
    let vpb = (*device).vpb;
    
    if vpb.is_null() {
        dbg_print("   [IO] Device has no VPB - not a mountable volume\n");
        return;
    }
    
    dbg_print("   [IO] Device has VPB at 0x");
    crate::kd::dbg_print_hex(vpb as u64);
    dbg_print("\n");
    
    // Пробуем смонтировать
    dbg_print("   [IO] Attempting mount...\n");
    let mount_status = iop_mount_volume(device, true);
    
    if mount_status == 0 {
        dbg_print("   [IO] Volume mounted successfully!\n");
    } else {
        dbg_print("   [IO] Mount failed with status 0x");
        crate::kd::dbg_print_hex(mount_status as u64);
        dbg_print("\n");
    }
}

// =============================================================================
// Root Device Node
// =============================================================================

/// Создаёт корневой узел дерева устройств
unsafe fn iop_create_root_device_node() -> NTSTATUS {
    dbg_print("   [PnP] Creating root device node...\n");

    // Создаём root devnode без PDO (PDO будет создан Root bus driver'ом)
    let root_node = pip_allocate_device_node(core::ptr::null_mut());

    if root_node.is_null() {
        dbg_print("   [PnP] Failed to allocate root device node\n");
        return STATUS_INSUFFICIENT_RESOURCES;
    }

    // Устанавливаем флаги
    (*root_node).set_flag(DNF_MADEUP | DNF_ENUMERATED);

    // Устанавливаем InstancePath для root
    // В NT это "HTREE\\ROOT\\0" или подобное
    // Пока оставляем пустым

    // Сохраняем глобально
    IOP_ROOT_DEVICE_NODE = root_node;

    dbg_print("   [PnP] Root device node created\n");

    STATUS_SUCCESS
}

// =============================================================================
// Arbiters
// =============================================================================

/// Инициализация арбитров ресурсов
///
/// Арбитры управляют распределением ресурсов (IRQ, Memory, I/O ports, DMA, Bus numbers).
unsafe fn iop_initialize_arbiters() -> NTSTATUS {
    dbg_print("   [PnP] Initializing resource arbiters...\n");

    let status = super::arbiter::iop_init_resource_arbiters();
    if status < 0 {
        dbg_print("   [PnP] Failed to initialize arbiters\n");
        return status;
    }

    dbg_print("   [PnP] Resource arbiters initialized\n");

    STATUS_SUCCESS
}

// =============================================================================
// PnP Events
// =============================================================================

/// Глобальная очередь событий PnP
static mut IOP_PNP_EVENT_QUEUE_HEAD: crate::nt::LIST_ENTRY = crate::nt::LIST_ENTRY::new();

/// Событие для уведомления о новых PnP событиях
static mut IOP_PNP_NOTIFY_EVENT: crate::ke::event::KEVENT = crate::ke::event::KEVENT::new();

/// Инициализация очереди событий PnP
unsafe fn iop_init_plug_play_events() -> NTSTATUS {
    use crate::nt::LIST_ENTRY;

    LIST_ENTRY::init_head(&raw mut IOP_PNP_EVENT_QUEUE_HEAD);

    // Событие инициализируется как synchronization event
    // (auto-reset после ожидания)

    STATUS_SUCCESS
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Проверяет, инициализирован ли PnP
#[inline]
pub fn pnp_is_initialized() -> bool {
    unsafe { PNP_INITIALIZED }
}

/// Возвращает корневой узел дерева
#[inline]
pub unsafe fn iop_get_root_device_node() -> *mut super::devnode::DEVICE_NODE {
    IOP_ROOT_DEVICE_NODE
}

