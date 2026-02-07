//! NT-подобный Pool Allocator
//!
//! Полноценная реализация ExAllocatePool* с поддержкой:
//! - Free lists по размерам блоков
//! - Block coalescing (объединение свободных блоков)
//! - Pool headers с tag tracking
//! - Выделение страниц через PFN database
//!
//! # Архитектура
//!
//! ```text
//! Pool Page Layout:
//! +------------------+
//! | Pool Page Header |
//! +------------------+
//! | Block 1 (Header) |
//! | Block 1 (Data)   |
//! +------------------+
//! | Block 2 (Header) |
//! | Block 2 (Data)   |
//! +------------------+
//! | ...              |
//! +------------------+
//! ```
//!
//! Источники:
//! - ReactOS: mm/ARM3/expool.c
//! - NT6.1: mm/pool.c

use core::ptr;
use core::sync::atomic::AtomicBool;
use core::sync::atomic::AtomicU64;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;

use super::pool::POOL_BLOCK_SIZE;
use super::pool::POOL_HEADER;
use super::pool::POOL_SMALL_LISTS;
use super::pool::POOL_TAG;
use super::pool::POOL_TYPE;
use crate::ke::spinlock::KSPIN_LOCK;
use crate::mm::PAGE_SIZE;
use crate::nt::LIST_ENTRY;
use crate::nt::PVOID;

// =============================================================================
// Constants
// =============================================================================

/// Максимальный размер для small pool allocation (страница минус overhead)
const POOL_MAX_SMALL_ALLOC: usize = PAGE_SIZE - 256;

/// Количество free lists
const NUM_FREE_LISTS: usize = POOL_SMALL_LISTS;

/// Pool page signature
const POOL_PAGE_SIGNATURE: u32 = 0x6C6F6F50; // "Pool"

/// Free block signature
const POOL_FREE_SIGNATURE: u32 = 0x65657246; // "Free"

// =============================================================================
// Pool Page Header
// =============================================================================

/// Заголовок страницы пула
#[repr(C)]
pub struct POOL_PAGE_HEADER {
    /// Сигнатура для валидации
    pub signature: u32,
    /// Тип пула
    pub pool_type: u8,
    /// Флаги
    pub flags: u8,
    /// Количество свободных блоков
    pub free_blocks: u16,
    /// Общее количество блоков
    pub total_blocks: u16,
    /// Размер наибольшего свободного блока (в единицах POOL_BLOCK_SIZE)
    pub largest_free: u16,
    /// Связь в списке страниц пула
    pub page_links: LIST_ENTRY,
    /// Указатель на первый свободный блок
    pub first_free: *mut POOL_FREE_BLOCK,
    /// PFN этой страницы
    pub pfn: usize,
}

impl POOL_PAGE_HEADER {
    pub const fn new() -> Self {
        Self {
            signature: POOL_PAGE_SIGNATURE,
            pool_type: 0,
            flags: 0,
            free_blocks: 0,
            total_blocks: 0,
            largest_free: 0,
            page_links: LIST_ENTRY::new(),
            first_free: ptr::null_mut(),
            pfn: 0,
        }
    }

    /// Размер заголовка страницы (выровнен до POOL_BLOCK_SIZE)
    pub const fn header_size() -> usize {
        (core::mem::size_of::<Self>() + POOL_BLOCK_SIZE - 1) & !(POOL_BLOCK_SIZE - 1)
    }
}

// =============================================================================
// Pool Free Block
// =============================================================================

/// Свободный блок в пуле
#[repr(C)]
pub struct POOL_FREE_BLOCK {
    /// Заголовок (как у выделенного блока)
    pub header: POOL_HEADER,
    /// Сигнатура свободного блока
    pub signature: u32,
    /// Связь в free list
    pub list_entry: LIST_ENTRY,
}

impl POOL_FREE_BLOCK {
    /// Минимальный размер свободного блока (в байтах)
    pub const MIN_SIZE: usize = core::mem::size_of::<Self>();

    /// Минимальный размер в блоках
    pub const MIN_BLOCKS: usize = (Self::MIN_SIZE + POOL_BLOCK_SIZE - 1) / POOL_BLOCK_SIZE;
}

// =============================================================================
// Pool Tracker
// =============================================================================

/// Pool tracker для отслеживания аллокаций по тегам
#[repr(C)]
pub struct POOL_TRACKER {
    /// Tag
    pub tag: POOL_TAG,
    /// Количество аллокаций
    pub alloc_count: AtomicU64,
    /// Количество освобождений
    pub free_count: AtomicU64,
    /// Общий размер выделенной памяти
    pub total_bytes: AtomicU64,
}

// =============================================================================
// NT Pool Descriptor
// =============================================================================

/// NT-подобный дескриптор пула
pub struct NtPoolDescriptor {
    /// Тип пула
    pub pool_type: POOL_TYPE,
    /// Спинлок
    pub lock: KSPIN_LOCK,
    /// Free lists по размерам (индекс = размер в блоках - 1)
    pub free_lists: [LIST_ENTRY; NUM_FREE_LISTS],
    /// Список всех страниц пула
    pub page_list: LIST_ENTRY,
    /// Общее количество страниц
    pub total_pages: AtomicUsize,
    /// Количество свободных байт
    pub free_bytes: AtomicUsize,
    /// Количество используемых байт
    pub used_bytes: AtomicUsize,
    /// Количество аллокаций
    pub alloc_count: AtomicUsize,
    /// Количество освобождений
    pub free_count: AtomicUsize,
    /// Пул инициализирован
    pub initialized: AtomicBool,
}

impl NtPoolDescriptor {
    pub const fn new(pool_type: POOL_TYPE) -> Self {
        Self {
            pool_type,
            lock: KSPIN_LOCK::new(),
            free_lists: [LIST_ENTRY::new(); NUM_FREE_LISTS],
            page_list: LIST_ENTRY::new(),
            total_pages: AtomicUsize::new(0),
            free_bytes: AtomicUsize::new(0),
            used_bytes: AtomicUsize::new(0),
            alloc_count: AtomicUsize::new(0),
            free_count: AtomicUsize::new(0),
            initialized: AtomicBool::new(false),
        }
    }

    /// Инициализирует дескриптор
    pub unsafe fn init(&mut self) {
        unsafe {
            // Инициализируем free lists
            for list in &mut self.free_lists {
                LIST_ENTRY::init_head(list as *mut LIST_ENTRY);
            }
            LIST_ENTRY::init_head(&mut self.page_list as *mut LIST_ENTRY);
            self.initialized.store(true, Ordering::Release);
        }
    }

    /// Проверяет инициализирован ли пул
    #[inline]
    pub fn is_initialized(&self) -> bool {
        self.initialized.load(Ordering::Acquire)
    }
}

// =============================================================================
// Global Pool Descriptors
// =============================================================================

use crate::ke::init::GlobalData;

/// NonPaged Pool descriptor
static NT_NONPAGED_POOL: GlobalData<NtPoolDescriptor> =
    GlobalData::new(NtPoolDescriptor::new(POOL_TYPE::NonPagedPool));

/// Paged Pool descriptor (пока реализован как nonpaged)
static NT_PAGED_POOL: GlobalData<NtPoolDescriptor> =
    GlobalData::new(NtPoolDescriptor::new(POOL_TYPE::PagedPool));

/// NT pool включён
static NT_POOL_ENABLED: AtomicBool = AtomicBool::new(false);

// =============================================================================
// Pool Initialization
// =============================================================================

/// Инициализирует NT pool system
pub unsafe fn nt_pool_init() {
    unsafe {
        (*NT_NONPAGED_POOL.get()).init();
        (*NT_PAGED_POOL.get()).init();
        NT_POOL_ENABLED.store(true, Ordering::Release);
    }
}

/// Проверяет включён ли NT pool
#[inline]
pub fn nt_pool_enabled() -> bool {
    NT_POOL_ENABLED.load(Ordering::Acquire)
}

// =============================================================================
// Pool Allocation
// =============================================================================

/// Выделяет память из NT pool
pub fn nt_pool_allocate(pool_type: POOL_TYPE, size: usize, tag: POOL_TAG) -> PVOID {
    if size == 0 {
        return ptr::null_mut();
    }

    // Вычисляем размер с заголовком
    let total_size = size + core::mem::size_of::<POOL_HEADER>();

    // Вычисляем количество блоков (округляем вверх)
    let blocks_needed = (total_size + POOL_BLOCK_SIZE - 1) / POOL_BLOCK_SIZE;

    // Получаем дескриптор пула
    let descriptor = if pool_type.is_paged() {
        unsafe { &mut *NT_PAGED_POOL.get() }
    } else {
        unsafe { &mut *NT_NONPAGED_POOL.get() }
    };

    if !descriptor.is_initialized() {
        return ptr::null_mut();
    }

    // Пробуем найти подходящий свободный блок
    let ptr = unsafe { find_and_allocate_block(descriptor, blocks_needed, tag) };

    if !ptr.is_null() {
        descriptor.alloc_count.fetch_add(1, Ordering::Relaxed);
        descriptor
            .used_bytes
            .fetch_add(blocks_needed * POOL_BLOCK_SIZE, Ordering::Relaxed);
    }

    ptr
}

/// Ищет и выделяет блок из free lists
unsafe fn find_and_allocate_block(
    descriptor: &mut NtPoolDescriptor,
    blocks_needed: usize,
    tag: POOL_TAG,
) -> PVOID {
    unsafe {
        // Индекс free list = blocks_needed - 1 (но не больше NUM_FREE_LISTS - 1)
        let start_index = blocks_needed.min(NUM_FREE_LISTS) - 1;

        // Ищем в free lists начиная с нужного размера
        for i in start_index..NUM_FREE_LISTS {
            let list = &mut descriptor.free_lists[i];

            if !LIST_ENTRY::is_empty(list) {
                // Есть свободный блок подходящего размера
                let entry = (*list).flink;
                let free_block = entry as *mut POOL_FREE_BLOCK;

                // Удаляем из free list
                LIST_ENTRY::remove_entry(entry);

                let block_size = (*free_block).header.block_size as usize;

                // Проверяем, можно ли разделить блок
                if block_size > blocks_needed + POOL_FREE_BLOCK::MIN_BLOCKS {
                    // Разделяем блок
                    split_block(descriptor, free_block, blocks_needed);
                }

                // Заполняем заголовок
                let header = &mut (*free_block).header;
                header.pool_type = descriptor.pool_type as u8;
                header.pool_tag = tag;
                header.block_size = blocks_needed as u8;

                // Обновляем статистику
                descriptor
                    .free_bytes
                    .fetch_sub(blocks_needed * POOL_BLOCK_SIZE, Ordering::Relaxed);

                // Возвращаем указатель на данные (после заголовка)
                return (free_block as *mut u8).add(core::mem::size_of::<POOL_HEADER>()) as PVOID;
            }
        }

        // Нет подходящего свободного блока — нужна новая страница
        allocate_new_pool_page(descriptor, blocks_needed, tag)
    }
}

/// Разделяет блок на два
unsafe fn split_block(
    descriptor: &mut NtPoolDescriptor,
    block: *mut POOL_FREE_BLOCK,
    blocks_to_use: usize,
) {
    unsafe {
        let original_size = (*block).header.block_size as usize;
        let remaining_size = original_size - blocks_to_use;

        // Создаём новый свободный блок после выделяемого
        let new_free =
            (block as *mut u8).add(blocks_to_use * POOL_BLOCK_SIZE) as *mut POOL_FREE_BLOCK;

        (*new_free).header.block_size = remaining_size as u8;
        (*new_free).header.previous_size = blocks_to_use as u8;
        (*new_free).header.pool_type = descriptor.pool_type as u8;
        (*new_free).signature = POOL_FREE_SIGNATURE;

        // Обновляем размер оригинального блока
        (*block).header.block_size = blocks_to_use as u8;

        // Добавляем новый свободный блок в free list
        let list_index = (remaining_size - 1).min(NUM_FREE_LISTS - 1);
        LIST_ENTRY::insert_head(
            &mut descriptor.free_lists[list_index],
            &mut (*new_free).list_entry,
        );
    }
}

/// Выделяет новую страницу пула
unsafe fn allocate_new_pool_page(
    descriptor: &mut NtPoolDescriptor,
    blocks_needed: usize,
    tag: POOL_TAG,
) -> PVOID {
    unsafe {
        // Выделяем физическую страницу
        let pfn = crate::mm::mi_allocate_pfn();
        if pfn.is_none() {
            return ptr::null_mut();
        }
        let pfn = pfn.unwrap();

        // Маппим страницу в System PTE region
        let va = crate::mm::mi_reserve_system_ptes(1);
        if va == 0 {
            crate::mm::mi_free_pfn(pfn);
            return ptr::null_mut();
        }

        crate::mm::mi_map_system_pte(va, pfn, true);

        // Инициализируем страницу пула
        let page_header = va as *mut POOL_PAGE_HEADER;
        core::ptr::write(page_header, POOL_PAGE_HEADER::new());
        (*page_header).pool_type = descriptor.pool_type as u8;
        (*page_header).pfn = pfn;

        // Вычисляем доступное пространство
        let header_size = POOL_PAGE_HEADER::header_size();
        let available_size = PAGE_SIZE - header_size;
        let available_blocks = available_size / POOL_BLOCK_SIZE;

        (*page_header).total_blocks = available_blocks as u16;
        (*page_header).free_blocks = available_blocks as u16;
        (*page_header).largest_free = available_blocks as u16;

        // Создаём первый свободный блок
        let first_block = (va as *mut u8).add(header_size) as *mut POOL_FREE_BLOCK;
        (*first_block).header.block_size = available_blocks as u8;
        (*first_block).header.previous_size = 0;
        (*first_block).header.pool_type = descriptor.pool_type as u8;
        (*first_block).signature = POOL_FREE_SIGNATURE;

        (*page_header).first_free = first_block;

        // Добавляем страницу в список
        LIST_ENTRY::insert_tail(&mut descriptor.page_list, &mut (*page_header).page_links);
        descriptor.total_pages.fetch_add(1, Ordering::Relaxed);
        descriptor
            .free_bytes
            .fetch_add(available_blocks * POOL_BLOCK_SIZE, Ordering::Relaxed);

        // Теперь выделяем из этой страницы
        if blocks_needed <= available_blocks {
            // Разделяем если нужно
            if available_blocks > blocks_needed + POOL_FREE_BLOCK::MIN_BLOCKS {
                split_block(descriptor, first_block, blocks_needed);
            }

            // Заполняем заголовок
            (*first_block).header.pool_tag = tag;
            (*first_block).header.block_size = blocks_needed as u8;

            (*page_header).free_blocks -= blocks_needed as u16;
            descriptor
                .free_bytes
                .fetch_sub(blocks_needed * POOL_BLOCK_SIZE, Ordering::Relaxed);

            return (first_block as *mut u8).add(core::mem::size_of::<POOL_HEADER>()) as PVOID;
        }

        ptr::null_mut()
    }
}

// =============================================================================
// Pool Deallocation
// =============================================================================

/// Освобождает память в NT pool
pub fn nt_pool_free(ptr: PVOID, tag: POOL_TAG) {
    if ptr.is_null() {
        return;
    }

    unsafe {
        // Получаем заголовок блока
        let header = (ptr as *mut u8).sub(core::mem::size_of::<POOL_HEADER>()) as *mut POOL_HEADER;

        // Проверяем тег если указан
        if tag != 0 && (*header).pool_tag != tag {
            // Tag mismatch - потенциальная ошибка
            return;
        }

        let pool_type = (*header).pool_type;
        let block_size = (*header).block_size as usize;

        // Получаем дескриптор
        let descriptor = if pool_type == POOL_TYPE::PagedPool as u8 {
            &mut *NT_PAGED_POOL.get()
        } else {
            &mut *NT_NONPAGED_POOL.get()
        };

        // Преобразуем в free block
        let free_block = header as *mut POOL_FREE_BLOCK;
        (*free_block).signature = POOL_FREE_SIGNATURE;

        // Пробуем объединить со смежными свободными блоками
        let final_size = coalesce_blocks(descriptor, free_block);

        // Добавляем в free list
        let list_index = (final_size - 1).min(NUM_FREE_LISTS - 1);
        LIST_ENTRY::insert_head(
            &mut descriptor.free_lists[list_index],
            &mut (*free_block).list_entry,
        );

        // Обновляем статистику
        descriptor.free_count.fetch_add(1, Ordering::Relaxed);
        descriptor
            .used_bytes
            .fetch_sub(block_size * POOL_BLOCK_SIZE, Ordering::Relaxed);
        descriptor
            .free_bytes
            .fetch_add(final_size * POOL_BLOCK_SIZE, Ordering::Relaxed);
    }
}

/// Объединяет смежные свободные блоки
unsafe fn coalesce_blocks(
    _descriptor: &mut NtPoolDescriptor,
    block: *mut POOL_FREE_BLOCK,
) -> usize {
    unsafe {
        let mut current_size = (*block).header.block_size as usize;

        // Пробуем объединить с предыдущим блоком
        let prev_size = (*block).header.previous_size as usize;
        if prev_size > 0 {
            let prev_block =
                (block as *mut u8).sub(prev_size * POOL_BLOCK_SIZE) as *mut POOL_FREE_BLOCK;

            // Проверяем, свободен ли предыдущий блок
            if (*prev_block).signature == POOL_FREE_SIGNATURE {
                // Удаляем предыдущий блок из free list
                LIST_ENTRY::remove_entry(&mut (*prev_block).list_entry);

                // Объединяем
                let new_size = prev_size + current_size;
                (*prev_block).header.block_size = new_size as u8;

                // Теперь работаем с объединённым блоком
                // (block больше не используем)
                current_size = new_size;

                // Обновляем previous_size следующего блока если есть
                let next_block =
                    (prev_block as *mut u8).add(new_size * POOL_BLOCK_SIZE) as *mut POOL_FREE_BLOCK;
                // TODO: проверить границы страницы
                (*next_block).header.previous_size = new_size as u8;
            }
        }

        // Пробуем объединить со следующим блоком
        let next_block =
            (block as *mut u8).add(current_size * POOL_BLOCK_SIZE) as *mut POOL_FREE_BLOCK;

        // TODO: проверить, что next_block в пределах страницы
        if (*next_block).signature == POOL_FREE_SIGNATURE {
            let next_size = (*next_block).header.block_size as usize;

            // Удаляем следующий блок из free list
            LIST_ENTRY::remove_entry(&mut (*next_block).list_entry);

            // Объединяем
            current_size += next_size;
            (*block).header.block_size = current_size as u8;
        }

        current_size
    }
}

// =============================================================================
// Pool Statistics
// =============================================================================

/// Возвращает статистику пула
pub fn nt_pool_stats(pool_type: POOL_TYPE) -> PoolStats {
    let descriptor = if pool_type.is_paged() {
        unsafe { &*NT_PAGED_POOL.get() }
    } else {
        unsafe { &*NT_NONPAGED_POOL.get() }
    };

    PoolStats {
        total_pages: descriptor.total_pages.load(Ordering::Relaxed),
        free_bytes: descriptor.free_bytes.load(Ordering::Relaxed),
        used_bytes: descriptor.used_bytes.load(Ordering::Relaxed),
        alloc_count: descriptor.alloc_count.load(Ordering::Relaxed),
        free_count: descriptor.free_count.load(Ordering::Relaxed),
    }
}

/// Статистика пула
#[derive(Clone, Copy, Debug)]
pub struct PoolStats {
    pub total_pages: usize,
    pub free_bytes: usize,
    pub used_bytes: usize,
    pub alloc_count: usize,
    pub free_count: usize,
}

// =============================================================================
// Lookaside Lists
// =============================================================================

/// Lookaside list для частых размеров аллокаций
#[repr(C)]
pub struct NPAGED_LOOKASIDE_LIST {
    /// Список свободных элементов
    pub list_head: LIST_ENTRY,
    /// Глубина списка (текущее количество элементов)
    pub depth: AtomicU16,
    /// Максимальная глубина
    pub max_depth: u16,
    /// Размер элемента
    pub size: u32,
    /// Tag
    pub tag: POOL_TAG,
    /// Тип пула
    pub pool_type: POOL_TYPE,
    /// Количество allocate hits
    pub alloc_hits: AtomicU64,
    /// Количество allocate misses
    pub alloc_misses: AtomicU64,
    /// Количество free hits
    pub free_hits: AtomicU64,
    /// Количество free misses
    pub free_misses: AtomicU64,
    /// Спинлок
    pub lock: KSPIN_LOCK,
}

use core::sync::atomic::AtomicU16;

impl NPAGED_LOOKASIDE_LIST {
    pub const fn new() -> Self {
        Self {
            list_head: LIST_ENTRY::new(),
            depth: AtomicU16::new(0),
            max_depth: 256,
            size: 0,
            tag: 0,
            pool_type: POOL_TYPE::NonPagedPool,
            alloc_hits: AtomicU64::new(0),
            alloc_misses: AtomicU64::new(0),
            free_hits: AtomicU64::new(0),
            free_misses: AtomicU64::new(0),
            lock: KSPIN_LOCK::new(),
        }
    }

    /// Инициализирует lookaside list
    pub unsafe fn init(&mut self, size: u32, tag: POOL_TAG, max_depth: u16) {
        unsafe {
            LIST_ENTRY::init_head(&mut self.list_head);
            self.size = size;
            self.tag = tag;
            self.max_depth = max_depth;
            self.depth.store(0, Ordering::Release);
        }
    }

    /// Выделяет элемент из lookaside list
    pub unsafe fn allocate(&mut self) -> PVOID {
        unsafe {
            if !LIST_ENTRY::is_empty(&self.list_head) {
                let entry = self.list_head.flink;
                LIST_ENTRY::remove_entry(entry);
                self.depth.fetch_sub(1, Ordering::Relaxed);
                self.alloc_hits.fetch_add(1, Ordering::Relaxed);
                return entry as PVOID;
            }

            // Lookaside пуст — выделяем из пула
            self.alloc_misses.fetch_add(1, Ordering::Relaxed);
            nt_pool_allocate(self.pool_type, self.size as usize, self.tag)
        }
    }

    /// Освобождает элемент в lookaside list
    pub unsafe fn free(&mut self, ptr: PVOID) {
        unsafe {
            if ptr.is_null() {
                return;
            }

            let current_depth = self.depth.load(Ordering::Relaxed);
            if current_depth < self.max_depth {
                // Добавляем в lookaside
                let entry = ptr as *mut LIST_ENTRY;
                LIST_ENTRY::insert_head(&mut self.list_head, entry);
                self.depth.fetch_add(1, Ordering::Relaxed);
                self.free_hits.fetch_add(1, Ordering::Relaxed);
            } else {
                // Lookaside полон — освобождаем в пул
                self.free_misses.fetch_add(1, Ordering::Relaxed);
                nt_pool_free(ptr, self.tag);
            }
        }
    }
}

/// ExInitializeNPagedLookasideList
pub unsafe fn ex_initialize_npaged_lookaside_list(
    lookaside: *mut NPAGED_LOOKASIDE_LIST,
    size: u32,
    tag: POOL_TAG,
    depth: u16,
) {
    unsafe {
        if !lookaside.is_null() {
            (*lookaside).init(size, tag, depth);
        }
    }
}

/// ExAllocateFromNPagedLookasideList
pub unsafe fn ex_allocate_from_npaged_lookaside_list(
    lookaside: *mut NPAGED_LOOKASIDE_LIST,
) -> PVOID {
    unsafe {
        if lookaside.is_null() {
            return ptr::null_mut();
        }
        (*lookaside).allocate()
    }
}

/// ExFreeToNPagedLookasideList
pub unsafe fn ex_free_to_npaged_lookaside_list(
    lookaside: *mut NPAGED_LOOKASIDE_LIST,
    entry: PVOID,
) {
    unsafe {
        if !lookaside.is_null() {
            (*lookaside).free(entry);
        }
    }
}
