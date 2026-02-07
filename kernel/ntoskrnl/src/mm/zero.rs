//! Zero Page Thread
//!
//! MmZeroPageThread - системный поток, обнуляющий свободные страницы памяти.
//!
//! # Архитектура
//!
//! ```text
//! Phase1Initialization (ex/init.rs)
//!       │
//!       └── mm_zero_page_thread()
//!             │
//!             └── [бесконечный цикл]
//!                   ├── Ожидание (MmZeroingPageEvent или watermark)
//!                   └── Обнуление страниц из FreePageList -> ZeroedPageList
//! ```
//!
//! # Назначение
//!
//! В NT поток Phase 1 инициализации после завершения своей работы не уничтожается,
//! а переиспользуется как MmZeroPageThread. Этот поток работает на низком приоритете
//! и обнуляет свободные страницы памяти, чтобы они были готовы для быстрого выделения.
//!
//! # Политика
//!
//! - Поток работает когда FreePageList не пуст и ZeroedPageList меньше high watermark
//! - Засыпает когда FreePageList пуст или достаточно zeroed страниц
//! - Может быть разбужен при low zeroed watermark
//!
//! Источники:
//! - NT6.1: mm/zeropage.c (MmZeroPageThread)
//! - ReactOS: mm/ARM3/zeropage.c:145-200

use core::sync::atomic::AtomicBool;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;

use super::hyperspace::mi_zero_physical_page;
use super::pfn::mi_allocate_pfn_for_zero_page;
use super::pfn::mi_insert_zeroed_page;
use super::pfn::mm_get_free_page_count;
use super::pfn::mm_get_zeroed_page_count;
use super::pfn::MM_PFN_DATABASE;
use super::pfn::MMLISTS;

// =============================================================================
// Zero Page Thread Configuration
// =============================================================================

/// Минимальное количество zeroed страниц (low watermark)
/// Если меньше — zero thread должен работать активно
pub const MM_ZEROED_PAGE_LOW_WATERMARK: usize = 32;

/// Целевое количество zeroed страниц (high watermark)
/// Если достигнуто — zero thread может засыпать
pub const MM_ZEROED_PAGE_HIGH_WATERMARK: usize = 256;

/// Максимальное количество страниц для обнуления за одну итерацию
const ZERO_BATCH_SIZE: usize = 16;

/// Время ожидания между итерациями (в "тиках")
const ZERO_SLEEP_TICKS: usize = 100;

// =============================================================================
// Zero Page Thread State
// =============================================================================

/// Флаг активности zero page thread
static ZERO_THREAD_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Флаг запроса на пробуждение zero thread
static ZERO_THREAD_WAKEUP_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Счетчик обнуленных страниц (статистика)
pub static MM_ZEROED_PAGE_TOTAL: AtomicUsize = AtomicUsize::new(0);

// =============================================================================
// MmZeroPageThread
// =============================================================================

/// MmZeroPageThread
///
/// Системный поток обнуления свободных страниц.
/// Вызывается из Phase1Initialization после завершения инициализации.
/// Никогда не возвращается.
///
/// # Алгоритм
///
/// 1. Понижает приоритет до 0 (ниже всех пользовательских потоков)
/// 2. Входит в бесконечный цикл:
///    - Проверяет watermarks
///    - Если нужно: забирает страницы из FreePageList
///    - Обнуляет их через hyperspace
///    - Переносит в ZeroedPageList
///    - Если нечего делать: засыпает (HLT)
///
/// Источники:
/// - NT6.1: mm/zeropage.c (MmZeroPageThread)
/// - ReactOS: mm/ARM3/zeropage.c:145
pub unsafe fn mm_zero_page_thread() -> ! {
    unsafe {
        crate::ke::debug::debug_raw("[MM] MmZeroPageThread started\n");
        ZERO_THREAD_ACTIVE.store(true, Ordering::Release);

        // Понижаем приоритет потока до 0 (lowest)
        // В NT это делается через KeSetPriorityThread(KeGetCurrentThread(), 0);
        let current_thread = crate::ps::process::ps_get_current_thread();
        crate::ke::debug::debug_raw("[MM] ZeroPageThread: current_thread=");
        crate::ke::debug::debug_hex(current_thread as u64);
        crate::ke::debug::debug_raw("\n");

        if !current_thread.is_null() {
            let kthread = current_thread as *mut crate::ke::thread::KTHREAD;
            let old_prio = (*kthread).priority;
            let old_base = (*kthread).base_priority;
            crate::ke::debug::debug_raw("[MM] ZeroPageThread: old priority=");
            crate::ke::debug::debug_dec(old_prio as u64);
            crate::ke::debug::debug_raw(" base=");
            crate::ke::debug::debug_dec(old_base as u64);
            crate::ke::debug::debug_raw(", setting to 0\n");

            // Для zero page thread нужно установить BasePriority = 0
            // Это позволит KeSetPriorityThread установить Priority = 0
            // (иначе нормализация изменит 0 на 1 если BasePriority != 0)
            (*kthread).base_priority = 0;

            let result = crate::ke::priority::ke_set_priority_thread(kthread, 0);

            crate::ke::debug::debug_raw("[MM] ZeroPageThread: ke_set_priority_thread returned ");
            crate::ke::debug::debug_dec(result as u64);
            crate::ke::debug::debug_raw(", new priority=");
            crate::ke::debug::debug_dec((*kthread).priority as u64);
            crate::ke::debug::debug_raw(" base=");
            crate::ke::debug::debug_dec((*kthread).base_priority as u64);
            crate::ke::debug::debug_raw("\n");
        }

        crate::ke::debug::debug_raw("[MM] ZeroPageThread: entering main loop\n");

        // Основной цикл обнуления
        loop {
            // Проверяем, нужно ли работать
            let zeroed_count = mm_get_zeroed_page_count();
            let free_count = mm_get_free_page_count();

            // Если zeroed достаточно и нет срочного запроса — засыпаем
            if zeroed_count >= MM_ZEROED_PAGE_HIGH_WATERMARK
                && !ZERO_THREAD_WAKEUP_REQUESTED.load(Ordering::Acquire)
            {
                // Достаточно zeroed страниц — можно поспать
                mi_zero_thread_sleep();
                continue;
            }

            // Если free list пуст — ничего делать
            if free_count == 0 {
                // Нечего обнулять — спим
                mi_zero_thread_sleep();
                continue;
            }

            // Сбрасываем флаг запроса
            ZERO_THREAD_WAKEUP_REQUESTED.store(false, Ordering::Release);

            // Обнуляем batch страниц
            let pages_to_zero = free_count.min(ZERO_BATCH_SIZE);
            let zeroed = mi_zero_page_batch(pages_to_zero);

            // Обновляем статистику
            MM_ZEROED_PAGE_TOTAL.fetch_add(zeroed, Ordering::Relaxed);

            // Если обнулили мало страниц — даём другим поработать
            if zeroed < ZERO_BATCH_SIZE / 2 {
                mi_zero_thread_yield();
            }
        }
    }
}

/// Обнуляет batch страниц из free list
///
/// # Returns
/// Количество реально обнуленных страниц
unsafe fn mi_zero_page_batch(max_pages: usize) -> usize {
    unsafe {
        let mut zeroed = 0;

        for _ in 0..max_pages {
            // Пытаемся получить страницу из free list
            let pfn = mi_allocate_pfn_for_zero_page();
            if pfn.is_none() {
                // Free list пуст
                break;
            }
            let pfn = pfn.unwrap();

            // Обнуляем страницу через hyperspace
            mi_zero_physical_page(pfn);

            // Обновляем PFN entry
            let db = &mut *MM_PFN_DATABASE.get();
            if let Some(entry) = db.get_mut(pfn) {
                entry.page_location = MMLISTS::ZeroedPageList as u8;
            }

            // Добавляем в zeroed list
            mi_insert_zeroed_page(pfn);

            zeroed += 1;
        }

        zeroed
    }
}

/// Zero thread sleep — ожидание работы
///
/// В NT используется KeWaitForSingleObject на MmZeroingPageEvent.
/// Пока используем простой HLT с периодическим пробуждением.
#[inline]
fn mi_zero_thread_sleep() {
    // Простая реализация: HLT несколько раз
    // В реальной NT: KeWaitForSingleObject(MmZeroingPageEvent, ...)
    for _ in 0..ZERO_SLEEP_TICKS {
        unsafe {
            core::arch::asm!("hlt", options(nomem, nostack, preserves_flags));
        }
    }
}

/// Zero thread yield — уступаем CPU другим потокам
#[inline]
fn mi_zero_thread_yield() {
    // Одиночный HLT
    unsafe {
        core::arch::asm!("hlt", options(nomem, nostack, preserves_flags));
    }
}

// =============================================================================
// Zero Thread Control API
// =============================================================================

/// Запрашивает пробуждение zero page thread
///
/// Вызывается когда zeroed страниц мало и нужно срочно обнулить.
#[inline]
pub fn mm_request_zero_pages() {
    ZERO_THREAD_WAKEUP_REQUESTED.store(true, Ordering::Release);
}

/// Проверяет, активен ли zero page thread
#[inline]
pub fn mm_is_zero_thread_active() -> bool {
    ZERO_THREAD_ACTIVE.load(Ordering::Acquire)
}

/// Возвращает общее количество обнуленных страниц (статистика)
#[inline]
pub fn mm_get_total_zeroed_pages() -> usize {
    MM_ZEROED_PAGE_TOTAL.load(Ordering::Relaxed)
}

// =============================================================================
// Integration with PFN Allocator
// =============================================================================

/// Вызывается из mi_allocate_pfn когда zeroed list пуст
///
/// Может пробудить zero thread для срочного обнуления.
pub fn mi_signal_zeroed_pages_low() {
    let zeroed = mm_get_zeroed_page_count();
    if zeroed < MM_ZEROED_PAGE_LOW_WATERMARK {
        mm_request_zero_pages();
    }
}
