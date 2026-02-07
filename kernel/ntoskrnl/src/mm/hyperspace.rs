//! Hyperspace — временные маппинги для kernel
//!
//! Hyperspace — специальная область VA для быстрых временных отображений
//! физических страниц внутри ядра.
//!
//! # Назначение
//!
//! - Доступ к физическим страницам без постоянного маппинга
//! - Обнуление страниц перед выдачей (zero page thread)
//! - Копирование между страницами
//! - Чтение/запись page tables других процессов
//!
//! # Архитектура
//!
//! Hyperspace region: `HYPER_SPACE` - `HYPER_SPACE_END`
//! (0xFFFF_F700_0000_0000 - 0xFFFF_F7FF_FFFF_FFFF)
//!
//! Каждый процессор имеет свой slot для избежания конфликтов.
//!
//! # Инициализация
//!
//! Для работы hyperspace необходимо создать page tables (PDPT, PD, PT)
//! для области HYPER_SPACE. Это делается в `mm_init_hyperspace()`.
//!
//! # API
//!
//! - `mm_init_hyperspace()` — инициализация page tables
//! - `mi_map_page_in_hyperspace(pfn)` → виртуальный адрес
//! - `mi_unmap_page_in_hyperspace(va)`
//!
//! Источники:
//! - ReactOS: mm/ARM3/hypermap.c
//! - NT6.1: mm/hypermap.c

use core::sync::atomic::AtomicBool;
use core::sync::atomic::AtomicU64;
use core::sync::atomic::Ordering;

use super::pte::PTE_FRAME_MASK;
use super::pte::PTE_READWRITE;
use super::pte::PTE_VALID;
use super::pte::mm_get_pte_address;
use super::types::HYPER_SPACE;
use super::types::PAGE_SIZE;
use super::types::PFN_NUMBER;

// =============================================================================
// Constants
// =============================================================================

/// Количество hyperspace slots на процессор
const HYPERSPACE_SLOTS_PER_CPU: usize = 16;

/// Максимальное количество процессоров (должно соответствовать KE)
const MAX_PROCESSORS: usize = 64;

/// Размер hyperspace на один процессор
const HYPERSPACE_SIZE_PER_CPU: usize = HYPERSPACE_SLOTS_PER_CPU * PAGE_SIZE;

// =============================================================================
// Page Table Constants for Hyperspace
// =============================================================================

/// HYPER_SPACE = 0xFFFFF70000000000
/// PML4 index = (va >> 39) & 0x1FF = 494
const HYPERSPACE_PML4_INDEX: usize = ((HYPER_SPACE >> 39) & 0x1FF) as usize;

/// PDPT index = (va >> 30) & 0x1FF = 0
const HYPERSPACE_PDPT_INDEX: usize = ((HYPER_SPACE >> 30) & 0x1FF) as usize;

/// PD index = (va >> 21) & 0x1FF = 0
const HYPERSPACE_PD_INDEX: usize = ((HYPER_SPACE >> 21) & 0x1FF) as usize;

// =============================================================================
// Static Page Tables for Hyperspace
// =============================================================================

/// Статические page tables для hyperspace
/// Выделяются в BSS секции - не требуют page allocator
#[repr(C, align(4096))]
struct HyperspacePageTables {
    /// PDPT для PML4[494]
    pdpt: [u64; 512],
    /// Page Directory
    pd: [u64; 512],
    /// Page Table для первых 16 slots * 64 CPUs = 1024 страниц
    /// Но реально используем только первые несколько
    pt: [u64; 512],
}

/// Глобальные статические page tables для hyperspace
static mut HYPERSPACE_TABLES: HyperspacePageTables = HyperspacePageTables {
    pdpt: [0; 512],
    pd: [0; 512],
    pt: [0; 512],
};

/// Флаг инициализации hyperspace
static HYPERSPACE_INITIALIZED: AtomicBool = AtomicBool::new(false);

// =============================================================================
// Per-CPU Hyperspace State
// =============================================================================

/// Bitmap занятых slots для каждого процессора
/// Каждый бит = один slot
static HYPERSPACE_BITMAP: [AtomicU64; MAX_PROCESSORS] = {
    const INIT: AtomicU64 = AtomicU64::new(0);
    [INIT; MAX_PROCESSORS]
};

// =============================================================================
// Initialization
// =============================================================================

/// MiInitializeHyperspace
///
/// Инициализирует page tables для hyperspace региона.
///
/// Создает иерархию page tables:
/// - PML4[494] -> PDPT (статический)
/// - PDPT[0] -> PD (статический)
/// - PD[0] -> PT (статический)
///
/// После инициализации можно использовать mi_map_page_in_hyperspace().
///
/// # Safety
/// - Должна вызываться после инициализации HHDM
/// - Должна вызываться до первого использования hyperspace
/// - Вызывается только один раз
///
/// # Returns
/// true если инициализация успешна
pub unsafe fn mm_init_hyperspace() -> bool {
    unsafe {
        if HYPERSPACE_INITIALIZED.load(Ordering::Acquire) {
            return true;
        }

        let hhdm = super::init::mm_get_hhdm_offset() as u64;
        if hhdm == 0 {
            crate::kd::dbg_print("[HYPERSPACE] HHDM not initialized\n");
            return false;
        }

        // Читаем CR3 для получения физического адреса PML4
        let cr3 = super::pte::read_cr3();
        let pml4_phys = cr3 & !0xFFF;
        let pml4_virt = (hhdm + pml4_phys) as *mut u64;

        // Получаем физические адреса наших статических таблиц
        let pdpt_phys = match get_physical_address(hhdm, core::ptr::addr_of!(HYPERSPACE_TABLES.pdpt) as u64) {
            Some(addr) => addr,
            None => {
                crate::kd::dbg_print("[HYPERSPACE] Failed to get PDPT phys addr\n");
                return false;
            }
        };
        let pd_phys = match get_physical_address(hhdm, core::ptr::addr_of!(HYPERSPACE_TABLES.pd) as u64) {
            Some(addr) => addr,
            None => {
                crate::kd::dbg_print("[HYPERSPACE] Failed to get PD phys addr\n");
                return false;
            }
        };
        let pt_phys = match get_physical_address(hhdm, core::ptr::addr_of!(HYPERSPACE_TABLES.pt) as u64) {
            Some(addr) => addr,
            None => {
                crate::kd::dbg_print("[HYPERSPACE] Failed to get PT phys addr\n");
                return false;
            }
        };

        // Шаг 1: Настраиваем PML4[494] -> наш PDPT
        let pml4e = core::ptr::read_volatile(pml4_virt.add(HYPERSPACE_PML4_INDEX));
        if (pml4e & PTE_VALID) == 0 {
            // Создаем PML4 entry
            let new_pml4e = pdpt_phys | PTE_VALID | PTE_READWRITE;
            core::ptr::write_volatile(pml4_virt.add(HYPERSPACE_PML4_INDEX), new_pml4e);
        }

        // Шаг 2: Настраиваем PDPT[0] -> наш PD
        let pdpt_virt = (hhdm + pdpt_phys) as *mut u64;
        let pdpte = core::ptr::read_volatile(pdpt_virt.add(HYPERSPACE_PDPT_INDEX));
        if (pdpte & PTE_VALID) == 0 {
            let new_pdpte = pd_phys | PTE_VALID | PTE_READWRITE;
            core::ptr::write_volatile(pdpt_virt.add(HYPERSPACE_PDPT_INDEX), new_pdpte);
        }

        // Шаг 3: Настраиваем PD[0] -> наш PT
        let pd_virt = (hhdm + pd_phys) as *mut u64;
        let pde = core::ptr::read_volatile(pd_virt.add(HYPERSPACE_PD_INDEX));
        if (pde & PTE_VALID) == 0 {
            let new_pde = pt_phys | PTE_VALID | PTE_READWRITE;
            core::ptr::write_volatile(pd_virt.add(HYPERSPACE_PD_INDEX), new_pde);
        }

        // Сбрасываем TLB
        super::pte::flush_tlb();

        HYPERSPACE_INITIALIZED.store(true, Ordering::Release);
        true
    }
}

/// Проверяет инициализирован ли hyperspace
#[inline]
pub fn is_hyperspace_initialized() -> bool {
    HYPERSPACE_INITIALIZED.load(Ordering::Acquire)
}

/// Получает физический адрес через HHDM page tables
///
/// Нужно для получения физических адресов статических таблиц.
unsafe fn get_physical_address(hhdm: u64, va: u64) -> Option<u64> {
    unsafe {
        // Для HHDM адресов: phys = va - hhdm
        if va >= hhdm && va < hhdm + 0x800_0000_0000 {
            return Some(va - hhdm);
        }

        // Для других адресов нужно читать page tables
        let cr3 = super::pte::read_cr3();
        let pml4_phys = cr3 & !0xFFF;
        let pml4_virt = (hhdm + pml4_phys) as *const u64;

        let pml4_idx = ((va >> 39) & 0x1FF) as usize;
        let pdpt_idx = ((va >> 30) & 0x1FF) as usize;
        let pd_idx = ((va >> 21) & 0x1FF) as usize;
        let pt_idx = ((va >> 12) & 0x1FF) as usize;

        let pml4e = core::ptr::read_volatile(pml4_virt.add(pml4_idx));
        if (pml4e & PTE_VALID) == 0 {
            return None;
        }

        let pdpt_phys = pml4e & PTE_FRAME_MASK;
        let pdpt_virt = (hhdm + pdpt_phys) as *const u64;
        let pdpte = core::ptr::read_volatile(pdpt_virt.add(pdpt_idx));
        if (pdpte & PTE_VALID) == 0 {
            return None;
        }

        // 1GB page?
        if (pdpte & (1 << 7)) != 0 {
            let base = pdpte & PTE_FRAME_MASK;
            return Some(base + (va & 0x3FFF_FFFF));
        }

        let pd_phys = pdpte & PTE_FRAME_MASK;
        let pd_virt = (hhdm + pd_phys) as *const u64;
        let pde = core::ptr::read_volatile(pd_virt.add(pd_idx));
        if (pde & PTE_VALID) == 0 {
            return None;
        }

        // 2MB page?
        if (pde & (1 << 7)) != 0 {
            let base = pde & PTE_FRAME_MASK;
            return Some(base + (va & 0x1F_FFFF));
        }

        let pt_phys = pde & PTE_FRAME_MASK;
        let pt_virt = (hhdm + pt_phys) as *const u64;
        let pte = core::ptr::read_volatile(pt_virt.add(pt_idx));
        if (pte & PTE_VALID) == 0 {
            return None;
        }

        let base = pte & PTE_FRAME_MASK;
        Some(base + (va & 0xFFF))
    }
}

// =============================================================================
// Mapping Functions
// =============================================================================

/// MiMapPageInHyperSpace
///
/// Временно отображает физическую страницу в hyperspace текущего процессора.
///
/// # Arguments
/// * `pfn` - PFN страницы для отображения
///
/// # Returns
/// Виртуальный адрес в hyperspace или 0 при ошибке.
///
/// # Note
/// Caller ДОЛЖЕН вызвать `mi_unmap_page_in_hyperspace` после использования!
///
/// # Safety
/// - PFN должен быть валидным
/// - Маппинг должен быть снят до выхода из критической секции
pub unsafe fn mi_map_page_in_hyperspace(pfn: PFN_NUMBER) -> u64 {
    unsafe {
        let cpu_id = get_current_cpu_id();
        if cpu_id >= MAX_PROCESSORS {
            return 0;
        }

        // Находим свободный slot
        let slot = allocate_hyperspace_slot(cpu_id);
        if slot >= HYPERSPACE_SLOTS_PER_CPU {
            return 0;
        }

        // Вычисляем VA для этого slot
        let va = hyperspace_slot_to_va(cpu_id, slot);

        // Получаем PTE и маппим
        let pte_ptr = mm_get_pte_address(va);
        let pte_value = ((pfn as u64) << 12) | PTE_VALID | PTE_READWRITE;
        core::ptr::write_volatile(pte_ptr as *mut u64, pte_value);

        // Инвалидируем TLB для этого адреса
        crate::arch::x86_64::cpu::invlpg(va);

        va
    }
}

/// MiMapPageInHyperSpaceReadOnly
///
/// То же что `mi_map_page_in_hyperspace`, но read-only.
pub unsafe fn mi_map_page_in_hyperspace_readonly(pfn: PFN_NUMBER) -> u64 {
    unsafe {
        let cpu_id = get_current_cpu_id();
        if cpu_id >= MAX_PROCESSORS {
            return 0;
        }

        let slot = allocate_hyperspace_slot(cpu_id);
        if slot >= HYPERSPACE_SLOTS_PER_CPU {
            return 0;
        }

        let va = hyperspace_slot_to_va(cpu_id, slot);
        let pte_ptr = mm_get_pte_address(va);

        // Read-only: без PTE_READWRITE
        let pte_value = ((pfn as u64) << 12) | PTE_VALID;
        core::ptr::write_volatile(pte_ptr as *mut u64, pte_value);

        crate::arch::x86_64::cpu::invlpg(va);

        va
    }
}

/// MiUnmapPageInHyperSpace
///
/// Снимает временный маппинг из hyperspace.
///
/// # Arguments
/// * `va` - виртуальный адрес, возвращённый `mi_map_page_in_hyperspace`
pub unsafe fn mi_unmap_page_in_hyperspace(va: u64) {
    unsafe {
        if va == 0 {
            return;
        }

        let cpu_id = get_current_cpu_id();
        if cpu_id >= MAX_PROCESSORS {
            return;
        }

        // Проверяем что VA в hyperspace диапазоне
        let cpu_base = hyperspace_cpu_base(cpu_id);
        let cpu_end = cpu_base + HYPERSPACE_SIZE_PER_CPU as u64;

        if va < cpu_base || va >= cpu_end {
            return;
        }

        // Вычисляем slot
        let slot = ((va - cpu_base) / PAGE_SIZE as u64) as usize;
        if slot >= HYPERSPACE_SLOTS_PER_CPU {
            return;
        }

        // Снимаем маппинг
        let pte_ptr = mm_get_pte_address(va);
        core::ptr::write_volatile(pte_ptr as *mut u64, 0);
        crate::arch::x86_64::cpu::invlpg(va);

        // Освобождаем slot
        free_hyperspace_slot(cpu_id, slot);
    }
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Вычисляет базовый адрес hyperspace для CPU
#[inline]
fn hyperspace_cpu_base(cpu_id: usize) -> u64 {
    HYPER_SPACE + (cpu_id * HYPERSPACE_SIZE_PER_CPU) as u64
}

/// Вычисляет VA для конкретного slot
#[inline]
fn hyperspace_slot_to_va(cpu_id: usize, slot: usize) -> u64 {
    hyperspace_cpu_base(cpu_id) + (slot * PAGE_SIZE) as u64
}

/// Выделяет свободный slot в hyperspace
fn allocate_hyperspace_slot(cpu_id: usize) -> usize {
    let bitmap = &HYPERSPACE_BITMAP[cpu_id];

    loop {
        let current = bitmap.load(Ordering::Acquire);

        // Находим первый свободный бит
        let free_slot = (!current).trailing_zeros() as usize;
        if free_slot >= HYPERSPACE_SLOTS_PER_CPU {
            // Все slots заняты
            return usize::MAX;
        }

        let new_bitmap = current | (1u64 << free_slot);

        if bitmap
            .compare_exchange(current, new_bitmap, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            return free_slot;
        }
        // Retry при конфликте
    }
}

/// Освобождает slot в hyperspace
fn free_hyperspace_slot(cpu_id: usize, slot: usize) {
    if slot >= HYPERSPACE_SLOTS_PER_CPU {
        return;
    }

    let bitmap = &HYPERSPACE_BITMAP[cpu_id];
    bitmap.fetch_and(!(1u64 << slot), Ordering::AcqRel);
}

/// Получает ID текущего процессора
///
/// TODO: интеграция с KE/PRCB
#[inline]
fn get_current_cpu_id() -> usize {
    // Временная реализация — всегда 0 (BSP)
    // В будущем: читать из PRCB через GS
    0
}

// =============================================================================
// Zero Page Helper
// =============================================================================

/// Обнуляет физическую страницу через hyperspace
///
/// Используется zero page thread и mi_allocate_pfn.
///
/// # Safety
/// PFN должен быть валидным и не используемым.
pub unsafe fn mi_zero_physical_page(pfn: PFN_NUMBER) {
    unsafe {
        let va = mi_map_page_in_hyperspace(pfn);
        if va != 0 {
            core::ptr::write_bytes(va as *mut u8, 0, PAGE_SIZE);
            mi_unmap_page_in_hyperspace(va);
        }
    }
}

/// Копирует содержимое одной физической страницы в другую
///
/// # Safety
/// Оба PFN должны быть валидными.
pub unsafe fn mi_copy_physical_page(src_pfn: PFN_NUMBER, dst_pfn: PFN_NUMBER) {
    unsafe {
        let src_va = mi_map_page_in_hyperspace_readonly(src_pfn);
        if src_va == 0 {
            return;
        }

        let dst_va = mi_map_page_in_hyperspace(dst_pfn);
        if dst_va == 0 {
            mi_unmap_page_in_hyperspace(src_va);
            return;
        }

        core::ptr::copy_nonoverlapping(src_va as *const u8, dst_va as *mut u8, PAGE_SIZE);

        mi_unmap_page_in_hyperspace(dst_va);
        mi_unmap_page_in_hyperspace(src_va);
    }
}
