//! Virtual Address Descriptor (VAD)
//!
//! VAD дерево описывает зарезервированные и committed регионы виртуальной
//! памяти процесса.
//!
//! # Архитектура
//!
//! Каждый процесс имеет своё VAD дерево (`EPROCESS.vad_root`).
//! VAD организованы в AVL-дерево для быстрого поиска по адресу.
//!
//! # Типы VAD
//!
//! - `MMVAD_SHORT` — базовая информация (start/end VPN, protection, flags)
//! - `MMVAD` — расширенная (+ section reference, subsection, etc.)
//!
//! # Операции
//!
//! - `mi_insert_vad()` — вставка VAD в дерево
//! - `mi_remove_vad()` — удаление VAD
//! - `mi_find_vad()` — поиск VAD по адресу
//! - `mi_find_empty_address_range()` — поиск свободного диапазона
//!
//! Источники:
//! - ReactOS: mm/ARM3/vadnode.c
//! - NT6.1: mm/vadnode.c

use core::ptr;

use super::types::_64K;
use super::types::MM_HIGHEST_USER_ADDRESS;
use super::types::MM_LOWEST_USER_ADDRESS;
use super::types::PAGE_SHIFT;
use super::types::PAGE_SIZE;
use crate::ke::spinlock::KSPIN_LOCK;
use crate::nt::NTSTATUS;
use crate::nt::PVOID;
use crate::nt::STATUS_CONFLICTING_ADDRESSES;
use crate::nt::STATUS_NO_MEMORY;
use crate::nt::STATUS_SUCCESS;

// =============================================================================
// Constants
// =============================================================================

/// Гранулярность выделения (64KB для совместимости с NT)
pub const MM_ALLOCATION_GRANULARITY: usize = _64K;

/// Сдвиг для VPN
pub const VPN_SHIFT: usize = PAGE_SHIFT;

// =============================================================================
// VAD Flags
// =============================================================================

/// VAD типы
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VAD_TYPE {
    /// Private memory (NtAllocateVirtualMemory)
    VadPrivateMemory = 0,
    /// Mapped view (NtMapViewOfSection)
    VadMapped = 1,
    /// Image section (PE image)
    VadImage = 2,
    /// AWE (Address Windowing Extensions)
    VadAwe = 3,
    /// Large pages
    VadLargePages = 4,
    /// Rotate physical memory
    VadRotatePhysical = 5,
}

/// VAD protection mask (внутренний формат MM)
pub type MM_PROTECTION_MASK = u32;

// =============================================================================
// MMVAD_FLAGS
// =============================================================================

/// Флаги VAD (упакованы в u32)
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MMVAD_FLAGS {
    value: u32,
}

impl MMVAD_FLAGS {
    pub const fn new() -> Self {
        Self { value: 0 }
    }

    /// Тип VAD (биты 0-2)
    #[inline]
    pub fn vad_type(&self) -> VAD_TYPE {
        match self.value & 0x7 {
            0 => VAD_TYPE::VadPrivateMemory,
            1 => VAD_TYPE::VadMapped,
            2 => VAD_TYPE::VadImage,
            3 => VAD_TYPE::VadAwe,
            4 => VAD_TYPE::VadLargePages,
            5 => VAD_TYPE::VadRotatePhysical,
            _ => VAD_TYPE::VadPrivateMemory,
        }
    }

    pub fn set_vad_type(&mut self, t: VAD_TYPE) {
        self.value = (self.value & !0x7) | (t as u32);
    }

    /// Protection (биты 3-7)
    #[inline]
    pub fn protection(&self) -> u32 {
        (self.value >> 3) & 0x1F
    }

    pub fn set_protection(&mut self, p: u32) {
        self.value = (self.value & !(0x1F << 3)) | ((p & 0x1F) << 3);
    }

    /// Commit (бит 8) — регион committed
    #[inline]
    pub fn commit(&self) -> bool {
        (self.value & (1 << 8)) != 0
    }

    pub fn set_commit(&mut self, c: bool) {
        if c {
            self.value |= 1 << 8;
        } else {
            self.value &= !(1 << 8);
        }
    }

    /// PrivateMemory (бит 9)
    #[inline]
    pub fn private_memory(&self) -> bool {
        (self.value & (1 << 9)) != 0
    }

    pub fn set_private_memory(&mut self, p: bool) {
        if p {
            self.value |= 1 << 9;
        } else {
            self.value &= !(1 << 9);
        }
    }

    /// NoChange (бит 10) — protection cannot be changed
    #[inline]
    pub fn no_change(&self) -> bool {
        (self.value & (1 << 10)) != 0
    }

    pub fn set_no_change(&mut self, n: bool) {
        if n {
            self.value |= 1 << 10;
        } else {
            self.value &= !(1 << 10);
        }
    }
}

// =============================================================================
// MMVAD_SHORT — базовая структура VAD
// =============================================================================

/// Базовая структура VAD (короткая версия)
#[repr(C)]
pub struct MMVAD_SHORT {
    /// Указатель на родителя в AVL дереве
    pub parent: *mut MMVAD_SHORT,
    /// Левый потомок
    pub left_child: *mut MMVAD_SHORT,
    /// Правый потомок
    pub right_child: *mut MMVAD_SHORT,

    /// Начальный VPN (Virtual Page Number)
    pub starting_vpn: u64,
    /// Конечный VPN (включительно)
    pub ending_vpn: u64,

    /// Флаги VAD
    pub flags: MMVAD_FLAGS,

    /// Баланс AVL дерева (-1, 0, +1)
    pub balance: i8,
    /// Padding
    _reserved: [u8; 3],

    /// Reference count
    pub reference_count: u32,

    /// Push lock для синхронизации
    pub push_lock: u64,
}

impl MMVAD_SHORT {
    pub const fn new() -> Self {
        Self {
            parent: ptr::null_mut(),
            left_child: ptr::null_mut(),
            right_child: ptr::null_mut(),
            starting_vpn: 0,
            ending_vpn: 0,
            flags: MMVAD_FLAGS::new(),
            balance: 0,
            _reserved: [0; 3],
            reference_count: 1,
            push_lock: 0,
        }
    }

    /// Возвращает начальный виртуальный адрес
    #[inline]
    pub fn start_address(&self) -> u64 {
        self.starting_vpn << VPN_SHIFT
    }

    /// Возвращает конечный виртуальный адрес (не включительно)
    #[inline]
    pub fn end_address(&self) -> u64 {
        (self.ending_vpn + 1) << VPN_SHIFT
    }

    /// Возвращает размер региона в байтах
    #[inline]
    pub fn size(&self) -> usize {
        ((self.ending_vpn - self.starting_vpn + 1) as usize) << VPN_SHIFT
    }

    /// Проверяет, содержит ли VAD указанный адрес
    #[inline]
    pub fn contains_address(&self, va: u64) -> bool {
        let vpn = va >> VPN_SHIFT;
        vpn >= self.starting_vpn && vpn <= self.ending_vpn
    }
}

// =============================================================================
// MMVAD — расширенная структура VAD
// =============================================================================

/// Расширенная структура VAD (для mapped sections)
#[repr(C)]
pub struct MMVAD {
    /// Базовая часть
    pub short: MMVAD_SHORT,

    /// Commit charge для этого VAD
    pub commit_charge: u64,

    /// Указатель на Control Area (для mapped files)
    pub control_area: PVOID,

    /// Первая prototype PTE
    pub first_prototype_pte: PVOID,

    /// Последняя prototype PTE
    pub last_contiguous_pte: PVOID,

    /// Subsection внутри section
    pub subsection: PVOID,
}

impl MMVAD {
    pub const fn new() -> Self {
        Self {
            short: MMVAD_SHORT::new(),
            commit_charge: 0,
            control_area: ptr::null_mut(),
            first_prototype_pte: ptr::null_mut(),
            last_contiguous_pte: ptr::null_mut(),
            subsection: ptr::null_mut(),
        }
    }
}

// =============================================================================
// MM_AVL_TABLE — корень VAD дерева
// =============================================================================

/// Корень AVL дерева VAD
#[repr(C)]
pub struct MM_AVL_TABLE {
    /// Указатель на корневой узел
    pub root: *mut MMVAD_SHORT,

    /// Количество узлов в дереве
    pub node_count: usize,

    /// Hint для поиска свободного места
    pub free_hint: *mut MMVAD_SHORT,

    /// Спинлок для синхронизации
    pub lock: KSPIN_LOCK,
}

impl MM_AVL_TABLE {
    pub const fn new() -> Self {
        Self {
            root: ptr::null_mut(),
            node_count: 0,
            free_hint: ptr::null_mut(),
            lock: KSPIN_LOCK::new(),
        }
    }

    /// Проверяет, пусто ли дерево
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.root.is_null()
    }
}

// =============================================================================
// VAD Tree Operations
// =============================================================================

/// MiFindVad
///
/// Ищет VAD, содержащий указанный виртуальный адрес.
///
/// # Arguments
/// * `table` - корень VAD дерева
/// * `va` - виртуальный адрес для поиска
///
/// # Returns
/// Указатель на VAD или null если не найден.
pub unsafe fn mi_find_vad(table: *mut MM_AVL_TABLE, va: u64) -> *mut MMVAD_SHORT {
    unsafe {
        if table.is_null() {
            return ptr::null_mut();
        }

        let vpn = va >> VPN_SHIFT;
        let mut current = (*table).root;

        while !current.is_null() {
            let node = &*current;

            if vpn < node.starting_vpn {
                current = node.left_child;
            } else if vpn > node.ending_vpn {
                current = node.right_child;
            } else {
                // Нашли VAD, содержащий этот VPN
                return current;
            }
        }

        ptr::null_mut()
    }
}

/// MiFindEmptyAddressRange
///
/// Ищет свободный диапазон адресов указанного размера.
///
/// # Arguments
/// * `table` - корень VAD дерева
/// * `size` - требуемый размер в байтах
/// * `alignment` - выравнивание (обычно 64KB)
/// * `top_down` - искать сверху вниз
///
/// # Returns
/// Начальный адрес найденного диапазона или 0.
pub unsafe fn mi_find_empty_address_range(
    table: *mut MM_AVL_TABLE,
    size: usize,
    alignment: usize,
    top_down: bool,
) -> u64 {
    unsafe {
        if table.is_null() || size == 0 {
            return 0;
        }

        let pages_needed = ((size + PAGE_SIZE - 1) / PAGE_SIZE) as u64;
        let align_pages = (alignment / PAGE_SIZE) as u64;

        // Границы user space
        let lowest_vpn = MM_LOWEST_USER_ADDRESS >> VPN_SHIFT;
        let highest_vpn = MM_HIGHEST_USER_ADDRESS >> VPN_SHIFT;

        if (*table).is_empty() {
            // Дерево пустое — возвращаем выровненный lowest address
            let start_vpn = (lowest_vpn + align_pages - 1) & !(align_pages - 1);
            if start_vpn + pages_needed <= highest_vpn {
                return start_vpn << VPN_SHIFT;
            }
            return 0;
        }

        if top_down {
            // Поиск сверху вниз (MEM_TOP_DOWN)
            mi_find_gap_top_down(table, pages_needed, align_pages, lowest_vpn, highest_vpn)
        } else {
            // Поиск снизу вверх (по умолчанию)
            mi_find_gap_bottom_up(table, pages_needed, align_pages, lowest_vpn, highest_vpn)
        }
    }
}

/// Ищет промежуток снизу вверх
unsafe fn mi_find_gap_bottom_up(
    table: *mut MM_AVL_TABLE,
    pages_needed: u64,
    align_pages: u64,
    lowest_vpn: u64,
    highest_vpn: u64,
) -> u64 {
    unsafe {
        // In-order обход дерева
        let mut current_vpn = lowest_vpn;
        let mut stack: [*mut MMVAD_SHORT; 64] = [ptr::null_mut(); 64];
        let mut stack_idx = 0;
        let mut node = (*table).root;

        // In-order traversal
        loop {
            while !node.is_null() {
                if stack_idx >= 64 {
                    return 0; // Стек переполнен
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

            // Проверяем промежуток перед этим VAD
            let gap_start = (current_vpn + align_pages - 1) & !(align_pages - 1);
            let gap_end = (*node).starting_vpn;

            if gap_end > gap_start && gap_end - gap_start >= pages_needed {
                return gap_start << VPN_SHIFT;
            }

            // Обновляем текущую позицию
            current_vpn = (*node).ending_vpn + 1;

            node = (*node).right_child;
        }

        // Проверяем промежуток после последнего VAD
        let gap_start = (current_vpn + align_pages - 1) & !(align_pages - 1);
        if highest_vpn > gap_start && highest_vpn - gap_start >= pages_needed {
            return gap_start << VPN_SHIFT;
        }

        0
    }
}

/// Ищет промежуток сверху вниз
unsafe fn mi_find_gap_top_down(
    table: *mut MM_AVL_TABLE,
    pages_needed: u64,
    align_pages: u64,
    lowest_vpn: u64,
    highest_vpn: u64,
) -> u64 {
    unsafe {
        // Reverse in-order обход
        let mut current_vpn = highest_vpn;
        let mut stack: [*mut MMVAD_SHORT; 64] = [ptr::null_mut(); 64];
        let mut stack_idx = 0;
        let mut node = (*table).root;

        loop {
            while !node.is_null() {
                if stack_idx >= 64 {
                    return 0;
                }
                stack[stack_idx] = node;
                stack_idx += 1;
                node = (*node).right_child;
            }

            if stack_idx == 0 {
                break;
            }

            stack_idx -= 1;
            node = stack[stack_idx];

            // Проверяем промежуток после этого VAD
            let gap_end = current_vpn;
            let gap_start_aligned = ((*node).ending_vpn + 1 + align_pages - 1) & !(align_pages - 1);

            if gap_end > gap_start_aligned && gap_end - gap_start_aligned >= pages_needed {
                // Выравниваем сверху
                let result = ((gap_end - pages_needed) & !(align_pages - 1)) << VPN_SHIFT;
                if result >= (lowest_vpn << VPN_SHIFT) {
                    return result;
                }
            }

            current_vpn = (*node).starting_vpn;
            node = (*node).left_child;
        }

        // Проверяем промежуток перед первым VAD
        let gap_end = current_vpn;
        let gap_start = lowest_vpn;
        let gap_start_aligned = (gap_start + align_pages - 1) & !(align_pages - 1);

        if gap_end > gap_start_aligned && gap_end - gap_start_aligned >= pages_needed {
            let result = ((gap_end - pages_needed) & !(align_pages - 1)) << VPN_SHIFT;
            if result >= (lowest_vpn << VPN_SHIFT) {
                return result;
            }
        }

        0
    }
}

// =============================================================================
// AVL Tree Balancing Operations
// =============================================================================

/// Вычисляет высоту поддерева (рекурсивно)
/// В production использовать кэшированную высоту в balance field
unsafe fn mi_get_height(node: *mut MMVAD_SHORT) -> i32 {
    if node.is_null() {
        return 0;
    }
    unsafe {
        let left_height = mi_get_height((*node).left_child);
        let right_height = mi_get_height((*node).right_child);
        1 + if left_height > right_height {
            left_height
        } else {
            right_height
        }
    }
}

/// Вычисляет баланс узла (высота правого - высота левого)
unsafe fn mi_get_balance(node: *mut MMVAD_SHORT) -> i8 {
    if node.is_null() {
        return 0;
    }
    unsafe {
        let left_height = mi_get_height((*node).left_child);
        let right_height = mi_get_height((*node).right_child);
        (right_height - left_height) as i8
    }
}

/// Обновляет баланс узла
unsafe fn mi_update_balance(node: *mut MMVAD_SHORT) {
    if !node.is_null() {
        unsafe {
            (*node).balance = mi_get_balance(node);
        }
    }
}

/// Left rotation (для right-heavy subtree)
///
/// ```text
///     x                y
///    / \              / \
///   a   y    =>      x   c
///      / \          / \
///     b   c        a   b
/// ```
unsafe fn mi_rotate_left(table: *mut MM_AVL_TABLE, x: *mut MMVAD_SHORT) {
    unsafe {
        let y = (*x).right_child;
        if y.is_null() {
            return;
        }

        // Переносим левое поддерево y в правое поддерево x
        (*x).right_child = (*y).left_child;
        if !(*y).left_child.is_null() {
            (*(*y).left_child).parent = x;
        }

        // Обновляем parent связи
        (*y).parent = (*x).parent;
        if (*x).parent.is_null() {
            (*table).root = y;
        } else if (*(*x).parent).left_child == x {
            (*(*x).parent).left_child = y;
        } else {
            (*(*x).parent).right_child = y;
        }

        // Завершаем rotation
        (*y).left_child = x;
        (*x).parent = y;

        // Обновляем балансы
        mi_update_balance(x);
        mi_update_balance(y);
    }
}

/// Right rotation (для left-heavy subtree)
///
/// ```text
///       y              x
///      / \            / \
///     x   c    =>    a   y
///    / \                / \
///   a   b              b   c
/// ```
unsafe fn mi_rotate_right(table: *mut MM_AVL_TABLE, y: *mut MMVAD_SHORT) {
    unsafe {
        let x = (*y).left_child;
        if x.is_null() {
            return;
        }

        // Переносим правое поддерево x в левое поддерево y
        (*y).left_child = (*x).right_child;
        if !(*x).right_child.is_null() {
            (*(*x).right_child).parent = y;
        }

        // Обновляем parent связи
        (*x).parent = (*y).parent;
        if (*y).parent.is_null() {
            (*table).root = x;
        } else if (*(*y).parent).left_child == y {
            (*(*y).parent).left_child = x;
        } else {
            (*(*y).parent).right_child = x;
        }

        // Завершаем rotation
        (*x).right_child = y;
        (*y).parent = x;

        // Обновляем балансы
        mi_update_balance(y);
        mi_update_balance(x);
    }
}

/// Балансирует дерево после вставки/удаления, начиная с указанного узла
unsafe fn mi_rebalance_tree(table: *mut MM_AVL_TABLE, mut node: *mut MMVAD_SHORT) {
    unsafe {
        while !node.is_null() {
            mi_update_balance(node);
            let balance = (*node).balance;

            if balance < -1 {
                // Left-heavy
                let left = (*node).left_child;
                if !left.is_null() && (*left).balance > 0 {
                    // Left-Right case: сначала left rotate на left child
                    mi_rotate_left(table, left);
                }
                // Left-Left case или после LR: right rotate
                mi_rotate_right(table, node);
            } else if balance > 1 {
                // Right-heavy
                let right = (*node).right_child;
                if !right.is_null() && (*right).balance < 0 {
                    // Right-Left case: сначала right rotate на right child
                    mi_rotate_right(table, right);
                }
                // Right-Right case или после RL: left rotate
                mi_rotate_left(table, node);
            }

            node = (*node).parent;
        }
    }
}

/// MiInsertVad
///
/// Вставляет VAD в дерево с AVL балансировкой.
///
/// # Arguments
/// * `table` - корень VAD дерева
/// * `vad` - VAD для вставки
///
/// # Returns
/// STATUS_SUCCESS или код ошибки.
pub unsafe fn mi_insert_vad(table: *mut MM_AVL_TABLE, vad: *mut MMVAD_SHORT) -> NTSTATUS {
    unsafe {
        if table.is_null() || vad.is_null() {
            return STATUS_NO_MEMORY;
        }

        let new_start = (*vad).starting_vpn;
        let new_end = (*vad).ending_vpn;

        // Инициализируем новый узел
        (*vad).left_child = ptr::null_mut();
        (*vad).right_child = ptr::null_mut();
        (*vad).balance = 0;

        if (*table).is_empty() {
            // Первый узел
            (*table).root = vad;
            (*vad).parent = ptr::null_mut();
            (*table).node_count = 1;
            return STATUS_SUCCESS;
        }

        // Ищем место для вставки
        let mut parent: *mut MMVAD_SHORT = ptr::null_mut();
        let mut current = (*table).root;
        let mut go_left = false;

        while !current.is_null() {
            parent = current;

            if new_end < (*current).starting_vpn {
                go_left = true;
                current = (*current).left_child;
            } else if new_start > (*current).ending_vpn {
                go_left = false;
                current = (*current).right_child;
            } else {
                // Перекрытие с существующим VAD
                return STATUS_CONFLICTING_ADDRESSES;
            }
        }

        // Вставляем
        (*vad).parent = parent;

        if go_left {
            (*parent).left_child = vad;
        } else {
            (*parent).right_child = vad;
        }

        (*table).node_count += 1;

        // AVL rebalancing
        mi_rebalance_tree(table, vad);

        STATUS_SUCCESS
    }
}

/// Находит минимальный узел в поддереве
unsafe fn mi_find_minimum(mut node: *mut MMVAD_SHORT) -> *mut MMVAD_SHORT {
    unsafe {
        while !node.is_null() && !(*node).left_child.is_null() {
            node = (*node).left_child;
        }
        node
    }
}

/// MiRemoveVad
///
/// Удаляет VAD из дерева с AVL балансировкой.
///
/// # Arguments
/// * `table` - корень VAD дерева  
/// * `vad` - VAD для удаления
pub unsafe fn mi_remove_vad(table: *mut MM_AVL_TABLE, vad: *mut MMVAD_SHORT) {
    unsafe {
        if table.is_null() || vad.is_null() {
            return;
        }

        let parent = (*vad).parent;
        let left = (*vad).left_child;
        let right = (*vad).right_child;

        // Узел для начала rebalancing
        let mut rebalance_start: *mut MMVAD_SHORT;

        if left.is_null() && right.is_null() {
            // Случай 1: Лист (нет детей)
            if parent.is_null() {
                (*table).root = ptr::null_mut();
            } else if (*parent).left_child == vad {
                (*parent).left_child = ptr::null_mut();
            } else {
                (*parent).right_child = ptr::null_mut();
            }
            rebalance_start = parent;
        } else if left.is_null() {
            // Случай 2: Только правый ребенок
            if parent.is_null() {
                (*table).root = right;
            } else if (*parent).left_child == vad {
                (*parent).left_child = right;
            } else {
                (*parent).right_child = right;
            }
            (*right).parent = parent;
            rebalance_start = parent;
        } else if right.is_null() {
            // Случай 3: Только левый ребенок
            if parent.is_null() {
                (*table).root = left;
            } else if (*parent).left_child == vad {
                (*parent).left_child = left;
            } else {
                (*parent).right_child = left;
            }
            (*left).parent = parent;
            rebalance_start = parent;
        } else {
            // Случай 4: Два ребенка — находим in-order successor
            let successor = mi_find_minimum(right);
            let successor_parent = (*successor).parent;
            let successor_right = (*successor).right_child;

            // Отвязываем successor от его родителя
            if successor_parent == vad {
                // Successor - прямой правый ребенок удаляемого узла
                rebalance_start = successor;
            } else {
                // Successor глубже в дереве
                (*successor_parent).left_child = successor_right;
                if !successor_right.is_null() {
                    (*successor_right).parent = successor_parent;
                }
                // Подключаем правое поддерево удаляемого узла к successor
                (*successor).right_child = right;
                (*right).parent = successor;
                rebalance_start = successor_parent;
            }

            // Подключаем левое поддерево удаляемого узла к successor
            (*successor).left_child = left;
            (*left).parent = successor;

            // Заменяем удаляемый узел на successor
            (*successor).parent = parent;
            if parent.is_null() {
                (*table).root = successor;
            } else if (*parent).left_child == vad {
                (*parent).left_child = successor;
            } else {
                (*parent).right_child = successor;
            }

            // Копируем баланс (будет пересчитан)
            (*successor).balance = (*vad).balance;
        }

        (*table).node_count -= 1;

        // Очищаем связи удаленного узла
        (*vad).parent = ptr::null_mut();
        (*vad).left_child = ptr::null_mut();
        (*vad).right_child = ptr::null_mut();

        // AVL rebalancing
        if !rebalance_start.is_null() {
            mi_rebalance_tree(table, rebalance_start);
        }
    }
}

// =============================================================================
// VAD Split/Merge Operations
// =============================================================================

/// MiSplitVad
///
/// Разделяет VAD на два в указанной точке.
/// Используется при частичном decommit/release/protect.
///
/// # Arguments
/// * `table` - корень VAD дерева
/// * `vad` - VAD для разделения
/// * `split_vpn` - VPN точки разделения (первая страница нового VAD)
///
/// # Returns
/// Указатель на новый VAD (правая часть) или null при ошибке.
///
/// После операции:
/// - Оригинальный VAD содержит [starting_vpn, split_vpn - 1]
/// - Новый VAD содержит [split_vpn, ending_vpn]
pub unsafe fn mi_split_vad(
    table: *mut MM_AVL_TABLE,
    vad: *mut MMVAD_SHORT,
    split_vpn: u64,
) -> *mut MMVAD_SHORT {
    unsafe {
        if table.is_null() || vad.is_null() {
            return ptr::null_mut();
        }

        let original_start = (*vad).starting_vpn;
        let original_end = (*vad).ending_vpn;

        // Проверяем, что точка разделения внутри VAD
        if split_vpn <= original_start || split_vpn > original_end {
            return ptr::null_mut();
        }

        // Выделяем новый VAD для правой части
        let new_vad = mi_allocate_vad_short();
        if new_vad.is_null() {
            return ptr::null_mut();
        }

        // Настраиваем новый VAD (правая часть)
        (*new_vad).starting_vpn = split_vpn;
        (*new_vad).ending_vpn = original_end;
        (*new_vad).flags = (*vad).flags;
        (*new_vad).reference_count = 1;
        (*new_vad).push_lock = 0;

        // Обрезаем оригинальный VAD (левая часть)
        (*vad).ending_vpn = split_vpn - 1;

        // Вставляем новый VAD в дерево
        let status = mi_insert_vad(table, new_vad);
        if status != STATUS_SUCCESS {
            // Восстанавливаем оригинальный VAD
            (*vad).ending_vpn = original_end;
            mi_free_vad_short(new_vad);
            return ptr::null_mut();
        }

        new_vad
    }
}

/// MiMergeVad
///
/// Пытается объединить VAD с соседним VAD (если они смежные и совместимы).
///
/// # Arguments
/// * `table` - корень VAD дерева
/// * `vad` - VAD для объединения
///
/// # Returns
/// true если объединение произошло, false если не удалось
pub unsafe fn mi_try_merge_vad(table: *mut MM_AVL_TABLE, vad: *mut MMVAD_SHORT) -> bool {
    unsafe {
        if table.is_null() || vad.is_null() {
            return false;
        }

        // Пробуем найти и объединить со следующим VAD
        let next = mi_find_successor(vad);
        if !next.is_null() && mi_can_merge_vads(vad, next) {
            // Расширяем текущий VAD
            (*vad).ending_vpn = (*next).ending_vpn;
            // Удаляем следующий VAD
            mi_remove_vad(table, next);
            mi_free_vad_short(next);
            return true;
        }

        // Пробуем найти и объединить с предыдущим VAD
        let prev = mi_find_predecessor(vad);
        if !prev.is_null() && mi_can_merge_vads(prev, vad) {
            // Расширяем предыдущий VAD
            (*prev).ending_vpn = (*vad).ending_vpn;
            // Удаляем текущий VAD
            mi_remove_vad(table, vad);
            mi_free_vad_short(vad);
            return true;
        }

        false
    }
}

/// Проверяет, можно ли объединить два VAD
unsafe fn mi_can_merge_vads(left: *mut MMVAD_SHORT, right: *mut MMVAD_SHORT) -> bool {
    unsafe {
        if left.is_null() || right.is_null() {
            return false;
        }

        // Проверяем смежность (конец левого + 1 == начало правого)
        if (*left).ending_vpn + 1 != (*right).starting_vpn {
            return false;
        }

        // Проверяем совместимость флагов
        // Объединяем только VAD с одинаковыми типами и protection
        (*left).flags.vad_type() == (*right).flags.vad_type()
            && (*left).flags.protection() == (*right).flags.protection()
            && (*left).flags.commit() == (*right).flags.commit()
            && (*left).flags.private_memory() == (*right).flags.private_memory()
    }
}

/// Находит in-order successor (следующий по порядку VAD)
unsafe fn mi_find_successor(node: *mut MMVAD_SHORT) -> *mut MMVAD_SHORT {
    unsafe {
        if node.is_null() {
            return ptr::null_mut();
        }

        // Если есть правое поддерево, successor - минимум в нем
        if !(*node).right_child.is_null() {
            return mi_find_minimum((*node).right_child);
        }

        // Иначе идем вверх, пока не найдем родителя, для которого мы левый потомок
        let mut current = node;
        let mut parent = (*current).parent;

        while !parent.is_null() && current == (*parent).right_child {
            current = parent;
            parent = (*parent).parent;
        }

        parent
    }
}

/// Находит in-order predecessor (предыдущий по порядку VAD)
unsafe fn mi_find_predecessor(node: *mut MMVAD_SHORT) -> *mut MMVAD_SHORT {
    unsafe {
        if node.is_null() {
            return ptr::null_mut();
        }

        // Если есть левое поддерево, predecessor - максимум в нем
        if !(*node).left_child.is_null() {
            let mut pred = (*node).left_child;
            while !(*pred).right_child.is_null() {
                pred = (*pred).right_child;
            }
            return pred;
        }

        // Иначе идем вверх, пока не найдем родителя, для которого мы правый потомок
        let mut current = node;
        let mut parent = (*current).parent;

        while !parent.is_null() && current == (*parent).left_child {
            current = parent;
            parent = (*parent).parent;
        }

        parent
    }
}

/// MiFindVadForRange
///
/// Находит VAD, содержащий указанный диапазон адресов.
///
/// # Arguments
/// * `table` - корень VAD дерева
/// * `start_vpn` - начальный VPN диапазона
/// * `end_vpn` - конечный VPN диапазона (включительно)
///
/// # Returns
/// Указатель на VAD или null, если диапазон не полностью внутри одного VAD
pub unsafe fn mi_find_vad_for_range(
    table: *mut MM_AVL_TABLE,
    start_vpn: u64,
    end_vpn: u64,
) -> *mut MMVAD_SHORT {
    unsafe {
        if table.is_null() {
            return ptr::null_mut();
        }

        // Находим VAD по начальному адресу
        let start_addr = start_vpn << VPN_SHIFT;
        let vad = mi_find_vad(table, start_addr);

        if vad.is_null() {
            return ptr::null_mut();
        }

        // Проверяем, что весь диапазон внутри этого VAD
        if (*vad).starting_vpn <= start_vpn && (*vad).ending_vpn >= end_vpn {
            return vad;
        }

        ptr::null_mut()
    }
}

/// MiCheckVadConflict
///
/// Проверяет, конфликтует ли указанный диапазон с существующими VAD.
///
/// # Returns
/// true если есть конфликт, false если диапазон свободен
pub unsafe fn mi_check_vad_conflict(
    table: *mut MM_AVL_TABLE,
    start_vpn: u64,
    end_vpn: u64,
) -> bool {
    unsafe {
        if table.is_null() || (*table).is_empty() {
            return false;
        }

        // Проверяем начальную точку
        let start_addr = start_vpn << VPN_SHIFT;
        if !mi_find_vad(table, start_addr).is_null() {
            return true;
        }

        // Проверяем конечную точку
        let end_addr = end_vpn << VPN_SHIFT;
        if !mi_find_vad(table, end_addr).is_null() {
            return true;
        }

        // Проверяем, нет ли VAD внутри диапазона
        // Это нужно, если наш диапазон "охватывает" существующий VAD
        let mut current = (*table).root;
        let mut stack: [*mut MMVAD_SHORT; 64] = [ptr::null_mut(); 64];
        let mut stack_idx = 0;

        loop {
            while !current.is_null() {
                if stack_idx >= 64 {
                    return false; // Стек переполнен, считаем что конфликта нет
                }
                stack[stack_idx] = current;
                stack_idx += 1;
                current = (*current).left_child;
            }

            if stack_idx == 0 {
                break;
            }

            stack_idx -= 1;
            current = stack[stack_idx];

            // Проверяем, перекрывается ли текущий VAD с нашим диапазоном
            if (*current).starting_vpn <= end_vpn && (*current).ending_vpn >= start_vpn {
                return true;
            }

            // Оптимизация: если текущий VAD начинается после нашего конца, дальше искать не нужно
            if (*current).starting_vpn > end_vpn {
                break;
            }

            current = (*current).right_child;
        }

        false
    }
}

// =============================================================================
// Allocation Helpers
// =============================================================================

/// Выделяет MMVAD_SHORT из pool
pub unsafe fn mi_allocate_vad_short() -> *mut MMVAD_SHORT {
    unsafe {
        let ptr = crate::ex::pool::ex_allocate_pool_with_tag(
            crate::ex::pool::POOL_TYPE::NonPagedPool,
            core::mem::size_of::<MMVAD_SHORT>(),
            u32::from_le_bytes(*b"daVm"), // "mVad"
        ) as *mut MMVAD_SHORT;

        if !ptr.is_null() {
            core::ptr::write(ptr, MMVAD_SHORT::new());
        }

        ptr
    }
}

/// Освобождает MMVAD_SHORT
pub unsafe fn mi_free_vad_short(vad: *mut MMVAD_SHORT) {
    if !vad.is_null() {
        crate::ex::pool::ex_free_pool_with_tag(vad as PVOID, u32::from_le_bytes(*b"daVm"));
    }
}

/// Выделяет MMVAD из pool
pub unsafe fn mi_allocate_vad() -> *mut MMVAD {
    unsafe {
        let ptr = crate::ex::pool::ex_allocate_pool_with_tag(
            crate::ex::pool::POOL_TYPE::NonPagedPool,
            core::mem::size_of::<MMVAD>(),
            u32::from_le_bytes(*b"daVM"), // "MVad"
        ) as *mut MMVAD;

        if !ptr.is_null() {
            core::ptr::write(ptr, MMVAD::new());
        }

        ptr
    }
}

/// Освобождает MMVAD
pub unsafe fn mi_free_vad(vad: *mut MMVAD) {
    if !vad.is_null() {
        crate::ex::pool::ex_free_pool_with_tag(vad as PVOID, u32::from_le_bytes(*b"daVM"));
    }
}
