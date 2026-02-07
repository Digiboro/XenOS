//! Memory Descriptor Lists (MDL)
//!
//! Управление MDL для I/O операций.
//!
//! # Архитектура
//!
//! MDL описывает физические страницы, составляющие виртуальный буфер.
//! Используется для DMA и Direct I/O.
//!
//! ```text
//! MDL header:
//! +------------------+
//! | next             | -> следующий MDL в цепочке
//! | size             | размер структуры
//! | mdl_flags        | флаги состояния
//! | process          | владелец буфера
//! | mapped_system_va | system VA (после MmMapLockedPages)
//! | start_va         | оригинальный VA
//! | byte_count       | размер данных
//! | byte_offset      | offset в первой странице
//! +------------------+
//! | PFN[0]           | физические номера страниц
//! | PFN[1]           |
//! | ...              |
//! +------------------+
//! ```
//!
//! # Жизненный цикл
//!
//! 1. `IoAllocateMdl()` — выделение MDL
//! 2. `MmProbeAndLockPages()` — page walk, заполнение PFN array, pinning
//! 3. `MmMapLockedPages()` — отображение в system VA через System PTE
//! 4. ... использование ...
//! 5. `MmUnmapLockedPages()` — удаление отображения
//! 6. `MmUnlockPages()` — unpin страниц
//! 7. `IoFreeMdl()` — освобождение MDL
//!
//! Источники:
//! - ReactOS: mm/ARM3/mdlsup.c
//! - NT6.1: mm/mdlsup.c

use super::pfn::*;
use super::pte::*;
use super::syspte::*;
use super::types::*;
use crate::nt::NTSTATUS;
use crate::nt::PVOID;
use crate::nt::STATUS_ACCESS_VIOLATION;
use crate::nt::STATUS_INVALID_PARAMETER;
use crate::nt::STATUS_SUCCESS;

// =============================================================================
// MDL Flags
// =============================================================================

/// MDL выделен из paged pool
pub const MDL_MAPPED_TO_SYSTEM_VA: u32 = 0x0001;
/// Страницы заблокированы
pub const MDL_PAGES_LOCKED: u32 = 0x0002;
/// MDL описывает non-paged память
pub const MDL_SOURCE_IS_NONPAGED_POOL: u32 = 0x0004;
/// MDL выделен из NonPaged pool
pub const MDL_ALLOCATED_FIXED_SIZE: u32 = 0x0008;
/// Partial MDL
pub const MDL_PARTIAL: u32 = 0x0010;
/// Partial уже отображен
pub const MDL_PARTIAL_HAS_BEEN_MAPPED: u32 = 0x0020;
/// I/O page lock
pub const MDL_IO_PAGE_READ: u32 = 0x0040;
/// Write operation
pub const MDL_WRITE_OPERATION: u32 = 0x0080;
/// Parent MDL
pub const MDL_PARENT_MAPPED_SYSTEM_VA: u32 = 0x0100;
/// Free extra pages on destroy
pub const MDL_FREE_EXTRA_PTES: u32 = 0x0200;
/// Describes I/O space
pub const MDL_DESCRIBES_AWE: u32 = 0x0400;
/// IO space
pub const MDL_IO_SPACE: u32 = 0x0800;
/// Network header
pub const MDL_NETWORK_HEADER: u32 = 0x1000;
/// Mapping can fail
pub const MDL_MAPPING_CAN_FAIL: u32 = 0x2000;
/// Allocated must succeed
pub const MDL_ALLOCATED_MUST_SUCCEED: u32 = 0x4000;
/// Internal
pub const MDL_INTERNAL: u32 = 0x8000;

// =============================================================================
// MDL Structure
// =============================================================================

/// Memory Descriptor List
///
/// Описывает набор физических страниц, которые составляют
/// виртуальный буфер. Используется для DMA и прямого I/O.
#[repr(C)]
pub struct MDL {
    /// Следующий MDL в цепочке
    pub next: *mut MDL,
    /// Размер MDL структуры
    pub size: u16,
    /// Флаги MDL
    pub mdl_flags: u16,
    /// Процесс, владеющий буфером
    pub process: PVOID, // *mut EPROCESS
    /// Отображенный системный адрес
    pub mapped_system_va: PVOID,
    /// Оригинальный виртуальный адрес
    pub start_va: PVOID,
    /// Количество байт
    pub byte_count: u32,
    /// Offset внутри первой страницы
    pub byte_offset: u32,
    // После этого следует массив PFN_NUMBER
}

impl MDL {
    /// Минимальный размер MDL (без PFN array)
    pub const BASE_SIZE: usize = core::mem::size_of::<MDL>();

    /// Вычисляет размер MDL для заданного количества страниц
    #[inline]
    pub const fn size_for_pages(page_count: usize) -> usize {
        Self::BASE_SIZE + page_count * core::mem::size_of::<PFN_NUMBER>()
    }

    /// Вычисляет количество страниц для буфера
    #[inline]
    pub fn pages_for_buffer(virtual_address: PVOID, length: usize) -> usize {
        let offset = (virtual_address as usize) & PAGE_MASK;
        bytes_to_pages(length + offset)
    }

    /// Возвращает указатель на массив PFN
    #[inline]
    pub fn pfn_array(&self) -> *mut PFN_NUMBER {
        unsafe { (self as *const Self as *const u8).add(Self::BASE_SIZE) as *mut PFN_NUMBER }
    }

    /// Возвращает количество страниц в MDL
    #[inline]
    pub fn page_count(&self) -> usize {
        let offset = self.byte_offset as usize;
        bytes_to_pages(self.byte_count as usize + offset)
    }

    /// Возвращает системный виртуальный адрес
    #[inline]
    pub fn system_address(&self) -> PVOID {
        if self.mapped_system_va.is_null() {
            // Если не отображен, вернуть оригинальный адрес
            // (работает только для non-paged pool)
            if (self.mdl_flags as u32 & MDL_SOURCE_IS_NONPAGED_POOL) != 0 {
                unsafe { (self.start_va as *mut u8).add(self.byte_offset as usize) as PVOID }
            } else {
                core::ptr::null_mut()
            }
        } else {
            self.mapped_system_va
        }
    }
}

// =============================================================================
// IoAllocateMdl
// =============================================================================

/// Выделяет MDL для буфера
///
/// # Arguments
/// * `virtual_address` - Виртуальный адрес буфера
/// * `length` - Размер буфера в байтах
/// * `secondary_buffer` - TRUE если это secondary buffer (добавляется к цепочке)
/// * `charge_quota` - TRUE если нужно учитывать квоту
/// * `irp` - IRP для связывания (или NULL)
pub fn io_allocate_mdl(
    virtual_address: PVOID,
    length: u32,
    _secondary_buffer: bool,
    _charge_quota: bool,
    _irp: PVOID,
) -> *mut MDL {
    if length == 0 {
        return core::ptr::null_mut();
    }

    // Вычисляем количество страниц
    let page_count = MDL::pages_for_buffer(virtual_address, length as usize);

    // Вычисляем размер MDL
    let mdl_size = MDL::size_for_pages(page_count);

    // Выделяем память для MDL
    let mdl = crate::ex::pool::ex_allocate_pool_with_tag(
        crate::ex::pool::POOL_TYPE::NonPagedPool,
        mdl_size,
        u32::from_le_bytes(*b"lldM"), // 'Mdll'
    ) as *mut MDL;

    if mdl.is_null() {
        return core::ptr::null_mut();
    }

    unsafe {
        // Обнуляем MDL
        core::ptr::write_bytes(mdl, 0, mdl_size);

        // Инициализируем поля
        (*mdl).next = core::ptr::null_mut();
        (*mdl).size = mdl_size as u16;
        (*mdl).mdl_flags = 0;
        (*mdl).process = core::ptr::null_mut();
        (*mdl).mapped_system_va = core::ptr::null_mut();
        (*mdl).start_va = (virtual_address as usize & !PAGE_MASK) as PVOID;
        (*mdl).byte_count = length;
        (*mdl).byte_offset = (virtual_address as usize & PAGE_MASK) as u32;
    }

    mdl
}

/// Освобождает MDL
pub fn io_free_mdl(mdl: *mut MDL) {
    if mdl.is_null() {
        return;
    }

    unsafe {
        // Если страницы заблокированы - разблокировать
        if ((*mdl).mdl_flags as u32 & MDL_PAGES_LOCKED) != 0 {
            mm_unlock_pages(mdl);
        }

        // Если отображен - удалить отображение
        if ((*mdl).mdl_flags as u32 & MDL_MAPPED_TO_SYSTEM_VA) != 0 {
            mm_unmap_locked_pages((*mdl).mapped_system_va, mdl);
        }

        // Освобождаем память
        crate::ex::pool::ex_free_pool_with_tag(mdl as PVOID, u32::from_le_bytes(*b"lldM"));
    }
}

// =============================================================================
// MmProbeAndLockPages
// =============================================================================

/// Lock mode для MmProbeAndLockPages
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LOCK_OPERATION {
    IoReadAccess = 0,
    IoWriteAccess = 1,
    IoModifyAccess = 2,
}

/// MmProbeAndLockPages
///
/// Проверяет и блокирует страницы, описанные MDL.
///
/// Выполняет page walk по таблицам страниц, заполняет PFN array
/// и увеличивает reference count для каждой страницы (pinning).
///
/// # Аргументы
/// * `mdl` - указатель на MDL
/// * `access_mode` - KernelMode (0) или UserMode (1)
/// * `operation` - тип операции (Read/Write/Modify)
///
/// # Возвращает
/// STATUS_SUCCESS при успехе, иначе код ошибки.
pub fn mm_probe_and_lock_pages(
    mdl: *mut MDL,
    access_mode: u8, // KernelMode = 0, UserMode = 1
    operation: LOCK_OPERATION,
) -> NTSTATUS {
    if mdl.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    unsafe {
        let mdl_ref = &mut *mdl;

        // Проверяем, не заблокированы ли уже
        if (mdl_ref.mdl_flags as u32 & MDL_PAGES_LOCKED) != 0 {
            return STATUS_INVALID_PARAMETER;
        }

        let page_count = mdl_ref.page_count();
        let pfn_array = mdl_ref.pfn_array();

        // Получаем виртуальный адрес (выровненный по странице)
        let start_va = mdl_ref.start_va as u64;
        
        #[cfg(feature = "trace-mm")]
        {
            crate::kd::dbg_print("[MM] probe_and_lock: start_va=0x");
            crate::kd::dbg_print_hex(start_va);
            crate::kd::dbg_print(" page_count=");
            crate::kd::dbg_print_num(page_count as u64);
            crate::kd::dbg_print("\n");
        }

        // Проверяем write access если требуется
        let need_write = operation == LOCK_OPERATION::IoWriteAccess
            || operation == LOCK_OPERATION::IoModifyAccess;

        // Фаза 1: Page walk — получаем PFN для каждой страницы
        for i in 0..page_count {
            let va = start_va + (i as u64 * PAGE_SIZE as u64);

            // Выполняем page walk
            let pfn = match mi_page_walk(va, access_mode, need_write) {
                Ok(pfn) => pfn,
                Err(status) => {
                    // Откат: unlock уже заблокированных страниц
                    for j in 0..i {
                        let locked_pfn = *pfn_array.add(j);
                        mi_unpin_pfn(locked_pfn);
                    }
                    return status;
                },
            };

            // Сохраняем PFN
            *pfn_array.add(i) = pfn;
        }

        // Фаза 2: Pinning — увеличиваем reference count
        for i in 0..page_count {
            let pfn = *pfn_array.add(i);
            mi_pin_pfn(pfn);
        }

        // Устанавливаем флаги
        mdl_ref.mdl_flags |= MDL_PAGES_LOCKED as u16;

        if need_write {
            mdl_ref.mdl_flags |= MDL_WRITE_OPERATION as u16;
        }

        STATUS_SUCCESS
    }
}

/// Выполняет page walk для получения PFN из VA
///
/// Поддерживает 4KB pages и 2MB large pages.
///
/// # Аргументы
/// * `va` - виртуальный адрес
/// * `access_mode` - режим доступа
/// * `need_write` - требуется ли write access
///
/// # Возвращает
/// Ok(PFN) или Err(NTSTATUS)
unsafe fn mi_page_walk(va: u64, access_mode: u8, need_write: bool) -> Result<PFN_NUMBER, NTSTATUS> {
    unsafe {
        // Для kernel-mode (access_mode == 0) адресов в HHDM диапазоне,
        // используем прямое преобразование без page walk
        const HHDM_BASE: u64 = 0xFFFF8000_00000000;
        const HHDM_END: u64 = 0xFFFFFFFF_80000000;
        
        if access_mode == 0 && va >= HHDM_BASE && va < HHDM_END {
            // HHDM: виртуальный адрес = физический адрес + HHDM_BASE
            let phys_addr = va - HHDM_BASE;
            let pfn = (phys_addr >> PAGE_SHIFT) as PFN_NUMBER;
            
            #[cfg(feature = "trace-mm")]
            {
                crate::kd::dbg_print("[MM] HHDM direct: va=0x");
                crate::kd::dbg_print_hex(va);
                crate::kd::dbg_print(" -> pfn=0x");
                crate::kd::dbg_print_hex(pfn as u64);
                crate::kd::dbg_print("\n");
            }
            
            return Ok(pfn);
        }
        
        // Для не-HHDM адресов - полный page table walk
        // Сначала проверяем PDE на наличие large page
        let pde_ptr = mm_get_pde_address(va) as *const u64;
        let pde_value = core::ptr::read_volatile(pde_ptr);

        // PDE должен быть valid
        if (pde_value & PTE_VALID) == 0 {
            return Err(STATUS_ACCESS_VIOLATION);
        }

        // Проверяем Large Page (2MB)
        if (pde_value & PTE_LARGE_PAGE) != 0 {
            // Large page: PFN берём из PDE
            // Для 2MB page: биты 20:12 VA добавляются к базовому PFN

            // Проверяем права доступа на PDE level
            if access_mode == 1 && (pde_value & PTE_USER) == 0 {
                return Err(STATUS_ACCESS_VIOLATION);
            }
            if need_write && (pde_value & PTE_READWRITE) == 0 {
                return Err(STATUS_ACCESS_VIOLATION);
            }

            // Базовый PFN из PDE (биты 51:21 содержат физический адрес 2MB-aligned)
            let base_pfn = ((pde_value & 0x000F_FFFF_FFE0_0000) >> PAGE_SHIFT) as PFN_NUMBER;
            // Offset внутри 2MB page
            let offset_pfn = ((va >> PAGE_SHIFT) & 0x1FF) as PFN_NUMBER;

            return Ok(base_pfn + offset_pfn);
        }

        // Обычная 4KB page: получаем PTE
        let pte_ptr = mm_get_pte_address(va) as *const u64;
        let pte_value = core::ptr::read_volatile(pte_ptr);

        // Проверяем валидность PTE
        if (pte_value & PTE_VALID) == 0 {
            return Err(STATUS_ACCESS_VIOLATION);
        }

        // Проверяем права доступа
        if access_mode == 1 {
            // UserMode
            if (pte_value & PTE_USER) == 0 {
                return Err(STATUS_ACCESS_VIOLATION);
            }
        }

        // Проверяем write access
        if need_write && (pte_value & PTE_READWRITE) == 0 {
            return Err(STATUS_ACCESS_VIOLATION);
        }

        // Извлекаем PFN из PTE
        let pfn = ((pte_value & PTE_FRAME_MASK) >> PAGE_SHIFT) as PFN_NUMBER;

        Ok(pfn)
    }
}

/// Увеличивает reference count для PFN (pin)
unsafe fn mi_pin_pfn(pfn: PFN_NUMBER) {
    unsafe {
        let db = &mut *MM_PFN_DATABASE.get();

        if let Some(entry) = db.get_mut(pfn) {
            // Увеличиваем reference count
            entry.reference_count = entry.reference_count.saturating_add(1);

            // Если страница была в standby/modified — переводим в active
            if entry.page_location == MMLISTS::StandbyPageList as u8
                || entry.page_location == MMLISTS::ModifiedPageList as u8
            {
                // TODO: удалить из списка и установить ActiveAndValid
                entry.page_location = MMLISTS::ActiveAndValid as u8;
            }
        }
    }
}

/// Уменьшает reference count для PFN (unpin)
unsafe fn mi_unpin_pfn(pfn: PFN_NUMBER) {
    unsafe {
        let db = &mut *MM_PFN_DATABASE.get();

        if let Some(entry) = db.get_mut(pfn) {
            // Уменьшаем reference count
            entry.reference_count = entry.reference_count.saturating_sub(1);
        }
    }
}

/// MmUnlockPages
///
/// Разблокирует страницы, описанные MDL.
///
/// Уменьшает reference count для каждой страницы,
/// позволяя им быть выгруженными при необходимости.
pub fn mm_unlock_pages(mdl: *mut MDL) {
    if mdl.is_null() {
        return;
    }

    unsafe {
        let mdl_ref = &mut *mdl;

        // Проверяем, заблокированы ли
        if (mdl_ref.mdl_flags as u32 & MDL_PAGES_LOCKED) == 0 {
            return;
        }

        let page_count = mdl_ref.page_count();
        let pfn_array = mdl_ref.pfn_array();

        // Уменьшаем reference count для каждой страницы
        for i in 0..page_count {
            let pfn = *pfn_array.add(i);
            mi_unpin_pfn(pfn);
        }

        // Снимаем флаг
        mdl_ref.mdl_flags &= !(MDL_PAGES_LOCKED as u16);
        mdl_ref.mdl_flags &= !(MDL_WRITE_OPERATION as u16);
    }
}

// =============================================================================
// MmMapLockedPages
// =============================================================================

/// Тип memory caching
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MEMORY_CACHING_TYPE {
    MmNonCached = 0,
    MmCached = 1,
    MmWriteCombined = 2,
    MmHardwareCoherentCached = 3,
    MmNonCachedUnordered = 4,
    MmMaximumCacheType = 5,
}

/// MmMapLockedPagesSpecifyCache
///
/// Отображает заблокированные страницы в системное адресное пространство.
///
/// Выделяет System PTE и маппит физические страницы из MDL.
///
/// # Аргументы
/// * `mdl` - указатель на MDL с заблокированными страницами
/// * `access_mode` - KernelMode (0) или UserMode (1)
/// * `cache_type` - тип кэширования
/// * `requested_address` - желаемый адрес (обычно NULL)
/// * `bug_check_on_failure` - вызвать bugcheck при ошибке
/// * `priority` - приоритет выделения
///
/// # Возвращает
/// System VA или NULL при ошибке.
pub fn mm_map_locked_pages_specify_cache(
    mdl: *mut MDL,
    _access_mode: u8,
    cache_type: MEMORY_CACHING_TYPE,
    _requested_address: PVOID,
    _bug_check_on_failure: bool,
    _priority: u32,
) -> PVOID {
    if mdl.is_null() {
        return core::ptr::null_mut();
    }

    unsafe {
        let mdl_ref = &mut *mdl;

        // Страницы должны быть заблокированы
        if (mdl_ref.mdl_flags as u32 & MDL_PAGES_LOCKED) == 0 {
            return core::ptr::null_mut();
        }

        // Если уже отображен - вернуть существующий адрес
        if (mdl_ref.mdl_flags as u32 & MDL_MAPPED_TO_SYSTEM_VA) != 0 {
            return mdl_ref.mapped_system_va;
        }

        // Для non-paged pool память уже отображена напрямую
        if (mdl_ref.mdl_flags as u32 & MDL_SOURCE_IS_NONPAGED_POOL) != 0 {
            return (mdl_ref.start_va as *mut u8).add(mdl_ref.byte_offset as usize) as PVOID;
        }

        let page_count = mdl_ref.page_count();
        let pfn_array = mdl_ref.pfn_array();

        // Проверяем, инициализирован ли System PTE allocator
        if !mi_system_ptes_initialized() {
            // Fallback: возвращаем оригинальный адрес (работает для kernel VA)
            let mapped = (mdl_ref.start_va as *mut u8).add(mdl_ref.byte_offset as usize) as PVOID;
            mdl_ref.mapped_system_va = mapped;
            mdl_ref.mdl_flags |= MDL_MAPPED_TO_SYSTEM_VA as u16;
            return mapped;
        }

        // Резервируем System PTEs
        let system_va = mi_reserve_system_ptes(page_count);
        if system_va == 0 {
            return core::ptr::null_mut();
        }

        // Определяем writable из MDL flags
        let writable = (mdl_ref.mdl_flags as u32 & MDL_WRITE_OPERATION) != 0;

        // Маппим каждую страницу
        for i in 0..page_count {
            let pfn = *pfn_array.add(i);
            let va = system_va + (i as u64 * PAGE_SIZE as u64);

            // Устанавливаем PTE с учётом cache type
            mi_map_system_pte_with_cache(va, pfn, writable, cache_type);
        }

        // Сохраняем результат: system_va + byte_offset
        let mapped = (system_va as *mut u8).add(mdl_ref.byte_offset as usize) as PVOID;

        mdl_ref.mapped_system_va = mapped;
        mdl_ref.mdl_flags |= MDL_MAPPED_TO_SYSTEM_VA as u16;

        mapped
    }
}

/// Маппит страницу с учётом cache type
unsafe fn mi_map_system_pte_with_cache(
    va: u64,
    pfn: PFN_NUMBER,
    writable: bool,
    cache_type: MEMORY_CACHING_TYPE,
) {
    unsafe {
        let pte_ptr = mm_get_pte_address(va) as *mut u64;

        // Базовые флаги
        let mut flags = PTE_VALID;
        if writable {
            flags |= PTE_READWRITE;
        }

        // Флаги кэширования
        match cache_type {
            MEMORY_CACHING_TYPE::MmNonCached | MEMORY_CACHING_TYPE::MmNonCachedUnordered => {
                flags |= PTE_DISABLE_CACHE;
            },
            MEMORY_CACHING_TYPE::MmWriteCombined => {
                // Write-combined: PAT bit + PCD
                flags |= PTE_WRITETHROUGH;
            },
            _ => {
                // Cached - по умолчанию
            },
        }

        let pte_value = ((pfn as u64) << PAGE_SHIFT) | flags;
        core::ptr::write_volatile(pte_ptr, pte_value);

        // Инвалидируем TLB
        crate::arch::x86_64::cpu::invlpg(va);
    }
}

/// Упрощенная версия без параметров кэширования
pub fn mm_map_locked_pages(mdl: *mut MDL, access_mode: u8) -> PVOID {
    mm_map_locked_pages_specify_cache(
        mdl,
        access_mode,
        MEMORY_CACHING_TYPE::MmCached,
        core::ptr::null_mut(),
        true,
        0,
    )
}

/// MmUnmapLockedPages
///
/// Удаляет отображение страниц из системного адресного пространства.
///
/// Освобождает System PTEs, использованные для маппинга.
pub fn mm_unmap_locked_pages(base_address: PVOID, mdl: *mut MDL) {
    if mdl.is_null() || base_address.is_null() {
        return;
    }

    unsafe {
        let mdl_ref = &mut *mdl;

        // Проверяем, отображен ли
        if (mdl_ref.mdl_flags as u32 & MDL_MAPPED_TO_SYSTEM_VA) == 0 {
            return;
        }

        // Для non-paged pool ничего освобождать не нужно
        if (mdl_ref.mdl_flags as u32 & MDL_SOURCE_IS_NONPAGED_POOL) != 0 {
            mdl_ref.mapped_system_va = core::ptr::null_mut();
            mdl_ref.mdl_flags &= !(MDL_MAPPED_TO_SYSTEM_VA as u16);
            return;
        }

        let page_count = mdl_ref.page_count();

        // Вычисляем базовый адрес (без byte_offset)
        let system_va_base = (base_address as usize & !PAGE_MASK) as u64;

        // Размапим каждую страницу
        for i in 0..page_count {
            let va = system_va_base + (i as u64 * PAGE_SIZE as u64);
            mi_unmap_system_pte(va);
        }

        // Освобождаем System PTEs
        if mi_system_ptes_initialized() {
            mi_release_system_ptes(system_va_base, page_count);
        }

        mdl_ref.mapped_system_va = core::ptr::null_mut();
        mdl_ref.mdl_flags &= !(MDL_MAPPED_TO_SYSTEM_VA as u16);
    }
}

// =============================================================================
// MDL Helper Functions
// =============================================================================

/// Возвращает виртуальный адрес для MDL
#[inline]
pub fn mm_get_mdl_virtual_address(mdl: *const MDL) -> PVOID {
    if mdl.is_null() {
        return core::ptr::null_mut();
    }

    unsafe { ((*mdl).start_va as *mut u8).add((*mdl).byte_offset as usize) as PVOID }
}

/// Возвращает размер данных в MDL
#[inline]
pub fn mm_get_mdl_byte_count(mdl: *const MDL) -> u32 {
    if mdl.is_null() {
        return 0;
    }
    unsafe { (*mdl).byte_count }
}

/// Возвращает offset в первой странице
#[inline]
pub fn mm_get_mdl_byte_offset(mdl: *const MDL) -> u32 {
    if mdl.is_null() {
        return 0;
    }
    unsafe { (*mdl).byte_offset }
}

/// Возвращает указатель на PFN array
#[inline]
pub fn mm_get_mdl_pfn_array(mdl: *const MDL) -> *const PFN_NUMBER {
    if mdl.is_null() {
        return core::ptr::null();
    }
    unsafe { (*mdl).pfn_array() }
}
