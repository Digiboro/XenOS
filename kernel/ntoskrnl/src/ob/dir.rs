//! Object Directory
//!
//! Директории объектов для namespace.
//!
//! Источники:
//! - ReactOS: ob/obdir.c

use super::header::*;
use super::types::*;
use crate::ke::EX_PUSH_LOCK;
use crate::nt::NTSTATUS;
use crate::nt::PVOID;
use crate::nt::STATUS_SUCCESS;
use crate::nt::ULONG;
use crate::nt::UNICODE_STRING;
use crate::rtl::unicode::rtl_equal_unicode_string;
use crate::rtl::unicode::rtl_upcase_unicode_char;
use core::sync::atomic::{AtomicUsize, Ordering};

/// Количество bucket'ов в hash table директории
pub const OBP_HASH_TABLE_SIZE: usize = 37;


/// Статистика directory operations
static DIRECTORY_STATS: DirectoryStats = DirectoryStats::new();

struct DirectoryStats {
    inserts: AtomicUsize,
    lookups: AtomicUsize,
    deletes: AtomicUsize,
    null_object_detections: AtomicUsize,
}

impl DirectoryStats {
    const fn new() -> Self {
        Self {
            inserts: AtomicUsize::new(0),
            lookups: AtomicUsize::new(0),
            deletes: AtomicUsize::new(0),
            null_object_detections: AtomicUsize::new(0),
        }
    }
    
    fn report(&self) {
        crate::dbg_print!(
            "[OB] Directory Stats: inserts={}, lookups={}, deletes={}, null_detections={}\n",
            self.inserts.load(Ordering::Relaxed),
            self.lookups.load(Ordering::Relaxed),
            self.deletes.load(Ordering::Relaxed),
            self.null_object_detections.load(Ordering::Relaxed)
        );
    }
}

/// Публичная функция для получения статистики
pub fn obp_get_directory_stats() {
    DIRECTORY_STATS.report();
}

/// Проверяет целостность directory
pub unsafe fn obp_validate_directory(directory: *mut OBJECT_DIRECTORY) -> bool {
    if directory.is_null() {
        crate::dbg_print!("[OB] Validate: directory is NULL\n");
        return false;
    }
    
    let mut total_entries = 0usize;
    let mut null_objects = 0usize;
    let mut valid_entries = 0usize;
    
    unsafe {
        // Проходим по всем buckets
        for bucket_idx in 0..OBP_HASH_TABLE_SIZE {
            let mut entry = (*directory).hash_buckets[bucket_idx];
            let mut bucket_len = 0usize;
            
            while !entry.is_null() {
                total_entries += 1;
                bucket_len += 1;
                
                // Проверяем object
                if (*entry).object.is_null() {
                    null_objects += 1;
                    crate::dbg_print!(
                        "[OB] Validate: NULL object in bucket {} at entry 0x{:016X}\n",
                        bucket_idx, entry as usize
                    );
                } else {
                    valid_entries += 1;
                    
                    // Проверяем что object header валиден
                    let header = object_to_object_header((*entry).object);
                    if (*header).type_ptr.is_null() {
                        crate::dbg_print!(
                            "[OB] Validate: Invalid header (NULL type) for object 0x{:016X}\n",
                            (*entry).object as usize
                        );
                    }
                }
                
                // Защита от циклов
                if bucket_len > 1000 {
                    crate::dbg_print!(
                        "[OB] Validate: Potential loop in bucket {} (len > 1000)\n",
                        bucket_idx
                    );
                    return false;
                }
                
                entry = (*entry).chain_link;
            }
        }
    }
    
    crate::dbg_print!(
        "[OB] Validate: dir=0x{:016X} total={} valid={} null={}\n",
        directory as usize, total_entries, valid_entries, null_objects
    );
    
    null_objects == 0
}

// =============================================================================
// OBJECT_DIRECTORY
// =============================================================================

/// Директория объектов
#[repr(C)]
pub struct OBJECT_DIRECTORY {
    /// Hash table для быстрого поиска объектов по имени
    pub hash_buckets: [*mut OBJECT_DIRECTORY_ENTRY; OBP_HASH_TABLE_SIZE],
    /// Lock для синхронизации доступа
    pub lock: EX_PUSH_LOCK,
    /// Device map
    pub device_map: PVOID,
    /// Session ID
    pub session_id: u32,
    /// Namespace entry (для shadow directories)
    pub namespace_entry: PVOID,
    /// Флаги
    pub flags: ULONG,
}

impl OBJECT_DIRECTORY {
    pub const fn new() -> Self {
        Self {
            hash_buckets: [core::ptr::null_mut(); OBP_HASH_TABLE_SIZE],
            lock: EX_PUSH_LOCK::new(),
            device_map: core::ptr::null_mut(),
            session_id: 0,
            namespace_entry: core::ptr::null_mut(),
            flags: 0,
        }
    }
}

/// Entry в hash table директории
#[repr(C)]
pub struct OBJECT_DIRECTORY_ENTRY {
    /// Следующий entry в chain
    pub chain_link: *mut OBJECT_DIRECTORY_ENTRY,
    /// Указатель на объект
    pub object: PVOID,
    /// Hash value имени
    pub hash_value: ULONG,
}

impl OBJECT_DIRECTORY_ENTRY {
    pub const fn new() -> Self {
        Self {
            chain_link: core::ptr::null_mut(),
            object: core::ptr::null_mut(),
            hash_value: 0,
        }
    }
}

// =============================================================================
// Lookup Context
// =============================================================================

/// Контекст поиска в директории
#[repr(C)]
pub struct OBP_LOOKUP_CONTEXT {
    /// Директория для поиска
    pub directory: *mut OBJECT_DIRECTORY,
    /// Найденный объект
    pub object: PVOID,
    /// Entry в hash table
    pub entry: *mut *mut OBJECT_DIRECTORY_ENTRY,
    /// Hash value
    pub hash_value: ULONG,
    /// Режим блокировки директории: 0=нет, 1=shared, 2=exclusive
    pub directory_lock_mode: u8,
}

impl OBP_LOOKUP_CONTEXT {
    pub const fn new() -> Self {
        Self {
            directory: core::ptr::null_mut(),
            object: core::ptr::null_mut(),
            entry: core::ptr::null_mut(),
            hash_value: 0,
            directory_lock_mode: 0,
        }
    }

    /// Инициализирует lookup context
    pub fn init(&mut self) {
        self.directory = core::ptr::null_mut();
        self.object = core::ptr::null_mut();
        self.entry = core::ptr::null_mut();
        self.hash_value = 0;
        self.directory_lock_mode = 0;
    }
}

// =============================================================================
// Hash Function
// =============================================================================

/// Вычисляет hash для имени объекта (как в NT/ReactOS: hash всегда case-folded).
///
/// Важно: hash считается **без учета** `OBJ_CASE_INSENSITIVE`.
/// Case-sensitive/insensitive влияет только на сравнение имен, но не на выбор bucket.
pub fn obp_hash_object_name(name: &UNICODE_STRING) -> ULONG {
    let mut hash: ULONG = 0;

    if name.buffer.is_null() || name.length == 0 {
        return 0;
    }

    let len = (name.length / 2) as usize;
    let chars = unsafe { core::slice::from_raw_parts(name.buffer, len) };

    for &ch in chars {
        // NT/ReactOS: hash всегда строится по upcase char.
        let c = rtl_upcase_unicode_char(ch) as ULONG;
        // "мешалка" как в ReactOS ob/obdir.c
        hash = hash.wrapping_add((hash << 1).wrapping_add(hash >> 1));
        hash = hash.wrapping_add(c);
    }

    hash
}

// =============================================================================
// Directory Operations
// =============================================================================

/// Ищет объект в директории
pub fn obp_lookup_entry_directory(
    directory: *mut OBJECT_DIRECTORY,
    name: &UNICODE_STRING,
    attributes: ULONG,
    look_for_insertion: bool,
    context: *mut OBP_LOOKUP_CONTEXT,
) -> bool {
    DIRECTORY_STATS.lookups.fetch_add(1, Ordering::Relaxed);
    
    if directory.is_null() || name.buffer.is_null() {
        return false;
    }

    let case_insensitive = (attributes & OBJ_CASE_INSENSITIVE) != 0;
    let hash_value = obp_hash_object_name(name);
    let hash_index = hash_value % OBP_HASH_TABLE_SIZE as ULONG;

    unsafe {
        let ctx = &mut *context;
        ctx.directory = directory;
        ctx.hash_value = hash_value;
        
        #[cfg(feature = "trace-ob")]
        {
            let len = (name.length / 2) as usize;
            let chars = core::slice::from_raw_parts(name.buffer, len);
            // Последние 4 символа
            let last = if len >= 4 { len - 4 } else { 0 };
            let c0 = if len > last { chars[last] } else { 0 };
            let c1 = if len > last + 1 { chars[last + 1] } else { 0 };
            let c2 = if len > last + 2 { chars[last + 2] } else { 0 };
            let c3 = if len > last + 3 { chars[last + 3] } else { 0 };
            crate::dbg_print!(
                "[OB] Lookup: hash={} idx={} len={} last4={:04X},{:04X},{:04X},{:04X} insert={}\n",
                hash_value, hash_index, name.length, c0, c1, c2, c3, look_for_insertion
            );
        }

        // Захватываем lock директории.
        // В NT: shared для lookup, exclusive для insertion path.
        if look_for_insertion {
            crate::ke::ex_acquire_push_lock_exclusive(&(*directory).lock);
            ctx.directory_lock_mode = 2;
        } else {
            crate::ke::ex_acquire_push_lock_shared(&(*directory).lock);
            ctx.directory_lock_mode = 1;
        }

        let bucket = &mut (*directory).hash_buckets[hash_index as usize];
        ctx.entry = bucket;

        let mut entry = *bucket;

        while !entry.is_null() {
            // Проверяем что object не NULL (защита от повреждённых структур)
            if (*entry).object.is_null() {
                // Обнаружен NULL object - это потенциальный баг!
                DIRECTORY_STATS.null_object_detections.fetch_add(1, Ordering::Relaxed);
                
                crate::dbg_print!(
                    "[OB] WARNING: NULL object detected in directory! entry=0x{:016X} hash_idx={}\n",
                    entry as usize, hash_index
                );
                
                // Пропускаем некорректный entry
                ctx.entry = &mut (*entry).chain_link;
                entry = (*entry).chain_link;
                continue;
            }
            
            // Получаем имя объекта
            let obj_header = object_to_object_header((*entry).object);

            if let Some(name_info) = (*obj_header).name_info() {
                // Сравниваем hash
                if (*entry).hash_value == hash_value {
                    // Сравниваем имена
                    if rtl_equal_unicode_string(&name_info.name, name, case_insensitive) {
                        ctx.object = (*entry).object;
                        return true;
                    }
                }
            }

            ctx.entry = &mut (*entry).chain_link;
            entry = (*entry).chain_link;
        }

        // Не найдено
        if look_for_insertion {
            // entry указывает на место для вставки
        }

        false
    }
}

/// Вставляет объект в директорию
pub fn obp_insert_entry_directory(
    directory: *mut OBJECT_DIRECTORY,
    context: *mut OBP_LOOKUP_CONTEXT,
    header: *mut OBJECT_HEADER,
) -> bool {
    DIRECTORY_STATS.inserts.fetch_add(1, Ordering::Relaxed);
    
    if directory.is_null() || header.is_null() {
        return false;
    }

    unsafe {
        let ctx = &mut *context;
        
        #[cfg(feature = "trace-ob")]
        {
            let object = object_header_to_object(header);
            crate::dbg_print!(
                "[OB] Insert: dir=0x{:016X} object=0x{:016X} hash={}\n",
                directory as usize, object as usize, ctx.hash_value
            );
        }

        // Выделяем entry
        let entry = crate::ex::pool::ex_allocate_pool_with_tag(
            crate::ex::pool::POOL_TYPE::PagedPool,
            core::mem::size_of::<OBJECT_DIRECTORY_ENTRY>(),
            u32::from_le_bytes(*b"erDO"), // 'ODir'
        ) as *mut OBJECT_DIRECTORY_ENTRY;

        if entry.is_null() {
            return false;
        }

        // Заполняем entry
        let object = object_header_to_object(header);
        debug_assert!(!object.is_null(), "obp_insert_entry_directory: object is NULL");
        
        (*entry).object = object;
        (*entry).hash_value = ctx.hash_value;
        (*entry).chain_link = *ctx.entry;

        // Вставляем в chain
        *ctx.entry = entry;

        // Устанавливаем директорию в name info объекта
        if let Some(name_info) = (*header).name_info_mut() {
            name_info.directory = directory as PVOID;
        }

        true
    }
}

/// Удаляет объект из директории
pub fn obp_delete_entry_directory(context: *mut OBP_LOOKUP_CONTEXT) -> bool {
    DIRECTORY_STATS.deletes.fetch_add(1, Ordering::Relaxed);
    
    if context.is_null() {
        return false;
    }

    unsafe {
        let ctx = &mut *context;

        if ctx.entry.is_null() || (*ctx.entry).is_null() {
            return false;
        }

        let entry = *ctx.entry;
        
        #[cfg(feature = "trace-ob")]
        crate::dbg_print!(
            "[OB] Delete: entry=0x{:016X} object=0x{:016X}\n",
            entry as usize, (*entry).object as usize
        );

        // Убираем из chain
        *ctx.entry = (*entry).chain_link;

        // Очищаем директорию в name info
        if !(*entry).object.is_null() {
            let header = object_to_object_header((*entry).object);
            if let Some(name_info) = (*header).name_info_mut() {
                name_info.directory = core::ptr::null_mut();
            }
        }

        // Освобождаем entry
        crate::ex::pool::ex_free_pool_with_tag(entry as PVOID, u32::from_le_bytes(*b"erDO"));

        true
    }
}

// =============================================================================
// NtCreateDirectoryObject
// =============================================================================

/// Создает директорию объектов
pub fn nt_create_directory_object(
    directory_handle: *mut PVOID,
    desired_access: u32,
    object_attributes: *mut OBJECT_ATTRIBUTES,
) -> NTSTATUS {
    if directory_handle.is_null() {
        return crate::nt::STATUS_INVALID_PARAMETER;
    }

    unsafe {
        *directory_handle = core::ptr::null_mut();

        // Получаем тип Directory
        let directory_type = super::init::obp_get_directory_object_type();
        if directory_type.is_null() {
            return crate::nt::STATUS_INSUFFICIENT_RESOURCES;
        }

        // Создаем объект
        let mut directory: PVOID = core::ptr::null_mut();
        let status = super::life::ob_create_object(
            0, // KernelMode
            directory_type,
            object_attributes,
            0, // KernelMode
            core::ptr::null_mut(),
            core::mem::size_of::<OBJECT_DIRECTORY>(),
            0,
            0,
            &mut directory,
        );

        if status != STATUS_SUCCESS {
            return status;
        }

        // Инициализируем директорию
        let dir = directory as *mut OBJECT_DIRECTORY;
        *dir = OBJECT_DIRECTORY::new();

        // Вставляем объект
        let status = super::life::ob_insert_object(
            directory,
            core::ptr::null_mut(),
            desired_access,
            0,
            core::ptr::null_mut(),
            directory_handle,
        );

        status
    }
}

/// Открывает существующую директорию
pub fn nt_open_directory_object(
    directory_handle: *mut PVOID,
    desired_access: u32,
    object_attributes: *mut OBJECT_ATTRIBUTES,
) -> NTSTATUS {
    use crate::nt::STATUS_INVALID_PARAMETER;

    // Проверяем параметры
    if directory_handle.is_null() || object_attributes.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    // Этап 5: открытие должно возвращать реальный HANDLE_TABLE handle.
    unsafe {
        *directory_handle = core::ptr::null_mut();
    }

    let directory_type = super::init::obp_get_directory_object_type();
    super::handle::ob_open_object_by_name(
        object_attributes,
        directory_type,
        0, // KernelMode
        core::ptr::null_mut(),
        desired_access,
        core::ptr::null_mut(),
        directory_handle,
    )
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Освобождает lookup context
pub fn obp_release_lookup_context(context: *mut OBP_LOOKUP_CONTEXT) {
    if context.is_null() {
        return;
    }

    unsafe {
        let ctx = &mut *context;

        // Разблокируем директорию если заблокирована
        if ctx.directory_lock_mode != 0 && !ctx.directory.is_null() {
            match ctx.directory_lock_mode {
                1 => crate::ke::ex_release_push_lock_shared(&(*ctx.directory).lock),
                2 => crate::ke::ex_release_push_lock_exclusive(&(*ctx.directory).lock),
                _ => {},
            }
            ctx.directory_lock_mode = 0;
        }

        ctx.directory = core::ptr::null_mut();
        ctx.object = core::ptr::null_mut();
        ctx.entry = core::ptr::null_mut();
    }
}
