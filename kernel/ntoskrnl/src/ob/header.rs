//! Object Header
//!
//! Заголовок объекта и вспомогательные информационные структуры.

use core::sync::atomic::AtomicIsize;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;

use super::types::OBJECT_CREATE_INFORMATION;
use super::types::OBJECT_TYPE;
use crate::nt::LIST_ENTRY;
use crate::nt::PVOID;
use crate::nt::UCHAR;
use crate::nt::ULONG;
use crate::nt::UNICODE_STRING;

// =============================================================================
// OBJECT_HEADER_NAME_INFO
// =============================================================================

/// Информация об имени объекта
#[repr(C)]
pub struct OBJECT_HEADER_NAME_INFO {
    /// Директория, содержащая объект
    pub directory: PVOID, // *mut OBJECT_DIRECTORY
    /// Имя объекта
    pub name: UNICODE_STRING,
    /// Счетчик запросов имени
    pub query_references: i32,
}

impl OBJECT_HEADER_NAME_INFO {
    pub const fn new() -> Self {
        Self {
            directory: core::ptr::null_mut(),
            name: UNICODE_STRING::new(),
            query_references: 1,
        }
    }
}

// =============================================================================
// OBJECT_HEADER_CREATOR_INFO
// =============================================================================

/// Информация о создателе объекта
#[repr(C)]
pub struct OBJECT_HEADER_CREATOR_INFO {
    /// Список объектов данного типа
    pub type_list: LIST_ENTRY,
    /// BackTrace index создателя (для отладки)
    pub creator_back_trace_index: u16,
    pub reserved: u16,
    /// Unique ID процесса-создателя
    pub creator_unique_process: PVOID,
}

impl OBJECT_HEADER_CREATOR_INFO {
    pub const fn new() -> Self {
        Self {
            type_list: LIST_ENTRY::new(),
            creator_back_trace_index: 0,
            reserved: 0,
            creator_unique_process: core::ptr::null_mut(),
        }
    }
}

// =============================================================================
// OBJECT_HEADER_HANDLE_INFO
// =============================================================================

/// Информация о хэндлах объекта
#[repr(C)]
pub struct OBJECT_HEADER_HANDLE_INFO {
    /// Single entry для простых объектов
    pub single_entry: OBJECT_HANDLE_COUNT_ENTRY,
}

/// Счетчик хэндлов для процесса
#[repr(C)]
#[derive(Clone, Copy)]
pub struct OBJECT_HANDLE_COUNT_ENTRY {
    pub process: PVOID, // *mut EPROCESS
    pub handle_count: u32,
}

impl OBJECT_HEADER_HANDLE_INFO {
    pub const fn new() -> Self {
        Self {
            single_entry: OBJECT_HANDLE_COUNT_ENTRY {
                process: core::ptr::null_mut(),
                handle_count: 0,
            },
        }
    }
}

// =============================================================================
// OBJECT_HEADER_QUOTA_INFO
// =============================================================================

/// Информация о квотах объекта
#[repr(C)]
pub struct OBJECT_HEADER_QUOTA_INFO {
    pub paged_pool_charge: ULONG,
    pub non_paged_pool_charge: ULONG,
    pub security_descriptor_charge: ULONG,
    pub exclusive_process: PVOID, // *mut EPROCESS
}

impl OBJECT_HEADER_QUOTA_INFO {
    pub const fn new() -> Self {
        Self {
            paged_pool_charge: 0,
            non_paged_pool_charge: 0,
            security_descriptor_charge: 0,
            exclusive_process: core::ptr::null_mut(),
        }
    }
}

// =============================================================================
// OBJECT_HEADER
// =============================================================================

/// Заголовок объекта
///
/// Располагается непосредственно перед телом объекта в памяти.
/// Опциональные информационные структуры располагаются перед заголовком.
#[repr(C)]
pub struct OBJECT_HEADER {
    /// Счетчик ссылок (указатели)
    pub pointer_count: AtomicIsize,
    /// Счетчик хэндлов или указатель на следующий объект для удаления
    pub handle_count_or_next_to_free: HandleCountUnion,
    /// EX_PUSH_LOCK
    pub lock: AtomicUsize,
    /// Security descriptor
    pub security_descriptor: PVOID,
    /// Тип объекта
    pub type_ptr: *mut OBJECT_TYPE,
    /// Offset к NAME_INFO от начала заголовка
    pub name_info_offset: UCHAR,
    /// Offset к HANDLE_INFO от начала заголовка
    pub handle_info_offset: UCHAR,
    /// Offset к QUOTA_INFO от начала заголовка
    pub quota_info_offset: UCHAR,
    /// Флаги объекта
    pub flags: UCHAR,
    /// Информация о создании или QuotaBlockCharged
    pub object_create_info_or_quota: CreateInfoUnion,
    // Тело объекта начинается сразу после этого поля
}

#[repr(C)]
pub union HandleCountUnion {
    pub handle_count: isize,
    pub next_to_free: *mut OBJECT_HEADER,
}

#[repr(C)]
pub union CreateInfoUnion {
    pub object_create_info: *mut OBJECT_CREATE_INFORMATION,
    pub quota_block_charged: PVOID,
}

impl OBJECT_HEADER {
    /// Создает новый заголовок объекта
    pub const fn new() -> Self {
        Self {
            pointer_count: AtomicIsize::new(1),
            handle_count_or_next_to_free: HandleCountUnion { handle_count: 0 },
            lock: AtomicUsize::new(0),
            security_descriptor: core::ptr::null_mut(),
            type_ptr: core::ptr::null_mut(),
            name_info_offset: 0,
            handle_info_offset: 0,
            quota_info_offset: 0,
            flags: 0,
            object_create_info_or_quota: CreateInfoUnion {
                object_create_info: core::ptr::null_mut(),
            },
        }
    }

    /// Возвращает указатель на тело объекта
    #[inline]
    pub fn body(&self) -> PVOID {
        unsafe { (self as *const Self as *const u8).add(core::mem::size_of::<Self>()) as PVOID }
    }

    /// Возвращает счетчик указателей
    #[inline]
    pub fn pointer_count(&self) -> isize {
        self.pointer_count.load(Ordering::Acquire)
    }

    /// Возвращает счетчик хэндлов
    #[inline]
    pub fn handle_count(&self) -> isize {
        unsafe { self.handle_count_or_next_to_free.handle_count }
    }

    /// Получает NAME_INFO если есть
    #[inline]
    pub fn name_info(&self) -> Option<&OBJECT_HEADER_NAME_INFO> {
        if self.name_info_offset == 0 {
            return None;
        }
        unsafe {
            let ptr = (self as *const Self as *const u8).sub(self.name_info_offset as usize);
            Some(&*(ptr as *const OBJECT_HEADER_NAME_INFO))
        }
    }

    /// Получает NAME_INFO mutable если есть
    #[inline]
    pub fn name_info_mut(&mut self) -> Option<&mut OBJECT_HEADER_NAME_INFO> {
        if self.name_info_offset == 0 {
            return None;
        }
        unsafe {
            let ptr = (self as *mut Self as *mut u8).sub(self.name_info_offset as usize);
            Some(&mut *(ptr as *mut OBJECT_HEADER_NAME_INFO))
        }
    }

    /// Получает CREATOR_INFO если есть
    #[inline]
    pub fn creator_info(&self) -> Option<&OBJECT_HEADER_CREATOR_INFO> {
        use super::types::OB_FLAG_CREATOR_INFO;
        if (self.flags & OB_FLAG_CREATOR_INFO) == 0 {
            return None;
        }
        // CREATOR_INFO располагается сразу перед заголовком
        unsafe {
            let ptr = (self as *const Self as *const u8)
                .sub(core::mem::size_of::<OBJECT_HEADER_CREATOR_INFO>());
            Some(&*(ptr as *const OBJECT_HEADER_CREATOR_INFO))
        }
    }

    /// Получает HANDLE_INFO если есть
    #[inline]
    pub fn handle_info(&self) -> Option<&OBJECT_HEADER_HANDLE_INFO> {
        if self.handle_info_offset == 0 {
            return None;
        }
        unsafe {
            let ptr = (self as *const Self as *const u8).sub(self.handle_info_offset as usize);
            Some(&*(ptr as *const OBJECT_HEADER_HANDLE_INFO))
        }
    }

    /// Получает HANDLE_INFO mutable если есть
    #[inline]
    pub fn handle_info_mut(&mut self) -> Option<&mut OBJECT_HEADER_HANDLE_INFO> {
        if self.handle_info_offset == 0 {
            return None;
        }
        unsafe {
            let ptr = (self as *mut Self as *mut u8).sub(self.handle_info_offset as usize);
            Some(&mut *(ptr as *mut OBJECT_HEADER_HANDLE_INFO))
        }
    }

    /// Получает QUOTA_INFO если есть
    #[inline]
    pub fn quota_info(&self) -> Option<&OBJECT_HEADER_QUOTA_INFO> {
        if self.quota_info_offset == 0 {
            return None;
        }
        unsafe {
            let ptr = (self as *const Self as *const u8).sub(self.quota_info_offset as usize);
            Some(&*(ptr as *const OBJECT_HEADER_QUOTA_INFO))
        }
    }

    /// Получает QUOTA_INFO mutable если есть
    #[inline]
    pub fn quota_info_mut(&mut self) -> Option<&mut OBJECT_HEADER_QUOTA_INFO> {
        if self.quota_info_offset == 0 {
            return None;
        }
        unsafe {
            let ptr = (self as *mut Self as *mut u8).sub(self.quota_info_offset as usize);
            Some(&mut *(ptr as *mut OBJECT_HEADER_QUOTA_INFO))
        }
    }
}

// =============================================================================
// Helper Functions/Macros
// =============================================================================

/// Получает OBJECT_HEADER из указателя на тело объекта
#[inline]
pub unsafe fn object_to_object_header(object: PVOID) -> *mut OBJECT_HEADER {
    unsafe {
        debug_assert!(!object.is_null(), "object_to_object_header called with NULL");
        (object as *mut u8).sub(core::mem::size_of::<OBJECT_HEADER>()) as *mut OBJECT_HEADER
    }
}

/// Получает тело объекта из OBJECT_HEADER
#[inline]
pub unsafe fn object_header_to_object(header: *mut OBJECT_HEADER) -> PVOID {
    unsafe { (header as *mut u8).add(core::mem::size_of::<OBJECT_HEADER>()) as PVOID }
}

/// Получает NAME_INFO из OBJECT_HEADER
#[inline]
pub unsafe fn object_header_to_name_info(
    header: *const OBJECT_HEADER,
) -> *const OBJECT_HEADER_NAME_INFO {
    unsafe {
        let offset = (*header).name_info_offset;
        if offset == 0 {
            return core::ptr::null();
        }
        (header as *const u8).sub(offset as usize) as *const OBJECT_HEADER_NAME_INFO
    }
}

/// Получает CREATOR_INFO из OBJECT_HEADER
#[inline]
pub unsafe fn object_header_to_creator_info(
    header: *const OBJECT_HEADER,
) -> *const OBJECT_HEADER_CREATOR_INFO {
    unsafe {
        use super::types::OB_FLAG_CREATOR_INFO;
        if ((*header).flags & OB_FLAG_CREATOR_INFO) == 0 {
            return core::ptr::null();
        }
        (header as *const u8).sub(core::mem::size_of::<OBJECT_HEADER_CREATOR_INFO>())
            as *const OBJECT_HEADER_CREATOR_INFO
    }
}
