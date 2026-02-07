//! Модуль настройки страничной адресации для передачи управления ядру.
//!
//! Создаёт page tables для:
//! - Identity mapping для нижних адресов (где выполняется UEFI код)
//! - HHDM mapping: 0xFFFF800000000000 + phys -> phys
//! - Kernel mapping: 0xFFFFF80000000000 -> kernel physical address (как Windows NT)

use alloc::boxed::Box;
use alloc::vec::Vec;

/// HHDM base address (Higher Half Direct Map)
/// Используется для доступа к физической памяти: virt = HHDM_OFFSET + phys
pub const HHDM_OFFSET: u64 = 0xFFFF_8000_0000_0000;

/// Kernel virtual address base (как в Windows NT x64)
/// Ядро и драйверы загружаются начиная с этого адреса
pub const KERNEL_VA_BASE: u64 = 0xFFFF_F800_0000_0000;

/// Размер страницы 4KB
const PAGE_SIZE: u64 = 4096;
/// Размер страницы 2MB  
const PAGE_SIZE_2MB: u64 = 2 * 1024 * 1024;
/// Размер страницы 1GB
const PAGE_SIZE_1GB: u64 = 1024 * 1024 * 1024;

/// Page table entry flags
const PTE_PRESENT: u64 = 1 << 0;
const PTE_WRITABLE: u64 = 1 << 1;
const PTE_PAGE_SIZE: u64 = 1 << 7; // PS bit для 2MB/1GB страниц

/// Количество записей в одной page table
const ENTRIES_PER_TABLE: usize = 512;

/// Выровненная page table (4KB alignment)
#[repr(C, align(4096))]
pub struct PageTable {
    entries: [u64; ENTRIES_PER_TABLE],
}

impl PageTable {
    pub const fn new() -> Self {
        Self {
            entries: [0; ENTRIES_PER_TABLE],
        }
    }

    pub fn as_phys_addr(&self) -> u64 {
        self as *const _ as u64
    }
}

/// HAL MMIO region base (NT-compatible)
/// Local APIC, IOAPIC маппятся сюда
const HAL_MMIO_BASE: u64 = 0xFFFF_FFFF_FFFE_0000;

/// Структура для хранения всех page tables
pub struct PageTables {
    /// PML4 (верхний уровень)
    pub pml4: Box<PageTable>,
    /// PDPT для identity mapping (нижние 512GB)
    pub pdpt_low: Box<PageTable>,
    /// PDPT для HHDM (0xFFFF800000000000)
    pub pdpt_hhdm: Box<PageTable>,
    /// PDPT для kernel space (0xFFFFF80000000000)
    pub pdpt_kernel: Box<PageTable>,
    /// PDPT для HAL MMIO (0xFFFFFFFF80000000) - PML4[511]
    pub pdpt_hal: Box<PageTable>,
    /// Page directories для HHDM (2MB страницы)
    pub pd_hhdm: Vec<Box<PageTable>>,
    /// Page directories для kernel (4KB страницы для точного маппинга)
    pub pd_kernel: Vec<Box<PageTable>>,
    /// Page tables для kernel (4KB страницы)
    pub pt_kernel: Vec<Box<PageTable>>,
}

impl PageTables {
    /// Создаёт новую структуру page tables.
    pub fn new() -> Self {
        Self {
            pml4: Box::new(PageTable::new()),
            pdpt_low: Box::new(PageTable::new()),
            pdpt_hhdm: Box::new(PageTable::new()),
            pdpt_kernel: Box::new(PageTable::new()),
            pdpt_hal: Box::new(PageTable::new()),
            pd_hhdm: Vec::new(),
            pd_kernel: Vec::new(),
            pt_kernel: Vec::new(),
        }
    }

    /// Настраивает page tables для HHDM и identity mapping.
    ///
    /// # Arguments
    /// * `max_phys_memory` - максимальный физический адрес для HHDM маппинга
    pub fn setup(&mut self, max_phys_memory: u64) {
        // PML4 indices:
        // - 0: Identity mapping (0x0000000000000000)
        // - 256: HHDM (0xFFFF800000000000) - bits 47:39 = 256
        // - 496: Kernel (0xFFFFF80000000000) - bits 47:39 = 496
        // - 511: HAL MMIO (0xFFFFFFFF80000000) - bits 47:39 = 511
        let hhdm_pml4_index = ((HHDM_OFFSET >> 39) & 0x1FF) as usize; // 256
        let kernel_pml4_index = ((KERNEL_VA_BASE >> 39) & 0x1FF) as usize; // 496
        let hal_pml4_index = ((HAL_MMIO_BASE >> 39) & 0x1FF) as usize; // 511

        // 1. Identity mapping для нижних 512GB (где работает UEFI код)
        // UEFI может загрузить winload.efi по высокому адресу (например 0x140000000 = 5GB),
        // поэтому нужно покрыть достаточный диапазон для работы после switch_cr3.
        self.pml4.entries[0] = self.pdpt_low.as_phys_addr() | PTE_PRESENT | PTE_WRITABLE;

        // PDPT[0..512] -> 1GB страницы для 0-512GB identity mapping
        // Используем все 512 entries PDPT для максимального покрытия
        for i in 0..512 {
            let phys_addr = (i as u64) * PAGE_SIZE_1GB;
            self.pdpt_low.entries[i] = phys_addr | PTE_PRESENT | PTE_WRITABLE | PTE_PAGE_SIZE;
        }

        // 2. HHDM mapping (0xFFFF800000000000+)
        // Используем 1GB страницы для быстрой инициализации (без дополнительных аллокаций)
        self.pml4.entries[hhdm_pml4_index] =
            self.pdpt_hhdm.as_phys_addr() | PTE_PRESENT | PTE_WRITABLE;

        // PDPT[0..512] -> 1GB страницы для полного HHDM (512GB)
        // Это покрывает всю возможную физическую память через единственную PDPT
        for i in 0..512 {
            let phys_addr = (i as u64) * PAGE_SIZE_1GB;
            self.pdpt_hhdm.entries[i] = phys_addr | PTE_PRESENT | PTE_WRITABLE | PTE_PAGE_SIZE;
        }
        // pd_hhdm больше не используется для 1GB страниц

        // 3. Kernel space (0xFFFFF80000000000+) - настроим позже через map_kernel()
        self.pml4.entries[kernel_pml4_index] =
            self.pdpt_kernel.as_phys_addr() | PTE_PRESENT | PTE_WRITABLE;

        // 4. HAL MMIO space (0xFFFFFFFF80000000+) - PML4[511]
        // Ядро (mm_init_hal_mmio) само настроит PDPT[511], PD[511], PT для HAL MMIO
        // Здесь только создаём пустую PDPT для PML4[511]
        self.pml4.entries[hal_pml4_index] =
            self.pdpt_hal.as_phys_addr() | PTE_PRESENT | PTE_WRITABLE;
    }

    /// Маппирует kernel image в виртуальное адресное пространство.
    ///
    /// # Arguments
    /// * `kernel_phys` - физический адрес загруженного ядра
    /// * `kernel_size` - размер образа ядра в байтах
    ///
    /// # Returns
    /// Виртуальный базовый адрес ядра (KERNEL_VA_BASE)
    pub fn map_kernel(&mut self, kernel_phys: u64, kernel_size: u64) -> u64 {
        // Выравниваем размер до страницы
        let kernel_size_aligned = (kernel_size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        let page_count = kernel_size_aligned / PAGE_SIZE;

        // Вычисляем индексы для KERNEL_VA_BASE
        // VA: 0xFFFFF80000000000
        // PML4 index: bits 47:39 = 496 (уже настроен в setup())
        // PDPT index: bits 38:30 = 0
        // PD index:   bits 29:21 = 0
        // PT index:   bits 20:12 = 0

        let pdpt_index = ((KERNEL_VA_BASE >> 30) & 0x1FF) as usize; // 0
        let pd_index_start = ((KERNEL_VA_BASE >> 21) & 0x1FF) as usize; // 0
        let pt_index_start = ((KERNEL_VA_BASE >> 12) & 0x1FF) as usize; // 0

        // Создаём PD для kernel
        let mut pd = Box::new(PageTable::new());

        // Сколько PT нам нужно? Каждая PT покрывает 2MB (512 * 4KB)
        let pt_count = ((page_count + 511) / 512) as usize;
        let pt_count = pt_count.max(1);

        let mut pages_remaining = page_count;
        let mut current_phys = kernel_phys;

        for pt_idx in 0..pt_count {
            let mut pt = Box::new(PageTable::new());

            // Сколько страниц в этой PT?
            let pages_in_this_pt = pages_remaining.min(512);

            for page_idx in 0..pages_in_this_pt as usize {
                pt.entries[page_idx] = current_phys | PTE_PRESENT | PTE_WRITABLE;
                current_phys += PAGE_SIZE;
            }

            pages_remaining = pages_remaining.saturating_sub(512);

            // Добавляем PT в PD
            let pd_idx = pd_index_start + pt_idx;
            pd.entries[pd_idx] = pt.as_phys_addr() | PTE_PRESENT | PTE_WRITABLE;
            self.pt_kernel.push(pt);
        }

        // Добавляем PD в PDPT
        self.pdpt_kernel.entries[pdpt_index] = pd.as_phys_addr() | PTE_PRESENT | PTE_WRITABLE;
        self.pd_kernel.push(pd);

        KERNEL_VA_BASE
    }

    /// Возвращает физический адрес PML4 для загрузки в CR3.
    pub fn cr3_value(&self) -> u64 {
        self.pml4.as_phys_addr()
    }

    /// Маппирует дополнительный модуль (boot driver) в виртуальное адресное пространство.
    ///
    /// Модуль маппится последовательно после ядра в том же PDPT.
    ///
    /// # Arguments
    /// * `module_phys` - физический адрес загруженного модуля
    /// * `module_va` - виртуальный адрес (должен быть >= KERNEL_VA_BASE)
    /// * `module_size` - размер модуля в байтах
    pub fn map_module(&mut self, module_phys: u64, module_va: u64, module_size: u64) {
        // Выравниваем размер до страницы
        let size_aligned = (module_size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        let page_count = size_aligned / PAGE_SIZE;

        // Вычисляем индексы для module_va
        let pdpt_index = ((module_va >> 30) & 0x1FF) as usize;
        let pd_index_start = ((module_va >> 21) & 0x1FF) as usize;
        let pt_index_start = ((module_va >> 12) & 0x1FF) as usize;

        // Проверяем, есть ли уже PD для этого PDPT индекса
        // Если нет - создаём новую
        if self.pdpt_kernel.entries[pdpt_index] == 0 {
            let pd = Box::new(PageTable::new());
            self.pdpt_kernel.entries[pdpt_index] = pd.as_phys_addr() | PTE_PRESENT | PTE_WRITABLE;
            self.pd_kernel.push(pd);
        }

        // Получаем PD из PDPT
        let pd_phys = self.pdpt_kernel.entries[pdpt_index] & !0xFFF;

        // Маппируем страницы
        let mut pages_remaining = page_count;
        let mut current_phys = module_phys;
        let mut current_va = module_va;

        while pages_remaining > 0 {
            let pd_idx = ((current_va >> 21) & 0x1FF) as usize;
            let pt_idx = ((current_va >> 12) & 0x1FF) as usize;

            // Получаем или создаём PT
            let pd = unsafe { &mut *(pd_phys as *mut PageTable) };

            if pd.entries[pd_idx] == 0 {
                let pt = Box::new(PageTable::new());
                pd.entries[pd_idx] = pt.as_phys_addr() | PTE_PRESENT | PTE_WRITABLE;
                self.pt_kernel.push(pt);
            }

            let pt_phys = pd.entries[pd_idx] & !0xFFF;
            let pt = unsafe { &mut *(pt_phys as *mut PageTable) };

            // Заполняем PT entries
            let entries_in_this_pt = (512 - pt_idx).min(pages_remaining as usize);
            for i in 0..entries_in_this_pt {
                pt.entries[pt_idx + i] = current_phys | PTE_PRESENT | PTE_WRITABLE;
                current_phys += PAGE_SIZE;
                current_va += PAGE_SIZE;
            }

            pages_remaining = pages_remaining.saturating_sub(entries_in_this_pt as u64);
        }
    }
}

/// Переключает CR3 на новые page tables.
///
/// # Safety
/// Вызывающий должен гарантировать что page tables корректно настроены
/// и identity mapping покрывает текущий исполняемый код.
#[inline(always)]
pub unsafe fn switch_cr3(cr3: u64) {
    unsafe {
        core::arch::asm!(
            "mov cr3, {}",
            in(reg) cr3,
            options(nostack, preserves_flags)
        );
    }
}

/// Вычисляет виртуальный entry point ядра.
///
/// # Arguments
/// * `kernel_va_base` - виртуальный базовый адрес ядра
/// * `kernel_phys_base` - физический базовый адрес ядра
/// * `entry_phys` - физический адрес entry point
pub fn compute_kernel_entry_va(kernel_va_base: u64, kernel_phys_base: u64, entry_phys: u64) -> u64 {
    let offset = entry_phys - kernel_phys_base;
    kernel_va_base + offset
}
