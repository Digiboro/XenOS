//! Virtual Memory Management
//!
//! Управление виртуальной памятью.
//!
//! Реализует VM syscalls с полной интеграцией VAD и demand paging.
//!
//! # Архитектура
//!
//! - **Reserve**: Создает VAD для диапазона адресов без выделения памяти
//! - **Commit**: Создает demand-zero PTE для страниц (реальные страницы
//!   выделяются при page fault в MmAccessFault)
//! - **Decommit**: Освобождает commit charge и страницы
//! - **Release**: Удаляет VAD и освобождает весь регион
//!
//! Источники:
//! - ReactOS: mm/ARM3/virtual.c, mm/ARM3/pagfault.c
//! - NT6.1: mm/virtual.c

use core::sync::atomic::Ordering;

use super::pagefault::mi_make_demand_zero_pte;
use super::pte::*;
use super::types::*;
use super::vad::*;
use crate::ke::ex_acquire_push_lock_exclusive;
use crate::ke::ex_release_push_lock_exclusive;
use crate::nt::NTSTATUS;
use crate::nt::PVOID;
use crate::nt::STATUS_ACCESS_VIOLATION;
use crate::nt::STATUS_CONFLICTING_ADDRESSES;
use crate::nt::STATUS_INVALID_PARAMETER;
use crate::nt::STATUS_INVALID_PAGE_PROTECTION;
use crate::nt::STATUS_MEMORY_NOT_ALLOCATED;
use crate::nt::STATUS_NO_MEMORY;
use crate::nt::STATUS_SUCCESS;
use crate::nt::STATUS_UNABLE_TO_DELETE_SECTION;
use crate::ps::process::ps_get_current_process;
use crate::ps::process::EPROCESS;

// =============================================================================
// Global Commit Accounting
// =============================================================================

use core::sync::atomic::AtomicUsize;

/// Глобальный счетчик committed страниц
pub static MM_TOTAL_COMMITTED_PAGES: AtomicUsize = AtomicUsize::new(0);

/// Глобальный лимит commit (можно настроить при инициализации)
pub static MM_TOTAL_COMMIT_LIMIT: AtomicUsize = AtomicUsize::new(0);

/// Устанавливает глобальный commit limit
pub fn mm_set_commit_limit(pages: usize) {
    MM_TOTAL_COMMIT_LIMIT.store(pages, Ordering::Release);
}

/// Возвращает текущий committed count
pub fn mm_get_committed_pages() -> usize {
    MM_TOTAL_COMMITTED_PAGES.load(Ordering::Acquire)
}

/// Пытается зарезервировать commit charge
/// Возвращает true если успешно, false если превышен лимит
fn mi_charge_commit(pages: usize) -> bool {
    let limit = MM_TOTAL_COMMIT_LIMIT.load(Ordering::Acquire);

    // Если лимит не установлен (0), всегда разрешаем
    if limit == 0 {
        MM_TOTAL_COMMITTED_PAGES.fetch_add(pages, Ordering::AcqRel);
        return true;
    }

    // Атомарно проверяем и увеличиваем
    loop {
        let current = MM_TOTAL_COMMITTED_PAGES.load(Ordering::Acquire);
        if current + pages > limit {
            return false;
        }
        if MM_TOTAL_COMMITTED_PAGES
            .compare_exchange(current, current + pages, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
        {
            return true;
        }
    }
}

/// Возвращает commit charge
fn mi_return_commit(pages: usize) {
    MM_TOTAL_COMMITTED_PAGES.fetch_sub(pages, Ordering::AcqRel);
}

// =============================================================================
// NtAllocateVirtualMemory
// =============================================================================

/// NtAllocateVirtualMemory
///
/// Резервирует и/или выделяет (commit) виртуальную память в адресном
/// пространстве процесса.
///
/// # Arguments
/// * `process_handle` - Хэндл процесса (NtCurrentProcess = -1)
/// * `base_address` - Указатель на желаемый базовый адрес (0 для автовыбора)
/// * `zero_bits` - Количество старших нулевых бит в адресе
/// * `region_size` - Указатель на размер региона
/// * `allocation_type` - MEM_RESERVE и/или MEM_COMMIT
/// * `protect` - PAGE_* защита
///
/// # Returns
/// * `base_address` - Фактический базовый адрес (выровнен по 64KB)
/// * `region_size` - Фактический размер (выровнен по страницам)
pub fn nt_allocate_virtual_memory(
    process_handle: PVOID,
    base_address: *mut PVOID,
    zero_bits: usize,
    region_size: *mut usize,
    allocation_type: u32,
    protect: u32,
) -> NTSTATUS {
    // Валидация указателей
    if base_address.is_null() || region_size.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    unsafe {
        let requested_base = *base_address as u64;
        let requested_size = *region_size;

        // Размер должен быть > 0
        if requested_size == 0 {
            return STATUS_INVALID_PARAMETER;
        }

        // Проверяем тип выделения
        let do_reserve = (allocation_type & MEM_RESERVE) != 0;
        let do_commit = (allocation_type & MEM_COMMIT) != 0;
        let top_down = (allocation_type & MEM_TOP_DOWN) != 0;

        if !do_reserve && !do_commit {
            return STATUS_INVALID_PARAMETER;
        }

        // Проверяем protection
        if !is_valid_protection(protect) {
            return STATUS_INVALID_PAGE_PROTECTION;
        }

        // Получаем процесс
        let process = mi_get_target_process(process_handle);
        if process.is_null() {
            return STATUS_INVALID_PARAMETER;
        }

        // Получаем VAD root
        let vad_root = (*process).vad_root as *mut MM_AVL_TABLE;
        if vad_root.is_null() {
            // VAD root не инициализирован - нужно создать
            return mi_allocate_virtual_memory_new_vad_root(
                process,
                base_address,
                region_size,
                requested_base,
                requested_size,
                zero_bits,
                allocation_type,
                protect,
                top_down,
            );
        }

        // Захватываем address_creation_lock
        ex_acquire_push_lock_exclusive(&(*process).address_creation_lock);

        let status = mi_allocate_virtual_memory_internal(
            process,
            vad_root,
            base_address,
            region_size,
            requested_base,
            requested_size,
            zero_bits,
            allocation_type,
            protect,
            top_down,
        );

        ex_release_push_lock_exclusive(&(*process).address_creation_lock);

        status
    }
}

/// Получает целевой процесс по handle
unsafe fn mi_get_target_process(process_handle: PVOID) -> *mut EPROCESS {
    unsafe {
        // NtCurrentProcess pseudo-handle (-1 или 0xFFFFFFFF...)
        let handle_value = process_handle as isize;
        if handle_value == -1 {
            return ps_get_current_process();
        }

        // Для других handle: TODO - использовать ObReferenceObjectByHandle
        // Пока поддерживаем только текущий процесс
        if process_handle.is_null() {
            return ps_get_current_process();
        }

        // TODO: ObReferenceObjectByHandle для произвольных процессов
        ps_get_current_process()
    }
}

/// Создает VAD root и выполняет аллокацию (для первого вызова на процесс)
unsafe fn mi_allocate_virtual_memory_new_vad_root(
    process: *mut EPROCESS,
    base_address: *mut PVOID,
    region_size: *mut usize,
    requested_base: u64,
    requested_size: usize,
    zero_bits: usize,
    allocation_type: u32,
    protect: u32,
    top_down: bool,
) -> NTSTATUS {
    unsafe {
        // Выделяем MM_AVL_TABLE для VAD root
        let vad_root = crate::ex::pool::ex_allocate_pool_with_tag(
            crate::ex::pool::POOL_TYPE::NonPagedPool,
            core::mem::size_of::<MM_AVL_TABLE>(),
            u32::from_le_bytes(*b"daVR"), // "RVad"
        ) as *mut MM_AVL_TABLE;

        if vad_root.is_null() {
            return STATUS_NO_MEMORY;
        }

        // Инициализируем
        core::ptr::write(vad_root, MM_AVL_TABLE::new());

        // Устанавливаем в процесс
        (*process).vad_root = vad_root as PVOID;

        // Захватываем lock и выполняем аллокацию
        ex_acquire_push_lock_exclusive(&(*process).address_creation_lock);

        let status = mi_allocate_virtual_memory_internal(
            process,
            vad_root,
            base_address,
            region_size,
            requested_base,
            requested_size,
            zero_bits,
            allocation_type,
            protect,
            top_down,
        );

        ex_release_push_lock_exclusive(&(*process).address_creation_lock);

        status
    }
}

/// Внутренняя реализация NtAllocateVirtualMemory
unsafe fn mi_allocate_virtual_memory_internal(
    process: *mut EPROCESS,
    vad_root: *mut MM_AVL_TABLE,
    base_address: *mut PVOID,
    region_size: *mut usize,
    requested_base: u64,
    requested_size: usize,
    zero_bits: usize,
    allocation_type: u32,
    protect: u32,
    top_down: bool,
) -> NTSTATUS {
    unsafe {
        let do_reserve = (allocation_type & MEM_RESERVE) != 0;
        let do_commit = (allocation_type & MEM_COMMIT) != 0;

        // Вычисляем выровненные адреса
        let (start_va, end_va, start_vpn, end_vpn, page_count) = if requested_base == 0 {
            // Автовыбор адреса
            let aligned_size =
                (requested_size + MM_ALLOCATION_GRANULARITY - 1) & !(MM_ALLOCATION_GRANULARITY - 1);
            let found_va = mi_find_empty_address_range(
                vad_root,
                aligned_size,
                MM_ALLOCATION_GRANULARITY,
                top_down,
            );

            if found_va == 0 {
                return STATUS_NO_MEMORY;
            }

            // Применяем zero_bits ограничение
            if zero_bits > 0 && !mi_check_zero_bits(found_va, zero_bits) {
                return STATUS_NO_MEMORY;
            }

            let start = found_va;
            let end = start + aligned_size as u64 - 1;
            let s_vpn = start >> VPN_SHIFT;
            let e_vpn = end >> VPN_SHIFT;
            let pages = ((aligned_size + PAGE_SIZE - 1) / PAGE_SIZE) as usize;
            (start, end, s_vpn, e_vpn, pages)
        } else {
            // Указан конкретный адрес
            // Выравниваем вниз по allocation granularity
            let aligned_base =
                (requested_base as usize) & !(MM_ALLOCATION_GRANULARITY - 1);
            let offset = (requested_base as usize) - aligned_base;
            let total_size = requested_size + offset;
            let aligned_size = page_align_up(total_size);

            let start = aligned_base as u64;
            let end = start + aligned_size as u64 - 1;
            let s_vpn = start >> VPN_SHIFT;
            let e_vpn = end >> VPN_SHIFT;
            let pages = aligned_size / PAGE_SIZE;
            (start, end, s_vpn, e_vpn, pages)
        };

        // Проверяем границы user space
        if start_va < MM_LOWEST_USER_ADDRESS || end_va > MM_HIGHEST_USER_ADDRESS {
            return STATUS_INVALID_PARAMETER;
        }

        // Ищем существующий VAD
        let existing_vad = mi_find_vad(vad_root, start_va);

        if do_reserve {
            // Резервирование нового региона
            if !existing_vad.is_null() {
                // Уже есть VAD - конфликт
                return STATUS_CONFLICTING_ADDRESSES;
            }

            // Проверяем полный диапазон на конфликты
            if mi_check_vad_conflict(vad_root, start_vpn, end_vpn) {
                return STATUS_CONFLICTING_ADDRESSES;
            }

            // Создаем новый VAD
            let new_vad = mi_allocate_vad_short();
            if new_vad.is_null() {
                return STATUS_NO_MEMORY;
            }

            // Заполняем VAD
            (*new_vad).starting_vpn = start_vpn;
            (*new_vad).ending_vpn = end_vpn;
            (*new_vad).flags.set_vad_type(VAD_TYPE::VadPrivateMemory);
            (*new_vad).flags.set_protection(mi_convert_protection(protect));
            (*new_vad).flags.set_private_memory(true);

            // Если также commit - ставим флаг и заряжаем commit
            if do_commit {
                if !mi_charge_commit(page_count) {
                    mi_free_vad_short(new_vad);
                    return STATUS_NO_MEMORY;
                }
                (*new_vad).flags.set_commit(true);

                // Обновляем per-process commit charge
                (*process)
                    .commit_charge
                    .fetch_add(page_count, Ordering::AcqRel);
            }

            // Вставляем VAD в дерево
            let status = mi_insert_vad(vad_root, new_vad);
            if status != STATUS_SUCCESS {
                if do_commit {
                    mi_return_commit(page_count);
                    (*process)
                        .commit_charge
                        .fetch_sub(page_count, Ordering::AcqRel);
                }
                mi_free_vad_short(new_vad);
                return status;
            }

            // Если commit - создаем demand-zero PTE для каждой страницы
            if do_commit {
                mi_commit_pages(start_va, page_count, protect);
            }
        } else if do_commit {
            // Только commit (без reserve) - должен быть существующий VAD
            if existing_vad.is_null() {
                return STATUS_MEMORY_NOT_ALLOCATED;
            }

            // Проверяем что весь диапазон внутри VAD
            if (*existing_vad).starting_vpn > start_vpn || (*existing_vad).ending_vpn < end_vpn {
                return STATUS_CONFLICTING_ADDRESSES;
            }

            // Проверяем что это private memory
            if (*existing_vad).flags.vad_type() != VAD_TYPE::VadPrivateMemory {
                return STATUS_CONFLICTING_ADDRESSES;
            }

            // Если регион уже committed - это нормально в NT
            // TODO: per-page commit tracking для частичного commit
            if !(*existing_vad).flags.commit() {
                // Заряжаем commit
                if !mi_charge_commit(page_count) {
                    return STATUS_NO_MEMORY;
                }

                (*existing_vad).flags.set_commit(true);
                (*process)
                    .commit_charge
                    .fetch_add(page_count, Ordering::AcqRel);
            }

            // Создаем demand-zero PTE
            mi_commit_pages(start_va, page_count, protect);
        }

        // Возвращаем фактические значения
        *base_address = start_va as PVOID;
        *region_size = page_count * PAGE_SIZE;

        STATUS_SUCCESS
    }
}

/// Проверяет zero_bits ограничение
fn mi_check_zero_bits(address: u64, zero_bits: usize) -> bool {
    if zero_bits == 0 {
        return true;
    }
    // Проверяем что старшие zero_bits битов равны 0
    // Для user space (47-bit addresses на x64)
    let mask = !((1u64 << (48 - zero_bits)) - 1);
    (address & mask) == 0
}

/// Создает demand-zero PTE для диапазона страниц
unsafe fn mi_commit_pages(start_va: u64, page_count: usize, protect: u32) {
    unsafe {
        let mm_protect = mi_convert_protection(protect);

        for i in 0..page_count {
            let va = start_va + (i as u64 * PAGE_SIZE as u64);

            // Убеждаемся что page table hierarchy существует
            if !mi_ensure_page_table_hierarchy(va) {
                continue; // Не удалось создать PTE - пропускаем
            }

            // Устанавливаем demand-zero PTE
            mi_make_demand_zero_pte(va, mm_protect);
        }
    }
}

/// Создает page table hierarchy для адреса если не существует
unsafe fn mi_ensure_page_table_hierarchy(va: u64) -> bool {
    use super::pfn::mi_allocate_pfn;

    unsafe {
        // Проверяем/создаем PML4E
        let pxe_ptr = mm_get_pxe_address(va) as *mut u64;
        let pxe = core::ptr::read_volatile(pxe_ptr);
        if (pxe & PTE_VALID) == 0 {
            // Нужно создать PDPT
            if let Some(pfn) = mi_allocate_pfn() {
                let new_pxe = ((pfn as u64) << 12) | PTE_VALID | PTE_READWRITE | PTE_USER;
                core::ptr::write_volatile(pxe_ptr, new_pxe);
            } else {
                return false;
            }
        }

        // Проверяем/создаем PDPTE
        let ppe_ptr = mm_get_ppe_address(va) as *mut u64;
        let ppe = core::ptr::read_volatile(ppe_ptr);
        if (ppe & PTE_VALID) == 0 {
            if let Some(pfn) = mi_allocate_pfn() {
                let new_ppe = ((pfn as u64) << 12) | PTE_VALID | PTE_READWRITE | PTE_USER;
                core::ptr::write_volatile(ppe_ptr, new_ppe);
            } else {
                return false;
            }
        }

        // Проверяем/создаем PDE
        let pde_ptr = mm_get_pde_address(va) as *mut u64;
        let pde = core::ptr::read_volatile(pde_ptr);
        if (pde & PTE_VALID) == 0 {
            if let Some(pfn) = mi_allocate_pfn() {
                let new_pde = ((pfn as u64) << 12) | PTE_VALID | PTE_READWRITE | PTE_USER;
                core::ptr::write_volatile(pde_ptr, new_pde);
            } else {
                return false;
            }
        }

        true
    }
}

// =============================================================================
// NtFreeVirtualMemory
// =============================================================================

/// NtFreeVirtualMemory
///
/// Освобождает (decommit) или удаляет (release) виртуальную память.
///
/// # Arguments
/// * `process_handle` - Хэндл процесса
/// * `base_address` - Базовый адрес региона
/// * `region_size` - Размер региона (0 для MEM_RELEASE)
/// * `free_type` - MEM_DECOMMIT или MEM_RELEASE
///
/// # Семантика
///
/// - **MEM_DECOMMIT**: Освобождает committed страницы, но сохраняет VAD
///   (регион остается reserved). Возвращает commit charge.
/// - **MEM_RELEASE**: Полностью удаляет регион (VAD). Размер должен быть 0,
///   адрес должен быть началом региона.
pub fn nt_free_virtual_memory(
    process_handle: PVOID,
    base_address: *mut PVOID,
    region_size: *mut usize,
    free_type: u32,
) -> NTSTATUS {
    if base_address.is_null() || region_size.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    unsafe {
        let requested_base = *base_address as u64;
        let requested_size = *region_size;

        // Проверяем тип освобождения
        let do_decommit = (free_type & MEM_DECOMMIT) != 0;
        let do_release = (free_type & MEM_RELEASE) != 0;

        // Нельзя одновременно decommit и release
        if do_decommit && do_release {
            return STATUS_INVALID_PARAMETER;
        }

        if !do_decommit && !do_release {
            return STATUS_INVALID_PARAMETER;
        }

        // Для MEM_RELEASE размер должен быть 0
        if do_release && requested_size != 0 {
            return STATUS_INVALID_PARAMETER;
        }

        // Получаем процесс
        let process = mi_get_target_process(process_handle);
        if process.is_null() {
            return STATUS_INVALID_PARAMETER;
        }

        // Получаем VAD root
        let vad_root = (*process).vad_root as *mut MM_AVL_TABLE;
        if vad_root.is_null() {
            return STATUS_MEMORY_NOT_ALLOCATED;
        }

        // Захватываем lock
        ex_acquire_push_lock_exclusive(&(*process).address_creation_lock);

        let status = if do_release {
            mi_free_virtual_memory_release(
                process,
                vad_root,
                base_address,
                region_size,
                requested_base,
            )
        } else {
            mi_free_virtual_memory_decommit(
                process,
                vad_root,
                base_address,
                region_size,
                requested_base,
                requested_size,
            )
        };

        ex_release_push_lock_exclusive(&(*process).address_creation_lock);

        status
    }
}

/// MEM_RELEASE: Полное освобождение региона
unsafe fn mi_free_virtual_memory_release(
    process: *mut EPROCESS,
    vad_root: *mut MM_AVL_TABLE,
    base_address: *mut PVOID,
    region_size: *mut usize,
    requested_base: u64,
) -> NTSTATUS {
    use super::pagefault::mi_decommit_page;

    unsafe {
        // Находим VAD
        let vad = mi_find_vad(vad_root, requested_base);
        if vad.is_null() {
            return STATUS_MEMORY_NOT_ALLOCATED;
        }

        // Проверяем что адрес - начало региона
        let vad_start = (*vad).start_address();
        if requested_base != vad_start {
            return STATUS_INVALID_PARAMETER;
        }

        // Проверяем что это private memory (не section mapping)
        if (*vad).flags.vad_type() != VAD_TYPE::VadPrivateMemory {
            return STATUS_UNABLE_TO_DELETE_SECTION;
        }

        let vad_end = (*vad).end_address();
        let page_count = (*vad).size() / PAGE_SIZE;

        // Освобождаем committed страницы
        if (*vad).flags.commit() {
            // Decommit каждую страницу
            for i in 0..page_count {
                let va = vad_start + (i as u64 * PAGE_SIZE as u64);
                mi_decommit_page(va);
            }

            // Возвращаем commit charge
            mi_return_commit(page_count);
            (*process)
                .commit_charge
                .fetch_sub(page_count, Ordering::AcqRel);
        }

        // Удаляем VAD из дерева
        mi_remove_vad(vad_root, vad);

        // Освобождаем память VAD
        mi_free_vad_short(vad);

        // Возвращаем фактические значения
        *base_address = vad_start as PVOID;
        *region_size = (vad_end - vad_start) as usize;

        STATUS_SUCCESS
    }
}

/// MEM_DECOMMIT: Освобождение committed страниц без удаления VAD
unsafe fn mi_free_virtual_memory_decommit(
    process: *mut EPROCESS,
    vad_root: *mut MM_AVL_TABLE,
    base_address: *mut PVOID,
    region_size: *mut usize,
    requested_base: u64,
    requested_size: usize,
) -> NTSTATUS {
    use super::pagefault::mi_decommit_page;

    unsafe {
        if requested_size == 0 {
            return STATUS_INVALID_PARAMETER;
        }

        // Выравниваем адрес и размер
        let start_va = page_align(requested_base as usize) as u64;
        let end_va = page_align_up(requested_base as usize + requested_size) as u64;
        let start_vpn = start_va >> VPN_SHIFT;
        let end_vpn = (end_va - 1) >> VPN_SHIFT;

        // Находим VAD
        let vad = mi_find_vad(vad_root, start_va);
        if vad.is_null() {
            return STATUS_MEMORY_NOT_ALLOCATED;
        }

        // Проверяем что весь диапазон внутри VAD
        if (*vad).starting_vpn > start_vpn || (*vad).ending_vpn < end_vpn {
            return STATUS_INVALID_PARAMETER;
        }

        // Проверяем что это private memory
        if (*vad).flags.vad_type() != VAD_TYPE::VadPrivateMemory {
            return STATUS_INVALID_PARAMETER;
        }

        // Если не committed - ничего делать не нужно
        if !(*vad).flags.commit() {
            *base_address = start_va as PVOID;
            *region_size = (end_va - start_va) as usize;
            return STATUS_SUCCESS;
        }

        let page_count = ((end_va - start_va) / PAGE_SIZE as u64) as usize;

        // Определяем, нужно ли разделять VAD
        let vad_covers_entire_range =
            (*vad).starting_vpn == start_vpn && (*vad).ending_vpn == end_vpn;

        if vad_covers_entire_range {
            // Decommit весь VAD
            for i in 0..page_count {
                let va = start_va + (i as u64 * PAGE_SIZE as u64);
                mi_decommit_page(va);
            }

            // Снимаем commit флаг
            (*vad).flags.set_commit(false);

            // Возвращаем commit charge
            mi_return_commit(page_count);
            (*process)
                .commit_charge
                .fetch_sub(page_count, Ordering::AcqRel);
        } else {
            // Частичный decommit - нужно разделить VAD
            // Для упрощения пока decommit страницы без разделения VAD
            // TODO: полное разделение VAD и per-page commit tracking

            for i in 0..page_count {
                let va = start_va + (i as u64 * PAGE_SIZE as u64);
                mi_decommit_page(va);
            }

            // Возвращаем commit charge за decommitted страницы
            mi_return_commit(page_count);
            (*process)
                .commit_charge
                .fetch_sub(page_count, Ordering::AcqRel);

            // Примечание: VAD остается committed, но отдельные страницы decommitted
            // Это упрощение - в полной NT реализации используется bitmap
        }

        *base_address = start_va as PVOID;
        *region_size = (end_va - start_va) as usize;

        STATUS_SUCCESS
    }
}

// =============================================================================
// NtProtectVirtualMemory
// =============================================================================

/// NtProtectVirtualMemory
///
/// Изменяет защиту виртуальной памяти.
///
/// # Arguments
/// * `process_handle` - Хэндл процесса
/// * `base_address` - Базовый адрес региона
/// * `region_size` - Размер региона
/// * `new_protect` - Новая защита (PAGE_*)
/// * `old_protect` - Возвращает предыдущую защиту
///
/// # Семантика
///
/// - Изменяет protection в VAD (для reserved/committed страниц)
/// - Для present страниц обновляет hardware PTE и инвалидирует TLB
/// - Для invalid страниц обновляет software protection encoding
/// - Может потребовать разделения VAD если protection различается
pub fn nt_protect_virtual_memory(
    process_handle: PVOID,
    base_address: *mut PVOID,
    region_size: *mut usize,
    new_protect: u32,
    old_protect: *mut u32,
) -> NTSTATUS {
    if base_address.is_null() || region_size.is_null() || old_protect.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    unsafe {
        let requested_base = *base_address as u64;
        let requested_size = *region_size;

        if requested_size == 0 {
            return STATUS_INVALID_PARAMETER;
        }

        // Проверяем protection
        if !is_valid_protection(new_protect) {
            return STATUS_INVALID_PAGE_PROTECTION;
        }

        // Получаем процесс
        let process = mi_get_target_process(process_handle);
        if process.is_null() {
            return STATUS_INVALID_PARAMETER;
        }

        // Получаем VAD root
        let vad_root = (*process).vad_root as *mut MM_AVL_TABLE;
        if vad_root.is_null() {
            return STATUS_MEMORY_NOT_ALLOCATED;
        }

        // Захватываем lock
        ex_acquire_push_lock_exclusive(&(*process).address_creation_lock);

        let status = mi_protect_virtual_memory_internal(
            vad_root,
            base_address,
            region_size,
            requested_base,
            requested_size,
            new_protect,
            old_protect,
        );

        ex_release_push_lock_exclusive(&(*process).address_creation_lock);

        status
    }
}

/// Внутренняя реализация NtProtectVirtualMemory
unsafe fn mi_protect_virtual_memory_internal(
    vad_root: *mut MM_AVL_TABLE,
    base_address: *mut PVOID,
    region_size: *mut usize,
    requested_base: u64,
    requested_size: usize,
    new_protect: u32,
    old_protect: *mut u32,
) -> NTSTATUS {
    use crate::arch::x86_64::cpu::invlpg;

    unsafe {
        // Выравниваем адрес и размер по страницам
        let start_va = page_align(requested_base as usize) as u64;
        let end_va = page_align_up(requested_base as usize + requested_size) as u64;
        let start_vpn = start_va >> VPN_SHIFT;
        let end_vpn = (end_va - 1) >> VPN_SHIFT;

        // Находим VAD
        let vad = mi_find_vad(vad_root, start_va);
        if vad.is_null() {
            return STATUS_MEMORY_NOT_ALLOCATED;
        }

        // Проверяем что весь диапазон внутри VAD
        if (*vad).starting_vpn > start_vpn || (*vad).ending_vpn < end_vpn {
            return STATUS_INVALID_PARAMETER;
        }

        // Сохраняем старую protection
        let old_mm_protect = (*vad).flags.protection();
        *old_protect = mi_protection_to_page_constant(old_mm_protect);

        // Конвертируем новую protection
        let new_mm_protect = mi_convert_protection(new_protect);

        // Определяем, нужно ли разделять VAD
        let vad_start_vpn = (*vad).starting_vpn;
        let vad_end_vpn = (*vad).ending_vpn;
        let covers_entire_vad = start_vpn == vad_start_vpn && end_vpn == vad_end_vpn;

        if covers_entire_vad {
            // Простой случай - меняем protection всего VAD
            (*vad).flags.set_protection(new_mm_protect);
        } else {
            // Нужно разделить VAD
            // Случай 1: Начало VAD совпадает - отрезаем конец
            // Случай 2: Конец VAD совпадает - отрезаем начало
            // Случай 3: В середине - разделяем на три части

            if start_vpn == vad_start_vpn {
                // Отрезаем начало: [start..end] | [end+1..vad_end]
                let new_vad = mi_split_vad(vad_root, vad, end_vpn + 1);
                if new_vad.is_null() {
                    return STATUS_NO_MEMORY;
                }
                (*vad).flags.set_protection(new_mm_protect);
            } else if end_vpn == vad_end_vpn {
                // Отрезаем конец: [vad_start..start-1] | [start..end]
                let new_vad = mi_split_vad(vad_root, vad, start_vpn);
                if new_vad.is_null() {
                    return STATUS_NO_MEMORY;
                }
                (*new_vad).flags.set_protection(new_mm_protect);
            } else {
                // В середине: [vad_start..start-1] | [start..end] | [end+1..vad_end]
                // Первое разделение
                let middle_vad = mi_split_vad(vad_root, vad, start_vpn);
                if middle_vad.is_null() {
                    return STATUS_NO_MEMORY;
                }
                // Второе разделение
                let end_vad = mi_split_vad(vad_root, middle_vad, end_vpn + 1);
                if end_vad.is_null() {
                    // Откат сложен, оставляем как есть
                    return STATUS_NO_MEMORY;
                }
                (*middle_vad).flags.set_protection(new_mm_protect);
            }
        }

        // Обновляем PTE для всех страниц в диапазоне
        let page_count = ((end_va - start_va) / PAGE_SIZE as u64) as usize;
        let pte_flags = mi_protection_to_pte_mask(new_mm_protect);

        for i in 0..page_count {
            let va = start_va + (i as u64 * PAGE_SIZE as u64);
            let pte_ptr = mm_get_pte_address(va) as *mut u64;
            let pte_value = core::ptr::read_volatile(pte_ptr);

            if (pte_value & PTE_VALID) != 0 {
                // Страница present - обновляем hardware bits
                let pfn = (pte_value >> 12) & 0xFFFFFFFFFF;
                let new_pte = (pfn << 12) | pte_flags | PTE_VALID;
                core::ptr::write_volatile(pte_ptr, new_pte);

                // Инвалидируем TLB
                invlpg(va);
            } else if pte_value != 0 {
                // Страница not present но не нулевая (demand-zero/transition/etc)
                // Обновляем software protection bits (биты 5-9)
                let new_pte = (pte_value & !0x3E0) | ((new_mm_protect as u64 & 0x1F) << 5);
                core::ptr::write_volatile(pte_ptr, new_pte);
            }
            // Нулевые PTE (decommitted) не трогаем
        }

        // Возвращаем фактические значения
        *base_address = start_va as PVOID;
        *region_size = (end_va - start_va) as usize;

        STATUS_SUCCESS
    }
}

/// Конвертирует MM protection в PAGE_* константу
fn mi_protection_to_page_constant(mm_protect: u32) -> u32 {
    let base = mm_protect & MM_PROTECT_ACCESS;
    let modifiers = mm_protect & !(MM_PROTECT_ACCESS);

    let page_base = match base {
        MM_ZERO_ACCESS => PAGE_NOACCESS,
        MM_READONLY => PAGE_READONLY,
        MM_EXECUTE => PAGE_EXECUTE,
        MM_EXECUTE_READ => PAGE_EXECUTE_READ,
        MM_READWRITE => PAGE_READWRITE,
        MM_WRITECOPY => PAGE_WRITECOPY,
        MM_EXECUTE_READWRITE => PAGE_EXECUTE_READWRITE,
        MM_EXECUTE_WRITECOPY => PAGE_EXECUTE_WRITECOPY,
        _ => PAGE_NOACCESS,
    };

    let mut result = page_base;

    if (modifiers & MM_GUARDPAGE) != 0 {
        result |= PAGE_GUARD;
    }
    if (modifiers & MM_NOCACHE) != 0 {
        result |= PAGE_NOCACHE;
    }
    if (modifiers & MM_WRITECOMBINE) != 0 {
        result |= PAGE_WRITECOMBINE;
    }

    result
}

// =============================================================================
// NtQueryVirtualMemory
// =============================================================================

/// MemoryInformationClass для NtQueryVirtualMemory
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MEMORY_INFORMATION_CLASS {
    MemoryBasicInformation = 0,
    MemoryWorkingSetInformation = 1,
    MemoryMappedFilenameInformation = 2,
    MemoryRegionInformation = 3,
    MemoryWorkingSetExInformation = 4,
    // ... другие классы
}

/// NtQueryVirtualMemory
///
/// Запрашивает информацию о виртуальной памяти.
///
/// # Arguments
/// * `process_handle` - Хэндл процесса
/// * `base_address` - Адрес для запроса
/// * `memory_info` - Буфер для результата
/// * `memory_info_length` - Размер буфера
/// * `return_length` - Возвращает реальный размер данных
///
/// # Семантика
///
/// Возвращает MEMORY_BASIC_INFORMATION с агрегацией contiguous страниц
/// с одинаковыми state/protection/type.
pub fn nt_query_virtual_memory(
    process_handle: PVOID,
    base_address: PVOID,
    memory_info: *mut MEMORY_BASIC_INFORMATION,
    memory_info_length: usize,
    return_length: *mut usize,
) -> NTSTATUS {
    if memory_info.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    if memory_info_length < core::mem::size_of::<MEMORY_BASIC_INFORMATION>() {
        return STATUS_INVALID_PARAMETER;
    }

    unsafe {
        let query_address = base_address as u64;

        // Получаем процесс
        let process = mi_get_target_process(process_handle);
        if process.is_null() {
            return STATUS_INVALID_PARAMETER;
        }

        // Получаем VAD root
        let vad_root = (*process).vad_root as *mut MM_AVL_TABLE;

        // Захватываем lock (shared достаточно для query, но используем exclusive для простоты)
        ex_acquire_push_lock_exclusive(&(*process).address_creation_lock);

        let status = mi_query_virtual_memory_internal(
            vad_root,
            query_address,
            memory_info,
            return_length,
        );

        ex_release_push_lock_exclusive(&(*process).address_creation_lock);

        status
    }
}

/// Внутренняя реализация NtQueryVirtualMemory
unsafe fn mi_query_virtual_memory_internal(
    vad_root: *mut MM_AVL_TABLE,
    query_address: u64,
    memory_info: *mut MEMORY_BASIC_INFORMATION,
    return_length: *mut usize,
) -> NTSTATUS {
    unsafe {
        let info = &mut *memory_info;

        // Выравниваем адрес по странице
        let page_address = page_align(query_address as usize) as u64;

        // Проверяем границы
        if query_address < MM_LOWEST_USER_ADDRESS {
            // Ниже user space
            info.base_address = query_address as PVOID;
            info.allocation_base = core::ptr::null_mut();
            info.allocation_protect = 0;
            info.region_size = (MM_LOWEST_USER_ADDRESS - query_address) as usize;
            info.state = MEM_FREE;
            info.protect = PAGE_NOACCESS;
            info.memory_type = 0;

            if !return_length.is_null() {
                *return_length = core::mem::size_of::<MEMORY_BASIC_INFORMATION>();
            }
            return STATUS_SUCCESS;
        }

        if query_address > MM_HIGHEST_USER_ADDRESS {
            // Выше user space (kernel space)
            info.base_address = query_address as PVOID;
            info.allocation_base = core::ptr::null_mut();
            info.allocation_protect = 0;
            info.region_size = 0;
            info.state = MEM_FREE;
            info.protect = PAGE_NOACCESS;
            info.memory_type = 0;

            if !return_length.is_null() {
                *return_length = core::mem::size_of::<MEMORY_BASIC_INFORMATION>();
            }
            return STATUS_SUCCESS;
        }

        // Ищем VAD
        let vad = if vad_root.is_null() {
            core::ptr::null_mut()
        } else {
            mi_find_vad(vad_root, page_address)
        };

        if vad.is_null() {
            // Адрес не в VAD - это FREE память
            // Нужно найти следующий VAD чтобы определить размер свободного региона
            let next_vad_start = if vad_root.is_null() {
                MM_HIGHEST_USER_ADDRESS + 1
            } else {
                mi_find_next_vad_start(vad_root, page_address)
            };

            info.base_address = page_address as PVOID;
            info.allocation_base = core::ptr::null_mut();
            info.allocation_protect = 0;
            info.region_size = (next_vad_start - page_address) as usize;
            info.state = MEM_FREE;
            info.protect = PAGE_NOACCESS;
            info.memory_type = 0;
        } else {
            // Адрес внутри VAD
            let vad_start = (*vad).start_address();
            let vad_end = (*vad).end_address();
            let vad_type = (*vad).flags.vad_type();
            let protection = (*vad).flags.protection();
            let is_committed = (*vad).flags.commit();

            info.allocation_base = vad_start as PVOID;
            info.allocation_protect = mi_protection_to_page_constant(protection);

            // Определяем memory_type
            info.memory_type = match vad_type {
                VAD_TYPE::VadPrivateMemory => MEM_PRIVATE,
                VAD_TYPE::VadMapped => MEM_MAPPED,
                VAD_TYPE::VadImage => MEM_MAPPED, // Image тоже mapped в терминах MEMORY_BASIC_INFO
                _ => MEM_PRIVATE,
            };

            // Определяем state на основе PTE
            // TODO: Для точности нужно смотреть PTE каждой страницы
            // Сейчас используем VAD commit flag
            if is_committed {
                // Проверяем PTE для определения реального состояния
                let pte_ptr = mm_get_pte_address(page_address) as *const u64;
                let pte_value = core::ptr::read_volatile(pte_ptr);

                if (pte_value & PTE_VALID) != 0 {
                    // Страница в памяти
                    info.state = MEM_COMMIT;
                    // Получаем реальную protection из PTE
                    info.protect = mi_pte_to_page_protection(pte_value);
                } else if pte_value != 0 {
                    // Demand-zero или другое software состояние
                    info.state = MEM_COMMIT;
                    info.protect = mi_protection_to_page_constant(protection);
                } else {
                    // PTE нулевой - decommitted в рамках committed VAD
                    info.state = MEM_RESERVE;
                    info.protect = 0;
                }
            } else {
                // Reserved но не committed
                info.state = MEM_RESERVE;
                info.protect = 0;
            }

            // Вычисляем размер региона с одинаковыми атрибутами
            // Для простоты возвращаем размер от текущей страницы до конца VAD
            // TODO: более точная агрегация по состоянию каждой страницы
            info.base_address = page_address as PVOID;
            info.region_size = (vad_end - page_address) as usize;
        }

        if !return_length.is_null() {
            *return_length = core::mem::size_of::<MEMORY_BASIC_INFORMATION>();
        }

        STATUS_SUCCESS
    }
}

/// Находит начальный адрес следующего VAD после указанного адреса
unsafe fn mi_find_next_vad_start(vad_root: *mut MM_AVL_TABLE, address: u64) -> u64 {
    unsafe {
        if vad_root.is_null() || (*vad_root).is_empty() {
            return MM_HIGHEST_USER_ADDRESS + 1;
        }

        let vpn = address >> VPN_SHIFT;
        let mut best_start = MM_HIGHEST_USER_ADDRESS + 1;

        // In-order traversal для поиска минимального VAD с starting_vpn > vpn
        let mut stack: [*mut MMVAD_SHORT; 64] = [core::ptr::null_mut(); 64];
        let mut stack_idx = 0;
        let mut node = (*vad_root).root;

        loop {
            while !node.is_null() {
                if stack_idx >= 64 {
                    break;
                }
                stack[stack_idx] = node;
                stack_idx += 1;
                node = (*node).left_child;
            }

            if stack_idx == 0 {
                break;
            }

            stack_idx -= 1;
            node = stack[stack_idx];

            if (*node).starting_vpn > vpn {
                let start = (*node).start_address();
                if start < best_start {
                    best_start = start;
                }
                // Нашли первый VAD после нашего адреса, можно прекратить
                break;
            }

            node = (*node).right_child;
        }

        best_start
    }
}

/// Конвертирует PTE флаги в PAGE_* protection
fn mi_pte_to_page_protection(pte: u64) -> u32 {
    let mut protect = 0u32;

    if (pte & PTE_VALID) == 0 {
        return PAGE_NOACCESS;
    }

    let writable = (pte & PTE_READWRITE) != 0;
    let executable = (pte & PTE_NX) == 0;

    protect = match (writable, executable) {
        (false, false) => PAGE_READONLY,
        (true, false) => PAGE_READWRITE,
        (false, true) => PAGE_EXECUTE_READ,
        (true, true) => PAGE_EXECUTE_READWRITE,
    };

    protect
}

// =============================================================================
// Page Fault Handler
// =============================================================================

/// Информация о page fault
#[derive(Clone, Copy, Debug)]
pub struct PageFaultInfo {
    /// Адрес, вызвавший fault
    pub fault_address: u64,
    /// Код ошибки
    pub error_code: u64,
    /// Адрес инструкции
    pub instruction_address: u64,
}

/// Обрабатывает page fault
///
/// # Returns
/// STATUS_SUCCESS если fault обработан, иначе код ошибки
pub fn mm_access_fault(fault_info: &PageFaultInfo) -> NTSTATUS {
    let fault_addr = fault_info.fault_address;
    let error_code = fault_info.error_code;

    // Разбираем error code (x86_64):
    // Bit 0: 0 = not present, 1 = protection violation
    // Bit 1: 0 = read, 1 = write
    // Bit 2: 0 = kernel, 1 = user
    // Bit 3: reserved bit violation
    // Bit 4: instruction fetch

    let is_present = (error_code & 1) != 0;
    let _is_write = (error_code & 2) != 0;
    let _is_user = (error_code & 4) != 0;
    let is_reserved = (error_code & 8) != 0;
    let _is_fetch = (error_code & 16) != 0;

    // Reserved bit violation - это ошибка в page table
    if is_reserved {
        return STATUS_ACCESS_VIOLATION;
    }

    // Protection violation - права доступа нарушены
    if is_present {
        // Страница в памяти, но защита не позволяет доступ
        // TODO: Проверить copy-on-write, guard pages и т.д.
        return STATUS_ACCESS_VIOLATION;
    }

    // Page not present - нужно «поднять» страницу
    // TODO(MVP): demand-zero по committed VAD
    // 1. Проверить, есть ли VAD для этого адреса
    // 2. Если есть и committed: выделить PFN, обнулить/взять из zeroed, выставить valid PTE
    // 3. Если нет: access violation
    //
    // TODO: transition/prototype/pagefile/COW/guard/stack growth

    // Проверяем, в kernel space ли адрес
    if fault_addr >= MM_SYSTEM_RANGE_START {
        // Kernel space fault - серьезная проблема
        return STATUS_ACCESS_VIOLATION;
    }

    // User space fault без VAD
    STATUS_ACCESS_VIOLATION
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Проверяет валидность protection флагов
fn is_valid_protection(protect: u32) -> bool {
    // Базовые protection флаги (только один должен быть установлен)
    let base_protect = protect & 0xFF;

    match base_protect {
        PAGE_NOACCESS
        | PAGE_READONLY
        | PAGE_READWRITE
        | PAGE_WRITECOPY
        | PAGE_EXECUTE
        | PAGE_EXECUTE_READ
        | PAGE_EXECUTE_READWRITE
        | PAGE_EXECUTE_WRITECOPY => true,
        _ => false,
    }
}

/// Конвертирует Windows protection в internal MM protection
pub fn mi_convert_protection(protect: u32) -> u32 {
    let base = protect & 0xFF;
    let modifiers = protect & 0xFF00;

    let mm_protect = match base {
        PAGE_NOACCESS => MM_NOACCESS,
        PAGE_READONLY => MM_READONLY,
        PAGE_READWRITE => MM_READWRITE,
        PAGE_WRITECOPY => MM_WRITECOPY,
        PAGE_EXECUTE => MM_EXECUTE,
        PAGE_EXECUTE_READ => MM_EXECUTE_READ,
        PAGE_EXECUTE_READWRITE => MM_EXECUTE_READWRITE,
        PAGE_EXECUTE_WRITECOPY => MM_EXECUTE_WRITECOPY,
        _ => MM_NOACCESS,
    };

    let mut result = mm_protect;

    if (modifiers & PAGE_GUARD) != 0 {
        result |= MM_GUARDPAGE;
    }
    if (modifiers & PAGE_NOCACHE) != 0 {
        result |= MM_NOCACHE;
    }
    if (modifiers & PAGE_WRITECOMBINE) != 0 {
        result |= MM_WRITECOMBINE;
    }

    result
}

/// Конвертирует MM protection в PTE флаги
pub fn mi_protection_to_pte_mask(mm_protect: u32) -> u64 {
    let base = mm_protect & MM_PROTECT_ACCESS;

    let mut pte_flags = PTE_VALID;

    match base {
        MM_READONLY | MM_EXECUTE | MM_EXECUTE_READ => {
            // Read-only
        },
        MM_READWRITE | MM_WRITECOPY | MM_EXECUTE_READWRITE | MM_EXECUTE_WRITECOPY => {
            pte_flags |= PTE_READWRITE;
        },
        _ => {
            // No access
            return 0;
        },
    }

    // No-execute для non-execute pages
    if base == MM_READONLY || base == MM_READWRITE || base == MM_WRITECOPY {
        pte_flags |= PTE_NX;
    }

    // Cache flags
    if (mm_protect & MM_NOCACHE) != 0 {
        pte_flags |= PTE_DISABLE_CACHE;
    }

    pte_flags
}
