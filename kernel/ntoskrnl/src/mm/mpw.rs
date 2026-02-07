//! Modified Page Writer
//!
//! MiModifiedPageWriter — системный поток, записывающий dirty страницы на диск.
//!
//! # Архитектура
//!
//! ```text
//! Phase1Initialization
//!       |
//!       +-> MiModifiedPageWriter thread
//!             |
//!             +-> [бесконечный цикл]
//!                   +-- Ожидание (event или threshold)
//!                   +-- Сбор dirty страниц из ModifiedPageList
//!                   +-- Запись на диск/pagefile
//!                   +-- Перенос в StandbyPageList
//! ```
//!
//! # Назначение
//!
//! Когда страницы памяти модифицируются и затем вытесняются из working set,
//! они попадают в ModifiedPageList. Modified Page Writer периодически
//! записывает эти страницы в соответствующие backing store:
//!
//! - Private memory → pagefile
//! - File-backed memory → исходный файл
//! - Shared memory → pagefile или mapped file
//!
//! После успешной записи страница переходит в StandbyPageList
//! и может быть переиспользована.
//!
//! # Интеграция
//!
//! - MM_MODIFIED_PAGE_LIST_HEAD — очередь dirty страниц
//! - Pagefile I/O — TODO: интеграция с IO manager
//! - Section objects — для file-backed pages
//!
//! # Текущее состояние
//!
//! Базовая реализация без реального I/O (страницы просто переходят в standby).
//! Для полной реализации требуется:
//! - Pagefile support
//! - I/O manager integration
//! - Section/Segment tracking
//!
//! Источники:
//! - NT6.1: mm/modwrite.c (MiModifiedPageWriter)
//! - ReactOS: mm/ARM3/mdlsup.c, mm/ARM3/section.c

use core::sync::atomic::AtomicBool;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;

use super::pfn::mi_complete_modified_page_write;
use super::pfn::mi_get_modified_page_for_write;
use super::pfn::mm_get_modified_page_count;
use super::pfn::mm_should_write_modified_pages;
use super::pfn::MM_MODIFIED_PAGE_WRITER_THRESHOLD;
use super::pfn::MM_PFN_LOCK;
use super::types::PFN_NUMBER;
use crate::ke::spinlock::KSpinLockGuard;

// =============================================================================
// Modified Page Writer Configuration
// =============================================================================

/// Максимальное количество страниц для записи за один batch
const MPW_BATCH_SIZE: usize = 16;

/// Интервал ожидания между итерациями (в "тиках")
const MPW_SLEEP_TICKS: usize = 500;

/// Критический порог для агрессивной записи
const MPW_CRITICAL_THRESHOLD: usize = 256;

// =============================================================================
// Modified Page Writer State
// =============================================================================

/// Флаг активности MPW thread
static MPW_THREAD_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Флаг запроса на пробуждение MPW
static MPW_WAKEUP_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Счетчик записанных страниц (статистика)
pub static MM_PAGES_WRITTEN: AtomicUsize = AtomicUsize::new(0);

/// Счетчик I/O операций (статистика)
pub static MM_MPW_IO_COUNT: AtomicUsize = AtomicUsize::new(0);

// =============================================================================
// MiModifiedPageWriter
// =============================================================================

/// MiModifiedPageWriter
///
/// Системный поток записи модифицированных страниц.
/// Запускается при Phase1Initialization.
/// Никогда не возвращается.
///
/// # Алгоритм
///
/// 1. Ожидает события или порогового количества modified страниц
/// 2. Собирает batch страниц из ModifiedPageList
/// 3. Записывает их в backing store (pagefile/file)
/// 4. Переводит успешно записанные в StandbyPageList
/// 5. Повторяет
///
/// # Текущие ограничения
///
/// - Нет реального I/O (страницы просто помечаются как clean)
/// - Нет pagefile support
/// - Нет file-backed page writeback
///
/// Источники:
/// - NT6.1: mm/modwrite.c
/// - ReactOS: mm/ARM3/mdlsup.c
pub unsafe fn mi_modified_page_writer() -> ! {
    unsafe {
        MPW_THREAD_ACTIVE.store(true, Ordering::Release);

        // Устанавливаем приоритет потока (выше zero thread, но ниже обычных)
        // В NT MPW работает на приоритете ~8-14
        let current_thread = crate::ps::process::ps_get_current_thread();
        if !current_thread.is_null() {
            let kthread = current_thread as *mut crate::ke::thread::KTHREAD;
            crate::ke::priority::ke_set_priority_thread(kthread, 8);
        }

        loop {
            // Проверяем, нужно ли работать
            let modified_count = mm_get_modified_page_count();

            // Если страниц мало и нет срочного запроса — засыпаем
            if modified_count < MM_MODIFIED_PAGE_WRITER_THRESHOLD
                && !MPW_WAKEUP_REQUESTED.load(Ordering::Acquire)
            {
                mi_mpw_sleep();
                continue;
            }

            // Сбрасываем флаг запроса
            MPW_WAKEUP_REQUESTED.store(false, Ordering::Release);

            // Определяем размер batch
            let batch_size = if modified_count >= MPW_CRITICAL_THRESHOLD {
                // Критический уровень — пишем больше
                MPW_BATCH_SIZE * 2
            } else {
                MPW_BATCH_SIZE
            };

            // Обрабатываем batch
            let written = mi_write_modified_pages_batch(batch_size);

            // Обновляем статистику
            MM_PAGES_WRITTEN.fetch_add(written, Ordering::Relaxed);
            if written > 0 {
                MM_MPW_IO_COUNT.fetch_add(1, Ordering::Relaxed);
            }

            // Если записали мало — даём другим поработать
            if written < batch_size / 2 {
                mi_mpw_yield();
            }
        }
    }
}

/// Обрабатывает batch модифицированных страниц
///
/// # Returns
/// Количество обработанных страниц
unsafe fn mi_write_modified_pages_batch(max_pages: usize) -> usize {
    unsafe {
        let mut written = 0;

        for _ in 0..max_pages {
            // Получаем страницу для записи (с PFN lock)
            let page_info = {
                let _lock = KSpinLockGuard::new(MM_PFN_LOCK.get());
                mi_get_modified_page_for_write()
            };

            let Some((pfn, original_pte, pte_address)) = page_info else {
                // Больше нет modified страниц
                break;
            };

            // Определяем backing store и записываем
            let success = mi_write_page_to_backing_store(pfn, original_pte, pte_address);

            // Завершаем операцию (с PFN lock)
            {
                let _lock = KSpinLockGuard::new(MM_PFN_LOCK.get());
                mi_complete_modified_page_write(pfn, success);
            }

            if success {
                written += 1;
            }
        }

        written
    }
}

/// Записывает страницу в backing store
///
/// # Returns
/// true если запись успешна
///
/// # Текущая реализация
/// Заглушка — всегда возвращает true без реального I/O.
/// Для полной реализации требуется:
/// - Определить тип backing store по original_pte
/// - Для pagefile: записать в pagefile
/// - Для file-backed: записать в файл через IO manager
fn mi_write_page_to_backing_store(
    _pfn: PFN_NUMBER,
    original_pte: u64,
    _pte_address: *mut u64,
) -> bool {
    // Анализируем original_pte для определения backing store
    //
    // В NT 6.1 original_pte содержит:
    // - Для private memory: pagefile offset/index
    // - Для file-backed: prototype PTE pointer
    // - Для demand-zero: 0 или demand-zero marker
    //
    // Биты original_pte:
    // - Bits 0: Valid (always 0 for stored pte)
    // - Bits 1-4: Protection
    // - Bits 5-9: PageFile index (для pagefile)
    // - Bits 12+: PageFile offset (для pagefile)
    // ИЛИ
    // - Bits 1: Prototype flag
    // - Bits 8+: Prototype PTE address

    let is_prototype = (original_pte & 0x2) != 0;

    if is_prototype {
        // File-backed or shared memory
        // TODO: Запись через IO manager в mapped file
        //
        // let proto_pte_addr = (original_pte >> 8) << 3;
        // Нужно:
        // 1. Найти CONTROL_AREA по proto_pte
        // 2. Определить file object
        // 3. Вычислить offset в файле
        // 4. Создать IRP для записи
        // 5. Вызвать IoCallDriver

        // Пока: помечаем как успешно записанную (flush to disk при close)
        true
    } else if original_pte != 0 {
        // Pagefile-backed
        // TODO: Запись в pagefile
        //
        // let pagefile_index = ((original_pte >> 5) & 0xF) as usize;
        // let pagefile_offset = original_pte >> 12;
        // Нужно:
        // 1. Получить pagefile object по индексу
        // 2. Создать IRP для записи
        // 3. Записать страницу по offset

        // Пока: помечаем как успешно записанную
        true
    } else {
        // Demand-zero или уже чистая страница
        // Ничего писать не нужно
        true
    }
}

/// MPW sleep — ожидание работы
#[inline]
fn mi_mpw_sleep() {
    // В NT: KeWaitForSingleObject(MmModifiedPageWriterEvent, ...)
    // Пока: HLT loop
    for _ in 0..MPW_SLEEP_TICKS {
        unsafe {
            core::arch::asm!("hlt", options(nomem, nostack, preserves_flags));
        }
    }
}

/// MPW yield — уступаем CPU
#[inline]
fn mi_mpw_yield() {
    unsafe {
        core::arch::asm!("hlt", options(nomem, nostack, preserves_flags));
    }
}

// =============================================================================
// Modified Page Writer Control API
// =============================================================================

/// Запрашивает пробуждение Modified Page Writer
///
/// Вызывается когда modified страниц много и нужно срочно освободить память.
#[inline]
pub fn mm_request_page_write() {
    MPW_WAKEUP_REQUESTED.store(true, Ordering::Release);
}

/// Проверяет, активен ли Modified Page Writer
#[inline]
pub fn mm_is_mpw_active() -> bool {
    MPW_THREAD_ACTIVE.load(Ordering::Acquire)
}

/// Возвращает общее количество записанных страниц (статистика)
#[inline]
pub fn mm_get_pages_written() -> usize {
    MM_PAGES_WRITTEN.load(Ordering::Relaxed)
}

/// Возвращает количество I/O операций MPW (статистика)
#[inline]
pub fn mm_get_mpw_io_count() -> usize {
    MM_MPW_IO_COUNT.load(Ordering::Relaxed)
}

// =============================================================================
// Integration with Memory Pressure
// =============================================================================

/// Вызывается при нехватке памяти для ускорения освобождения
pub fn mi_signal_memory_pressure() {
    let modified_count = mm_get_modified_page_count();

    // Если есть modified страницы — будим MPW
    if modified_count > 0 {
        mm_request_page_write();
    }

    // Также можно сигнализировать zero thread если zeroed мало
    super::zero::mi_signal_zeroed_pages_low();
}

/// Flush всех modified страниц (для shutdown/sync)
///
/// Блокирующая операция — ждёт завершения записи всех страниц.
///
/// # Safety
/// Должен вызываться на PASSIVE_LEVEL.
pub unsafe fn mm_flush_all_modified_pages() {
    unsafe {
        // Записываем все modified страницы
        loop {
            let count = mm_get_modified_page_count();
            if count == 0 {
                break;
            }

            // Записываем batch
            let written = mi_write_modified_pages_batch(count.min(MPW_BATCH_SIZE * 4));
            if written == 0 {
                // Не удалось записать — выходим чтобы избежать infinite loop
                break;
            }

            // Небольшая пауза между batches
            mi_mpw_yield();
        }
    }
}

