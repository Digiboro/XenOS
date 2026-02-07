//! Page Frame Number Database
//!
//! База данных физических страниц (PFN database).
//!
//! # Архитектура
//!
//! PFN database — глобальный массив структур `MMPFN`, индексированный по PFN.
//! Каждая физическая страница имеет соответствующую запись в базе.
//!
//! # Списки страниц
//!
//! Страницы организованы в двусвязные списки по состоянию:
//!
//! | Список            | Описание                                    |
//! |-------------------|---------------------------------------------|
//! | ZeroedPageList    | Обнуленные страницы (готовы к выдаче)       |
//! | FreePageList      | Свободные страницы (не обнулены)            |
//! | StandbyPageList   | Standby — могут быть переиспользованы       |
//! | ModifiedPageList  | Модифицированные — ждут записи на диск      |
//! | BadPageList       | Плохие страницы (не использовать)           |
//!
//! # Политика аллокации
//!
//! 1. Сначала zeroed list (страницы уже обнулены)
//! 2. Затем free list (нужно обнулить перед выдачей)
//! 3. Затем standby list (перевести в free и выдать)
//!
//! # Синхронизация
//!
//! Доступ к PFN database защищён `MM_PFN_LOCK` (спинлок).
//! Операции с PFN должны выполняться на IRQL >= DISPATCH_LEVEL.
//!
//! Источники:
//! - ReactOS: mm/ARM3/pfnlist.c
//! - NT6.1: mm/pfnlist.c

use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;

use super::types::*;
use crate::ke::spinlock::KSPIN_LOCK;

// =============================================================================
// PFN List Types
// =============================================================================

/// Типы списков PFN
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MMLISTS {
    /// Обнуленные страницы (готовы к использованию)
    ZeroedPageList = 0,
    /// Свободные страницы (не обнулены)
    FreePageList = 1,
    /// Standby - могут быть повторно использованы
    StandbyPageList = 2,
    /// Модифицированные - нужно записать на диск
    ModifiedPageList = 3,
    /// Модифицированные, не записывать
    ModifiedNoWritePageList = 4,
    /// Плохие страницы
    BadPageList = 5,
    /// Активная страница
    ActiveAndValid = 6,
    /// Страница в переходном состоянии
    TransitionPage = 7,
}

/// Sentinel value для конца списка
pub const LIST_HEAD: usize = usize::MAX;

// =============================================================================
// MMPFNLIST - Заголовок списка PFN
// =============================================================================

/// Заголовок списка страниц
#[repr(C)]
pub struct MMPFNLIST {
    /// Количество страниц в списке
    pub total: AtomicUsize,
    /// Тип списка
    pub list_name: MMLISTS,
    /// Первый элемент (PFN)
    pub flink: AtomicUsize,
    /// Последний элемент (PFN)
    pub blink: AtomicUsize,
}

impl MMPFNLIST {
    pub const fn new(list_name: MMLISTS) -> Self {
        Self {
            total: AtomicUsize::new(0),
            list_name,
            flink: AtomicUsize::new(LIST_HEAD),
            blink: AtomicUsize::new(LIST_HEAD),
        }
    }

    /// Проверяет, пуст ли список
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.total.load(Ordering::Acquire) == 0
    }

    /// Возвращает количество страниц
    #[inline]
    pub fn count(&self) -> usize {
        self.total.load(Ordering::Acquire)
    }
}

// =============================================================================
// MMPFN - Page Frame Number Entry
// =============================================================================

/// Флаги PFN entry
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PFN_FLAGS {
    /// Страница в процессе записи
    WriteInProgress = 0x01,
    /// Страница в процессе чтения
    ReadInProgress = 0x02,
    /// Страница модифицирована
    Modified = 0x04,
    /// Парентная страница
    ParityError = 0x08,
    /// ROM страница
    Rom = 0x10,
    /// Prototype PTE
    PrototypePte = 0x20,
}

/// Entry для одной физической страницы
#[repr(C)]
pub struct MMPFN {
    /// Forward link (следующий PFN в списке)
    pub flink: usize,
    /// Backward link (предыдущий PFN в списке)
    pub blink: usize,
    /// PTE, которая ссылается на эту страницу
    pub pte_address: *mut u64,
    /// Счетчик ссылок
    pub reference_count: u16,
    /// Флаги страницы
    pub flags: u8,
    /// Позиция в списке (MMLISTS)
    pub page_location: u8,
    /// Оригинальный PTE
    pub original_pte: u64,
    /// Share count
    pub share_count: u32,
    /// Color для NUMA/cache coloring
    pub color: u16,
    /// Padding
    pub _reserved: u16,
}

impl MMPFN {
    pub const fn new() -> Self {
        Self {
            flink: LIST_HEAD,
            blink: LIST_HEAD,
            pte_address: core::ptr::null_mut(),
            reference_count: 0,
            flags: 0,
            page_location: MMLISTS::FreePageList as u8,
            original_pte: 0,
            share_count: 0,
            color: 0,
            _reserved: 0,
        }
    }

    /// Проверяет, находится ли страница в свободном списке
    #[inline]
    pub fn is_free(&self) -> bool {
        self.page_location == MMLISTS::FreePageList as u8
            || self.page_location == MMLISTS::ZeroedPageList as u8
    }

    /// Проверяет флаг
    #[inline]
    pub fn has_flag(&self, flag: PFN_FLAGS) -> bool {
        (self.flags & flag as u8) != 0
    }

    /// Устанавливает флаг
    #[inline]
    pub fn set_flag(&mut self, flag: PFN_FLAGS) {
        self.flags |= flag as u8;
    }

    /// Снимает флаг
    #[inline]
    pub fn clear_flag(&mut self, flag: PFN_FLAGS) {
        self.flags &= !(flag as u8);
    }
}

// =============================================================================
// PFN Database
// =============================================================================

/// PFN Database
pub struct PfnDatabase {
    /// Указатель на массив MMPFN
    entries: *mut MMPFN,
    /// Количество entries
    count: usize,
    /// Начальный PFN
    start_pfn: PFN_NUMBER,
    /// Конечный PFN
    end_pfn: PFN_NUMBER,
}

impl PfnDatabase {
    /// Создает пустую PFN database
    pub const fn new() -> Self {
        Self {
            entries: core::ptr::null_mut(),
            count: 0,
            start_pfn: 0,
            end_pfn: 0,
        }
    }

    /// Инициализирует PFN database
    ///
    /// # Safety
    /// Memory region должен быть валидным и достаточного размера
    pub unsafe fn init(&mut self, memory: *mut u8, start_pfn: PFN_NUMBER, end_pfn: PFN_NUMBER) {
        unsafe {
            self.entries = memory as *mut MMPFN;
            self.count = end_pfn - start_pfn;
            self.start_pfn = start_pfn;
            self.end_pfn = end_pfn;

            // Инициализируем все entries
            for i in 0..self.count {
                let entry = self.entries.add(i);
                *entry = MMPFN::new();
            }
        }
    }

    /// Возвращает entry для данного PFN
    #[inline]
    pub fn get(&self, pfn: PFN_NUMBER) -> Option<&MMPFN> {
        if pfn >= self.start_pfn && pfn < self.end_pfn {
            let index = pfn - self.start_pfn;
            unsafe { Some(&*self.entries.add(index)) }
        } else {
            None
        }
    }

    /// Возвращает mutable entry для данного PFN
    #[inline]
    pub fn get_mut(&mut self, pfn: PFN_NUMBER) -> Option<&mut MMPFN> {
        if pfn >= self.start_pfn && pfn < self.end_pfn {
            let index = pfn - self.start_pfn;
            unsafe { Some(&mut *self.entries.add(index)) }
        } else {
            None
        }
    }

    /// Проверяет валидность PFN
    #[inline]
    pub fn is_valid_pfn(&self, pfn: PFN_NUMBER) -> bool {
        pfn >= self.start_pfn && pfn < self.end_pfn
    }

    /// Возвращает количество страниц
    #[inline]
    pub fn page_count(&self) -> usize {
        self.count
    }
}

// =============================================================================
// Global PFN Lists
// =============================================================================

use core::cell::UnsafeCell;

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

/// Список обнуленных страниц
pub static MM_ZEROED_PAGE_LIST_HEAD: SyncUnsafeCell<MMPFNLIST> =
    SyncUnsafeCell::new(MMPFNLIST::new(MMLISTS::ZeroedPageList));

/// Список свободных страниц
pub static MM_FREE_PAGE_LIST_HEAD: SyncUnsafeCell<MMPFNLIST> =
    SyncUnsafeCell::new(MMPFNLIST::new(MMLISTS::FreePageList));

/// Список standby страниц
pub static MM_STANDBY_PAGE_LIST_HEAD: SyncUnsafeCell<MMPFNLIST> =
    SyncUnsafeCell::new(MMPFNLIST::new(MMLISTS::StandbyPageList));

/// Список модифицированных страниц
pub static MM_MODIFIED_PAGE_LIST_HEAD: SyncUnsafeCell<MMPFNLIST> =
    SyncUnsafeCell::new(MMPFNLIST::new(MMLISTS::ModifiedPageList));

/// Список плохих страниц
pub static MM_BAD_PAGE_LIST_HEAD: SyncUnsafeCell<MMPFNLIST> =
    SyncUnsafeCell::new(MMPFNLIST::new(MMLISTS::BadPageList));

/// Глобальная PFN database
pub static MM_PFN_DATABASE: SyncUnsafeCell<PfnDatabase> = SyncUnsafeCell::new(PfnDatabase::new());

/// Счетчик доступных страниц (zeroed + free + standby)
pub static MM_AVAILABLE_PAGES: AtomicUsize = AtomicUsize::new(0);

/// Общее количество физических страниц в системе
pub static MM_NUMBER_OF_PHYSICAL_PAGES: AtomicUsize = AtomicUsize::new(0);

/// Глобальный спинлок для PFN operations
///
/// Все операции с PFN database и списками должны захватывать этот lock.
/// В NT это `MmPfnLock`, доступ через `MI_LOCK_PFN` / `MI_UNLOCK_PFN`.
pub static MM_PFN_LOCK: SyncUnsafeCell<KSPIN_LOCK> = SyncUnsafeCell::new(KSPIN_LOCK::new());

/// Счетчик модифицированных страниц (для modified page writer)
pub static MM_MODIFIED_PAGE_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Счетчик standby страниц
pub static MM_STANDBY_PAGE_COUNT: AtomicUsize = AtomicUsize::new(0);

// =============================================================================
// PFN List Operations
// =============================================================================

/// Вставляет страницу в начало списка
pub unsafe fn mi_insert_page_in_list(list: &mut MMPFNLIST, pfn: PFN_NUMBER) {
    unsafe {
        let db = &mut *MM_PFN_DATABASE.get();

        // Используем raw pointer чтобы избежать проблем с borrow checker
        let entry_ptr = if pfn >= db.start_pfn && pfn < db.end_pfn {
            let index = pfn - db.start_pfn;
            db.entries.add(index)
        } else {
            return;
        };

        let old_head = list.flink.load(Ordering::Acquire);

        (*entry_ptr).flink = old_head;
        (*entry_ptr).blink = LIST_HEAD;
        (*entry_ptr).page_location = list.list_name as u8;

        if old_head != LIST_HEAD {
            if old_head >= db.start_pfn && old_head < db.end_pfn {
                let old_ptr = db.entries.add(old_head - db.start_pfn);
                (*old_ptr).blink = pfn;
            }
        } else {
            list.blink.store(pfn, Ordering::Release);
        }

        list.flink.store(pfn, Ordering::Release);
        list.total.fetch_add(1, Ordering::AcqRel);

        // Увеличиваем счетчик доступных страниц для free/zeroed
        if list.list_name == MMLISTS::FreePageList || list.list_name == MMLISTS::ZeroedPageList {
            MM_AVAILABLE_PAGES.fetch_add(1, Ordering::AcqRel);
        }
    }
}

/// Удаляет страницу из списка
pub unsafe fn mi_remove_page_from_list(list: &mut MMPFNLIST, pfn: PFN_NUMBER) {
    unsafe {
        let db = &mut *MM_PFN_DATABASE.get();

        // Используем raw pointer чтобы избежать проблем с borrow checker
        let entry_ptr = if pfn >= db.start_pfn && pfn < db.end_pfn {
            let index = pfn - db.start_pfn;
            db.entries.add(index)
        } else {
            return;
        };

        let old_flink = (*entry_ptr).flink;
        let old_blink = (*entry_ptr).blink;

        // Update forward link
        if old_flink != LIST_HEAD {
            if old_flink >= db.start_pfn && old_flink < db.end_pfn {
                let next_ptr = db.entries.add(old_flink - db.start_pfn);
                (*next_ptr).blink = old_blink;
            }
        } else {
            list.blink.store(old_blink, Ordering::Release);
        }

        // Update backward link
        if old_blink != LIST_HEAD {
            if old_blink >= db.start_pfn && old_blink < db.end_pfn {
                let prev_ptr = db.entries.add(old_blink - db.start_pfn);
                (*prev_ptr).flink = old_flink;
            }
        } else {
            list.flink.store(old_flink, Ordering::Release);
        }

        (*entry_ptr).flink = LIST_HEAD;
        (*entry_ptr).blink = LIST_HEAD;

        list.total.fetch_sub(1, Ordering::AcqRel);

        // Уменьшаем счетчик доступных страниц
        if list.list_name == MMLISTS::FreePageList || list.list_name == MMLISTS::ZeroedPageList {
            MM_AVAILABLE_PAGES.fetch_sub(1, Ordering::AcqRel);
        }
    }
}

/// Удаляет и возвращает страницу из начала списка
pub unsafe fn mi_remove_any_page(list: &mut MMPFNLIST) -> Option<PFN_NUMBER> {
    unsafe {
        let head = list.flink.load(Ordering::Acquire);

        if head == LIST_HEAD {
            return None;
        }

        mi_remove_page_from_list(list, head);
        Some(head)
    }
}

// =============================================================================
// Page Allocation
// =============================================================================

/// MiRemoveZeroPage
///
/// Удаляет и возвращает страницу из zeroed list.
///
/// # Returns
/// PFN обнулённой страницы или None
pub unsafe fn mi_remove_zero_page() -> Option<PFN_NUMBER> {
    unsafe {
        let zeroed_list = &mut *MM_ZEROED_PAGE_LIST_HEAD.get();
        mi_remove_any_page(zeroed_list)
    }
}

/// MiRemoveFreePage  
///
/// Удаляет и возвращает страницу из free list.
/// Страница НЕ обнулена — caller должен обнулить если нужно.
///
/// # Returns
/// PFN свободной страницы или None
pub unsafe fn mi_remove_free_page() -> Option<PFN_NUMBER> {
    unsafe {
        let free_list = &mut *MM_FREE_PAGE_LIST_HEAD.get();
        mi_remove_any_page(free_list)
    }
}

/// MiAllocatePfn
///
/// Выделяет физическую страницу.
///
/// Политика выбора:
/// 1. Zeroed list (страницы уже обнулены)
/// 2. Free list (нужно обнулить)
/// 3. Standby list (TODO: перевести в free)
///
/// Страница гарантированно обнулена при возврате.
///
/// # Safety
/// Должен вызываться с захваченным PFN lock.
pub unsafe fn mi_allocate_pfn() -> Option<PFN_NUMBER> {
    unsafe {
        // 1. Сначала пробуем zeroed list (уже обнулены)
        if let Some(pfn) = mi_remove_zero_page() {
            // Устанавливаем location = ActiveAndValid
            if let Some(entry) = get_pfn_entry_mut(pfn) {
                entry.page_location = MMLISTS::ActiveAndValid as u8;
                entry.reference_count = 1;
            }
            return Some(pfn);
        }

        // 2. Затем free list (нужно обнулить)
        if let Some(pfn) = mi_remove_free_page() {
            // Обнуляем страницу перед выдачей
            let page_addr = super::init::mm_pfn_to_virtual(pfn);
            core::ptr::write_bytes(page_addr, 0, PAGE_SIZE);

            // Устанавливаем location = ActiveAndValid
            if let Some(entry) = get_pfn_entry_mut(pfn) {
                entry.page_location = MMLISTS::ActiveAndValid as u8;
                entry.reference_count = 1;
            }
            return Some(pfn);
        }

        // 3. TODO: Standby list
        // let standby_list = &mut *MM_STANDBY_PAGE_LIST_HEAD.get();
        // ...

        None
    }
}

/// MiAllocatePfnForZeroPage
///
/// Выделяет страницу специально для zero page thread.
/// Берёт только из free list (для последующего обнуления).
///
/// # Safety
/// Должен вызываться с захваченным PFN lock.
pub unsafe fn mi_allocate_pfn_for_zero_page() -> Option<PFN_NUMBER> {
    unsafe {
        let free_list = &mut *MM_FREE_PAGE_LIST_HEAD.get();
        mi_remove_any_page(free_list)
    }
}

/// MiFreePfn
///
/// Освобождает физическую страницу, добавляя в free list.
///
/// # Arguments
/// * `pfn` - номер страницы для освобождения
///
/// # Safety
/// Должен вызываться с захваченным PFN lock.
/// PFN не должен быть в каком-либо списке.
pub unsafe fn mi_free_pfn(pfn: PFN_NUMBER) {
    unsafe {
        // Обновляем MMPFN entry
        if let Some(entry) = get_pfn_entry_mut(pfn) {
            entry.reference_count = 0;
            entry.share_count = 0;
            entry.pte_address = core::ptr::null_mut();
        }

        // Добавляем в free list
        let free_list = &mut *MM_FREE_PAGE_LIST_HEAD.get();
        mi_insert_page_in_list(free_list, pfn);
    }
}

/// MiInsertZeroedPage
///
/// Добавляет страницу в zeroed list.
/// Используется zero page thread после обнуления.
///
/// # Safety
/// Должен вызываться с захваченным PFN lock.
pub unsafe fn mi_insert_zeroed_page(pfn: PFN_NUMBER) {
    unsafe {
        let zeroed_list = &mut *MM_ZEROED_PAGE_LIST_HEAD.get();
        mi_insert_page_in_list(zeroed_list, pfn);
    }
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Получает mutable ссылку на MMPFN entry для данного PFN
#[inline]
unsafe fn get_pfn_entry_mut(pfn: PFN_NUMBER) -> Option<&'static mut MMPFN> {
    unsafe {
        let db = &mut *MM_PFN_DATABASE.get();
        db.get_mut(pfn)
    }
}

/// Получает ссылку на MMPFN entry для данного PFN
#[inline]
pub unsafe fn get_pfn_entry(pfn: PFN_NUMBER) -> Option<&'static MMPFN> {
    unsafe {
        let db = &*MM_PFN_DATABASE.get();
        db.get(pfn)
    }
}

// =============================================================================
// Query Functions
// =============================================================================

/// Возвращает количество доступных страниц (zeroed + free + standby)
#[inline]
pub fn mm_get_available_pages() -> usize {
    MM_AVAILABLE_PAGES.load(Ordering::Acquire)
}

/// Возвращает общее количество физических страниц в системе
#[inline]
pub fn mm_get_number_of_physical_pages() -> usize {
    MM_NUMBER_OF_PHYSICAL_PAGES.load(Ordering::Acquire)
}

/// Возвращает количество zeroed страниц
#[inline]
pub fn mm_get_zeroed_page_count() -> usize {
    unsafe {
        let zeroed_list = &*MM_ZEROED_PAGE_LIST_HEAD.get();
        zeroed_list.count()
    }
}

/// Возвращает количество free страниц
#[inline]
pub fn mm_get_free_page_count() -> usize {
    unsafe {
        let free_list = &*MM_FREE_PAGE_LIST_HEAD.get();
        free_list.count()
    }
}

/// Возвращает количество modified страниц
#[inline]
pub fn mm_get_modified_page_count() -> usize {
    MM_MODIFIED_PAGE_COUNT.load(Ordering::Acquire)
}

/// Возвращает количество standby страниц
#[inline]
pub fn mm_get_standby_page_count() -> usize {
    MM_STANDBY_PAGE_COUNT.load(Ordering::Acquire)
}

// =============================================================================
// Standby/Modified List Operations
// =============================================================================

/// MiInsertPageInStandbyList
///
/// Добавляет страницу в standby list.
/// Вызывается при trimming clean страниц из working set.
///
/// # Safety
/// Должен вызываться с захваченным PFN lock.
pub unsafe fn mi_insert_page_in_standby_list(pfn: PFN_NUMBER) {
    unsafe {
        // Обновляем PFN entry
        if let Some(entry) = get_pfn_entry_mut(pfn) {
            entry.page_location = MMLISTS::StandbyPageList as u8;
            entry.clear_flag(PFN_FLAGS::Modified);
        }

        // Добавляем в standby list
        let standby_list = &mut *MM_STANDBY_PAGE_LIST_HEAD.get();
        mi_insert_page_in_list(standby_list, pfn);
        MM_STANDBY_PAGE_COUNT.fetch_add(1, Ordering::AcqRel);
    }
}

/// MiInsertPageInModifiedList
///
/// Добавляет страницу в modified list.
/// Вызывается при trimming dirty страниц из working set.
///
/// # Safety
/// Должен вызываться с захваченным PFN lock.
pub unsafe fn mi_insert_page_in_modified_list(pfn: PFN_NUMBER) {
    unsafe {
        // Обновляем PFN entry
        if let Some(entry) = get_pfn_entry_mut(pfn) {
            entry.page_location = MMLISTS::ModifiedPageList as u8;
            entry.set_flag(PFN_FLAGS::Modified);
        }

        // Добавляем в modified list
        let modified_list = &mut *MM_MODIFIED_PAGE_LIST_HEAD.get();
        mi_insert_page_in_list(modified_list, pfn);
        MM_MODIFIED_PAGE_COUNT.fetch_add(1, Ordering::AcqRel);
    }
}

/// MiRemovePageFromStandbyList
///
/// Удаляет страницу из standby list.
/// Вызывается при reclaim (transition fault) или при repurpose.
///
/// # Safety
/// Должен вызываться с захваченным PFN lock.
pub unsafe fn mi_remove_page_from_standby_list(pfn: PFN_NUMBER) {
    unsafe {
        let standby_list = &mut *MM_STANDBY_PAGE_LIST_HEAD.get();
        mi_remove_page_from_list(standby_list, pfn);
        MM_STANDBY_PAGE_COUNT.fetch_sub(1, Ordering::AcqRel);
    }
}

/// MiRemovePageFromModifiedList
///
/// Удаляет страницу из modified list.
/// Вызывается modified page writer после записи на диск.
///
/// # Safety
/// Должен вызываться с захваченным PFN lock.
pub unsafe fn mi_remove_page_from_modified_list(pfn: PFN_NUMBER) {
    unsafe {
        let modified_list = &mut *MM_MODIFIED_PAGE_LIST_HEAD.get();
        mi_remove_page_from_list(modified_list, pfn);
        MM_MODIFIED_PAGE_COUNT.fetch_sub(1, Ordering::AcqRel);
    }
}

/// MiUnlinkPageFromList
///
/// Удаляет страницу из её текущего списка на основе page_location.
///
/// # Safety
/// Должен вызываться с захваченным PFN lock.
pub unsafe fn mi_unlink_page_from_list(pfn: PFN_NUMBER) {
    unsafe {
        let location = {
            let entry = get_pfn_entry(pfn);
            if entry.is_none() {
                return;
            }
            entry.unwrap().page_location
        };

        let list = match location {
            x if x == MMLISTS::ZeroedPageList as u8 => &mut *MM_ZEROED_PAGE_LIST_HEAD.get(),
            x if x == MMLISTS::FreePageList as u8 => &mut *MM_FREE_PAGE_LIST_HEAD.get(),
            x if x == MMLISTS::StandbyPageList as u8 => {
                MM_STANDBY_PAGE_COUNT.fetch_sub(1, Ordering::AcqRel);
                &mut *MM_STANDBY_PAGE_LIST_HEAD.get()
            }
            x if x == MMLISTS::ModifiedPageList as u8 => {
                MM_MODIFIED_PAGE_COUNT.fetch_sub(1, Ordering::AcqRel);
                &mut *MM_MODIFIED_PAGE_LIST_HEAD.get()
            }
            x if x == MMLISTS::BadPageList as u8 => &mut *MM_BAD_PAGE_LIST_HEAD.get(),
            _ => return, // ActiveAndValid или TransitionPage — не в списке
        };

        mi_remove_page_from_list(list, pfn);
    }
}

// =============================================================================
// Standby Reclaim (for allocation)
// =============================================================================

/// MiRemovePageFromStandbyListForAllocation
///
/// Забирает страницу из standby list для выделения.
/// Страница теряет связь с оригинальным содержимым.
///
/// # Returns
/// PFN страницы или None если standby list пуст.
///
/// # Safety
/// Должен вызываться с захваченным PFN lock.
pub unsafe fn mi_repurpose_standby_page() -> Option<PFN_NUMBER> {
    unsafe {
        let standby_list = &mut *MM_STANDBY_PAGE_LIST_HEAD.get();
        let pfn = mi_remove_any_page(standby_list)?;

        MM_STANDBY_PAGE_COUNT.fetch_sub(1, Ordering::AcqRel);

        // Инвалидируем transition PTE, если была
        if let Some(entry) = get_pfn_entry_mut(pfn) {
            if !entry.pte_address.is_null() {
                // Очищаем transition PTE
                core::ptr::write_volatile(entry.pte_address, 0);
                // TLB flush не нужен — страница была standby (not valid)
            }
            entry.page_location = MMLISTS::ActiveAndValid as u8;
            entry.reference_count = 1;
            entry.pte_address = core::ptr::null_mut();
            entry.original_pte = 0;
        }

        Some(pfn)
    }
}

// =============================================================================
// Full Page Allocation with Standby Fallback
// =============================================================================

/// MiAllocatePfnEx
///
/// Расширенное выделение страницы с fallback на standby list.
///
/// Политика:
/// 1. Zeroed list
/// 2. Free list (обнуляем)
/// 3. Standby list (repurpose + обнуляем)
///
/// # Safety
/// Должен вызываться с захваченным PFN lock.
pub unsafe fn mi_allocate_pfn_ex() -> Option<PFN_NUMBER> {
    unsafe {
        // 1. Zeroed list
        if let Some(pfn) = mi_remove_zero_page() {
            if let Some(entry) = get_pfn_entry_mut(pfn) {
                entry.page_location = MMLISTS::ActiveAndValid as u8;
                entry.reference_count = 1;
            }
            return Some(pfn);
        }

        // 2. Free list
        if let Some(pfn) = mi_remove_free_page() {
            // Обнуляем
            let page_addr = super::init::mm_pfn_to_virtual(pfn);
            core::ptr::write_bytes(page_addr, 0, PAGE_SIZE);

            if let Some(entry) = get_pfn_entry_mut(pfn) {
                entry.page_location = MMLISTS::ActiveAndValid as u8;
                entry.reference_count = 1;
            }
            return Some(pfn);
        }

        // 3. Standby list (repurpose)
        if let Some(pfn) = mi_repurpose_standby_page() {
            // Обнуляем (содержимое от другого процесса)
            let page_addr = super::init::mm_pfn_to_virtual(pfn);
            core::ptr::write_bytes(page_addr, 0, PAGE_SIZE);
            return Some(pfn);
        }

        // 4. TODO: Если всё пусто — можно попробовать trimming
        // mi_balance_set_manager_iteration();
        // и повторить

        None
    }
}

// =============================================================================
// Modified Page Writer Support
// =============================================================================

/// Порог для запуска Modified Page Writer
pub const MM_MODIFIED_PAGE_WRITER_THRESHOLD: usize = 64;

/// MiGetModifiedPageForWrite
///
/// Получает страницу из modified list для записи на диск.
///
/// # Returns
/// (PFN, original_pte, pte_address) или None если нет страниц.
///
/// # Safety
/// Должен вызываться с захваченным PFN lock.
pub unsafe fn mi_get_modified_page_for_write() -> Option<(PFN_NUMBER, u64, *mut u64)> {
    unsafe {
        let modified_list = &mut *MM_MODIFIED_PAGE_LIST_HEAD.get();

        let pfn = modified_list.flink.load(Ordering::Acquire);
        if pfn == LIST_HEAD {
            return None;
        }

        let entry = get_pfn_entry(pfn)?;
        let original_pte = entry.original_pte;
        let pte_address = entry.pte_address;

        // Помечаем что запись в прогрессе
        if let Some(entry_mut) = get_pfn_entry_mut(pfn) {
            entry_mut.set_flag(PFN_FLAGS::WriteInProgress);
        }

        Some((pfn, original_pte, pte_address))
    }
}

/// MiCompleteModifiedPageWrite
///
/// Завершает запись страницы. Перемещает из modified в standby.
///
/// # Safety
/// Должен вызываться с захваченным PFN lock.
pub unsafe fn mi_complete_modified_page_write(pfn: PFN_NUMBER, success: bool) {
    unsafe {
        // Убираем из modified list
        mi_remove_page_from_modified_list(pfn);

        if let Some(entry) = get_pfn_entry_mut(pfn) {
            entry.clear_flag(PFN_FLAGS::WriteInProgress);
            entry.clear_flag(PFN_FLAGS::Modified);
        }

        if success {
            // Успешно записано — переводим в standby
            mi_insert_page_in_standby_list(pfn);
        } else {
            // Ошибка — возвращаем в modified
            mi_insert_page_in_modified_list(pfn);
        }
    }
}

/// Проверяет, нужно ли запускать Modified Page Writer
#[inline]
pub fn mm_should_write_modified_pages() -> bool {
    MM_MODIFIED_PAGE_COUNT.load(Ordering::Relaxed) >= MM_MODIFIED_PAGE_WRITER_THRESHOLD
}

// =============================================================================
// Transition State Operations
// =============================================================================

/// MiMakeTransitionPage
///
/// Переводит страницу в transition state (для trimming).
/// Страница остаётся в памяти, но PTE помечается как transition.
///
/// # Arguments
/// * `pfn` - PFN страницы
/// * `is_dirty` - была ли страница модифицирована
///
/// # Safety
/// Должен вызываться с захваченным PFN lock.
/// PTE уже должна быть обновлена caller'ом.
pub unsafe fn mi_make_transition_page(pfn: PFN_NUMBER, is_dirty: bool) {
    unsafe {
        if is_dirty {
            mi_insert_page_in_modified_list(pfn);
        } else {
            mi_insert_page_in_standby_list(pfn);
        }
    }
}

/// MiResolveTransitionPage
///
/// Восстанавливает transition страницу (при transition fault).
/// Удаляет из standby/modified и делает active.
///
/// # Safety
/// Должен вызываться с захваченным PFN lock.
pub unsafe fn mi_resolve_transition_page(pfn: PFN_NUMBER) {
    unsafe {
        // Удаляем из текущего списка
        mi_unlink_page_from_list(pfn);

        // Делаем active
        if let Some(entry) = get_pfn_entry_mut(pfn) {
            entry.page_location = MMLISTS::ActiveAndValid as u8;
            entry.reference_count = entry.reference_count.saturating_add(1);
        }
    }
}

// =============================================================================
// Reference Count Operations
// =============================================================================

/// MiReferencePfn
///
/// Увеличивает reference count страницы.
///
/// # Safety
/// Должен вызываться с захваченным PFN lock.
pub unsafe fn mi_reference_pfn(pfn: PFN_NUMBER) {
    unsafe {
        if let Some(entry) = get_pfn_entry_mut(pfn) {
            entry.reference_count = entry.reference_count.saturating_add(1);
        }
    }
}

/// MiDereferencePfn
///
/// Уменьшает reference count страницы.
/// При достижении 0 — освобождает страницу.
///
/// # Returns
/// true если страница была освобождена.
///
/// # Safety
/// Должен вызываться с захваченным PFN lock.
pub unsafe fn mi_dereference_pfn(pfn: PFN_NUMBER) -> bool {
    unsafe {
        let should_free = {
            if let Some(entry) = get_pfn_entry_mut(pfn) {
                entry.reference_count = entry.reference_count.saturating_sub(1);
                entry.reference_count == 0 && entry.share_count == 0
            } else {
                false
            }
        };

        if should_free {
            mi_free_pfn(pfn);
            true
        } else {
            false
        }
    }
}

/// MiIncrementShareCount
///
/// Увеличивает share count страницы.
///
/// # Safety
/// Должен вызываться с захваченным PFN lock.
pub unsafe fn mi_increment_share_count(pfn: PFN_NUMBER) {
    unsafe {
        if let Some(entry) = get_pfn_entry_mut(pfn) {
            entry.share_count = entry.share_count.saturating_add(1);
        }
    }
}

/// MiDecrementShareCount
///
/// Уменьшает share count страницы.
///
/// # Safety
/// Должен вызываться с захваченным PFN lock.
pub unsafe fn mi_decrement_share_count(pfn: PFN_NUMBER) {
    unsafe {
        if let Some(entry) = get_pfn_entry_mut(pfn) {
            entry.share_count = entry.share_count.saturating_sub(1);
        }
    }
}
