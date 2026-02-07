//! KEVENT - событие ядра
//!
//! Источники:
//! - NT5: inc/ke.h, ke/eventobj.c
//! - ReactOS: include/ndk/ketypes.h, ke/eventobj.c

#![allow(non_camel_case_types)]
#![allow(dead_code)]

use core::sync::atomic::AtomicI32;
use core::sync::atomic::Ordering;

use crate::nt::LIST_ENTRY;
use crate::nt::LONG;

/// Типы событий
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EVENT_TYPE {
    /// NotificationEvent - остается signaled пока не сброшен вручную
    NotificationEvent = 0,
    /// SynchronizationEvent - автоматически сбрасывается после пробуждения одного потока
    SynchronizationEvent = 1,
}

/// DISPATCHER_HEADER - заголовок всех dispatcher объектов
///
/// Общий заголовок для KEVENT, KMUTEX, KSEMAPHORE, KTIMER и др.
#[repr(C)]
pub struct DISPATCHER_HEADER {
    /// Тип объекта (из KOBJECTS enum)
    pub r#type: u8,
    /// Размер объекта в DWORD
    pub size: u8,
    /// Флаги объекта
    pub flags: u8,
    /// Reserved / SignalState high byte
    pub reserved: u8,
    /// Состояние сигнала (>0 = signaled для event/semaphore)
    pub signal_state: AtomicI32,
    /// Список ожидающих потоков
    pub wait_list_head: LIST_ENTRY,
}

impl DISPATCHER_HEADER {
    /// Создает новый заголовок
    pub const fn new(object_type: u8, object_size: u8) -> Self {
        Self {
            r#type: object_type,
            size: object_size,
            flags: 0,
            reserved: 0,
            signal_state: AtomicI32::new(0),
            wait_list_head: LIST_ENTRY::new(),
        }
    }

    /// Инициализирует wait list (должен вызываться перед использованием)
    ///
    /// # Safety
    /// Вызывающий должен гарантировать что объект валиден
    pub unsafe fn init_wait_list(&mut self) {
        unsafe {
            LIST_ENTRY::init_head(&mut self.wait_list_head as *mut LIST_ENTRY);
        }
    }
}

// Типы dispatcher объектов (из KOBJECTS enum)
pub const EVENT_NOTIFICATION_OBJECT: u8 = 0;
pub const EVENT_SYNCHRONIZATION_OBJECT: u8 = 1;
pub const MUTANT_OBJECT: u8 = 2;
pub const PROCESS_OBJECT: u8 = 3;
pub const QUEUE_OBJECT: u8 = 4;
pub const SEMAPHORE_OBJECT: u8 = 5;
pub const THREAD_OBJECT: u8 = 6;
pub const TIMER_NOTIFICATION_OBJECT: u8 = 8;
pub const TIMER_SYNCHRONIZATION_OBJECT: u8 = 9;

/// KOBJECT_TYPE - типы kernel объектов
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KOBJECT_TYPE {
    EventNotification = 0,
    EventSynchronization = 1,
    Mutant = 2,
    Process = 3,
    Queue = 4,
    Semaphore = 5,
    Thread = 6,
    Gate = 7,
    TimerNotification = 8,
    TimerSynchronization = 9,
    Spare2 = 10,
    Spare3 = 11,
    Spare4 = 12,
    Spare5 = 13,
    Spare6 = 14,
    Spare7 = 15,
    Spare8 = 16,
    Spare9 = 17,
    ApcState = 18,
    Dpc = 19,
    DeviceQueue = 20,
    EventPair = 21,
    InterruptObject = 22,
    Profile = 23,
    ThreadedDpc = 24,
}

/// KEVENT - объект события ядра
///
/// События используются для синхронизации между потоками.
/// Могут быть notification (manual reset) или synchronization (auto reset).
#[repr(C)]
pub struct KEVENT {
    pub header: DISPATCHER_HEADER,
}

impl KEVENT {
    /// Создает новое событие (неинициализированное)
    pub const fn new() -> Self {
        Self {
            header: DISPATCHER_HEADER::new(EVENT_NOTIFICATION_OBJECT, 0),
        }
    }

    /// Проверяет signaled ли событие
    #[inline]
    pub fn is_signaled(&self) -> bool {
        self.header.signal_state.load(Ordering::Acquire) > 0
    }

    /// Получает текущее состояние сигнала
    #[inline]
    pub fn signal_state(&self) -> LONG {
        self.header.signal_state.load(Ordering::Acquire)
    }
}

impl Default for KEVENT {
    fn default() -> Self {
        Self::new()
    }
}

/// KeInitializeEvent - инициализирует событие
///
/// # Arguments
/// * `event` - указатель на событие
/// * `event_type` - тип события (Notification или Synchronization)
/// * `initial_state` - начальное состояние (true = signaled)
pub fn ke_initialize_event(event: &mut KEVENT, event_type: EVENT_TYPE, initial_state: bool) {
    let object_type = match event_type {
        EVENT_TYPE::NotificationEvent => EVENT_NOTIFICATION_OBJECT,
        EVENT_TYPE::SynchronizationEvent => EVENT_SYNCHRONIZATION_OBJECT,
    };

    event.header.r#type = object_type;
    event.header.size = (core::mem::size_of::<KEVENT>() / 4) as u8;
    event.header.flags = 0;
    event.header.reserved = 0;
    event
        .header
        .signal_state
        .store(if initial_state { 1 } else { 0 }, Ordering::Release);

    // Инициализируем wait list
    unsafe {
        event.header.init_wait_list();
    }
}

/// KeSetEvent - устанавливает событие в signaled состояние
///
/// Возвращает предыдущее состояние сигнала
pub fn ke_set_event(event: &KEVENT, increment: i32, wait: bool) -> LONG {
    let _ = wait; // TODO: wait optimization

    // Захватываем dispatcher lock
    let old_irql = super::spinlock::ke_acquire_spin_lock(&super::sched::KI_DISPATCHER_LOCK);

    let previous = event.header.signal_state.swap(1, Ordering::AcqRel);

    // Пробуждаем ожидающие потоки
    // Safety: header указывает на валидный DISPATCHER_HEADER внутри event
    unsafe {
        let header = &event.header as *const DISPATCHER_HEADER as *mut DISPATCHER_HEADER;
        super::sched::ki_wait_test(header, increment as i8);
    }

    // Освобождаем dispatcher lock
    super::spinlock::ke_release_spin_lock(&super::sched::KI_DISPATCHER_LOCK, old_irql);

    previous
}

/// KeClearEvent - сбрасывает событие (делает non-signaled)
pub fn ke_clear_event(event: &KEVENT) {
    event.header.signal_state.store(0, Ordering::Release);
}

/// KeResetEvent - сбрасывает событие и возвращает предыдущее состояние
pub fn ke_reset_event(event: &KEVENT) -> LONG {
    event.header.signal_state.swap(0, Ordering::AcqRel)
}

/// KeReadStateEvent - читает текущее состояние события
pub fn ke_read_state_event(event: &KEVENT) -> LONG {
    event.header.signal_state.load(Ordering::Acquire)
}

/// KePulseEvent - устанавливает событие и сразу сбрасывает
///
/// Пробуждает все ожидающие потоки для NotificationEvent
/// или один поток для SynchronizationEvent
pub fn ke_pulse_event(event: &KEVENT, increment: i32, wait: bool) -> LONG {
    let _ = wait;

    // Захватываем dispatcher lock
    let old_irql = super::spinlock::ke_acquire_spin_lock(&super::sched::KI_DISPATCHER_LOCK);

    // Устанавливаем в signaled
    let previous = event.header.signal_state.swap(1, Ordering::AcqRel);

    // Пробуждаем ожидающие потоки
    // Safety: header указывает на валидный DISPATCHER_HEADER внутри event
    unsafe {
        let header = &event.header as *const DISPATCHER_HEADER as *mut DISPATCHER_HEADER;
        super::sched::ki_wait_test(header, increment as i8);
    }

    // Сбрасываем обратно (для Pulse)
    event.header.signal_state.store(0, Ordering::Release);

    // Освобождаем dispatcher lock
    super::spinlock::ke_release_spin_lock(&super::sched::KI_DISPATCHER_LOCK, old_irql);

    previous
}
