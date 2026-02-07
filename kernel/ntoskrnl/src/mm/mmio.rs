//! MMIO Mapping - Memory Mapped I/O
//!
//! Общие функции для маппинга MMIO устройств.
//!
//! Для HAL MMIO (APIC, IOAPIC) используйте модуль hal_mmio.rs
//! который предоставляет статические page tables без зависимости от page allocator.
//!
//! Источники:
//! - NT5: mm/amd64/miamd.c
//! - ReactOS: mm/ARM3/iosup.c

#![allow(dead_code)]

use super::init::mm_is_self_mapping_initialized;
use super::pte::PTE_DISABLE_CACHE;
use super::pte::PTE_READWRITE;
use super::pte::PTE_VALID;
use super::pte::PTE_WRITETHROUGH;
use super::pte::mm_get_pde_address;
use super::pte::mm_get_ppe_address;
use super::pte::mm_get_pte_address;
use super::pte::mm_get_pxe_address;
use super::types::PAGE_SIZE;
use crate::arch::x86_64::cpu::invlpg;

// =============================================================================
// Caching Types
// =============================================================================

/// Memory caching type для MMIO mapping
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MmCachingType {
    /// Нормальное кеширование (Write-Back)
    MmCached = 0,
    /// Некешируемая память (для MMIO)
    MmNonCached = 1,
    /// Write-Combined (для framebuffer)
    MmWriteCombined = 2,
}

// =============================================================================
// MMIO Mapping Functions
// =============================================================================

/// MmMapIoSpace - маппит физическую память на указанный виртуальный адрес
///
/// Используется для MMIO устройств (APIC, IOAPIC, etc.)
///
/// # Arguments
/// * `physical_address` - физический адрес для маппинга
/// * `size` - размер региона в байтах
/// * `virtual_address` - целевой виртуальный адрес
/// * `caching_type` - тип кеширования (обычно MmNonCached для MMIO)
///
/// # Returns
/// true если маппинг успешен
///
/// # Safety
/// - Требует инициализированный self-referencing PML4
/// - virtual_address должен быть в kernel space и не конфликтовать с другими маппингами
pub unsafe fn mm_map_io_space(
    physical_address: u64,
    size: u64,
    virtual_address: u64,
    caching_type: MmCachingType,
) -> bool {
    unsafe {
        // Проверяем что self-referencing инициализирован
        if !mm_is_self_mapping_initialized() {
            return false;
        }

        // Выравниваем на границу страницы
        let phys_page = physical_address & !0xFFF;
        let virt_page = virtual_address & !0xFFF;
        let offset = (physical_address & 0xFFF) as usize;
        let num_pages = ((size as usize + offset + PAGE_SIZE - 1) / PAGE_SIZE) as u64;

        // Определяем флаги кеширования
        let cache_flags = match caching_type {
            MmCachingType::MmCached => 0,
            MmCachingType::MmNonCached => PTE_WRITETHROUGH | PTE_DISABLE_CACHE,
            MmCachingType::MmWriteCombined => PTE_WRITETHROUGH, // PAT would be better
        };

        // Маппим каждую страницу
        for i in 0..num_pages {
            let virt = virt_page + (i * PAGE_SIZE as u64);
            let phys = phys_page + (i * PAGE_SIZE as u64);

            // Убеждаемся что все уровни page table существуют
            if !ensure_page_table_hierarchy(virt) {
                return false;
            }

            // Получаем PTE для этого виртуального адреса
            let pte_ptr = mm_get_pte_address(virt);

            // Формируем PTE value
            let pte_value = phys | PTE_VALID | PTE_READWRITE | cache_flags;

            // Записываем PTE
            unsafe {
                core::ptr::write_volatile(pte_ptr as *mut u64, pte_value);
            }

            // Сбрасываем TLB для этой страницы
            invlpg(virt);
        }

        true
    }
}

/// MmUnmapIoSpace - размаппливает IO space
///
/// # Arguments
/// * `virtual_address` - виртуальный адрес для размаппинга
/// * `size` - размер региона в байтах
pub unsafe fn mm_unmap_io_space(virtual_address: u64, size: u64) {
    if !mm_is_self_mapping_initialized() {
        return;
    }

    let virt_page = virtual_address & !0xFFF;
    let offset = (virtual_address & 0xFFF) as usize;
    let num_pages = ((size as usize + offset + PAGE_SIZE - 1) / PAGE_SIZE) as u64;

    for i in 0..num_pages {
        let virt = virt_page + (i * PAGE_SIZE as u64);
        let pte_ptr = mm_get_pte_address(virt);

        // Очищаем PTE
        unsafe {
            core::ptr::write_volatile(pte_ptr as *mut u64, 0);
        }

        // Сбрасываем TLB
        invlpg(virt);
    }
}

// =============================================================================
// Page Table Hierarchy
// =============================================================================

/// Убеждается что все уровни page table существуют для адреса
///
/// Если какой-то уровень отсутствует, он будет создан.
///
/// # Returns
/// true если иерархия готова, false если не удалось создать
unsafe fn ensure_page_table_hierarchy(virtual_address: u64) -> bool {
    // Проверяем PXE (PML4E)
    let pxe_ptr = mm_get_pxe_address(virtual_address);
    let pxe = unsafe { core::ptr::read_volatile(pxe_ptr as *const u64) };

    if (pxe & PTE_VALID) == 0 {
        // PML4E не существует - для MMIO это ошибка конфигурации
        // Обычно kernel space уже настроен bootloader'ом
        return false;
    }

    // Проверяем PPE (PDPTE)
    let ppe_ptr = mm_get_ppe_address(virtual_address);
    let ppe = unsafe { core::ptr::read_volatile(ppe_ptr as *const u64) };

    if (ppe & PTE_VALID) == 0 {
        // PDPTE не существует
        return false;
    }

    // Проверяем PDE
    let pde_ptr = mm_get_pde_address(virtual_address);
    let pde = unsafe { core::ptr::read_volatile(pde_ptr as *const u64) };

    if (pde & PTE_VALID) == 0 {
        // PDE не существует - нужно выделить PT
        // Для простоты пока возвращаем false
        // В полной реализации здесь нужно выделить страницу для PT
        return false;
    }

    // Проверяем что это не large page (2MB)
    if (pde & (1 << 7)) != 0 {
        // Large page - нельзя создать 4KB mapping
        return false;
    }

    true
}
