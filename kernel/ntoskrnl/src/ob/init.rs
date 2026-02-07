//! Object Manager Initialization
//!
//! Инициализация Object Manager.
//!
//! Источники:
//! - ReactOS: ob/obinit.c

use core::cell::UnsafeCell;
use core::sync::atomic::AtomicPtr;
use core::sync::atomic::AtomicU32;
use core::sync::atomic::Ordering;

use super::dir::OBJECT_DIRECTORY;
use super::header::object_to_object_header;
use super::types::*;
use crate::ex::WORK_QUEUE_ITEM;
use crate::ke::KEVENT;
use crate::nt::NTSTATUS;
use crate::nt::PVOID;
use crate::nt::STATUS_SUCCESS;
use crate::nt::ULONG;
use crate::nt::UNICODE_STRING;

// =============================================================================
// Global Variables
// =============================================================================

/// Wrapper для глобальных данных
#[repr(transparent)]
pub struct SyncUnsafeCell<T>(UnsafeCell<T>);
unsafe impl<T> Sync for SyncUnsafeCell<T> {}

impl<T> SyncUnsafeCell<T> {
    pub const fn new(value: T) -> Self {
        Self(UnsafeCell::new(value))
    }

    #[inline]
    pub fn get(&self) -> *mut T {
        self.0.get()
    }
}

/// Фаза инициализации Object Manager
pub static OBP_INITIALIZATION_PHASE: AtomicU32 = AtomicU32::new(0);

/// Тип объекта "Type"
pub static OBP_TYPE_OBJECT_TYPE: SyncUnsafeCell<*mut OBJECT_TYPE> =
    SyncUnsafeCell::new(core::ptr::null_mut());

/// Тип объекта "Directory"
pub static OBP_DIRECTORY_OBJECT_TYPE: SyncUnsafeCell<*mut OBJECT_TYPE> =
    SyncUnsafeCell::new(core::ptr::null_mut());

/// Тип объекта "SymbolicLink"
pub static OBP_SYMBOLIC_LINK_OBJECT_TYPE: SyncUnsafeCell<*mut OBJECT_TYPE> =
    SyncUnsafeCell::new(core::ptr::null_mut());

/// Корневая директория "\"
pub static OBP_ROOT_DIRECTORY_OBJECT: SyncUnsafeCell<*mut OBJECT_DIRECTORY> =
    SyncUnsafeCell::new(core::ptr::null_mut());

/// Директория типов "\ObjectTypes"
pub static OBP_TYPE_DIRECTORY_OBJECT: SyncUnsafeCell<*mut OBJECT_DIRECTORY> =
    SyncUnsafeCell::new(core::ptr::null_mut());

/// Kernel handle table
pub static OBP_KERNEL_HANDLE_TABLE: AtomicPtr<crate::ex::HANDLE_TABLE> =
    AtomicPtr::new(core::ptr::null_mut());

/// Default object для waitable типов
pub static OBP_DEFAULT_OBJECT: SyncUnsafeCell<KEVENT> = SyncUnsafeCell::new(KEVENT::new());

/// Список объектов для отложенного удаления
pub static OBP_REAPER_LIST: AtomicPtr<super::header::OBJECT_HEADER> =
    AtomicPtr::new(core::ptr::null_mut());

/// Work item для reaper
pub static OBP_REAPER_WORK_ITEM: SyncUnsafeCell<WORK_QUEUE_ITEM> =
    SyncUnsafeCell::new(WORK_QUEUE_ITEM::new());

/// Массив всех типов объектов
pub static OBP_OBJECT_TYPES: SyncUnsafeCell<[*mut OBJECT_TYPE; OB_MAX_OBJECT_TYPES]> =
    SyncUnsafeCell::new([core::ptr::null_mut(); OB_MAX_OBJECT_TYPES]);

// =============================================================================
// Generic Mappings
// =============================================================================

/// Generic mapping для типа объекта
pub static OBP_TYPE_MAPPING: GENERIC_MAPPING = GENERIC_MAPPING {
    generic_read: STANDARD_RIGHTS_READ,
    generic_write: STANDARD_RIGHTS_WRITE,
    generic_execute: STANDARD_RIGHTS_EXECUTE,
    generic_all: OBJECT_TYPE_ALL_ACCESS,
};

/// Generic mapping для директории
pub static OBP_DIRECTORY_MAPPING: GENERIC_MAPPING = GENERIC_MAPPING {
    generic_read: STANDARD_RIGHTS_READ | DIRECTORY_QUERY | DIRECTORY_TRAVERSE,
    generic_write: STANDARD_RIGHTS_WRITE | DIRECTORY_CREATE_SUBDIRECTORY | DIRECTORY_CREATE_OBJECT,
    generic_execute: STANDARD_RIGHTS_EXECUTE | DIRECTORY_QUERY | DIRECTORY_TRAVERSE,
    generic_all: DIRECTORY_ALL_ACCESS,
};

/// Generic mapping для символической ссылки
pub static OBP_SYMBOLIC_LINK_MAPPING: GENERIC_MAPPING = GENERIC_MAPPING {
    generic_read: STANDARD_RIGHTS_READ | SYMBOLIC_LINK_QUERY,
    generic_write: STANDARD_RIGHTS_WRITE,
    generic_execute: STANDARD_RIGHTS_EXECUTE | SYMBOLIC_LINK_QUERY,
    generic_all: SYMBOLIC_LINK_ALL_ACCESS,
};

// =============================================================================
// Initialization Functions
// =============================================================================

/// Инициализирует Object Manager
///
/// Вызывается дважды:
/// - Phase 0: Создание базовых типов (Type, Directory, SymbolicLink)
/// - Phase 1: Создание namespace (/, /ObjectTypes, /KernelObjects)
pub fn ob_init_system() -> bool {
    let phase = OBP_INITIALIZATION_PHASE.load(Ordering::Acquire);

    if phase == 0 {
        ob_init_phase0()
    } else {
        ob_init_phase1()
    }
}

/// Phase 0 инициализации
fn ob_init_phase0() -> bool {
    // Инициализируем default event
    unsafe {
        let event = &mut *OBP_DEFAULT_OBJECT.get();
        crate::ke::ke_initialize_event(event, crate::ke::EVENT_TYPE::NotificationEvent, true);
    }

    // Инициализируем reaper work item
    unsafe {
        let work_item = &mut *OBP_REAPER_WORK_ITEM.get();
        // Используем обертку для unsafe callback
        crate::ex::ex_initialize_work_item(
            work_item,
            obp_reap_object_wrapper,
            core::ptr::null_mut(),
        );
    }

    // Создаем тип "Type"
    let type_type = match create_type_object_type() {
        Ok(t) => t,
        Err(_) => return false,
    };
    unsafe {
        *OBP_TYPE_OBJECT_TYPE.get() = type_type;
    }

    // Создаем тип "Directory"
    let dir_type = match create_directory_object_type() {
        Ok(t) => t,
        Err(_) => return false,
    };
    unsafe {
        *OBP_DIRECTORY_OBJECT_TYPE.get() = dir_type;
    }

    // Создаем тип "SymbolicLink"
    let link_type = match create_symbolic_link_object_type() {
        Ok(t) => t,
        Err(_) => return false,
    };
    unsafe {
        *OBP_SYMBOLIC_LINK_OBJECT_TYPE.get() = link_type;
    }

    // Создаём kernel handle table (используется для OBJ_KERNEL_HANDLE)
    if OBP_KERNEL_HANDLE_TABLE.load(Ordering::Acquire).is_null() {
        if let Some(tbl) = crate::ex::handle::ex_create_handle_table(core::ptr::null_mut()) {
            OBP_KERNEL_HANDLE_TABLE.store(tbl, Ordering::Release);
        } else {
            return false;
        }
    }

    // Phase 0 завершена
    OBP_INITIALIZATION_PHASE.store(1, Ordering::Release);
    true
}

/// Phase 1 инициализации
fn ob_init_phase1() -> bool {
    // Создаем корневую директорию "\"
    let root_dir = match create_root_directory() {
        Ok(d) => d,
        Err(_) => return false,
    };
    if root_dir.is_null() {
        return false;
    }
    unsafe {
        *OBP_ROOT_DIRECTORY_OBJECT.get() = root_dir;
    }

    // Создаем директорию "\ObjectTypes"
    let types_dir = match create_object_types_directory() {
        Ok(d) => d,
        Err(_) => return false,
    };
    if types_dir.is_null() {
        return false;
    }
    unsafe {
        *OBP_TYPE_DIRECTORY_OBJECT.get() = types_dir;
    }

    // Создаем директорию "\KernelObjects"
    if create_kernel_objects_directory().is_err() {
        return false;
    }

    // Создаем базовые каталоги namespace (минимум для NT‑подобного окружения)
    if create_base_namespace_directories().is_err() {
        return false;
    }

    // Заполняем \ObjectTypes всеми известными типами
    if populate_object_types_directory(types_dir).is_err() {
        return false;
    }

    // Phase 1 завершена
    OBP_INITIALIZATION_PHASE.store(2, Ordering::Release);
    true
}

/// Создает тип объекта "Type"
fn create_type_object_type() -> Result<*mut OBJECT_TYPE, NTSTATUS> {
    let mut init = OBJECT_TYPE_INITIALIZER::new();
    init.use_default_object = true;
    init.maintain_type_list = true;
    init.pool_type = NON_PAGED_POOL;
    init.valid_access_mask = OBJECT_TYPE_ALL_ACCESS;
    init.generic_mapping = OBP_TYPE_MAPPING;
    init.default_non_paged_pool_charge = core::mem::size_of::<OBJECT_TYPE>() as ULONG;
    init.invalid_attributes = OBJ_OPENLINK;

    // Для первого типа мы не можем использовать ob_create_object_type
    // так как OBP_TYPE_OBJECT_TYPE еще не установлен
    // Выделяем память напрямую
    let type_size = core::mem::size_of::<OBJECT_TYPE>();
    let header_size = core::mem::size_of::<super::header::OBJECT_HEADER>();
    let name_info_size = core::mem::size_of::<super::header::OBJECT_HEADER_NAME_INFO>();
    let creator_info_size = core::mem::size_of::<super::header::OBJECT_HEADER_CREATOR_INFO>();

    let total_size = name_info_size + creator_info_size + header_size + type_size;

    let ptr = crate::ex::pool::ex_allocate_pool_with_tag(
        crate::ex::pool::POOL_TYPE::NonPagedPool,
        total_size,
        u32::from_le_bytes(*b"TybO"), // 'ObTy'
    );

    if ptr.is_null() {
        return Err(crate::nt::STATUS_INSUFFICIENT_RESOURCES);
    }

    unsafe {
        // Обнуляем всю память
        core::ptr::write_bytes(ptr, 0, total_size);

        // Настраиваем структуры
        let name_info = ptr as *mut super::header::OBJECT_HEADER_NAME_INFO;
        let creator_info =
            (ptr as *mut u8).add(name_info_size) as *mut super::header::OBJECT_HEADER_CREATOR_INFO;
        let header = (ptr as *mut u8).add(name_info_size + creator_info_size)
            as *mut super::header::OBJECT_HEADER;
        let object_type = (ptr as *mut u8).add(name_info_size + creator_info_size + header_size)
            as *mut OBJECT_TYPE;

        // Инициализируем header
        (*header).pointer_count = core::sync::atomic::AtomicIsize::new(1);
        (*header).handle_count_or_next_to_free.handle_count = 0;
        (*header).flags = super::types::OB_FLAG_KERNEL_MODE
            | super::types::OB_FLAG_PERMANENT
            | super::types::OB_FLAG_CREATOR_INFO;
        (*header).name_info_offset = (name_info_size + creator_info_size) as u8;
        (*header).type_ptr = object_type; // Тип указывает сам на себя

        // Инициализируем name info
        (*name_info).query_references = 1;
        // Имя "Type" - статическая строка
        static TYPE_NAME: [u16; 5] = [b'T' as u16, b'y' as u16, b'p' as u16, b'e' as u16, 0];
        (*name_info).name.buffer = TYPE_NAME.as_ptr() as *mut u16;
        (*name_info).name.length = 8; // 4 chars * 2 bytes
        (*name_info).name.maximum_length = 10;

        // Инициализируем creator info
        (*creator_info).type_list = crate::nt::LIST_ENTRY::new();
        crate::nt::LIST_ENTRY::init_head(&mut (*creator_info).type_list);

        // Инициализируем object_type
        (*object_type).type_list = crate::nt::LIST_ENTRY::new();
        crate::nt::LIST_ENTRY::init_head(&mut (*object_type).type_list);
        (*object_type).name = (*name_info).name.clone();
        (*object_type).default_object = OBP_DEFAULT_OBJECT.get() as *mut _ as PVOID;
        (*object_type).index = 1;
        (*object_type).total_number_of_objects = 1;
        (*object_type).type_info = init;
        (*object_type).key = u32::from_le_bytes(*b"TybO");

        // Добавляем в массив типов
        let types = &mut *OBP_OBJECT_TYPES.get();
        types[0] = object_type;

        Ok(object_type)
    }
}

/// Создает тип объекта "Directory"
fn create_directory_object_type() -> Result<*mut OBJECT_TYPE, NTSTATUS> {
    let mut init = OBJECT_TYPE_INITIALIZER::new();
    init.case_insensitive = true;
    init.pool_type = PAGED_POOL;
    init.valid_access_mask = DIRECTORY_ALL_ACCESS & !SYNCHRONIZE;
    init.generic_mapping = OBP_DIRECTORY_MAPPING;
    init.default_paged_pool_charge = core::mem::size_of::<OBJECT_DIRECTORY>() as ULONG;

    static DIR_NAME: [u16; 10] = [
        b'D' as u16,
        b'i' as u16,
        b'r' as u16,
        b'e' as u16,
        b'c' as u16,
        b't' as u16,
        b'o' as u16,
        b'r' as u16,
        b'y' as u16,
        0,
    ];

    let mut name = UNICODE_STRING {
        length: 18,
        maximum_length: 20,
        buffer: DIR_NAME.as_ptr() as *mut u16,
    };

    let mut object_type: *mut OBJECT_TYPE = core::ptr::null_mut();
    let status = super::life::ob_create_object_type(
        &mut name,
        &init,
        core::ptr::null_mut(),
        &mut object_type,
    );

    if status == STATUS_SUCCESS {
        Ok(object_type)
    } else {
        Err(status)
    }
}

/// Создает тип объекта "SymbolicLink"
fn create_symbolic_link_object_type() -> Result<*mut OBJECT_TYPE, NTSTATUS> {
    let mut init = OBJECT_TYPE_INITIALIZER::new();
    init.pool_type = PAGED_POOL;
    init.valid_access_mask = SYMBOLIC_LINK_ALL_ACCESS & !SYNCHRONIZE;
    init.generic_mapping = OBP_SYMBOLIC_LINK_MAPPING;
    init.default_paged_pool_charge =
        core::mem::size_of::<super::link::OBJECT_SYMBOLIC_LINK>() as ULONG;
    init.parse_procedure = Some(obp_parse_symbolic_link);
    init.delete_procedure = Some(obp_delete_symbolic_link);

    static LINK_NAME: [u16; 13] = [
        b'S' as u16,
        b'y' as u16,
        b'm' as u16,
        b'b' as u16,
        b'o' as u16,
        b'l' as u16,
        b'i' as u16,
        b'c' as u16,
        b'L' as u16,
        b'i' as u16,
        b'n' as u16,
        b'k' as u16,
        0,
    ];

    let mut name = UNICODE_STRING {
        length: 24,
        maximum_length: 26,
        buffer: LINK_NAME.as_ptr() as *mut u16,
    };

    let mut object_type: *mut OBJECT_TYPE = core::ptr::null_mut();
    let status = super::life::ob_create_object_type(
        &mut name,
        &init,
        core::ptr::null_mut(),
        &mut object_type,
    );

    if status == STATUS_SUCCESS {
        Ok(object_type)
    } else {
        Err(status)
    }
}

/// Создает корневую директорию
fn create_root_directory() -> Result<*mut OBJECT_DIRECTORY, NTSTATUS> {
    // Создаём объект директории без имени (корень namespace).
    let directory_type = obp_get_directory_object_type();
    if directory_type.is_null() {
        return Err(crate::nt::STATUS_INSUFFICIENT_RESOURCES);
    }

    // Атрибуты: permanent + case-insensitive (как в NT namespace).
    let mut attrs = OBJECT_ATTRIBUTES::new();
    attrs.attributes = OBJ_PERMANENT | OBJ_CASE_INSENSITIVE;

    let mut dir_obj: PVOID = core::ptr::null_mut();
    let status = super::life::ob_create_object(
        0, // KernelMode
        directory_type,
        &attrs as *const _,
        0, // KernelMode
        core::ptr::null_mut(),
        core::mem::size_of::<OBJECT_DIRECTORY>(),
        0,
        0,
        &mut dir_obj,
    );

    if status != STATUS_SUCCESS || dir_obj.is_null() {
        return Err(status);
    }

    unsafe {
        // Инициализируем тело директории
        *(dir_obj as *mut OBJECT_DIRECTORY) = OBJECT_DIRECTORY::new();
    }

    Ok(dir_obj as *mut OBJECT_DIRECTORY)
}

/// Создает директорию /ObjectTypes
fn create_object_types_directory() -> Result<*mut OBJECT_DIRECTORY, NTSTATUS> {
    let root = obp_get_root_directory();
    if root.is_null() {
        return Err(crate::nt::STATUS_OBJECT_NAME_NOT_FOUND);
    }

    let obj = create_named_directory_in(
        root,
        &UNICODE_STRING {
            length: 22, // 11 chars * 2 bytes
            maximum_length: 24,
            buffer: OBJECT_TYPES_DIR_NAME.as_ptr() as *mut u16,
        },
    )?;

    Ok(obj)
}

/// Создает директорию /KernelObjects
fn create_kernel_objects_directory() -> Result<(), NTSTATUS> {
    let root = obp_get_root_directory();
    if root.is_null() {
        return Err(crate::nt::STATUS_OBJECT_NAME_NOT_FOUND);
    }

    let _ = create_named_directory_in(
        root,
        &UNICODE_STRING {
            length: 26, // 13 chars * 2 bytes
            maximum_length: 28,
            buffer: KERNEL_OBJECTS_DIR_NAME.as_ptr() as *mut u16,
        },
    )?;

    Ok(())
}

// =============================================================================
// Namespace helpers (Phase 1)
// =============================================================================

/// Имя директории "ObjectTypes"
static OBJECT_TYPES_DIR_NAME: [u16; 12] = [
    b'O' as u16,
    b'b' as u16,
    b'j' as u16,
    b'e' as u16,
    b'c' as u16,
    b't' as u16,
    b'T' as u16,
    b'y' as u16,
    b'p' as u16,
    b'e' as u16,
    b's' as u16,
    0,
];

/// Имя директории "KernelObjects"
static KERNEL_OBJECTS_DIR_NAME: [u16; 14] = [
    b'K' as u16,
    b'e' as u16,
    b'r' as u16,
    b'n' as u16,
    b'e' as u16,
    b'l' as u16,
    b'O' as u16,
    b'b' as u16,
    b'j' as u16,
    b'e' as u16,
    b'c' as u16,
    b't' as u16,
    b's' as u16,
    0,
];

// Константы имён директорий для DOS devices (OB namespace)
static DOS_DEVICES_DIR_NAME: [u16; 3] = [b'?' as u16, b'?' as u16, 0];
static GLOBAL_DOS_DEVICES_DIR_NAME: [u16; 9] = [
    b'G' as u16,
    b'L' as u16,
    b'O' as u16,
    b'B' as u16,
    b'A' as u16,
    b'L' as u16,
    b'?' as u16,
    b'?' as u16,
    0,
];

/// ObpCreateNamedDirectory — создаёт именованную директорию в указанной родительской директории
///
/// Используется I/O Manager для создания `\Device`, `\Driver`, `\FileSystem`.
///
/// # Arguments
/// * `parent` - родительская директория (или NULL для root)
/// * `name` - имя создаваемой директории
///
/// # Returns
/// Указатель на созданную директорию или код ошибки
pub fn obp_create_named_directory(
    parent: *mut OBJECT_DIRECTORY,
    name: &UNICODE_STRING,
) -> Result<*mut OBJECT_DIRECTORY, NTSTATUS> {
    // Если parent == NULL, используем root
    let actual_parent = if parent.is_null() {
        obp_get_root_directory()
    } else {
        parent
    };
    create_named_directory_in(actual_parent, name)
}

/// Внутренняя функция создания директории
fn create_named_directory_in(
    parent: *mut OBJECT_DIRECTORY,
    name: &UNICODE_STRING,
) -> Result<*mut OBJECT_DIRECTORY, NTSTATUS> {
    if parent.is_null() || name.buffer.is_null() || name.length == 0 {
        return Err(crate::nt::STATUS_INVALID_PARAMETER);
    }

    let directory_type = obp_get_directory_object_type();
    if directory_type.is_null() {
        return Err(crate::nt::STATUS_INSUFFICIENT_RESOURCES);
    }

    // Создаём объект с именем (NAME_INFO будет выделен и имя будет скопировано).
    let mut name_copy = *name;
    let mut attrs = OBJECT_ATTRIBUTES::new();
    attrs.object_name = &mut name_copy as *mut _;
    attrs.root_directory = parent as PVOID;
    attrs.attributes = OBJ_PERMANENT | OBJ_CASE_INSENSITIVE;

    let mut dir_obj: PVOID = core::ptr::null_mut();
    let status = super::life::ob_create_object(
        0, // KernelMode
        directory_type,
        &attrs as *const _,
        0, // KernelMode
        core::ptr::null_mut(),
        core::mem::size_of::<OBJECT_DIRECTORY>(),
        0,
        0,
        &mut dir_obj,
    );

    if status != STATUS_SUCCESS || dir_obj.is_null() {
        return Err(status);
    }

    unsafe {
        *(dir_obj as *mut OBJECT_DIRECTORY) = OBJECT_DIRECTORY::new();
    }

    // Вставляем объект в родительскую директорию.
    insert_object_into_directory(parent, dir_obj)?;

    Ok(dir_obj as *mut OBJECT_DIRECTORY)
}

/// Вставляет объект (с NAME_INFO) в директорию namespace.
fn insert_object_into_directory(
    directory: *mut OBJECT_DIRECTORY,
    object: PVOID,
) -> Result<(), NTSTATUS> {
    use super::dir::OBP_LOOKUP_CONTEXT;
    use super::dir::obp_insert_entry_directory;
    use super::dir::obp_lookup_entry_directory;
    use super::dir::obp_release_lookup_context;

    if directory.is_null() || object.is_null() {
        return Err(crate::nt::STATUS_INVALID_PARAMETER);
    }

    unsafe {
        let header = object_to_object_header(object);
        let Some(name_info) = (*header).name_info() else {
            return Err(crate::nt::STATUS_INVALID_PARAMETER);
        };

        let mut ctx = OBP_LOOKUP_CONTEXT::new();
        let found = obp_lookup_entry_directory(
            directory,
            &name_info.name,
            OBJ_CASE_INSENSITIVE,
            true, // look_for_insertion
            &mut ctx,
        );

        if found {
            obp_release_lookup_context(&mut ctx);
            return Err(crate::nt::STATUS_OBJECT_NAME_COLLISION);
        }

        if !obp_insert_entry_directory(directory, &mut ctx, header) {
            obp_release_lookup_context(&mut ctx);
            return Err(crate::nt::STATUS_INSUFFICIENT_RESOURCES);
        }

        obp_release_lookup_context(&mut ctx);
        Ok(())
    }
}

/// Создаёт базовые директории OB namespace (`\??`, `\GLOBAL??`)
///
/// Примечание: директории `\Device`, `\Driver`, `\FileSystem` создаются I/O Manager'ом
/// в соответствии с архитектурой NT.
fn create_base_namespace_directories() -> Result<(), NTSTATUS> {
    let root = obp_get_root_directory();
    if root.is_null() {
        return Err(crate::nt::STATUS_OBJECT_NAME_NOT_FOUND);
    }

    // OB создаёт только DOS devices directories
    let _ = create_named_directory_in(
        root,
        &UNICODE_STRING {
            length: 4,
            maximum_length: 6,
            buffer: DOS_DEVICES_DIR_NAME.as_ptr() as *mut u16,
        },
    )?;
    let _ = create_named_directory_in(
        root,
        &UNICODE_STRING {
            length: 16,
            maximum_length: 18,
            buffer: GLOBAL_DOS_DEVICES_DIR_NAME.as_ptr() as *mut u16,
        },
    )?;

    Ok(())
}

/// Вставляет все зарегистрированные типы объектов в директорию \ObjectTypes.
fn populate_object_types_directory(types_dir: *mut OBJECT_DIRECTORY) -> Result<(), NTSTATUS> {
    if types_dir.is_null() {
        return Err(crate::nt::STATUS_INVALID_PARAMETER);
    }

    unsafe {
        let types = &*OBP_OBJECT_TYPES.get();
        for &ty in types.iter() {
            if ty.is_null() {
                continue;
            }

            // OBJECT_TYPE является телом объекта типа.
            let obj = ty as PVOID;

            match insert_object_into_directory(types_dir, obj) {
                Ok(()) => {},
                Err(crate::nt::STATUS_OBJECT_NAME_COLLISION) => {
                    // Уже вставлен — это не ошибка для Phase1.
                },
                Err(status) => return Err(status),
            }
        }
    }

    Ok(())
}

// =============================================================================
// Callback Functions
// =============================================================================

/// Safe wrapper для obp_reap_object
extern "win64" fn obp_reap_object_wrapper(parameter: PVOID) {
    unsafe { obp_reap_object(parameter) }
}

/// Work item для удаления объектов
///
/// Обрабатывает список объектов, ожидающих удаления (OBP_REAPER_LIST).
/// Для каждого объекта вызывает канонический путь удаления (см. `ob/refcount.rs`).
unsafe fn obp_reap_object(_parameter: PVOID) {
    unsafe {
        // Атомарно забираем весь список
        let mut header =
            OBP_REAPER_LIST.swap(core::ptr::null_mut(), core::sync::atomic::Ordering::AcqRel);

        while !header.is_null() {
            // Сохраняем следующий элемент перед удалением текущего
            let next = (*header).handle_count_or_next_to_free.next_to_free;

            // Канонический путь удаления: процедуры типа + освобождение вспомогательных данных + free.
            let object = (*header).body();
            super::refcount::obp_delete_object(object, true);

            // Переходим к следующему объекту
            header = next;
        }
    }
}

/// Parse procedure для symbolic links
unsafe extern "win64" fn obp_parse_symbolic_link(
    parse_object: PVOID,
    _object_type: *mut OBJECT_TYPE,
    _access_state: PVOID,
    _access_mode: u8,
    attributes: ULONG,
    complete_name: *mut UNICODE_STRING,
    remaining_name: *mut UNICODE_STRING,
    context: PVOID,
    _security_qos: PVOID,
    object: *mut PVOID,
) -> NTSTATUS {
    use super::dir::OBJECT_DIRECTORY;
    use super::link::OBJECT_SYMBOLIC_LINK;
    use crate::ex::pool::POOL_TYPE;
    use crate::ex::pool::ex_allocate_pool_with_tag;
    use crate::nt::STATUS_INVALID_PARAMETER;
    use crate::nt::STATUS_OBJECT_PATH_NOT_FOUND;
    use crate::nt::STATUS_REPARSE;

    if parse_object.is_null()
        || complete_name.is_null()
        || remaining_name.is_null()
        || object.is_null()
    {
        return STATUS_INVALID_PARAMETER;
    }

    unsafe {
        *object = core::ptr::null_mut();
    }

    // OBJ_OPENLINK: открываем сам symlink, а не цель
    if (attributes & (OBJ_OPENLINK as ULONG)) != 0 {
        unsafe {
            // Если путь продолжается — это невалидно для OBJ_OPENLINK
            if (*remaining_name).length != 0 {
                return STATUS_OBJECT_PATH_NOT_FOUND;
            }
            *object = parse_object;
        }
        return STATUS_SUCCESS;
    }

    unsafe {
        let link = parse_object as *mut OBJECT_SYMBOLIC_LINK;
        let target = (*link).link_target;
        if target.buffer.is_null() || target.length == 0 {
            return crate::nt::STATUS_OBJECT_NAME_NOT_FOUND;
        }

        // Формируем новый путь: target + remaining (remaining включает ведущий '\\' если он там)
        let target_u16_len = (target.length / 2) as usize;
        let rem_u16_len = ((*remaining_name).length / 2) as usize;

        // Нужен ли разделитель между target и remaining
        let need_sep = rem_u16_len != 0
            && (target_u16_len == 0 || *target.buffer.add(target_u16_len - 1) != b'\\' as u16)
            && *(*remaining_name).buffer != b'\\' as u16;

        let new_u16_len = target_u16_len + (if need_sep { 1 } else { 0 }) + rem_u16_len;
        let new_bytes = (new_u16_len * 2) + 2; // + NUL

        let tag = u32::from_le_bytes(*b"pLbO"); // 'ObLp'
        let new_ptr = ex_allocate_pool_with_tag(POOL_TYPE::PagedPool, new_bytes, tag) as *mut u16;
        if new_ptr.is_null() {
            return crate::nt::STATUS_INSUFFICIENT_RESOURCES;
        }

        // target
        core::ptr::copy_nonoverlapping(target.buffer, new_ptr, target_u16_len);
        let mut w = target_u16_len;
        if need_sep {
            *new_ptr.add(w) = b'\\' as u16;
            w += 1;
        }

        // remaining
        if rem_u16_len != 0 {
            core::ptr::copy_nonoverlapping((*remaining_name).buffer, new_ptr.add(w), rem_u16_len);
            w += rem_u16_len;
        }
        // NUL terminate
        *new_ptr.add(w) = 0;

        // Возвращаем новый complete/remaining как один путь (перезапуск парсинга)
        (*complete_name).buffer = new_ptr;
        (*complete_name).length = (new_u16_len * 2) as u16;
        (*complete_name).maximum_length = (new_u16_len * 2 + 2) as u16;

        (*remaining_name).buffer = new_ptr;
        (*remaining_name).length = (*complete_name).length;
        (*remaining_name).maximum_length = (*complete_name).maximum_length;

        // Возвращаем стартовую директорию: абсолютный путь -> root, относительный -> context
        if new_u16_len != 0 && *new_ptr == b'\\' as u16 {
            *object = obp_get_root_directory() as PVOID;
        } else {
            *object = context as *mut OBJECT_DIRECTORY as PVOID;
        }

        STATUS_REPARSE
    }
}

/// Delete procedure для symbolic links
///
/// Освобождает буфер target string символической ссылки.
unsafe extern "win64" fn obp_delete_symbolic_link(object: PVOID) {
    unsafe {
        use super::link::OBJECT_SYMBOLIC_LINK;
        use crate::ex::pool::ex_free_pool_with_tag;

        if object.is_null() {
            return;
        }

        let link = object as *mut OBJECT_SYMBOLIC_LINK;

        // Освобождаем буфер target string если он был выделен
        if !(*link).link_target.buffer.is_null() {
            ex_free_pool_with_tag(
                (*link).link_target.buffer as PVOID,
                u32::from_le_bytes(*b"tmyS"),
            );
            (*link).link_target.buffer = core::ptr::null_mut();
            (*link).link_target.length = 0;
            (*link).link_target.maximum_length = 0;
        }
    }
}

// =============================================================================
// Public API
// =============================================================================

/// Возвращает тип объекта Type
pub fn obp_get_type_object_type() -> *mut OBJECT_TYPE {
    unsafe { *OBP_TYPE_OBJECT_TYPE.get() }
}

/// Возвращает тип объекта Directory
pub fn obp_get_directory_object_type() -> *mut OBJECT_TYPE {
    unsafe { *OBP_DIRECTORY_OBJECT_TYPE.get() }
}

/// Возвращает тип объекта SymbolicLink
pub fn obp_get_symbolic_link_object_type() -> *mut OBJECT_TYPE {
    unsafe { *OBP_SYMBOLIC_LINK_OBJECT_TYPE.get() }
}

/// Возвращает корневую директорию
pub fn obp_get_root_directory() -> *mut OBJECT_DIRECTORY {
    unsafe { *OBP_ROOT_DIRECTORY_OBJECT.get() }
}
