//! Pool Allocator - аллокатор памяти ядра
//!
//! Источники:
//! - NT5: mm/pool.c, mm/poolsup.c
//! - ReactOS: mm/ARM3/expool.c

#![allow(dead_code)]
#![allow(non_camel_case_types)]

use core::alloc::GlobalAlloc;
use core::alloc::Layout;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;

use crate::ke::spinlock::KSPIN_LOCK;
use crate::nt::LIST_ENTRY;
use crate::nt::PVOID;
use crate::nt::ULONG;

/// Тип пула памяти
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum POOL_TYPE {
    /// NonPagedPool - не выгружаемая память (доступна на любом IRQL)
    NonPagedPool = 0,
    /// PagedPool - выгружаемая память (только PASSIVE_LEVEL)
    PagedPool = 1,
    /// NonPagedPoolMustSucceed - критическая невыгружаемая (deprecated)
    NonPagedPoolMustSucceed = 2,
    /// DontUseThisType
    DontUseThisType = 3,
    /// NonPagedPoolCacheAligned - выровненная по кэш-линии
    NonPagedPoolCacheAligned = 4,
    /// PagedPoolCacheAligned
    PagedPoolCacheAligned = 5,
    /// NonPagedPoolCacheAlignedMustS
    NonPagedPoolCacheAlignedMustS = 6,
    /// MaxPoolType
    MaxPoolType = 7,
    /// NonPagedPoolSession
    NonPagedPoolSession = 32,
    /// PagedPoolSession
    PagedPoolSession = 33,
}

impl POOL_TYPE {
    /// Проверяет является ли пул paged
    #[inline]
    pub fn is_paged(self) -> bool {
        matches!(
            self,
            POOL_TYPE::PagedPool | POOL_TYPE::PagedPoolCacheAligned | POOL_TYPE::PagedPoolSession
        )
    }

    /// Проверяет требуется ли выравнивание по кэш-линии
    #[inline]
    pub fn is_cache_aligned(self) -> bool {
        matches!(
            self,
            POOL_TYPE::NonPagedPoolCacheAligned
                | POOL_TYPE::PagedPoolCacheAligned
                | POOL_TYPE::NonPagedPoolCacheAlignedMustS
        )
    }
}

/// Pool tag для отслеживания аллокаций
pub type POOL_TAG = u32;

/// Размер кэш-линии
pub const CACHE_LINE_SIZE: usize = 64;

/// Минимальный размер блока
pub const POOL_SMALLEST_BLOCK: usize = 8;

/// Максимальный размер для small pool
pub const POOL_SMALL_LISTS: usize = 32;

/// Гранулярность пула
pub const POOL_BLOCK_SHIFT: usize = 4; // 16 байт
pub const POOL_BLOCK_SIZE: usize = 1 << POOL_BLOCK_SHIFT;

/// Заголовок блока пула
#[repr(C)]
pub struct POOL_HEADER {
    /// Предыдущий размер (в блоках)
    pub previous_size: u8,
    /// Индекс пула
    pub pool_index: u8,
    /// Тип блока
    pub block_size: u8,
    /// Тип пула
    pub pool_type: u8,
    /// Tag для отладки
    pub pool_tag: POOL_TAG,
}

impl POOL_HEADER {
    pub const fn new() -> Self {
        Self {
            previous_size: 0,
            pool_index: 0,
            block_size: 0,
            pool_type: 0,
            pool_tag: 0,
        }
    }
}

/// Free list entry
#[repr(C)]
pub struct POOL_FREE_ENTRY {
    pub list_entry: LIST_ENTRY,
    pub block_size: ULONG,
}

/// Pool descriptor
pub struct POOL_DESCRIPTOR {
    /// Тип пула
    pub pool_type: POOL_TYPE,
    /// Спинлок для синхронизации
    pub lock: KSPIN_LOCK,
    /// Общее количество страниц
    pub total_pages: AtomicUsize,
    /// Использованные байты
    pub total_bytes: AtomicUsize,
    /// Количество аллокаций
    pub run_count: AtomicUsize,
    /// Free lists по размерам
    pub list_heads: [LIST_ENTRY; POOL_SMALL_LISTS],
}

impl POOL_DESCRIPTOR {
    pub const fn new(pool_type: POOL_TYPE) -> Self {
        Self {
            pool_type,
            lock: KSPIN_LOCK::new(),
            total_pages: AtomicUsize::new(0),
            total_bytes: AtomicUsize::new(0),
            run_count: AtomicUsize::new(0),
            list_heads: [LIST_ENTRY::new(); POOL_SMALL_LISTS],
        }
    }

    /// Инициализирует free lists
    ///
    /// # Safety
    /// Должен вызываться один раз при инициализации
    pub unsafe fn init(&mut self) {
        unsafe {
            for list in &mut self.list_heads {
                LIST_ENTRY::init_head(list as *mut LIST_ENTRY);
            }
        }
    }
}

// =============================================================================
// Глобальные дескрипторы пулов
// =============================================================================

use crate::ke::init::GlobalData;

/// NonPagedPool descriptor
static NONPAGED_POOL_DESCRIPTOR: GlobalData<POOL_DESCRIPTOR> =
    GlobalData::new(POOL_DESCRIPTOR::new(POOL_TYPE::NonPagedPool));

/// PagedPool descriptor
static PAGED_POOL_DESCRIPTOR: GlobalData<POOL_DESCRIPTOR> =
    GlobalData::new(POOL_DESCRIPTOR::new(POOL_TYPE::PagedPool));

// =============================================================================
// Простой bump allocator для начальной стадии
// =============================================================================

/// Начальный размер heap (1MB)
const INITIAL_HEAP_SIZE: usize = 1024 * 1024;

/// Простой bump allocator
pub struct BumpAllocator {
    heap_start: AtomicUsize,
    heap_end: AtomicUsize,
    next: AtomicUsize,
    allocations: AtomicUsize,
}

impl BumpAllocator {
    pub const fn new() -> Self {
        Self {
            heap_start: AtomicUsize::new(0),
            heap_end: AtomicUsize::new(0),
            next: AtomicUsize::new(0),
            allocations: AtomicUsize::new(0),
        }
    }

    /// Инициализирует аллокатор
    ///
    /// # Safety
    /// Регион памяти должен быть валиден и не использоваться
    pub unsafe fn init(&self, heap_start: usize, heap_size: usize) {
        self.heap_start.store(heap_start, Ordering::SeqCst);
        self.heap_end
            .store(heap_start + heap_size, Ordering::SeqCst);
        self.next.store(heap_start, Ordering::SeqCst);
    }

    /// Проверяет инициализирован ли аллокатор
    pub fn is_initialized(&self) -> bool {
        self.heap_start.load(Ordering::SeqCst) != 0
    }

    /// Выделяет память
    pub fn allocate(&self, size: usize, align: usize) -> Option<*mut u8> {
        loop {
            let current = self.next.load(Ordering::SeqCst);
            let aligned = (current + align - 1) & !(align - 1);
            let new_next = aligned + size;

            if new_next > self.heap_end.load(Ordering::SeqCst) {
                return None;
            }

            if self
                .next
                .compare_exchange(current, new_next, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                self.allocations.fetch_add(1, Ordering::SeqCst);
                return Some(aligned as *mut u8);
            }
        }
    }

    /// Освобождает память (bump allocator не поддерживает освобождение)
    pub fn deallocate(&self, _ptr: *mut u8, _size: usize) {
        // Bump allocator doesn't support deallocation
        // This is fine for early boot stage
    }

    /// Возвращает статистику
    pub fn stats(&self) -> (usize, usize, usize) {
        let start = self.heap_start.load(Ordering::SeqCst);
        let end = self.heap_end.load(Ordering::SeqCst);
        let next = self.next.load(Ordering::SeqCst);
        (
            next - start,
            end - start,
            self.allocations.load(Ordering::SeqCst),
        )
    }
}

/// Глобальный bump allocator
static BUMP_ALLOCATOR: BumpAllocator = BumpAllocator::new();

// =============================================================================
// Pool API
// =============================================================================

/// ExAllocatePool - выделяет память из пула
///
/// # Arguments
/// * `pool_type` - тип пула (NonPaged/Paged)
/// * `number_of_bytes` - размер в байтах
///
/// # Returns
/// Указатель на выделенную память или NULL
pub fn ex_allocate_pool(pool_type: POOL_TYPE, number_of_bytes: usize) -> PVOID {
    ex_allocate_pool_with_tag(pool_type, number_of_bytes, b"None"[0] as u32)
}

/// ExAllocatePoolWithTag - выделяет память с тегом
pub fn ex_allocate_pool_with_tag(
    pool_type: POOL_TYPE,
    number_of_bytes: usize,
    tag: POOL_TAG,
) -> PVOID {
    if number_of_bytes == 0 {
        return core::ptr::null_mut();
    }

    // Используем NT pool если он инициализирован
    if super::pool_nt::nt_pool_enabled() {
        let result = super::pool_nt::nt_pool_allocate(pool_type, number_of_bytes, tag);
        return result;
    }

    // Fallback на bump allocator для раннего boot

    // Вычисляем выравнивание
    let align = if pool_type.is_cache_aligned() {
        CACHE_LINE_SIZE
    } else {
        core::mem::align_of::<usize>()
    };

    // Добавляем место для заголовка
    let total_size = number_of_bytes + core::mem::size_of::<POOL_HEADER>();

    // Используем bump allocator
    let ptr = match BUMP_ALLOCATOR.allocate(total_size, align) {
        Some(p) => p,
        None => return core::ptr::null_mut(),
    };

    // Заполняем заголовок
    let header = ptr as *mut POOL_HEADER;
    unsafe {
        (*header).pool_type = pool_type as u8;
        (*header).pool_tag = tag;
        (*header).block_size = ((total_size + POOL_BLOCK_SIZE - 1) / POOL_BLOCK_SIZE) as u8;
        (*header).previous_size = 0;
        (*header).pool_index = 0;
    }

    // Обновляем статистику
    let descriptor = if pool_type.is_paged() {
        unsafe { &*PAGED_POOL_DESCRIPTOR.get() }
    } else {
        unsafe { &*NONPAGED_POOL_DESCRIPTOR.get() }
    };
    descriptor
        .total_bytes
        .fetch_add(number_of_bytes, Ordering::SeqCst);
    descriptor.run_count.fetch_add(1, Ordering::SeqCst);

    // Результат - указатель после заголовка
    let result = unsafe { ptr.add(core::mem::size_of::<POOL_HEADER>()) as PVOID };

    result
}

/// ExAllocatePoolWithQuotaTag - выделяет память с квотой
pub fn ex_allocate_pool_with_quota_tag(
    pool_type: POOL_TYPE,
    number_of_bytes: usize,
    tag: POOL_TAG,
) -> PVOID {
    // Пока без учета квот
    ex_allocate_pool_with_tag(pool_type, number_of_bytes, tag)
}

/// ExFreePool - освобождает память
pub fn ex_free_pool(ptr: PVOID) {
    ex_free_pool_with_tag(ptr, 0)
}

/// ExFreePoolWithTag - освобождает память с проверкой тега
pub fn ex_free_pool_with_tag(ptr: PVOID, tag: POOL_TAG) {
    if ptr.is_null() {
        return;
    }

    // Используем NT pool если он инициализирован
    if super::pool_nt::nt_pool_enabled() {
        super::pool_nt::nt_pool_free(ptr, tag);
        return;
    }

    // Fallback для bump allocator

    // Получаем заголовок
    let header =
        unsafe { (ptr as *mut u8).sub(core::mem::size_of::<POOL_HEADER>()) as *mut POOL_HEADER };

    // Проверяем тег если указан
    if tag != 0 {
        unsafe {
            if (*header).pool_tag != tag {
                // Tag mismatch - потенциальный баг
                // В production это может быть bugcheck
                return;
            }
        }
    }

    let pool_type = unsafe { (*header).pool_type };
    let block_size = unsafe { (*header).block_size as usize * POOL_BLOCK_SIZE };

    // Обновляем статистику
    let descriptor = if pool_type == POOL_TYPE::PagedPool as u8 {
        unsafe { &*PAGED_POOL_DESCRIPTOR.get() }
    } else {
        unsafe { &*NONPAGED_POOL_DESCRIPTOR.get() }
    };
    descriptor.total_bytes.fetch_sub(
        block_size.saturating_sub(core::mem::size_of::<POOL_HEADER>()),
        Ordering::SeqCst,
    );

    // Bump allocator не поддерживает освобождение
    // Память будет переиспользована только после перезагрузки
    BUMP_ALLOCATOR.deallocate(header as *mut u8, block_size);
}

/// ExQueryPoolUsage - возвращает использование пула
pub fn ex_query_pool_usage(pool_type: POOL_TYPE) -> (usize, usize) {
    let descriptor = if pool_type.is_paged() {
        unsafe { &*PAGED_POOL_DESCRIPTOR.get() }
    } else {
        unsafe { &*NONPAGED_POOL_DESCRIPTOR.get() }
    };

    (
        descriptor.total_bytes.load(Ordering::SeqCst),
        descriptor.run_count.load(Ordering::SeqCst),
    )
}

// =============================================================================
// Инициализация
// =============================================================================

/// Инициализирует pool subsystem
///
/// # Safety
/// Должен вызываться один раз при старте системы
/// heap_start должен указывать на валидную область памяти
pub unsafe fn exp_init_pool_system(heap_start: usize, heap_size: usize) {
    unsafe {
        // Инициализируем bump allocator
        BUMP_ALLOCATOR.init(heap_start, heap_size);

        // Инициализируем дескрипторы
        (*NONPAGED_POOL_DESCRIPTOR.get()).init();
        (*PAGED_POOL_DESCRIPTOR.get()).init();
    }
}

/// Проверяет инициализирован ли pool
pub fn exp_pool_initialized() -> bool {
    BUMP_ALLOCATOR.is_initialized()
}

// =============================================================================
// Global Allocator trait (для интеграции с Rust)
// =============================================================================

/// Kernel allocator для #[global_allocator]
pub struct KernelAllocator;

unsafe impl GlobalAlloc for KernelAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = ex_allocate_pool_with_tag(
            POOL_TYPE::NonPagedPool,
            layout.size(),
            u32::from_le_bytes(*b"Rust"),
        );
        ptr as *mut u8
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        ex_free_pool(ptr as PVOID);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        unsafe {
            // Выделяем новый блок
            let new_ptr = self.alloc(Layout::from_size_align_unchecked(new_size, layout.align()));
            if new_ptr.is_null() {
                return core::ptr::null_mut();
            }

            // Копируем данные
            if !ptr.is_null() {
                core::ptr::copy_nonoverlapping(ptr, new_ptr, layout.size().min(new_size));
                self.dealloc(ptr, layout);
            }

            new_ptr
        }
    }
}

/// Статистика bump allocator
pub fn bump_allocator_stats() -> (usize, usize, usize) {
    BUMP_ALLOCATOR.stats()
}
