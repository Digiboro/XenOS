//! Lookaside Lists - кэш блоков фиксированного размера
//!
//! Источники:
//! - NT5: ex/lookas.c
//! - ReactOS: ex/lookas.c

#![allow(dead_code)]
#![allow(non_camel_case_types)]

use core::sync::atomic::AtomicPtr;
use core::sync::atomic::AtomicU32;
use core::sync::atomic::Ordering;

use super::pool::POOL_TAG;
use super::pool::POOL_TYPE;
use super::pool::ex_allocate_pool_with_tag;
use super::pool::ex_free_pool_with_tag;
use crate::nt::LIST_ENTRY;
use crate::nt::PVOID;
use crate::nt::ULONG;
use crate::nt::USHORT;

/// Тип функции выделения
pub type PALLOCATE_FUNCTION = Option<fn(POOL_TYPE, usize, POOL_TAG) -> PVOID>;

/// Тип функции освобождения
pub type PFREE_FUNCTION = Option<fn(PVOID)>;

/// Глубина lookaside list по умолчанию
pub const LOOKASIDE_MINIMUM_DEPTH: u16 = 4;
pub const LOOKASIDE_MAXIMUM_DEPTH: u16 = 256;

/// SLIST_HEADER - lock-free single-linked list header
#[repr(C)]
pub struct SLIST_HEADER {
    /// Указатель на первый элемент
    pub next: AtomicPtr<SLIST_ENTRY>,
    /// Глубина и sequence number (для ABA protection)
    pub depth_sequence: AtomicU32,
}

impl SLIST_HEADER {
    pub const fn new() -> Self {
        Self {
            next: AtomicPtr::new(core::ptr::null_mut()),
            depth_sequence: AtomicU32::new(0),
        }
    }

    /// Возвращает текущую глубину
    #[inline]
    pub fn depth(&self) -> u16 {
        (self.depth_sequence.load(Ordering::Acquire) & 0xFFFF) as u16
    }
}

impl Default for SLIST_HEADER {
    fn default() -> Self {
        Self::new()
    }
}

/// SLIST_ENTRY - элемент single-linked list
#[repr(C)]
pub struct SLIST_ENTRY {
    pub next: *mut SLIST_ENTRY,
}

impl SLIST_ENTRY {
    pub const fn new() -> Self {
        Self {
            next: core::ptr::null_mut(),
        }
    }
}

/// Инициализирует SLIST_HEADER
pub fn ex_initialize_slist_head(list_head: &mut SLIST_HEADER) {
    list_head
        .next
        .store(core::ptr::null_mut(), Ordering::Release);
    list_head.depth_sequence.store(0, Ordering::Release);
}

/// Добавляет элемент в начало списка (push)
///
/// # Safety
/// entry должен быть валидным указателем
pub unsafe fn ex_interlock_push_entry_slist(
    list_head: &SLIST_HEADER,
    list_entry: *mut SLIST_ENTRY,
) -> *mut SLIST_ENTRY {
    loop {
        let old_next = list_head.next.load(Ordering::Acquire);
        unsafe {
            (*list_entry).next = old_next;
        }

        if list_head
            .next
            .compare_exchange_weak(old_next, list_entry, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            // Увеличиваем depth
            list_head.depth_sequence.fetch_add(1, Ordering::AcqRel);
            return old_next;
        }
    }
}

/// Извлекает элемент из начала списка (pop)
pub fn ex_interlock_pop_entry_slist(list_head: &SLIST_HEADER) -> *mut SLIST_ENTRY {
    loop {
        let entry = list_head.next.load(Ordering::Acquire);
        if entry.is_null() {
            return core::ptr::null_mut();
        }

        let next = unsafe { (*entry).next };

        if list_head
            .next
            .compare_exchange_weak(entry, next, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            // Уменьшаем depth
            list_head.depth_sequence.fetch_sub(1, Ordering::AcqRel);
            return entry;
        }
    }
}

/// Очищает список и возвращает первый элемент
pub fn ex_interlock_flush_slist(list_head: &SLIST_HEADER) -> *mut SLIST_ENTRY {
    loop {
        let first = list_head.next.load(Ordering::Acquire);

        if list_head
            .next
            .compare_exchange_weak(
                first,
                core::ptr::null_mut(),
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
        {
            list_head.depth_sequence.store(0, Ordering::Release);
            return first;
        }
    }
}

/// Возвращает глубину списка
pub fn ex_query_depth_slist(list_head: &SLIST_HEADER) -> u16 {
    list_head.depth()
}

// =============================================================================
// NPAGED_LOOKASIDE_LIST - NonPaged lookaside list
// =============================================================================

/// NPAGED_LOOKASIDE_LIST - lookaside для NonPagedPool
#[repr(C)]
pub struct NPAGED_LOOKASIDE_LIST {
    /// Lock-free список свободных блоков
    pub list_head: SLIST_HEADER,
    /// Глубина (макс количество кэшированных блоков)
    pub depth: USHORT,
    /// Максимальная глубина
    pub maximum_depth: USHORT,
    /// Количество успешных аллокаций из кэша
    pub total_allocates: AtomicU32,
    /// Количество промахов (нужно было выделять из пула)
    pub allocate_misses: AtomicU32,
    /// Количество освобождений в кэш
    pub total_frees: AtomicU32,
    /// Количество "лишних" освобождений (кэш полон)
    pub free_misses: AtomicU32,
    /// Тип пула
    pub pool_type: POOL_TYPE,
    /// Tag для пула
    pub tag: POOL_TAG,
    /// Размер блока
    pub size: ULONG,
    /// Функция выделения
    pub allocate: PALLOCATE_FUNCTION,
    /// Функция освобождения
    pub free: PFREE_FUNCTION,
    /// Элемент в глобальном списке lookaside lists
    pub list_entry: LIST_ENTRY,
    /// Последний счетчик для tuning
    pub last_total_allocates: ULONG,
    /// Последние промахи
    pub last_allocate_misses: ULONG,
    /// Padding
    _padding: [u8; 16],
}

impl NPAGED_LOOKASIDE_LIST {
    pub const fn new() -> Self {
        Self {
            list_head: SLIST_HEADER::new(),
            depth: LOOKASIDE_MINIMUM_DEPTH,
            maximum_depth: LOOKASIDE_MAXIMUM_DEPTH,
            total_allocates: AtomicU32::new(0),
            allocate_misses: AtomicU32::new(0),
            total_frees: AtomicU32::new(0),
            free_misses: AtomicU32::new(0),
            pool_type: POOL_TYPE::NonPagedPool,
            tag: 0,
            size: 0,
            allocate: None,
            free: None,
            list_entry: LIST_ENTRY::new(),
            last_total_allocates: 0,
            last_allocate_misses: 0,
            _padding: [0; 16],
        }
    }
}

impl Default for NPAGED_LOOKASIDE_LIST {
    fn default() -> Self {
        Self::new()
    }
}

/// ExInitializeNPagedLookasideList - инициализирует nonpaged lookaside list
pub fn ex_initialize_npaged_lookaside_list(
    lookaside: &mut NPAGED_LOOKASIDE_LIST,
    allocate: PALLOCATE_FUNCTION,
    free: PFREE_FUNCTION,
    flags: ULONG,
    size: usize,
    tag: POOL_TAG,
    depth: USHORT,
) {
    let _ = flags; // Reserved

    ex_initialize_slist_head(&mut lookaside.list_head);

    lookaside.depth = if depth == 0 {
        LOOKASIDE_MINIMUM_DEPTH
    } else {
        depth.min(LOOKASIDE_MAXIMUM_DEPTH)
    };
    lookaside.maximum_depth = LOOKASIDE_MAXIMUM_DEPTH;
    lookaside.pool_type = POOL_TYPE::NonPagedPool;
    lookaside.tag = tag;
    lookaside.size = size as ULONG;
    lookaside.allocate = allocate;
    lookaside.free = free;

    lookaside.total_allocates.store(0, Ordering::Release);
    lookaside.allocate_misses.store(0, Ordering::Release);
    lookaside.total_frees.store(0, Ordering::Release);
    lookaside.free_misses.store(0, Ordering::Release);
}

/// ExDeleteNPagedLookasideList - удаляет nonpaged lookaside list
pub fn ex_delete_npaged_lookaside_list(lookaside: &mut NPAGED_LOOKASIDE_LIST) {
    // Освобождаем все кэшированные блоки
    loop {
        let entry = ex_interlock_pop_entry_slist(&lookaside.list_head);
        if entry.is_null() {
            break;
        }

        // Освобождаем блок
        if let Some(free_fn) = lookaside.free {
            free_fn(entry as PVOID);
        } else {
            ex_free_pool_with_tag(entry as PVOID, lookaside.tag);
        }
    }
}

/// ExAllocateFromNPagedLookasideList - выделяет блок из lookaside list
pub fn ex_allocate_from_npaged_lookaside_list(lookaside: &NPAGED_LOOKASIDE_LIST) -> PVOID {
    lookaside.total_allocates.fetch_add(1, Ordering::Relaxed);

    // Пробуем взять из кэша
    let entry = ex_interlock_pop_entry_slist(&lookaside.list_head);
    if !entry.is_null() {
        return entry as PVOID;
    }

    // Cache miss - выделяем из пула
    lookaside.allocate_misses.fetch_add(1, Ordering::Relaxed);

    if let Some(alloc_fn) = lookaside.allocate {
        alloc_fn(lookaside.pool_type, lookaside.size as usize, lookaside.tag)
    } else {
        ex_allocate_pool_with_tag(lookaside.pool_type, lookaside.size as usize, lookaside.tag)
    }
}

/// ExFreeToNPagedLookasideList - освобождает блок в lookaside list
///
/// # Safety
/// entry должен быть выделен через ExAllocateFromNPagedLookasideList
pub unsafe fn ex_free_to_npaged_lookaside_list(lookaside: &NPAGED_LOOKASIDE_LIST, entry: PVOID) {
    lookaside.total_frees.fetch_add(1, Ordering::Relaxed);

    // Проверяем глубину
    if lookaside.list_head.depth() < lookaside.depth {
        // Есть место в кэше
        unsafe {
            ex_interlock_push_entry_slist(&lookaside.list_head, entry as *mut SLIST_ENTRY);
        }
    } else {
        // Кэш полон - освобождаем в пул
        lookaside.free_misses.fetch_add(1, Ordering::Relaxed);

        if let Some(free_fn) = lookaside.free {
            free_fn(entry);
        } else {
            ex_free_pool_with_tag(entry, lookaside.tag);
        }
    }
}

// =============================================================================
// PAGED_LOOKASIDE_LIST - Paged lookaside list
// =============================================================================

/// PAGED_LOOKASIDE_LIST - lookaside для PagedPool
#[repr(C)]
pub struct PAGED_LOOKASIDE_LIST {
    /// Lock-free список (используем SLIST даже для paged)
    pub list_head: SLIST_HEADER,
    /// Глубина
    pub depth: USHORT,
    /// Максимальная глубина
    pub maximum_depth: USHORT,
    /// Статистика
    pub total_allocates: AtomicU32,
    pub allocate_misses: AtomicU32,
    pub total_frees: AtomicU32,
    pub free_misses: AtomicU32,
    /// Тип пула
    pub pool_type: POOL_TYPE,
    /// Tag
    pub tag: POOL_TAG,
    /// Размер
    pub size: ULONG,
    /// Функции
    pub allocate: PALLOCATE_FUNCTION,
    pub free: PFREE_FUNCTION,
    /// List entry
    pub list_entry: LIST_ENTRY,
    /// Tuning
    pub last_total_allocates: ULONG,
    pub last_allocate_misses: ULONG,
    /// Padding
    _padding: [u8; 16],
}

impl PAGED_LOOKASIDE_LIST {
    pub const fn new() -> Self {
        Self {
            list_head: SLIST_HEADER::new(),
            depth: LOOKASIDE_MINIMUM_DEPTH,
            maximum_depth: LOOKASIDE_MAXIMUM_DEPTH,
            total_allocates: AtomicU32::new(0),
            allocate_misses: AtomicU32::new(0),
            total_frees: AtomicU32::new(0),
            free_misses: AtomicU32::new(0),
            pool_type: POOL_TYPE::PagedPool,
            tag: 0,
            size: 0,
            allocate: None,
            free: None,
            list_entry: LIST_ENTRY::new(),
            last_total_allocates: 0,
            last_allocate_misses: 0,
            _padding: [0; 16],
        }
    }
}

impl Default for PAGED_LOOKASIDE_LIST {
    fn default() -> Self {
        Self::new()
    }
}

/// ExInitializePagedLookasideList - инициализирует paged lookaside list
pub fn ex_initialize_paged_lookaside_list(
    lookaside: &mut PAGED_LOOKASIDE_LIST,
    allocate: PALLOCATE_FUNCTION,
    free: PFREE_FUNCTION,
    flags: ULONG,
    size: usize,
    tag: POOL_TAG,
    depth: USHORT,
) {
    let _ = flags;

    ex_initialize_slist_head(&mut lookaside.list_head);

    lookaside.depth = if depth == 0 {
        LOOKASIDE_MINIMUM_DEPTH
    } else {
        depth.min(LOOKASIDE_MAXIMUM_DEPTH)
    };
    lookaside.maximum_depth = LOOKASIDE_MAXIMUM_DEPTH;
    lookaside.pool_type = POOL_TYPE::PagedPool;
    lookaside.tag = tag;
    lookaside.size = size as ULONG;
    lookaside.allocate = allocate;
    lookaside.free = free;

    lookaside.total_allocates.store(0, Ordering::Release);
    lookaside.allocate_misses.store(0, Ordering::Release);
    lookaside.total_frees.store(0, Ordering::Release);
    lookaside.free_misses.store(0, Ordering::Release);
}

/// ExDeletePagedLookasideList - удаляет paged lookaside list
pub fn ex_delete_paged_lookaside_list(lookaside: &mut PAGED_LOOKASIDE_LIST) {
    loop {
        let entry = ex_interlock_pop_entry_slist(&lookaside.list_head);
        if entry.is_null() {
            break;
        }

        if let Some(free_fn) = lookaside.free {
            free_fn(entry as PVOID);
        } else {
            ex_free_pool_with_tag(entry as PVOID, lookaside.tag);
        }
    }
}

/// ExAllocateFromPagedLookasideList
pub fn ex_allocate_from_paged_lookaside_list(lookaside: &PAGED_LOOKASIDE_LIST) -> PVOID {
    lookaside.total_allocates.fetch_add(1, Ordering::Relaxed);

    let entry = ex_interlock_pop_entry_slist(&lookaside.list_head);
    if !entry.is_null() {
        return entry as PVOID;
    }

    lookaside.allocate_misses.fetch_add(1, Ordering::Relaxed);

    if let Some(alloc_fn) = lookaside.allocate {
        alloc_fn(lookaside.pool_type, lookaside.size as usize, lookaside.tag)
    } else {
        ex_allocate_pool_with_tag(lookaside.pool_type, lookaside.size as usize, lookaside.tag)
    }
}

/// ExFreeToPagedLookasideList
///
/// # Safety
/// entry должен быть выделен через ExAllocateFromPagedLookasideList
pub unsafe fn ex_free_to_paged_lookaside_list(lookaside: &PAGED_LOOKASIDE_LIST, entry: PVOID) {
    lookaside.total_frees.fetch_add(1, Ordering::Relaxed);

    if lookaside.list_head.depth() < lookaside.depth {
        unsafe {
            ex_interlock_push_entry_slist(&lookaside.list_head, entry as *mut SLIST_ENTRY);
        }
    } else {
        lookaside.free_misses.fetch_add(1, Ordering::Relaxed);

        if let Some(free_fn) = lookaside.free {
            free_fn(entry);
        } else {
            ex_free_pool_with_tag(entry, lookaside.tag);
        }
    }
}
