//! Handle Tables - таблицы хэндлов
//!
//! Источники:
//! - NT5: ex/handle.c
//! - ReactOS: ex/handle.c

#![allow(dead_code)]
#![allow(non_camel_case_types)]

use core::sync::atomic::AtomicU32;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;

use crate::ke::spinlock::KSPIN_LOCK;
use crate::ke::spinlock::ke_acquire_spin_lock;
use crate::ke::spinlock::ke_release_spin_lock;
use crate::nt::LIST_ENTRY;
use crate::nt::PVOID;
use crate::nt::ULONG;

/// HANDLE - тип хэндла
pub type HANDLE = usize;

/// Invalid handle value
pub const INVALID_HANDLE_VALUE: HANDLE = usize::MAX;

/// Null handle
pub const NULL_HANDLE: HANDLE = 0;

/// Флаги хэндла (HANDLE_TABLE_ENTRY.handle_attributes)
///
/// Важно: это **не** OBJ_* (object attributes). Это именно handle-flags как в NT:
/// - HANDLE_FLAG_INHERIT
/// - HANDLE_FLAG_PROTECT_FROM_CLOSE
pub const HANDLE_FLAG_INHERIT: u32 = 0x00000001;
pub const HANDLE_FLAG_PROTECT_FROM_CLOSE: u32 = 0x00000002;

/// Handle index shift (кратность 4 для выравнивания)
pub const HANDLE_INDEX_SHIFT: u32 = 2;

/// Размер одного уровня таблицы
pub const HANDLE_TABLE_ENTRY_SIZE: usize = 256;

/// Максимальное количество хэндлов
pub const MAX_HANDLES: usize = 16 * 1024 * 1024; // 16M handles

/// HANDLE_TABLE_ENTRY - запись в таблице хэндлов
#[repr(C)]
pub struct HANDLE_TABLE_ENTRY {
    /// Указатель на объект (или следующий свободный)
    pub object: AtomicUsize,
    /// Granted access mask
    pub granted_access: AtomicU32,
    /// Атрибуты хэндла (HANDLE_FLAG_*)
    pub handle_attributes: AtomicU32,
}

/// Внутренний флаг: entry находится в free-list.
///
/// В этом случае `object` хранит не указатель, а "encoded next index".
const HANDLE_ENTRY_FLAG_FREE: usize = 0x1;

#[inline]
const fn encode_free_next(next_index: usize) -> usize {
    // Кодируем next_index так, чтобы отличать свободные entries от занятых:
    // - bit0 = 1 -> free
    // - next_index хранится в старших битах, сдвиг на 3 для совместимости с маской pointer flags.
    //
    // Спец-значение: usize::MAX = конец free-list.
    if next_index == usize::MAX {
        usize::MAX
    } else {
        (next_index << 3) | HANDLE_ENTRY_FLAG_FREE
    }
}

#[inline]
const fn decode_free_next(encoded: usize) -> usize {
    if encoded == usize::MAX {
        usize::MAX
    } else {
        encoded >> 3
    }
}

impl HANDLE_TABLE_ENTRY {
    pub const fn new() -> Self {
        Self {
            object: AtomicUsize::new(0),
            granted_access: AtomicU32::new(0),
            handle_attributes: AtomicU32::new(0),
        }
    }

    /// Проверяет свободна ли запись
    #[inline]
    pub fn is_free(&self) -> bool {
        (self.object.load(Ordering::Acquire) & HANDLE_ENTRY_FLAG_FREE) != 0
    }

    /// Получает объект
    #[inline]
    pub fn get_object(&self) -> PVOID {
        let v = self.object.load(Ordering::Acquire);
        if (v & HANDLE_ENTRY_FLAG_FREE) != 0 {
            return core::ptr::null_mut();
        }
        // Маскируем младшие биты (используются для флагов)
        (v & !0x7) as PVOID
    }

    /// Получает следующий свободный индекс (валидно только если entry свободен).
    #[inline]
    pub fn get_next_free_index(&self) -> usize {
        let v = self.object.load(Ordering::Acquire);
        debug_assert!((v & HANDLE_ENTRY_FLAG_FREE) != 0);
        decode_free_next(v)
    }

    /// Устанавливает объект
    #[inline]
    pub fn set_object(&self, object: PVOID, flags: usize) {
        self.object
            .store((object as usize) | (flags & 0x7), Ordering::Release);
    }

    /// Очищает запись
    #[inline]
    pub fn clear(&self) {
        self.object.store(0, Ordering::Release);
        self.granted_access.store(0, Ordering::Release);
        self.handle_attributes.store(0, Ordering::Release);
    }
}

impl Default for HANDLE_TABLE_ENTRY {
    fn default() -> Self {
        Self::new()
    }
}

/// HANDLE_TABLE - таблица хэндлов
pub struct HANDLE_TABLE {
    /// Спинлок
    pub lock: KSPIN_LOCK,
    /// Первый уровень таблицы
    pub table: *mut HANDLE_TABLE_ENTRY,
    /// Количество записей
    pub table_size: AtomicUsize,
    /// Следующий свободный индекс
    pub first_free_index: AtomicUsize,
    /// Количество хэндлов
    pub handle_count: AtomicU32,
    /// Связанный процесс
    pub owning_process: PVOID,
    /// Флаги
    pub flags: ULONG,
    /// Элемент в списке таблиц
    pub handle_table_list: LIST_ENTRY,
}

impl HANDLE_TABLE {
    /// Создает неинициализированную таблицу
    pub const fn new() -> Self {
        Self {
            lock: KSPIN_LOCK::new(),
            table: core::ptr::null_mut(),
            table_size: AtomicUsize::new(0),
            first_free_index: AtomicUsize::new(0),
            handle_count: AtomicU32::new(0),
            owning_process: core::ptr::null_mut(),
            flags: 0,
            handle_table_list: LIST_ENTRY::new(),
        }
    }

    /// Проверяет валидность индекса
    #[inline]
    pub fn is_valid_index(&self, index: usize) -> bool {
        index < self.table_size.load(Ordering::Acquire)
    }

    /// Преобразует хэндл в индекс
    #[inline]
    pub fn handle_to_index(handle: HANDLE) -> usize {
        (handle >> HANDLE_INDEX_SHIFT) as usize
    }

    /// Преобразует индекс в хэндл
    #[inline]
    pub fn index_to_handle(index: usize) -> HANDLE {
        (index << HANDLE_INDEX_SHIFT) as HANDLE
    }
}

impl Default for HANDLE_TABLE {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Internal Functions
// =============================================================================

/// Расширяет таблицу хэндлов
///
/// Удваивает размер таблицы, копирует существующие записи,
/// инициализирует новые записи как свободные.
///
/// # Safety
/// Должна вызываться с захваченным спинлоком таблицы.
unsafe fn exp_grow_handle_table(handle_table: *mut HANDLE_TABLE) -> bool {
    unsafe {
        let old_size = (*handle_table).table_size.load(Ordering::Acquire);

        // Проверяем лимит
        if old_size >= MAX_HANDLES {
            return false;
        }

        // Новый размер - удвоение (но не более MAX_HANDLES)
        let new_size = (old_size * 2).min(MAX_HANDLES);

        // Выделяем новый массив записей
        let new_entries_bytes = new_size * core::mem::size_of::<HANDLE_TABLE_ENTRY>();
        let new_entries_ptr = super::pool::ex_allocate_pool_with_tag(
            super::pool::POOL_TYPE::NonPagedPool,
            new_entries_bytes,
            u32::from_le_bytes(*b"Hent"),
        );

        if new_entries_ptr.is_null() {
            return false;
        }

        let new_entries = new_entries_ptr as *mut HANDLE_TABLE_ENTRY;
        let old_entries = (*handle_table).table;
        let old_first_free = (*handle_table).first_free_index.load(Ordering::Acquire);

        // Копируем существующие записи
        core::ptr::copy_nonoverlapping(old_entries, new_entries, old_size);

        // Инициализируем новые записи как свободные и цепляем к старому free-list.
        for i in old_size..new_size {
            let entry = new_entries.add(i);
            (*entry) = HANDLE_TABLE_ENTRY::new();
            // Связываем в список свободных
            let next = if i < new_size - 1 {
                i + 1
            } else {
                old_first_free
            };
            (*entry)
                .object
                .store(encode_free_next(next), Ordering::Release);
        }

        // Обновляем first_free_index на начало новых записей
        (*handle_table)
            .first_free_index
            .store(old_size, Ordering::Release);

        // Обновляем указатель и размер таблицы
        (*handle_table).table = new_entries;
        (*handle_table)
            .table_size
            .store(new_size, Ordering::Release);

        // Освобождаем старый массив
        if !old_entries.is_null() {
            super::pool::ex_free_pool_with_tag(old_entries as PVOID, u32::from_le_bytes(*b"Hent"));
        }

        true
    }
}

// =============================================================================
// API
// =============================================================================

/// ExCreateHandleTable - создает таблицу хэндлов
pub fn ex_create_handle_table(owning_process: PVOID) -> Option<*mut HANDLE_TABLE> {
    // Выделяем структуру таблицы
    let table_ptr = super::pool::ex_allocate_pool_with_tag(
        super::pool::POOL_TYPE::NonPagedPool,
        core::mem::size_of::<HANDLE_TABLE>(),
        u32::from_le_bytes(*b"Htbl"),
    );

    if table_ptr.is_null() {
        return None;
    }

    let table = table_ptr as *mut HANDLE_TABLE;

    // Выделяем начальный массив записей
    let entries_size = HANDLE_TABLE_ENTRY_SIZE * core::mem::size_of::<HANDLE_TABLE_ENTRY>();
    let entries_ptr = super::pool::ex_allocate_pool_with_tag(
        super::pool::POOL_TYPE::NonPagedPool,
        entries_size,
        u32::from_le_bytes(*b"Hent"),
    );

    if entries_ptr.is_null() {
        super::pool::ex_free_pool(table_ptr);
        return None;
    }

    unsafe {
        // Инициализируем таблицу
        (*table).lock = KSPIN_LOCK::new();
        (*table).table = entries_ptr as *mut HANDLE_TABLE_ENTRY;
        (*table)
            .table_size
            .store(HANDLE_TABLE_ENTRY_SIZE, Ordering::Release);
        // В NT handle 0 считается NULL/невалидным. Поэтому индекс 0 резервируем
        // и никогда не выдаём как валидный handle.
        (*table).first_free_index.store(1, Ordering::Release);
        (*table).handle_count.store(0, Ordering::Release);
        (*table).owning_process = owning_process;
        (*table).flags = 0;
        (*table).handle_table_list = LIST_ENTRY::new();

        // Инициализируем записи как свободные
        let entries = core::slice::from_raw_parts_mut(
            entries_ptr as *mut HANDLE_TABLE_ENTRY,
            HANDLE_TABLE_ENTRY_SIZE,
        );
        for (i, entry) in entries.iter_mut().enumerate() {
            *entry = HANDLE_TABLE_ENTRY::new();
            if i == 0 {
                // Индекс 0 занят/зарезервирован (NULL handle).
                entry.object.store(0, Ordering::Release);
            } else if i < HANDLE_TABLE_ENTRY_SIZE - 1 {
                // Связываем свободные записи в список, начиная с 1.
                entry
                    .object
                    .store(encode_free_next(i + 1), Ordering::Release);
            } else {
                entry
                    .object
                    .store(encode_free_next(usize::MAX), Ordering::Release); // End of free list
            }
        }
    }

    Some(table)
}

/// ExDestroyHandleTable - уничтожает таблицу хэндлов
pub fn ex_destroy_handle_table(handle_table: *mut HANDLE_TABLE) {
    if handle_table.is_null() {
        return;
    }

    unsafe {
        // Освобождаем массив записей
        if !(*handle_table).table.is_null() {
            super::pool::ex_free_pool_with_tag(
                (*handle_table).table as PVOID,
                u32::from_le_bytes(*b"Hent"),
            );
        }

        // Освобождаем структуру таблицы
        super::pool::ex_free_pool_with_tag(handle_table as PVOID, u32::from_le_bytes(*b"Htbl"));
    }
}

/// ExCreateHandle - создает хэндл для объекта
///
/// # Returns
/// Новый хэндл или INVALID_HANDLE_VALUE при ошибке
pub fn ex_create_handle(handle_table: *mut HANDLE_TABLE, object: PVOID, access: u32) -> HANDLE {
    ex_create_handle_ex(handle_table, object, access, 0)
}

/// ExCreateHandleEx - создает хэндл для объекта с атрибутами
///
/// Примечание: пока поддерживаем только `OBJ_INHERIT` (младший слой под ObDuplicateObject/наследование).
pub fn ex_create_handle_ex(
    handle_table: *mut HANDLE_TABLE,
    object: PVOID,
    access: u32,
    handle_attributes: u32,
) -> HANDLE {
    if handle_table.is_null() || object.is_null() {
        return INVALID_HANDLE_VALUE;
    }

    unsafe {
        let old_irql = ke_acquire_spin_lock(&(*handle_table).lock);

        // Получаем первый свободный индекс
        let free_index = (*handle_table).first_free_index.load(Ordering::Acquire);

        if free_index == usize::MAX
            || free_index >= (*handle_table).table_size.load(Ordering::Acquire)
        {
            // Пытаемся расширить таблицу
            if !exp_grow_handle_table(handle_table) {
                ke_release_spin_lock(&(*handle_table).lock, old_irql);
                return INVALID_HANDLE_VALUE;
            }
            // После расширения first_free_index обновлен
        }

        let entry = (*handle_table).table.add(free_index);

        // Запись должна быть свободной.
        if !(*entry).is_free() {
            ke_release_spin_lock(&(*handle_table).lock, old_irql);
            return INVALID_HANDLE_VALUE;
        }

        // Обновляем first_free_index на следующий свободный
        let next_free = (*entry).get_next_free_index();
        (*handle_table)
            .first_free_index
            .store(next_free, Ordering::Release);

        // Записываем объект
        (*entry).set_object(object, 0);
        (*entry).granted_access.store(access, Ordering::Release);
        (*entry)
            .handle_attributes
            .store(handle_attributes, Ordering::Release);

        (*handle_table).handle_count.fetch_add(1, Ordering::SeqCst);

        ke_release_spin_lock(&(*handle_table).lock, old_irql);

        HANDLE_TABLE::index_to_handle(free_index)
    }
}

/// ExDestroyHandle - удаляет хэндл
pub fn ex_destroy_handle(handle_table: *mut HANDLE_TABLE, handle: HANDLE) -> bool {
    if handle_table.is_null() || handle == NULL_HANDLE || handle == INVALID_HANDLE_VALUE {
        return false;
    }

    let index = HANDLE_TABLE::handle_to_index(handle);

    unsafe {
        let old_irql = ke_acquire_spin_lock(&(*handle_table).lock);

        if !(*handle_table).is_valid_index(index) {
            ke_release_spin_lock(&(*handle_table).lock, old_irql);
            return false;
        }

        let entry = (*handle_table).table.add(index);

        if (*entry).is_free() {
            ke_release_spin_lock(&(*handle_table).lock, old_irql);
            return false;
        }

        // Добавляем в free list
        let old_first_free = (*handle_table).first_free_index.load(Ordering::Acquire);
        (*entry)
            .object
            .store(encode_free_next(old_first_free), Ordering::Release);
        (*entry).granted_access.store(0, Ordering::Release);
        (*entry).handle_attributes.store(0, Ordering::Release);
        (*handle_table)
            .first_free_index
            .store(index, Ordering::Release);

        (*handle_table).handle_count.fetch_sub(1, Ordering::SeqCst);

        ke_release_spin_lock(&(*handle_table).lock, old_irql);

        true
    }
}

/// ExMapHandleToPointer - получает объект по хэндлу
///
/// # Safety
/// Возвращаемый указатель должен быть dereferenced только если таблица не изменяется
pub fn ex_map_handle_to_pointer(handle_table: *mut HANDLE_TABLE, handle: HANDLE) -> PVOID {
    if handle_table.is_null() || handle == NULL_HANDLE || handle == INVALID_HANDLE_VALUE {
        return core::ptr::null_mut();
    }

    let index = HANDLE_TABLE::handle_to_index(handle);

    unsafe {
        if !(*handle_table).is_valid_index(index) {
            return core::ptr::null_mut();
        }

        let entry = (*handle_table).table.add(index);
        (*entry).get_object()
    }
}

/// ExMapHandleToPointerEx - получает объект и access mask
pub fn ex_map_handle_to_pointer_ex(
    handle_table: *mut HANDLE_TABLE,
    handle: HANDLE,
) -> Option<(PVOID, u32)> {
    if handle_table.is_null() || handle == NULL_HANDLE || handle == INVALID_HANDLE_VALUE {
        return None;
    }

    let index = HANDLE_TABLE::handle_to_index(handle);

    unsafe {
        if !(*handle_table).is_valid_index(index) {
            return None;
        }

        let entry = (*handle_table).table.add(index);
        let object = (*entry).get_object();

        if object.is_null() {
            return None;
        }

        let access = (*entry).granted_access.load(Ordering::Acquire);
        Some((object, access))
    }
}

/// ExMapHandleToPointerFull - получает object, access и handle_attributes
pub fn ex_map_handle_to_pointer_full(
    handle_table: *mut HANDLE_TABLE,
    handle: HANDLE,
) -> Option<(PVOID, u32, u32)> {
    if handle_table.is_null() || handle == NULL_HANDLE || handle == INVALID_HANDLE_VALUE {
        return None;
    }

    let index = HANDLE_TABLE::handle_to_index(handle);

    unsafe {
        if !(*handle_table).is_valid_index(index) {
            return None;
        }

        let entry = (*handle_table).table.add(index);
        let object = (*entry).get_object();
        if object.is_null() {
            return None;
        }

        let access = (*entry).granted_access.load(Ordering::Acquire);
        let attrs = (*entry).handle_attributes.load(Ordering::Acquire);
        Some((object, access, attrs))
    }
}

/// ExCreateHandleAt - создает хэндл по фиксированному индексу (для наследования).
///
/// ВНИМАНИЕ: реализовано как “удаление” индекса из free list (O(n)).
pub fn ex_create_handle_at(
    handle_table: *mut HANDLE_TABLE,
    handle: HANDLE,
    object: PVOID,
    access: u32,
    handle_attributes: u32,
) -> bool {
    if handle_table.is_null()
        || object.is_null()
        || handle == NULL_HANDLE
        || handle == INVALID_HANDLE_VALUE
    {
        return false;
    }

    let index = HANDLE_TABLE::handle_to_index(handle);

    unsafe {
        let old_irql = ke_acquire_spin_lock(&(*handle_table).lock);

        // Расширяем таблицу до нужного индекса.
        while !(*handle_table).is_valid_index(index) {
            if !exp_grow_handle_table(handle_table) {
                ke_release_spin_lock(&(*handle_table).lock, old_irql);
                return false;
            }
        }

        // Ищем index в free list
        let mut prev: Option<usize> = None;
        let mut cur = (*handle_table).first_free_index.load(Ordering::Acquire);
        while cur != usize::MAX {
            if cur == index {
                break;
            }
            let next = (*(*handle_table).table.add(cur)).get_next_free_index();
            prev = Some(cur);
            cur = next;
        }

        if cur != index {
            // Уже занят или не найден
            ke_release_spin_lock(&(*handle_table).lock, old_irql);
            return false;
        }

        let next_free = (*(*handle_table).table.add(index)).get_next_free_index();
        match prev {
            None => (*handle_table)
                .first_free_index
                .store(next_free, Ordering::Release),
            Some(p) => (*(*handle_table).table.add(p))
                .object
                .store(encode_free_next(next_free), Ordering::Release),
        }

        // Записываем entry
        let entry = (*handle_table).table.add(index);
        (*entry).set_object(object, 0);
        (*entry).granted_access.store(access, Ordering::Release);
        (*entry)
            .handle_attributes
            .store(handle_attributes, Ordering::Release);

        (*handle_table).handle_count.fetch_add(1, Ordering::SeqCst);

        ke_release_spin_lock(&(*handle_table).lock, old_irql);
        true
    }
}

/// ExChangeHandle - изменяет хэндл
pub fn ex_change_handle(
    handle_table: *mut HANDLE_TABLE,
    handle: HANDLE,
    new_object: PVOID,
    new_access: u32,
) -> bool {
    if handle_table.is_null() || handle == NULL_HANDLE || handle == INVALID_HANDLE_VALUE {
        return false;
    }

    let index = HANDLE_TABLE::handle_to_index(handle);

    unsafe {
        let old_irql = ke_acquire_spin_lock(&(*handle_table).lock);

        if !(*handle_table).is_valid_index(index) {
            ke_release_spin_lock(&(*handle_table).lock, old_irql);
            return false;
        }

        let entry = (*handle_table).table.add(index);

        if (*entry).is_free() {
            ke_release_spin_lock(&(*handle_table).lock, old_irql);
            return false;
        }

        (*entry).set_object(new_object, 0);
        (*entry).granted_access.store(new_access, Ordering::Release);

        ke_release_spin_lock(&(*handle_table).lock, old_irql);

        true
    }
}

/// Возвращает количество хэндлов в таблице
pub fn ex_get_handle_count(handle_table: *mut HANDLE_TABLE) -> u32 {
    if handle_table.is_null() {
        return 0;
    }

    unsafe { (*handle_table).handle_count.load(Ordering::Acquire) }
}
