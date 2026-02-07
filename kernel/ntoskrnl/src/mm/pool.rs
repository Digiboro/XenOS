//! Memory Pool (Mm level)
//!
//! Низкоуровневое управление пулами памяти.
//! Высокоуровневые функции ExAllocatePool находятся в ex/pool.rs
//!
//! Источники:
//! - ReactOS: mm/ARM3/pool.c, mm/ARM3/expool.c

use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;

use super::pfn::*;
use super::types::*;
use crate::nt::PVOID;

// =============================================================================
// Pool Types
// =============================================================================

/// Тип пула для Mm уровня
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MmPoolType {
    /// Non-paged pool (всегда в памяти)
    NonPagedPool = 0,
    /// Paged pool (может быть выгружен)
    PagedPool = 1,
    /// Non-paged pool must succeed
    NonPagedPoolMustSucceed = 2,
    /// Non-paged pool cache aligned
    NonPagedPoolCacheAligned = 4,
    /// Paged pool cache aligned
    PagedPoolCacheAligned = 5,
    /// Non-paged pool NX (no execute)
    NonPagedPoolNx = 512,
}

// =============================================================================
// Pool Statistics
// =============================================================================

/// Статистика NonPaged pool
pub static MM_ALLOCATED_NON_PAGED_POOL: AtomicUsize = AtomicUsize::new(0);
/// Пик NonPaged pool
pub static MM_PEAK_NON_PAGED_POOL_USAGE: AtomicUsize = AtomicUsize::new(0);
/// Статистика Paged pool
pub static MM_ALLOCATED_PAGED_POOL: AtomicUsize = AtomicUsize::new(0);
/// Пик Paged pool
pub static MM_PEAK_PAGED_POOL_USAGE: AtomicUsize = AtomicUsize::new(0);

// =============================================================================
// Pool Header
// =============================================================================

/// Заголовок блока пула
#[repr(C)]
pub struct POOL_HEADER {
    /// Предыдущий размер (в 16-байтных единицах)
    pub previous_size: u16,
    /// Индекс пула
    pub pool_index: u8,
    /// Тип блока
    pub block_type: u8,
    /// Размер блока (в 16-байтных единицах)
    pub block_size: u16,
    /// Тип пула
    pub pool_type: u16,
    /// Tag (4 символа)
    pub pool_tag: u32,
    /// Зарезервировано
    pub _reserved: u32,
}

impl POOL_HEADER {
    pub const SIZE: usize = 16;

    pub const fn new() -> Self {
        Self {
            previous_size: 0,
            pool_index: 0,
            block_type: 0,
            block_size: 0,
            pool_type: 0,
            pool_tag: 0,
            _reserved: 0,
        }
    }

    /// Размер в байтах
    #[inline]
    pub fn size_in_bytes(&self) -> usize {
        (self.block_size as usize) << 4 // * 16
    }
}

// =============================================================================
// Low-Level Pool Allocation
// =============================================================================

/// Выделяет страницы для NonPaged pool
///
/// # Arguments
/// * `pool_type` - тип пула
/// * `size_in_bytes` - размер в байтах
///
/// # Returns
/// Виртуальный адрес выделенной памяти или NULL при ошибке
///
/// # Safety
/// Размер должен быть >= PAGE_SIZE
pub unsafe fn mi_allocate_pool_pages(pool_type: MmPoolType, size_in_bytes: usize) -> PVOID {
    unsafe {
        let pages_needed = bytes_to_pages(size_in_bytes);

        // Для NonPaged pool выделяем физические страницы и маппим
        if pool_type == MmPoolType::NonPagedPool
            || pool_type == MmPoolType::NonPagedPoolNx
            || pool_type == MmPoolType::NonPagedPoolMustSucceed
            || pool_type == MmPoolType::NonPagedPoolCacheAligned
        {
            return mi_allocate_non_paged_pool_pages(pages_needed, pool_type);
        }

        // Для PagedPool — выделяем VA и создаём demand-zero PTEs
        if pool_type == MmPoolType::PagedPool || pool_type == MmPoolType::PagedPoolCacheAligned {
            return mi_allocate_paged_pool_pages(pages_needed);
        }

        core::ptr::null_mut()
    }
}

/// Выделяет страницы для NonPaged pool
unsafe fn mi_allocate_non_paged_pool_pages(pages: usize, pool_type: MmPoolType) -> PVOID {
    unsafe {
        // Проверяем доступность физической памяти
        let available = mm_get_available_pages();
        if available < pages {
            return core::ptr::null_mut();
        }

        // Резервируем VA в system PTE region
        let va = super::syspte::mi_reserve_system_ptes(pages);
        if va == 0 {
            return core::ptr::null_mut();
        }

        // Выделяем физические страницы и маппим одновременно
        let nx = pool_type == MmPoolType::NonPagedPoolNx;
        let mut allocated = 0;

        {
            let _lock = crate::ke::spinlock::KSpinLockGuard::new(MM_PFN_LOCK.get());

            for i in 0..pages {
                if let Some(pfn) = mi_allocate_pfn() {
                    let page_va = va + (i * PAGE_SIZE) as u64;
                    mi_map_pool_page(page_va, pfn, !nx);
                    allocated += 1;
                } else {
                    // Не хватило страниц — откатываем
                    for j in 0..allocated {
                        let page_va = va + (j * PAGE_SIZE) as u64;
                        let pte_ptr = super::pte::mm_get_pte_address(page_va) as *mut u64;
                        let pte_value = core::ptr::read_volatile(pte_ptr);
                        if (pte_value & super::pte::PTE_VALID) != 0 {
                            let pfn_to_free = ((pte_value >> 12) & 0xFFFFFFFFFF) as usize;
                            mi_free_pfn(pfn_to_free);
                        }
                        core::ptr::write_volatile(pte_ptr, 0);
                    }
                    super::syspte::mi_release_system_ptes(va, pages);
                    return core::ptr::null_mut();
                }
            }
        }

        // Обновляем статистику
        mi_update_pool_stats_alloc(pool_type, pages * PAGE_SIZE);

        va as PVOID
    }
}

/// Маппит страницу пула
unsafe fn mi_map_pool_page(va: u64, pfn: usize, execute: bool) {
    unsafe {
        let pte_ptr = super::pte::mm_get_pte_address(va) as *mut u64;

        let mut flags = super::pte::PTE_VALID | super::pte::PTE_READWRITE;
        if !execute {
            flags |= super::pte::PTE_NX;
        }

        let pte_value = ((pfn as u64) << 12) | flags;
        core::ptr::write_volatile(pte_ptr, pte_value);
        crate::arch::x86_64::cpu::invlpg(va);
    }
}

/// Выделяет страницы для Paged pool (demand-zero)
unsafe fn mi_allocate_paged_pool_pages(pages: usize) -> PVOID {
    unsafe {
        // Проверяем commit
        if !super::pagefile::mi_charge_commitment(pages, core::ptr::null_mut()) {
            return core::ptr::null_mut();
        }

        // Резервируем VA в paged pool region
        let va = mi_reserve_paged_pool_va(pages);
        if va == 0 {
            super::pagefile::mi_return_commitment(pages, core::ptr::null_mut());
            return core::ptr::null_mut();
        }

        // Создаём demand-zero PTEs
        for i in 0..pages {
            let page_va = va + (i * PAGE_SIZE) as u64;
            let pte_ptr = super::pte::mm_get_pte_address(page_va) as *mut u64;

            // Demand-zero PTE: valid=0, demand_zero marker
            let pte_value = super::pagefault::pte_state::DEMAND_ZERO;
            core::ptr::write_volatile(pte_ptr, pte_value);
        }

        // Обновляем статистику
        mi_update_pool_stats_alloc(MmPoolType::PagedPool, pages * PAGE_SIZE);

        va as PVOID
    }
}

/// Резервирует VA для paged pool
///
/// Использует простой bump allocator в paged pool region
static PAGED_POOL_NEXT_VA: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

unsafe fn mi_reserve_paged_pool_va(pages: usize) -> u64 {
    unsafe {
        let start = super::init::MM_PAGED_POOL_START.load(Ordering::Acquire) as u64;
        let end = super::init::MM_PAGED_POOL_END.load(Ordering::Acquire) as u64;

        // Инициализируем если первый вызов
        let _ = PAGED_POOL_NEXT_VA.compare_exchange(0, start, Ordering::AcqRel, Ordering::Acquire);

        loop {
            let current = PAGED_POOL_NEXT_VA.load(Ordering::Acquire);
            let new_va = current + (pages * PAGE_SIZE) as u64;

            if new_va > end {
                return 0;
            }

            if PAGED_POOL_NEXT_VA
                .compare_exchange(current, new_va, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return current;
            }
        }
    }
}

/// Освобождает страницы пула
///
/// # Arguments
/// * `address` - адрес, возвращённый mi_allocate_pool_pages
/// * `size_in_bytes` - размер в байтах (нужен для освобождения)
pub unsafe fn mi_free_pool_pages(address: PVOID, size_in_bytes: usize) {
    unsafe {
        if address.is_null() {
            return;
        }

        let va = address as u64;
        let pages = bytes_to_pages(size_in_bytes);

        // Определяем тип пула по адресу
        if mi_is_non_paged_pool_address(address) || mi_is_system_pte_address(va) {
            mi_free_non_paged_pool_pages(va, pages);
        } else if mi_is_paged_pool_address(address) {
            mi_free_paged_pool_pages(va, pages);
        }
    }
}

/// Проверяет, является ли адрес system PTE адресом
fn mi_is_system_pte_address(va: u64) -> bool {
    va >= super::types::MM_SYSTEM_PTE_START && va < super::types::MM_SYSTEM_PTE_END
}

/// Освобождает NonPaged pool страницы
unsafe fn mi_free_non_paged_pool_pages(va: u64, pages: usize) {
    unsafe {
        // Собираем PFN и размаппим
        {
            let _lock = crate::ke::spinlock::KSpinLockGuard::new(MM_PFN_LOCK.get());

            for i in 0..pages {
                let page_va = va + (i * PAGE_SIZE) as u64;
                let pte_ptr = super::pte::mm_get_pte_address(page_va) as *mut u64;
                let pte_value = core::ptr::read_volatile(pte_ptr);

                if (pte_value & super::pte::PTE_VALID) != 0 {
                    let pfn = ((pte_value >> 12) & 0xFFFFFFFFFF) as usize;
                    mi_free_pfn(pfn);
                }

                // Очищаем PTE
                core::ptr::write_volatile(pte_ptr, 0);
                crate::arch::x86_64::cpu::invlpg(page_va);
            }
        }

        // Освобождаем VA
        super::syspte::mi_release_system_ptes(va, pages);

        // Обновляем статистику
        mi_update_pool_stats_free(MmPoolType::NonPagedPool, pages * PAGE_SIZE);
    }
}

/// Освобождает Paged pool страницы
unsafe fn mi_free_paged_pool_pages(va: u64, pages: usize) {
    unsafe {
        // Освобождаем физические страницы если есть
        {
            let _lock = crate::ke::spinlock::KSpinLockGuard::new(MM_PFN_LOCK.get());

            for i in 0..pages {
                let page_va = va + (i * PAGE_SIZE) as u64;
                let pte_ptr = super::pte::mm_get_pte_address(page_va) as *mut u64;
                let pte_value = core::ptr::read_volatile(pte_ptr);

                if (pte_value & super::pte::PTE_VALID) != 0 {
                    let pfn = ((pte_value >> 12) & 0xFFFFFFFFFF) as usize;
                    mi_free_pfn(pfn);
                }

                // Очищаем PTE
                core::ptr::write_volatile(pte_ptr, 0);
                crate::arch::x86_64::cpu::invlpg(page_va);
            }
        }

        // Возвращаем commit
        super::pagefile::mi_return_commitment(pages, core::ptr::null_mut());

        // Обновляем статистику
        mi_update_pool_stats_free(MmPoolType::PagedPool, pages * PAGE_SIZE);

        // TODO: Освободить VA в paged pool для переиспользования
        // Текущая реализация не переиспользует VA
    }
}

/// Выделяет большой блок NonPaged pool с выравниванием
///
/// Используется для DMA буферов и т.п.
pub unsafe fn mi_allocate_contiguous_pool_pages(
    pages: usize,
    alignment: usize,
) -> PVOID {
    unsafe {
        if pages == 0 {
            return core::ptr::null_mut();
        }

        // Проверяем доступность
        let available = mm_get_available_pages();
        if available < pages {
            return core::ptr::null_mut();
        }

        // Резервируем VA с выравниванием
        let alignment_pages = (alignment + PAGE_SIZE - 1) / PAGE_SIZE;
        let va = super::syspte::mi_reserve_system_ptes_contiguous(pages, alignment_pages.max(1));
        if va == 0 {
            return core::ptr::null_mut();
        }

        // Выделяем физические страницы и маппим одновременно
        let mut allocated = 0;

        {
            let _lock = crate::ke::spinlock::KSpinLockGuard::new(MM_PFN_LOCK.get());

            for i in 0..pages {
                if let Some(pfn) = mi_allocate_pfn() {
                    let page_va = va + (i * PAGE_SIZE) as u64;
                    mi_map_pool_page(page_va, pfn, false);
                    allocated += 1;
                } else {
                    // Откатываем
                    for j in 0..allocated {
                        let page_va = va + (j * PAGE_SIZE) as u64;
                        let pte_ptr = super::pte::mm_get_pte_address(page_va) as *mut u64;
                        let pte_value = core::ptr::read_volatile(pte_ptr);
                        if (pte_value & super::pte::PTE_VALID) != 0 {
                            let pfn_to_free = ((pte_value >> 12) & 0xFFFFFFFFFF) as usize;
                            mi_free_pfn(pfn_to_free);
                        }
                        core::ptr::write_volatile(pte_ptr, 0);
                    }
                    super::syspte::mi_release_system_ptes(va, pages);
                    return core::ptr::null_mut();
                }
            }
        }

        mi_update_pool_stats_alloc(MmPoolType::NonPagedPool, pages * PAGE_SIZE);

        va as PVOID
    }
}

// =============================================================================
// Pool Verification
// =============================================================================

/// Проверяет, принадлежит ли адрес NonPaged pool
#[inline]
pub fn mi_is_non_paged_pool_address(address: PVOID) -> bool {
    let addr = address as usize;
    let start = super::init::MM_NON_PAGED_POOL_START.load(Ordering::Acquire);
    let end = super::init::MM_NON_PAGED_POOL_END.load(Ordering::Acquire);

    addr >= start && addr < end
}

/// Проверяет, принадлежит ли адрес Paged pool
#[inline]
pub fn mi_is_paged_pool_address(address: PVOID) -> bool {
    let addr = address as usize;
    let start = super::init::MM_PAGED_POOL_START.load(Ordering::Acquire);
    let end = super::init::MM_PAGED_POOL_END.load(Ordering::Acquire);

    addr >= start && addr < end
}

/// Проверяет, принадлежит ли адрес какому-либо пулу
#[inline]
pub fn mi_is_pool_address(address: PVOID) -> bool {
    mi_is_non_paged_pool_address(address) || mi_is_paged_pool_address(address)
}

// =============================================================================
// Pool Statistics Functions
// =============================================================================

/// Возвращает использование NonPaged pool
#[inline]
pub fn mm_get_non_paged_pool_usage() -> usize {
    MM_ALLOCATED_NON_PAGED_POOL.load(Ordering::Acquire)
}

/// Возвращает использование Paged pool
#[inline]
pub fn mm_get_paged_pool_usage() -> usize {
    MM_ALLOCATED_PAGED_POOL.load(Ordering::Acquire)
}

/// Обновляет статистику после выделения
pub fn mi_update_pool_stats_alloc(pool_type: MmPoolType, size: usize) {
    if pool_type == MmPoolType::NonPagedPool || pool_type == MmPoolType::NonPagedPoolNx {
        let new_usage = MM_ALLOCATED_NON_PAGED_POOL.fetch_add(size, Ordering::AcqRel) + size;

        // Обновляем пик
        let mut peak = MM_PEAK_NON_PAGED_POOL_USAGE.load(Ordering::Acquire);
        while new_usage > peak {
            match MM_PEAK_NON_PAGED_POOL_USAGE.compare_exchange(
                peak,
                new_usage,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(p) => peak = p,
            }
        }
    } else {
        let new_usage = MM_ALLOCATED_PAGED_POOL.fetch_add(size, Ordering::AcqRel) + size;

        let mut peak = MM_PEAK_PAGED_POOL_USAGE.load(Ordering::Acquire);
        while new_usage > peak {
            match MM_PEAK_PAGED_POOL_USAGE.compare_exchange(
                peak,
                new_usage,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(p) => peak = p,
            }
        }
    }
}

/// Обновляет статистику после освобождения
pub fn mi_update_pool_stats_free(pool_type: MmPoolType, size: usize) {
    if pool_type == MmPoolType::NonPagedPool || pool_type == MmPoolType::NonPagedPoolNx {
        MM_ALLOCATED_NON_PAGED_POOL.fetch_sub(size, Ordering::AcqRel);
    } else {
        MM_ALLOCATED_PAGED_POOL.fetch_sub(size, Ordering::AcqRel);
    }
}
