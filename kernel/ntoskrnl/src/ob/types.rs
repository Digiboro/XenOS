//! Object Manager Types
//!
//! Базовые типы и константы Object Manager.

use crate::ke::ERESOURCE;
use crate::nt::LIST_ENTRY;
use crate::nt::NTSTATUS;
use crate::nt::PVOID;
use crate::nt::ULONG;
use crate::nt::UNICODE_STRING;
use crate::nt::USHORT;

/// Максимальное количество типов объектов
pub const OB_MAX_OBJECT_TYPES: usize = 32;

// =============================================================================
// Object Flags
// =============================================================================

/// Object создан с информацией о создании
pub const OB_FLAG_CREATE_INFO: u8 = 0x01;
/// Kernel mode объект
pub const OB_FLAG_KERNEL_MODE: u8 = 0x02;
/// Объект имеет информацию о создателе
pub const OB_FLAG_CREATOR_INFO: u8 = 0x04;
/// Объект эксклюзивный
pub const OB_FLAG_EXCLUSIVE: u8 = 0x08;
/// Объект постоянный
pub const OB_FLAG_PERMANENT: u8 = 0x10;
/// Объект имеет security descriptor
pub const OB_FLAG_SECURITY: u8 = 0x20;
/// Объект принадлежит одному процессу
pub const OB_FLAG_SINGLE_PROCESS: u8 = 0x40;
/// Отложенное удаление
pub const OB_FLAG_DEFER_DELETE: u8 = 0x80;

// =============================================================================
// Object Attributes Flags
// =============================================================================

/// Объект наследуется дочерними процессами
pub const OBJ_INHERIT: u32 = 0x00000002;
/// Запретить закрытие хэндла (handle-level флаг)
pub const OBJ_PROTECT_CLOSE: u32 = 0x00000001;
/// Объект постоянный
pub const OBJ_PERMANENT: u32 = 0x00000010;
/// Объект эксклюзивный
pub const OBJ_EXCLUSIVE: u32 = 0x00000020;
/// Сравнение имен без учета регистра
pub const OBJ_CASE_INSENSITIVE: u32 = 0x00000040;
/// Открыть символическую ссылку, а не целевой объект
pub const OBJ_OPENLINK: u32 = 0x00000100;
/// Использовать kernel handle
pub const OBJ_KERNEL_HANDLE: u32 = 0x00000200;
/// Открыть только если объект существует
pub const OBJ_OPENIF: u32 = 0x00000080;
/// Force access check
pub const OBJ_FORCE_ACCESS_CHECK: u32 = 0x00000400;
/// Kernel exclusive объект
pub const OBJ_KERNEL_EXCLUSIVE: u32 = 0x00010000;

/// Валидные атрибуты для kernel mode
pub const OBJ_VALID_KERNEL_ATTRIBUTES: u32 = OBJ_PROTECT_CLOSE
    | OBJ_INHERIT
    | OBJ_PERMANENT
    | OBJ_EXCLUSIVE
    | OBJ_CASE_INSENSITIVE
    | OBJ_OPENLINK
    | OBJ_KERNEL_HANDLE
    | OBJ_OPENIF
    | OBJ_FORCE_ACCESS_CHECK
    | OBJ_KERNEL_EXCLUSIVE;

/// Атрибуты хэндла (маска OBJ_* для параметра HandleAttributes)
///
/// Важно: в HANDLE_TABLE_ENTRY хранится **HANDLE_FLAG_***, но публичные API
/// (например NtDuplicateObject) исторически принимают эти биты как OBJ_*.
pub const OBJ_HANDLE_ATTRIBUTES: u32 = OBJ_INHERIT | OBJ_PROTECT_CLOSE;

// =============================================================================
// OB_OPEN_REASON
// =============================================================================

/// Причина открытия handle (OB_OPEN_REASON)
pub const OB_OPEN_REASON_CREATE: ULONG = 0;
pub const OB_OPEN_REASON_OPEN: ULONG = 1;
pub const OB_OPEN_REASON_DUPLICATE: ULONG = 2;
pub const OB_OPEN_REASON_INHERIT: ULONG = 3;

// =============================================================================
// Access Rights
// =============================================================================

/// Стандартные права доступа
pub const STANDARD_RIGHTS_READ: u32 = 0x00020000;
pub const STANDARD_RIGHTS_WRITE: u32 = 0x00020000;
pub const STANDARD_RIGHTS_EXECUTE: u32 = 0x00020000;
pub const STANDARD_RIGHTS_ALL: u32 = 0x001F0000;

pub const DELETE: u32 = 0x00010000;
pub const READ_CONTROL: u32 = 0x00020000;
pub const WRITE_DAC: u32 = 0x00040000;
pub const WRITE_OWNER: u32 = 0x00080000;
pub const SYNCHRONIZE: u32 = 0x00100000;

/// Права доступа к типу объекта
pub const OBJECT_TYPE_CREATE: u32 = 0x0001;
pub const OBJECT_TYPE_ALL_ACCESS: u32 = STANDARD_RIGHTS_ALL | OBJECT_TYPE_CREATE;

/// Права доступа к директории
pub const DIRECTORY_QUERY: u32 = 0x0001;
pub const DIRECTORY_TRAVERSE: u32 = 0x0002;
pub const DIRECTORY_CREATE_OBJECT: u32 = 0x0004;
pub const DIRECTORY_CREATE_SUBDIRECTORY: u32 = 0x0008;
pub const DIRECTORY_ALL_ACCESS: u32 = STANDARD_RIGHTS_ALL | 0x000F;

/// Права доступа к символической ссылке
pub const SYMBOLIC_LINK_QUERY: u32 = 0x0001;
pub const SYMBOLIC_LINK_ALL_ACCESS: u32 = STANDARD_RIGHTS_ALL | SYMBOLIC_LINK_QUERY;

// =============================================================================
// OBJECT_ATTRIBUTES
// =============================================================================

/// Атрибуты объекта для создания/открытия
#[repr(C)]
#[derive(Clone, Copy)]
pub struct OBJECT_ATTRIBUTES {
    pub length: ULONG,
    pub root_directory: PVOID, // HANDLE
    pub object_name: *mut UNICODE_STRING,
    pub attributes: ULONG,
    pub security_descriptor: PVOID,
    pub security_quality_of_service: PVOID,
}

impl OBJECT_ATTRIBUTES {
    pub const fn new() -> Self {
        Self {
            length: core::mem::size_of::<Self>() as ULONG,
            root_directory: core::ptr::null_mut(),
            object_name: core::ptr::null_mut(),
            attributes: 0,
            security_descriptor: core::ptr::null_mut(),
            security_quality_of_service: core::ptr::null_mut(),
        }
    }

    pub fn init(
        &mut self,
        name: *mut UNICODE_STRING,
        attributes: ULONG,
        root_directory: PVOID,
        security_descriptor: PVOID,
    ) {
        self.length = core::mem::size_of::<Self>() as ULONG;
        self.root_directory = root_directory;
        self.object_name = name;
        self.attributes = attributes;
        self.security_descriptor = security_descriptor;
        self.security_quality_of_service = core::ptr::null_mut();
    }
}

impl Default for OBJECT_ATTRIBUTES {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// GENERIC_MAPPING
// =============================================================================

/// Маппинг generic прав доступа на специфичные
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct GENERIC_MAPPING {
    pub generic_read: u32,
    pub generic_write: u32,
    pub generic_execute: u32,
    pub generic_all: u32,
}

impl GENERIC_MAPPING {
    pub const fn new() -> Self {
        Self {
            generic_read: 0,
            generic_write: 0,
            generic_execute: 0,
            generic_all: 0,
        }
    }
}

impl Default for GENERIC_MAPPING {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// OBJECT_TYPE_INITIALIZER
// =============================================================================

/// Процедура открытия объекта
pub type OB_OPEN_METHOD = Option<
    unsafe extern "win64" fn(
        open_reason: ULONG,
        process: PVOID,
        object: PVOID,
        granted_access: u32,
        handle_count: ULONG,
    ) -> NTSTATUS,
>;

/// Процедура закрытия объекта
pub type OB_CLOSE_METHOD = Option<
    unsafe extern "win64" fn(
        process: PVOID,
        object: PVOID,
        granted_access: u32,
        process_handle_count: ULONG,
        system_handle_count: ULONG,
    ),
>;

/// Процедура удаления объекта
pub type OB_DELETE_METHOD = Option<unsafe extern "win64" fn(object: PVOID)>;

/// Процедура парсинга объекта
pub type OB_PARSE_METHOD = Option<
    unsafe extern "win64" fn(
        parse_object: PVOID,
        object_type: *mut OBJECT_TYPE,
        access_state: PVOID,
        access_mode: u8,
        attributes: ULONG,
        complete_name: *mut UNICODE_STRING,
        remaining_name: *mut UNICODE_STRING,
        context: PVOID,
        security_qos: PVOID,
        object: *mut PVOID,
    ) -> NTSTATUS,
>;

/// Процедура безопасности объекта
pub type OB_SECURITY_METHOD = Option<
    unsafe extern "win64" fn(
        object: PVOID,
        operation_code: ULONG,
        security_information: *mut ULONG,
        security_descriptor: PVOID,
        length: *mut ULONG,
        object_sd: *mut PVOID,
        pool_type: ULONG,
        generic_mapping: *mut GENERIC_MAPPING,
    ) -> NTSTATUS,
>;

/// Процедура запроса имени объекта
pub type OB_QUERY_NAME_METHOD = Option<
    unsafe extern "win64" fn(
        object: PVOID,
        has_name: bool,
        object_name_info: PVOID,
        length: ULONG,
        return_length: *mut ULONG,
        access_mode: u8,
    ) -> NTSTATUS,
>;

/// Инициализатор типа объекта
#[repr(C)]
pub struct OBJECT_TYPE_INITIALIZER {
    pub length: USHORT,
    pub object_type_flags: USHORT,
    pub case_insensitive: bool,
    pub unnamed_objects_only: bool,
    pub use_default_object: bool,
    pub security_required: bool,
    pub maintain_handle_count: bool,
    pub maintain_type_list: bool,
    pub supports_object_callbacks: bool,
    pub cache_aligned: bool,
    pub _padding: [u8; 2],
    pub object_type_code: ULONG,
    pub invalid_attributes: ULONG,
    pub generic_mapping: GENERIC_MAPPING,
    pub valid_access_mask: u32,
    pub retain_access: u32,
    pub pool_type: ULONG,
    pub default_paged_pool_charge: ULONG,
    pub default_non_paged_pool_charge: ULONG,
    pub dump_procedure: PVOID,
    pub open_procedure: OB_OPEN_METHOD,
    pub close_procedure: OB_CLOSE_METHOD,
    pub delete_procedure: OB_DELETE_METHOD,
    pub parse_procedure: OB_PARSE_METHOD,
    pub security_procedure: OB_SECURITY_METHOD,
    pub query_name_procedure: OB_QUERY_NAME_METHOD,
    pub okay_to_close_procedure: PVOID,
    pub wait_object_flag_mask: ULONG,
    pub wait_object_flag_offset: USHORT,
    pub wait_object_pointer_offset: USHORT,
}

impl OBJECT_TYPE_INITIALIZER {
    pub const fn new() -> Self {
        Self {
            length: core::mem::size_of::<Self>() as USHORT,
            object_type_flags: 0,
            case_insensitive: false,
            unnamed_objects_only: false,
            use_default_object: false,
            security_required: false,
            maintain_handle_count: false,
            maintain_type_list: false,
            supports_object_callbacks: false,
            cache_aligned: false,
            _padding: [0; 2],
            object_type_code: 0,
            invalid_attributes: 0,
            generic_mapping: GENERIC_MAPPING::new(),
            valid_access_mask: 0,
            retain_access: 0,
            pool_type: 0,
            default_paged_pool_charge: 0,
            default_non_paged_pool_charge: 0,
            dump_procedure: core::ptr::null_mut(),
            open_procedure: None,
            close_procedure: None,
            delete_procedure: None,
            parse_procedure: None,
            security_procedure: None,
            query_name_procedure: None,
            okay_to_close_procedure: core::ptr::null_mut(),
            wait_object_flag_mask: 0,
            wait_object_flag_offset: 0,
            wait_object_pointer_offset: 0,
        }
    }
}

impl Default for OBJECT_TYPE_INITIALIZER {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// OBJECT_TYPE
// =============================================================================

/// Тип объекта
#[repr(C)]
pub struct OBJECT_TYPE {
    /// Список объектов этого типа
    pub type_list: LIST_ENTRY,
    /// Имя типа
    pub name: UNICODE_STRING,
    /// Wait object по умолчанию
    pub default_object: PVOID,
    /// Индекс типа
    pub index: u8,
    /// Общее количество объектов
    pub total_number_of_objects: u32,
    /// Общее количество хэндлов
    pub total_number_of_handles: u32,
    /// High water mark для объектов
    pub high_water_number_of_objects: u32,
    /// High water mark для хэндлов
    pub high_water_number_of_handles: u32,
    /// Информация о типе
    pub type_info: OBJECT_TYPE_INITIALIZER,
    /// Мьютекс типа
    pub type_mutex: ERESOURCE,
    /// Locks для объектов
    pub object_locks: [ERESOURCE; 4],
    /// Key (tag) для аллокаций
    pub key: u32,
}

impl OBJECT_TYPE {
    pub const fn new() -> Self {
        Self {
            type_list: LIST_ENTRY::new(),
            name: UNICODE_STRING::new(),
            default_object: core::ptr::null_mut(),
            index: 0,
            total_number_of_objects: 0,
            total_number_of_handles: 0,
            high_water_number_of_objects: 0,
            high_water_number_of_handles: 0,
            type_info: OBJECT_TYPE_INITIALIZER::new(),
            type_mutex: ERESOURCE::new(),
            object_locks: [
                ERESOURCE::new(),
                ERESOURCE::new(),
                ERESOURCE::new(),
                ERESOURCE::new(),
            ],
            key: 0,
        }
    }
}

// =============================================================================
// OBJECT_CREATE_INFORMATION
// =============================================================================

/// Информация о создании объекта
#[repr(C)]
pub struct OBJECT_CREATE_INFORMATION {
    pub attributes: ULONG,
    pub root_directory: PVOID, // HANDLE
    pub probe_mode: u8,
    pub paged_pool_charge: ULONG,
    pub non_paged_pool_charge: ULONG,
    pub security_descriptor_charge: ULONG,
    pub security_descriptor: PVOID,
    pub security_qos: PVOID,
}

impl OBJECT_CREATE_INFORMATION {
    pub const fn new() -> Self {
        Self {
            attributes: 0,
            root_directory: core::ptr::null_mut(),
            probe_mode: 0,
            paged_pool_charge: 0,
            non_paged_pool_charge: 0,
            security_descriptor_charge: 0,
            security_descriptor: core::ptr::null_mut(),
            security_qos: core::ptr::null_mut(),
        }
    }
}

// =============================================================================
// Pool Types
// =============================================================================

/// Non-paged pool
pub const NON_PAGED_POOL: ULONG = 0;
/// Paged pool
pub const PAGED_POOL: ULONG = 1;
