//! Базовые определения типов NT
//!
//! Источники:
//! - NT5: inc/ntdef.h
//! - ReactOS: include/ndk/ntdef.h

#![allow(non_camel_case_types)]

// Базовые целочисленные типы
pub type CHAR = i8;
pub type UCHAR = u8;
pub type SHORT = i16;
pub type CSHORT = i16; // "Counted" SHORT
pub type USHORT = u16;
pub type LONG = i32;
pub type CLONG = i32; // "Counted" LONG
pub type ULONG = u32;
pub type LONGLONG = i64;
pub type ULONGLONG = u64;

// Указательные типы
pub type PVOID = *mut core::ffi::c_void;
pub type PCVOID = *const core::ffi::c_void;
pub type ULONG_PTR = usize;
pub type LONG_PTR = isize;
pub type SIZE_T = usize;

/// HANDLE - дескриптор объекта
pub type HANDLE = usize;
pub type PHANDLE = *mut HANDLE;

/// Специальные значения HANDLE
pub const NULL_HANDLE: HANDLE = 0;
pub const INVALID_HANDLE_VALUE: HANDLE = !0usize;

/// ACCESS_MASK - маска прав доступа
pub type ACCESS_MASK = u32;

// Булевы типы
pub type BOOLEAN = u8;
pub type BOOL = i32;

pub const TRUE: BOOLEAN = 1;
pub const FALSE: BOOLEAN = 0;

/// LARGE_INTEGER - 64-битное целое (union в оригинале)
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LARGE_INTEGER {
    pub quad_part: i64,
}

impl LARGE_INTEGER {
    pub const fn new(value: i64) -> Self {
        Self { quad_part: value }
    }

    pub fn low_part(&self) -> u32 {
        self.quad_part as u32
    }

    pub fn high_part(&self) -> i32 {
        (self.quad_part >> 32) as i32
    }
}

/// ULARGE_INTEGER - 64-битное беззнаковое целое
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ULARGE_INTEGER {
    pub quad_part: u64,
}

impl ULARGE_INTEGER {
    pub const fn new(value: u64) -> Self {
        Self { quad_part: value }
    }

    pub fn low_part(&self) -> u32 {
        self.quad_part as u32
    }

    pub fn high_part(&self) -> u32 {
        (self.quad_part >> 32) as u32
    }
}

/// LIST_ENTRY - двусвязный список (intrusive)
///
/// Ключевая структура NT для связных списков.
/// Используется во всех подсистемах ядра.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LIST_ENTRY {
    pub flink: *mut LIST_ENTRY,
    pub blink: *mut LIST_ENTRY,
}

impl LIST_ENTRY {
    /// Создает пустой элемент списка
    pub const fn new() -> Self {
        Self {
            flink: core::ptr::null_mut(),
            blink: core::ptr::null_mut(),
        }
    }

    /// Инициализирует заголовок списка (указывает на себя)
    ///
    /// # Safety
    /// Вызывающий должен гарантировать что указатель валиден
    pub unsafe fn init_head(head: *mut LIST_ENTRY) {
        unsafe {
            (*head).flink = head;
            (*head).blink = head;
        }
    }

    /// Проверяет пуст ли список
    ///
    /// # Safety
    /// Вызывающий должен гарантировать что указатель валиден
    pub unsafe fn is_empty(head: *const LIST_ENTRY) -> bool {
        unsafe { (*head).flink == head as *mut LIST_ENTRY }
    }

    /// Вставляет элемент в начало списка (после head)
    ///
    /// # Safety
    /// Вызывающий должен гарантировать что указатели валидны
    pub unsafe fn insert_head(head: *mut LIST_ENTRY, entry: *mut LIST_ENTRY) {
        unsafe {
            let flink = (*head).flink;
            (*entry).flink = flink;
            (*entry).blink = head;
            (*flink).blink = entry;
            (*head).flink = entry;
        }
    }

    /// Вставляет элемент в конец списка (перед head)
    ///
    /// # Safety
    /// Вызывающий должен гарантировать что указатели валидны
    pub unsafe fn insert_tail(head: *mut LIST_ENTRY, entry: *mut LIST_ENTRY) {
        unsafe {
            let blink = (*head).blink;
            (*entry).flink = head;
            (*entry).blink = blink;
            (*blink).flink = entry;
            (*head).blink = entry;
        }
    }

    /// Удаляет элемент из списка
    ///
    /// # Safety
    /// Вызывающий должен гарантировать что указатель валиден и элемент в списке
    pub unsafe fn remove_entry(entry: *mut LIST_ENTRY) -> bool {
        unsafe {
            let flink = (*entry).flink;
            let blink = (*entry).blink;
            (*blink).flink = flink;
            (*flink).blink = blink;
            flink == blink
        }
    }

    /// CONTAINING_RECORD — получает указатель на структуру по указателю на её поле
    ///
    /// # Arguments
    /// * `entry` - указатель на LIST_ENTRY
    /// * `offset` - смещение LIST_ENTRY в структуре (в байтах)
    ///
    /// # Safety
    /// Вызывающий должен гарантировать правильность типа и смещения
    #[inline]
    pub unsafe fn containing_record<T>(entry: *mut LIST_ENTRY, offset: usize) -> *mut T {
        (entry as usize - offset) as *mut T
    }

    /// Удаляет элемент из начала списка
    ///
    /// # Safety
    /// Вызывающий должен гарантировать что указатель валиден
    pub unsafe fn remove_head(head: *mut LIST_ENTRY) -> *mut LIST_ENTRY {
        unsafe {
            let entry = (*head).flink;
            let flink = (*entry).flink;
            (*head).flink = flink;
            (*flink).blink = head;
            entry
        }
    }

    /// Удаляет элемент из конца списка
    ///
    /// # Safety
    /// Вызывающий должен гарантировать что указатель валиден
    pub unsafe fn remove_tail(head: *mut LIST_ENTRY) -> *mut LIST_ENTRY {
        unsafe {
            let entry = (*head).blink;
            let blink = (*entry).blink;
            (*head).blink = blink;
            (*blink).flink = head;
            entry
        }
    }
}

impl Default for LIST_ENTRY {
    fn default() -> Self {
        Self::new()
    }
}

/// SINGLE_LIST_ENTRY - односвязный список
#[repr(C)]
pub struct SINGLE_LIST_ENTRY {
    pub next: *mut SINGLE_LIST_ENTRY,
}

impl SINGLE_LIST_ENTRY {
    pub const fn new() -> Self {
        Self {
            next: core::ptr::null_mut(),
        }
    }
}

impl Default for SINGLE_LIST_ENTRY {
    fn default() -> Self {
        Self::new()
    }
}

/// UNICODE_STRING - строка Unicode
///
/// Основной строковый тип NT ядра.
/// Length - текущая длина в байтах (не включая null terminator)
/// MaximumLength - максимальная длина буфера в байтах
#[repr(C)]
#[derive(Clone, Copy)]
pub struct UNICODE_STRING {
    pub length: USHORT,
    pub maximum_length: USHORT,
    pub buffer: *mut u16,
}

impl UNICODE_STRING {
    pub const fn new() -> Self {
        Self {
            length: 0,
            maximum_length: 0,
            buffer: core::ptr::null_mut(),
        }
    }

    /// Инициализирует UNICODE_STRING из статического буфера
    ///
    /// # Safety
    /// Буфер должен быть валидным и содержать корректную UTF-16 строку
    pub unsafe fn init_from_buffer(buffer: *mut u16, length: USHORT, max_length: USHORT) -> Self {
        Self {
            length,
            maximum_length: max_length,
            buffer,
        }
    }

    /// Создает UNICODE_STRING из статической строки (ASCII only)
    ///
    /// ВНИМАНИЕ: Возвращает пустую строку, так как не можем
    /// выделить память в const контексте. Используйте init_from_buffer
    /// для реального использования.
    pub const fn from_str(_s: &str) -> Self {
        // Для инициализации типов объектов используем пустую строку
        // Реальное имя будет установлено позже через init_from_buffer
        Self::new()
    }
}

impl Default for UNICODE_STRING {
    fn default() -> Self {
        Self::new()
    }
}

/// ANSI_STRING - строка ANSI (8-bit)
#[repr(C)]
pub struct ANSI_STRING {
    pub length: USHORT,
    pub maximum_length: USHORT,
    pub buffer: *mut CHAR,
}

impl ANSI_STRING {
    pub const fn new() -> Self {
        Self {
            length: 0,
            maximum_length: 0,
            buffer: core::ptr::null_mut(),
        }
    }
}

impl Default for ANSI_STRING {
    fn default() -> Self {
        Self::new()
    }
}

/// STRING - синоним для ANSI_STRING
pub type STRING = ANSI_STRING;

/// OBJECT_ATTRIBUTES - атрибуты объекта для создания/открытия
#[repr(C)]
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
}

// Флаги OBJECT_ATTRIBUTES
pub const OBJ_PROTECT_CLOSE: ULONG = 0x00000001;
pub const OBJ_INHERIT: ULONG = 0x00000002;
pub const OBJ_PERMANENT: ULONG = 0x00000010;
pub const OBJ_EXCLUSIVE: ULONG = 0x00000020;
pub const OBJ_CASE_INSENSITIVE: ULONG = 0x00000040;
pub const OBJ_OPENIF: ULONG = 0x00000080;
pub const OBJ_OPENLINK: ULONG = 0x00000100;
pub const OBJ_KERNEL_HANDLE: ULONG = 0x00000200;
pub const OBJ_FORCE_ACCESS_CHECK: ULONG = 0x00000400;

/// CLIENT_ID - идентификатор клиента (процесс + поток)
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct CLIENT_ID {
    pub unique_process: PVOID, // HANDLE
    pub unique_thread: PVOID,  // HANDLE
}

/// Макрос CONTAINING_RECORD - получение указателя на структуру по указателю на её поле
///
/// Rust реализация классического NT макроса.
/// Использование: containing_record!(ptr, StructType, field_name)
#[macro_export]
macro_rules! containing_record {
    ($ptr:expr, $type:ty, $field:ident) => {{
        let ptr = $ptr as *const u8;
        let offset = core::mem::offset_of!($type, $field);
        (ptr.sub(offset)) as *mut $type
    }};
}
