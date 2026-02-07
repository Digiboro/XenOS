//! Pagefile Manager
//!
//! Управление файлами подкачки (pagefile.sys).
//!
//! # Архитектура
//!
//! ```text
//! MmPagingFile[] (до 16 pagefile'ов)
//!     |
//!     +---> MMPAGING_FILE
//!               |
//!               +---> FILE_OBJECT (pagefile.sys)
//!               +---> Bitmap (свободные/занятые страницы)
//!               +---> Размер и статистика
//! ```
//!
//! # Commit Accounting
//!
//! Система отслеживает "commit" — количество виртуальных страниц,
//! которые имеют backing store (RAM или pagefile).
//!
//! - `MmTotalCommitLimit` — макс. количество commit (RAM + pagefile)
//! - `MmTotalCommittedPages` — текущий commit
//! - Per-process commit в EPROCESS
//!
//! # Pagefile Allocation
//!
//! Когда страница впервые записывается в pagefile, ей выделяется слот:
//!
//! 1. Выбирается pagefile (round-robin или по заполненности)
//! 2. В bitmap ищется свободный бит
//! 3. Слот помечается занятым
//! 4. В PTE сохраняется (pagefile_index, offset)
//!
//! При чтении обратно: (pagefile_index, offset) из PTE → файл → RAM
//!
//! # Текущее состояние
//!
//! Базовая реализация без реального файла:
//! - Commit accounting работает
//! - Pagefile space отслеживается в bitmap
//! - Реальный I/O требует интеграции с IO Manager
//!
//! Источники:
//! - NT6.1: mm/pagefile.c
//! - ReactOS: mm/ARM3/pagefile.c

use core::ptr;
use core::sync::atomic::AtomicU64;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;

use super::pfn::SyncUnsafeCell;
use super::types::PAGE_SIZE;
use crate::io::file::FILE_OBJECT;
use crate::ke::spinlock::KSPIN_LOCK;
use crate::nt::NTSTATUS;
use crate::nt::PVOID;
use crate::nt::STATUS_COMMITMENT_LIMIT;
use crate::nt::STATUS_INVALID_PARAMETER;
use crate::nt::STATUS_NO_MEMORY;
use crate::nt::STATUS_SUCCESS;

// =============================================================================
// Pagefile Constants
// =============================================================================

/// Максимальное количество pagefile'ов
pub const MM_MAX_PAGING_FILES: usize = 16;

/// Минимальный размер pagefile в страницах (16 MB)
pub const MM_MIN_PAGEFILE_SIZE: usize = 4096;

/// Размер по умолчанию в страницах (256 MB)
pub const MM_DEFAULT_PAGEFILE_SIZE: usize = 65536;

/// Максимальный размер в страницах (16 GB для x64)
pub const MM_MAX_PAGEFILE_SIZE: usize = 4 * 1024 * 1024;

/// Порог расширения pagefile (когда свободно меньше X%)
pub const MM_PAGEFILE_EXTEND_THRESHOLD_PERCENT: usize = 10;

// =============================================================================
// MMPAGING_FILE — Pagefile Descriptor
// =============================================================================

/// MMPAGING_FILE — структура описания одного pagefile
#[repr(C)]
pub struct MMPAGING_FILE {
    /// FILE_OBJECT для pagefile.sys
    pub file_object: *mut FILE_OBJECT,

    /// Текущий размер в страницах
    pub size: AtomicUsize,

    /// Минимальный размер
    pub minimum_size: usize,

    /// Максимальный размер
    pub maximum_size: usize,

    /// Количество свободных страниц в файле
    pub free_space: AtomicUsize,

    /// Количество использованных страниц
    pub used_space: AtomicUsize,

    /// Пиковое использование
    pub peak_usage: AtomicUsize,

    /// Bitmap свободных страниц (1 = свободен, 0 = занят)
    /// Каждый бит = одна страница в pagefile
    pub bitmap: *mut u64,

    /// Размер bitmap в u64 словах
    pub bitmap_size: usize,

    /// Следующий hint для поиска свободного слота
    pub next_hint: AtomicUsize,

    /// Индекс этого pagefile в массиве
    pub page_file_number: u8,

    /// Флаги
    pub flags: u8,

    /// Padding
    _reserved: [u8; 6],

    /// Lock для bitmap операций
    pub lock: KSPIN_LOCK,
}

/// Флаги pagefile
pub mod pagefile_flags {
    /// Pagefile активен
    pub const PAGEFILE_ACTIVE: u8 = 0x01;
    /// Pagefile можно расширять
    pub const PAGEFILE_EXTENDABLE: u8 = 0x02;
    /// Pagefile на SSD (оптимизации)
    pub const PAGEFILE_SSD: u8 = 0x04;
    /// Pagefile для аварийного дампа
    pub const PAGEFILE_CRASH_DUMP: u8 = 0x08;
    /// Temporary pagefile (в RAM)
    pub const PAGEFILE_IN_MEMORY: u8 = 0x10;
}

impl MMPAGING_FILE {
    pub const fn new() -> Self {
        Self {
            file_object: ptr::null_mut(),
            size: AtomicUsize::new(0),
            minimum_size: 0,
            maximum_size: 0,
            free_space: AtomicUsize::new(0),
            used_space: AtomicUsize::new(0),
            peak_usage: AtomicUsize::new(0),
            bitmap: ptr::null_mut(),
            bitmap_size: 0,
            next_hint: AtomicUsize::new(0),
            page_file_number: 0,
            flags: 0,
            _reserved: [0; 6],
            lock: KSPIN_LOCK::new(),
        }
    }

    /// Проверяет, активен ли pagefile
    #[inline]
    pub fn is_active(&self) -> bool {
        (self.flags & pagefile_flags::PAGEFILE_ACTIVE) != 0
    }

    /// Проверяет, можно ли расширять
    #[inline]
    pub fn is_extendable(&self) -> bool {
        (self.flags & pagefile_flags::PAGEFILE_EXTENDABLE) != 0
    }

    /// Возвращает процент использования
    #[inline]
    pub fn usage_percent(&self) -> usize {
        let size = self.size.load(Ordering::Relaxed);
        if size == 0 {
            return 0;
        }
        (self.used_space.load(Ordering::Relaxed) * 100) / size
    }
}

// =============================================================================
// Global Pagefile State
// =============================================================================

/// Массив pagefile'ов
pub static MM_PAGING_FILE: SyncUnsafeCell<[MMPAGING_FILE; MM_MAX_PAGING_FILES]> =
    SyncUnsafeCell::new([const { MMPAGING_FILE::new() }; MM_MAX_PAGING_FILES]);

/// Количество активных pagefile'ов
pub static MM_NUMBER_OF_PAGING_FILES: AtomicUsize = AtomicUsize::new(0);

/// Общий размер всех pagefile'ов в страницах
pub static MM_PAGEFILE_SIZE: AtomicUsize = AtomicUsize::new(0);

// =============================================================================
// Commit Accounting
// =============================================================================

/// Commit limit (RAM + pagefile space)
pub static MM_TOTAL_COMMIT_LIMIT: AtomicUsize = AtomicUsize::new(0);

/// Текущий committed pages (глобально)
pub static MM_TOTAL_COMMITTED_PAGES: AtomicUsize = AtomicUsize::new(0);

/// Peak commit
pub static MM_PEAK_COMMITMENT: AtomicUsize = AtomicUsize::new(0);

/// Minimum commit available (below this -> low commit warning)
pub static MM_MINIMUM_COMMIT_AVAILABLE: AtomicUsize = AtomicUsize::new(0);

/// System commit (kernel allocations)
pub static MM_SYSTEM_COMMIT: AtomicUsize = AtomicUsize::new(0);

/// Shared commit (section pages)
pub static MM_SHARED_COMMIT: AtomicUsize = AtomicUsize::new(0);

// =============================================================================
// Commit Accounting Operations
// =============================================================================

/// MiChargeCommitment
///
/// Резервирует commit для выделения памяти.
///
/// # Arguments
/// * `pages` - количество страниц для резервирования
/// * `process` - указатель на EPROCESS (может быть NULL для системы)
///
/// # Returns
/// true если commit успешно зарезервирован
pub fn mi_charge_commitment(pages: usize, _process: PVOID) -> bool {
    if pages == 0 {
        return true;
    }

    // Атомарно пытаемся увеличить commit
    loop {
        let current = MM_TOTAL_COMMITTED_PAGES.load(Ordering::Acquire);
        let limit = MM_TOTAL_COMMIT_LIMIT.load(Ordering::Acquire);

        let new_commit = current.saturating_add(pages);

        if new_commit > limit {
            // Превышен лимит — попробуем расширить pagefile
            if !mi_try_extend_pagefiles(pages) {
                return false;
            }
            // После расширения — повторяем проверку
            continue;
        }

        if MM_TOTAL_COMMITTED_PAGES
            .compare_exchange(current, new_commit, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            // Успешно зарезервировали commit
            // Обновляем peak
            let peak = MM_PEAK_COMMITMENT.load(Ordering::Relaxed);
            if new_commit > peak {
                MM_PEAK_COMMITMENT.store(new_commit, Ordering::Relaxed);
            }

            // TODO: Обновить per-process commit в EPROCESS
            // if !process.is_null() {
            //     (*process).commit_charge += pages;
            // }

            return true;
        }
        // Conflict — retry
    }
}

/// MiReturnCommitment
///
/// Возвращает commit при освобождении памяти.
///
/// # Arguments
/// * `pages` - количество страниц для возврата
/// * `process` - указатель на EPROCESS (может быть NULL)
pub fn mi_return_commitment(pages: usize, _process: PVOID) {
    if pages == 0 {
        return;
    }

    MM_TOTAL_COMMITTED_PAGES.fetch_sub(pages, Ordering::AcqRel);

    // TODO: Обновить per-process commit в EPROCESS
    // if !process.is_null() {
    //     (*process).commit_charge -= pages;
    // }
}

/// Проверяет, доступен ли commit
#[inline]
pub fn mi_check_commit_available(pages: usize) -> bool {
    let current = MM_TOTAL_COMMITTED_PAGES.load(Ordering::Relaxed);
    let limit = MM_TOTAL_COMMIT_LIMIT.load(Ordering::Relaxed);
    current.saturating_add(pages) <= limit
}

// =============================================================================
// Pagefile Space Allocation
// =============================================================================

/// MiAllocatePagefileSpace
///
/// Выделяет место в pagefile для страницы.
///
/// # Returns
/// (pagefile_index, offset) или None если нет места
pub fn mi_allocate_pagefile_space() -> Option<(u8, u64)> {
    let num_files = MM_NUMBER_OF_PAGING_FILES.load(Ordering::Acquire);
    if num_files == 0 {
        return None;
    }

    unsafe {
        let files = &mut *MM_PAGING_FILE.get();

        // Round-robin поиск по pagefile'ам
        for file_idx in 0..num_files {
            let pf = &mut files[file_idx];

            if !pf.is_active() {
                continue;
            }

            // Пытаемся найти свободный слот в bitmap
            if let Some(offset) = mi_allocate_slot_in_pagefile(pf) {
                return Some((file_idx as u8, offset));
            }
        }

        None
    }
}

/// Выделяет слот в конкретном pagefile
unsafe fn mi_allocate_slot_in_pagefile(pf: &mut MMPAGING_FILE) -> Option<u64> {
    unsafe {
        let size = pf.size.load(Ordering::Relaxed);
        if size == 0 || pf.bitmap.is_null() {
            return None;
        }

        let free = pf.free_space.load(Ordering::Relaxed);
        if free == 0 {
            return None;
        }

        // Начинаем поиск с hint
        let hint = pf.next_hint.load(Ordering::Relaxed);
        let bitmap_words = pf.bitmap_size;

        // Поиск свободного бита
        for i in 0..bitmap_words {
            let word_idx = (hint / 64 + i) % bitmap_words;
            let word_ptr = pf.bitmap.add(word_idx);
            let word = core::ptr::read_volatile(word_ptr);

            if word == 0 {
                // Все биты заняты в этом слове
                continue;
            }

            // Находим первый установленный бит (свободный слот)
            let bit_idx = word.trailing_zeros() as usize;
            let offset = word_idx * 64 + bit_idx;

            if offset >= size {
                continue;
            }

            // Атомарно сбрасываем бит (занимаем слот)
            let new_word = word & !(1u64 << bit_idx);
            core::ptr::write_volatile(word_ptr, new_word);

            // Обновляем статистику
            pf.free_space.fetch_sub(1, Ordering::AcqRel);
            let used = pf.used_space.fetch_add(1, Ordering::AcqRel) + 1;

            // Обновляем peak
            let peak = pf.peak_usage.load(Ordering::Relaxed);
            if used > peak {
                pf.peak_usage.store(used, Ordering::Relaxed);
            }

            // Обновляем hint
            pf.next_hint.store(offset + 1, Ordering::Relaxed);

            return Some(offset as u64);
        }

        None
    }
}

/// MiFreePagefileSpace
///
/// Освобождает место в pagefile.
///
/// # Arguments
/// * `pagefile_index` - индекс pagefile
/// * `offset` - смещение в pagefile
pub fn mi_free_pagefile_space(pagefile_index: u8, offset: u64) {
    unsafe {
        let files = &mut *MM_PAGING_FILE.get();

        if pagefile_index as usize >= MM_MAX_PAGING_FILES {
            return;
        }

        let pf = &mut files[pagefile_index as usize];

        if !pf.is_active() || pf.bitmap.is_null() {
            return;
        }

        let size = pf.size.load(Ordering::Relaxed);
        if offset as usize >= size {
            return;
        }

        // Устанавливаем бит обратно (освобождаем слот)
        let word_idx = (offset as usize) / 64;
        let bit_idx = (offset as usize) % 64;

        let word_ptr = pf.bitmap.add(word_idx);
        let word = core::ptr::read_volatile(word_ptr);
        let new_word = word | (1u64 << bit_idx);
        core::ptr::write_volatile(word_ptr, new_word);

        // Обновляем статистику
        pf.free_space.fetch_add(1, Ordering::AcqRel);
        pf.used_space.fetch_sub(1, Ordering::AcqRel);

        // Обновляем hint если освободили раньше текущего
        let hint = pf.next_hint.load(Ordering::Relaxed);
        if (offset as usize) < hint {
            pf.next_hint.store(offset as usize, Ordering::Relaxed);
        }
    }
}

// =============================================================================
// Pagefile Creation and Management
// =============================================================================

/// NtCreatePagingFile
///
/// Создаёт новый pagefile.
///
/// # Arguments
/// * `file_object` - FILE_OBJECT для файла (может быть NULL для in-memory)
/// * `minimum_size` - минимальный размер в страницах
/// * `maximum_size` - максимальный размер в страницах
///
/// # Returns
/// NTSTATUS
pub unsafe fn nt_create_paging_file(
    file_object: *mut FILE_OBJECT,
    minimum_size: usize,
    maximum_size: usize,
) -> NTSTATUS {
    unsafe {
        // Проверяем параметры
        if minimum_size < MM_MIN_PAGEFILE_SIZE {
            return STATUS_INVALID_PARAMETER;
        }

        if maximum_size < minimum_size || maximum_size > MM_MAX_PAGEFILE_SIZE {
            return STATUS_INVALID_PARAMETER;
        }

        // Находим свободный слот
        let num_files = MM_NUMBER_OF_PAGING_FILES.load(Ordering::Acquire);
        if num_files >= MM_MAX_PAGING_FILES {
            return STATUS_NO_MEMORY;
        }

        let files = &mut *MM_PAGING_FILE.get();

        // Ищем первый свободный слот
        let mut slot_idx = None;
        for i in 0..MM_MAX_PAGING_FILES {
            if !files[i].is_active() {
                slot_idx = Some(i);
                break;
            }
        }

        let slot_idx = match slot_idx {
            Some(i) => i,
            None => return STATUS_NO_MEMORY,
        };

        // Выделяем bitmap
        let bitmap_words = (minimum_size + 63) / 64;
        let bitmap_size_bytes = bitmap_words * 8;

        let bitmap = crate::ex::pool::ex_allocate_pool_with_tag(
            crate::ex::pool::POOL_TYPE::NonPagedPool,
            bitmap_size_bytes,
            u32::from_le_bytes(*b"MmPf"),
        ) as *mut u64;

        if bitmap.is_null() {
            return STATUS_NO_MEMORY;
        }

        // Инициализируем bitmap (все биты = 1 = свободно)
        for i in 0..bitmap_words {
            core::ptr::write_volatile(bitmap.add(i), u64::MAX);
        }

        // Если размер не кратен 64, маскируем лишние биты
        let extra_bits = minimum_size % 64;
        if extra_bits != 0 {
            let last_word = bitmap.add(bitmap_words - 1);
            let mask = (1u64 << extra_bits) - 1;
            core::ptr::write_volatile(last_word, mask);
        }

        // Настраиваем MMPAGING_FILE
        let pf = &mut files[slot_idx];
        pf.file_object = file_object;
        pf.size.store(minimum_size, Ordering::Release);
        pf.minimum_size = minimum_size;
        pf.maximum_size = maximum_size;
        pf.free_space.store(minimum_size, Ordering::Release);
        pf.used_space.store(0, Ordering::Release);
        pf.peak_usage.store(0, Ordering::Release);
        pf.bitmap = bitmap;
        pf.bitmap_size = bitmap_words;
        pf.next_hint.store(0, Ordering::Release);
        pf.page_file_number = slot_idx as u8;

        // Устанавливаем флаги
        pf.flags = pagefile_flags::PAGEFILE_ACTIVE;
        if maximum_size > minimum_size {
            pf.flags |= pagefile_flags::PAGEFILE_EXTENDABLE;
        }
        if file_object.is_null() {
            pf.flags |= pagefile_flags::PAGEFILE_IN_MEMORY;
        }

        // Обновляем глобальные счётчики
        MM_NUMBER_OF_PAGING_FILES.fetch_add(1, Ordering::AcqRel);
        MM_PAGEFILE_SIZE.fetch_add(minimum_size, Ordering::AcqRel);

        // Увеличиваем commit limit
        MM_TOTAL_COMMIT_LIMIT.fetch_add(minimum_size, Ordering::AcqRel);

        STATUS_SUCCESS
    }
}

/// Пытается расширить pagefile'ы
fn mi_try_extend_pagefiles(needed_pages: usize) -> bool {
    unsafe {
        let files = &mut *MM_PAGING_FILE.get();

        for i in 0..MM_MAX_PAGING_FILES {
            let pf = &mut files[i];

            if !pf.is_active() || !pf.is_extendable() {
                continue;
            }

            let current_size = pf.size.load(Ordering::Relaxed);
            if current_size >= pf.maximum_size {
                continue;
            }

            // Вычисляем новый размер
            let extend_by = needed_pages.max(current_size / 4); // минимум 25% расширение
            let new_size = (current_size + extend_by).min(pf.maximum_size);

            if new_size <= current_size {
                continue;
            }

            // TODO: Реально расширить файл через I/O
            // Для in-memory pagefile просто увеличиваем bitmap

            // Расширяем bitmap
            let new_bitmap_words = (new_size + 63) / 64;
            if new_bitmap_words > pf.bitmap_size {
                // Нужен новый bitmap
                let new_bitmap = crate::ex::pool::ex_allocate_pool_with_tag(
                    crate::ex::pool::POOL_TYPE::NonPagedPool,
                    new_bitmap_words * 8,
                    u32::from_le_bytes(*b"MmPf"),
                ) as *mut u64;

                if new_bitmap.is_null() {
                    continue;
                }

                // Копируем старый bitmap
                core::ptr::copy_nonoverlapping(pf.bitmap, new_bitmap, pf.bitmap_size);

                // Инициализируем новые слова
                for j in pf.bitmap_size..new_bitmap_words {
                    core::ptr::write_volatile(new_bitmap.add(j), u64::MAX);
                }

                // Освобождаем старый bitmap
                crate::ex::pool::ex_free_pool_with_tag(pf.bitmap as PVOID, u32::from_le_bytes(*b"MmPf"));

                pf.bitmap = new_bitmap;
                pf.bitmap_size = new_bitmap_words;
            }

            // Обновляем размер
            let added = new_size - current_size;
            pf.size.store(new_size, Ordering::Release);
            pf.free_space.fetch_add(added, Ordering::AcqRel);

            // Обновляем глобальные счётчики
            MM_PAGEFILE_SIZE.fetch_add(added, Ordering::AcqRel);
            MM_TOTAL_COMMIT_LIMIT.fetch_add(added, Ordering::AcqRel);

            return true;
        }

        false
    }
}

// =============================================================================
// Pagefile I/O (требует IO Manager)
// =============================================================================

/// MiReadPageFromPagefile
///
/// Читает страницу из pagefile.
///
/// # Arguments
/// * `pagefile_index` - индекс pagefile
/// * `offset` - смещение в pagefile (в страницах)
/// * `destination` - адрес для записи данных
///
/// # Returns
/// NTSTATUS
///
/// # Текущее состояние
/// Заглушка — заполняет нулями (для in-memory pagefile это корректно).
pub unsafe fn mi_read_page_from_pagefile(
    pagefile_index: u8,
    offset: u64,
    destination: *mut u8,
) -> NTSTATUS {
    unsafe {
        let files = &*MM_PAGING_FILE.get();

        if pagefile_index as usize >= MM_MAX_PAGING_FILES {
            return STATUS_INVALID_PARAMETER;
        }

        let pf = &files[pagefile_index as usize];

        if !pf.is_active() {
            return STATUS_INVALID_PARAMETER;
        }

        let size = pf.size.load(Ordering::Relaxed);
        if offset as usize >= size {
            return STATUS_INVALID_PARAMETER;
        }

        // TODO: Реальное I/O через IO Manager
        // Для in-memory pagefile или тестирования — заполняем нулями
        if (pf.flags & pagefile_flags::PAGEFILE_IN_MEMORY) != 0 {
            // In-memory pagefile: данные должны храниться где-то
            // В текущей реализации просто обнуляем
            core::ptr::write_bytes(destination, 0, PAGE_SIZE);
        } else {
            // Требуется реальный I/O
            // let file_offset = offset * PAGE_SIZE as u64;
            // IoReadFile(pf.file_object, destination, PAGE_SIZE, file_offset);
            core::ptr::write_bytes(destination, 0, PAGE_SIZE);
        }

        STATUS_SUCCESS
    }
}

/// MiWritePageToPagefile
///
/// Записывает страницу в pagefile.
///
/// # Arguments
/// * `pagefile_index` - индекс pagefile
/// * `offset` - смещение в pagefile (в страницах)
/// * `source` - адрес данных для записи
///
/// # Returns
/// NTSTATUS
///
/// # Текущее состояние
/// Заглушка — данные не сохраняются (для in-memory pagefile это "OK").
pub unsafe fn mi_write_page_to_pagefile(
    pagefile_index: u8,
    offset: u64,
    _source: *const u8,
) -> NTSTATUS {
    unsafe {
        let files = &*MM_PAGING_FILE.get();

        if pagefile_index as usize >= MM_MAX_PAGING_FILES {
            return STATUS_INVALID_PARAMETER;
        }

        let pf = &files[pagefile_index as usize];

        if !pf.is_active() {
            return STATUS_INVALID_PARAMETER;
        }

        let size = pf.size.load(Ordering::Relaxed);
        if offset as usize >= size {
            return STATUS_INVALID_PARAMETER;
        }

        // TODO: Реальное I/O через IO Manager
        // Для in-memory pagefile — данные просто "записаны" (noop)
        // Для real pagefile:
        // let file_offset = offset * PAGE_SIZE as u64;
        // IoWriteFile(pf.file_object, source, PAGE_SIZE, file_offset);

        STATUS_SUCCESS
    }
}

// =============================================================================
// Initialization
// =============================================================================

/// Инициализирует подсистему pagefile
///
/// Должен вызываться при инициализации MM.
///
/// # Arguments
/// * `total_physical_pages` - общее количество физических страниц RAM
pub fn mi_initialize_pagefile_subsystem(total_physical_pages: usize) {
    // Начальный commit limit = RAM
    MM_TOTAL_COMMIT_LIMIT.store(total_physical_pages, Ordering::Release);

    // Minimum available commit = 10% RAM
    MM_MINIMUM_COMMIT_AVAILABLE.store(total_physical_pages / 10, Ordering::Release);
}

/// Создаёт in-memory pagefile (для тестирования или RAM-only систем)
///
/// Используется когда нет реального диска для pagefile.
pub unsafe fn mi_create_in_memory_pagefile(size_pages: usize) -> NTSTATUS {
    unsafe {
        let size = size_pages.clamp(MM_MIN_PAGEFILE_SIZE, MM_MAX_PAGEFILE_SIZE);
        nt_create_paging_file(ptr::null_mut(), size, size)
    }
}

// =============================================================================
// Query Functions
// =============================================================================

/// Возвращает текущий commit limit
#[inline]
pub fn mm_get_total_commit_limit() -> usize {
    MM_TOTAL_COMMIT_LIMIT.load(Ordering::Relaxed)
}

/// Возвращает текущий committed pages
#[inline]
pub fn mm_get_total_committed_pages() -> usize {
    MM_TOTAL_COMMITTED_PAGES.load(Ordering::Relaxed)
}

/// Возвращает доступный commit (limit - committed)
#[inline]
pub fn mm_get_available_commit() -> usize {
    let limit = MM_TOTAL_COMMIT_LIMIT.load(Ordering::Relaxed);
    let committed = MM_TOTAL_COMMITTED_PAGES.load(Ordering::Relaxed);
    limit.saturating_sub(committed)
}

/// Возвращает пиковый commit
#[inline]
pub fn mm_get_peak_commitment() -> usize {
    MM_PEAK_COMMITMENT.load(Ordering::Relaxed)
}

/// Возвращает количество pagefile'ов
#[inline]
pub fn mm_get_number_of_paging_files() -> usize {
    MM_NUMBER_OF_PAGING_FILES.load(Ordering::Relaxed)
}

/// Возвращает общий размер pagefile'ов
#[inline]
pub fn mm_get_pagefile_size() -> usize {
    MM_PAGEFILE_SIZE.load(Ordering::Relaxed)
}

// =============================================================================
// PTE Encoding/Decoding for Pagefile
// =============================================================================

/// Кодирует pagefile информацию в software PTE
///
/// Format:
/// [63:12] - Pagefile offset (страницы)
/// [11:8]  - Pagefile index
/// [7:5]   - Protection
/// [4]     - Reserved
/// [3]     - Pagefile PTE marker
/// [2:1]   - Reserved
/// [0]     - Valid (always 0)
pub fn mi_encode_pagefile_pte(pagefile_index: u8, offset: u64, protection: u32) -> u64 {
    let pf_idx = (pagefile_index as u64 & 0xF) << 8;
    let pf_offset = offset << 12;
    let prot = ((protection as u64) & 0x7) << 5;
    let marker = 0x8u64; // Bit 3 = pagefile PTE

    pf_offset | pf_idx | prot | marker
}

/// Декодирует pagefile информацию из software PTE
///
/// # Returns
/// (pagefile_index, offset, protection)
pub fn mi_decode_pagefile_pte(pte_value: u64) -> (u8, u64, u32) {
    let pf_idx = ((pte_value >> 8) & 0xF) as u8;
    let offset = pte_value >> 12;
    let protection = ((pte_value >> 5) & 0x7) as u32;

    (pf_idx, offset, protection)
}

/// Проверяет, является ли PTE pagefile PTE
#[inline]
pub fn mi_is_pagefile_pte(pte_value: u64) -> bool {
    // Valid = 0, Bit 3 = 1
    (pte_value & 0x9) == 0x8
}

