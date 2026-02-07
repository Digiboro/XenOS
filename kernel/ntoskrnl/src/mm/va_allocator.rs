//! Virtual Address Allocator для MMIO space
//!
//! Управляет выделением виртуальных адресов для MmMapIoSpace.
//!
//! Диапазон: 0xFFFFF80000000000 - 0xFFFFFFFF7FFFFFFF (System PTE space)

use core::sync::atomic::{AtomicU64, Ordering};
use crate::ke::spinlock::KSPIN_LOCK;

// =============================================================================
// MMIO VA Range
// =============================================================================

/// Начало MMIO VA space (System PTE range)
/// Windows NT использует System PTE space для dynamic mappings
const MMIO_VA_START: u64 = 0xFFFFF80000000000;

/// Конец MMIO VA space (до HAL VA range)
const MMIO_VA_END: u64 = 0xFFFFFFFF7FFFFFFF;

/// Размер MMIO VA space (512 GB)
const MMIO_VA_SIZE: u64 = MMIO_VA_END - MMIO_VA_START + 1;

// =============================================================================
// Simple Bump Allocator
// =============================================================================

/// Текущий указатель в MMIO VA space
static MMIO_VA_NEXT: AtomicU64 = AtomicU64::new(MMIO_VA_START);

/// Spinlock для защиты аллокатора
static MMIO_VA_LOCK: KSPIN_LOCK = KSPIN_LOCK::new();

/// Выделяет виртуальный адрес для MMIO mapping
///
/// # Arguments
/// * `size` - размер в байтах (будет выровнен на PAGE_SIZE)
///
/// # Returns
/// Виртуальный адрес или 0 при ошибке
pub unsafe fn mm_allocate_mmio_va(size: u64) -> u64 {
    use crate::ke::spinlock::{ke_acquire_spin_lock, ke_release_spin_lock};
    use super::types::PAGE_SIZE;

    // Выравниваем размер на границу страницы
    let aligned_size = (size + (PAGE_SIZE as u64 - 1)) & !(PAGE_SIZE as u64 - 1);

    // Захватываем spinlock
    let old_irql = ke_acquire_spin_lock(&MMIO_VA_LOCK);

    // Получаем текущий указатель
    let current = MMIO_VA_NEXT.load(Ordering::SeqCst);
    let next = current + aligned_size;

    // Проверяем переполнение
    if next > MMIO_VA_END || next < current {
        ke_release_spin_lock(&MMIO_VA_LOCK, old_irql);
        return 0;
    }

    // Обновляем указатель
    MMIO_VA_NEXT.store(next, Ordering::SeqCst);

    ke_release_spin_lock(&MMIO_VA_LOCK, old_irql);

    current
}

/// Освобождает виртуальный адрес MMIO
///
/// В текущей реализации (bump allocator) освобождение не поддерживается.
/// Для полной реализации нужен bitmap allocator или AVL tree.
///
/// # Arguments
/// * `virtual_address` - виртуальный адрес для освобождения
/// * `size` - размер региона
pub unsafe fn mm_free_mmio_va(_virtual_address: u64, _size: u64) {
    // TODO: Реализовать bitmap allocator для переиспользования VA
    // Пока просто игнорируем - bump allocator не поддерживает free
}

/// Проверяет, находится ли адрес в MMIO VA range
pub fn is_mmio_va(virtual_address: u64) -> bool {
    virtual_address >= MMIO_VA_START && virtual_address <= MMIO_VA_END
}

/// Получает статистику использования MMIO VA space
pub fn mm_get_mmio_va_stats() -> (u64, u64, u64) {
    let next = MMIO_VA_NEXT.load(Ordering::Relaxed);
    let used = next - MMIO_VA_START;
    let free = MMIO_VA_END - next + 1;
    (MMIO_VA_SIZE, used, free)
}

