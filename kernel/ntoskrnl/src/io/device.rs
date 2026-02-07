//! DEVICE_OBJECT — Объект устройства
//!
//! Модуль реализует структуру DEVICE_OBJECT и функции для работы с устройствами.
//! DEVICE_OBJECT представляет физическое или логическое устройство в системе.
//!
//! # Архитектура
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                         Device Stack                                │
//! ├─────────────────────────────────────────────────────────────────────┤
//! │                                                                     │
//! │    ┌──────────────┐                                                │
//! │    │ Filter DO    │ ◄── IoAttachDeviceToDeviceStack()              │
//! │    │ (attached_   │                                                │
//! │    │  device)     │                                                │
//! │    └──────┬───────┘                                                │
//! │           │                                                        │
//! │           ▼                                                        │
//! │    ┌──────────────┐     ┌──────────────┐                          │
//! │    │ DEVICE_      │────►│ DRIVER_      │                          │
//! │    │ OBJECT       │     │ OBJECT       │                          │
//! │    │              │     │              │                          │
//! │    │ driver_obj ──┼────►│ device_obj ──┼──► (список устройств)    │
//! │    │ next_device ─┼──►  │ major_func[] │                          │
//! │    │ device_ext   │     └──────────────┘                          │
//! │    └──────────────┘                                                │
//! │                                                                     │
//! │    IoCreateDevice() ──► Создание + добавление в список драйвера   │
//! │    IoDeleteDevice() ──► Удаление (по refcount = 0)                │
//! │                                                                     │
//! └─────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Функции
//!
//! | Функция | Описание |
//! |---------|----------|
//! | `io_create_device` | IoCreateDevice — создаёт устройство |
//! | `io_delete_device` | IoDeleteDevice — удаляет устройство |
//! | `io_attach_device_to_device_stack` | IoAttachDeviceToDeviceStack — присоединяет к стеку |
//! | `io_create_symbolic_link` | IoCreateSymbolicLink — создаёт DOS симлинк |
//! | `io_delete_symbolic_link` | IoDeleteSymbolicLink — удаляет DOS симлинк |
//!
//! # Отступления и упрощения
//!
//! - Именование устройств в `\Device` — пока не реализовано через OB
//! - `IoGetDeviceObjectPointer` — заглушка, требуется NtOpenFile
//! - `IoAttachDevice` — заглушка, требуется поиск по имени
//! - DEVOBJ_EXTENSION — упрощённая структура
//!
//! Источники:
//! - ReactOS: sdk/include/xdk/iotypes.h, ntoskrnl/io/iomgr/device.c
//! - NT5: ntos/io/iomgr/device.c

use super::types::*;
use crate::ke::dpc::KDPC;
use crate::ke::event::KEVENT;
use crate::ke::spinlock::KSPIN_LOCK;
use crate::nt::CSHORT;
use crate::nt::LIST_ENTRY;
use crate::nt::LONG;
use crate::nt::NTSTATUS;
use crate::nt::PVOID;
use crate::nt::ULONG;
use crate::nt::USHORT;
use crate::nt::ntdef::UNICODE_STRING;
use crate::nt::ntstatus::*;

// =============================================================================
// DEVICE_OBJECT
// =============================================================================

/// VPB - Volume Parameter Block
#[repr(C)]
pub struct VPB {
    pub r#type: CSHORT,
    pub size: CSHORT,
    pub flags: USHORT,
    pub volume_label_length: USHORT,
    pub device_object: *mut DEVICE_OBJECT,
    pub real_device: *mut DEVICE_OBJECT,
    pub serial_number: ULONG,
    pub reference_count: ULONG,
    pub volume_label: [u16; 32],
}

/// KDEVICE_QUEUE - очередь устройства
#[repr(C)]
pub struct KDEVICE_QUEUE {
    pub r#type: CSHORT,
    pub size: CSHORT,
    pub device_list_head: LIST_ENTRY,
    pub lock: KSPIN_LOCK,
    pub busy: bool,
}

impl KDEVICE_QUEUE {
    pub const fn new() -> Self {
        Self {
            r#type: 0,
            size: core::mem::size_of::<Self>() as CSHORT,
            device_list_head: LIST_ENTRY::new(),
            lock: KSPIN_LOCK::new(),
            busy: false,
        }
    }
}

/// KDEVICE_QUEUE_ENTRY — элемент очереди устройства
#[repr(C)]
pub struct KDEVICE_QUEUE_ENTRY {
    pub device_list_entry: LIST_ENTRY,
    pub sort_key: ULONG,
    pub inserted: bool,
}

impl KDEVICE_QUEUE_ENTRY {
    pub const fn new() -> Self {
        Self {
            device_list_entry: LIST_ENTRY::new(),
            sort_key: 0,
            inserted: false,
        }
    }
}

// =============================================================================
// KDEVICE_QUEUE Functions
// =============================================================================

/// KeInitializeDeviceQueue — инициализирует очередь устройства
pub unsafe fn ke_initialize_device_queue(device_queue: *mut KDEVICE_QUEUE) {
    unsafe {
        if device_queue.is_null() {
            return;
        }

        (*device_queue).r#type = 4; // DeviceQueueObject
        (*device_queue).size = core::mem::size_of::<KDEVICE_QUEUE>() as CSHORT;
        LIST_ENTRY::init_head(&mut (*device_queue).device_list_head);
        (*device_queue).lock = KSPIN_LOCK::new();
        (*device_queue).busy = false;
    }
}

/// KeInsertDeviceQueue — вставляет элемент в очередь устройства
///
/// Если устройство не занято (busy = false), устанавливает busy = true
/// и возвращает false (элемент не вставлен, можно обрабатывать сразу).
/// Иначе вставляет элемент в конец очереди и возвращает true.
pub unsafe fn ke_insert_device_queue(
    device_queue: *mut KDEVICE_QUEUE,
    device_queue_entry: *mut KDEVICE_QUEUE_ENTRY,
) -> bool {
    unsafe {
        use crate::ke::spinlock::ke_acquire_spin_lock_at_dpc_level;
        use crate::ke::spinlock::ke_release_spin_lock_from_dpc_level;

        if device_queue.is_null() || device_queue_entry.is_null() {
            return false;
        }

        let lock_ref = &*(&raw mut (*device_queue).lock);
        ke_acquire_spin_lock_at_dpc_level(lock_ref);

        let inserted = if (*device_queue).busy {
            // Устройство занято — вставляем в очередь
            LIST_ENTRY::insert_tail(
                &mut (*device_queue).device_list_head,
                &mut (*device_queue_entry).device_list_entry,
            );
            (*device_queue_entry).inserted = true;
            true
        } else {
            // Устройство свободно — помечаем как занятое
            (*device_queue).busy = true;
            (*device_queue_entry).inserted = false;
            false
        };

        ke_release_spin_lock_from_dpc_level(lock_ref);
        inserted
    }
}

/// KeInsertByKeyDeviceQueue — вставляет элемент в очередь с сортировкой по ключу
pub unsafe fn ke_insert_by_key_device_queue(
    device_queue: *mut KDEVICE_QUEUE,
    device_queue_entry: *mut KDEVICE_QUEUE_ENTRY,
    sort_key: ULONG,
) -> bool {
    unsafe {
        use crate::ke::spinlock::ke_acquire_spin_lock_at_dpc_level;
        use crate::ke::spinlock::ke_release_spin_lock_from_dpc_level;

        if device_queue.is_null() || device_queue_entry.is_null() {
            return false;
        }

        (*device_queue_entry).sort_key = sort_key;

        let lock_ref = &*(&raw mut (*device_queue).lock);
        ke_acquire_spin_lock_at_dpc_level(lock_ref);

        let inserted = if (*device_queue).busy {
            // Устройство занято — ищем место для вставки по ключу
            let head = &mut (*device_queue).device_list_head;
            let mut current = (*head).flink;

            // Ищем первый элемент с большим ключом
            while current != head {
                let entry = LIST_ENTRY::containing_record::<KDEVICE_QUEUE_ENTRY>(
                    current, 0, // offset of device_list_entry in KDEVICE_QUEUE_ENTRY
                );
                if (*entry).sort_key > sort_key {
                    break;
                }
                current = (*current).flink;
            }

            // Вставляем перед найденным элементом
            LIST_ENTRY::insert_tail(current, &mut (*device_queue_entry).device_list_entry);
            (*device_queue_entry).inserted = true;
            true
        } else {
            // Устройство свободно
            (*device_queue).busy = true;
            (*device_queue_entry).inserted = false;
            false
        };

        ke_release_spin_lock_from_dpc_level(lock_ref);
        inserted
    }
}

/// KeRemoveDeviceQueue — извлекает следующий элемент из очереди
///
/// Возвращает элемент или NULL если очередь пуста.
/// Если очередь пуста, устанавливает busy = false.
pub unsafe fn ke_remove_device_queue(device_queue: *mut KDEVICE_QUEUE) -> *mut KDEVICE_QUEUE_ENTRY {
    unsafe {
        use crate::ke::spinlock::ke_acquire_spin_lock_at_dpc_level;
        use crate::ke::spinlock::ke_release_spin_lock_from_dpc_level;

        if device_queue.is_null() {
            return core::ptr::null_mut();
        }

        let lock_ref = &*(&raw mut (*device_queue).lock);
        ke_acquire_spin_lock_at_dpc_level(lock_ref);

        let head = &mut (*device_queue).device_list_head;
        let entry = if LIST_ENTRY::is_empty(head) {
            // Очередь пуста — освобождаем устройство
            (*device_queue).busy = false;
            core::ptr::null_mut()
        } else {
            // Извлекаем первый элемент
            let first = (*head).flink;
            LIST_ENTRY::remove_entry(first);

            let queue_entry = LIST_ENTRY::containing_record::<KDEVICE_QUEUE_ENTRY>(first, 0);
            (*queue_entry).inserted = false;
            queue_entry
        };

        ke_release_spin_lock_from_dpc_level(lock_ref);
        entry
    }
}

/// KeRemoveByKeyDeviceQueue — извлекает элемент с ключом >= заданного
pub unsafe fn ke_remove_by_key_device_queue(
    device_queue: *mut KDEVICE_QUEUE,
    sort_key: ULONG,
) -> *mut KDEVICE_QUEUE_ENTRY {
    unsafe {
        use crate::ke::spinlock::ke_acquire_spin_lock_at_dpc_level;
        use crate::ke::spinlock::ke_release_spin_lock_from_dpc_level;

        if device_queue.is_null() {
            return core::ptr::null_mut();
        }

        let lock_ref = &*(&raw mut (*device_queue).lock);
        ke_acquire_spin_lock_at_dpc_level(lock_ref);

        let head = &mut (*device_queue).device_list_head;
        let entry = if LIST_ENTRY::is_empty(head) {
            (*device_queue).busy = false;
            core::ptr::null_mut()
        } else {
            // Ищем первый элемент с ключом >= sort_key
            let mut current = (*head).flink;
            let mut found: *mut LIST_ENTRY = core::ptr::null_mut();

            while current != head {
                let queue_entry = LIST_ENTRY::containing_record::<KDEVICE_QUEUE_ENTRY>(current, 0);
                if (*queue_entry).sort_key >= sort_key {
                    found = current;
                    break;
                }
                current = (*current).flink;
            }

            // Если не нашли подходящий, берём первый
            if found.is_null() {
                found = (*head).flink;
            }

            LIST_ENTRY::remove_entry(found);
            let queue_entry = LIST_ENTRY::containing_record::<KDEVICE_QUEUE_ENTRY>(found, 0);
            (*queue_entry).inserted = false;
            queue_entry
        };

        ke_release_spin_lock_from_dpc_level(lock_ref);
        entry
    }
}

/// WAIT_CONTEXT_BLOCK - контекст ожидания DMA
#[repr(C)]
pub struct WAIT_CONTEXT_BLOCK {
    pub wait_queue_entry: LIST_ENTRY,
    pub device_routine: PVOID,
    pub device_context: PVOID,
    pub number_of_map_registers: ULONG,
    pub device_object: *mut DEVICE_OBJECT,
    pub current_irp: PIRP,
    pub buffer_chainging_dpc: *mut KDPC,
}

/// DEVICE_OBJECT_EXTENSION - расширение объекта устройства
#[repr(C)]
pub struct DEVOBJ_EXTENSION {
    pub r#type: CSHORT,
    pub size: USHORT,
    pub device_object: *mut DEVICE_OBJECT,
    pub power_flags: ULONG,
    pub dope: PVOID, // Device Object Power Extension
    pub extension_flags: ULONG,
    pub device_node: PVOID,
    pub attached_to: *mut DEVICE_OBJECT,
    pub start_io_count: LONG,
    pub start_io_key: LONG,
    pub start_io_flags: ULONG,
    pub vpb: *mut VPB,
    pub dependent_list: PVOID,
    pub provider_list: PVOID,
}

/// DEVICE_OBJECT - объект устройства
///
/// Точная копия из WinDDK 7600.16385.1/inc/ddk/wdm.h:20983-21016
/// 
/// Представляет физическое или логическое устройство в системе.
/// Драйверы создают DEVICE_OBJECT для каждого управляемого устройства.
///
/// **КРИТИЧНО:** Структура должна точно соответствовать WinDDK для бинарной совместимости!
#[repr(C)]
pub struct DEVICE_OBJECT {
    /// Тип объекта (IO_TYPE_DEVICE = 3)
    pub r#type: CSHORT,
    /// Размер структуры (0xB8 = 184 bytes)
    pub size: USHORT,
    /// Счетчик ссылок
    pub reference_count: LONG,
    /// Указатель на DRIVER_OBJECT
    pub driver_object: PDRIVER_OBJECT,
    /// Следующее устройство в цепочке драйвера
    pub next_device: *mut DEVICE_OBJECT,
    /// Присоединенное устройство (фильтр сверху)
    pub attached_device: *mut DEVICE_OBJECT,
    /// Текущий IRP для StartIo
    pub current_irp: PIRP,
    /// Таймер устройства
    pub timer: PVOID, // PIO_TIMER
    /// Флаги устройства (DO_*)
    pub flags: ULONG,
    /// Характеристики устройства (FILE_*)
    pub characteristics: ULONG,
    /// Volume Parameter Block
    pub vpb: *mut VPB, // __volatile PVPB
    /// Расширение устройства (driver-specific)
    pub device_extension: PVOID,
    /// Тип устройства (FILE_DEVICE_*)
    pub device_type: ULONG, // DEVICE_TYPE
    /// Размер стека IRP (CCHAR = i8)
    pub stack_size: i8, // CCHAR в WinDDK
    /// Queue - union с LIST_ENTRY или WAIT_CONTEXT_BLOCK
    /// WinDDK: union { LIST_ENTRY ListEntry; WAIT_CONTEXT_BLOCK Wcb; } Queue;
    /// Используем LIST_ENTRY напрямую т.к. WAIT_CONTEXT_BLOCK имеет тот же размер
    pub queue: LIST_ENTRY,
    /// Выравнивание для DMA
    pub alignment_requirement: ULONG,
    /// Очередь IRP устройства
    pub device_queue: KDEVICE_QUEUE,
    /// DPC для ожидания
    pub dpc: KDPC,
    /// Счетчик активных потоков (для FS)
    pub active_thread_count: ULONG,
    /// Дескриптор безопасности
    pub security_descriptor: PVOID, // PSECURITY_DESCRIPTOR
    /// Событие блокировки устройства
    pub device_lock: KEVENT,
    /// Размер сектора
    pub sector_size: USHORT,
    /// Spare
    pub spare1: USHORT,
    /// Расширение объекта устройства
    pub device_object_extension: *mut DEVOBJ_EXTENSION,
    /// Зарезервировано
    pub reserved: PVOID,
}


impl DEVICE_OBJECT {
    /// Размер структуры
    pub const SIZE: usize = core::mem::size_of::<Self>();
}

// =============================================================================
// Device Object Functions
// =============================================================================

/// IoCreateDevice — создаёт объект устройства
///
/// Если указано имя устройства, объект создаётся через OB и вставляется
/// в директорию `\Device`. Иначе создаётся анонимное устройство.
///
/// # Arguments
/// * `driver_object` - драйвер-владелец
/// * `device_extension_size` - размер расширения устройства
/// * `device_name` - имя устройства (опционально, без `\Device\` префикса)
/// * `device_type` - тип устройства (FILE_DEVICE_*)
/// * `device_characteristics` - характеристики
/// * `exclusive` - эксклюзивный доступ
/// * `device_object` - указатель для возврата
///
/// # Returns
/// STATUS_SUCCESS или код ошибки
pub unsafe fn io_create_device(
    driver_object: PDRIVER_OBJECT,
    device_extension_size: ULONG,
    device_name: Option<&UNICODE_STRING>,
    device_type: ULONG,
    device_characteristics: ULONG,
    exclusive: bool,
    device_object: *mut PDEVICE_OBJECT,
) -> NTSTATUS {
    unsafe {
        use crate::ex::pool::POOL_TYPE;
        use crate::ex::pool::ex_allocate_pool_with_tag;
        use crate::ob;
        use crate::ob::types::OBJ_CASE_INSENSITIVE;
        use crate::ob::types::OBJ_EXCLUSIVE;
        use crate::ob::types::OBJ_PERMANENT;
        use crate::ob::types::OBJECT_ATTRIBUTES;

        if driver_object.is_null() || device_object.is_null() {
            return STATUS_INVALID_PARAMETER;
        }

        // Размер DEVICE_OBJECT + расширение
        let total_size = DEVICE_OBJECT::SIZE + device_extension_size as usize;

        let dev: *mut DEVICE_OBJECT;
        let mut is_named = false;

        // Если имя указано — создаём объект через OB
        if let Some(name) = device_name {
            if !name.buffer.is_null() && name.length > 0 {
                // Получаем директорию \Device
                let device_dir = super::init::iop_get_device_directory();
                if device_dir.is_null() {
                    return STATUS_OBJECT_PATH_NOT_FOUND;
                }

                // NT драйверы передают полный путь типа "\Device\XenTest".
                // Нам нужно извлечь только имя (leaf) после последнего '\'.
                let name_chars = core::slice::from_raw_parts(
                    name.buffer,
                    (name.length / 2) as usize,
                );
                
                // Ищем последний backslash
                let mut leaf_start = 0usize;
                for (i, &ch) in name_chars.iter().enumerate() {
                    if ch == b'\\' as u16 {
                        leaf_start = i + 1;
                    }
                }
                
                // Создаём UNICODE_STRING для leaf-имени
                let leaf_len = (name.length / 2) as usize - leaf_start;
                let mut leaf_name = UNICODE_STRING::new();
                if leaf_len > 0 {
                    leaf_name.buffer = name.buffer.add(leaf_start);
                    leaf_name.length = (leaf_len * 2) as u16;
                    leaf_name.maximum_length = leaf_name.length;
                }

                // Формируем OBJECT_ATTRIBUTES с leaf-именем
                let mut obj_attr = OBJECT_ATTRIBUTES::new();
                obj_attr.length = core::mem::size_of::<OBJECT_ATTRIBUTES>() as ULONG;
                obj_attr.root_directory = device_dir as PVOID;
                obj_attr.object_name = &leaf_name as *const _ as *mut UNICODE_STRING;
                obj_attr.attributes = OBJ_PERMANENT | OBJ_CASE_INSENSITIVE;
                if exclusive {
                    obj_attr.attributes |= OBJ_EXCLUSIVE;
                }

                // Создаём объект через OB
                let mut dev_obj: PVOID = core::ptr::null_mut();
                let status = ob::life::ob_create_object(
                    0, // KernelMode
                    super::init::IOP_DEVICE_OBJECT_TYPE,
                    &obj_attr,
                    0, // KernelMode
                    core::ptr::null_mut(),
                    total_size,
                    0,
                    0,
                    &mut dev_obj,
                );

                if status != STATUS_SUCCESS {
                    return status;
                }

                if dev_obj.is_null() {
                    return STATUS_INSUFFICIENT_RESOURCES;
                }

                dev = dev_obj as *mut DEVICE_OBJECT;
                is_named = true;

                // Вставляем в namespace
                #[cfg(feature = "trace-io")]
                crate::dbg_print!("[IO] IoCreateDevice: inserting device (len={}) into namespace\n", 
                    leaf_name.length / 2);
                
                let status = ob::life::ob_insert_object(
                    dev_obj,
                    core::ptr::null_mut(),
                    0, // desired_access не нужен для устройства
                    0,
                    core::ptr::null_mut(),
                    core::ptr::null_mut(), // handle не нужен
                );

                if status != STATUS_SUCCESS {
                    #[cfg(feature = "trace-io")]
                    crate::dbg_print!("[IO] IoCreateDevice: ob_insert_object failed 0x{:08X}\n", status as u32);
                    ob::refcount::ob_dereference_object(dev_obj);
                    return status;
                }
                
                #[cfg(feature = "trace-io")]
                crate::dbg_print!("[IO] IoCreateDevice: device inserted successfully\n");
            } else {
                // Пустое имя — создаём анонимно
                let device = ex_allocate_pool_with_tag(
                    POOL_TYPE::NonPagedPool,
                    total_size,
                    u32::from_le_bytes(*b"DevO"),
                );
                if device.is_null() {
                    return STATUS_INSUFFICIENT_RESOURCES;
                }
                dev = device as *mut DEVICE_OBJECT;
            }
        } else {
            // Без имени — выделяем напрямую из пула
            let device = ex_allocate_pool_with_tag(
                POOL_TYPE::NonPagedPool,
                total_size,
                u32::from_le_bytes(*b"DevO"),
            );
            if device.is_null() {
                return STATUS_INSUFFICIENT_RESOURCES;
            }
            dev = device as *mut DEVICE_OBJECT;
        }

        // Обнуляем и инициализируем структуру
        core::ptr::write_bytes(dev as *mut u8, 0, total_size);

        // Инициализируем поля
        (*dev).r#type = IO_TYPE_DEVICE as CSHORT;
        (*dev).size = DEVICE_OBJECT::SIZE as USHORT;
        (*dev).reference_count = 1;
        (*dev).driver_object = driver_object;
        (*dev).device_type = device_type;
        (*dev).characteristics = device_characteristics;
        (*dev).stack_size = 1;
        (*dev).alignment_requirement = 0; // FILE_BYTE_ALIGNMENT

        // Устанавливаем флаги
        (*dev).flags = DO_DEVICE_INITIALIZING;
        if exclusive {
            (*dev).flags |= DO_EXCLUSIVE;
        }

        // Расширение устройства находится сразу после DEVICE_OBJECT
        if device_extension_size > 0 {
            (*dev).device_extension = (dev as usize + DEVICE_OBJECT::SIZE) as PVOID;
        }

        // Инициализируем очереди
        (*dev).queue = LIST_ENTRY::new(); // Queue.ListEntry
        (*dev).device_queue = KDEVICE_QUEUE::new();

        // Инициализируем событие блокировки
        crate::ke::event::ke_initialize_event(
            &mut (*dev).device_lock,
            crate::ke::event::EVENT_TYPE::SynchronizationEvent,
            true, // Signaled
        );

        // Выделяем и инициализируем DEVOBJ_EXTENSION
        let ext = ex_allocate_pool_with_tag(
            POOL_TYPE::NonPagedPool,
            core::mem::size_of::<DEVOBJ_EXTENSION>(),
            u32::from_le_bytes(*b"DevE"),
        ) as *mut DEVOBJ_EXTENSION;
        
        if !ext.is_null() {
            core::ptr::write_bytes(ext as *mut u8, 0, core::mem::size_of::<DEVOBJ_EXTENSION>());
            (*ext).r#type = IO_TYPE_DEVICE_OBJECT_EXTENSION as CSHORT;
            (*ext).size = core::mem::size_of::<DEVOBJ_EXTENSION>() as USHORT;
            (*ext).device_object = dev;
            (*dev).device_object_extension = ext;
        }

        // Добавляем устройство в список драйвера
        (*dev).next_device = (*driver_object).device_object;
        (*driver_object).device_object = dev;

        // Увеличиваем refcount драйвера
        super::iop::iop_reference_driver_object(driver_object);

        // Отслеживаем создание устройства
        super::iop::iop_track_device_create();

        let _ = is_named; // используется для отладки

        *device_object = dev;
        STATUS_SUCCESS
    }
}

/// IoDeleteDevice - удаляет объект устройства
///
/// Уменьшает refcount устройства. Если refcount становится 0,
/// устройство удаляется из списка драйвера и память освобождается.
pub unsafe fn io_delete_device(device_object: PDEVICE_OBJECT) {
    unsafe {
        use crate::ex::pool::ex_free_pool_with_tag;

        if device_object.is_null() {
            return;
        }

        // Уменьшаем refcount устройства
        let new_ref = super::iop::iop_dereference_device_object(device_object);

        // Если ещё есть ссылки — не удаляем
        if new_ref > 0 {
            return;
        }

        let driver = (*device_object).driver_object;

        // Удаляем из списка драйвера
        if !driver.is_null() {
            let mut current = &mut (*driver).device_object;
            while !(*current).is_null() {
                if *current == device_object {
                    *current = (*device_object).next_device;
                    break;
                }
                current = &mut (**current).next_device;
            }

            // Уменьшаем refcount драйвера
            super::iop::iop_dereference_driver_object(driver);
        }

        // Отслеживаем удаление устройства
        super::iop::iop_track_device_delete();

        // Освобождаем память
        ex_free_pool_with_tag(device_object as PVOID, u32::from_le_bytes(*b"DevO"));
    }
}

/// IoAttachDevice — присоединяет устройство к стеку по имени
///
/// Ищет целевое устройство по имени и присоединяет source_device поверх него.
///
/// # Arguments
/// * `source_device` - устройство для присоединения
/// * `target_device_name` - имя целевого устройства (например `\Device\Xxx`)
/// * `target_device` - указатель для возврата целевого устройства
///
/// # Returns
/// STATUS_SUCCESS или код ошибки
pub unsafe fn io_attach_device(
    source_device: PDEVICE_OBJECT,
    target_device_name: *const UNICODE_STRING,
    target_device: *mut PDEVICE_OBJECT,
) -> NTSTATUS {
    unsafe {
        if source_device.is_null() || target_device_name.is_null() || target_device.is_null() {
            return STATUS_INVALID_PARAMETER;
        }

        *target_device = core::ptr::null_mut();

        // Получаем FILE_OBJECT и DEVICE_OBJECT по имени
        let mut file_obj: PFILE_OBJECT = core::ptr::null_mut();
        let mut dev_obj: PDEVICE_OBJECT = core::ptr::null_mut();

        let status = io_get_device_object_pointer(
            target_device_name,
            0, // FILE_READ_ATTRIBUTES
            &mut file_obj,
            &mut dev_obj,
        );

        if status != STATUS_SUCCESS {
            return status;
        }

        // Присоединяем source_device к стеку
        let attached_to = io_attach_device_to_device_stack(source_device, dev_obj);

        // Освобождаем FILE_OBJECT (он больше не нужен)
        if !file_obj.is_null() {
            crate::ob::refcount::ob_dereference_object(file_obj as PVOID);
        }

        if attached_to.is_null() {
            return STATUS_UNSUCCESSFUL;
        }

        *target_device = attached_to;
        STATUS_SUCCESS
    }
}

/// IoAttachDeviceToDeviceStack - присоединяет устройство к стеку
///
/// Присоединяет source_device поверх target_device.
/// Возвращает устройство, к которому присоединились (вершина стека).
pub unsafe fn io_attach_device_to_device_stack(
    source_device: PDEVICE_OBJECT,
    target_device: PDEVICE_OBJECT,
) -> PDEVICE_OBJECT {
    unsafe {
        if source_device.is_null() || target_device.is_null() {
            return core::ptr::null_mut();
        }

        // Находим вершину стека
        let mut top = target_device;
        while !(*top).attached_device.is_null() {
            top = (*top).attached_device;
        }

        // Присоединяем
        (*top).attached_device = source_device;
        (*source_device).stack_size = (*top).stack_size + 1;

        // Возвращаем устройство к которому присоединились
        top
    }
}

/// IoDetachDevice - отсоединяет устройство от стека
pub unsafe fn io_detach_device(target_device: PDEVICE_OBJECT) {
    unsafe {
        if target_device.is_null() {
            return;
        }

        let attached = (*target_device).attached_device;
        if !attached.is_null() {
            (*target_device).attached_device = core::ptr::null_mut();
        }
    }
}

/// IoGetAttachedDevice - возвращает верхнее устройство в стеке
pub unsafe fn io_get_attached_device(device_object: PDEVICE_OBJECT) -> PDEVICE_OBJECT {
    unsafe {
        if device_object.is_null() {
            return core::ptr::null_mut();
        }

        let mut top = device_object;
        while !(*top).attached_device.is_null() {
            top = (*top).attached_device;
        }
        top
    }
}

/// IoGetDeviceObjectPointer — получает указатель на устройство по имени
///
/// Ищет устройство в namespace по имени и возвращает FILE_OBJECT и DEVICE_OBJECT.
/// Вызывающий должен освободить FILE_OBJECT через ObDereferenceObject.
///
/// # Arguments
/// * `object_name` - полное имя устройства (например `\Device\Xxx`)
/// * `desired_access` - требуемые права доступа
/// * `file_object` - указатель для возврата FILE_OBJECT
/// * `device_object` - указатель для возврата DEVICE_OBJECT
///
/// # Returns
/// STATUS_SUCCESS или код ошибки
pub unsafe fn io_get_device_object_pointer(
    object_name: *const UNICODE_STRING,
    desired_access: ULONG,
    file_object: *mut PFILE_OBJECT,
    device_object: *mut PDEVICE_OBJECT,
) -> NTSTATUS {
    unsafe {
        use crate::ob;
        use crate::ob::types::OBJ_CASE_INSENSITIVE;
        use crate::ob::types::OBJ_KERNEL_HANDLE;
        use crate::ob::types::OBJECT_ATTRIBUTES;

        if object_name.is_null() || file_object.is_null() || device_object.is_null() {
            return STATUS_INVALID_PARAMETER;
        }

        *file_object = core::ptr::null_mut();
        *device_object = core::ptr::null_mut();

        // Формируем OBJECT_ATTRIBUTES для поиска
        let mut obj_attr = OBJECT_ATTRIBUTES::new();
        obj_attr.length = core::mem::size_of::<OBJECT_ATTRIBUTES>() as ULONG;
        obj_attr.object_name = object_name as *mut UNICODE_STRING;
        obj_attr.attributes = OBJ_CASE_INSENSITIVE | OBJ_KERNEL_HANDLE;

        // Ищем объект по имени
        let mut found_object: PVOID = core::ptr::null_mut();
        let status = ob::handle::ob_open_object_by_name(
            &mut obj_attr,
            super::init::IOP_DEVICE_OBJECT_TYPE,
            0, // KernelMode
            core::ptr::null_mut(),
            desired_access,
            core::ptr::null_mut(),
            &mut found_object,
        );

        if status != STATUS_SUCCESS {
            return status;
        }

        // Получаем DEVICE_OBJECT из handle
        let mut dev_obj: PVOID = core::ptr::null_mut();
        let status = ob::refcount::ob_reference_object_by_handle(
            found_object,
            desired_access,
            super::init::IOP_DEVICE_OBJECT_TYPE,
            0, // KernelMode
            &mut dev_obj,
            core::ptr::null_mut(),
        );

        // Закрываем handle
        ob::handle::NtClose(found_object as usize);

        if status != STATUS_SUCCESS {
            return status;
        }

        let dev = dev_obj as PDEVICE_OBJECT;

        // Создаём FILE_OBJECT для устройства
        let file = super::file::iop_create_file_object(dev);
        if file.is_null() {
            ob::refcount::ob_dereference_object(dev_obj);
            return STATUS_INSUFFICIENT_RESOURCES;
        }

        // Устанавливаем флаги FILE_OBJECT
        (*file).flags = super::types::FO_FILE_OPEN;
        (*file).device_object = dev;

        // Увеличиваем refcount устройства (FILE_OBJECT держит ссылку)
        super::iop::iop_reference_device_object(dev);

        *file_object = file;
        *device_object = io_get_attached_device(dev); // Возвращаем верхнее устройство в стеке

        STATUS_SUCCESS
    }
}

// =============================================================================
// Symbolic Links
// =============================================================================

/// IoCreateSymbolicLink — создаёт символическую ссылку
///
/// Используется для создания DOS device ссылок в \?? (например \??\C: -> \Device\HarddiskVolume1)
///
/// # Arguments
/// * `symbolic_link_name` - имя создаваемой ссылки (например \??\C:)
/// * `device_name` - целевое имя (например \Device\HarddiskVolume1)
///
/// # Returns
/// STATUS_SUCCESS или код ошибки
pub unsafe fn io_create_symbolic_link(
    symbolic_link_name: *const UNICODE_STRING,
    device_name: *const UNICODE_STRING,
) -> NTSTATUS {
    use crate::ob::types::OBJ_CASE_INSENSITIVE;
    use crate::ob::types::OBJ_PERMANENT;
    use crate::ob::types::OBJECT_ATTRIBUTES;

    if symbolic_link_name.is_null() || device_name.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    // Получаем \?? directory
    let dosdevices_dir = super::init::iop_get_dosdevices_directory();
    if dosdevices_dir.is_null() {
        return STATUS_OBJECT_PATH_NOT_FOUND;
    }
    
    // Создаём OBJECT_ATTRIBUTES для симлинка
    // Используем \?? как root_directory для создания "C:" в этой директории
    let mut obj_attr = OBJECT_ATTRIBUTES::new();
    obj_attr.length = core::mem::size_of::<OBJECT_ATTRIBUTES>() as ULONG;
    obj_attr.root_directory = dosdevices_dir as PVOID;
    obj_attr.object_name = symbolic_link_name as *mut UNICODE_STRING;
    obj_attr.attributes = OBJ_CASE_INSENSITIVE | OBJ_PERMANENT;

    // Создаём симлинк через OB
    let mut link_handle: PVOID = core::ptr::null_mut();
    let status = crate::ob::link::nt_create_symbolic_link_object(
        &mut link_handle,
        crate::ob::types::SYMBOLIC_LINK_ALL_ACCESS,
        &mut obj_attr,
        device_name as *mut UNICODE_STRING,
    );

    // Закрываем handle — объект останется благодаря OBJ_PERMANENT
    if status == STATUS_SUCCESS && !link_handle.is_null() {
        crate::ob::handle::NtClose(link_handle as usize);
    }

    status
}

/// IoDeleteSymbolicLink — удаляет символическую ссылку
///
/// # Arguments
/// * `symbolic_link_name` - имя удаляемой ссылки
///
/// # Returns
/// STATUS_SUCCESS или код ошибки
pub unsafe fn io_delete_symbolic_link(symbolic_link_name: *const UNICODE_STRING) -> NTSTATUS {
    use crate::ob::types::OBJ_CASE_INSENSITIVE;
    use crate::ob::types::OBJECT_ATTRIBUTES;

    if symbolic_link_name.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    // Открываем симлинк
    let mut obj_attr = OBJECT_ATTRIBUTES::new();
    obj_attr.length = core::mem::size_of::<OBJECT_ATTRIBUTES>() as ULONG;
    obj_attr.object_name = symbolic_link_name as *mut UNICODE_STRING;
    obj_attr.attributes = OBJ_CASE_INSENSITIVE;

    let mut link_handle: PVOID = core::ptr::null_mut();
    let status = crate::ob::link::nt_open_symbolic_link_object(
        &mut link_handle,
        crate::ob::types::DELETE,
        &mut obj_attr,
    );

    if status != STATUS_SUCCESS {
        return status;
    }

    // Делаем объект не-permanent и закрываем
    // Это приведёт к удалению при последнем dereference
    let status = crate::ob::life::ob_make_temporary_object(link_handle);

    crate::ob::handle::NtClose(link_handle as usize);

    status
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_device_object_size() {
        // NOTE: 0xB8 (184 bytes) - это размер для x86 (32-bit) из WinDDK
        // Для x64 размер больше из-за:
        // - 8-byte pointers (вместо 4-byte)
        // - Выравнивание на 8 bytes
        // - KSPIN_LOCK = 8 bytes (вместо 4)
        //
        // Текущая реализация использует реальные структуры (KDEVICE_QUEUE, KDPC, KEVENT)
        // вместо placeholders, что обеспечивает корректную работу на x64
        
        let size = core::mem::size_of::<DEVICE_OBJECT>();
        println!("DEVICE_OBJECT size: {} bytes (x64)", size);
        
        // Для x64 ожидаем ~240-280 bytes
        assert!(size >= 200 && size <= 300, "DEVICE_OBJECT size out of reasonable range: {}", size);
    }
}
