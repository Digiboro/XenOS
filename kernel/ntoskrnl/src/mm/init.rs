//! Memory Manager Initialization
//!
//! Инициализация Memory Manager.
//!
//! Источники:
//! - ReactOS: mm/ARM3/mminit.c

use core::sync::atomic::AtomicU32;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;

use super::pfn::MM_FREE_PAGE_LIST_HEAD;
use super::pfn::MM_NUMBER_OF_PHYSICAL_PAGES;
use super::pfn::MM_PFN_DATABASE;
use super::pfn::MMPFN;
use super::pfn::mi_insert_page_in_list;
use super::types::PAGE_SHIFT;
use super::types::PAGE_SIZE;
use super::types::bytes_to_pages;
use crate::ke::globals::ke_get_loader_block;

// =============================================================================
// Global Variables
// =============================================================================

/// Фаза инициализации MM
pub static MM_INITIALIZATION_PHASE: AtomicU32 = AtomicU32::new(0);

/// Начало NonPaged pool
pub static MM_NON_PAGED_POOL_START: AtomicUsize = AtomicUsize::new(0);
/// Конец NonPaged pool
pub static MM_NON_PAGED_POOL_END: AtomicUsize = AtomicUsize::new(0);
/// Размер NonPaged pool
pub static MM_SIZE_OF_NON_PAGED_POOL_IN_BYTES: AtomicUsize = AtomicUsize::new(0);

/// Начало Paged pool
pub static MM_PAGED_POOL_START: AtomicUsize = AtomicUsize::new(0);
/// Конец Paged pool
pub static MM_PAGED_POOL_END: AtomicUsize = AtomicUsize::new(0);
/// Размер Paged pool
pub static MM_SIZE_OF_PAGED_POOL_IN_BYTES: AtomicUsize = AtomicUsize::new(0);

/// System PTE start
pub static MM_SYSTEM_PTE_START: AtomicUsize = AtomicUsize::new(0);

/// Lowest physical page
pub static MM_LOWEST_PHYSICAL_PAGE: AtomicUsize = AtomicUsize::new(0);
/// Highest physical page
pub static MM_HIGHEST_PHYSICAL_PAGE: AtomicUsize = AtomicUsize::new(0);

/// Low memory threshold
pub static MM_LOW_MEMORY_THRESHOLD: AtomicUsize = AtomicUsize::new(0);
/// High memory threshold
pub static MM_HIGH_MEMORY_THRESHOLD: AtomicUsize = AtomicUsize::new(0);
/// Minimum free pages
pub static MM_MINIMUM_FREE_PAGES: AtomicUsize = AtomicUsize::new(0);

/// HHDM (Higher Half Direct Map) offset
/// Используется для конвертации физических адресов в виртуальные
pub static MM_HHDM_OFFSET: AtomicUsize = AtomicUsize::new(0);

// =============================================================================
// Self-Referencing PML4 Initialization
// =============================================================================

use super::pte::PXE_BASE;
use super::pte::PXI_SELF;

/// Флаг что self-referencing PML4 инициализирован
pub static MM_SELF_MAPPING_INITIALIZED: AtomicU32 = AtomicU32::new(0);

/// MiInitializeSelfMap
///
/// Инициализирует self-referencing PML4 (recursive mapping).
///
/// # Механизм
///
/// NT x64 использует self-map slot в PML4, который указывает на физический адрес
/// самой PML4. Это создаёт «окно» для доступа ко всем page tables через
/// фиксированные виртуальные адреса:
///
/// - PML4[PXI_SELF] = физ. адрес PML4 | Present | Writable
///
/// После этого CPU при трансляции адресов из диапазона PTE_BASE..PXE_BASE
/// будет «проваливаться» через self-reference entry и интерпретировать
/// page tables как данные.
///
/// # Конфигурация XenCore
///
/// - `PXI_SELF = 493` — индекс self-reference в PML4
/// - `PTE_BASE = 0xFFFFF68000000000` — база для mm_get_pte_address()
///
/// # После инициализации работают функции
///
/// - `mm_get_pte_address(va)` — возвращает указатель на PTE для va
/// - `mm_get_pde_address(va)` — возвращает указатель на PDE для va
/// - `mm_get_ppe_address(va)` — возвращает указатель на PDPTE для va
/// - `mm_get_pxe_address(va)` — возвращает указатель на PML4E для va
///
/// # Safety
///
/// - Должен вызываться после инициализации HHDM (`mm_set_hhdm_offset`)
/// - Должен вызываться до использования `mm_get_pte_address`, `mm_map_io_space`
/// - Вызывается один раз на BSP
pub unsafe fn mm_init_self_mapping() -> bool {
    unsafe {
        use crate::arch::x86_64::cpu::cr4;
        use crate::arch::x86_64::cpu::read_cr3;
        use crate::arch::x86_64::cpu::read_cr4;
        use crate::arch::x86_64::cpu::write_cr4;

        // Читаем CR3 для получения физического адреса PML4
        let cr3 = read_cr3();
        let pml4_phys = cr3 & !0xFFF; // Убираем флаги (PCID и др.)

        // Получаем HHDM offset для доступа к PML4
        let hhdm = mm_get_hhdm_offset() as u64;
        if hhdm == 0 {
            return false;
        }

        // Вычисляем виртуальный адрес PML4 через HHDM
        let pml4_virt = (hhdm + pml4_phys) as *mut u64;

        // Устанавливаем self-map slot (PXI_SELF) = физический адрес PML4 | flags
        // Flags: Present (0x1) | Writable (0x2) = 0x3
        // Не устанавливаем NX т.к. это page table entry, не данные
        let pml4_entry = pml4_phys | 0x3; // Present | Writable

        // Записываем self-reference entry
        core::ptr::write_volatile(pml4_virt.add(PXI_SELF), pml4_entry);

        // Memory fence для гарантии записи в память
        core::sync::atomic::fence(Ordering::SeqCst);

        // Полный сброс TLB (включая глобальные страницы)
        // Для этого временно сбрасываем CR4.PGE, затем восстанавливаем
        let cr4_val = read_cr4();
        if cr4_val & cr4::PGE != 0 {
            // Сбрасываем PGE для flush глобальных страниц
            write_cr4(cr4_val & !cr4::PGE);
            // Восстанавливаем PGE - это также сбросит TLB
            write_cr4(cr4_val);
        } else {
            // PGE не установлен, простой reload CR3
            let cr3_val = read_cr3();
            crate::arch::x86_64::cpu::write_cr3(cr3_val);
        }

        MM_SELF_MAPPING_INITIALIZED.store(1, Ordering::Release);

        // Верификация: проверяем что self-map работает
        // Читаем обратно через HHDM (не через self-ref) для проверки записи
        let read_back_hhdm = core::ptr::read_volatile(pml4_virt.add(PXI_SELF));
        if read_back_hhdm != pml4_entry {
            // Запись в PML4 не удалась
            return false;
        }

        // Проверяем self-referencing через PXE_BASE (4 уровня самореференса)
        // PXE_BASE + (PXI_SELF << 3) должен указывать на наш новый entry
        let pxe_addr = (PXE_BASE + ((PXI_SELF as u64) << 3)) as *const u64;
        let read_entry = core::ptr::read_volatile(pxe_addr);

        // ВАЖНО: При чтении через self-ref CPU автоматически устанавливает Accessed bit (0x20)
        // на каждом уровне трансляции (4 раза для полного self-ref).
        // Поэтому сравниваем с маской, игнорируя Accessed и Dirty биты.
        const ACCESSED_DIRTY_MASK: u64 = 0x60; // bits 5 (Accessed) и 6 (Dirty)
        if (read_entry & !ACCESSED_DIRTY_MASK) != (pml4_entry & !ACCESSED_DIRTY_MASK) {
            // Self-map не работает
            crate::kd::dbg_print("\n       SELFMAP DEBUG: CR3=");
            crate::kd::dbg_print_hex(cr3);
            crate::kd::dbg_print(" HHDM=");
            crate::kd::dbg_print_hex(hhdm);
            crate::kd::dbg_print("\n       PML4[493] via HHDM=");
            crate::kd::dbg_print_hex(read_back_hhdm);
            crate::kd::dbg_print(" via PXE_BASE=");
            crate::kd::dbg_print_hex(read_entry);
            crate::kd::dbg_print("\n       Expected=");
            crate::kd::dbg_print_hex(pml4_entry);
            crate::kd::dbg_print("\n");
            return false;
        }

        true
    }
}

/// Проверяет инициализирован ли self-referencing PML4
#[inline]
pub fn mm_is_self_mapping_initialized() -> bool {
    MM_SELF_MAPPING_INITIALIZED.load(Ordering::Acquire) != 0
}

// =============================================================================
// Physical/Virtual Address Conversion
// =============================================================================

/// Устанавливает HHDM offset
///
/// Должен вызываться один раз при инициализации из main.rs
pub unsafe fn init_hhdm_offset(offset: usize) {
    MM_HHDM_OFFSET.store(offset, Ordering::Release);
}

/// Устанавливает HHDM offset (публичная функция)
///
/// Должен вызываться один раз при инициализации из main.rs
pub fn mm_set_hhdm_offset(offset: u64) {
    MM_HHDM_OFFSET.store(offset as usize, Ordering::Release);
}

/// Возвращает HHDM offset
#[inline]
pub fn mm_get_hhdm_offset() -> usize {
    MM_HHDM_OFFSET.load(Ordering::Acquire)
}

/// Конвертирует физический адрес в виртуальный через HHDM
#[inline]
pub fn mm_physical_to_virtual(physical: u64) -> *mut u8 {
    let hhdm = MM_HHDM_OFFSET.load(Ordering::Acquire);
    (physical as usize + hhdm) as *mut u8
}

/// Конвертирует PFN в виртуальный адрес через HHDM
#[inline]
pub fn mm_pfn_to_virtual(pfn: usize) -> *mut u8 {
    let physical = pfn * PAGE_SIZE;
    mm_physical_to_virtual(physical as u64)
}

// =============================================================================
// Memory Region from Bootloader
// =============================================================================

/// Тип региона памяти
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryRegionType {
    /// Доступная RAM
    Usable = 0,
    /// Зарезервировано (BIOS, ACPI и т.д.)
    Reserved = 1,
    /// ACPI reclaimable
    AcpiReclaimable = 2,
    /// ACPI NVS
    AcpiNvs = 3,
    /// Bad memory
    BadMemory = 4,
    /// Bootloader reclaimable
    BootloaderReclaimable = 5,
    /// Kernel and modules
    Kernel = 6,
    /// Framebuffer
    Framebuffer = 7,
}

/// Регион памяти
#[derive(Clone, Copy, Debug)]
pub struct MemoryRegion {
    pub base: u64,
    pub length: u64,
    pub region_type: MemoryRegionType,
}

// =============================================================================
// MmInitSystem
// =============================================================================

/// Инициализирует Memory Manager
///
/// Вызывается несколько раз в разных фазах:
/// - Phase 0: Ранняя инициализация (PFN database, basic pool)
/// - Phase 1: Основная инициализация (paged pool, system PTEs)
/// - Phase 2: Финальная инициализация
pub fn mm_init_system(phase: u32) -> bool {
    match phase {
        0 => mm_init_system_phase0(),
        1 => mm_init_system_phase1(),
        2 => mm_init_system_phase2(),
        _ => false,
    }
}

/// Phase 0: Ранняя инициализация
fn mm_init_system_phase0() -> bool {
    // Устанавливаем пороговые значения по умолчанию
    MM_LOW_MEMORY_THRESHOLD.store(16, Ordering::Release); // 16 pages = 64KB
    MM_HIGH_MEMORY_THRESHOLD.store(64, Ordering::Release); // 64 pages = 256KB
    MM_MINIMUM_FREE_PAGES.store(8, Ordering::Release); // 8 pages = 32KB

    // Инициализируем PFN database из LoaderBlock
    let lpb = ke_get_loader_block();
    if !lpb.is_null() {
        unsafe {
            if !mi_init_pfn_from_loader_block(lpb) {
                return false;
            }
        }
    }

    MM_INITIALIZATION_PHASE.store(1, Ordering::Release);
    true
}

/// Инициализация hyperspace
///
/// Вызывается ПОСЛЕ mm_init_self_mapping(), т.к. нужен self-referencing PML4
/// для получения физических адресов статических таблиц.
pub fn mm_init_hyperspace_after_selfmap() -> bool {
    crate::kd::dbg_print("       mm_init_hyperspace_after_selfmap()... ");
    unsafe {
        if !super::hyperspace::mm_init_hyperspace() {
            crate::kd::dbg_print_fail();
            crate::kd::dbg_print("\n");
            return false;
        }
    }
    crate::kd::dbg_print_ok();
    crate::kd::dbg_print("\n");
    true
}

/// Phase 1: Основная инициализация
fn mm_init_system_phase1() -> bool {
    // Регистрируем Section Object Type в Object Manager
    let status = super::section::mm_create_section_object_type();
    if status != crate::nt::STATUS_SUCCESS {
        // Не фатально - fallback на legacy реализацию
        crate::kd::dbg_print("       Section object type: failed (using legacy)\n");
    } else {
        crate::kd::dbg_print("       Section object type: created\n");
    }

    MM_INITIALIZATION_PHASE.store(2, Ordering::Release);
    true
}

/// Phase 2: Финальная инициализация
fn mm_init_system_phase2() -> bool {
    MM_INITIALIZATION_PHASE.store(3, Ordering::Release);
    true
}

// =============================================================================
// PFN Initialization from LoaderBlock
// =============================================================================

/// MiInitPfnFromLoaderBlock
///
/// Инициализирует PFN database из MemoryDescriptorListHead в LoaderBlock.
/// Это основной путь инициализации физической памяти в NT-совместимом ядре.
///
/// # Алгоритм
///
/// 1. **Первый проход**: определение границ физической памяти
///    - Находим lowest/highest PFN из LoaderFree регионов
///    - Считаем общее количество usable страниц
///
/// 2. **Выделение PFN database**: находим достаточно большой LoaderFree регион
///
/// 3. **Второй проход**: заполнение free list
///    - Все LoaderFree страницы добавляются в MM_FREE_PAGE_LIST_HEAD
///    - Исключаются: страница 0, страницы PFN database
///
/// # Типы памяти (из LoaderBlock)
///
/// | Тип                    | Действие                              |
/// |------------------------|---------------------------------------|
/// | LoaderFree             | Добавляется в free list               |
/// | LoaderFirmwareTemporary| Зарезервировано (reclaim в Phase1)    |
/// | LoaderSystemCode       | Ядро, не трогать                      |
/// | LoaderHalCode          | HAL, не трогать                       |
/// | LoaderBootDriver       | Boot drivers, не трогать              |
/// | LoaderRegistryData     | Registry hive, не трогать             |
/// | LoaderNlsData          | NLS tables, не трогать                |
/// | остальные              | Не включать в PFN database            |
///
/// # Safety
///
/// - lpb должен быть валидным указателем на LOADER_PARAMETER_BLOCK
/// - MemoryDescriptorListHead должен быть корректно инициализирован
/// - HHDM должен быть настроен до вызова
unsafe fn mi_init_pfn_from_loader_block(lpb: *const ntldr::LOADER_PARAMETER_BLOCK) -> bool {
    unsafe {
        use ntldr::LIST_ENTRY;
        use ntldr::MEMORY_ALLOCATION_DESCRIPTOR;
        use ntldr::MEMORY_TYPE;

        let lpb_ref = &*lpb;
        let hhdm_offset = lpb_ref.HhdmOffset;

        // Проверяем что список не пуст
        let list_head = &lpb_ref.MemoryDescriptorListHead;

        if list_head.Flink.is_null() {
            return false;
        }

        // ВАЖНО: Указатели в списке содержат физические адреса (из winload).
        // Нужно конвертировать их через HHDM для доступа из ядра.
        let list_head_phys = list_head as *const _ as u64;
        let first_flink_phys = list_head.Flink as u64;

        if first_flink_phys == list_head_phys {
            // Список пуст
            return false;
        }

        // =========================================================================
        // Первый проход: находим границы физической памяти
        // =========================================================================

        let mut lowest_pfn = usize::MAX;
        let mut highest_pfn = 0usize;
        let mut usable_pages = 0usize;
        let mut descriptor_count = 0usize;

        let mut current_phys = first_flink_phys;

        while current_phys != list_head_phys {
            let current = (hhdm_offset + current_phys) as *mut LIST_ENTRY;
            let desc = containing_record::<MEMORY_ALLOCATION_DESCRIPTOR, LIST_ENTRY>(
                current,
                offset_of_list_entry(),
            );

            let base_pfn = (*desc).BasePage as usize;
            let page_count = (*desc).PageCount as usize;
            let end_pfn = base_pfn + page_count;
            let mem_type = (*desc).MemoryType;

            // Границы определяем только по usable RAM (LoaderFree)
            // MMIO и firmware регионы имеют высокие адреса и не должны влиять на PFN db
            if mi_is_usable_memory_type(mem_type) {
                if base_pfn < lowest_pfn {
                    lowest_pfn = base_pfn;
                }
                if end_pfn > highest_pfn {
                    highest_pfn = end_pfn;
                }
                usable_pages += page_count;
            }

            descriptor_count += 1;
            current_phys = (*current).Flink as u64;

            // Защита от бесконечного цикла
            if descriptor_count > 10000 {
                break;
            }
        }

        if highest_pfn == 0 || lowest_pfn == usize::MAX || usable_pages == 0 {
            return false;
        }

        // =========================================================================
        // Вычисляем размер и расположение PFN database
        // =========================================================================

        let pfn_count = highest_pfn;
        let pfn_db_size = pfn_count * core::mem::size_of::<MMPFN>();
        let pfn_db_pages = bytes_to_pages(pfn_db_size);

        // Находим место для PFN database в LoaderFree памяти
        let mut pfn_db_base_pfn = 0usize;

        current_phys = first_flink_phys;
        while current_phys != list_head_phys {
            let current = (hhdm_offset + current_phys) as *mut LIST_ENTRY;
            let desc = containing_record::<MEMORY_ALLOCATION_DESCRIPTOR, LIST_ENTRY>(
                current,
                offset_of_list_entry(),
            );

            if (*desc).MemoryType == MEMORY_TYPE::LoaderFree {
                let page_count = (*desc).PageCount as usize;
                // Нужно место для PFN db + запас для других ранних структур
                if page_count >= pfn_db_pages + 64 {
                    pfn_db_base_pfn = (*desc).BasePage as usize;
                    // Пропускаем страницу 0 (NULL pointer guard)
                    if pfn_db_base_pfn == 0 {
                        pfn_db_base_pfn = 1;
                    }
                    break;
                }
            }
            current_phys = (*current).Flink as u64;
        }

        if pfn_db_base_pfn == 0 {
            // Не нашли достаточно большой LoaderFree регион для PFN database
            return false;
        }

        // Конвертируем физический адрес PFN db в виртуальный через HHDM
        let pfn_db_phys = (pfn_db_base_pfn as u64) << PAGE_SHIFT;
        let pfn_db_virt = (hhdm_offset + pfn_db_phys) as *mut u8;
        let pfn_db_end_pfn = pfn_db_base_pfn + pfn_db_pages;

        // Сохраняем границы
        MM_LOWEST_PHYSICAL_PAGE.store(lowest_pfn, Ordering::Release);
        MM_HIGHEST_PHYSICAL_PAGE.store(highest_pfn, Ordering::Release);
        MM_NUMBER_OF_PHYSICAL_PAGES.store(pfn_count, Ordering::Release);

        // Сохраняем информацию о PFN database для отладки
        MM_PFN_DATABASE_BASE_PFN.store(pfn_db_base_pfn, Ordering::Release);
        MM_PFN_DATABASE_SIZE_PAGES.store(pfn_db_pages, Ordering::Release);

        // =========================================================================
        // Инициализируем PFN database
        // =========================================================================

        let db = &mut *MM_PFN_DATABASE.get();
        db.init(pfn_db_virt, 0, highest_pfn);

        // =========================================================================
        // Второй проход: добавляем usable страницы в free list
        // =========================================================================

        current_phys = first_flink_phys;
        while current_phys != list_head_phys {
            let current = (hhdm_offset + current_phys) as *mut LIST_ENTRY;
            let desc = containing_record::<MEMORY_ALLOCATION_DESCRIPTOR, LIST_ENTRY>(
                current,
                offset_of_list_entry(),
            );

            // Только LoaderFree добавляем в free list
            // Остальные типы будут reclaimed позже или остаются занятыми
            if (*desc).MemoryType == MEMORY_TYPE::LoaderFree {
                let base_pfn = (*desc).BasePage as usize;
                let page_count = (*desc).PageCount as usize;
                let end_pfn = base_pfn + page_count;

                for pfn in base_pfn..end_pfn {
                    // Пропускаем:
                    // - страницу 0 (NULL pointer guard)
                    // - страницы занятые PFN database
                    if pfn == 0 {
                        continue;
                    }
                    if pfn >= pfn_db_base_pfn && pfn < pfn_db_end_pfn {
                        continue;
                    }

                    let free_list = &mut *MM_FREE_PAGE_LIST_HEAD.get();
                    mi_insert_page_in_list(free_list, pfn);
                }
            }

            current_phys = (*current).Flink as u64;
        }

        true
    }
}

/// Проверяет, является ли тип памяти usable для PFN boundaries
///
/// В этих регионах находится реальная RAM, которая может быть использована.
#[inline]
fn mi_is_usable_memory_type(mem_type: ntldr::MEMORY_TYPE) -> bool {
    use ntldr::MEMORY_TYPE;
    matches!(mem_type, MEMORY_TYPE::LoaderFree)
}

/// PFN database base для отладки
pub static MM_PFN_DATABASE_BASE_PFN: AtomicUsize = AtomicUsize::new(0);
/// PFN database size в страницах
pub static MM_PFN_DATABASE_SIZE_PAGES: AtomicUsize = AtomicUsize::new(0);

/// Вычисляет offset поля ListEntry в MEMORY_ALLOCATION_DESCRIPTOR
#[inline]
const fn offset_of_list_entry() -> usize {
    0 // ListEntry - первое поле в структуре
}

/// CONTAINING_RECORD - получает указатель на структуру по указателю на её поле
#[inline]
unsafe fn containing_record<T, F>(field_ptr: *mut F, field_offset: usize) -> *mut T {
    unsafe { (field_ptr as *mut u8).sub(field_offset) as *mut T }
}

// =============================================================================
// PFN Database Initialization
// =============================================================================

/// Инициализирует PFN database из карты памяти загрузчика
///
/// # Arguments
/// * `regions` - Массив регионов памяти от загрузчика
/// * `pfn_db_base` - Виртуальный адрес для размещения PFN database
/// * `hhdm_offset` - Смещение HHDM (Higher Half Direct Map)
pub unsafe fn mi_initialize_pfn_database(
    regions: &[MemoryRegion],
    pfn_db_base: *mut u8,
    _hhdm_offset: u64,
) -> bool {
    unsafe {
        // Находим границы физической памяти
        let mut lowest_pfn = usize::MAX;
        let mut highest_pfn = 0usize;
        let mut total_pages = 0usize;
        let mut usable_pages = 0usize;

        for region in regions {
            let start_pfn = (region.base >> PAGE_SHIFT) as usize;
            let end_pfn = ((region.base + region.length) >> PAGE_SHIFT) as usize;

            if start_pfn < lowest_pfn {
                lowest_pfn = start_pfn;
            }
            if end_pfn > highest_pfn {
                highest_pfn = end_pfn;
            }

            if region.region_type == MemoryRegionType::Usable
                || region.region_type == MemoryRegionType::BootloaderReclaimable
            {
                usable_pages += end_pfn - start_pfn;
            }

            total_pages = highest_pfn - lowest_pfn;
        }

        // Сохраняем границы
        MM_LOWEST_PHYSICAL_PAGE.store(lowest_pfn, Ordering::Release);
        MM_HIGHEST_PHYSICAL_PAGE.store(highest_pfn, Ordering::Release);
        MM_NUMBER_OF_PHYSICAL_PAGES.store(total_pages, Ordering::Release);

        // Инициализируем PFN database
        let db = &mut *MM_PFN_DATABASE.get();
        db.init(pfn_db_base, lowest_pfn, highest_pfn);

        // Добавляем usable страницы в free list
        for region in regions {
            if region.region_type == MemoryRegionType::Usable {
                let start_pfn = (region.base >> PAGE_SHIFT) as usize;
                let end_pfn = ((region.base + region.length) >> PAGE_SHIFT) as usize;

                // Пропускаем первую страницу (обычно NULL pointer guard)
                let start = if start_pfn == 0 { 1 } else { start_pfn };

                for pfn in start..end_pfn {
                    let free_list = &mut *MM_FREE_PAGE_LIST_HEAD.get();
                    mi_insert_page_in_list(free_list, pfn);
                }
            }
        }

        true
    }
}

// =============================================================================
// Pool Initialization
// =============================================================================

/// Инициализирует NonPaged pool
pub unsafe fn mi_init_non_paged_pool(base: usize, size: usize) {
    MM_NON_PAGED_POOL_START.store(base, Ordering::Release);
    MM_NON_PAGED_POOL_END.store(base + size, Ordering::Release);
    MM_SIZE_OF_NON_PAGED_POOL_IN_BYTES.store(size, Ordering::Release);
}

/// Инициализирует Paged pool
pub unsafe fn mi_init_paged_pool(base: usize, size: usize) {
    MM_PAGED_POOL_START.store(base, Ordering::Release);
    MM_PAGED_POOL_END.store(base + size, Ordering::Release);
    MM_SIZE_OF_PAGED_POOL_IN_BYTES.store(size, Ordering::Release);
}

// =============================================================================
// System PTEs
// =============================================================================

/// Счетчик свободных system PTEs
pub static MM_NUMBER_OF_FREE_SYSTEM_PTES: AtomicUsize = AtomicUsize::new(0);

/// Инициализирует system PTEs
pub unsafe fn mi_init_system_ptes(base: usize, count: usize) {
    MM_SYSTEM_PTE_START.store(base, Ordering::Release);
    MM_NUMBER_OF_FREE_SYSTEM_PTES.store(count, Ordering::Release);
}

// =============================================================================
// Query Functions
// =============================================================================

/// Возвращает фазу инициализации
#[inline]
pub fn mm_get_initialization_phase() -> u32 {
    MM_INITIALIZATION_PHASE.load(Ordering::Acquire)
}

/// Возвращает размер NonPaged pool
#[inline]
pub fn mm_get_non_paged_pool_size() -> usize {
    MM_SIZE_OF_NON_PAGED_POOL_IN_BYTES.load(Ordering::Acquire)
}

/// Возвращает размер Paged pool
#[inline]
pub fn mm_get_paged_pool_size() -> usize {
    MM_SIZE_OF_PAGED_POOL_IN_BYTES.load(Ordering::Acquire)
}
