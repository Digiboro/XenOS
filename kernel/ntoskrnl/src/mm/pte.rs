//! Page Table Entries (PTE)
//!
//! Управление таблицами страниц x86_64.
//!
//! # Self-Referencing PML4 (Recursive Mapping)
//!
//! NT использует self-referencing PML4 (recursive mapping) для доступа к page tables.
//! PML4[PXI_SELF] указывает на физический адрес самой PML4, создавая «окно» для
//! доступа ко всем уровням page tables через фиксированные виртуальные адреса.
//!
//! ## Конфигурация XenCore (NT6.1-совместимая)
//!
//! - **PXI_SELF = 493** — индекс self-reference entry в PML4
//! - **PTE_BASE = 0xFFFFF68000000000** — база для доступа ко всем PTE
//!
//! ## Как это работает
//!
//! Когда CPU обращается к адресу в диапазоне PTE_BASE, трансляция происходит так:
//! 1. PML4[493] -> физ. адрес PML4 (self-ref) -> PML4 используется как PDPT
//! 2. Далее индексы из адреса позволяют «провалиться» до нужного уровня таблицы
//!
//! ```text
//! Уровень        Формула доступа                    Виртуальный диапазон
//! ─────────────────────────────────────────────────────────────────────────
//! PT entries     PTE_BASE + (va >> 12) << 3         0xFFFFF68000000000
//! PD entries     PDE_BASE + (va >> 21) << 3         0xFFFFF6FB40000000
//! PDPT entries   PPE_BASE + (va >> 30) << 3         0xFFFFF6FB7DA00000
//! PML4 entries   PXE_BASE + (va >> 39) << 3         0xFFFFF6FB7DBED000
//! ```
//!
//! Источники:
//! - NT5: mm/amd64/miamd.c
//! - ReactOS: mm/ARM3/miarm.h, mm/ARM3/amd64/init.c

use super::types::*;

// =============================================================================
// Page Table Self-Referencing Constants (NT AMD64)
// =============================================================================

/// Self-referencing PML4 entry index
///
/// Вычисляется как: (PTE_BASE >> 39) & 0x1FF = 493
///
/// В NT этот индекс зависит от версии:
/// - NT5 (XP/2003): различные значения
/// - NT6.x (Vista+): обычно 493 или близкие
/// - KASLR (Win8+): рандомизируется
///
/// XenCore использует фиксированный слот 493 для совместимости с NT6.1.
pub const PXI_SELF: usize = 493;

/// PTE Base — виртуальный адрес начала «окна» для доступа ко всем PTE
///
/// Любой PTE в системе доступен по адресу:
///   PTE_BASE + ((va >> 12) & 0x7FFFFFFFF) << 3
///
/// Соответствует трансляции: PML4[493] -> self-ref -> далее по индексам
pub const PTE_BASE: u64 = 0xFFFFF68000000000;

/// PDE Base — виртуальный адрес для доступа ко всем Page Directory entries
///
/// Любой PDE доступен по адресу:
///   PDE_BASE + ((va >> 21) & 0x3FFFFFFF) << 3
pub const PDE_BASE: u64 = 0xFFFFF6FB40000000;

/// PPE Base (PDPTE) — виртуальный адрес для доступа ко всем PDPT entries
///
/// Любой PDPTE доступен по адресу:
///   PPE_BASE + ((va >> 30) & 0x1FFFFF) << 3
pub const PPE_BASE: u64 = 0xFFFFF6FB7DA00000;

/// PXE Base (PML4E) — виртуальный адрес для доступа ко всем PML4 entries
///
/// Любой PML4E доступен по адресу:
///   PXE_BASE + (pml4_index << 3)
pub const PXE_BASE: u64 = 0xFFFFF6FB7DBED000;

// =============================================================================
// PTE Address Functions (NT style)
// =============================================================================

/// Возвращает виртуальный адрес PTE для заданного виртуального адреса
///
/// NT использует self-referencing для прямого доступа к PTE.
///
/// Formula: PTE_BASE + ((VA >> 12) & 0x7FFFFFFFF) << 3)
/// - VA >> 12 = page number (убираем 12 bits offset)
/// - & 0x7FFFFFFFF = маска для 39-bit index (PML4+PDPT+PD+PT)
/// - << 3 = умножаем на 8 (размер PTE entry)
///
/// # Arguments
/// * `virtual_address` - виртуальный адрес для которого нужен PTE
///
/// # Returns
/// Виртуальный адрес PTE
#[inline]
pub fn mm_get_pte_address(virtual_address: u64) -> *mut MMPTE {
    // Для 48-bit VA: page index = VA[47:12] (36 bits)
    // Маска: 0x0000_000F_FFFF_FFFF = 36 bits
    // Это покрывает максимум 2^36 страниц = 256TB address space
    const PAGE_INDEX_MASK: u64 = 0x0000_000F_FFFF_FFFF;
    (PTE_BASE + (((virtual_address >> 12) & PAGE_INDEX_MASK) << 3)) as *mut MMPTE
}

/// Возвращает виртуальный адрес PDE для заданного виртуального адреса
///
/// Formula: PDE_BASE + (((VA >> 21) & mask) << 3)
#[inline]
pub fn mm_get_pde_address(virtual_address: u64) -> *mut MMPTE {
    // Для 48-bit VA: PD index = VA[47:21] (27 bits)
    // Маска: 0x0000_0007_FFFF_FFFF = 27 bits
    const PD_INDEX_MASK: u64 = 0x0000_0007_FFFF_FFFF;
    (PDE_BASE + (((virtual_address >> 21) & PD_INDEX_MASK) << 3)) as *mut MMPTE
}

/// Возвращает виртуальный адрес PPE (PDPTE) для заданного виртуального адреса
///
/// Formula: PPE_BASE + (((VA >> 30) & mask) << 3)
#[inline]
pub fn mm_get_ppe_address(virtual_address: u64) -> *mut MMPTE {
    // Для 48-bit VA: PPE index = VA[47:30] (18 bits)
    // Маска: 0x0000_0000_0003_FFFF = 18 bits
    const PPE_INDEX_MASK: u64 = 0x0000_0000_0003_FFFF;
    (PPE_BASE + (((virtual_address >> 30) & PPE_INDEX_MASK) << 3)) as *mut MMPTE
}

/// Возвращает виртуальный адрес PXE (PML4E) для заданного виртуального адреса
///
/// Formula: PXE_BASE + ((pml4_index) << 3)
#[inline]
pub fn mm_get_pxe_address(virtual_address: u64) -> *mut MMPTE {
    // Маска для 9 бит (только PML4 index)
    let pml4_index = (virtual_address >> 39) & 0x1FF;
    (PXE_BASE + (pml4_index << 3)) as *mut MMPTE
}

// =============================================================================
// Index Extraction
// =============================================================================

/// Извлекает PML4 (PXI) индекс из виртуального адреса
#[inline]
pub const fn mm_get_pxe_index(va: u64) -> usize {
    ((va >> 39) & 0x1FF) as usize
}

/// Извлекает PDPT (PPE) индекс из виртуального адреса
#[inline]
pub const fn mm_get_ppe_index(va: u64) -> usize {
    ((va >> 30) & 0x1FF) as usize
}

/// Извлекает PD (PDE) индекс из виртуального адреса
#[inline]
pub const fn mm_get_pde_index(va: u64) -> usize {
    ((va >> 21) & 0x1FF) as usize
}

/// Извлекает PT (PTE) индекс из виртуального адреса
#[inline]
pub const fn mm_get_pte_index(va: u64) -> usize {
    ((va >> 12) & 0x1FF) as usize
}

// =============================================================================
// PTE Flags (x86_64)
// =============================================================================

/// Страница присутствует в памяти
pub const PTE_VALID: u64 = 1 << 0;
/// Страница доступна для записи
pub const PTE_READWRITE: u64 = 1 << 1;
/// Страница доступна из user mode
pub const PTE_USER: u64 = 1 << 2;
/// Write-through caching
pub const PTE_WRITETHROUGH: u64 = 1 << 3;
/// Отключить кэширование
pub const PTE_DISABLE_CACHE: u64 = 1 << 4;
/// Страница была прочитана
pub const PTE_ACCESSED: u64 = 1 << 5;
/// Страница была записана
pub const PTE_DIRTY: u64 = 1 << 6;
/// Large page (для PDE)
pub const PTE_LARGE_PAGE: u64 = 1 << 7;
/// Global page (не сбрасывается при переключении CR3)
pub const PTE_GLOBAL: u64 = 1 << 8;
/// Copy-on-write (используется ОС)
pub const PTE_WRITECOPY: u64 = 1 << 9;
/// Prototype PTE (используется ОС)
pub const PTE_PROTOTYPE: u64 = 1 << 10;
/// Reserved for OS
pub const PTE_RESERVED: u64 = 1 << 11;
/// No execute (NX bit)
pub const PTE_NX: u64 = 1 << 63;

/// Маска для физического адреса в PTE (биты 12-51)
pub const PTE_FRAME_MASK: u64 = 0x000F_FFFF_FFFF_F000;

/// Маска для protection bits
pub const PTE_PROTECT_MASK: u64 =
    PTE_READWRITE | PTE_USER | PTE_WRITETHROUGH | PTE_DISABLE_CACHE | PTE_NX;

// =============================================================================
// MMPTE - Memory Manager PTE
// =============================================================================

/// Page Table Entry
#[repr(transparent)]
#[derive(Clone, Copy, Debug)]
pub struct MMPTE {
    pub value: u64,
}

impl MMPTE {
    /// Создает невалидный PTE
    #[inline]
    pub const fn invalid() -> Self {
        Self { value: 0 }
    }

    /// Создает PTE из raw value
    #[inline]
    pub const fn from_raw(value: u64) -> Self {
        Self { value }
    }

    /// Проверяет, валиден ли PTE
    #[inline]
    pub const fn is_valid(&self) -> bool {
        (self.value & PTE_VALID) != 0
    }

    /// Проверяет, large page ли это
    #[inline]
    pub const fn is_large(&self) -> bool {
        (self.value & PTE_LARGE_PAGE) != 0
    }

    /// Проверяет writeable
    #[inline]
    pub const fn is_writable(&self) -> bool {
        (self.value & PTE_READWRITE) != 0
    }

    /// Проверяет user accessible
    #[inline]
    pub const fn is_user(&self) -> bool {
        (self.value & PTE_USER) != 0
    }

    /// Проверяет accessed bit
    #[inline]
    pub const fn is_accessed(&self) -> bool {
        (self.value & PTE_ACCESSED) != 0
    }

    /// Проверяет dirty bit
    #[inline]
    pub const fn is_dirty(&self) -> bool {
        (self.value & PTE_DIRTY) != 0
    }

    /// Возвращает физический адрес
    #[inline]
    pub const fn frame(&self) -> u64 {
        self.value & PTE_FRAME_MASK
    }

    /// Возвращает PFN
    #[inline]
    pub const fn pfn(&self) -> PFN_NUMBER {
        (self.frame() >> PAGE_SHIFT) as PFN_NUMBER
    }

    /// Устанавливает PFN
    #[inline]
    pub fn set_pfn(&mut self, pfn: PFN_NUMBER) {
        self.value =
            (self.value & !PTE_FRAME_MASK) | (((pfn as u64) << PAGE_SHIFT) & PTE_FRAME_MASK);
    }

    /// Устанавливает флаги
    #[inline]
    pub fn set_flags(&mut self, flags: u64) {
        self.value |= flags;
    }

    /// Снимает флаги
    #[inline]
    pub fn clear_flags(&mut self, flags: u64) {
        self.value &= !flags;
    }

    /// Создает kernel PTE
    pub fn kernel(pfn: PFN_NUMBER, writable: bool) -> Self {
        let mut value = PTE_VALID | PTE_GLOBAL | (((pfn as u64) << PAGE_SHIFT) & PTE_FRAME_MASK);
        if writable {
            value |= PTE_READWRITE;
        }
        Self { value }
    }

    /// Создает user PTE
    pub fn user(pfn: PFN_NUMBER, writable: bool, executable: bool) -> Self {
        let mut value = PTE_VALID | PTE_USER | (((pfn as u64) << PAGE_SHIFT) & PTE_FRAME_MASK);
        if writable {
            value |= PTE_READWRITE;
        }
        if !executable {
            value |= PTE_NX;
        }
        Self { value }
    }
}

impl Default for MMPTE {
    fn default() -> Self {
        Self::invalid()
    }
}

// =============================================================================
// MMPDE - Page Directory Entry (same structure as PTE on x64)
// =============================================================================

pub type MMPDE = MMPTE;

// =============================================================================
// Page Table Layout
// =============================================================================

/// Количество entries в page table
pub const PTE_PER_PAGE: usize = 512;
/// Количество entries в page directory
pub const PDE_PER_PAGE: usize = 512;

/// Page Map Level 4 (PML4) - top level
#[repr(C, align(4096))]
pub struct PageMapLevel4 {
    pub entries: [MMPTE; 512],
}

/// Page Directory Pointer Table (PDPT)
#[repr(C, align(4096))]
pub struct PageDirectoryPointerTable {
    pub entries: [MMPTE; 512],
}

/// Page Directory (PD)
#[repr(C, align(4096))]
pub struct PageDirectory {
    pub entries: [MMPDE; 512],
}

/// Page Table (PT)
#[repr(C, align(4096))]
pub struct PageTable {
    pub entries: [MMPTE; 512],
}

impl PageMapLevel4 {
    pub const fn new() -> Self {
        Self {
            entries: [MMPTE::invalid(); 512],
        }
    }
}

impl PageDirectoryPointerTable {
    pub const fn new() -> Self {
        Self {
            entries: [MMPTE::invalid(); 512],
        }
    }
}

impl PageDirectory {
    pub const fn new() -> Self {
        Self {
            entries: [MMPDE::invalid(); 512],
        }
    }
}

impl PageTable {
    pub const fn new() -> Self {
        Self {
            entries: [MMPTE::invalid(); 512],
        }
    }
}

// =============================================================================
// Virtual Address Decomposition
// =============================================================================

/// Индексы в page tables для виртуального адреса
#[derive(Clone, Copy, Debug)]
pub struct VirtualAddressIndices {
    /// Индекс в PML4 (биты 47-39)
    pub pml4: usize,
    /// Индекс в PDPT (биты 38-30)
    pub pdpt: usize,
    /// Индекс в PD (биты 29-21)
    pub pd: usize,
    /// Индекс в PT (биты 20-12)
    pub pt: usize,
    /// Offset внутри страницы (биты 11-0)
    pub offset: usize,
}

impl VirtualAddressIndices {
    /// Разбирает виртуальный адрес на индексы
    pub fn from_address(addr: u64) -> Self {
        Self {
            pml4: ((addr >> 39) & 0x1FF) as usize,
            pdpt: ((addr >> 30) & 0x1FF) as usize,
            pd: ((addr >> 21) & 0x1FF) as usize,
            pt: ((addr >> 12) & 0x1FF) as usize,
            offset: (addr & 0xFFF) as usize,
        }
    }

    /// Собирает виртуальный адрес из индексов
    pub fn to_address(&self) -> u64 {
        let mut addr = 0u64;
        addr |= (self.pml4 as u64) << 39;
        addr |= (self.pdpt as u64) << 30;
        addr |= (self.pd as u64) << 21;
        addr |= (self.pt as u64) << 12;
        addr |= self.offset as u64;

        // Sign extension для canonical addresses
        if addr & (1 << 47) != 0 {
            addr |= 0xFFFF_0000_0000_0000;
        }

        addr
    }
}

// =============================================================================
// Page Table Operations
// =============================================================================

/// Получает текущий CR3
#[inline]
pub fn read_cr3() -> u64 {
    let value: u64;
    unsafe {
        core::arch::asm!("mov {}, cr3", out(reg) value, options(nomem, nostack));
    }
    value
}

/// Записывает CR3
///
/// # Safety
/// Должен быть валидный физический адрес PML4
#[inline]
pub unsafe fn write_cr3(value: u64) {
    unsafe {
        core::arch::asm!("mov cr3, {}", in(reg) value, options(nostack));
    }
}

/// Инвалидирует TLB entry для адреса
#[inline]
pub fn invlpg(addr: u64) {
    unsafe {
        core::arch::asm!("invlpg [{}]", in(reg) addr, options(nostack));
    }
}

/// Полная инвалидация TLB (reload CR3)
#[inline]
pub fn flush_tlb() {
    unsafe {
        let cr3 = read_cr3();
        write_cr3(cr3);
    }
}
