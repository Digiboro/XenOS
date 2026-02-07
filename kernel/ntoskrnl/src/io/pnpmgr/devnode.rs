//! DEVICE_NODE — узел дерева устройств PnP
//!
//! Структура представляет узел в дереве устройств. Каждый PDO
//! (Physical Device Object) связан с одним DEVICE_NODE.
//!
//! Источники:
//! - ReactOS: ntoskrnl/io/pnpmgr/devnode.c, sdk/include/ndk/iotypes.h
//! - Windows 7: ntos/io/pnpmgr/

use core::sync::atomic::{AtomicI32, AtomicU32, Ordering};

use crate::ke::spinlock::KSPIN_LOCK;
use crate::nt::ntdef::UNICODE_STRING;
use crate::nt::{LIST_ENTRY, NTSTATUS, PVOID, ULONG};

use super::state::{PnpDevnodeState, DEVNODE_HISTORY_SIZE};
use super::super::device::DEVICE_OBJECT;
use super::super::irp::IRP;
use super::super::types::INTERFACE_TYPE;

// =============================================================================
// DEVICE_NODE
// =============================================================================

/// DEVICE_NODE — узел дерева устройств PnP
///
/// Каждому PDO соответствует один DEVICE_NODE, который хранит:
/// - Топологию (parent/child/sibling)
/// - Состояние PnP state machine
/// - Идентификаторы устройства (InstancePath, HardwareIDs, etc.)
/// - Назначенные ресурсы
/// - Флаги и проблемы
#[repr(C)]
pub struct DEVICE_NODE {
    // =========================================================================
    // Топология дерева
    // =========================================================================
    /// Следующий sibling (устройство на том же уровне)
    pub sibling: *mut DEVICE_NODE,
    /// Первый дочерний узел
    pub child: *mut DEVICE_NODE,
    /// Родительский узел
    pub parent: *mut DEVICE_NODE,
    /// Последний дочерний узел (для быстрой вставки)
    pub last_child: *mut DEVICE_NODE,
    /// Уровень в дереве (root = 0)
    pub level: ULONG,

    // =========================================================================
    // PnP State Machine
    // =========================================================================
    /// Текущее состояние
    pub state: PnpDevnodeState,
    /// Предыдущее состояние
    pub previous_state: PnpDevnodeState,
    /// История состояний (ring buffer)
    pub state_history: [PnpDevnodeState; DEVNODE_HISTORY_SIZE],
    /// Индекс в истории состояний
    pub state_history_entry: ULONG,
    /// Статус завершения последней операции
    pub completion_status: NTSTATUS,
    /// Pending PnP IRP
    pub pending_irp: *mut IRP,

    // =========================================================================
    // Флаги и проблемы
    // =========================================================================
    /// Флаги devnode (DNF_*)
    pub flags: ULONG,
    /// User flags
    pub user_flags: ULONG,
    /// Код проблемы (CM_PROB_*)
    pub problem: ULONG,

    // =========================================================================
    // Связь с DEVICE_OBJECT
    // =========================================================================
    /// Physical Device Object (PDO), связанный с этим узлом
    pub physical_device_object: *mut DEVICE_OBJECT,
    /// Дубликат PDO (если есть)
    pub duplicate_pdo: *mut DEVICE_OBJECT,

    // =========================================================================
    // Идентификация
    // =========================================================================
    /// Путь экземпляра (например "PCI\VEN_8086&DEV_1234\3&abc&0")
    pub instance_path: UNICODE_STRING,
    /// Имя службы/драйвера
    pub service_name: UNICODE_STRING,

    // =========================================================================
    // Ресурсы
    // =========================================================================
    /// Назначенные ресурсы (raw)
    pub resource_list: PVOID, // PCM_RESOURCE_LIST
    /// Назначенные ресурсы (translated)
    pub resource_list_translated: PVOID, // PCM_RESOURCE_LIST
    /// Требования к ресурсам
    pub resource_requirements: PVOID, // PIO_RESOURCE_REQUIREMENTS_LIST
    /// Boot ресурсы
    pub boot_resources: PVOID, // PCM_RESOURCE_LIST

    // =========================================================================
    // Информация о шине
    // =========================================================================
    /// Тип интерфейса (PCI, ISA, etc.)
    pub interface_type: INTERFACE_TYPE,
    /// Номер шины
    pub bus_number: ULONG,
    /// Тип интерфейса дочерних устройств
    pub child_interface_type: INTERFACE_TYPE,
    /// Номер шины дочерних устройств
    pub child_bus_number: ULONG,
    /// Индекс типа шины дочерних устройств
    pub child_bus_type_index: i16,

    // =========================================================================
    // Политики
    // =========================================================================
    /// Политика удаления
    pub removal_policy: u8,
    /// Hardware removal policy
    pub hardware_removal_policy: u8,

    // =========================================================================
    // Списки уведомлений и арбитров
    // =========================================================================
    /// Список уведомлений target device
    pub target_device_notify: LIST_ENTRY,
    /// Список арбитров устройства
    pub device_arbiter_list: LIST_ENTRY,
    /// Список трансляторов устройства
    pub device_translator_list: LIST_ENTRY,

    // =========================================================================
    // Маски арбитров/трансляторов
    // =========================================================================
    /// Маска отсутствующих трансляторов
    pub no_translator_mask: u16,
    /// Маска запрошенных трансляторов
    pub query_translator_mask: u16,
    /// Маска отсутствующих арбитров
    pub no_arbiter_mask: u16,
    /// Маска запрошенных арбитров
    pub query_arbiter_mask: u16,

    // =========================================================================
    // Capability flags
    // =========================================================================
    /// Флаги возможностей (из DEVICE_CAPABILITIES)
    pub capability_flags: ULONG,

    // =========================================================================
    // Дополнительные поля
    // =========================================================================
    /// Счётчик зависимостей для disable
    pub disableable_depends: ULONG,
    /// Pending set interface state operations
    pub pended_set_interface_state: LIST_ENTRY,
    /// Счётчик попыток выгрузки драйвера
    pub driver_unload_retry_count: ULONG,
    /// Предыдущий родитель (при перемещении)
    pub previous_parent: *mut DEVICE_NODE,
    /// Количество удалённых дочерних узлов
    pub deleted_children: ULONG,
}

impl DEVICE_NODE {
    /// Создаёт новый DEVICE_NODE для указанного PDO
    ///
    /// # Safety
    /// PDO должен быть валидным указателем или null
    pub const fn new() -> Self {
        Self {
            // Топология
            sibling: core::ptr::null_mut(),
            child: core::ptr::null_mut(),
            parent: core::ptr::null_mut(),
            last_child: core::ptr::null_mut(),
            level: 0,

            // State machine
            state: PnpDevnodeState::Uninitialized,
            previous_state: PnpDevnodeState::Unspecified,
            state_history: [PnpDevnodeState::Unspecified; DEVNODE_HISTORY_SIZE],
            state_history_entry: 0,
            completion_status: 0,
            pending_irp: core::ptr::null_mut(),

            // Флаги
            flags: 0,
            user_flags: 0,
            problem: 0,

            // DEVICE_OBJECT
            physical_device_object: core::ptr::null_mut(),
            duplicate_pdo: core::ptr::null_mut(),

            // Идентификация
            instance_path: UNICODE_STRING::new(),
            service_name: UNICODE_STRING::new(),

            // Ресурсы
            resource_list: core::ptr::null_mut(),
            resource_list_translated: core::ptr::null_mut(),
            resource_requirements: core::ptr::null_mut(),
            boot_resources: core::ptr::null_mut(),

            // Шина
            interface_type: INTERFACE_TYPE::InterfaceTypeUndefined,
            bus_number: u32::MAX, // -1 как unsigned
            child_interface_type: INTERFACE_TYPE::InterfaceTypeUndefined,
            child_bus_number: u32::MAX,
            child_bus_type_index: -1,

            // Политики
            removal_policy: 0,
            hardware_removal_policy: 0,

            // Списки
            target_device_notify: LIST_ENTRY::new(),
            device_arbiter_list: LIST_ENTRY::new(),
            device_translator_list: LIST_ENTRY::new(),

            // Маски
            no_translator_mask: 0,
            query_translator_mask: 0,
            no_arbiter_mask: 0,
            query_arbiter_mask: 0,

            // Capability
            capability_flags: 0,

            // Дополнительные
            disableable_depends: 0,
            pended_set_interface_state: LIST_ENTRY::new(),
            driver_unload_retry_count: 0,
            previous_parent: core::ptr::null_mut(),
            deleted_children: 0,
        }
    }

    /// Устанавливает флаг
    #[inline]
    pub fn set_flag(&mut self, flag: ULONG) {
        self.flags |= flag;
    }

    /// Очищает флаг
    #[inline]
    pub fn clear_flag(&mut self, flag: ULONG) {
        self.flags &= !flag;
    }

    /// Проверяет наличие флага
    #[inline]
    pub fn has_flag(&self, flag: ULONG) -> bool {
        (self.flags & flag) != 0
    }

    /// Устанавливает проблему
    #[inline]
    pub fn set_problem(&mut self, problem: ULONG) {
        self.flags |= super::state::DNF_HAS_PROBLEM;
        self.problem = problem;
    }

    /// Очищает проблему
    #[inline]
    pub fn clear_problem(&mut self) {
        self.flags &= !super::state::DNF_HAS_PROBLEM;
        self.problem = 0;
    }

    /// Проверяет наличие проблемы
    #[inline]
    pub fn has_problem(&self) -> bool {
        (self.flags & super::state::DNF_HAS_PROBLEM) != 0
    }
}

// =============================================================================
// Глобальные переменные PnP Manager
// =============================================================================

/// Корневой узел дерева устройств
pub static mut IOP_ROOT_DEVICE_NODE: *mut DEVICE_NODE = core::ptr::null_mut();

/// Спинлок для защиты дерева устройств
pub static IOP_DEVICE_TREE_LOCK: KSPIN_LOCK = KSPIN_LOCK::new();

/// Счётчик узлов устройств
pub static IOP_NUMBER_DEVICE_NODES: AtomicI32 = AtomicI32::new(0);

// =============================================================================
// Функции работы с DEVICE_NODE
// =============================================================================

/// Выделяет и инициализирует новый DEVICE_NODE
///
/// # Arguments
/// * `pdo` - Physical Device Object (может быть null для root)
///
/// # Returns
/// Указатель на новый DEVICE_NODE или null при ошибке
pub unsafe fn pip_allocate_device_node(pdo: *mut DEVICE_OBJECT) -> *mut DEVICE_NODE {
    use crate::ex::pool::{ex_allocate_pool_with_tag, POOL_TYPE};

    // Выделяем память
    let node = ex_allocate_pool_with_tag(
        POOL_TYPE::NonPagedPool,
        core::mem::size_of::<DEVICE_NODE>(),
        u32::from_le_bytes(*b"PnDN"),
    ) as *mut DEVICE_NODE;

    if node.is_null() {
        return core::ptr::null_mut();
    }

    // Инициализируем
    core::ptr::write(node, DEVICE_NODE::new());

    // Статистика
    IOP_NUMBER_DEVICE_NODES.fetch_add(1, Ordering::SeqCst);

    // Инициализируем списки
    LIST_ENTRY::init_head(&mut (*node).target_device_notify);
    LIST_ENTRY::init_head(&mut (*node).device_arbiter_list);
    LIST_ENTRY::init_head(&mut (*node).device_translator_list);
    LIST_ENTRY::init_head(&mut (*node).pended_set_interface_state);

    // Связываем с PDO
    if !pdo.is_null() {
        (*node).physical_device_object = pdo;

        // Связываем PDO → DevNode через DeviceObjectExtension
        let ext = (*pdo).device_object_extension;
        if !ext.is_null() {
            (*ext).device_node = node as PVOID;
        }

        // Снимаем флаг DO_DEVICE_INITIALIZING
        (*pdo).flags &= !super::super::types::DO_DEVICE_INITIALIZING;
    }

    node
}

/// Вставляет узел в дерево как дочерний к parent
pub unsafe fn pi_insert_dev_node(node: *mut DEVICE_NODE, parent: *mut DEVICE_NODE) {
    use crate::ke::spinlock::{ke_acquire_spin_lock, ke_release_spin_lock};

    if node.is_null() || parent.is_null() {
        return;
    }

    debug_assert!((*node).parent.is_null(), "Node already has a parent");

    let old_irql = ke_acquire_spin_lock(&IOP_DEVICE_TREE_LOCK);

    (*node).parent = parent;
    (*node).sibling = core::ptr::null_mut();

    if (*parent).last_child.is_null() {
        // Первый ребёнок
        (*parent).child = node;
        (*parent).last_child = node;
    } else {
        // Добавляем в конец списка
        (*(*parent).last_child).sibling = node;
        (*parent).last_child = node;
    }

    // Устанавливаем уровень
    (*node).level = (*parent).level + 1;

    ke_release_spin_lock(&IOP_DEVICE_TREE_LOCK, old_irql);
}

/// Устанавливает новое состояние узла
///
/// # Returns
/// Предыдущее состояние
pub unsafe fn pi_set_dev_node_state(
    node: *mut DEVICE_NODE,
    new_state: PnpDevnodeState,
) -> PnpDevnodeState {
    use crate::ke::spinlock::{ke_acquire_spin_lock, ke_release_spin_lock};

    if node.is_null() {
        return PnpDevnodeState::Unspecified;
    }

    let old_irql = ke_acquire_spin_lock(&IOP_DEVICE_TREE_LOCK);

    let prev_state = (*node).state;

    if prev_state != new_state {
        (*node).state = new_state;
        (*node).previous_state = prev_state;

        // Записываем в историю
        let idx = (*node).state_history_entry as usize % DEVNODE_HISTORY_SIZE;
        (*node).state_history[idx] = prev_state;
        (*node).state_history_entry = (*node).state_history_entry.wrapping_add(1);
    }

    ke_release_spin_lock(&IOP_DEVICE_TREE_LOCK, old_irql);

    prev_state
}

/// Устанавливает instance path для узла
///
/// Конвертирует UTF-8 строку в UNICODE_STRING.
pub unsafe fn pi_set_dev_node_instance_path(
    node: *mut DEVICE_NODE,
    path: &str,
) {
    use crate::ex::pool::{ex_allocate_pool_with_tag, POOL_TYPE};

    if node.is_null() || path.is_empty() {
        return;
    }

    // Освобождаем старый буфер если есть
    if !(*node).instance_path.buffer.is_null() {
        use crate::ex::pool::ex_free_pool_with_tag;
        ex_free_pool_with_tag((*node).instance_path.buffer as PVOID, u32::from_le_bytes(*b"PnIP"));
        (*node).instance_path.buffer = core::ptr::null_mut();
        (*node).instance_path.length = 0;
        (*node).instance_path.maximum_length = 0;
    }

    // Конвертируем UTF-8 в UTF-16
    let utf16: alloc::vec::Vec<u16> = path.encode_utf16().collect();
    let byte_len = utf16.len() * 2;

    // Выделяем буфер (+ 2 для null-terminator)
    let buffer = ex_allocate_pool_with_tag(
        POOL_TYPE::NonPagedPool,
        byte_len + 2,
        u32::from_le_bytes(*b"PnIP"),
    ) as *mut u16;

    if buffer.is_null() {
        return;
    }

    // Копируем данные
    core::ptr::copy_nonoverlapping(utf16.as_ptr(), buffer, utf16.len());
    *buffer.add(utf16.len()) = 0; // null-terminator

    (*node).instance_path.buffer = buffer;
    (*node).instance_path.length = byte_len as u16;
    (*node).instance_path.maximum_length = (byte_len + 2) as u16;
}

/// Освобождает DEVICE_NODE
pub unsafe fn iop_free_device_node(node: *mut DEVICE_NODE) -> NTSTATUS {
    use crate::ex::pool::ex_free_pool_with_tag;
    use crate::nt::ntstatus::*;

    if node.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    // TODO: Освободить строки InstancePath, ServiceName если выделены

    // Отвязываем от PDO
    let pdo = (*node).physical_device_object;
    if !pdo.is_null() {
        let ext = (*pdo).device_object_extension;
        if !ext.is_null() {
            (*ext).device_node = core::ptr::null_mut();
        }
    }

    // Статистика
    IOP_NUMBER_DEVICE_NODES.fetch_sub(1, Ordering::SeqCst);

    // Освобождаем память
    ex_free_pool_with_tag(node as PVOID, u32::from_le_bytes(*b"PnDN"));

    STATUS_SUCCESS
}

/// Получает DEVICE_NODE из DEVICE_OBJECT
///
/// # Safety
/// device_object должен быть валидным указателем
#[inline]
pub unsafe fn iop_get_device_node(device_object: *mut DEVICE_OBJECT) -> *mut DEVICE_NODE {
    if device_object.is_null() {
        return core::ptr::null_mut();
    }

    let ext = (*device_object).device_object_extension;
    if ext.is_null() {
        return core::ptr::null_mut();
    }

    (*ext).device_node as *mut DEVICE_NODE
}

/// Проверяет, является ли PDO валидным (enumerated)
#[inline]
pub unsafe fn iop_is_valid_pdo(pdo: *mut DEVICE_OBJECT) -> bool {
    if pdo.is_null() {
        return false;
    }

    let node = iop_get_device_node(pdo);
    if node.is_null() {
        return false;
    }

    (*node).has_flag(super::state::DNF_ENUMERATED)
}

// =============================================================================
// Device Tree Traversal
// =============================================================================

/// Callback для обхода дерева устройств
pub type DeviceTreeTraverseRoutine =
    unsafe fn(node: *mut DEVICE_NODE, context: PVOID) -> NTSTATUS;

/// Контекст для обхода дерева устройств
pub struct DeviceTreeTraverseContext {
    /// Текущий узел
    pub device_node: *mut DEVICE_NODE,
    /// Начальный узел обхода
    pub first_device_node: *mut DEVICE_NODE,
    /// Callback функция
    pub action: DeviceTreeTraverseRoutine,
    /// Контекст для callback
    pub context: PVOID,
}

impl DeviceTreeTraverseContext {
    /// Создаёт новый контекст обхода
    pub fn new(
        start_node: *mut DEVICE_NODE,
        action: DeviceTreeTraverseRoutine,
        context: PVOID,
    ) -> Self {
        Self {
            device_node: start_node,
            first_device_node: start_node,
            action,
            context,
        }
    }
}

/// Выполняет preorder обход дерева устройств
///
/// # Returns
/// STATUS_SUCCESS или код ошибки от callback
pub unsafe fn iop_traverse_device_tree(ctx: &mut DeviceTreeTraverseContext) -> NTSTATUS {
    use crate::nt::ntstatus::*;

    let mut current = ctx.first_device_node;

    while !current.is_null() {
        ctx.device_node = current;

        // Вызываем callback
        let status = (ctx.action)(current, ctx.context);

        // STATUS_UNSUCCESSFUL означает "остановить обход, но вернуть успех"
        if status == STATUS_UNSUCCESSFUL {
            return STATUS_SUCCESS;
        }

        // Любая другая ошибка — прекращаем
        if status < 0 {
            return status;
        }

        // Спускаемся к детям
        if !(*current).child.is_null() {
            current = (*current).child;
            continue;
        }

        // Идём к sibling или поднимаемся
        while !current.is_null() {
            if !(*current).sibling.is_null() {
                current = (*current).sibling;
                break;
            }

            // Поднимаемся к родителю
            current = (*current).parent;

            // Если вернулись к начальному узлу — конец
            if current == ctx.first_device_node {
                return STATUS_SUCCESS;
            }
        }
    }

    STATUS_SUCCESS
}

// =============================================================================
// Device Tree Dump (для отладки)
// =============================================================================

/// Выводит дерево устройств в лог
///
/// Формат вывода:
/// ```
/// Device Tree:
/// ROOT
/// ├── ACPI\_SB
/// │   ├── ACPI\PNP0A03 (PCI Host Bridge)
/// │   │   └── PCI\VEN_8086&DEV_1234
/// │   └── ACPI\PNP0C01
/// └── ...
/// ```
pub unsafe fn iop_dump_device_tree() {
    use crate::kd::dbg_print;

    dbg_print("\n   ┌───────────────────────────────────────┐\n");
    dbg_print("   │         Device Tree                   │\n");
    dbg_print("   └───────────────────────────────────────┘\n");

    let root = IOP_ROOT_DEVICE_NODE;
    if root.is_null() {
        dbg_print("   (empty - no root node)\n\n");
        return;
    }

    // Печатаем root
    dbg_print("   ROOT\n");

    // Рекурсивно печатаем детей
    dump_device_node_children(root, 0);

    dbg_print("\n");
}

/// Рекурсивно выводит дочерние узлы
unsafe fn dump_device_node_children(parent: *mut DEVICE_NODE, depth: usize) {
    use crate::kd::dbg_print;

    if parent.is_null() {
        return;
    }

    let mut child = (*parent).child;
    let mut has_next;

    while !child.is_null() {
        // Проверяем есть ли следующий sibling
        has_next = !(*child).sibling.is_null();

        // Печатаем отступ
        print_tree_indent(depth, has_next);

        // Печатаем информацию об узле
        print_device_node_info(child);

        // Рекурсивно печатаем детей
        dump_device_node_children(child, depth + 1);

        child = (*child).sibling;
    }
}

/// Печатает отступы для дерева
unsafe fn print_tree_indent(depth: usize, has_next: bool) {
    use crate::kd::dbg_print;

    dbg_print("   ");

    // Вертикальные линии для предыдущих уровней
    for _ in 0..depth {
        dbg_print("│   ");
    }

    // Символ ветвления
    if has_next {
        dbg_print("├── ");
    } else {
        dbg_print("└── ");
    }
}

/// Печатает информацию об одном узле устройства
unsafe fn print_device_node_info(node: *mut DEVICE_NODE) {
    use crate::kd::{dbg_print, dbg_print_hex};

    if node.is_null() {
        dbg_print("(null)\n");
        return;
    }

    // Получаем instance path
    let instance_path = &(*node).instance_path;
    if instance_path.length > 0 && !instance_path.buffer.is_null() {
        // Конвертируем UNICODE_STRING в UTF-8 для вывода
        let len = (instance_path.length / 2) as usize;
        let slice = core::slice::from_raw_parts(instance_path.buffer, len);

        // Простой вывод (ASCII только)
        for &ch in slice {
            if ch < 128 {
                let c = ch as u8 as char;
                // Печатаем по одному символу
                let buf = [c as u8, 0];
                let s = core::str::from_utf8_unchecked(&buf[..1]);
                dbg_print(s);
            } else {
                dbg_print("?");
            }
        }
    } else {
        // Нет instance path — показываем адрес PDO
        if !(*node).physical_device_object.is_null() {
            dbg_print("PDO@0x");
            dbg_print_hex((*node).physical_device_object as u64);
        } else {
            dbg_print("(unnamed)");
        }
    }

    // Показываем состояние
    dbg_print(" [");
    match (*node).state {
        PnpDevnodeState::Uninitialized => dbg_print("Uninit"),
        PnpDevnodeState::Initialized => dbg_print("Init"),
        PnpDevnodeState::DriversAdded => dbg_print("DrvAdded"),
        PnpDevnodeState::ResourcesAssigned => dbg_print("Resources"),
        PnpDevnodeState::Started => dbg_print("Started"),
        PnpDevnodeState::StartPending => dbg_print("Starting"),
        PnpDevnodeState::Removed => dbg_print("Removed"),
        _ => dbg_print("???"),
    }
    dbg_print("]");

    // Показываем флаги (сокращённо)
    if (*node).has_flag(super::state::DNF_HAS_PROBLEM) {
        dbg_print(" PROBLEM");
    }
    if (*node).has_flag(super::state::DNF_ENUMERATED) {
        dbg_print(" enum");
    }
    if (*node).has_flag(super::state::DNF_ADDED) {
        dbg_print(" +FDO");
    }

    dbg_print("\n");
}

