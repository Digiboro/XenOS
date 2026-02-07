//! Memory Manager Types
//!
//! Базовые типы и константы Memory Manager.

use crate::nt::PVOID;

// =============================================================================
// Constants
// =============================================================================

/// Размер страницы (4KB)
pub const PAGE_SIZE: usize = 4096;
/// Сдвиг для номера страницы
pub const PAGE_SHIFT: usize = 12;
/// Маска для offset внутри страницы
pub const PAGE_MASK: usize = PAGE_SIZE - 1;

/// Размер большой страницы (2MB)
pub const LARGE_PAGE_SIZE: usize = 2 * 1024 * 1024;

/// 1KB
pub const _1KB: usize = 1024;
/// 1MB
pub const _1MB: usize = 1024 * _1KB;
/// 1GB
pub const _1GB: usize = 1024 * _1MB;
/// 64KB (allocation granularity)
pub const _64K: usize = 64 * _1KB;

// =============================================================================
// Memory Protection Constants
// =============================================================================

/// Нет доступа
pub const MM_ZERO_ACCESS: u32 = 0;
/// Только чтение
pub const MM_READONLY: u32 = 1;
/// Только выполнение
pub const MM_EXECUTE: u32 = 2;
/// Выполнение + чтение
pub const MM_EXECUTE_READ: u32 = 3;
/// Чтение + запись
pub const MM_READWRITE: u32 = 4;
/// Write copy
pub const MM_WRITECOPY: u32 = 5;
/// Выполнение + чтение + запись
pub const MM_EXECUTE_READWRITE: u32 = 6;
/// Execute write copy
pub const MM_EXECUTE_WRITECOPY: u32 = 7;
/// Маска для access bits
pub const MM_PROTECT_ACCESS: u32 = 7;

/// No cache
pub const MM_NOCACHE: u32 = 0x08;
/// Guard page
pub const MM_GUARDPAGE: u32 = 0x10;
/// Write combine
pub const MM_WRITECOMBINE: u32 = 0x18;

/// Decommitted
pub const MM_DECOMMIT: u32 = MM_ZERO_ACCESS | MM_GUARDPAGE;
/// No access (invalid)
pub const MM_NOACCESS: u32 = MM_ZERO_ACCESS | MM_WRITECOMBINE;

// =============================================================================
// Page Protection (Windows API style)
// =============================================================================

pub const PAGE_NOACCESS: u32 = 0x01;
pub const PAGE_READONLY: u32 = 0x02;
pub const PAGE_READWRITE: u32 = 0x04;
pub const PAGE_WRITECOPY: u32 = 0x08;
pub const PAGE_EXECUTE: u32 = 0x10;
pub const PAGE_EXECUTE_READ: u32 = 0x20;
pub const PAGE_EXECUTE_READWRITE: u32 = 0x40;
pub const PAGE_EXECUTE_WRITECOPY: u32 = 0x80;
pub const PAGE_GUARD: u32 = 0x100;
pub const PAGE_NOCACHE: u32 = 0x200;
pub const PAGE_WRITECOMBINE: u32 = 0x400;

// =============================================================================
// Memory Allocation Types
// =============================================================================

pub const MEM_COMMIT: u32 = 0x00001000;
pub const MEM_RESERVE: u32 = 0x00002000;
pub const MEM_DECOMMIT: u32 = 0x00004000;
pub const MEM_RELEASE: u32 = 0x00008000;
pub const MEM_FREE: u32 = 0x00010000;
pub const MEM_PRIVATE: u32 = 0x00020000;
pub const MEM_MAPPED: u32 = 0x00040000;
pub const MEM_RESET: u32 = 0x00080000;
pub const MEM_TOP_DOWN: u32 = 0x00100000;
pub const MEM_LARGE_PAGES: u32 = 0x20000000;
pub const MEM_4MB_PAGES: u32 = 0x80000000;

// =============================================================================
// PFN Number Type
// =============================================================================

/// Page Frame Number - номер физической страницы
pub type PFN_NUMBER = usize;
/// Счетчик страниц
pub type PFN_COUNT = usize;

// =============================================================================
// Physical Address
// =============================================================================

/// Физический адрес
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PHYSICAL_ADDRESS {
    pub quad_part: u64,
}

impl PHYSICAL_ADDRESS {
    pub const fn new(addr: u64) -> Self {
        Self { quad_part: addr }
    }

    pub const fn zero() -> Self {
        Self { quad_part: 0 }
    }

    /// Конвертирует физический адрес в PFN
    #[inline]
    pub const fn to_pfn(&self) -> PFN_NUMBER {
        (self.quad_part >> PAGE_SHIFT) as PFN_NUMBER
    }

    /// Создает из PFN
    #[inline]
    pub const fn from_pfn(pfn: PFN_NUMBER) -> Self {
        Self {
            quad_part: (pfn as u64) << PAGE_SHIFT as u64,
        }
    }
}

// =============================================================================
// Memory Information
// =============================================================================

/// Информация о памяти
#[repr(C)]
pub struct MEMORY_BASIC_INFORMATION {
    pub base_address: PVOID,
    pub allocation_base: PVOID,
    pub allocation_protect: u32,
    pub region_size: usize,
    pub state: u32,
    pub protect: u32,
    pub memory_type: u32,
}

impl MEMORY_BASIC_INFORMATION {
    pub const fn new() -> Self {
        Self {
            base_address: core::ptr::null_mut(),
            allocation_base: core::ptr::null_mut(),
            allocation_protect: 0,
            region_size: 0,
            state: 0,
            protect: 0,
            memory_type: 0,
        }
    }
}

// =============================================================================
// Pool Types (for Mm integration)
// =============================================================================

/// NonPaged pool
pub const NON_PAGED_POOL: u32 = 0;
/// Paged pool
pub const PAGED_POOL: u32 = 1;
/// NonPaged pool must succeed
pub const NON_PAGED_POOL_MUST_SUCCEED: u32 = 2;
/// Don't use
pub const DONT_USE_THIS_TYPE: u32 = 3;
/// NonPaged pool cache aligned
pub const NON_PAGED_POOL_CACHE_ALIGNED: u32 = 4;
/// Paged pool cache aligned
pub const PAGED_POOL_CACHE_ALIGNED: u32 = 5;
/// NonPaged pool cache aligned must succeed
pub const NON_PAGED_POOL_CACHE_ALIGNED_MUST_S: u32 = 6;
/// Maximum pool type
pub const MAX_POOL_TYPE: u32 = 7;

// =============================================================================
// Virtual Address Space Layout (x86_64)
// =============================================================================
//
// Карта виртуального адресного пространства XenCore (NT6.1-совместимая):
//
// 0x0000_0000_0000_0000 - 0x0000_7FFF_FFFF_FFFF : User space (128TB)
// 0x0000_8000_0000_0000 - 0xFFFF_7FFF_FFFF_FFFF : Non-canonical hole
// 0xFFFF_8000_0000_0000 - 0xFFFF_87FF_FFFF_FFFF : HHDM (limine, ~8TB)
// 0xFFFF_F680_0000_0000 - 0xFFFF_F6FF_FFFF_FFFF : Self-map PTE region
// 0xFFFF_F700_0000_0000 - 0xFFFF_F7FF_FFFF_FFFF : Hyperspace
// 0xFFFF_F800_0000_0000 - 0xFFFF_FBFF_FFFF_FFFF : System PTE area
// 0xFFFF_FFFF_FFE0_0000 - 0xFFFF_FFFF_FFFF_FFFF : HAL MMIO (APIC, IOAPIC)
// =============================================================================

/// Начало user space
pub const MM_LOWEST_USER_ADDRESS: u64 = 0x0000_0000_0001_0000;
/// Конец user space (128TB на x64)
pub const MM_HIGHEST_USER_ADDRESS: u64 = 0x0000_7FFF_FFFF_FFFF;
/// User space probe address
pub const MM_USER_PROBE_ADDRESS: u64 = 0x0000_7FFF_FFFF_0000;

/// Начало system space
pub const MM_SYSTEM_RANGE_START: u64 = 0xFFFF_8000_0000_0000;
/// Конец system space
pub const MM_SYSTEM_RANGE_END: u64 = 0xFFFF_FFFF_FFFF_FFFF;

// Константы PTE_BASE/PDE_BASE/PPE_BASE/PXE_BASE определены в pte.rs как единственная
// точка правды. Используйте mm::pte::PTE_BASE и т.д.

/// Hyperspace base — область для временных kernel mappings
pub const HYPER_SPACE: u64 = 0xFFFF_F700_0000_0000;
/// Hyperspace end
pub const HYPER_SPACE_END: u64 = 0xFFFF_F7FF_FFFF_FFFF;

/// System PTE area start — для MiReserveSystemPtes/MiReleaseSystemPtes
pub const MM_SYSTEM_PTE_START: u64 = 0xFFFF_F800_0000_0000;
/// System PTE area end
pub const MM_SYSTEM_PTE_END: u64 = 0xFFFF_FBFF_FFFF_FFFF;

// =============================================================================
// Alignment Helpers
// =============================================================================

/// Выравнивание адреса вниз до границы страницы
#[inline]
pub const fn page_align(addr: usize) -> usize {
    addr & !PAGE_MASK
}

/// Выравнивание адреса вверх до границы страницы
#[inline]
pub const fn page_align_up(addr: usize) -> usize {
    (addr + PAGE_MASK) & !PAGE_MASK
}

/// Конвертирует виртуальный адрес в PFN
#[inline]
pub const fn address_to_pfn(addr: usize) -> PFN_NUMBER {
    addr >> PAGE_SHIFT
}

/// Конвертирует PFN в адрес
#[inline]
pub const fn pfn_to_address(pfn: PFN_NUMBER) -> usize {
    pfn << PAGE_SHIFT
}

/// Вычисляет количество страниц для размера
#[inline]
pub const fn bytes_to_pages(bytes: usize) -> usize {
    (bytes + PAGE_MASK) >> PAGE_SHIFT
}

/// Вычисляет размер в байтах для количества страниц
#[inline]
pub const fn pages_to_bytes(pages: usize) -> usize {
    pages << PAGE_SHIFT
}
