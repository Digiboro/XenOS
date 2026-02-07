//! HAL MMIO Region Management
//!
//! Статические page tables для HAL MMIO региона (APIC, IOAPIC, etc.)
//! на фиксированных NT-совместимых виртуальных адресах.
//!
//! Адресное пространство:
//! - 0xFFFFFFFFFFFE0000 - Local APIC (4KB)
//! - 0xFFFFFFFFFFFE1000 - IOAPIC #0 (4KB)
//! - 0xFFFFFFFFFFFE2000 - IOAPIC #1 (4KB)
//! - ...
//! - 0xFFFFFFFFFFFEF000 - IOAPIC #14 (до 15 IOAPIC)
//!
//! Для адреса 0xFFFFFFFFFFFE0000:
//! - PML4[511] - kernel space (существует)
//! - PDPT[511] - может отсутствовать
//! - PD[511] - может отсутствовать
//! - PT[480-511] - HAL MMIO страницы
//!
//! Источники:
//! - NT5: ntos/mm/amd64/miamd.c
//! - ReactOS: hal/halx86/apic/apic.c

use core::sync::atomic::AtomicBool;
use core::sync::atomic::AtomicU8;
use core::sync::atomic::Ordering;

use super::pte::PTE_DISABLE_CACHE;
use super::pte::PTE_FRAME_MASK;
use super::pte::PTE_READWRITE;
use super::pte::PTE_VALID;
use super::pte::PTE_WRITETHROUGH;

// =============================================================================
// HAL MMIO Address Constants (NT-compatible)
// =============================================================================

/// NT AMD64 фиксированный виртуальный адрес для Local APIC
pub const HAL_LOCAL_APIC_BASE: u64 = 0xFFFFFFFFFFFE0000;

/// NT AMD64 фиксированный виртуальный адрес для первого IO-APIC
pub const HAL_IOAPIC_BASE: u64 = 0xFFFFFFFFFFFE1000;

/// Максимальное количество IOAPIC (0-14, всего 15)
pub const HAL_MAX_IOAPIC_COUNT: usize = 15;

/// Размер страницы (4KB)
const PAGE_SIZE: u64 = 0x1000;

/// Базовый адрес HAL MMIO региона
const HAL_MMIO_REGION_BASE: u64 = 0xFFFFFFFFFFFE0000;

/// Размер HAL MMIO региона (32 страницы = 128KB)
const HAL_MMIO_REGION_SIZE: u64 = 32 * PAGE_SIZE;

// =============================================================================
// Page Table Indices for HAL_MMIO_REGION_BASE
// =============================================================================

/// PML4 index для HAL MMIO (511)
const PML4_INDEX: usize = ((HAL_MMIO_REGION_BASE >> 39) & 0x1FF) as usize;

/// PDPT index для HAL MMIO (511)
const PDPT_INDEX: usize = ((HAL_MMIO_REGION_BASE >> 30) & 0x1FF) as usize;

/// PD index для HAL MMIO (511)
const PD_INDEX: usize = ((HAL_MMIO_REGION_BASE >> 21) & 0x1FF) as usize;

/// PT index для Local APIC (480)
const PT_INDEX_BASE: usize = ((HAL_MMIO_REGION_BASE >> 12) & 0x1FF) as usize;

// =============================================================================
// Static Page Tables
// =============================================================================

/// Статические page tables для HAL MMIO региона
/// Выделяются в BSS секции - не требуют page allocator
#[repr(C, align(4096))]
struct HalMmioPageTables {
    /// Page Directory для PML4[511]/PDPT[511]
    pd: [u64; 512],
    /// Page Table для PD[511]
    pt: [u64; 512],
}

/// Глобальные статические page tables
static mut HAL_MMIO_TABLES: HalMmioPageTables = HalMmioPageTables {
    pd: [0; 512],
    pt: [0; 512],
};

/// Флаг инициализации HAL MMIO
static HAL_MMIO_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Количество зарегистрированных IOAPIC
static HAL_IOAPIC_COUNT: AtomicU8 = AtomicU8::new(0);

// =============================================================================
// Initialization
// =============================================================================

/// Инициализирует HAL MMIO region
///
/// Создает page table hierarchy для региона 0xFFFFFFFFFFFE0000-0xFFFFFFFFFFFFFFFF
/// используя HHDM для доступа к существующим page tables.
///
/// # Safety
/// - Должна вызываться после инициализации HHDM
/// - Должна вызываться до первого использования HAL APIC/IOAPIC
/// - Должна вызываться только один раз
///
/// # Returns
/// true если инициализация успешна
pub unsafe fn mm_init_hal_mmio() -> bool {
    unsafe {
        if HAL_MMIO_INITIALIZED.load(Ordering::Acquire) {
            return true;
        }

        let hhdm = super::init::mm_get_hhdm_offset() as u64;
        if hhdm == 0 {
            return false;
        }

        // Читаем CR3 для получения физического адреса PML4
        let cr3 = super::pte::read_cr3();
        let pml4_phys = cr3 & !0xFFF;
        let pml4_virt = (hhdm + pml4_phys) as *mut u64;

        // Шаг 1: Проверяем PML4[511] (должен существовать для kernel space)
        let pml4e = core::ptr::read_volatile(pml4_virt.add(PML4_INDEX));
        if (pml4e & PTE_VALID) == 0 {
            // Kernel space не настроен - критическая ошибка
            return false;
        }
        let pdpt_phys = pml4e & PTE_FRAME_MASK;
        let pdpt_virt = (hhdm + pdpt_phys) as *mut u64;

        // Шаг 2: Проверяем/создаем PDPT[511]
        let pdpte = core::ptr::read_volatile(pdpt_virt.add(PDPT_INDEX));
        let pd_phys: u64;

        if (pdpte & PTE_VALID) != 0 {
            // PDPT entry существует
            if (pdpte & (1 << 7)) != 0 {
                // 1GB page - не можем использовать для 4KB маппинга
                return false;
            }
            pd_phys = pdpte & PTE_FRAME_MASK;
        } else {
            // Создаем PDPT entry, указывающий на наш статический PD
            pd_phys =
                match get_physical_address(hhdm, core::ptr::addr_of!(HAL_MMIO_TABLES.pd) as u64) {
                    Some(addr) => addr,
                    None => return false,
                };

            // Записываем PDPT entry
            let new_pdpte = pd_phys | PTE_VALID | PTE_READWRITE;
            core::ptr::write_volatile(pdpt_virt.add(PDPT_INDEX), new_pdpte);
        }
        let pd_virt = (hhdm + pd_phys) as *mut u64;

        // Шаг 3: Проверяем/создаем PD[511]
        let pde = core::ptr::read_volatile(pd_virt.add(PD_INDEX));
        let pt_phys: u64;

        if (pde & PTE_VALID) != 0 {
            if (pde & (1 << 7)) != 0 {
                // 2MB page - не можем использовать
                return false;
            }
            pt_phys = pde & PTE_FRAME_MASK;
        } else {
            // Создаем PD entry, указывающий на наш статический PT
            pt_phys =
                match get_physical_address(hhdm, core::ptr::addr_of!(HAL_MMIO_TABLES.pt) as u64) {
                    Some(addr) => addr,
                    None => return false,
                };

            // Записываем PD entry
            let new_pde = pt_phys | PTE_VALID | PTE_READWRITE;
            core::ptr::write_volatile(pd_virt.add(PD_INDEX), new_pde);
        }

        // Сбрасываем TLB
        super::pte::flush_tlb();

        HAL_MMIO_INITIALIZED.store(true, Ordering::Release);
        true
    }
}

/// Проверяет инициализирован ли HAL MMIO region
#[inline]
pub fn is_hal_mmio_initialized() -> bool {
    HAL_MMIO_INITIALIZED.load(Ordering::Acquire)
}

// =============================================================================
// Device Mapping
// =============================================================================

/// Маппит Local APIC на фиксированный виртуальный адрес
///
/// # Arguments
/// * `apic_phys` - физический адрес Local APIC (обычно 0xFEE00000)
///
/// # Returns
/// Виртуальный адрес (всегда HAL_LOCAL_APIC_BASE) или 0 при ошибке
///
/// # Safety
/// Должна вызываться после mm_init_hal_mmio()
pub unsafe fn mm_map_local_apic(apic_phys: u64) -> u64 {
    unsafe {
        if !HAL_MMIO_INITIALIZED.load(Ordering::Acquire) {
            return 0;
        }

        let hhdm = super::init::mm_get_hhdm_offset() as u64;

        // Получаем PT для HAL MMIO
        let pt_virt = get_hal_pt_address(hhdm);
        if pt_virt.is_null() {
            return 0;
        }

        // PT index для Local APIC = 480
        let pt_index = PT_INDEX_BASE;

        // Создаем PTE для APIC (uncached)
        let pte =
            (apic_phys & !0xFFF) | PTE_VALID | PTE_READWRITE | PTE_DISABLE_CACHE | PTE_WRITETHROUGH;
        core::ptr::write_volatile(pt_virt.add(pt_index), pte);

        // Инвалидируем TLB
        super::pte::invlpg(HAL_LOCAL_APIC_BASE);

        HAL_LOCAL_APIC_BASE
    }
}

/// Маппит IOAPIC на фиксированный виртуальный адрес
///
/// # Arguments
/// * `ioapic_phys` - физический адрес IOAPIC
/// * `index` - индекс IOAPIC (0-14)
///
/// # Returns
/// Виртуальный адрес или 0 при ошибке
///
/// # Safety
/// Должна вызываться после mm_init_hal_mmio()
pub unsafe fn mm_map_ioapic(ioapic_phys: u64, index: u8) -> u64 {
    unsafe {
        if !HAL_MMIO_INITIALIZED.load(Ordering::Acquire) {
            return 0;
        }

        if index as usize >= HAL_MAX_IOAPIC_COUNT {
            return 0;
        }

        let hhdm = super::init::mm_get_hhdm_offset() as u64;

        let pt_virt = get_hal_pt_address(hhdm);
        if pt_virt.is_null() {
            return 0;
        }

        // PT index для IOAPIC: 481 + index (IOAPIC #0 = PT[481], #1 = PT[482], ...)
        let pt_index = PT_INDEX_BASE + 1 + index as usize;

        // Виртуальный адрес для этого IOAPIC
        let ioapic_virt = HAL_IOAPIC_BASE + (index as u64 * PAGE_SIZE);

        // Создаем PTE для IOAPIC (uncached)
        let pte = (ioapic_phys & !0xFFF)
            | PTE_VALID
            | PTE_READWRITE
            | PTE_DISABLE_CACHE
            | PTE_WRITETHROUGH;
        core::ptr::write_volatile(pt_virt.add(pt_index), pte);

        // Инвалидируем TLB
        super::pte::invlpg(ioapic_virt);

        // Обновляем счетчик IOAPIC
        let current = HAL_IOAPIC_COUNT.load(Ordering::Acquire);
        if index >= current {
            HAL_IOAPIC_COUNT.store(index + 1, Ordering::Release);
        }

        ioapic_virt
    }
}

/// Возвращает виртуальный адрес для IOAPIC по индексу
///
/// # Arguments
/// * `index` - индекс IOAPIC (0-14)
///
/// # Returns
/// Виртуальный адрес или 0 если не замаплен
#[inline]
pub fn mm_get_ioapic_address(index: u8) -> u64 {
    if index as usize >= HAL_MAX_IOAPIC_COUNT {
        return 0;
    }
    if index >= HAL_IOAPIC_COUNT.load(Ordering::Acquire) {
        return 0;
    }
    HAL_IOAPIC_BASE + (index as u64 * PAGE_SIZE)
}

/// Возвращает количество зарегистрированных IOAPIC
#[inline]
pub fn mm_get_ioapic_count() -> u8 {
    HAL_IOAPIC_COUNT.load(Ordering::Acquire)
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Получает виртуальный адрес HAL PT через текущую page table hierarchy
unsafe fn get_hal_pt_address(hhdm: u64) -> *mut u64 {
    unsafe {
        let cr3 = super::pte::read_cr3();
        let pml4_phys = cr3 & !0xFFF;
        let pml4_virt = (hhdm + pml4_phys) as *const u64;

        // PML4[511] -> PDPT
        let pml4e = core::ptr::read_volatile(pml4_virt.add(PML4_INDEX));
        if (pml4e & PTE_VALID) == 0 {
            return core::ptr::null_mut();
        }

        let pdpt_virt = (hhdm + (pml4e & PTE_FRAME_MASK)) as *const u64;

        // PDPT[511] -> PD
        let pdpte = core::ptr::read_volatile(pdpt_virt.add(PDPT_INDEX));
        if (pdpte & PTE_VALID) == 0 || (pdpte & (1 << 7)) != 0 {
            return core::ptr::null_mut();
        }

        let pd_virt = (hhdm + (pdpte & PTE_FRAME_MASK)) as *const u64;

        // PD[511] -> PT
        let pde = core::ptr::read_volatile(pd_virt.add(PD_INDEX));
        if (pde & PTE_VALID) == 0 || (pde & (1 << 7)) != 0 {
            return core::ptr::null_mut();
        }

        (hhdm + (pde & PTE_FRAME_MASK)) as *mut u64
    }
}

/// Получает физический адрес виртуального адреса через page walk
///
/// Используется для получения физического адреса статических page tables
unsafe fn get_physical_address(hhdm: u64, virt_addr: u64) -> Option<u64> {
    unsafe {
        let cr3 = super::pte::read_cr3();
        let pml4_phys = cr3 & !0xFFF;
        let pml4_virt = (hhdm + pml4_phys) as *const u64;

        let pml4_idx = ((virt_addr >> 39) & 0x1FF) as usize;
        let pdpt_idx = ((virt_addr >> 30) & 0x1FF) as usize;
        let pd_idx = ((virt_addr >> 21) & 0x1FF) as usize;
        let pt_idx = ((virt_addr >> 12) & 0x1FF) as usize;
        let offset = virt_addr & 0xFFF;

        // PML4
        let pml4e = core::ptr::read_volatile(pml4_virt.add(pml4_idx));
        if (pml4e & PTE_VALID) == 0 {
            return None;
        }

        // PDPT
        let pdpt_virt = (hhdm + (pml4e & PTE_FRAME_MASK)) as *const u64;
        let pdpte = core::ptr::read_volatile(pdpt_virt.add(pdpt_idx));
        if (pdpte & PTE_VALID) == 0 {
            return None;
        }

        // 1GB page?
        if (pdpte & (1 << 7)) != 0 {
            return Some((pdpte & PTE_FRAME_MASK) + (virt_addr & 0x3FFFFFFF));
        }

        // PD
        let pd_virt = (hhdm + (pdpte & PTE_FRAME_MASK)) as *const u64;
        let pde = core::ptr::read_volatile(pd_virt.add(pd_idx));
        if (pde & PTE_VALID) == 0 {
            return None;
        }

        // 2MB page?
        if (pde & (1 << 7)) != 0 {
            return Some((pde & PTE_FRAME_MASK) + (virt_addr & 0x1FFFFF));
        }

        // PT
        let pt_virt = (hhdm + (pde & PTE_FRAME_MASK)) as *const u64;
        let pte = core::ptr::read_volatile(pt_virt.add(pt_idx));
        if (pte & PTE_VALID) == 0 {
            return None;
        }

        Some((pte & PTE_FRAME_MASK) + offset)
    }
}
