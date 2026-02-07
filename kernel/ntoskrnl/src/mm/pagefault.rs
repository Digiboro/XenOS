//! Page Fault Handler (MmAccessFault)
//!
//! Обработчик page fault — центральная точка demand paging.
//!
//! # Обрабатываемые типы fault
//!
//! - **Demand-zero**: первый доступ к committed private page
//! - **Transition**: страница в standby/modified list
//! - **Guard page**: STATUS_GUARD_PAGE_VIOLATION
//! - **Copy-on-write**: запись в COW страницу
//! - **Prototype PTE**: mapped sections
//! - **Stack growth**: автоматическое расширение стека при guard page
//!
//! # Error Code (x86_64)
//!
//! ```text
//! Bit 0: P    - 0=not-present, 1=protection violation
//! Bit 1: W/R  - 0=read, 1=write
//! Bit 2: U/S  - 0=kernel, 1=user mode
//! Bit 3: RSVD - reserved bit violation
//! Bit 4: I/D  - 0=data, 1=instruction fetch
//! ```
//!
//! # VAD Gate
//!
//! Для not-present faults обязательна проверка через VAD:
//! - Адрес должен принадлежать валидному VAD
//! - Страница должна быть committed (или допускается stack growth)
//! - При нарушении: STATUS_ACCESS_VIOLATION
//!
//! Источники:
//! - ReactOS: mm/ARM3/pagfault.c
//! - NT6.1: mm/pagfault.c

use super::hyperspace::mi_copy_physical_page;
use super::hyperspace::mi_zero_physical_page;
use super::pfn::*;
use super::pte::*;
use super::types::*;
use super::vad::mi_find_vad;
use super::vad::MM_AVL_TABLE;
use super::vad::MMVAD_SHORT;
use super::vad::VAD_TYPE;
use super::vad::VPN_SHIFT;
use crate::arch::x86_64::cpu;
use crate::ke::bugcheck::bugcheck_codes;
use crate::ke::bugcheck::ke_bug_check_ex;
use crate::nt::NTSTATUS;
use crate::nt::STATUS_ACCESS_VIOLATION;
use crate::nt::STATUS_GUARD_PAGE_VIOLATION;
use crate::nt::STATUS_IN_PAGE_ERROR;
use crate::nt::STATUS_SUCCESS;
use crate::ps::process::ps_get_current_process;
use crate::ps::process::EPROCESS;

// =============================================================================
// Error Code Constants
// =============================================================================

/// Error code bit: Page was present (protection violation vs not-present)
pub const PF_PRESENT: u64 = 1 << 0;
/// Error code bit: Write access
pub const PF_WRITE: u64 = 1 << 1;
/// Error code bit: User mode access
pub const PF_USER: u64 = 1 << 2;
/// Error code bit: Reserved bit violation
pub const PF_RESERVED: u64 = 1 << 3;
/// Error code bit: Instruction fetch
pub const PF_INSTRUCTION: u64 = 1 << 4;
/// Error code bit: Protection key violation (PKU)
pub const PF_PROTECTION_KEY: u64 = 1 << 5;
/// Error code bit: Shadow stack access
pub const PF_SHADOW_STACK: u64 = 1 << 6;

// =============================================================================
// PTE States (Software)
// =============================================================================

/// PTE software states (в невалидных PTE)
///
/// Когда PTE.Present=0, биты 1-11 используются для software state.
pub mod pte_state {
    /// PTE полностью нулевой — decommitted/free
    pub const ZERO: u64 = 0;

    /// Demand-zero: страница committed, но не выделена
    /// Формат: [0][prototype=0][transition=0][demand_zero=1][protection:5][...]
    pub const DEMAND_ZERO: u64 = 1 << 1;

    /// Transition: страница в standby/modified list
    /// Формат: [PFN:40][transition=1][protection:5][valid=0]
    pub const TRANSITION: u64 = 1 << 2;

    /// Prototype: PTE указывает на prototype PTE
    pub const PROTOTYPE: u64 = 1 << 3;

    /// Guard page marker
    pub const GUARD: u64 = 1 << 4;
}

// =============================================================================
// MmAccessFault
// =============================================================================

/// MmAccessFault
///
/// Главный обработчик page fault. Вызывается из trap handler.
///
/// # Arguments
/// * `fault_address` - адрес, вызвавший fault (CR2)
/// * `error_code` - x86_64 error code
/// * `trap_frame_rip` - RIP из trap frame (для диагностики)
///
/// # Returns
/// - `STATUS_SUCCESS` — fault обработан, можно retry инструкцию
/// - `STATUS_ACCESS_VIOLATION` — невалидный доступ
/// - `STATUS_GUARD_PAGE_VIOLATION` — guard page hit
/// - `STATUS_IN_PAGE_ERROR` — ошибка при подъёме страницы
///
/// # Safety
/// Вызывается из trap handler с поднятым IRQL.
#[unsafe(no_mangle)]
pub extern "win64" fn MmAccessFault(
    fault_address: u64,
    error_code: u64,
    trap_frame_rip: u64,
) -> NTSTATUS {
    // Проверяем reserved bit violation — это hardware error
    if (error_code & PF_RESERVED) != 0 {
        // Bugcheck: страничные таблицы повреждены
        return mi_fatal_page_fault(fault_address, error_code, trap_frame_rip);
    }

    let is_present = (error_code & PF_PRESENT) != 0;
    let is_write = (error_code & PF_WRITE) != 0;
    let is_user = (error_code & PF_USER) != 0;
    let _is_fetch = (error_code & PF_INSTRUCTION) != 0;

    // Определяем тип адресного пространства
    let is_kernel_address = fault_address >= MM_SYSTEM_RANGE_START;

    // User mode не может обращаться к kernel space
    if is_user && is_kernel_address {
        return STATUS_ACCESS_VIOLATION;
    }

    // Получаем PTE для fault address
    let pte_ptr = mm_get_pte_address(fault_address) as *mut u64;

    // Проверяем валидность PTE pointer (может быть не замаплен)
    // Для этого проверяем соответствующие уровни page table
    if !mi_is_pte_pointer_valid(fault_address) {
        // Page table hierarchy не создана для этого адреса
        // Это либо не-committed память, либо kernel address без маппинга
        if is_kernel_address {
            return mi_fatal_page_fault(fault_address, error_code, trap_frame_rip);
        }
        return STATUS_ACCESS_VIOLATION;
    }

    // Читаем PTE
    let pte_value = unsafe { core::ptr::read_volatile(pte_ptr) };

    if is_present {
        // Protection violation на present page
        return mi_handle_protection_fault(fault_address, pte_value, is_write, is_user);
    }

    // Page not present — нужно resolve
    mi_resolve_not_present_fault(fault_address, pte_ptr as u64, pte_value, is_write, is_user)
}

// =============================================================================
// Not-Present Fault Resolution
// =============================================================================

/// Обрабатывает not-present fault с полной VAD проверкой
fn mi_resolve_not_present_fault(
    fault_address: u64,
    pte_ptr: u64,
    pte_value: u64,
    is_write: bool,
    is_user: bool,
) -> NTSTATUS {
    // Для user-mode адресов обязательна VAD проверка
    if fault_address < MM_SYSTEM_RANGE_START {
        // VAD Gate: проверяем что адрес в валидном committed VAD
        let vad_check = mi_check_vad_for_fault(fault_address);

        match vad_check {
            VadCheckResult::NotInVad => {
                // Адрес не принадлежит никакому VAD
                return STATUS_ACCESS_VIOLATION;
            },
            VadCheckResult::NotCommitted => {
                // VAD есть, но страница не committed
                return STATUS_ACCESS_VIOLATION;
            },
            VadCheckResult::StackGuard(vad) => {
                // Guard page около стека — нужно расширить стек
                return mi_handle_stack_growth(fault_address, vad);
            },
            VadCheckResult::Committed(_vad) => {
                // Страница committed — продолжаем обработку
            },
        }
    }

    // Анализируем software PTE state
    if pte_value == 0 {
        // PTE полностью нулевой — но VAD говорит что committed
        // Это может быть после decommit отдельных страниц в committed VAD
        // Или ошибка состояния. В NT это access violation.
        return STATUS_ACCESS_VIOLATION;
    }

    // Проверяем demand-zero bit
    if (pte_value & pte_state::DEMAND_ZERO) != 0 {
        return mi_handle_demand_zero_fault(fault_address, pte_ptr, pte_value, is_write);
    }

    // Проверяем transition bit
    if (pte_value & pte_state::TRANSITION) != 0 {
        return mi_handle_transition_fault(fault_address, pte_ptr, pte_value);
    }

    // Проверяем guard bit
    if (pte_value & pte_state::GUARD) != 0 {
        return mi_handle_guard_page_fault(fault_address, pte_ptr, pte_value);
    }

    // Проверяем prototype bit
    if (pte_value & pte_state::PROTOTYPE) != 0 {
        return mi_handle_prototype_fault(fault_address, pte_ptr, pte_value, is_write, is_user);
    }

    // Неизвестное состояние PTE
    STATUS_ACCESS_VIOLATION
}

// =============================================================================
// VAD Check
// =============================================================================

/// Результат проверки VAD для fault
enum VadCheckResult {
    /// Адрес не в VAD
    NotInVad,
    /// В VAD, но не committed
    NotCommitted,
    /// Guard page для стека — нужен stack growth
    StackGuard(*mut MMVAD_SHORT),
    /// Committed страница — можно обрабатывать fault
    Committed(*mut MMVAD_SHORT),
}

/// Проверяет VAD для fault address
fn mi_check_vad_for_fault(fault_address: u64) -> VadCheckResult {
    unsafe {
        // Получаем текущий процесс
        let process = ps_get_current_process();
        if process.is_null() {
            return VadCheckResult::NotInVad;
        }

        // Получаем VAD root
        let vad_root = (*process).vad_root as *mut MM_AVL_TABLE;
        if vad_root.is_null() {
            return VadCheckResult::NotInVad;
        }

        // Ищем VAD
        let vad = mi_find_vad(vad_root, fault_address);
        if vad.is_null() {
            return VadCheckResult::NotInVad;
        }

        // Проверяем тип VAD
        let vad_type = (*vad).flags.vad_type();

        // Для private memory проверяем commit
        if vad_type == VAD_TYPE::VadPrivateMemory {
            if !(*vad).flags.commit() {
                return VadCheckResult::NotCommitted;
            }
            return VadCheckResult::Committed(vad);
        }

        // Для mapped/image секций — всегда считаем committed (prototype PTE решает)
        if vad_type == VAD_TYPE::VadMapped || vad_type == VAD_TYPE::VadImage {
            return VadCheckResult::Committed(vad);
        }

        // Другие типы — не поддерживаем пока
        VadCheckResult::NotCommitted
    }
}

/// Обрабатывает stack growth при guard page fault
fn mi_handle_stack_growth(fault_address: u64, vad: *mut MMVAD_SHORT) -> NTSTATUS {
    unsafe {
        // В NT stack growth происходит когда:
        // 1. Fault на guard page около конца стека
        // 2. Система расширяет committed область стека
        // 3. Перемещает guard page дальше

        // Для простоты: если fault в пределах VAD и VAD committed,
        // просто делаем demand-zero для этой страницы

        if vad.is_null() {
            return STATUS_ACCESS_VIOLATION;
        }

        let vpn = fault_address >> VPN_SHIFT;

        // Проверяем что адрес в пределах VAD
        if vpn < (*vad).starting_vpn || vpn > (*vad).ending_vpn {
            return STATUS_ACCESS_VIOLATION;
        }

        // Получаем protection из VAD
        let protection = (*vad).flags.protection();

        // Создаем demand-zero PTE для этой страницы
        let pte_ptr = mm_get_pte_address(fault_address) as *mut u64;
        let pte_value = pte_state::DEMAND_ZERO | ((protection as u64 & 0x1F) << 5);
        core::ptr::write_volatile(pte_ptr, pte_value);

        // Теперь выделяем страницу (как в demand-zero fault)
        let pfn = mi_allocate_pfn();
        if pfn.is_none() {
            return STATUS_IN_PAGE_ERROR;
        }
        let pfn = pfn.unwrap();

        mi_zero_physical_page(pfn);

        let pte_flags = mi_software_protection_to_pte(protection);
        let new_pte = ((pfn as u64) << 12) | pte_flags | PTE_VALID;
        core::ptr::write_volatile(pte_ptr, new_pte);

        cpu::invlpg(fault_address);

        // Возвращаем GUARD_PAGE_VIOLATION чтобы usermode знал о расширении стека
        // В некоторых сценариях это нормально, в других — исключение
        STATUS_SUCCESS
    }
}

// =============================================================================
// Demand-Zero Fault
// =============================================================================

/// Обрабатывает demand-zero fault
///
/// Первый доступ к committed private page.
fn mi_handle_demand_zero_fault(
    fault_address: u64,
    pte_ptr: u64,
    pte_value: u64,
    _is_write: bool,
) -> NTSTATUS {
    // Извлекаем protection из software PTE
    let protection = (pte_value >> 5) & 0x1F;

    // Выделяем физическую страницу
    let pfn = unsafe { mi_allocate_pfn() };
    if pfn.is_none() {
        // Нет свободной памяти
        return STATUS_IN_PAGE_ERROR;
    }
    let pfn = pfn.unwrap();

    // Страница из mi_allocate_pfn может быть из free list (не обнулена)
    // Для demand-zero нужно обнулить
    unsafe {
        mi_zero_physical_page(pfn);
    }

    // Конвертируем protection в PTE flags
    let pte_flags = mi_software_protection_to_pte(protection as u32);

    // Строим valid PTE
    let new_pte = ((pfn as u64) << 12) | pte_flags | PTE_VALID;

    // Атомарно устанавливаем PTE
    unsafe {
        core::ptr::write_volatile(pte_ptr as *mut u64, new_pte);
    }

    // Инвалидируем TLB
    cpu::invlpg(fault_address);

    STATUS_SUCCESS
}

// =============================================================================
// Transition Fault
// =============================================================================

/// Обрабатывает transition fault
///
/// Страница в standby/modified list — можно вернуть без I/O.
fn mi_handle_transition_fault(fault_address: u64, pte_ptr: u64, pte_value: u64) -> NTSTATUS {
    // Извлекаем PFN из transition PTE (биты 12-51)
    let pfn = ((pte_value >> 12) & 0xFFFFFFFFFF) as usize;

    unsafe {
        // Проверяем и удаляем страницу из standby/modified list
        if let Some(entry) = get_pfn_entry(pfn) {
            let location = entry.page_location;

            // Проверяем что страница в ожидаемом состоянии
            if location == MMLISTS::StandbyPageList as u8 {
                // Удаляем из standby list
                let standby_list = &mut *MM_STANDBY_PAGE_LIST_HEAD.get();
                mi_remove_page_from_list(standby_list, pfn);
            } else if location == MMLISTS::ModifiedPageList as u8 {
                // Удаляем из modified list
                let modified_list = &mut *MM_MODIFIED_PAGE_LIST_HEAD.get();
                mi_remove_page_from_list(modified_list, pfn);
            }
            // Если страница не в списке — возможно уже активна, продолжаем

            // Обновляем состояние PFN
            if let Some(entry_mut) = get_pfn_entry_mut(pfn) {
                entry_mut.page_location = MMLISTS::ActiveAndValid as u8;
                entry_mut.reference_count = entry_mut.reference_count.saturating_add(1);
            }
        }

        // Извлекаем protection
        let protection = (pte_value >> 5) & 0x1F;
        let pte_flags = mi_software_protection_to_pte(protection as u32);

        // Строим valid PTE
        let new_pte = ((pfn as u64) << 12) | pte_flags | PTE_VALID;

        core::ptr::write_volatile(pte_ptr as *mut u64, new_pte);
    }

    cpu::invlpg(fault_address);

    STATUS_SUCCESS
}

/// Получает mutable entry для PFN (helper для transition fault)
unsafe fn get_pfn_entry_mut(pfn: usize) -> Option<&'static mut MMPFN> {
    unsafe {
        let db = &mut *MM_PFN_DATABASE.get();
        db.get_mut(pfn)
    }
}

// =============================================================================
// Guard Page Fault
// =============================================================================

/// Обрабатывает guard page fault
///
/// Снимает guard bit и возвращает STATUS_GUARD_PAGE_VIOLATION.
fn mi_handle_guard_page_fault(fault_address: u64, pte_ptr: u64, pte_value: u64) -> NTSTATUS {
    // Снимаем guard bit и делаем страницу demand-zero
    let new_pte = pte_value & !pte_state::GUARD;

    unsafe {
        core::ptr::write_volatile(pte_ptr as *mut u64, new_pte);
    }

    cpu::invlpg(fault_address);

    // Возвращаем специальный статус для guard page
    STATUS_GUARD_PAGE_VIOLATION
}

// =============================================================================
// Prototype PTE Fault
// =============================================================================

/// Обрабатывает prototype PTE fault
///
/// Prototype PTE используется для shared sections (mapped files, shared memory).
/// Процессный PTE содержит указатель на prototype PTE в SEGMENT.
fn mi_handle_prototype_fault(
    fault_address: u64,
    pte_ptr: u64,
    pte_value: u64,
    is_write: bool,
    _is_user: bool,
) -> NTSTATUS {
    // Извлекаем адрес prototype PTE из процессного PTE
    // Format: [prototype_pte_address:48][prototype=1][valid=0]
    let proto_pte_addr = pte_value & !0xFFF; // Маскируем младшие биты

    if proto_pte_addr == 0 {
        return STATUS_ACCESS_VIOLATION;
    }

    // Читаем prototype PTE
    let proto_pte_value = unsafe { core::ptr::read_volatile(proto_pte_addr as *const u64) };

    // Проверяем состояние prototype PTE
    if (proto_pte_value & PTE_VALID) != 0 {
        // Prototype PTE валиден — страница уже в памяти
        // Просто копируем в процессный PTE
        return mi_resolve_valid_prototype(fault_address, pte_ptr, proto_pte_value, is_write);
    }

    // Prototype PTE не валиден — нужно подгрузить страницу
    if (proto_pte_value & pte_state::DEMAND_ZERO) != 0 {
        // Demand-zero prototype — выделяем страницу
        return mi_resolve_prototype_demand_zero(
            fault_address,
            pte_ptr,
            proto_pte_addr,
            proto_pte_value,
        );
    }

    if (proto_pte_value & pte_state::TRANSITION) != 0 {
        // Transition prototype — страница в standby/modified
        return mi_resolve_prototype_transition(
            fault_address,
            pte_ptr,
            proto_pte_addr,
            proto_pte_value,
        );
    }

    // Неизвестное состояние prototype PTE
    STATUS_ACCESS_VIOLATION
}

/// Разрешает fault когда prototype PTE уже валиден
fn mi_resolve_valid_prototype(
    fault_address: u64,
    pte_ptr: u64,
    proto_pte_value: u64,
    is_write: bool,
) -> NTSTATUS {
    // Извлекаем PFN из prototype PTE
    let pfn = ((proto_pte_value >> 12) & 0xFFFFFFFFFF) as usize;

    // Для COW sections: если запись, но prototype read-only или COW
    if is_write {
        let is_writecopy = (proto_pte_value & PTE_WRITECOPY) != 0;
        let is_readonly = (proto_pte_value & PTE_READWRITE) == 0;

        if is_writecopy || is_readonly {
            // Нужен copy-on-write
            return mi_perform_cow_for_prototype(fault_address, pte_ptr, pfn, proto_pte_value);
        }
    }

    // Увеличиваем share count для страницы
    unsafe {
        if let Some(entry) = get_pfn_entry_mut(pfn) {
            entry.share_count = entry.share_count.saturating_add(1);
        }
    }

    // Строим процессный PTE (копируем из prototype)
    let new_pte = proto_pte_value;

    unsafe {
        core::ptr::write_volatile(pte_ptr as *mut u64, new_pte);
    }

    cpu::invlpg(fault_address);

    STATUS_SUCCESS
}

/// Выполняет COW для prototype-backed страницы
fn mi_perform_cow_for_prototype(
    fault_address: u64,
    pte_ptr: u64,
    old_pfn: usize,
    proto_pte_value: u64,
) -> NTSTATUS {
    unsafe {
        // Выделяем новую страницу
        let new_pfn = mi_allocate_pfn();
        if new_pfn.is_none() {
            return STATUS_IN_PAGE_ERROR;
        }
        let new_pfn = new_pfn.unwrap();

        // Копируем содержимое
        mi_copy_physical_page(old_pfn, new_pfn);

        // Настраиваем новую страницу как private (не shared)
        if let Some(new_entry) = get_pfn_entry_mut(new_pfn) {
            new_entry.page_location = MMLISTS::ActiveAndValid as u8;
            new_entry.reference_count = 1;
            new_entry.share_count = 1;
            new_entry.pte_address = pte_ptr as *mut u64;
        }

        // Строим новый PTE (private, writable)
        let preserved_flags = proto_pte_value & (PTE_USER | PTE_NX | PTE_GLOBAL);
        let new_pte =
            ((new_pfn as u64) << 12) | PTE_VALID | PTE_READWRITE | preserved_flags;

        core::ptr::write_volatile(pte_ptr as *mut u64, new_pte);

        cpu::invlpg(fault_address);

        STATUS_SUCCESS
    }
}

/// Разрешает demand-zero prototype fault
fn mi_resolve_prototype_demand_zero(
    fault_address: u64,
    pte_ptr: u64,
    proto_pte_addr: u64,
    proto_pte_value: u64,
) -> NTSTATUS {
    // Извлекаем protection
    let protection = (proto_pte_value >> 5) & 0x1F;

    // Выделяем физическую страницу
    let pfn = unsafe { mi_allocate_pfn() };
    if pfn.is_none() {
        return STATUS_IN_PAGE_ERROR;
    }
    let pfn = pfn.unwrap();

    // Обнуляем страницу
    unsafe {
        mi_zero_physical_page(pfn);
    }

    // Конвертируем protection в PTE flags
    let pte_flags = mi_software_protection_to_pte(protection as u32);

    // Обновляем prototype PTE (делаем valid)
    let new_proto = ((pfn as u64) << 12) | pte_flags | PTE_VALID;
    unsafe {
        core::ptr::write_volatile(proto_pte_addr as *mut u64, new_proto);
    }

    // Устанавливаем процессный PTE
    unsafe {
        core::ptr::write_volatile(pte_ptr as *mut u64, new_proto);
    }

    cpu::invlpg(fault_address);

    STATUS_SUCCESS
}

/// Разрешает transition prototype fault
fn mi_resolve_prototype_transition(
    fault_address: u64,
    pte_ptr: u64,
    proto_pte_addr: u64,
    proto_pte_value: u64,
) -> NTSTATUS {
    // Извлекаем PFN из transition PTE
    let pfn = (proto_pte_value >> 12) & 0xFFFFFFFFFF;
    let protection = (proto_pte_value >> 5) & 0x1F;

    // TODO: Удалить страницу из standby/modified list

    // Конвертируем protection в PTE flags
    let pte_flags = mi_software_protection_to_pte(protection as u32);

    // Делаем prototype PTE valid
    let new_proto = (pfn << 12) | pte_flags | PTE_VALID;
    unsafe {
        core::ptr::write_volatile(proto_pte_addr as *mut u64, new_proto);
    }

    // Устанавливаем процессный PTE
    unsafe {
        core::ptr::write_volatile(pte_ptr as *mut u64, new_proto);
    }

    cpu::invlpg(fault_address);

    STATUS_SUCCESS
}

// =============================================================================
// Protection Fault
// =============================================================================

/// Обрабатывает protection violation на present page
fn mi_handle_protection_fault(
    fault_address: u64,
    pte_value: u64,
    is_write: bool,
    _is_user: bool,
) -> NTSTATUS {
    // Проверяем copy-on-write
    if is_write && (pte_value & PTE_WRITECOPY) != 0 {
        return mi_handle_copy_on_write(fault_address, pte_value);
    }

    // Другие protection violations — access violation
    STATUS_ACCESS_VIOLATION
}

/// Обрабатывает copy-on-write
///
/// При записи в COW страницу:
/// 1. Выделяем новую физическую страницу
/// 2. Копируем содержимое старой страницы
/// 3. Обновляем PTE (снимаем COW, ставим writable)
/// 4. Уменьшаем share count оригинальной страницы
fn mi_handle_copy_on_write(fault_address: u64, pte_value: u64) -> NTSTATUS {
    unsafe {
        // Извлекаем старый PFN
        let old_pfn = ((pte_value >> 12) & 0xFFFFFFFFFF) as usize;

        // Выделяем новую страницу
        let new_pfn = mi_allocate_pfn();
        if new_pfn.is_none() {
            return STATUS_IN_PAGE_ERROR;
        }
        let new_pfn = new_pfn.unwrap();

        // Копируем содержимое старой страницы в новую
        mi_copy_physical_page(old_pfn, new_pfn);

        // Уменьшаем share count старой страницы
        if let Some(old_entry) = get_pfn_entry_mut(old_pfn) {
            if old_entry.share_count > 0 {
                old_entry.share_count -= 1;
            }

            // Если share_count стал 0 и reference_count тоже 0, можно освободить
            if old_entry.share_count == 0 && old_entry.reference_count == 0 {
                // Страница больше никому не нужна — освобождаем
                mi_free_pfn(old_pfn);
            }
        }

        // Настраиваем новую страницу
        if let Some(new_entry) = get_pfn_entry_mut(new_pfn) {
            new_entry.page_location = MMLISTS::ActiveAndValid as u8;
            new_entry.reference_count = 1;
            new_entry.share_count = 1;
            new_entry.pte_address = mm_get_pte_address(fault_address) as *mut u64;
        }

        // Строим новый PTE (writable, без COW)
        // Сохраняем остальные флаги (USER, NX и т.д.)
        let preserved_flags = pte_value & (PTE_USER | PTE_NX | PTE_GLOBAL | PTE_DISABLE_CACHE);
        let new_pte =
            ((new_pfn as u64) << 12) | PTE_VALID | PTE_READWRITE | preserved_flags;

        // Атомарно обновляем PTE
        let pte_ptr = mm_get_pte_address(fault_address) as *mut u64;
        core::ptr::write_volatile(pte_ptr, new_pte);

        // Инвалидируем TLB
        cpu::invlpg(fault_address);

        STATUS_SUCCESS
    }
}

// mi_copy_physical_page импортирован из hyperspace

// =============================================================================
// Helper Functions
// =============================================================================

/// Проверяет, валиден ли указатель на PTE
///
/// Для этого проверяем, что все уровни page table существуют.
fn mi_is_pte_pointer_valid(va: u64) -> bool {
    // Проверяем PXE (PML4 entry)
    let pxe_ptr = mm_get_pxe_address(va) as *const u64;
    let pxe_value = unsafe { core::ptr::read_volatile(pxe_ptr) };
    if (pxe_value & PTE_VALID) == 0 {
        return false;
    }

    // Проверяем PPE (PDPT entry)
    let ppe_ptr = mm_get_ppe_address(va) as *const u64;
    let ppe_value = unsafe { core::ptr::read_volatile(ppe_ptr) };
    if (ppe_value & PTE_VALID) == 0 {
        return false;
    }

    // Проверяем PDE (PD entry)
    let pde_ptr = mm_get_pde_address(va) as *const u64;
    let pde_value = unsafe { core::ptr::read_volatile(pde_ptr) };
    if (pde_value & PTE_VALID) == 0 {
        return false;
    }

    // Все уровни существуют — PTE pointer валиден
    true
}

/// Конвертирует software protection в PTE flags
fn mi_software_protection_to_pte(protection: u32) -> u64 {
    let mut flags: u64 = 0;

    // Базовые флаги
    match protection & 0x7 {
        MM_READONLY | MM_EXECUTE_READ => {
            // Read-only
        },
        MM_READWRITE | MM_EXECUTE_READWRITE => {
            flags |= PTE_READWRITE;
        },
        MM_WRITECOPY | MM_EXECUTE_WRITECOPY => {
            flags |= PTE_WRITECOPY;
        },
        MM_EXECUTE => {
            // Execute-only (если поддерживается)
        },
        _ => {
            // No access — оставляем 0
        },
    }

    // NX bit для non-execute pages
    if (protection & 0x4) == 0 {
        // Нет execute permission
        flags |= PTE_NX;
    }

    // User bit
    flags |= PTE_USER; // TODO: определять по контексту

    flags
}

/// Fatal page fault — вызывает bugcheck
fn mi_fatal_page_fault(fault_address: u64, error_code: u64, rip: u64) -> NTSTATUS {
    let is_write = (error_code & PF_WRITE) != 0;

    ke_bug_check_ex(
        bugcheck_codes::PAGE_FAULT_IN_NONPAGED_AREA,
        fault_address as usize,
        if is_write { 1 } else { 0 },
        rip as usize,
        error_code as usize,
    );
}

// =============================================================================
// PTE Commit Functions
// =============================================================================

/// Устанавливает demand-zero PTE для committed page
///
/// Вызывается из NtAllocateVirtualMemory при MEM_COMMIT.
pub unsafe fn mi_make_demand_zero_pte(va: u64, protection: u32) {
    unsafe {
        let pte_ptr = mm_get_pte_address(va) as *mut u64;

        // Формируем software PTE: demand_zero bit + protection
        let pte_value = pte_state::DEMAND_ZERO | ((protection as u64 & 0x1F) << 5);

        core::ptr::write_volatile(pte_ptr, pte_value);
    }
}

/// Устанавливает guard page PTE
pub unsafe fn mi_make_guard_pte(va: u64, protection: u32) {
    unsafe {
        let pte_ptr = mm_get_pte_address(va) as *mut u64;

        // Guard + demand-zero + protection
        let pte_value =
            pte_state::GUARD | pte_state::DEMAND_ZERO | ((protection as u64 & 0x1F) << 5);

        core::ptr::write_volatile(pte_ptr, pte_value);
    }
}

/// Деcommit страницу — обнуляет PTE
pub unsafe fn mi_decommit_page(va: u64) {
    unsafe {
        let pte_ptr = mm_get_pte_address(va) as *mut u64;
        let pte_value = core::ptr::read_volatile(pte_ptr);

        if (pte_value & PTE_VALID) != 0 {
            // Страница в памяти — нужно освободить PFN
            let pfn = ((pte_value >> 12) & 0xFFFFFFFFFF) as usize;
            mi_free_pfn(pfn);
        }

        // Обнуляем PTE
        core::ptr::write_volatile(pte_ptr, 0);
        cpu::invlpg(va);
    }
}
