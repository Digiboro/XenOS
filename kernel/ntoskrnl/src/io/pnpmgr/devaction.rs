//! PnP Device Actions — очередь действий PnP Manager
//!
//! Асинхронные действия PnP (enumerate, start, stop, remove) выполняются
//! через очередь devactions в контексте worker thread'а на PASSIVE_LEVEL.
//!
//! # Архитектура
//!
//! ```text
//!  ┌──────────────┐     ┌──────────────────┐     ┌────────────────┐
//!  │ IoInvalidate │────►│ PI_DEVICE_ACTION │────►│ PnP Worker     │
//!  │ DeviceRelations│    │ _QUEUE           │     │ Thread         │
//!  └──────────────┘     └──────────────────┘     └───────┬────────┘
//!                                                        │
//!                              ┌─────────────────────────┼───────────────┐
//!                              │                         │               │
//!                              ▼                         ▼               ▼
//!                       ┌─────────────┐         ┌─────────────┐  ┌─────────────┐
//!                       │ EnumDevice  │         │ StartDevice │  │ RemoveDevice│
//!                       │ Tree        │         │             │  │             │
//!                       └─────────────┘         └─────────────┘  └─────────────┘
//! ```
//!
//! Источники:
//! - ReactOS: ntoskrnl/io/pnpmgr/devaction.c
//! - Windows 7: ntos/io/pnpmgr/

use core::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

use crate::ke::event::KEVENT;
use crate::ke::spinlock::KSPIN_LOCK;
use crate::nt::{LIST_ENTRY, NTSTATUS, PVOID, HANDLE};

use super::devnode::DEVICE_NODE;

// =============================================================================
// Device Action Types
// =============================================================================

/// Типы действий PnP
#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeviceAction {
    /// Перечисление дерева устройств
    EnumDeviceTree = 0,
    /// Перечисление корневых устройств
    EnumRootDevices = 1,
    /// Сброс устройства
    ResetDevice = 2,
    /// Добавление boot устройств
    AddBootDevices = 3,
    /// Запуск устройства
    StartDevice = 4,
    /// Запрос состояния
    QueryState = 5,
    /// Остановка устройства
    StopDevice = 6,
    /// Удаление устройства
    RemoveDevice = 7,
    /// Неожиданное удаление
    SurpriseRemoval = 8,
    /// Инвалидация relations
    InvalidateRelations = 9,
    /// Инвалидация состояния
    InvalidateState = 10,
}

/// Запрос действия PnP в очереди
#[repr(C)]
pub struct DeviceActionRequest {
    /// Элемент списка
    pub list_entry: LIST_ENTRY,
    /// Узел устройства
    pub device_node: *mut DEVICE_NODE,
    /// Тип действия
    pub action: DeviceAction,
    /// Дополнительные флаги/параметры
    pub flags: u32,
    /// Событие завершения (опционально)
    pub completion_event: *mut KEVENT,
    /// Статус результата
    pub status: NTSTATUS,
}

impl DeviceActionRequest {
    /// Создаёт новый запрос
    pub const fn new(node: *mut DEVICE_NODE, action: DeviceAction) -> Self {
        Self {
            list_entry: LIST_ENTRY::new(),
            device_node: node,
            action,
            flags: 0,
            completion_event: core::ptr::null_mut(),
            status: 0,
        }
    }
}

// =============================================================================
// Глобальные структуры очереди
// =============================================================================

/// Очередь запросов действий
pub static mut PI_DEVICE_ACTION_QUEUE: LIST_ENTRY = LIST_ENTRY::new();

/// Спинлок для защиты очереди
pub static PI_DEVICE_ACTION_LOCK: KSPIN_LOCK = KSPIN_LOCK::new();

/// Событие для пробуждения worker thread
pub static mut PI_ENUMERATION_EVENT: KEVENT = KEVENT::new();

/// Флаг: enumeration завершена
pub static PI_ENUMERATION_FINISHED: AtomicBool = AtomicBool::new(false);

/// Флаг: worker thread активен
pub static PI_WORKER_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Event для сигнализации о завершении обработки очереди
/// Сигнализируется когда очередь становится пустой
pub static mut PI_ENUMERATION_LOCK: KEVENT = KEVENT::new();

/// Handle потока PnP worker
pub static PI_WORKER_THREAD_HANDLE: AtomicPtr<core::ffi::c_void> = AtomicPtr::new(core::ptr::null_mut());

/// Флаг: worker thread должен завершиться
pub static PI_WORKER_SHUTDOWN: AtomicBool = AtomicBool::new(false);

// =============================================================================
// Функции работы с очередью
// =============================================================================

/// Инициализирует очередь действий PnP
pub unsafe fn pi_init_device_action_queue() {
    use crate::ke::event::{ke_initialize_event, EVENT_TYPE};

    LIST_ENTRY::init_head(&raw mut PI_DEVICE_ACTION_QUEUE);

    // PI_ENUMERATION_EVENT — notification event для пробуждения worker thread
    ke_initialize_event(
        &mut *(&raw mut PI_ENUMERATION_EVENT),
        EVENT_TYPE::NotificationEvent,
        false, // not signaled
    );

    // PI_ENUMERATION_LOCK — synchronization event для ожидания завершения
    ke_initialize_event(
        &mut *(&raw mut PI_ENUMERATION_LOCK),
        EVENT_TYPE::SynchronizationEvent,
        true, // signaled (idle)
    );
}

/// Создаёт PnP worker thread
///
/// Вызывается один раз при инициализации PnP Manager.
pub unsafe fn pi_create_worker_thread() -> NTSTATUS {
    use crate::kd::dbg_print;
    use crate::nt::ntstatus::*;
    use crate::ps::thread::ps_create_system_thread;

    dbg_print("   [PnP] Creating worker thread...\n");

    let mut thread_handle: HANDLE = 0;

    let status = ps_create_system_thread(
        &mut thread_handle,
        0, // THREAD_ALL_ACCESS
        core::ptr::null_mut(), // ObjectAttributes
        0, // SystemProcess (0 = current process = System)
        core::ptr::null_mut(), // ClientId
        Some(pi_worker_thread_procedure), // StartRoutine
        core::ptr::null_mut(), // StartContext
    );

    if status >= 0 {
        PI_WORKER_THREAD_HANDLE.store(thread_handle as *mut _, Ordering::SeqCst);
        dbg_print("   [PnP] Worker thread created\n");
    } else {
        dbg_print("   [PnP] Failed to create worker thread\n");
    }

    status
}

/// Процедура PnP worker thread
///
/// Ожидает события и обрабатывает очередь действий.
unsafe extern "win64" fn pi_worker_thread_procedure(_context: PVOID) {
    use crate::kd::dbg_print;
    use crate::ke::wait::ke_wait_for_single_object;
    use crate::ke::event::ke_reset_event;

    dbg_print("   [PnP] Worker thread started\n");

    loop {
        // Проверяем флаг завершения
        if PI_WORKER_SHUTDOWN.load(Ordering::SeqCst) {
            dbg_print("   [PnP] Worker thread shutting down\n");
            break;
        }

        // Ждём события (с таймаутом для проверки shutdown)
        let event_ptr = &raw mut PI_ENUMERATION_EVENT;
        let _status = ke_wait_for_single_object(
            event_ptr as PVOID,
            0, // Executive wait reason
            0, // KernelMode
            false, // Alertable
            None, // Timeout (infinite)
        );

        // Проверяем флаг завершения после пробуждения
        if PI_WORKER_SHUTDOWN.load(Ordering::SeqCst) {
            break;
        }

        // Обрабатываем все действия в очереди
        PI_WORKER_ACTIVE.store(true, Ordering::SeqCst);
        pi_process_device_actions();
        PI_WORKER_ACTIVE.store(false, Ordering::SeqCst);

        // Сбрасываем событие
        ke_reset_event(&*event_ptr);
    }

    dbg_print("   [PnP] Worker thread exited\n");

    // Завершаем поток
    crate::ps::thread::psp_exit_thread(0);
}

/// Добавляет действие в очередь
///
/// # Arguments
/// * `node` - узел устройства (может быть null для глобальных действий)
/// * `action` - тип действия
/// * `flags` - дополнительные флаги
///
/// # Returns
/// STATUS_SUCCESS или код ошибки
pub unsafe fn pi_queue_device_action(
    node: *mut DEVICE_NODE,
    action: DeviceAction,
    flags: u32,
) -> NTSTATUS {
    use crate::ex::pool::{ex_allocate_pool_with_tag, POOL_TYPE};
    use crate::ke::event::{ke_set_event, ke_reset_event};
    use crate::ke::spinlock::{ke_acquire_spin_lock, ke_release_spin_lock};
    use crate::nt::ntstatus::*;

    // Выделяем запрос
    let request = ex_allocate_pool_with_tag(
        POOL_TYPE::NonPagedPool,
        core::mem::size_of::<DeviceActionRequest>(),
        u32::from_le_bytes(*b"PnAc"),
    ) as *mut DeviceActionRequest;

    if request.is_null() {
        return STATUS_INSUFFICIENT_RESOURCES;
    }

    // Инициализируем
    core::ptr::write(request, DeviceActionRequest::new(node, action));
    (*request).flags = flags;

    // Добавляем в очередь
    let old_irql = ke_acquire_spin_lock(&PI_DEVICE_ACTION_LOCK);

    // Проверка на дубликаты (дедупликация)
    // Если такой же action для того же devnode уже в очереди - пропускаем
    let mut current = PI_DEVICE_ACTION_QUEUE.flink;
    let mut found_duplicate = false;
    
    while current != &raw mut PI_DEVICE_ACTION_QUEUE {
        let entry = current as *mut DeviceActionRequest;
        if (*entry).device_node == node && (*entry).action == action {
            found_duplicate = true;
            break;
        }
        current = (*current).flink;
    }

    if found_duplicate {
        // Дубликат найден - освобождаем request и возвращаемся
        ke_release_spin_lock(&PI_DEVICE_ACTION_LOCK, old_irql);
        use crate::ex::pool::ex_free_pool_with_tag;
        ex_free_pool_with_tag(request as PVOID, u32::from_le_bytes(*b"PnAc"));
        return STATUS_SUCCESS;
    }

    // Сбрасываем lock — есть работа
    ke_reset_event(&*(&raw mut PI_ENUMERATION_LOCK));

    LIST_ENTRY::insert_tail(&raw mut PI_DEVICE_ACTION_QUEUE, &mut (*request).list_entry);

    ke_release_spin_lock(&PI_DEVICE_ACTION_LOCK, old_irql);

    // Пробуждаем worker thread
    ke_set_event(&*(&raw mut PI_ENUMERATION_EVENT), 0, false);

    STATUS_SUCCESS
}

/// Извлекает следующее действие из очереди
///
/// # Returns
/// Указатель на запрос или null если очередь пуста
pub unsafe fn pi_dequeue_device_action() -> *mut DeviceActionRequest {
    use crate::ke::spinlock::{ke_acquire_spin_lock, ke_release_spin_lock};

    let old_irql = ke_acquire_spin_lock(&PI_DEVICE_ACTION_LOCK);

    let request = if LIST_ENTRY::is_empty(&raw const PI_DEVICE_ACTION_QUEUE) {
        core::ptr::null_mut()
    } else {
        let queue_ptr = &raw mut PI_DEVICE_ACTION_QUEUE;
        let entry = (*queue_ptr).flink;
        LIST_ENTRY::remove_entry(entry);

        // Получаем DeviceActionRequest из LIST_ENTRY
        // list_entry - первое поле в структуре
        entry as *mut DeviceActionRequest
    };

    ke_release_spin_lock(&PI_DEVICE_ACTION_LOCK, old_irql);

    request
}

/// Освобождает обработанный запрос
pub unsafe fn pi_free_device_action(request: *mut DeviceActionRequest) {
    use crate::ex::pool::ex_free_pool_with_tag;
    use crate::ke::event::ke_set_event;

    if request.is_null() {
        return;
    }

    // Сигнализируем событие завершения если есть
    let event = (*request).completion_event;
    if !event.is_null() {
        ke_set_event(&*event, 0, false);
    }

    ex_free_pool_with_tag(request as PVOID, u32::from_le_bytes(*b"PnAc"));
}

/// Проверяет, пуста ли очередь
pub unsafe fn pi_is_action_queue_empty() -> bool {
    use crate::ke::spinlock::{ke_acquire_spin_lock, ke_release_spin_lock};

    let old_irql = ke_acquire_spin_lock(&PI_DEVICE_ACTION_LOCK);
    let empty = LIST_ENTRY::is_empty(&raw const PI_DEVICE_ACTION_QUEUE);
    ke_release_spin_lock(&PI_DEVICE_ACTION_LOCK, old_irql);

    empty
}

/// Ожидает завершения обработки текущей очереди действий
///
/// Блокируется до тех пор, пока worker thread не обработает все pending actions.
/// Используется при инициализации для синхронного ожидания enumeration.
pub unsafe fn pi_wait_for_enumeration_idle() {
    use crate::ke::wait::ke_wait_for_single_object;

    // Ждём пока worker завершит обработку
    let lock_ptr = &raw mut PI_ENUMERATION_LOCK;
    let _status = ke_wait_for_single_object(
        lock_ptr as PVOID,
        0, // Executive
        0, // KernelMode
        false, // Alertable
        None, // Infinite timeout
    );
}

// =============================================================================
// Worker Thread
// =============================================================================

/// Основная функция PnP worker thread
///
/// Вызывается из системного worker thread для обработки очереди действий.
/// Работает на PASSIVE_LEVEL.
pub unsafe fn pi_process_device_actions() {
    use crate::ke::event::{ke_reset_event, ke_set_event};

    // Помечаем что worker активен
    PI_WORKER_ACTIVE.store(true, Ordering::SeqCst);

    loop {
        // Извлекаем следующее действие
        let request = unsafe { pi_dequeue_device_action() };

        if request.is_null() {
            // Очередь пуста — выходим
            break;
        }

        let action = unsafe { (*request).action };
        let node = unsafe { (*request).device_node };

        // Обрабатываем действие
        let status = match action {
            DeviceAction::EnumDeviceTree => unsafe { pi_action_enum_device_tree(node) },
            DeviceAction::EnumRootDevices => unsafe { pi_action_enum_root_devices() },
            DeviceAction::StartDevice => unsafe { pi_action_start_device(node) },
            DeviceAction::StopDevice => unsafe { pi_action_stop_device(node) },
            DeviceAction::RemoveDevice => unsafe { pi_action_remove_device(node) },
            DeviceAction::SurpriseRemoval => unsafe { pi_action_surprise_removal(node) },
            DeviceAction::InvalidateRelations => unsafe { pi_action_invalidate_relations(node) },
            DeviceAction::InvalidateState => unsafe { pi_action_invalidate_state(node) },
            DeviceAction::AddBootDevices => unsafe { pi_action_add_boot_devices() },
            DeviceAction::ResetDevice => unsafe { pi_action_reset_device(node) },
            DeviceAction::QueryState => unsafe { pi_action_query_state(node) },
        };

        unsafe { (*request).status = status };

        // Освобождаем запрос
        unsafe { pi_free_device_action(request) };
    }

    // Сбрасываем событие пробуждения
    unsafe { ke_reset_event(&*(&raw mut PI_ENUMERATION_EVENT)) };

    // Сигнализируем что очередь пуста — разблокируем ожидающих
    unsafe { ke_set_event(&*(&raw mut PI_ENUMERATION_LOCK), 0, false) };

    PI_WORKER_ACTIVE.store(false, Ordering::SeqCst);
}

// =============================================================================
// Обработчики действий
// =============================================================================

/// EnumDeviceTree — перечисление дерева устройств
///
/// Запускает enumeration начиная с указанного узла или root.
/// Для каждого bus enumerator'а вызывается IRP_MN_QUERY_DEVICE_RELATIONS(BusRelations).
unsafe fn pi_action_enum_device_tree(node: *mut DEVICE_NODE) -> NTSTATUS {
    use crate::kd::dbg_print;
    use crate::nt::ntstatus::*;
    use super::devnode::IOP_ROOT_DEVICE_NODE;

    dbg_print("   [PnP] EnumDeviceTree action\n");

    let start_node = if node.is_null() {
        IOP_ROOT_DEVICE_NODE
    } else {
        node
    };

    if start_node.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    // Если это root node — сначала делаем ACPI enumeration
    if start_node == IOP_ROOT_DEVICE_NODE {
        let _status = pi_action_enum_root_devices();
    }

    // Выполняем enumeration дерева (BusRelations для каждого bus driver)
    let status = pi_enumerate_device_tree(start_node);

    // Выводим дерево устройств после enumeration
    // super::devnode::iop_dump_device_tree();

    status
}

/// Рекурсивный обход дерева для enumeration
unsafe fn pi_enumerate_device_tree(node: *mut DEVICE_NODE) -> NTSTATUS {
    use crate::kd::dbg_print;
    use crate::nt::ntstatus::*;

    if node.is_null() {
        return STATUS_SUCCESS;
    }

    // Если у узла есть PDO — запрашиваем BusRelations
    let pdo = (*node).physical_device_object;
    if !pdo.is_null() {
        // Получаем верхний объект в стеке
        let top_device = (*pdo).attached_device;
        if !top_device.is_null() {
            // Отправляем IRP_MN_QUERY_DEVICE_RELATIONS(BusRelations)
            let _status = pi_query_bus_relations(top_device);
        }
    }

    // Рекурсивно обрабатываем детей
    let mut child = (*node).child;
    while !child.is_null() {
        pi_enumerate_device_tree(child);
        child = (*child).sibling;
    }

    STATUS_SUCCESS
}

/// Запрашивает BusRelations у устройства
unsafe fn pi_query_bus_relations(device: *mut super::super::device::DEVICE_OBJECT) -> NTSTATUS {
    use crate::io::types::IRP_MJ_PNP;
    use crate::io::pnp::{IRP_MN_QUERY_DEVICE_RELATIONS, DEVICE_RELATIONS, DEVICE_RELATION_TYPE};
    use crate::io::irp::{io_allocate_irp, io_free_irp, io_get_next_irp_stack_location, io_set_next_irp_stack_location};
    use crate::kd::dbg_print;
    use crate::nt::ntstatus::*;

    if device.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    // Аллоцируем IRP
    let stack_size = (*device).stack_size;
    let irp = io_allocate_irp(stack_size as i8, false);
    if irp.is_null() {
        return STATUS_INSUFFICIENT_RESOURCES;
    }

    // Настраиваем IRP
    (*irp).io_status.status = STATUS_NOT_SUPPORTED;
    (*irp).io_status.information = 0;

    // Настраиваем IO_STACK_LOCATION (используем next, потом set)
    let stack = io_get_next_irp_stack_location(irp);
    if !stack.is_null() {
        (*stack).major_function = IRP_MJ_PNP;
        (*stack).minor_function = IRP_MN_QUERY_DEVICE_RELATIONS;
        (*stack).parameters.query_device_relations.relation_type = DEVICE_RELATION_TYPE::BusRelations;
        (*stack).device_object = device;
    }
    io_set_next_irp_stack_location(irp);

    // Синхронный вызов драйвера
    let driver = (*device).driver_object;
    if driver.is_null() {
        io_free_irp(irp);
        return STATUS_INVALID_DEVICE_REQUEST;
    }

    let dispatch = (*driver).major_function[IRP_MJ_PNP as usize];
    let status = if let Some(func) = dispatch {
        func(device, irp)
    } else {
        STATUS_NOT_SUPPORTED
    };

    // Обрабатываем результат
    if status >= 0 {
        let relations = (*irp).io_status.information as *const DEVICE_RELATIONS;
        if !relations.is_null() && (*relations).count > 0 {
            dbg_print("   [PnP] BusRelations returned ");
            crate::kd::dbg_print_num((*relations).count as u64);
            dbg_print(" devices\n");

            // NOTE: Обработка BusRelations теперь происходит в drvload.rs::pnp_query_bus_relations
            // pi_process_new_relations более не используется
            // Старый асинхронный код оставлен для справки

            // Освобождаем DEVICE_RELATIONS
            crate::ex::pool::ex_free_pool_with_tag(
                relations as PVOID,
                u32::from_le_bytes(*b"PnRl"),
            );
        }
    }

    io_free_irp(irp);
    status
}

/// Обрабатывает новые PDO из BusRelations
unsafe fn pi_process_new_relations(relations: *const crate::io::pnp::DEVICE_RELATIONS) {
    use super::devnode::{pip_allocate_device_node, pi_insert_dev_node, IOP_ROOT_DEVICE_NODE};
    use super::state::{DNF_ENUMERATED, PnpDevnodeState};
    use crate::kd::dbg_print;

    if relations.is_null() {
        return;
    }

    for i in 0..(*relations).count as usize {
        // objects - это массив из 1 элемента, но реально выделено больше памяти
        let pdo = *(*relations).objects.as_ptr().add(i);
        if pdo.is_null() {
            continue;
        }

        // Проверяем, есть ли уже devnode для этого PDO
        let existing_node = super::devnode::iop_get_device_node(pdo);
        if !existing_node.is_null() {
            // Уже есть — помечаем как enumerated
            (*existing_node).set_flag(DNF_ENUMERATED);
            continue;
        }

        // Создаём новый devnode
        let node = pip_allocate_device_node(pdo);
        if node.is_null() {
            dbg_print("   [PnP] Failed to allocate devnode for new PDO\n");
            continue;
        }

        (*node).set_flag(DNF_ENUMERATED);

        // NOTE: Эта функция больше не используется (отключена в pi_query_bus_relations)
        // Parent определяется в pnp_query_bus_relations() из drvload.rs
        // где parent берётся из PDO device_object_extension->device_node
        pi_insert_dev_node(node, IOP_ROOT_DEVICE_NODE);

        super::devnode::pi_set_dev_node_state(node, PnpDevnodeState::Initialized);

        // Пытаемся найти драйвер и запустить устройство
        pi_process_new_device_node(node);
    }
}

/// Обрабатывает новый devnode — поиск драйвера и запуск
unsafe fn pi_process_new_device_node(node: *mut DEVICE_NODE) -> NTSTATUS {
    use crate::kd::dbg_print;
    use crate::nt::ntstatus::*;

    if node.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    let pdo = (*node).physical_device_object;
    if pdo.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    // Запрашиваем Hardware ID у PDO
    let hardware_id = pi_query_device_id(pdo);

    if !hardware_id.is_empty() {
        dbg_print("   [PnP] New device: ");
        dbg_print(&hardware_id);
        dbg_print("\n");

        // Ищем драйвер и вызываем AddDevice
        let status = super::drvload::pnp_process_new_device(&hardware_id, pdo);
        if status >= 0 && !(*pdo).attached_device.is_null() {
            (*node).set_flag(super::state::DNF_ADDED);
            // Запускаем устройство
            return pi_start_device(node);
        }
    }

    STATUS_SUCCESS
}

/// Запрашивает Hardware ID у PDO через IRP_MN_QUERY_ID
unsafe fn pi_query_device_id(pdo: *mut super::super::device::DEVICE_OBJECT) -> alloc::string::String {
    use crate::io::types::IRP_MJ_PNP;
    use crate::io::pnp::IRP_MN_QUERY_ID;
    use crate::io::irp::{io_allocate_irp, io_free_irp, io_get_next_irp_stack_location, io_set_next_irp_stack_location};
    use crate::nt::ntstatus::*;
    use alloc::string::String;
    
    // BUS_QUERY_HARDWARE_IDS = 1
    const BUS_QUERY_HARDWARE_IDS: u32 = 1;

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
        (*stack).parameters.query_id.id_type = BUS_QUERY_HARDWARE_IDS;
        (*stack).device_object = pdo;
    }
    io_set_next_irp_stack_location(irp);

    // Вызываем PDO напрямую (не через device stack)
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
        // Результат — multi-sz строка UTF-16
        let buffer = (*irp).io_status.information as *const u16;
        let mut len = 0;
        while *buffer.add(len) != 0 {
            len += 1;
        }
        // Конвертируем первый ID в String
        let slice = core::slice::from_raw_parts(buffer, len);
        String::from_utf16_lossy(slice)
    } else {
        String::new()
    };

    io_free_irp(irp);
    result
}

/// Запускает устройство через IRP_MN_START_DEVICE
unsafe fn pi_start_device(node: *mut DEVICE_NODE) -> NTSTATUS {
    use crate::io::types::IRP_MJ_PNP;
    use crate::io::pnp::IRP_MN_START_DEVICE;
    use crate::io::irp::{io_allocate_irp, io_free_irp, io_get_next_irp_stack_location, io_set_next_irp_stack_location};
    use crate::kd::dbg_print;
    use crate::nt::ntstatus::*;

    if node.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    let pdo = (*node).physical_device_object;
    if pdo.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    // Получаем верхний объект в стеке
    let mut top_device = pdo;
    while !(*top_device).attached_device.is_null() {
        top_device = (*top_device).attached_device;
    }

    dbg_print("   [PnP] Starting device...\n");

    // Устанавливаем состояние
    super::devnode::pi_set_dev_node_state(node, super::state::PnpDevnodeState::StartPending);

    // Запрашиваем boot resources у PDO через QUERY_RESOURCES
    // Это стандартный NT контракт для получения ресурсов устройства
    let boot_resources = pi_query_boot_resources(pdo);
    
    if !boot_resources.is_null() {
        dbg_print("   [PnP] Got boot resources from PDO\n");
    }

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
        (*stack).minor_function = IRP_MN_START_DEVICE;
        (*stack).device_object = top_device;
        
        // Передаём boot resources через parameters.start_device
        // В NT архитектуре PnP Manager передаёт ресурсы таким образом
        (*stack).parameters.start_device.allocated_resources = boot_resources as crate::nt::PVOID;
        (*stack).parameters.start_device.allocated_resources_translated = boot_resources as crate::nt::PVOID;
    }
    io_set_next_irp_stack_location(irp);

    // Отправляем IRP
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
        super::devnode::pi_set_dev_node_state(node, super::state::PnpDevnodeState::Started);
        dbg_print("   [PnP] Device started successfully\n");
    } else {
        dbg_print("   [PnP] Device start failed\n");
        (*node).set_problem(super::state::CM_PROB_FAILED_START);
    }

    io_free_irp(irp);
    status
}

/// Запрашивает boot resources у PDO через IRP_MN_QUERY_RESOURCES
/// 
/// Возвращает указатель на CM_RESOURCE_LIST или NULL если ресурсов нет
unsafe fn pi_query_boot_resources(pdo: *mut crate::io::device::DEVICE_OBJECT) -> *mut u8 {
    use crate::io::irp::{io_allocate_irp, io_free_irp, io_get_next_irp_stack_location, io_set_next_irp_stack_location};
    use crate::io::pnp::IRP_MN_QUERY_RESOURCES;
    use crate::io::types::IRP_MJ_PNP;
    use crate::nt::ntstatus::*;

    if pdo.is_null() {
        return core::ptr::null_mut();
    }

    // QUERY_RESOURCES идёт напрямую к PDO (не через attached stack)
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

unsafe fn pi_action_enum_root_devices() -> NTSTATUS {
    use crate::acpi::{acpi_interpreter, acpi_is_ready, AcpiReadyLevel};
    use crate::kd::dbg_print;
    use crate::nt::ntstatus::*;
    use super::devnode::IOP_ROOT_DEVICE_NODE;
    use super::pnpacpi::{acpi_enumerate_namespace, acpi_create_device_nodes};

    dbg_print("   [PnP] EnumRootDevices action\n");

    // Проверяем что ACPI Interpreter готов
    if !acpi_is_ready(AcpiReadyLevel::AmlReady) {
        dbg_print("   [PnP] EnumRootDevices: ACPI Interpreter not ready\n");
        return STATUS_UNSUCCESSFUL;
    }

    let Some(interpreter) = acpi_interpreter() else {
        dbg_print("   [PnP] EnumRootDevices: No ACPI Interpreter\n");
        return STATUS_UNSUCCESSFUL;
    };

    // Enumerate ACPI namespace
    dbg_print("   [PnP] Enumerating ACPI namespace...\n");
    let result = acpi_enumerate_namespace(interpreter);

    if result.devices.is_empty() {
        dbg_print("   [PnP] No ACPI devices found\n");
        return STATUS_SUCCESS;
    }

    dbg_print("   [PnP] Found ");
    crate::kd::dbg_print_num(result.devices.len() as u64);
    dbg_print(" ACPI device(s), ");
    crate::kd::dbg_print_num(result.pci_bridge_count as u64);
    dbg_print(" PCI bridge(s)\n");

    // Создаём devnodes для ACPI устройств
    let parent_node = IOP_ROOT_DEVICE_NODE;
    if parent_node.is_null() {
        dbg_print("   [PnP] EnumRootDevices: No root device node\n");
        return STATUS_UNSUCCESSFUL;
    }

    let status = acpi_create_device_nodes(&result.devices, parent_node);
    if status < 0 {
        dbg_print("   [PnP] Failed to create ACPI device nodes\n");
    }

    status
}

unsafe fn pi_action_start_device(node: *mut DEVICE_NODE) -> NTSTATUS {
    pi_start_device(node)
}

unsafe fn pi_action_stop_device(node: *mut DEVICE_NODE) -> NTSTATUS {
    use crate::kd::dbg_print;
    use crate::nt::ntstatus::*;
    use crate::io::irp::{io_allocate_irp, io_free_irp, io_get_next_irp_stack_location, io_set_next_irp_stack_location};
    use crate::io::pnp::IRP_MN_STOP_DEVICE;
    use crate::io::types::IRP_MJ_PNP;
    use super::state::PnpDevnodeState;
    use super::devnode::pi_set_dev_node_state;

    if node.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    let pdo = (*node).physical_device_object;
    if pdo.is_null() {
        return STATUS_UNSUCCESSFUL;
    }

    // Проверяем текущее состояние
    if !(*node).state.is_started() {
        // Устройство не запущено - нечего останавливать
        return STATUS_SUCCESS;
    }

    dbg_print("   [PnP] Stopping device\n");

    // Получаем верхний объект в стеке (FDO)
    let mut top_device = pdo;
    while !(*top_device).attached_device.is_null() {
        top_device = (*top_device).attached_device;
    }

    // Создаём IRP_MN_STOP_DEVICE
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
        (*stack).minor_function = IRP_MN_STOP_DEVICE;
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

    io_free_irp(irp);

    if status >= 0 {
        // Обновляем состояние devnode
        pi_set_dev_node_state(node, PnpDevnodeState::Stopped);
        dbg_print("   [PnP] Device stopped\n");
    }

    status
}

unsafe fn pi_action_remove_device(node: *mut DEVICE_NODE) -> NTSTATUS {
    use crate::kd::dbg_print;
    use crate::nt::ntstatus::*;
    use crate::io::irp::{io_allocate_irp, io_free_irp, io_get_next_irp_stack_location, io_set_next_irp_stack_location};
    use crate::io::pnp::IRP_MN_REMOVE_DEVICE;
    use crate::io::types::IRP_MJ_PNP;
    use super::state::{PnpDevnodeState, DNF_DEVICE_GONE};
    use super::devnode::{iop_free_device_node, pi_set_dev_node_state};

    if node.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    let pdo = (*node).physical_device_object;
    if pdo.is_null() {
        // PDO уже удалён - просто удаляем devnode
        iop_free_device_node(node);
        return STATUS_SUCCESS;
    }

    dbg_print("   [PnP] Removing device\n");

    // Получаем верхний объект в стеке (FDO)
    let mut top_device = pdo;
    while !(*top_device).attached_device.is_null() {
        top_device = (*top_device).attached_device;
    }

    // Создаём IRP_MN_REMOVE_DEVICE
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
        (*stack).minor_function = IRP_MN_REMOVE_DEVICE;
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

    io_free_irp(irp);

    // После успешного удаления обновляем devnode
    if status >= 0 {
        (*node).set_flag(DNF_DEVICE_GONE);
        pi_set_dev_node_state(node, PnpDevnodeState::Removed);
        dbg_print("   [PnP] Device removed\n");
        
        // Удаляем devnode из дерева (но не освобождаем память сразу)
        // В полной реализации devnode остаётся до DeletePendingCloses
    }

    status
}

unsafe fn pi_action_surprise_removal(node: *mut DEVICE_NODE) -> NTSTATUS {
    use crate::kd::dbg_print;
    use crate::nt::ntstatus::*;
    use crate::io::irp::{io_allocate_irp, io_free_irp, io_get_next_irp_stack_location, io_set_next_irp_stack_location};
    use crate::io::pnp::IRP_MN_SURPRISE_REMOVAL;
    use crate::io::types::IRP_MJ_PNP;
    use super::state::{PnpDevnodeState, DNF_DEVICE_GONE};
    use super::devnode::pi_set_dev_node_state;

    if node.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    let pdo = (*node).physical_device_object;
    if pdo.is_null() {
        return STATUS_UNSUCCESSFUL;
    }

    dbg_print("   [PnP] Surprise removal\n");

    // Помечаем устройство как gone
    (*node).set_flag(DNF_DEVICE_GONE);

    // Получаем верхний объект в стеке
    let mut top_device = pdo;
    while !(*top_device).attached_device.is_null() {
        top_device = (*top_device).attached_device;
    }

    // Создаём IRP_MN_SURPRISE_REMOVAL
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
        (*stack).minor_function = IRP_MN_SURPRISE_REMOVAL;
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

    io_free_irp(irp);

    // После surprise removal переходим в RemovePendingCloses
    if status >= 0 {
        pi_set_dev_node_state(node, PnpDevnodeState::RemovePendingCloses);
        dbg_print("   [PnP] Surprise removal completed\n");
    }

    status
}

unsafe fn pi_action_invalidate_relations(node: *mut DEVICE_NODE) -> NTSTATUS {
    use crate::kd::dbg_print;
    use crate::nt::ntstatus::*;

    dbg_print("   [PnP] InvalidateRelations action\n");
    // Перечисляем дерево заново
    pi_action_enum_device_tree(node)
}

unsafe fn pi_action_invalidate_state(node: *mut DEVICE_NODE) -> NTSTATUS {
    use crate::kd::dbg_print;
    use crate::nt::ntstatus::*;
    use crate::io::irp::{io_allocate_irp, io_free_irp, io_get_next_irp_stack_location, io_set_next_irp_stack_location};
    use crate::io::pnp::IRP_MN_QUERY_PNP_DEVICE_STATE;
    use crate::io::types::IRP_MJ_PNP;

    if node.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    let pdo = (*node).physical_device_object;
    if pdo.is_null() {
        return STATUS_UNSUCCESSFUL;
    }

    dbg_print("   [PnP] Querying device state\n");

    // Получаем верхний объект
    let mut top_device = pdo;
    while !(*top_device).attached_device.is_null() {
        top_device = (*top_device).attached_device;
    }

    // Создаём IRP_MN_QUERY_PNP_DEVICE_STATE
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
        (*stack).minor_function = IRP_MN_QUERY_PNP_DEVICE_STATE;
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

    // Обрабатываем результат - device state flags в io_status.information
    if status >= 0 {
        let device_state = (*irp).io_status.information as u32;
        
        // PNP_DEVICE_STATE flags (из WDK):
        // 0x00000001 - PNP_DEVICE_DISABLED
        // 0x00000002 - PNP_DEVICE_DONT_DISPLAY_IN_UI
        // 0x00000004 - PNP_DEVICE_FAILED
        // 0x00000008 - PNP_DEVICE_REMOVED
        // 0x00000010 - PNP_DEVICE_RESOURCE_REQUIREMENTS_CHANGED
        // 0x00000020 - PNP_DEVICE_NOT_DISABLEABLE
        
        if (device_state & 0x00000004) != 0 {
            // Device failed
            use super::state::CM_PROB_FAILED_START;
            (*node).set_problem(CM_PROB_FAILED_START);
        }
        
        if (device_state & 0x00000008) != 0 {
            // Device removed
            use super::state::DNF_DEVICE_GONE;
            (*node).set_flag(DNF_DEVICE_GONE);
        }
    }

    io_free_irp(irp);
    status
}

unsafe fn pi_action_add_boot_devices() -> NTSTATUS {
    use crate::kd::dbg_print;
    use crate::nt::ntstatus::*;

    dbg_print("   [PnP] AddBootDevices action\n");
    // Boot устройства уже добавлены в Phase 1
    STATUS_SUCCESS
}

unsafe fn pi_action_reset_device(node: *mut DEVICE_NODE) -> NTSTATUS {
    use crate::kd::dbg_print;
    use crate::nt::ntstatus::*;

    if node.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    dbg_print("   [PnP] Resetting device (Stop + Start)\n");

    // 1. Останавливаем устройство
    let status = pi_action_stop_device(node);
    if status < 0 {
        dbg_print("   [PnP] Failed to stop device for reset\n");
        return status;
    }

    // 2. Запускаем устройство заново
    let status = pi_start_device(node);
    if status < 0 {
        dbg_print("   [PnP] Failed to start device after reset\n");
        return status;
    }

    dbg_print("   [PnP] Device reset completed\n");
    STATUS_SUCCESS
}

unsafe fn pi_action_query_state(node: *mut DEVICE_NODE) -> NTSTATUS {
    // QueryState - то же что InvalidateState (запрос device state)
    pi_action_invalidate_state(node)
}

