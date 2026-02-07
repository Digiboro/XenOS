//! System PTE Allocator
//!
//! Управление системными PTE для временных маппингов.
//! Реализация соответствует Windows NT 6.1 (Windows 7).
//!
//! # Назначение
//!
//! System PTE используются для:
//! - `MmMapLockedPages` — отображение MDL в system VA
//! - `MmMapIoSpace` — отображение MMIO регионов
//! - Временные маппинги внутри ядра
//!
//! # Архитектура
//!
//! Свободные PTE организованы в singly linked list кластеров,
//! отсортированный по возрастанию размера.
//!
//! ## Формат MMPTE_LIST (amd64):
//!
//! ```text
//! [63:32] = NextEntry (индекс следующего кластера, 32 бита)
//! [11]    = Transition
//! [10]    = Prototype  
//! [9:5]   = Protection
//! [4:2]   = filler
//! [1]     = OneEntry (1 = одиночный PTE)
//! [0]     = Valid (всегда 0)
//! ```
//!
//! ## Структура кластера:
//!
//! - **Одиночный (size=1)**: `PTE[0].OneEntry=1`, `PTE[0].NextEntry=next`
//! - **Multi-PTE (size>=2)**: `PTE[0].OneEntry=0`, `PTE[0].NextEntry=next`, `PTE[1].NextEntry=size`
//!
//! # Алгоритмы (NT-style)
//!
//! - **MiReserveSystemPtes**: обход списка, поиск кластера >= count, разбиение с конца
//! - **MiReleaseSystemPtes**: обход списка, coalescing смежных, вставка в sorted position
//!
//! # Сложность
//!
//! - Выделение: O(n) — first-fit в sorted list
//! - Освобождение: O(n) — coalescing + sorted insert

use core::cmp::Ordering as CmpOrdering;
use core::sync::atomic::AtomicBool;
use core::sync::atomic::AtomicU32;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;

use super::pfn::SyncUnsafeCell;
use super::pte::mm_get_pte_address;
use super::pte::PTE_NX;
use super::pte::PTE_READWRITE;
use super::pte::PTE_VALID;
use super::types::MM_SYSTEM_PTE_END;
use super::types::MM_SYSTEM_PTE_START;
use super::types::PAGE_SIZE;
use super::types::PFN_NUMBER;
use crate::ke::spinlock::KSpinLockGuard;
use crate::ke::spinlock::KSPIN_LOCK;

// =============================================================================
// Constants
// =============================================================================

/// Максимальное количество system PTE entries
const MAX_SYSTEM_PTES: usize = 0x100000; // 1M entries = 4GB VA

/// Минимальный размер резервирования (в PTE)
const MIN_RESERVE_SIZE: usize = 1;

/// Минимальный размер блока для AVL дерева (нужно 4 PTE для метаданных)
/// PTE[0]: header, PTE[1]: left/right, PTE[2]: parent/height, PTE[N-1]: footer
const AVL_MIN_BLOCK_SIZE: usize = 4;

/// Пустой список (конец linked list)
/// В NT это MM_EMPTY_PTE_LIST
const MM_EMPTY_LIST: u32 = 0xFFFF_FFFF;

// =============================================================================
// NT-style Free PTE Cluster Format
// =============================================================================
//
// Формат MMPTE_LIST для свободных кластеров:
//
// ```
// [63:32] = NextEntry (32 бита — индекс следующего кластера)
// [31:12] = reserved
// [11]    = Transition (не используется для free)
// [10]    = Prototype (не используется для free)
// [9:5]   = Protection (не используется для free)
// [4:2]   = filler
// [1]     = OneEntry (1 = одиночный PTE, 0 = multi-PTE кластер)
// [0]     = Valid (всегда 0 — not present)
// ```
//
// Для одиночного PTE (size=1):
//   PTE[0]: OneEntry=1, NextEntry=next_cluster_index
//
// Для multi-PTE кластера (size>=2):
//   PTE[0]: OneEntry=0, NextEntry=next_cluster_index
//   PTE[1]: NextEntry=cluster_size (32 бита — без ограничений!)
//
// Преимущества NT-подхода:
// - Размер кластера до 2^32 (4 миллиарда страниц)
// - NextEntry до 2^32 (4 миллиарда индексов)
// - Простой формат без сложного bit-packing

const ONE_ENTRY_BIT: u64 = 1 << 1;  // бит [1]
const NEXT_ENTRY_SHIFT: u32 = 32;   // NextEntry в [63:32]
const NEXT_ENTRY_MASK: u64 = 0xFFFF_FFFF_0000_0000;

/// Кодирует PTE для одиночного свободного entry (size=1)
#[inline]
fn encode_single_pte(next: u32) -> u64 {
    // Valid=0, OneEntry=1, NextEntry в верхних 32 битах
    ONE_ENTRY_BIT | ((next as u64) << NEXT_ENTRY_SHIFT)
}

/// Кодирует первый PTE multi-entry кластера
#[inline]
fn encode_cluster_head_pte(next: u32) -> u64 {
    // Valid=0, OneEntry=0, NextEntry в верхних 32 битах
    (next as u64) << NEXT_ENTRY_SHIFT
}

/// Кодирует второй PTE кластера (хранит размер)
#[inline]
fn encode_cluster_size_pte(size: u32) -> u64 {
    // Размер хранится в NextEntry поле (верхние 32 бита)
    (size as u64) << NEXT_ENTRY_SHIFT
}

/// Проверяет, является ли PTE свободным (Valid=0, не нулевой)
#[inline]
fn is_free_pte(pte: u64) -> bool {
    // Valid=0 и есть какие-то данные (NextEntry или OneEntry)
    (pte & 1) == 0 && pte != 0
}

/// Проверяет, является ли PTE одиночным entry (OneEntry=1)
#[inline]
fn is_one_entry(pte: u64) -> bool {
    (pte & ONE_ENTRY_BIT) != 0
}

/// Декодирует NextEntry из PTE
#[inline]
fn decode_next_entry(pte: u64) -> u32 {
    (pte >> NEXT_ENTRY_SHIFT) as u32
}

/// Устанавливает NextEntry в PTE (не меняя остальные биты)
#[inline]
fn set_next_entry(pte: u64, next: u32) -> u64 {
    (pte & !NEXT_ENTRY_MASK) | ((next as u64) << NEXT_ENTRY_SHIFT)
}

// =============================================================================
// AVL Node Metadata (PTE[1], PTE[2], PTE[3] для кластеров >= 4 PTE)
// =============================================================================
//
// Для AVL кластеров (size >= 4):
//   PTE[0]: cluster head (OneEntry=0, NextEntry=next in AVL? нет, это для list)
//   PTE[1]: size (в NextEntry поле)
//   PTE[2]: AVL left/right
//   PTE[3]: AVL parent/height
//
// Но AVL и linked-list — разные структуры. Для AVL нам нужно:
//   PTE[0]: marker что это AVL блок
//   PTE[1]: size
//   PTE[2]: left | right
//   PTE[3]: parent | height

/// Кодирует AVL children PTE
/// Format: [63:32] = left_idx, [31:0] = right_idx
#[inline]
fn encode_avl_children_pte(left: u32, right: u32) -> u64 {
    ((left as u64) << 32) | (right as u64)
}

#[inline]
fn decode_avl_left(pte: u64) -> u32 {
    (pte >> 32) as u32
}

#[inline]
fn decode_avl_right(pte: u64) -> u32 {
    pte as u32
}

/// Кодирует AVL parent/height PTE
/// Format: [63:32] = parent_idx, [31:0] = height
#[inline]
fn encode_avl_parent_pte(parent: u32, height: u32) -> u64 {
    ((parent as u64) << 32) | (height as u64)
}

#[inline]
fn decode_avl_parent(pte: u64) -> u32 {
    (pte >> 32) as u32
}

#[inline]
fn decode_avl_height(pte: u64) -> u32 {
    pte as u32
}

// =============================================================================
// Global State
// =============================================================================

/// System PTE allocator инициализирован
static SYSTEM_PTES_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Базовый VA региона system PTE
static SYSTEM_PTE_BASE_VA: AtomicUsize = AtomicUsize::new(0);

/// Базовый адрес массива PTE (указатель на PTE[0])
static SYSTEM_PTE_ARRAY_BASE: AtomicUsize = AtomicUsize::new(0);

/// Количество всего system PTE entries
static SYSTEM_PTE_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Количество свободных system PTE entries
static SYSTEM_PTE_FREE_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Следующий свободный индекс (bump allocator)
static SYSTEM_PTE_NEXT_FREE: AtomicUsize = AtomicUsize::new(0);

/// Голова единого free list (NT-style, отсортирован по размеру)
static FREE_LIST_HEAD: AtomicU32 = AtomicU32::new(MM_EMPTY_LIST);

// Legacy: для совместимости с AVL кодом (TODO: удалить)
static AVL_ROOT: AtomicU32 = AtomicU32::new(MM_EMPTY_LIST);
static SMALL_LIST_HEAD: AtomicU32 = AtomicU32::new(MM_EMPTY_LIST);

/// Спинлок для синхронизации
static SYSTEM_PTE_LOCK: SyncUnsafeCell<KSPIN_LOCK> = SyncUnsafeCell::new(KSPIN_LOCK::new());

// =============================================================================
// PTE Array Access
// =============================================================================

/// Получает указатель на PTE по индексу
#[inline]
unsafe fn get_pte_ptr(index: u32) -> *mut u64 {
    let base = SYSTEM_PTE_ARRAY_BASE.load(Ordering::Acquire);
    (base as *mut u64).add(index as usize)
}

/// Читает PTE по индексу
#[inline]
unsafe fn read_pte(index: u32) -> u64 {
    unsafe { core::ptr::read_volatile(get_pte_ptr(index)) }
}

/// Записывает PTE по индексу
#[inline]
unsafe fn write_pte(index: u32, value: u64) {
    unsafe { core::ptr::write_volatile(get_pte_ptr(index), value) }
}

/// Преобразует индекс PTE в виртуальный адрес
#[inline]
fn index_to_va(index: u32) -> u64 {
    let base = SYSTEM_PTE_BASE_VA.load(Ordering::Acquire);
    base as u64 + (index as u64) * (PAGE_SIZE as u64)
}

/// Преобразует виртуальный адрес в индекс PTE
#[inline]
fn va_to_index(va: u64) -> u32 {
    let base = SYSTEM_PTE_BASE_VA.load(Ordering::Acquire) as u64;
    ((va - base) / (PAGE_SIZE as u64)) as u32
}

// =============================================================================
// NT-style Block Metadata Access
// =============================================================================

/// Получает размер кластера (NT-style: MI_GET_CLUSTER_SIZE)
///
/// - size=1: OneEntry=1, размер = 1
/// - size>=2: OneEntry=0, размер в PTE[1].NextEntry
#[inline]
unsafe fn get_cluster_size(index: u32) -> u32 {
    unsafe {
        let pte0 = read_pte(index);
        if is_one_entry(pte0) {
            1
        } else {
            // Размер в PTE[1]
            decode_next_entry(read_pte(index + 1))
        }
    }
}

/// Проверяет, является ли PTE свободным кластером
#[inline]
unsafe fn is_block_free(index: u32) -> bool {
    unsafe { is_free_pte(read_pte(index)) }
}

/// Читает NextEntry (следующий кластер в linked list)
#[inline]
unsafe fn block_next(index: u32) -> u32 {
    unsafe { decode_next_entry(read_pte(index)) }
}

/// Устанавливает NextEntry
#[inline]
unsafe fn set_block_next(index: u32, next: u32) {
    unsafe {
        let pte = read_pte(index);
        write_pte(index, set_next_entry(pte, next));
    }
}

/// Инициализирует свободный кластер (NT-style)
///
/// - size=1: PTE[0] с OneEntry=1
/// - size>=2: PTE[0] с OneEntry=0, PTE[1] с размером
#[inline]
unsafe fn init_free_cluster(index: u32, size: u32, next: u32) {
    unsafe {
        if size == 1 {
            write_pte(index, encode_single_pte(next));
        } else {
            write_pte(index, encode_cluster_head_pte(next));
            write_pte(index + 1, encode_cluster_size_pte(size));
        }
    }
}

/// Очищает все метаданные кластера (при выделении)
#[inline]
unsafe fn clear_cluster_metadata(index: u32, size: u32) {
    unsafe {
        write_pte(index, 0);
        if size > 1 {
            write_pte(index + 1, 0);
        }
        // Очищаем AVL metadata если блок большой
        if size >= AVL_MIN_BLOCK_SIZE as u32 {
            write_pte(index + 2, 0);
            write_pte(index + 3, 0);
        }
    }
}

// Алиас для совместимости с AVL кодом
#[inline]
unsafe fn block_size(index: u32) -> u32 {
    unsafe { get_cluster_size(index) }
}

// =============================================================================
// AVL Tree Operations (метаданные в PTE[2] и PTE[3])
// =============================================================================
//
// AVL layout для кластеров >= 4 PTE:
//   PTE[0]: cluster head (OneEntry=0)
//   PTE[1]: size
//   PTE[2]: left | right
//   PTE[3]: parent | height

/// Читает AVL left child index (из PTE[2])
#[inline]
unsafe fn avl_left(index: u32) -> u32 {
    unsafe { decode_avl_left(read_pte(index + 2)) }
}

/// Читает AVL right child index (из PTE[2])
#[inline]
unsafe fn avl_right(index: u32) -> u32 {
    unsafe { decode_avl_right(read_pte(index + 2)) }
}

/// Читает AVL parent index (из PTE[3])
#[inline]
unsafe fn avl_parent(index: u32) -> u32 {
    unsafe { decode_avl_parent(read_pte(index + 3)) }
}

/// Читает AVL height (из PTE[3])
#[inline]
unsafe fn avl_height(index: u32) -> i32 {
    if index == MM_EMPTY_LIST {
        return 0;
    }
    unsafe { decode_avl_height(read_pte(index + 3)) as i32 }
}

/// Записывает AVL children (left, right) в PTE[2]
#[inline]
unsafe fn write_avl_children(index: u32, left: u32, right: u32) {
    unsafe { write_pte(index + 2, encode_avl_children_pte(left, right)) }
}

/// Записывает AVL parent и height в PTE[3]
#[inline]
unsafe fn write_avl_parent_height(index: u32, parent: u32, height: u32) {
    unsafe { write_pte(index + 3, encode_avl_parent_pte(parent, height)) }
}

/// Устанавливает AVL left child
#[inline]
unsafe fn set_avl_left(index: u32, left: u32) {
    unsafe {
        let right = avl_right(index);
        write_avl_children(index, left, right);
    }
}

/// Устанавливает AVL right child
#[inline]
unsafe fn set_avl_right(index: u32, right: u32) {
    unsafe {
        let left = avl_left(index);
        write_avl_children(index, left, right);
    }
}

/// Устанавливает AVL parent
#[inline]
unsafe fn set_avl_parent(index: u32, parent: u32) {
    unsafe {
        let height = avl_height(index) as u32;
        write_avl_parent_height(index, parent, height);
    }
}

/// Устанавливает AVL height
#[inline]
unsafe fn set_avl_height(index: u32, height: i32) {
    unsafe {
        let parent = avl_parent(index);
        write_avl_parent_height(index, parent, height as u32);
    }
}

/// Инициализирует AVL node для нового кластера (NT-style)
///
/// Layout:
///   PTE[0]: cluster head (OneEntry=0, NextEntry=0 для AVL)
///   PTE[1]: size
///   PTE[2]: left | right
///   PTE[3]: parent | height
#[inline]
unsafe fn init_avl_node(index: u32, size: u32) {
    unsafe {
        // PTE[0]: cluster head
        write_pte(index, encode_cluster_head_pte(0)); // NextEntry=0 для AVL (не linked list)
        // PTE[1]: size
        write_pte(index + 1, encode_cluster_size_pte(size));
        // PTE[2]: left | right
        write_avl_children(index, MM_EMPTY_LIST, MM_EMPTY_LIST);
        // PTE[3]: parent | height
        write_avl_parent_height(index, MM_EMPTY_LIST, 1);
    }
}

/// Balance factor узла
#[inline]
unsafe fn avl_balance_factor(index: u32) -> i32 {
    if index == MM_EMPTY_LIST {
        return 0;
    }
    unsafe { avl_height(avl_left(index)) - avl_height(avl_right(index)) }
}

/// Обновляет высоту узла
#[inline]
unsafe fn avl_update_height(index: u32) {
    if index == MM_EMPTY_LIST {
        return;
    }
    unsafe {
        let left_h = avl_height(avl_left(index));
        let right_h = avl_height(avl_right(index));
        set_avl_height(index, 1 + left_h.max(right_h));
    }
}

/// Сравнивает два блока по (size, index)
#[inline]
unsafe fn compare_blocks(a: u32, b: u32) -> CmpOrdering {
    unsafe {
        let a_size = block_size(a);
        let b_size = block_size(b);
        match a_size.cmp(&b_size) {
            CmpOrdering::Equal => a.cmp(&b),
            other => other,
        }
    }
}

/// Правый поворот AVL
unsafe fn avl_rotate_right(y: u32) -> u32 {
    unsafe {
        let x = avl_left(y);
        let b = avl_right(x);
        let y_parent = avl_parent(y);

        // x.right = y
        set_avl_right(x, y);
        set_avl_parent(x, y_parent);

        // y.left = b
        set_avl_left(y, b);
        set_avl_parent(y, x);

        if b != MM_EMPTY_LIST {
            set_avl_parent(b, y);
        }

        // Обновляем parent->child
        if y_parent != MM_EMPTY_LIST {
            if avl_left(y_parent) == y {
                set_avl_left(y_parent, x);
            } else {
                set_avl_right(y_parent, x);
            }
        }

        avl_update_height(y);
        avl_update_height(x);

        x
    }
}

/// Левый поворот AVL
unsafe fn avl_rotate_left(x: u32) -> u32 {
    unsafe {
        let y = avl_right(x);
        let b = avl_left(y);
        let x_parent = avl_parent(x);

        // y.left = x
        set_avl_left(y, x);
        set_avl_parent(y, x_parent);

        // x.right = b
        set_avl_right(x, b);
        set_avl_parent(x, y);

        if b != MM_EMPTY_LIST {
            set_avl_parent(b, x);
        }

        // Обновляем parent->child
        if x_parent != MM_EMPTY_LIST {
            if avl_left(x_parent) == x {
                set_avl_left(x_parent, y);
            } else {
                set_avl_right(x_parent, y);
            }
        }

        avl_update_height(x);
        avl_update_height(y);

        y
    }
}

/// Балансирует узел после вставки/удаления
unsafe fn avl_balance(index: u32) -> u32 {
    unsafe {
        avl_update_height(index);
        let balance = avl_balance_factor(index);

        // Left heavy
        if balance > 1 {
            let left = avl_left(index);
            if avl_balance_factor(left) < 0 {
                // Left-Right case
                let new_left = avl_rotate_left(left);
                set_avl_left(index, new_left);
            }
            return avl_rotate_right(index);
        }

        // Right heavy
        if balance < -1 {
            let right = avl_right(index);
            if avl_balance_factor(right) > 0 {
                // Right-Left case
                let new_right = avl_rotate_right(right);
                set_avl_right(index, new_right);
            }
            return avl_rotate_left(index);
        }

        index
    }
}

/// Вставляет узел в AVL дерево
unsafe fn avl_insert(root: u32, new_idx: u32) -> u32 {
    unsafe {
        if root == MM_EMPTY_LIST {
            return new_idx;
        }

        match compare_blocks(new_idx, root) {
            CmpOrdering::Less | CmpOrdering::Equal => {
                let new_left = avl_insert(avl_left(root), new_idx);
                set_avl_left(root, new_left);
                set_avl_parent(new_left, root);
            }
            CmpOrdering::Greater => {
                let new_right = avl_insert(avl_right(root), new_idx);
                set_avl_right(root, new_right);
                set_avl_parent(new_right, root);
            }
        }

        avl_balance(root)
    }
}

/// Находит минимальный узел в поддереве
unsafe fn avl_find_min(index: u32) -> u32 {
    unsafe {
        let mut current = index;
        while current != MM_EMPTY_LIST {
            let left = avl_left(current);
            if left == MM_EMPTY_LIST {
                return current;
            }
            current = left;
        }
        MM_EMPTY_LIST
    }
}

/// Transplant: заменяет узел u на v
unsafe fn avl_transplant(root: &mut u32, u: u32, v: u32) {
    unsafe {
        let u_parent = avl_parent(u);

        if u_parent == MM_EMPTY_LIST {
            *root = v;
        } else {
            if avl_left(u_parent) == u {
                set_avl_left(u_parent, v);
            } else {
                set_avl_right(u_parent, v);
            }
        }

        if v != MM_EMPTY_LIST {
            set_avl_parent(v, u_parent);
        }
    }
}

/// Удаляет узел из AVL дерева
unsafe fn avl_remove(root: u32, target: u32) -> u32 {
    unsafe {
        if root == MM_EMPTY_LIST {
            return MM_EMPTY_LIST;
        }

        // Найти узел
        let z = avl_find_exact(root, target);
        if z == MM_EMPTY_LIST {
            return root;
        }

        let z_parent = avl_parent(z);
        let z_left = avl_left(z);
        let z_right = avl_right(z);

        let mut new_root = root;
        let rebalance_start: u32;

        if z_left == MM_EMPTY_LIST {
            avl_transplant(&mut new_root, z, z_right);
            rebalance_start = z_parent;
        } else if z_right == MM_EMPTY_LIST {
            avl_transplant(&mut new_root, z, z_left);
            rebalance_start = z_parent;
        } else {
            // Два ребёнка — находим successor
            let y = avl_find_min(z_right);
            let y_parent = avl_parent(y);
            let y_right = avl_right(y);

            rebalance_start = if y_parent == z { y } else { y_parent };

            if y_parent != z {
                avl_transplant(&mut new_root, y, y_right);
                set_avl_right(y, z_right);
                set_avl_parent(z_right, y);
            }

            avl_transplant(&mut new_root, z, y);
            set_avl_left(y, z_left);
            set_avl_parent(z_left, y);
            avl_update_height(y);
        }

        // Ребалансировка вверх по parent
        let mut n = rebalance_start;
        while n != MM_EMPTY_LIST {
            let parent = avl_parent(n);
            let balanced = avl_balance(n);

            if balanced != n && parent != MM_EMPTY_LIST {
                if avl_left(parent) == n {
                    set_avl_left(parent, balanced);
                } else if avl_right(parent) == n {
                    set_avl_right(parent, balanced);
                }
            }

            if parent == MM_EMPTY_LIST {
                new_root = balanced;
            }

            n = parent;
        }

        if new_root != MM_EMPTY_LIST {
            set_avl_parent(new_root, MM_EMPTY_LIST);
        }

        new_root
    }
}

/// Best-fit поиск
unsafe fn avl_find_best_fit(root: u32, count: u32) -> u32 {
    unsafe {
        let mut best = MM_EMPTY_LIST;
        let mut best_size = u32::MAX;
        let mut current = root;

        while current != MM_EMPTY_LIST {
            let size = block_size(current);

            match size.cmp(&count) {
                CmpOrdering::Equal => return current,
                CmpOrdering::Greater => {
                    if size < best_size {
                        best = current;
                        best_size = size;
                    }
                    current = avl_left(current);
                }
                CmpOrdering::Less => {
                    current = avl_right(current);
                }
            }
        }

        best
    }
}

/// Поиск точного узла по индексу
unsafe fn avl_find_exact(root: u32, target: u32) -> u32 {
    unsafe {
        let mut current = root;

        while current != MM_EMPTY_LIST {
            match compare_blocks(target, current) {
                CmpOrdering::Equal => return current,
                CmpOrdering::Less => current = avl_left(current),
                CmpOrdering::Greater => current = avl_right(current),
            }
        }

        MM_EMPTY_LIST
    }
}

// =============================================================================
// Small Block List (для блоков < AVL_MIN_BLOCK_SIZE)
// =============================================================================
//
// Small blocks используют тот же unified tag формат:
// - PTE[0]: header tag с next_idx в поле данных
// - PTE[last]: footer tag (для size >= 2)
// - Для size=1: header tag служит и footer'ом по позиции

/// Инициализирует small block (NT-style cluster)
#[inline]
unsafe fn init_small_block(index: u32, size: u32, next: u32) {
    unsafe {
        init_free_cluster(index, size, next);
    }
}

/// Читает next из small block (из header tag)
#[inline]
unsafe fn small_next(index: u32) -> u32 {
    unsafe { block_next(index) }
}

/// Устанавливает next для small block
#[inline]
unsafe fn set_small_next(index: u32, next: u32) {
    unsafe { set_block_next(index, next) }
}

// =============================================================================
// NT-style Coalescing (MiReleaseSystemPtes)
// =============================================================================
//
// Coalescing происходит при освобождении путём обхода linked list.
// Если найден кластер смежный с освобождаемым, они объединяются.
// Это O(n), но надёжно и соответствует NT.

/// Освобождает PTEs с coalescing (NT-style MiReleaseSystemPtes)
///
/// Алгоритм:
/// 1. Проходим весь linked list
/// 2. Если кластер смежен с освобождаемым — объединяем
/// 3. Вставляем результат в отсортированную позицию
unsafe fn release_ptes_with_coalescing(mut start_index: u32, mut count: u32) {
    unsafe {
        let mut prev_pte_idx = MM_EMPTY_LIST; // Индекс "предыдущего" элемента в списке
        let mut insert_after_idx = MM_EMPTY_LIST; // Где вставить новый кластер
        
        // Голова списка
        let mut current_idx = FREE_LIST_HEAD.load(Ordering::Acquire);
        
        while current_idx != MM_EMPTY_LIST {
            let cluster_size = get_cluster_size(current_idx);
            let cluster_end = current_idx + cluster_size;
            let release_end = start_index + count;
            
            // Проверяем смежность
            if cluster_end == start_index || release_end == current_idx {
                // Смежные кластеры — объединяем
                count += cluster_size;
                if current_idx < start_index {
                    start_index = current_idx;
                }
                
                // Удаляем текущий кластер из списка
                let next_idx = block_next(current_idx);
                if prev_pte_idx == MM_EMPTY_LIST {
                    FREE_LIST_HEAD.store(next_idx, Ordering::Release);
                } else {
                    set_block_next(prev_pte_idx, next_idx);
                }
                
                // Обнуляем старый кластер
                write_pte(current_idx, 0);
                if cluster_size > 1 {
                    write_pte(current_idx + 1, 0);
                }
                
                // Сбрасываем insert position (пересчитаем)
                insert_after_idx = MM_EMPTY_LIST;
                
                // Продолжаем с того же prev (current удалён)
                current_idx = if prev_pte_idx == MM_EMPTY_LIST {
                    FREE_LIST_HEAD.load(Ordering::Acquire)
                } else {
                    block_next(prev_pte_idx)
                };
            } else {
                // Запоминаем позицию для вставки (отсортировано по размеру)
                if insert_after_idx == MM_EMPTY_LIST && count <= cluster_size {
                    insert_after_idx = prev_pte_idx;
                }
                
                prev_pte_idx = current_idx;
                current_idx = block_next(current_idx);
            }
        }
        
        // Если не нашли позицию — вставляем в конец
        if insert_after_idx == MM_EMPTY_LIST {
            insert_after_idx = prev_pte_idx;
        }
        
        // Создаём новый кластер
        let next_cluster = if insert_after_idx == MM_EMPTY_LIST {
            FREE_LIST_HEAD.load(Ordering::Acquire)
        } else {
            block_next(insert_after_idx)
        };
        
        init_free_cluster(start_index, count, next_cluster);
        
        // Вставляем в список
        if insert_after_idx == MM_EMPTY_LIST {
            FREE_LIST_HEAD.store(start_index, Ordering::Release);
        } else {
            set_block_next(insert_after_idx, start_index);
        }
    }
}

// =============================================================================
// Public API
// =============================================================================

/// Инициализирует System PTE allocator (NT-style MiInitializeSystemPtes)
///
/// Создаёт один большой кластер из всего региона.
pub unsafe fn mi_initialize_system_ptes(base_va: u64, count: usize) {
    if SYSTEM_PTES_INITIALIZED.load(Ordering::Acquire) {
        return;
    }

    let actual_count = count.min(MAX_SYSTEM_PTES);

    // Вычисляем адрес массива PTE
    let pte_array_base = mm_get_pte_address(base_va);

    SYSTEM_PTE_BASE_VA.store(base_va as usize, Ordering::Release);
    SYSTEM_PTE_ARRAY_BASE.store(pte_array_base as usize, Ordering::Release);
    SYSTEM_PTE_COUNT.store(actual_count, Ordering::Release);
    SYSTEM_PTE_FREE_COUNT.store(actual_count, Ordering::Release);

    // NT-style: обнуляем все PTE
    unsafe {
        for i in 0..actual_count {
            write_pte(i as u32, 0);
        }
    }

    // NT-style: создаём один кластер из всего региона
    // PTE[0]: NextEntry = MM_EMPTY_LIST (конец списка), OneEntry = 0
    // PTE[1]: NextEntry = size
    unsafe {
        write_pte(0, encode_cluster_head_pte(MM_EMPTY_LIST));
        write_pte(1, encode_cluster_size_pte(actual_count as u32));
    }

    // Голова списка указывает на первый кластер (индекс 0)
    FREE_LIST_HEAD.store(0, Ordering::Release);

    // Legacy (не используется)
    SYSTEM_PTE_NEXT_FREE.store(actual_count, Ordering::Release); // Bump allocator исчерпан
    AVL_ROOT.store(MM_EMPTY_LIST, Ordering::Release);
    SMALL_LIST_HEAD.store(MM_EMPTY_LIST, Ordering::Release);

    SYSTEM_PTES_INITIALIZED.store(true, Ordering::Release);
}

/// Инициализирует System PTE с параметрами по умолчанию
pub unsafe fn mi_initialize_system_ptes_default() {
    unsafe {
        let base = MM_SYSTEM_PTE_START;
        let size = MM_SYSTEM_PTE_END - MM_SYSTEM_PTE_START;
        let count = (size as usize) / PAGE_SIZE;

        mi_initialize_system_ptes(base, count.min(MAX_SYSTEM_PTES));
    }
}

/// Резервирует system PTE (NT-style MiReserveSystemPtes)
///
/// # Returns
/// Виртуальный адрес или 0 при ошибке
pub unsafe fn mi_reserve_system_ptes(count: usize) -> u64 {
    unsafe {
        if !SYSTEM_PTES_INITIALIZED.load(Ordering::Acquire) {
            return 0;
        }

        if count == 0 || count < MIN_RESERVE_SIZE {
            return 0;
        }

        let _lock = KSpinLockGuard::new(SYSTEM_PTE_LOCK.get());

        // NT-style: только free list (весь регион изначально там)
        if let Some(index) = allocate_from_free_list(count as u32) {
            SYSTEM_PTE_FREE_COUNT.fetch_sub(count, Ordering::AcqRel);
            return index_to_va(index);
        }

        // Нет свободных кластеров достаточного размера
        0
    }
}

/// Освобождает system PTE
pub unsafe fn mi_release_system_ptes(base: u64, count: usize) {
    unsafe {
        if !SYSTEM_PTES_INITIALIZED.load(Ordering::Acquire) {
            return;
        }

        if count == 0 || base == 0 {
            return;
        }

        let _lock = KSpinLockGuard::new(SYSTEM_PTE_LOCK.get());

        let index = va_to_index(base);

        // Очищаем PTEs (делаем invalid)
        for i in 0..count {
            write_pte(index + i as u32, 0);
            crate::arch::x86_64::cpu::invlpg(index_to_va(index + i as u32));
        }

        // Добавляем в free list с coalescing (NT-style)
        release_ptes_with_coalescing(index, count as u32);

        SYSTEM_PTE_FREE_COUNT.fetch_add(count, Ordering::AcqRel);
    }
}

/// Выделяет блок из free list (NT-style MiReserveSystemPtes)
///
/// Ищет первый кластер с достаточным количеством PTE.
/// Список отсортирован по размеру, так что это first-fit в отсортированном списке.
unsafe fn allocate_from_free_list(count: u32) -> Option<u32> {
    unsafe {
        let mut prev_idx = MM_EMPTY_LIST;
        let mut current_idx = FREE_LIST_HEAD.load(Ordering::Acquire);
        
        // Ищем кластер с достаточным размером
        while current_idx != MM_EMPTY_LIST {
            let cluster_size = get_cluster_size(current_idx);
            
            if cluster_size >= count {
                // Нашли подходящий кластер
                let next_idx = block_next(current_idx);
                
                // Удаляем из списка
                if prev_idx == MM_EMPTY_LIST {
                    FREE_LIST_HEAD.store(next_idx, Ordering::Release);
                } else {
                    set_block_next(prev_idx, next_idx);
                }
                
                // Разбиваем если нужно
                if cluster_size > count {
                    // Берём PTE с конца кластера
                    let return_idx = current_idx + cluster_size - count;
                    let remaining = cluster_size - count;
                    
                    // Перестраиваем кластер с уменьшенным размером
                    if remaining == 1 {
                        // Одиночный PTE
                        write_pte(current_idx, encode_single_pte(MM_EMPTY_LIST));
                    } else {
                        // Multi-PTE: обновляем только size
                        write_pte(current_idx + 1, encode_cluster_size_pte(remaining));
                    }
                    
                    // Вставляем обратно в отсортированную позицию
                    insert_cluster_sorted(current_idx, remaining);
                    
                    // Обнуляем выделяемые PTE
                    for i in 0..count {
                        write_pte(return_idx + i, 0);
                    }
                    
                    return Some(return_idx);
                } else {
                    // Весь кластер
                    // Обнуляем
                    write_pte(current_idx, 0);
                    if cluster_size > 1 {
                        write_pte(current_idx + 1, 0);
                    }
                    
                    return Some(current_idx);
                }
            }
            
            prev_idx = current_idx;
            current_idx = block_next(current_idx);
        }
        
        None
    }
}

/// Вставляет кластер в отсортированную позицию (по размеру)
unsafe fn insert_cluster_sorted(index: u32, size: u32) {
    unsafe {
        let mut prev_idx = MM_EMPTY_LIST;
        let mut current_idx = FREE_LIST_HEAD.load(Ordering::Acquire);
        
        // Ищем позицию (список отсортирован по возрастанию размера)
        while current_idx != MM_EMPTY_LIST {
            let current_size = get_cluster_size(current_idx);
            if size <= current_size {
                break;
            }
            prev_idx = current_idx;
            current_idx = block_next(current_idx);
        }
        
        // Устанавливаем next для нового кластера
        set_block_next(index, current_idx);
        
        // Вставляем
        if prev_idx == MM_EMPTY_LIST {
            FREE_LIST_HEAD.store(index, Ordering::Release);
        } else {
            set_block_next(prev_idx, index);
        }
    }
}

// =============================================================================
// Statistics
// =============================================================================

/// Возвращает статистику System PTE
pub fn mi_get_system_pte_stats() -> SystemPteStats {
    SystemPteStats {
        total: SYSTEM_PTE_COUNT.load(Ordering::Acquire),
        free: SYSTEM_PTE_FREE_COUNT.load(Ordering::Acquire),
        next_free: SYSTEM_PTE_NEXT_FREE.load(Ordering::Acquire),
        initialized: SYSTEM_PTES_INITIALIZED.load(Ordering::Acquire),
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SystemPteStats {
    pub total: usize,
    pub free: usize,
    pub next_free: usize,
    pub initialized: bool,
}

// =============================================================================
// Debug / Verification
// =============================================================================

/// Подсчитывает блоки в AVL дереве
pub unsafe fn mi_count_avl_blocks() -> usize {
    unsafe {
        let root = AVL_ROOT.load(Ordering::Acquire);
        count_avl_nodes(root)
    }
}

unsafe fn count_avl_nodes(index: u32) -> usize {
    if index == MM_EMPTY_LIST {
        return 0;
    }
    unsafe { 1 + count_avl_nodes(avl_left(index)) + count_avl_nodes(avl_right(index)) }
}

/// Подсчитывает блоки в small list
pub unsafe fn mi_count_small_blocks() -> usize {
    unsafe {
        let mut count = 0;
        let mut current = SMALL_LIST_HEAD.load(Ordering::Acquire);

        while current != MM_EMPTY_LIST {
            count += 1;
            current = small_next(current);

            // Защита от бесконечного цикла
            if count > MAX_SYSTEM_PTES {
                break;
            }
        }

        count
    }
}

/// Верифицирует целостность AVL дерева
pub unsafe fn mi_verify_avl_tree() -> bool {
    unsafe {
        let root = AVL_ROOT.load(Ordering::Acquire);
        if root == MM_EMPTY_LIST {
            return true;
        }
        verify_avl_node(root, MM_EMPTY_LIST)
    }
}

unsafe fn verify_avl_node(index: u32, expected_parent: u32) -> bool {
    if index == MM_EMPTY_LIST {
        return true;
    }

    unsafe {
        // Проверяем parent
        let parent = avl_parent(index);
        if parent != expected_parent {
            return false;
        }

        // Проверяем balance
        let balance = avl_balance_factor(index);
        if balance < -1 || balance > 1 {
            return false;
        }

        // Рекурсивно проверяем детей
        verify_avl_node(avl_left(index), index) && verify_avl_node(avl_right(index), index)
    }
}

// =============================================================================
// PTE Mapping API
// =============================================================================

/// Проверяет, инициализирован ли System PTE allocator
#[inline]
pub fn mi_system_ptes_initialized() -> bool {
    SYSTEM_PTES_INITIALIZED.load(Ordering::Acquire)
}

/// Резервирует contiguous system PTE с выравниванием
/// 
/// # Arguments
/// * `count` - количество PTE
/// * `alignment` - выравнивание в страницах (должно быть степенью 2)
///
/// # Returns
/// VA или 0 при ошибке
pub unsafe fn mi_reserve_system_ptes_contiguous(count: usize, alignment: usize) -> u64 {
    unsafe {
        // Простая реализация — пробуем обычный reserve, если не выровнен — берём больше
        if alignment <= 1 {
            return mi_reserve_system_ptes(count);
        }

        // Для выравнивания резервируем с запасом
        let extra = alignment - 1;
        let total = count + extra;

        let va = mi_reserve_system_ptes(total);
        if va == 0 {
            return 0;
        }

        // Выравниваем VA
        let alignment_bytes = alignment * PAGE_SIZE;
        let aligned_va = (va + alignment_bytes as u64 - 1) & !(alignment_bytes as u64 - 1);

        // Возвращаем лишнее в начале
        let skip_pages = ((aligned_va - va) / PAGE_SIZE as u64) as usize;
        if skip_pages > 0 {
            mi_release_system_ptes(va, skip_pages);
        }

        // Возвращаем лишнее в конце
        let end_pages = extra - skip_pages;
        if end_pages > 0 {
            let end_va = aligned_va + (count * PAGE_SIZE) as u64;
            mi_release_system_ptes(end_va, end_pages);
        }

        aligned_va
    }
}

/// Маппит одну system PTE на физическую страницу
///
/// # Arguments
/// * `va` - виртуальный адрес (должен быть в system PTE region)
/// * `pfn` - физический номер страницы
/// * `writable` - разрешить запись
pub unsafe fn mi_map_system_pte(va: u64, pfn: PFN_NUMBER, writable: bool) {
    unsafe {
        let pte_ptr = mm_get_pte_address(va) as *mut u64;

        let mut pte_value = PTE_VALID | PTE_NX | ((pfn as u64) << 12);
        if writable {
            pte_value |= PTE_READWRITE;
        }

        core::ptr::write_volatile(pte_ptr, pte_value);
    }
}

/// Маппит несколько system PTE на последовательные физические страницы
///
/// # Arguments
/// * `va` - начальный виртуальный адрес
/// * `pfns` - массив PFN
/// * `writable` - разрешить запись
pub unsafe fn mi_map_system_ptes(va: u64, pfns: &[PFN_NUMBER], writable: bool) {
    unsafe {
        for (i, &pfn) in pfns.iter().enumerate() {
            let page_va = va + (i * PAGE_SIZE) as u64;
            mi_map_system_pte(page_va, pfn, writable);
        }
    }
}

/// Отменяет маппинг system PTE (делает invalid)
pub unsafe fn mi_unmap_system_pte(va: u64) {
    unsafe {
        let pte_ptr = mm_get_pte_address(va) as *mut u64;
        core::ptr::write_volatile(pte_ptr, 0);
        crate::arch::x86_64::cpu::invlpg(va);
    }
}

/// Отменяет маппинг нескольких system PTE
pub unsafe fn mi_unmap_system_ptes(va: u64, count: usize) {
    unsafe {
        for i in 0..count {
            let page_va = va + (i * PAGE_SIZE) as u64;
            mi_unmap_system_pte(page_va);
        }
    }
}
