//! Working Set Manager
//!
//! Управление working set процессов — набором страниц, находящихся
//! в физической памяти для данного процесса.
//!
//! # Архитектура
//!
//! ```text
//! EPROCESS
//!     |
//!     +---> MMSUPPORT (Vm field)
//!               |
//!               +---> MMWSL (Working Set List)
//!                        |
//!                        +---> MMWSLE[] (Working Set List Entries)
//! ```
//!
//! # Working Set Limits
//!
//! - **MinimumWorkingSetSize** — минимальный гарантированный WS
//! - **MaximumWorkingSetSize** — максимальный размер до trimming
//!
//! # Trimming
//!
//! При нехватке памяти Balance Set Manager выбирает процессы
//! для trimming и переводит их страницы в Standby/Modified lists.
//!
//! Источники:
//! - ReactOS: mm/ARM3/wslist.c, mm/ARM3/balance.c
//! - NT6.1: mm/wslist.c, mm/balance.c

use core::ptr;
use core::sync::atomic::AtomicBool;
use core::sync::atomic::AtomicU32;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;

use super::pfn::*;
use super::pte::*;
use crate::ke::spinlock::KSPIN_LOCK;
use crate::nt::LIST_ENTRY;
use crate::nt::NTSTATUS;
use crate::nt::PVOID;
use crate::nt::STATUS_SUCCESS;

// =============================================================================
// Working Set Constants
// =============================================================================

/// Минимальный working set по умолчанию (в страницах)
pub const MM_MINIMUM_WORKING_SET_DEFAULT: usize = 50;

/// Максимальный working set по умолчанию (в страницах)  
pub const MM_MAXIMUM_WORKING_SET_DEFAULT: usize = 345;

/// Soft working set maximum
pub const MM_SOFT_MAXIMUM_WORKING_SET: usize = 0x7FFFFFFF;

/// Порог низкой памяти (в страницах)
pub const MM_LOW_MEMORY_THRESHOLD: usize = 256;

/// Порог критически низкой памяти
pub const MM_VERY_LOW_MEMORY_THRESHOLD: usize = 64;

/// Максимальное количество страниц для trim за один раз
pub const MM_MAX_TRIM_PAGES: usize = 64;

// =============================================================================
// MMWSLE — Working Set List Entry
// =============================================================================

/// Working Set List Entry
///
/// Каждая запись описывает одну страницу в working set.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MMWSLE {
    /// Виртуальный адрес страницы (верхние биты) + флаги (нижние биты)
    pub u1: MMWSLE_U1,
}

/// Union для MMWSLE
#[repr(C)]
#[derive(Clone, Copy)]
pub union MMWSLE_U1 {
    /// Виртуальный адрес (когда entry active)
    pub virtual_address: u64,
    /// Свободный индекс (когда entry free)
    pub next_free_index: u64,
    /// Packed value для прямого доступа
    pub value: u64,
}

impl MMWSLE {
    pub const fn new() -> Self {
        Self {
            u1: MMWSLE_U1 { value: 0 },
        }
    }

    /// Возвращает виртуальный адрес (очищенный от флагов)
    #[inline]
    pub fn virtual_address(&self) -> u64 {
        unsafe { self.u1.value & !0xFFF }
    }

    /// Устанавливает виртуальный адрес
    #[inline]
    pub fn set_virtual_address(&mut self, va: u64) {
        unsafe {
            let flags = self.u1.value & 0xFFF;
            self.u1.value = (va & !0xFFF) | flags;
        }
    }

    /// Возвращает флаги
    #[inline]
    pub fn flags(&self) -> u16 {
        unsafe { (self.u1.value & 0xFFF) as u16 }
    }

    /// Проверяет, active ли entry
    #[inline]
    pub fn is_active(&self) -> bool {
        unsafe { (self.u1.value & WSLE_VALID) != 0 }
    }

    /// Устанавливает valid flag
    #[inline]
    pub fn set_valid(&mut self, valid: bool) {
        unsafe {
            if valid {
                self.u1.value |= WSLE_VALID;
            } else {
                self.u1.value &= !WSLE_VALID;
            }
        }
    }
}

/// WSLE flags
pub const WSLE_VALID: u64 = 1 << 0;
pub const WSLE_LOCKED: u64 = 1 << 1;
pub const WSLE_DIRECT: u64 = 1 << 2;
pub const WSLE_PROTECTION_MASK: u64 = 0x1F << 3;

// =============================================================================
// MMWSL — Working Set List
// =============================================================================

/// Working Set List
///
/// Массив MMWSLE entries для процесса.
#[repr(C)]
pub struct MMWSL {
    /// Первый свободный индекс
    pub first_free: u32,
    /// Первый динамический индекс (после locked entries)
    pub first_dynamic: u32,
    /// Последний инициализированный индекс
    pub last_initialized_wsle: u32,
    /// Следующий доступный слот
    pub next_slot: u32,
    /// Количество entries в списке
    pub wsle_count: u32,
    /// Padding
    _reserved: u32,
    /// Указатель на массив WSLE
    pub wsle: *mut MMWSLE,
    /// Размер массива wsle
    pub wsle_size: usize,
    /// Hash table для быстрого поиска
    pub hash_table: *mut MMWSLE_HASH,
    /// Размер hash table
    pub hash_table_size: usize,
}

/// Hash table entry для быстрого поиска VA -> WSLE index
#[repr(C)]
pub struct MMWSLE_HASH {
    pub key: u64,   // Virtual address
    pub index: u32, // Index in WSLE array
    _reserved: u32,
}

impl MMWSL {
    pub const fn new() -> Self {
        Self {
            first_free: 0,
            first_dynamic: 0,
            last_initialized_wsle: 0,
            next_slot: 0,
            wsle_count: 0,
            _reserved: 0,
            wsle: ptr::null_mut(),
            wsle_size: 0,
            hash_table: ptr::null_mut(),
            hash_table_size: 0,
        }
    }
}

// =============================================================================
// MMSUPPORT — Working Set Support Structure
// =============================================================================

/// MMSUPPORT — основная структура working set для процесса
///
/// Хранится в EPROCESS (поле Vm) и описывает working set процесса.
#[repr(C)]
pub struct MMSUPPORT {
    /// Working set mutex (push lock)
    pub working_set_mutex: u64, // EX_PUSH_LOCK

    /// Working set list
    pub vm_working_set_list: *mut MMWSL,

    /// Время последнего trim
    pub last_trim_time: u64,

    /// Флаги
    pub flags: MmsupportFlags,

    /// Текущий размер working set (в страницах)
    pub working_set_size: AtomicUsize,

    /// Пиковый размер working set
    pub peak_working_set_size: AtomicUsize,

    /// Минимальный working set
    pub minimum_working_set_size: usize,

    /// Максимальный working set
    pub maximum_working_set_size: usize,

    /// Количество private pages
    pub number_of_committed_pages: AtomicUsize,

    /// Количество shared pages
    pub number_of_shared_pages: AtomicUsize,

    /// Количество locked pages
    pub number_of_locked_pages: AtomicUsize,

    /// Количество page faults
    pub page_fault_count: AtomicU32,

    /// Trim счетчик
    pub trim_count: AtomicU32,

    /// Claim (количество страниц для возможного trim)
    pub claim: AtomicUsize,

    /// Следующий estimation slot
    pub next_estimation_slot: u32,

    /// Estimated available
    pub estimated_available: u32,

    /// Working set expansion links
    pub working_set_expansion_links: LIST_ENTRY,
}

/// Флаги MMSUPPORT
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MmsupportFlags {
    value: u32,
}

impl MmsupportFlags {
    pub const fn new() -> Self {
        Self { value: 0 }
    }

    /// Working set инициализирован
    #[inline]
    pub fn initialized(&self) -> bool {
        (self.value & 1) != 0
    }

    pub fn set_initialized(&mut self, v: bool) {
        if v {
            self.value |= 1;
        } else {
            self.value &= !1;
        }
    }

    /// Процесс в expansion list
    #[inline]
    pub fn in_expansion_list(&self) -> bool {
        (self.value & 2) != 0
    }

    pub fn set_in_expansion_list(&mut self, v: bool) {
        if v {
            self.value |= 2;
        } else {
            self.value &= !2;
        }
    }

    /// Trimming в процессе
    #[inline]
    pub fn being_trimmed(&self) -> bool {
        (self.value & 4) != 0
    }

    pub fn set_being_trimmed(&mut self, v: bool) {
        if v {
            self.value |= 4;
        } else {
            self.value &= !4;
        }
    }

    /// Hard enforcement limits
    #[inline]
    pub fn hard_enforcement(&self) -> bool {
        (self.value & 8) != 0
    }

    pub fn set_hard_enforcement(&mut self, v: bool) {
        if v {
            self.value |= 8;
        } else {
            self.value &= !8;
        }
    }
}

impl MMSUPPORT {
    pub const fn new() -> Self {
        Self {
            working_set_mutex: 0,
            vm_working_set_list: ptr::null_mut(),
            last_trim_time: 0,
            flags: MmsupportFlags::new(),
            working_set_size: AtomicUsize::new(0),
            peak_working_set_size: AtomicUsize::new(0),
            minimum_working_set_size: MM_MINIMUM_WORKING_SET_DEFAULT,
            maximum_working_set_size: MM_MAXIMUM_WORKING_SET_DEFAULT,
            number_of_committed_pages: AtomicUsize::new(0),
            number_of_shared_pages: AtomicUsize::new(0),
            number_of_locked_pages: AtomicUsize::new(0),
            page_fault_count: AtomicU32::new(0),
            trim_count: AtomicU32::new(0),
            claim: AtomicUsize::new(0),
            next_estimation_slot: 0,
            estimated_available: 0,
            working_set_expansion_links: LIST_ENTRY::new(),
        }
    }
}

// =============================================================================
// Global Working Set State
// =============================================================================

/// Глобальный список процессов для trimming
static WORKING_SET_EXPANSION_HEAD: crate::ke::init::GlobalData<LIST_ENTRY> =
    crate::ke::init::GlobalData::new(LIST_ENTRY::new());

/// Spinlock для expansion list
static WORKING_SET_EXPANSION_LOCK: crate::ke::init::GlobalData<KSPIN_LOCK> =
    crate::ke::init::GlobalData::new(KSPIN_LOCK::new());

/// Количество resident страниц
static MM_RESIDENT_AVAILABLE_PAGES: AtomicUsize = AtomicUsize::new(0);

/// Committed pages (глобально)
static MM_TOTAL_COMMITTED_PAGES: AtomicUsize = AtomicUsize::new(0);

/// Commit limit
static MM_TOTAL_COMMIT_LIMIT: AtomicUsize = AtomicUsize::new(0);

/// Low memory event signaled
static MM_LOW_MEMORY_SIGNALED: AtomicBool = AtomicBool::new(false);

// =============================================================================
// Working Set Initialization
// =============================================================================

/// Инициализирует глобальные структуры Working Set
pub unsafe fn mi_initialize_working_set_subsystem() {
    unsafe {
        // Инициализируем expansion list
        LIST_ENTRY::init_head(WORKING_SET_EXPANSION_HEAD.get());
    }
}

/// Инициализирует working set для процесса
pub unsafe fn mi_initialize_working_set(
    mmsupport: *mut MMSUPPORT,
    min_ws: usize,
    max_ws: usize,
) -> NTSTATUS {
    unsafe {
        if mmsupport.is_null() {
            return crate::nt::STATUS_INVALID_PARAMETER;
        }

        // Устанавливаем лимиты
        (*mmsupport).minimum_working_set_size = min_ws.max(MM_MINIMUM_WORKING_SET_DEFAULT);
        (*mmsupport).maximum_working_set_size = max_ws.max(min_ws);

        // Выделяем MMWSL
        let wsl = crate::ex::pool::ex_allocate_pool_with_tag(
            crate::ex::pool::POOL_TYPE::NonPagedPool,
            core::mem::size_of::<MMWSL>(),
            u32::from_le_bytes(*b"MmWs"),
        ) as *mut MMWSL;

        if wsl.is_null() {
            return crate::nt::STATUS_NO_MEMORY;
        }

        core::ptr::write(wsl, MMWSL::new());

        // Выделяем начальный массив WSLE
        let initial_wsle_count = min_ws * 2;
        let wsle_size = initial_wsle_count * core::mem::size_of::<MMWSLE>();

        let wsle = crate::ex::pool::ex_allocate_pool_with_tag(
            crate::ex::pool::POOL_TYPE::NonPagedPool,
            wsle_size,
            u32::from_le_bytes(*b"MmWe"),
        ) as *mut MMWSLE;

        if wsle.is_null() {
            crate::ex::pool::ex_free_pool_with_tag(wsl as PVOID, u32::from_le_bytes(*b"MmWs"));
            return crate::nt::STATUS_NO_MEMORY;
        }

        // Инициализируем WSLE как free list
        for i in 0..initial_wsle_count {
            let entry = wsle.add(i);
            (*entry) = MMWSLE::new();
            (*entry).u1.next_free_index = (i + 1) as u64;
        }
        // Последний entry указывает на конец
        (*wsle.add(initial_wsle_count - 1)).u1.next_free_index = u64::MAX;

        (*wsl).wsle = wsle;
        (*wsl).wsle_size = initial_wsle_count;
        (*wsl).first_free = 0;
        (*wsl).last_initialized_wsle = initial_wsle_count as u32 - 1;

        (*mmsupport).vm_working_set_list = wsl;
        (*mmsupport).flags.set_initialized(true);

        STATUS_SUCCESS
    }
}

/// Освобождает working set
pub unsafe fn mi_delete_working_set(mmsupport: *mut MMSUPPORT) {
    unsafe {
        if mmsupport.is_null() {
            return;
        }

        let wsl = (*mmsupport).vm_working_set_list;
        if !wsl.is_null() {
            // Освобождаем WSLE массив
            let wsle = (*wsl).wsle;
            if !wsle.is_null() {
                crate::ex::pool::ex_free_pool_with_tag(wsle as PVOID, u32::from_le_bytes(*b"MmWe"));
            }

            // Освобождаем hash table если есть
            let hash = (*wsl).hash_table;
            if !hash.is_null() {
                crate::ex::pool::ex_free_pool_with_tag(hash as PVOID, u32::from_le_bytes(*b"MmWh"));
            }

            // Освобождаем MMWSL
            crate::ex::pool::ex_free_pool_with_tag(wsl as PVOID, u32::from_le_bytes(*b"MmWs"));
        }

        (*mmsupport).vm_working_set_list = ptr::null_mut();
        (*mmsupport).flags.set_initialized(false);
    }
}

// =============================================================================
// Working Set Operations
// =============================================================================

/// Добавляет страницу в working set
pub unsafe fn mi_add_page_to_working_set(
    mmsupport: *mut MMSUPPORT,
    virtual_address: u64,
    _pfn: usize,
) -> bool {
    unsafe {
        if mmsupport.is_null() || (*mmsupport).vm_working_set_list.is_null() {
            return false;
        }

        let wsl = (*mmsupport).vm_working_set_list;

        // Проверяем, есть ли свободные slots
        if (*wsl).first_free == u32::MAX {
            // Нужно расширить WSLE массив или trim
            if !mi_expand_working_set_list(wsl) {
                return false;
            }
        }

        // Получаем свободный slot
        let index = (*wsl).first_free;
        let entry = (*wsl).wsle.add(index as usize);

        // Обновляем first_free
        (*wsl).first_free = (*entry).u1.next_free_index as u32;

        // Заполняем entry
        (*entry).set_virtual_address(virtual_address);
        (*entry).set_valid(true);

        // Обновляем счетчики
        (*wsl).wsle_count += 1;
        (*mmsupport)
            .working_set_size
            .fetch_add(1, Ordering::Relaxed);

        // Обновляем peak
        let current = (*mmsupport).working_set_size.load(Ordering::Relaxed);
        let peak = (*mmsupport).peak_working_set_size.load(Ordering::Relaxed);
        if current > peak {
            (*mmsupport)
                .peak_working_set_size
                .store(current, Ordering::Relaxed);
        }

        true
    }
}

/// Удаляет страницу из working set
pub unsafe fn mi_remove_page_from_working_set(
    mmsupport: *mut MMSUPPORT,
    virtual_address: u64,
) -> bool {
    unsafe {
        if mmsupport.is_null() || (*mmsupport).vm_working_set_list.is_null() {
            return false;
        }

        let wsl = (*mmsupport).vm_working_set_list;

        // Ищем entry по virtual address
        // TODO: использовать hash table для быстрого поиска
        for i in 0..(*wsl).wsle_size {
            let entry = (*wsl).wsle.add(i);
            if (*entry).is_active() && (*entry).virtual_address() == (virtual_address & !0xFFF) {
                // Нашли — удаляем
                (*entry).set_valid(false);
                (*entry).u1.next_free_index = (*wsl).first_free as u64;
                (*wsl).first_free = i as u32;
                (*wsl).wsle_count -= 1;
                (*mmsupport)
                    .working_set_size
                    .fetch_sub(1, Ordering::Relaxed);
                return true;
            }
        }

        false
    }
}

/// Расширяет WSLE массив
unsafe fn mi_expand_working_set_list(wsl: *mut MMWSL) -> bool {
    unsafe {
        let old_size = (*wsl).wsle_size;
        let new_size = old_size * 2;

        // Выделяем новый массив
        let new_wsle = crate::ex::pool::ex_allocate_pool_with_tag(
            crate::ex::pool::POOL_TYPE::NonPagedPool,
            new_size * core::mem::size_of::<MMWSLE>(),
            u32::from_le_bytes(*b"MmWe"),
        ) as *mut MMWSLE;

        if new_wsle.is_null() {
            return false;
        }

        // Копируем старые entries
        core::ptr::copy_nonoverlapping((*wsl).wsle, new_wsle, old_size);

        // Инициализируем новые entries как free list
        for i in old_size..new_size {
            let entry = new_wsle.add(i);
            (*entry) = MMWSLE::new();
            (*entry).u1.next_free_index = (i + 1) as u64;
        }
        (*new_wsle.add(new_size - 1)).u1.next_free_index = (*wsl).first_free as u64;
        (*wsl).first_free = old_size as u32;

        // Освобождаем старый массив
        crate::ex::pool::ex_free_pool_with_tag((*wsl).wsle as PVOID, u32::from_le_bytes(*b"MmWe"));

        (*wsl).wsle = new_wsle;
        (*wsl).wsle_size = new_size;
        (*wsl).last_initialized_wsle = new_size as u32 - 1;

        true
    }
}

// =============================================================================
// Trimming
// =============================================================================

/// Выполняет trim working set для освобождения страниц
pub unsafe fn mi_trim_working_set(mmsupport: *mut MMSUPPORT, pages_to_trim: usize) -> usize {
    unsafe {
        if mmsupport.is_null() || (*mmsupport).vm_working_set_list.is_null() {
            return 0;
        }

        let wsl = (*mmsupport).vm_working_set_list;
        let mut trimmed = 0;
        let max_trim = pages_to_trim.min(MM_MAX_TRIM_PAGES);

        (*mmsupport).flags.set_being_trimmed(true);

        // Простой алгоритм: проходим по WSLE и удаляем unlocked страницы
        for i in 0..(*wsl).wsle_size {
            if trimmed >= max_trim {
                break;
            }

            let entry = (*wsl).wsle.add(i);

            // Пропускаем неактивные и locked entries
            if !(*entry).is_active() {
                continue;
            }
            if ((*entry).flags() as u64 & WSLE_LOCKED) != 0 {
                continue;
            }

            let va = (*entry).virtual_address();

            // Получаем PTE и PFN
            let pte_ptr = mm_get_pte_address(va) as *mut u64;
            let pte_value = core::ptr::read_volatile(pte_ptr);

            if (pte_value & PTE_VALID) == 0 {
                // Страница уже не в памяти — просто удаляем из WS
                (*entry).set_valid(false);
                (*entry).u1.next_free_index = (*wsl).first_free as u64;
                (*wsl).first_free = i as u32;
                (*wsl).wsle_count -= 1;
                continue;
            }

            let pfn = ((pte_value >> 12) & 0xFFFFFFFFFF) as usize;

            // Проверяем dirty bit
            let is_dirty = (pte_value & PTE_DIRTY) != 0;

            // Переводим страницу в transition
            let protection = (pte_value >> 1) & 0x1F; // Извлекаем protection bits
            let transition_pte =
                (pfn as u64) << 12 | super::pagefault::pte_state::TRANSITION | (protection << 5);

            core::ptr::write_volatile(pte_ptr, transition_pte);
            crate::arch::x86_64::cpu::invlpg(va);

            // Добавляем PFN в соответствующий список
            if is_dirty {
                // Modified list
                mi_insert_page_in_list(&mut *super::pfn::MM_MODIFIED_PAGE_LIST_HEAD.get(), pfn);
                super::pfn::MM_MODIFIED_PAGE_COUNT.fetch_add(1, Ordering::Relaxed);
            } else {
                // Standby list
                mi_insert_page_in_list(&mut *super::pfn::MM_STANDBY_PAGE_LIST_HEAD.get(), pfn);
                super::pfn::MM_STANDBY_PAGE_COUNT.fetch_add(1, Ordering::Relaxed);
            }

            // Удаляем из working set
            (*entry).set_valid(false);
            (*entry).u1.next_free_index = (*wsl).first_free as u64;
            (*wsl).first_free = i as u32;
            (*wsl).wsle_count -= 1;

            trimmed += 1;
        }

        (*mmsupport)
            .working_set_size
            .fetch_sub(trimmed, Ordering::Relaxed);
        (*mmsupport)
            .trim_count
            .fetch_add(trimmed as u32, Ordering::Relaxed);
        (*mmsupport).flags.set_being_trimmed(false);

        trimmed
    }
}

// =============================================================================
// Balance Set Manager
// =============================================================================

/// Balance set manager state
static BALANCE_MANAGER_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Запускает balance set manager iteration
///
/// Вызывается периодически или при нехватке памяти.
pub unsafe fn mi_balance_set_manager_iteration() {
    unsafe {
        if BALANCE_MANAGER_ACTIVE.swap(true, Ordering::AcqRel) {
            // Уже запущен
            return;
        }

        let available = super::pfn::mm_get_available_pages();

        // Проверяем пороги памяти
        if available < MM_VERY_LOW_MEMORY_THRESHOLD {
            // Критически низкая память — агрессивный trim
            mi_trim_all_working_sets(MM_MAX_TRIM_PAGES * 4);
            MM_LOW_MEMORY_SIGNALED.store(true, Ordering::Release);
        } else if available < MM_LOW_MEMORY_THRESHOLD {
            // Низкая память — умеренный trim
            mi_trim_all_working_sets(MM_MAX_TRIM_PAGES);
            MM_LOW_MEMORY_SIGNALED.store(true, Ordering::Release);
        } else {
            MM_LOW_MEMORY_SIGNALED.store(false, Ordering::Release);
        }

        BALANCE_MANAGER_ACTIVE.store(false, Ordering::Release);
    }
}

/// Выполняет trim для всех процессов в expansion list
unsafe fn mi_trim_all_working_sets(total_pages: usize) {
    unsafe {
        let list_head = WORKING_SET_EXPANSION_HEAD.get();

        if LIST_ENTRY::is_empty(list_head) {
            return;
        }

        let mut remaining = total_pages;
        let mut current = (*list_head).flink;

        while current != list_head && remaining > 0 {
            // Получаем MMSUPPORT из list entry
            let mmsupport = (current as usize
                - core::mem::offset_of!(MMSUPPORT, working_set_expansion_links))
                as *mut MMSUPPORT;

            let ws_size = (*mmsupport).working_set_size.load(Ordering::Relaxed);
            let min_ws = (*mmsupport).minimum_working_set_size;

            // Вычисляем, сколько можно trim
            let can_trim = ws_size.saturating_sub(min_ws);
            let to_trim = can_trim.min(remaining).min(MM_MAX_TRIM_PAGES);

            if to_trim > 0 {
                let trimmed = mi_trim_working_set(mmsupport, to_trim);
                remaining = remaining.saturating_sub(trimmed);
                // Available pages увеличиваются автоматически при добавлении в standby/modified
            }

            current = (*current).flink;
        }
    }
}

// =============================================================================
// Query Functions
// =============================================================================

/// Проверяет, signaled ли low memory
#[inline]
pub fn mm_is_low_memory() -> bool {
    MM_LOW_MEMORY_SIGNALED.load(Ordering::Relaxed)
}

/// Возвращает commit limit
#[inline]
pub fn mm_get_commit_limit() -> usize {
    MM_TOTAL_COMMIT_LIMIT.load(Ordering::Relaxed)
}

/// Возвращает committed pages
#[inline]
pub fn mm_get_committed_pages() -> usize {
    MM_TOTAL_COMMITTED_PAGES.load(Ordering::Relaxed)
}
