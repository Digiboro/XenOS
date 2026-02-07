//! Memory Manager Tests
//!
//! Тесты для подсистемы управления памятью.
//! Покрывают: types, pte, vad, alignment, edge cases.

use crate::mm::pte::*;
use crate::mm::types::*;
use crate::mm::vad::MM_ALLOCATION_GRANULARITY;
use crate::mm::vad::MMVAD_FLAGS;
use crate::mm::vad::VAD_TYPE;
use crate::test::harness::KernelTest;

// =============================================================================
// Types Tests - базовые константы и типы
// =============================================================================

/// Проверка базовых констант размеров
fn test_page_constants() {
    assert_eq!(PAGE_SIZE, 4096);
    assert_eq!(PAGE_SHIFT, 12);
    assert_eq!(PAGE_MASK, 0xFFF);
    assert_eq!(LARGE_PAGE_SIZE, 2 * 1024 * 1024);

    // Проверяем что PAGE_SIZE = 2^PAGE_SHIFT
    assert_eq!(PAGE_SIZE, 1 << PAGE_SHIFT);

    // Проверяем size constants
    assert_eq!(_1KB, 1024);
    assert_eq!(_1MB, 1024 * 1024);
    assert_eq!(_1GB, 1024 * 1024 * 1024);
    assert_eq!(_64K, 64 * 1024);
}

/// Проверка alignment функций - базовые случаи
fn test_page_align_basic() {
    // page_align (вниз)
    assert_eq!(page_align(0), 0);
    assert_eq!(page_align(4096), 4096);
    assert_eq!(page_align(4097), 4096);
    assert_eq!(page_align(8191), 4096);
    assert_eq!(page_align(8192), 8192);

    // page_align_up (вверх)
    assert_eq!(page_align_up(0), 0);
    assert_eq!(page_align_up(1), 4096);
    assert_eq!(page_align_up(4095), 4096);
    assert_eq!(page_align_up(4096), 4096);
    assert_eq!(page_align_up(4097), 8192);
}

/// Проверка alignment функций - граничные случаи
fn test_page_align_edge_cases() {
    // Большие адреса
    let large_addr: usize = 0xFFFF_8000_0000_1234;
    assert_eq!(page_align(large_addr), 0xFFFF_8000_0000_1000);
    assert_eq!(page_align_up(large_addr), 0xFFFF_8000_0000_2000);

    // Максимальный выровненный адрес
    let max_aligned: usize = 0xFFFF_FFFF_FFFF_F000;
    assert_eq!(page_align(max_aligned), max_aligned);
    assert_eq!(page_align_up(max_aligned), max_aligned);

    // Проверяем что page_align никогда не увеличивает адрес
    for offset in [0usize, 1, 100, 4095] {
        let addr = 0x1000 + offset;
        assert!(page_align(addr) <= addr);
    }
}

/// Проверка PFN конверсий
fn test_pfn_conversions() {
    // address_to_pfn
    assert_eq!(address_to_pfn(0), 0);
    assert_eq!(address_to_pfn(4096), 1);
    assert_eq!(address_to_pfn(4097), 1); // offset игнорируется
    assert_eq!(address_to_pfn(8192), 2);
    assert_eq!(address_to_pfn(0x100000), 256); // 1MB = 256 страниц

    // pfn_to_address
    assert_eq!(pfn_to_address(0), 0);
    assert_eq!(pfn_to_address(1), 4096);
    assert_eq!(pfn_to_address(256), 0x100000);

    // Round-trip: pfn_to_address(address_to_pfn(x)) == page_align(x)
    for addr in [0usize, 4096, 0x12345678, 0xFFFF_8000_0000_0000] {
        assert_eq!(pfn_to_address(address_to_pfn(addr)), page_align(addr));
    }
}

/// Проверка bytes_to_pages и pages_to_bytes
fn test_bytes_pages_conversions() {
    // bytes_to_pages округляет вверх
    assert_eq!(bytes_to_pages(0), 0);
    assert_eq!(bytes_to_pages(1), 1);
    assert_eq!(bytes_to_pages(4095), 1);
    assert_eq!(bytes_to_pages(4096), 1);
    assert_eq!(bytes_to_pages(4097), 2);
    assert_eq!(bytes_to_pages(8192), 2);
    assert_eq!(bytes_to_pages(_1MB), 256);

    // pages_to_bytes
    assert_eq!(pages_to_bytes(0), 0);
    assert_eq!(pages_to_bytes(1), 4096);
    assert_eq!(pages_to_bytes(256), _1MB);

    // Свойство: pages_to_bytes(bytes_to_pages(x)) >= x
    for size in [0usize, 1, 100, 4096, 4097, _1MB] {
        assert!(pages_to_bytes(bytes_to_pages(size)) >= size);
    }
}

/// Проверка PHYSICAL_ADDRESS
fn test_physical_address() {
    let zero = PHYSICAL_ADDRESS::zero();
    assert_eq!(zero.quad_part, 0);
    assert_eq!(zero.to_pfn(), 0);

    let addr = PHYSICAL_ADDRESS::new(0x123456000);
    assert_eq!(addr.quad_part, 0x123456000);
    assert_eq!(addr.to_pfn(), 0x123456); // адрес >> 12

    // from_pfn и to_pfn должны быть обратными операциями
    for pfn in [0usize, 1, 0x100, 0xFFFFF] {
        let addr = PHYSICAL_ADDRESS::from_pfn(pfn);
        assert_eq!(addr.to_pfn(), pfn);
    }
}

// =============================================================================
// PTE Tests - Page Table Entry операции
// =============================================================================

/// Проверка констант PTE
fn test_pte_constants() {
    // Self-referencing
    assert_eq!(PXI_SELF, 493);

    // Base addresses
    assert_eq!(PTE_BASE, 0xFFFFF68000000000);
    assert_eq!(PDE_BASE, 0xFFFFF6FB40000000);
    assert_eq!(PPE_BASE, 0xFFFFF6FB7DA00000);
    assert_eq!(PXE_BASE, 0xFFFFF6FB7DBED000);

    // PTE flags - проверяем битовые позиции
    assert_eq!(PTE_VALID, 1 << 0);
    assert_eq!(PTE_READWRITE, 1 << 1);
    assert_eq!(PTE_USER, 1 << 2);
    assert_eq!(PTE_ACCESSED, 1 << 5);
    assert_eq!(PTE_DIRTY, 1 << 6);
    assert_eq!(PTE_LARGE_PAGE, 1 << 7);
    assert_eq!(PTE_GLOBAL, 1 << 8);
    assert_eq!(PTE_NX, 1 << 63);

    // Frame mask должна покрывать биты 12-51
    assert_eq!(PTE_FRAME_MASK & 0xFFF, 0); // биты 0-11 = 0
    assert_eq!(PTE_FRAME_MASK >> 52, 0); // биты 52+ = 0
}

/// Проверка извлечения индексов из виртуального адреса
fn test_pte_index_extraction() {
    // Нулевой адрес - все индексы 0
    assert_eq!(mm_get_pxe_index(0), 0);
    assert_eq!(mm_get_ppe_index(0), 0);
    assert_eq!(mm_get_pde_index(0), 0);
    assert_eq!(mm_get_pte_index(0), 0);

    // Максимальные индексы (все биты установлены в соответствующих позициях)
    // PTE index: bits 12-20 (9 bits), max = 511
    assert_eq!(mm_get_pte_index(0x1FF000), 511);
    // PDE index: bits 21-29
    assert_eq!(mm_get_pde_index(0x3FE00000), 511);
    // PPE index: bits 30-38
    assert_eq!(mm_get_ppe_index(0x7FC0000000), 511);
    // PXE index: bits 39-47
    assert_eq!(mm_get_pxe_index(0xFF8000000000), 511);

    // Проверяем что индексы не влияют друг на друга
    let va: u64 = 0x0001_2345_6789_A000;
    let pxe = mm_get_pxe_index(va);
    let ppe = mm_get_ppe_index(va);
    let pde = mm_get_pde_index(va);
    let pte = mm_get_pte_index(va);

    // Каждый индекс должен быть в диапазоне 0-511
    assert!(pxe < 512);
    assert!(ppe < 512);
    assert!(pde < 512);
    assert!(pte < 512);
}

/// Проверка canonical адресов (kernel space)
fn test_pte_index_kernel_addresses() {
    // Kernel address (high half)
    let kernel_va: u64 = 0xFFFF_8000_0000_0000;

    // PXE index для kernel space должен быть >= 256 (sign-extended)
    let pxe = mm_get_pxe_index(kernel_va);
    assert!(pxe >= 256, "Kernel PXE index should be >= 256, got {}", pxe);

    // Проверяем конкретные kernel addresses
    assert_eq!(mm_get_pxe_index(0xFFFF_8000_0000_0000), 256); // HHDM start
    assert_eq!(mm_get_pxe_index(PTE_BASE), 493); // Self-map area
}

/// Проверка MMPTE структуры
fn test_mmpte_structure() {
    // Invalid PTE
    let invalid = MMPTE::invalid();
    assert_eq!(invalid.value, 0);
    assert!(!invalid.is_valid());
    assert!(!invalid.is_writable());
    assert!(!invalid.is_user());
    assert!(!invalid.is_large());

    // Valid kernel page (проверяем через raw value)
    let kernel_pte = MMPTE::from_raw(PTE_VALID | PTE_READWRITE | PTE_GLOBAL);
    assert!(kernel_pte.is_valid());
    assert!(kernel_pte.is_writable());
    assert!(!kernel_pte.is_user());
    // Global проверяем через битовую маску
    assert!((kernel_pte.value & PTE_GLOBAL) != 0);

    // User page
    let user_pte = MMPTE::from_raw(PTE_VALID | PTE_READWRITE | PTE_USER);
    assert!(user_pte.is_valid());
    assert!(user_pte.is_user());
    assert!((user_pte.value & PTE_GLOBAL) == 0);

    // NX bit - проверяем через битовую маску
    let nx_pte = MMPTE::from_raw(PTE_VALID | PTE_NX);
    assert!(nx_pte.is_valid());
    assert!((nx_pte.value & PTE_NX) != 0);

    // Accessed и Dirty bits
    let accessed_pte = MMPTE::from_raw(PTE_VALID | PTE_ACCESSED | PTE_DIRTY);
    assert!(accessed_pte.is_accessed());
    assert!(accessed_pte.is_dirty());
}

/// Проверка извлечения PFN из PTE
fn test_mmpte_pfn_operations() {
    // PTE с известным PFN
    let pfn: usize = 0x12345;
    let pte = MMPTE::from_raw(PTE_VALID | ((pfn as u64) << 12));

    assert_eq!(pte.pfn(), pfn);
    assert_eq!(pte.frame(), (pfn as u64) << 12);

    // Проверяем что frame mask работает правильно
    let full_pte = MMPTE::from_raw(0xFFFF_FFFF_FFFF_FFFF);
    let extracted_pfn = full_pte.pfn();
    // PFN должен быть в разумных пределах (40 бит max)
    assert!(extracted_pfn < (1usize << 40));

    // Проверяем set_pfn
    let mut mutable_pte = MMPTE::from_raw(PTE_VALID);
    mutable_pte.set_pfn(0xABCDE);
    assert_eq!(mutable_pte.pfn(), 0xABCDE);
    // Флаги должны сохраниться
    assert!(mutable_pte.is_valid());
}

// =============================================================================
// VAD Tests - Virtual Address Descriptor
// =============================================================================

/// Проверка MMVAD_FLAGS
fn test_vad_flags() {
    let mut flags = MMVAD_FLAGS::new();

    // По умолчанию все нули
    assert_eq!(flags.vad_type(), VAD_TYPE::VadPrivateMemory);
    assert_eq!(flags.protection(), 0);
    assert!(!flags.commit());
    assert!(!flags.private_memory());
    assert!(!flags.no_change());

    // Устанавливаем VAD type
    flags.set_vad_type(VAD_TYPE::VadMapped);
    assert_eq!(flags.vad_type(), VAD_TYPE::VadMapped);

    flags.set_vad_type(VAD_TYPE::VadImage);
    assert_eq!(flags.vad_type(), VAD_TYPE::VadImage);

    // Protection (5 бит, 0-31)
    flags.set_protection(MM_READWRITE);
    assert_eq!(flags.protection(), MM_READWRITE);

    flags.set_protection(31); // max value
    assert_eq!(flags.protection(), 31);

    // Commit bit
    flags.set_commit(true);
    assert!(flags.commit());
    flags.set_commit(false);
    assert!(!flags.commit());

    // Private memory bit
    flags.set_private_memory(true);
    assert!(flags.private_memory());

    // No change bit
    flags.set_no_change(true);
    assert!(flags.no_change());
}

/// Проверка что биты VAD flags не перекрываются
fn test_vad_flags_isolation() {
    let mut flags = MMVAD_FLAGS::new();

    // Устанавливаем все биты
    flags.set_vad_type(VAD_TYPE::VadLargePages); // bits 0-2 = 4
    flags.set_protection(0x1F); // bits 3-7 = all set
    flags.set_commit(true); // bit 8
    flags.set_private_memory(true); // bit 9
    flags.set_no_change(true); // bit 10

    // Проверяем что все значения сохранились
    assert_eq!(flags.vad_type(), VAD_TYPE::VadLargePages);
    assert_eq!(flags.protection(), 0x1F);
    assert!(flags.commit());
    assert!(flags.private_memory());
    assert!(flags.no_change());

    // Меняем один бит и проверяем что остальные не изменились
    flags.set_protection(0);
    assert_eq!(flags.vad_type(), VAD_TYPE::VadLargePages);
    assert_eq!(flags.protection(), 0);
    assert!(flags.commit());
    assert!(flags.private_memory());
    assert!(flags.no_change());
}

/// Проверка константы allocation granularity
fn test_allocation_granularity() {
    assert_eq!(MM_ALLOCATION_GRANULARITY, 64 * 1024);
    // Должна быть кратна PAGE_SIZE
    assert_eq!(MM_ALLOCATION_GRANULARITY % PAGE_SIZE, 0);
}

// =============================================================================
// Address Space Layout Tests
// =============================================================================

/// Проверка констант адресного пространства
fn test_address_space_layout() {
    // User space
    assert_eq!(MM_LOWEST_USER_ADDRESS, 0x0000_0000_0001_0000);
    assert_eq!(MM_HIGHEST_USER_ADDRESS, 0x0000_7FFF_FFFF_FFFF);
    assert!(MM_LOWEST_USER_ADDRESS < MM_HIGHEST_USER_ADDRESS);

    // System space
    assert_eq!(MM_SYSTEM_RANGE_START, 0xFFFF_8000_0000_0000);
    assert!(MM_SYSTEM_RANGE_START > MM_HIGHEST_USER_ADDRESS);

    // Hyperspace
    assert!(HYPER_SPACE > MM_SYSTEM_RANGE_START);
    assert!(HYPER_SPACE < HYPER_SPACE_END);

    // System PTE area
    assert!(MM_SYSTEM_PTE_START > HYPER_SPACE);
    assert!(MM_SYSTEM_PTE_START < MM_SYSTEM_PTE_END);
}

/// Проверка что user space canonical (нет дыры в середине)
fn test_user_space_canonical() {
    // Младшие 47 бит user space должны быть sign-extended в 0
    assert_eq!(MM_LOWEST_USER_ADDRESS >> 47, 0);
    assert_eq!(MM_HIGHEST_USER_ADDRESS >> 47, 0);

    // Kernel space sign-extended в 1
    assert_ne!(MM_SYSTEM_RANGE_START >> 47, 0);
}

// =============================================================================
// Edge Cases & Stress Tests
// =============================================================================

/// Тест граничных значений для всех функций
fn test_edge_values() {
    // Ноль
    assert_eq!(page_align(0), 0);
    assert_eq!(page_align_up(0), 0);
    assert_eq!(address_to_pfn(0), 0);
    assert_eq!(bytes_to_pages(0), 0);

    // Максимальные значения для usize на 64-bit
    // page_align не должен паниковать на больших значениях
    let large: usize = 0xFFFF_FFFF_FFFF_0000;
    let _ = page_align(large);
    let _ = page_align_up(large);

    // PTE индексы для граничных адресов
    assert_eq!(mm_get_pte_index(0xFFFF_FFFF_FFFF_FFFF), 511);
    assert_eq!(mm_get_pde_index(0xFFFF_FFFF_FFFF_FFFF), 511);
}

/// Тест memory protection констант
fn test_protection_constants() {
    // MM protection mask (внутренний формат NT)
    assert_eq!(MM_PROTECT_ACCESS, 7); // 3 bits для базовой защиты

    // Проверяем что MM_* константы в правильном диапазоне
    assert!(MM_ZERO_ACCESS <= MM_PROTECT_ACCESS);
    assert!(MM_READONLY <= MM_PROTECT_ACCESS);
    assert!(MM_READWRITE <= MM_PROTECT_ACCESS);
    assert!(MM_EXECUTE <= MM_PROTECT_ACCESS);

    // PAGE_* флаги - степени двойки (bitflags)
    assert_eq!(PAGE_NOACCESS.count_ones(), 1);
    assert_eq!(PAGE_READONLY.count_ones(), 1);
    assert_eq!(PAGE_READWRITE.count_ones(), 1);
    assert_eq!(PAGE_EXECUTE.count_ones(), 1);

    // MEM_* allocation flags не должны конфликтовать
    assert_eq!(MEM_COMMIT & MEM_RESERVE, 0);
    assert_eq!(MEM_COMMIT & MEM_RELEASE, 0);
    assert_eq!(MEM_RESERVE & MEM_RELEASE, 0);

    // MEM_COMMIT и MEM_RESERVE могут комбинироваться
    let combined = MEM_COMMIT | MEM_RESERVE;
    assert_eq!(combined & MEM_COMMIT, MEM_COMMIT);
    assert_eq!(combined & MEM_RESERVE, MEM_RESERVE);
}

// =============================================================================
// Реестр тестов MM
// =============================================================================

/// Все тесты Memory Manager
pub static MM_TESTS: &[KernelTest] = &[
    // Types tests
    KernelTest {
        name: "page_constants",
        module: "mm::types",
        test_fn: test_page_constants,
    },
    KernelTest {
        name: "page_align_basic",
        module: "mm::types",
        test_fn: test_page_align_basic,
    },
    KernelTest {
        name: "page_align_edge_cases",
        module: "mm::types",
        test_fn: test_page_align_edge_cases,
    },
    KernelTest {
        name: "pfn_conversions",
        module: "mm::types",
        test_fn: test_pfn_conversions,
    },
    KernelTest {
        name: "bytes_pages_conversions",
        module: "mm::types",
        test_fn: test_bytes_pages_conversions,
    },
    KernelTest {
        name: "physical_address",
        module: "mm::types",
        test_fn: test_physical_address,
    },
    // PTE tests
    KernelTest {
        name: "pte_constants",
        module: "mm::pte",
        test_fn: test_pte_constants,
    },
    KernelTest {
        name: "pte_index_extraction",
        module: "mm::pte",
        test_fn: test_pte_index_extraction,
    },
    KernelTest {
        name: "pte_index_kernel_addresses",
        module: "mm::pte",
        test_fn: test_pte_index_kernel_addresses,
    },
    KernelTest {
        name: "mmpte_structure",
        module: "mm::pte",
        test_fn: test_mmpte_structure,
    },
    KernelTest {
        name: "mmpte_pfn_operations",
        module: "mm::pte",
        test_fn: test_mmpte_pfn_operations,
    },
    // VAD tests
    KernelTest {
        name: "vad_flags",
        module: "mm::vad",
        test_fn: test_vad_flags,
    },
    KernelTest {
        name: "vad_flags_isolation",
        module: "mm::vad",
        test_fn: test_vad_flags_isolation,
    },
    KernelTest {
        name: "allocation_granularity",
        module: "mm::vad",
        test_fn: test_allocation_granularity,
    },
    // Address space tests
    KernelTest {
        name: "address_space_layout",
        module: "mm::layout",
        test_fn: test_address_space_layout,
    },
    KernelTest {
        name: "user_space_canonical",
        module: "mm::layout",
        test_fn: test_user_space_canonical,
    },
    // Edge cases
    KernelTest {
        name: "edge_values",
        module: "mm::edge",
        test_fn: test_edge_values,
    },
    KernelTest {
        name: "protection_constants",
        module: "mm::edge",
        test_fn: test_protection_constants,
    },
];
