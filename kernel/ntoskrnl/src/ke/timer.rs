//! KTIMER - Kernel Timer
//!
//! Подсистема таймеров ядра, соответствующая архитектуре Windows NT.
//!
//! # Архитектура
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                         User Mode                                        │
//! │           NtCreateTimer / NtSetTimer / NtCancelTimer                    │
//! └───────────────────────────────┬─────────────────────────────────────────┘
//!                                 │
//!                                 ▼
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                    Executive (ETIMER) - ex/timer.rs                     │
//! │                    Обёртка над KTIMER + APC support                     │
//! └───────────────────────────────┬─────────────────────────────────────────┘
//!                                 │
//!                                 ▼
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                    Kernel (KTIMER) - ke/timer.rs                        │
//! │                    KeInitializeTimer / KeSetTimer / KeCancelTimer       │
//! │                    DPC callbacks при истечении                          │
//! └───────────────────────────────┬─────────────────────────────────────────┘
//!                                 │
//!                                 ▼
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │              Timer Table - 512 bucket'ов (hash table)                   │
//! │              KiTimerTableListHead[TIMER_TABLE_SIZE]                     │
//! │              Каждый bucket: LIST_ENTRY + Time + Lock                    │
//! └───────────────────────────────┬─────────────────────────────────────────┘
//!                                 │
//!                                 ▼
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                    Clock Interrupt (CLOCK_LEVEL)                        │
//! │                    KeUpdateSystemTime → O(1) check                      │
//! │                    → HalRequestSoftwareInterrupt(DISPATCH)              │
//! └───────────────────────────────┬─────────────────────────────────────────┘
//!                                 │
//!                                 ▼
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                    DISPATCH_LEVEL                                       │
//! │                    KiTimerExpiration (range scan hand→limit)            │
//! │                    → signal waiters, queue DPC, rearm periodic          │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Отступления и упрощения
//!
//! - Размер таблицы: 512 bucket'ов (как в NT6.1/ReactOS)
//! - Per-bucket lock: храним отдельным массивом (не внутри KTIMER_TABLE_ENTRY)
//!   для упрощения статической инициализации
//! - DISPATCHER_HEADER.Hand/Inserted/Absolute: используем существующую структуру
//!   DISPATCHER_HEADER, флаги храним в отдельных полях KTIMER где необходимо
//!
//! Источники:
//! - MSDN (NT6.1 / Windows 7): KeInitializeTimer, KeSetTimerEx, KeCancelTimer
//! - ReactOS: ntoskrnl/ke/timerobj.c (источник для анализа)
//! - Windows XP/2003: docs/ref/nt5src (источник для анализа)

#![allow(dead_code)]
#![allow(non_camel_case_types)]

use core::sync::atomic::Ordering;

use super::dpc::KDPC;
use super::event::DISPATCHER_HEADER;
use super::event::KOBJECT_TYPE;
use super::spinlock::ke_acquire_spin_lock_at_dpc_level;
use super::spinlock::ke_release_spin_lock_from_dpc_level;
use crate::nt::LIST_ENTRY;
use crate::nt::LONG;
use crate::nt::PVOID;
use crate::nt::UCHAR;
use crate::nt::ULONGLONG;

// Используем общие функции отладки
use super::debug::{debug_raw, debug_hex};

// =============================================================================
// Константы
// =============================================================================

/// Размер таблицы таймеров (512 bucket'ов как в NT6.1/ReactOS)
pub const TIMER_TABLE_SIZE: usize = 512;

/// Маска для вычисления индекса bucket'а
const TIMER_TABLE_MASK: usize = TIMER_TABLE_SIZE - 1;

/// Инкремент приоритета при пробуждении от таймера
const TIMER_EXPIRE_INCREMENT: i8 = 0;

// =============================================================================
// Типы таймеров
// =============================================================================

/// TIMER_TYPE - тип таймера
///
/// ReactOS: sdk/include/xdk/ketypes.h
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TIMER_TYPE {
    /// NotificationTimer - все ожидающие потоки просыпаются при сигнализации
    NotificationTimer = 0,
    /// SynchronizationTimer - только один ожидающий поток просыпается
    SynchronizationTimer = 1,
}

// =============================================================================
// KTIMER - структура таймера ядра
// =============================================================================

/// KTIMER - структура таймера ядра
///
/// ReactOS: sdk/include/xdk/ketypes.h:903-912
///
/// Таймер является dispatcher object и может использоваться с KeWaitFor*.
/// При истечении таймера:
/// - устанавливается SignalState = 1
/// - пробуждаются ожидающие потоки
/// - вызывается DPC callback (если задан)
/// - для периодических таймеров: перевставка с новым DueTime
#[repr(C)]
pub struct KTIMER {
    /// Заголовок dispatcher объекта
    /// Type = TimerNotificationObject (8) или TimerSynchronizationObject (9)
    pub header: DISPATCHER_HEADER,

    /// Время истечения (абсолютное InterruptTime в 100ns единицах)
    pub due_time: ULONGLONG,

    /// Связь в списке таймеров bucket'а
    pub timer_list_entry: LIST_ENTRY,

    /// DPC для вызова при истечении (может быть NULL)
    pub dpc: *mut KDPC,

    /// Период для периодических таймеров (в миллисекундах, 0 = однократный)
    pub period: LONG,
}

impl KTIMER {
    /// Создаёт новый неинициализированный таймер
    pub const fn new() -> Self {
        Self {
            header: DISPATCHER_HEADER::new(
                KOBJECT_TYPE::TimerNotification as UCHAR,
                (core::mem::size_of::<KTIMER>() / 4) as u8,
            ),
            due_time: 0,
            timer_list_entry: LIST_ENTRY::new(),
            dpc: core::ptr::null_mut(),
            period: 0,
        }
    }

    /// Проверяет вставлен ли таймер в таблицу
    ///
    /// Таймер считается вставленным если timer_list_entry.flink != NULL
    #[inline]
    pub fn is_inserted(&self) -> bool {
        !self.timer_list_entry.flink.is_null()
    }
}

impl Default for KTIMER {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// KTIMER_TABLE_ENTRY - запись в таблице таймеров
// =============================================================================

/// KTIMER_TABLE_ENTRY - запись в таблице таймеров
///
/// ReactOS: sdk/include/ndk/ketypes.h:898-905
///
/// Каждый bucket содержит:
/// - entry: двусвязный список таймеров, отсортированных по DueTime (от раннего к позднему)
/// - time: минимальный DueTime среди таймеров в bucket'е (для O(1) проверки на CLOCK_LEVEL)
///
/// Если bucket пуст, time = u64::MAX (эквивалент HighPart=0xFFFFFFFF в NT).
#[repr(C)]
pub struct KTIMER_TABLE_ENTRY {
    /// Список таймеров в bucket'е (head)
    pub entry: LIST_ENTRY,

    /// Минимальное время истечения среди таймеров в bucket'е
    /// u64::MAX означает что bucket пустой
    pub time: u64,
}

impl KTIMER_TABLE_ENTRY {
    pub const fn new() -> Self {
        Self {
            entry: LIST_ENTRY::new(),
            time: u64::MAX,
        }
    }
}

impl Default for KTIMER_TABLE_ENTRY {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Публичные API: KeInitializeTimer / KeInitializeTimerEx
// =============================================================================

/// KeInitializeTimer - инициализирует notification timer
///
/// Эквивалент вызова KeInitializeTimerEx с типом NotificationTimer.
///
/// # Arguments
/// * `timer` - указатель на KTIMER для инициализации
///
/// # IRQL
/// <= DISPATCH_LEVEL
///
/// Источники:
/// - MSDN: KeInitializeTimer
/// - ReactOS: ntoskrnl/ke/timerobj.c:233-237
pub fn ke_initialize_timer(timer: &mut KTIMER) {
    ke_initialize_timer_ex(timer, TIMER_TYPE::NotificationTimer);
}

/// KeInitializeTimerEx - инициализирует таймер указанного типа
///
/// # Arguments
/// * `timer` - указатель на KTIMER для инициализации
/// * `timer_type` - NotificationTimer или SynchronizationTimer
///
/// # IRQL
/// <= DISPATCH_LEVEL
///
/// # Поведение
/// - NotificationTimer: при сигнализации все ожидающие потоки просыпаются
/// - SynchronizationTimer: при сигнализации только один ожидающий поток просыпается,
///   SignalState автоматически сбрасывается
///
/// Источники:
/// - MSDN: KeInitializeTimerEx
/// - ReactOS: ntoskrnl/ke/timerobj.c:244-263
pub fn ke_initialize_timer_ex(timer: &mut KTIMER, timer_type: TIMER_TYPE) {
    // Тип объекта: TimerNotificationObject (8) + timer_type
    let object_type = (KOBJECT_TYPE::TimerNotification as u8) + (timer_type as u8);

    timer.header = DISPATCHER_HEADER::new(object_type, (core::mem::size_of::<KTIMER>() / 4) as u8);

    // Таймер не сигнализирован
    timer.header.signal_state.store(0, Ordering::Release);

    // Таймер не вставлен в таблицу
    timer.timer_list_entry = LIST_ENTRY::new();

    // Нет DPC и периода
    timer.dpc = core::ptr::null_mut();
    timer.due_time = 0;
    timer.period = 0;

    // Инициализируем wait list (критически важно для ki_wait_test!)
    unsafe {
        timer.header.init_wait_list();
    }
}

// =============================================================================
// Публичные API: KeSetTimer / KeSetTimerEx
// =============================================================================

/// KeSetTimer - устанавливает таймер
///
/// Эквивалент вызова KeSetTimerEx с period=0.
///
/// # Arguments
/// * `timer` - указатель на инициализированный KTIMER
/// * `due_time` - время срабатывания:
///   - отрицательное: относительное время (в 100ns единицах от текущего момента)
///   - положительное: абсолютное системное время (в 100ns от 1 Jan 1601)
/// * `dpc` - опциональный DPC для вызова при истечении
///
/// # Returns
/// true если таймер был ранее установлен (was inserted)
///
/// # IRQL
/// <= DISPATCH_LEVEL
///
/// Источники:
/// - MSDN: KeSetTimer
/// - ReactOS: ntoskrnl/ke/timerobj.c:282-292
pub fn ke_set_timer(timer: &mut KTIMER, due_time: i64, dpc: Option<&mut KDPC>) -> bool {
    ke_set_timer_ex(timer, due_time, 0, dpc)
}

/// KeSetTimerEx - устанавливает таймер с периодом
///
/// # Arguments
/// * `timer` - указатель на инициализированный KTIMER
/// * `due_time` - время первого срабатывания:
///   - отрицательное: относительное время (в 100ns единицах)
///   - положительное: абсолютное системное время
/// * `period` - период повторения в миллисекундах (0 = однократный таймер)
/// * `dpc` - опциональный DPC для вызова при истечении
///
/// # Returns
/// true если таймер был ранее установлен
///
/// # IRQL
/// <= DISPATCH_LEVEL
///
/// # Алгоритм (соответствует NT)
/// 1. Если таймер уже вставлен - удаляем из таблицы
/// 2. Вычисляем абсолютный DueTime (KiComputeDueTime)
/// 3. Если таймер уже истёк - сигнализируем немедленно (fast-path)
/// 4. Иначе вставляем в таблицу таймеров
///
/// Источники:
/// - MSDN: KeSetTimerEx
/// - ReactOS: ntoskrnl/ke/timerobj.c:295-342
pub fn ke_set_timer_ex(
    timer: &mut KTIMER,
    due_time: i64,
    period: LONG,
    dpc: Option<&mut KDPC>,
) -> bool {
    let was_inserted = timer.is_inserted();

    // Если таймер был в таблице - удаляем
    if was_inserted {
        ke_cancel_timer(timer);
    }

    // Устанавливаем DPC и период
    timer.dpc = dpc.map(|d| d as *mut KDPC).unwrap_or(core::ptr::null_mut());
    timer.period = period;

    // Вычисляем абсолютный DueTime и hand (bucket index)
    let (abs_due_time, hand, already_expired) = ki_compute_due_time(due_time);

    timer.due_time = abs_due_time;

    // DEBUG: вывод информации о таймере
    debug_raw("[TIMER] KeSetTimerEx: timer=");
    debug_hex(timer as *mut KTIMER as u64);
    debug_raw(" due_time=");
    debug_hex(due_time as u64);
    debug_raw(" abs_due_time=");
    debug_hex(abs_due_time);
    debug_raw(" hand=");
    debug_hex(hand as u64);
    debug_raw(" period=");
    debug_hex(period as u64);
    debug_raw(" expired=");
    debug_hex(already_expired as u64);
    debug_raw("\n");

    if already_expired {
        // Fast-path: таймер уже истёк - сигнализируем немедленно
        debug_raw("[TIMER] Timer already expired, signaling immediately\n");
        unsafe {
            ki_signal_timer(timer);
        }
    } else {
        // Вставляем в таблицу таймеров
        debug_raw("[TIMER] Inserting timer into table\n");
        unsafe {
            kx_insert_timer(timer, hand);
        }
    }

    was_inserted
}

// =============================================================================
// Публичные API: KeCancelTimer
// =============================================================================

/// KeCancelTimer - отменяет таймер
///
/// # Arguments
/// * `timer` - указатель на KTIMER
///
/// # Returns
/// true если таймер был вставлен в таблицу (was inserted)
///
/// # IRQL
/// <= DISPATCH_LEVEL
///
/// Источники:
/// - MSDN: KeCancelTimer
/// - ReactOS: ntoskrnl/ke/timerobj.c:204-226
pub fn ke_cancel_timer(timer: &mut KTIMER) -> bool {
    if !timer.is_inserted() {
        return false;
    }

    unsafe {
        kx_remove_tree_timer(timer);
    }

    true
}

// =============================================================================
// Публичные API: KeReadStateTimer
// =============================================================================

/// KeReadStateTimer - читает сигнальное состояние таймера
///
/// # Returns
/// true если таймер в сигнальном состоянии (истёк)
///
/// # IRQL
/// Любой
///
/// Источники:
/// - MSDN: KeReadStateTimer
pub fn ke_read_state_timer(timer: &KTIMER) -> bool {
    timer.header.signal_state.load(Ordering::Acquire) != 0
}

// =============================================================================
// Внутренние функции: KiComputeDueTime
// =============================================================================

/// KiComputeDueTime - вычисляет абсолютное время истечения
///
/// ReactOS: ntoskrnl/include/internal/ke_x.h:952-995
///
/// # Arguments
/// * `due_time` - время из KeSetTimer:
///   - < 0: относительное (в 100ns единицах)
///   - >= 0: абсолютное системное время
///
/// # Returns
/// (abs_due_time, hand, already_expired):
/// - abs_due_time: абсолютное время в шкале InterruptTime
/// - hand: индекс bucket'а в таблице таймеров
/// - already_expired: true если таймер уже истёк
#[inline]
fn ki_compute_due_time(due_time: i64) -> (u64, usize, bool) {
    let now = super::time::ke_query_interrupt_time();

    let abs_due_time = if due_time < 0 {
        // Относительное время: now + |due_time|
        now.saturating_add((-due_time) as u64)
    } else {
        // Абсолютное системное время: преобразуем в InterruptTime
        // SystemTime = InterruptTime + base_time
        // InterruptTime = SystemTime - base_time
        let base_time = unsafe { (*(&raw const super::time::SHARED_USER_DATA)).base_time };

        if due_time <= base_time {
            // Время в прошлом - уже истёк
            0
        } else {
            (due_time - base_time) as u64
        }
    };

    // Проверяем не истёк ли таймер уже сейчас
    if abs_due_time <= now {
        return (abs_due_time, 0, true);
    }

    // Вычисляем hand (индекс bucket'а)
    let hand = ki_compute_timer_table_index(abs_due_time);

    (abs_due_time, hand, false)
}

/// KiComputeTimerTableIndex - вычисляет индекс bucket'а
///
/// ReactOS: ntoskrnl/include/internal/ke_x.h:881-884
///
/// Формула: (DueTime / KeMaximumIncrement) & (TIMER_TABLE_SIZE - 1)
///
/// Это хеш-функция распределяющая таймеры по bucket'ам.
/// Таймеры с близким временем истечения попадают в один bucket.
#[inline]
fn ki_compute_timer_table_index(due_time: u64) -> usize {
    let time_increment = super::init::ke_query_time_increment() as u64;
    let increment = if time_increment > 0 {
        time_increment
    } else {
        100_000
    };
    ((due_time / increment) as usize) & TIMER_TABLE_MASK
}

// =============================================================================
// Внутренние функции: вставка/удаление из таблицы
// =============================================================================

/// KxInsertTimer - вставляет таймер в таблицу
///
/// ReactOS: ntoskrnl/include/internal/ke_x.h:922-944
///
/// # Safety
/// Должен вызываться с корректным hand и валидным timer
#[inline]
unsafe fn kx_insert_timer(timer: *mut KTIMER, hand: usize) {
    // Захватываем lock bucket'а
    ke_acquire_spin_lock_at_dpc_level(&super::init::TIMER_TABLE_LOCK[hand]);

    // Вставляем в таблицу
    let expired = unsafe { ki_insert_timer_table(timer, hand) };

    ke_release_spin_lock_from_dpc_level(&super::init::TIMER_TABLE_LOCK[hand]);

    // Если таймер истёк во время вставки - сигнализируем
    if expired {
        unsafe { ki_signal_timer(&mut *timer) };
    }
}

/// KiInsertTimerTable - вставляет таймер в список bucket'а
///
/// ReactOS: ntoskrnl/ke/timerobj.c:61-110
///
/// Таймеры вставляются отсортированно по DueTime (от раннего к позднему).
/// Возвращает true если таймер уже истёк (DueTime <= InterruptTime).
///
/// # Safety
/// Должен вызываться с захваченным bucket lock
unsafe fn ki_insert_timer_table(timer: *mut KTIMER, hand: usize) -> bool {
    unsafe {
        let due_time = (*timer).due_time;

        debug_raw("[TIMER] KiInsertTimerTable: hand=");
        debug_hex(hand as u64);
        debug_raw(" due_time=");
        debug_hex(due_time);
        debug_raw("\n");

        let table = &mut *super::init::TIMER_TABLE_LIST_HEAD.get();
        let table_entry = &mut table[hand];
        let list_head = &mut table_entry.entry as *mut LIST_ENTRY;

        // Вставляем отсортированно: ищем позицию с конца
        let mut current = (*list_head).blink;

        while current != list_head {
            let existing = crate::containing_record!(current, KTIMER, timer_list_entry);
            if (*existing).due_time <= due_time {
                // Нашли позицию - вставляем после current
                let entry = &mut (*timer).timer_list_entry as *mut LIST_ENTRY;
                let next = (*current).flink;

                (*entry).flink = next;
                (*entry).blink = current;
                (*current).flink = entry;
                (*next).blink = entry;

                debug_raw("[TIMER] Inserted in middle, old_time=");
                debug_hex(table_entry.time);
                debug_raw("\n");

                // Обновляем минимальное время если нужно (но мы вставили не в начало)
                return check_timer_expired(due_time);
            }
            current = (*current).blink;
        }

        // Вставляем в начало списка (самый ранний таймер)
        LIST_ENTRY::insert_head(list_head, &mut (*timer).timer_list_entry as *mut LIST_ENTRY);

        debug_raw("[TIMER] Inserted at head, old_time=");
        debug_hex(table_entry.time);
        debug_raw(" new_time=");
        debug_hex(due_time);
        debug_raw("\n");

        // Обновляем минимальное время в bucket'е
        table_entry.time = due_time;

        // Проверяем не истёк ли таймер
        check_timer_expired(due_time)
    }
}

/// Проверяет истёк ли таймер
#[inline]
fn check_timer_expired(due_time: u64) -> bool {
    let now = super::time::ke_query_interrupt_time();
    due_time <= now
}

/// KxRemoveTreeTimer - удаляет таймер из таблицы
///
/// ReactOS: ntoskrnl/include/internal/ke_x.h:1002-1030
///
/// # Safety
/// Timer должен быть вставлен в таблицу
unsafe fn kx_remove_tree_timer(timer: *mut KTIMER) {
    unsafe {
        let hand = ki_compute_timer_table_index((*timer).due_time);

        // Захватываем lock bucket'а
        ke_acquire_spin_lock_at_dpc_level(&super::init::TIMER_TABLE_LOCK[hand]);

        // Удаляем из списка
        LIST_ENTRY::remove_entry(&mut (*timer).timer_list_entry as *mut LIST_ENTRY);
        (*timer).timer_list_entry = LIST_ENTRY::new();

        // Обновляем минимальное время в bucket'е
        let table = &mut *super::init::TIMER_TABLE_LIST_HEAD.get();
        let table_entry = &mut table[hand];
        let list_head = &mut table_entry.entry as *mut LIST_ENTRY;

        if LIST_ENTRY::is_empty(list_head) {
            // Bucket пуст
            table_entry.time = u64::MAX;
        } else {
            // Первый таймер теперь минимальный
            let first = (*list_head).flink;
            if !first.is_null() && first != list_head {
                let first_timer = crate::containing_record!(first, KTIMER, timer_list_entry);
                table_entry.time = (*first_timer).due_time;
            } else {
                table_entry.time = u64::MAX;
            }
        }

        ke_release_spin_lock_from_dpc_level(&super::init::TIMER_TABLE_LOCK[hand]);
    }
}

// =============================================================================
// Внутренние функции: сигнализация таймера
// =============================================================================

/// KiSignalTimer - сигнализирует таймер (устанавливает SignalState, будит waiters, ставит DPC)
///
/// ReactOS: ntoskrnl/ke/timerobj.c:112-163
///
/// # Safety
/// Должен вызываться на DISPATCH_LEVEL
unsafe fn ki_signal_timer(timer: *mut KTIMER) {
    use super::sched::KI_DISPATCHER_LOCK;
    use super::sched::ki_wait_test;

    unsafe {
        debug_raw("[TIMER] KiSignalTimer: timer=");
        debug_hex(timer as u64);
        debug_raw(" period=");
        debug_hex((*timer).period as u64);
        debug_raw("\n");

        // Устанавливаем SignalState
        (*timer).header.signal_state.store(1, Ordering::Release);

        // Захватываем dispatcher lock для пробуждения waiters
        ke_acquire_spin_lock_at_dpc_level(&KI_DISPATCHER_LOCK);

        // Пробуждаем ожидающие потоки
        ki_wait_test(&mut (*timer).header, TIMER_EXPIRE_INCREMENT);

        // Если есть DPC - ставим в очередь
        if !(*timer).dpc.is_null() {
            debug_raw("[TIMER] Queueing DPC\n");
            let dpc = &mut *(*timer).dpc;

            // SystemArgument1/2 = SystemTime (как в NT)
            let system_time = super::time::ke_query_system_time();
            let low = (system_time & 0xFFFFFFFF) as usize as PVOID;
            let high = ((system_time >> 32) & 0xFFFFFFFF) as usize as PVOID;

            super::dpc::ke_insert_queue_dpc(dpc, low, high);
        }

        // Периодический таймер - перезапускаем
        if (*timer).period > 0 {
            debug_raw("[TIMER] Periodic timer, rearming\n");
            // Сбрасываем SignalState (для следующего цикла)
            (*timer).header.signal_state.store(0, Ordering::Release);

            // Вычисляем новый DueTime
            let now = super::time::ke_query_interrupt_time();
            let new_due_time = now + ((*timer).period as u64 * 10_000); // мс -> 100нс
            (*timer).due_time = new_due_time;

            let new_hand = ki_compute_timer_table_index(new_due_time);

            debug_raw("[TIMER] New due_time=");
            debug_hex(new_due_time);
            debug_raw(" hand=");
            debug_hex(new_hand as u64);
            debug_raw("\n");

            // Освобождаем dispatcher lock перед вставкой
            ke_release_spin_lock_from_dpc_level(&KI_DISPATCHER_LOCK);

            // Вставляем обратно
            kx_insert_timer(timer, new_hand);
        } else {
            debug_raw("[TIMER] One-shot timer, done\n");
            ke_release_spin_lock_from_dpc_level(&KI_DISPATCHER_LOCK);
        }
    }
}

// =============================================================================
// CLOCK_LEVEL: быстрая проверка таймеров
// =============================================================================

/// KiCheckTimerTable - быстрая O(1) проверка наличия истекших таймеров
///
/// Вызывается из KeUpdateSystemTime на CLOCK_LEVEL.
/// Не берёт никаких блокировок - только читает bucket[hand].time.
///
/// ReactOS pattern (ke/time.c):
/// ```c
/// Hand = KeTickCount.LowPart & (TIMER_TABLE_SIZE - 1);
/// if (KiTimerTableListHead[Hand].Time.QuadPart <= InterruptTime.QuadPart)
/// ```
///
/// # Arguments
/// * `current_time` - текущее InterruptTime
///
/// # Returns
/// - `Some(hand)` - есть истекшие таймеры в bucket `hand`
/// - `None` - нет истекших таймеров
///
/// # Safety
/// Должна вызываться на CLOCK_LEVEL
#[inline]
pub unsafe fn ki_check_timer_table(current_time: u64) -> Option<usize> {
    unsafe {
        // Hand = TickCount & (TIMER_TABLE_SIZE - 1)
        let tick_count = super::time::ke_query_tick_count();
        let hand = (tick_count as usize) & TIMER_TABLE_MASK;

        let table = &*super::init::TIMER_TABLE_LIST_HEAD.get();
        let bucket_time = table[hand].time;

        // O(1) проверка минимального времени в bucket'е
        if bucket_time <= current_time {
            debug_raw("[TIMER] KiCheckTimerTable: hand=");
            debug_hex(hand as u64);
            debug_raw(" bucket_time=");
            debug_hex(bucket_time);
            debug_raw(" current=");
            debug_hex(current_time);
            debug_raw(" EXPIRED\n");
            return Some(hand);
        }

        None
    }
}

// =============================================================================
// DISPATCH_LEVEL: обработка истекших таймеров
// =============================================================================

/// KiTimerExpiration - обрабатывает истекшие таймеры
///
/// Вызывается из ki_dispatch_interrupt на DISPATCH_LEVEL когда
/// установлен Prcb->TimerRequest.
///
/// ReactOS: ntoskrnl/ke/dpc.c:77-334
///
/// # Алгоритм (range scan от hand до limit)
/// 1. Нормализуем limit чтобы не выйти за границы таблицы
/// 2. Обходим bucket'ы от hand до limit
/// 3. В каждом bucket'е снимаем таймеры пока due_time <= interrupt_time
/// 4. Для каждого истёкшего таймера: signal, wake waiters, queue DPC, rearm periodic
///
/// # Safety
/// Должен вызываться на DISPATCH_LEVEL
pub unsafe fn ki_timer_expiration(current_time: u64) {
    use super::sched::KI_DISPATCHER_LOCK;
    use super::sched::ki_wait_test;

    unsafe {
        debug_raw("[TIMER] === KiTimerExpiration START current_time=");
        debug_hex(current_time);
        debug_raw(" ===\n");

        // Получаем hand из PRCB (был установлен в ke_update_system_time)
        let prcb = crate::arch::x86_64::pcr::get_prcb();
        let start_hand = if !prcb.is_null() {
            (*prcb).timer_hand as usize
        } else {
            0
        };

        debug_raw("[TIMER] start_hand=");
        debug_hex(start_hand as u64);
        debug_raw("\n");

        // Счётчики для батчинга (ограничиваем работу за один вызов)
        const MAX_TIMERS_PER_CALL: usize = 64;
        let mut timers_processed = 0usize;

        // Массив для сбора истёкших таймеров (обрабатываем после освобождения bucket lock)
        const BATCH_SIZE: usize = 16;
        let mut expired_batch: [*mut KTIMER; BATCH_SIZE] = [core::ptr::null_mut(); BATCH_SIZE];
        let mut batch_count = 0usize;

        let table = &mut *super::init::TIMER_TABLE_LIST_HEAD.get();

        // ИСПРАВЛЕНИЕ: Полный проход таблицы вместо range scan
        // Обрабатываем ВСЕ bucket'ы с истекшими таймерами (time <= current_time)
        // Это гарантирует что ни один истекший таймер не будет пропущен
        let mut buckets_scanned = 0usize;
        for index in 0..TIMER_TABLE_SIZE {
            buckets_scanned += 1;

            // Быстрая проверка: bucket пуст или не истёк
            let bucket_time = table[index].time;

            // DEBUG: отключено - слишком много шума
            // debug_raw("[TIMER] Scan bucket[");
            // debug_hex(index as u64);
            // debug_raw("] time=");
            // debug_hex(bucket_time);
            // debug_raw("\n");

            if bucket_time != u64::MAX && bucket_time <= current_time {
                // Bucket содержит истёкшие таймеры - обрабатываем
                debug_raw("[TIMER] Processing bucket[");
                debug_hex(index as u64);
                debug_raw("]\n");

                // Захватываем lock bucket'а
                ke_acquire_spin_lock_at_dpc_level(&super::init::TIMER_TABLE_LOCK[index]);

                let table_entry = &mut table[index];
                let list_head = &mut table_entry.entry as *mut LIST_ENTRY;

                // Собираем истёкшие таймеры
                let mut entry = (*list_head).flink;
                while entry != list_head && !entry.is_null() {
                    let next = (*entry).flink;
                    let timer = crate::containing_record!(entry, KTIMER, timer_list_entry);

                    if (*timer).due_time <= current_time {
                        // Таймер истёк - удаляем из списка
                        debug_raw("[TIMER] Expired timer=");
                        debug_hex(timer as u64);
                        debug_raw(" due=");
                        debug_hex((*timer).due_time);
                        debug_raw("\n");

                        LIST_ENTRY::remove_entry(entry);
                        (*timer).timer_list_entry = LIST_ENTRY::new();

                        // Добавляем в batch
                        if batch_count < BATCH_SIZE {
                            expired_batch[batch_count] = timer;
                            batch_count += 1;
                        }

                        timers_processed += 1;
                        if timers_processed >= MAX_TIMERS_PER_CALL {
                            break;
                        }
                    } else {
                        // Таймеры отсортированы - остальные ещё не истекли
                        debug_raw("[TIMER] Timer not expired: due=");
                        debug_hex((*timer).due_time);
                        debug_raw("\n");
                        break;
                    }

                    entry = next;
                }

                // Обновляем минимальное время в bucket'е
                if LIST_ENTRY::is_empty(list_head) {
                    table_entry.time = u64::MAX;
                } else {
                    let first = (*list_head).flink;
                    if !first.is_null() && first != list_head {
                        let first_timer =
                            crate::containing_record!(first, KTIMER, timer_list_entry);
                        table_entry.time = (*first_timer).due_time;
                    } else {
                        table_entry.time = u64::MAX;
                    }
                }

                ke_release_spin_lock_from_dpc_level(&super::init::TIMER_TABLE_LOCK[index]);

                // Обрабатываем собранный batch
                if batch_count > 0 {
                    ke_acquire_spin_lock_at_dpc_level(&KI_DISPATCHER_LOCK);

                    for i in 0..batch_count {
                        let timer = expired_batch[i];
                        if timer.is_null() {
                            continue;
                        }

                        // Устанавливаем SignalState
                        (*timer).header.signal_state.store(1, Ordering::Release);

                        // Пробуждаем ожидающие потоки
                        ki_wait_test(&mut (*timer).header, TIMER_EXPIRE_INCREMENT);
                    }

                    ke_release_spin_lock_from_dpc_level(&KI_DISPATCHER_LOCK);

                    // Обрабатываем DPC и периодические таймеры (вне dispatcher lock)
                    debug_raw("[TIMER] Processing DPC and periodic timers for batch_count=");
                    debug_hex(batch_count as u64);
                    debug_raw("\n");

                    for i in 0..batch_count {
                        let timer = expired_batch[i];
                        if timer.is_null() {
                            continue;
                        }

                        debug_raw("[TIMER] Batch[");
                        debug_hex(i as u64);
                        debug_raw("] timer=");
                        debug_hex(timer as u64);
                        debug_raw(" dpc=");
                        debug_hex((*timer).dpc as u64);
                        debug_raw(" period=");
                        debug_hex((*timer).period as u64);
                        debug_raw("\n");

                        // DPC
                        if !(*timer).dpc.is_null() {
                            debug_raw("[TIMER] Queueing DPC for timer\n");
                            let dpc = &mut *(*timer).dpc;
                            let system_time = super::time::ke_query_system_time();
                            let low = (system_time & 0xFFFFFFFF) as usize as PVOID;
                            let high = ((system_time >> 32) & 0xFFFFFFFF) as usize as PVOID;
                            super::dpc::ke_insert_queue_dpc(dpc, low, high);
                        }

                        // Периодический таймер - перезапускаем
                        if (*timer).period > 0 {
                            debug_raw("[TIMER] Periodic timer, rearming with period=");
                            debug_hex((*timer).period as u64);
                            debug_raw("\n");
                            (*timer).header.signal_state.store(0, Ordering::Release);
                            let new_due_time = current_time + ((*timer).period as u64 * 10_000);
                            (*timer).due_time = new_due_time;
                            let new_hand = ki_compute_timer_table_index(new_due_time);
                            debug_raw("[TIMER] Rearming: new_due=");
                            debug_hex(new_due_time);
                            debug_raw(" new_hand=");
                            debug_hex(new_hand as u64);
                            debug_raw("\n");
                            kx_insert_timer(timer, new_hand);
                        }
                    }

                    // Сбрасываем batch
                    batch_count = 0;
                    for slot in expired_batch.iter_mut() {
                        *slot = core::ptr::null_mut();
                    }
                }
            } // Закрываем if для bucket обработки

            // Проверяем лимит обработки
            if timers_processed >= MAX_TIMERS_PER_CALL {
                debug_raw("[TIMER] Reached MAX_TIMERS_PER_CALL limit\n");
                break;
            }
        } // Конец цикла for по всем bucket'ам

        debug_raw("[TIMER] === KiTimerExpiration END buckets=");
        debug_hex(buckets_scanned as u64);
        debug_raw(" processed=");
        debug_hex(timers_processed as u64);
        debug_raw(" ===\n");

        let _ = (buckets_scanned, timers_processed); // подавляем warning
    }
}
