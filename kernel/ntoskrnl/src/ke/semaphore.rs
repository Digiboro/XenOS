//! KSEMAPHORE - семафор ядра
//!
//! Источники:
//! - NT5: inc/ke.h, ke/semphobj.c
//! - ReactOS: include/ndk/ketypes.h, ke/semphobj.c

#![allow(non_camel_case_types)]
#![allow(dead_code)]

use core::sync::atomic::Ordering;

use super::event::DISPATCHER_HEADER;
use super::event::SEMAPHORE_OBJECT;
use crate::nt::LONG;

/// KSEMAPHORE - объект семафора ядра
///
/// Семафор - это счетчик, который можно увеличивать (release)
/// и уменьшать (acquire). Когда счетчик > 0, семафор signaled.
#[repr(C)]
pub struct KSEMAPHORE {
    pub header: DISPATCHER_HEADER,
    /// Максимальное значение счетчика
    pub limit: LONG,
}

impl KSEMAPHORE {
    /// Создает новый семафор (неинициализированный)
    pub const fn new() -> Self {
        Self {
            header: DISPATCHER_HEADER::new(SEMAPHORE_OBJECT, 0),
            limit: 0,
        }
    }

    /// Проверяет signaled ли семафор (count > 0)
    #[inline]
    pub fn is_signaled(&self) -> bool {
        self.header.signal_state.load(Ordering::Acquire) > 0
    }

    /// Получает текущее значение счетчика
    #[inline]
    pub fn count(&self) -> LONG {
        self.header.signal_state.load(Ordering::Acquire)
    }
}

impl Default for KSEMAPHORE {
    fn default() -> Self {
        Self::new()
    }
}

/// KeInitializeSemaphore - инициализирует семафор
///
/// # Arguments
/// * `semaphore` - указатель на семафор
/// * `count` - начальное значение счетчика
/// * `limit` - максимальное значение счетчика
pub fn ke_initialize_semaphore(semaphore: &mut KSEMAPHORE, count: LONG, limit: LONG) {
    semaphore.header.r#type = SEMAPHORE_OBJECT;
    semaphore.header.size = (core::mem::size_of::<KSEMAPHORE>() / 4) as u8;
    semaphore.header.flags = 0;
    semaphore.header.reserved = 0;
    semaphore
        .header
        .signal_state
        .store(count, Ordering::Release);
    semaphore.limit = limit;

    unsafe {
        semaphore.header.init_wait_list();
    }
}

/// KeReleaseSemaphore - увеличивает счетчик семафора
///
/// # Arguments
/// * `semaphore` - указатель на семафор
/// * `increment` - priority boost для разбуженных потоков
/// * `adjustment` - значение для добавления к счетчику
/// * `wait` - оптимизация ожидания
///
/// # Returns
/// Предыдущее значение счетчика
///
/// # Errors
/// Возвращает STATUS_SEMAPHORE_LIMIT_EXCEEDED если превышен лимит
pub fn ke_release_semaphore(
    semaphore: &KSEMAPHORE,
    increment: i32,
    adjustment: LONG,
    wait: bool,
) -> Result<LONG, ()> {
    let _ = wait;

    // Захватываем dispatcher lock
    let old_irql = super::spinlock::ke_acquire_spin_lock(&super::sched::KI_DISPATCHER_LOCK);

    // Атомарно увеличиваем счетчик
    let previous = semaphore
        .header
        .signal_state
        .fetch_add(adjustment, Ordering::AcqRel);

    // Проверяем не превысили ли лимит
    let new_value = previous + adjustment;
    if new_value > semaphore.limit {
        // Откатываем изменение
        semaphore
            .header
            .signal_state
            .fetch_sub(adjustment, Ordering::AcqRel);

        // Освобождаем dispatcher lock
        super::spinlock::ke_release_spin_lock(&super::sched::KI_DISPATCHER_LOCK, old_irql);
        return Err(()); // STATUS_SEMAPHORE_LIMIT_EXCEEDED
    }

    // Пробуждаем ожидающие потоки если семафор стал signaled
    if previous == 0 && new_value > 0 {
        // Safety: header указывает на валидный DISPATCHER_HEADER внутри semaphore
        unsafe {
            let header = &semaphore.header as *const super::event::DISPATCHER_HEADER
                as *mut super::event::DISPATCHER_HEADER;
            super::sched::ki_wait_test(header, increment as i8);
        }
    }

    // Освобождаем dispatcher lock
    super::spinlock::ke_release_spin_lock(&super::sched::KI_DISPATCHER_LOCK, old_irql);

    Ok(previous)
}

/// KeReadStateSemaphore - читает текущее значение счетчика
pub fn ke_read_state_semaphore(semaphore: &KSEMAPHORE) -> LONG {
    semaphore.header.signal_state.load(Ordering::Acquire)
}

// =============================================================================
// ERESOURCE - ресурс для RW блокировок (Executive)
// =============================================================================

use super::spinlock::KSPIN_LOCK;

/// OWNER_ENTRY - запись о владельце ресурса
#[repr(C)]
pub struct OWNER_ENTRY {
    /// Идентификатор владельца (Thread или Table Index)
    pub owner_thread: usize,
    /// Счетчик владения / Флаги
    pub owner_count: LONG,
}

impl OWNER_ENTRY {
    pub const fn new() -> Self {
        Self {
            owner_thread: 0,
            owner_count: 0,
        }
    }
}

impl Default for OWNER_ENTRY {
    fn default() -> Self {
        Self::new()
    }
}

/// ERESOURCE - ресурс Executive для read-write блокировок
///
/// Более тяжелый чем EX_PUSH_LOCK, но поддерживает:
/// - Рекурсивный захват
/// - Отслеживание владельцев
/// - Статистику конкуренции
#[repr(C)]
pub struct ERESOURCE {
    /// Список ожидающих системных потоков
    pub system_resources_list: crate::nt::LIST_ENTRY,
    /// Владелец (exclusive) или таблица владельцев (shared)
    pub owner_table: *mut OWNER_ENTRY,
    /// Счетчик активных (>0 shared, <0 exclusive)
    pub active_count: i16,
    /// Флаги
    pub flag: u16,
    /// Семафор для shared ожидания
    pub shared_waiters: *mut KSEMAPHORE,
    /// Событие для exclusive ожидания
    pub exclusive_waiters: *mut super::event::KEVENT,
    /// Информация о владельце
    pub owner_entry: OWNER_ENTRY,
    /// Счетчик активных exclusive
    pub active_entries: LONG,
    /// Счетчик конкуренции
    pub contention_count: LONG,
    /// Счетчик shared ожидающих
    pub number_of_shared_waiters: LONG,
    /// Счетчик exclusive ожидающих
    pub number_of_exclusive_waiters: LONG,
    /// Зарезервировано / Адрес (для отладки)
    pub address: *mut core::ffi::c_void,
    /// Спинлок для защиты структуры
    pub spin_lock: KSPIN_LOCK,
}

// Флаги ERESOURCE
pub const RESOURCE_FLAG_DISABLE_BOOST: u16 = 0x0008;

impl ERESOURCE {
    pub const fn new() -> Self {
        Self {
            system_resources_list: crate::nt::LIST_ENTRY::new(),
            owner_table: core::ptr::null_mut(),
            active_count: 0,
            flag: 0,
            shared_waiters: core::ptr::null_mut(),
            exclusive_waiters: core::ptr::null_mut(),
            owner_entry: OWNER_ENTRY::new(),
            active_entries: 0,
            contention_count: 0,
            number_of_shared_waiters: 0,
            number_of_exclusive_waiters: 0,
            address: core::ptr::null_mut(),
            spin_lock: KSPIN_LOCK::new(),
        }
    }

    /// Проверяет захвачен ли ресурс
    #[inline]
    pub fn is_acquired(&self) -> bool {
        self.active_count != 0
    }

    /// Проверяет захвачен ли эксклюзивно
    #[inline]
    pub fn is_acquired_exclusive(&self) -> bool {
        self.active_count < 0
    }

    /// Проверяет захвачен ли для чтения
    #[inline]
    pub fn is_acquired_shared(&self) -> bool {
        self.active_count > 0
    }
}

impl Default for ERESOURCE {
    fn default() -> Self {
        Self::new()
    }
}

/// ExInitializeResourceLite - инициализирует ERESOURCE
pub fn ex_initialize_resource_lite(resource: &mut ERESOURCE) -> crate::nt::NTSTATUS {
    *resource = ERESOURCE::new();

    unsafe {
        crate::nt::LIST_ENTRY::init_head(
            &mut resource.system_resources_list as *mut crate::nt::LIST_ENTRY,
        );
    }

    crate::nt::STATUS_SUCCESS
}

/// ExDeleteResourceLite - удаляет ERESOURCE
pub fn ex_delete_resource_lite(resource: &mut ERESOURCE) -> crate::nt::NTSTATUS {
    // TODO: Проверить что ресурс не захвачен
    // TODO: Освободить owner_table если выделен
    let _ = resource;
    crate::nt::STATUS_SUCCESS
}

/// ExAcquireResourceExclusiveLite - захватывает ресурс эксклюзивно
pub fn ex_acquire_resource_exclusive_lite(resource: &mut ERESOURCE, wait: bool) -> bool {
    // Упрощенная реализация
    let _old_irql = super::spinlock::ke_acquire_spin_lock(&resource.spin_lock);

    if resource.active_count == 0 {
        // Свободен, захватываем
        resource.active_count = -1;
        // TODO: Установить владельца
        super::spinlock::ke_release_spin_lock(&resource.spin_lock, 0);
        return true;
    }

    super::spinlock::ke_release_spin_lock(&resource.spin_lock, 0);

    if !wait {
        return false;
    }

    // TODO: Ожидание
    false
}

/// ExAcquireResourceSharedLite - захватывает ресурс для чтения
pub fn ex_acquire_resource_shared_lite(resource: &mut ERESOURCE, wait: bool) -> bool {
    let _old_irql = super::spinlock::ke_acquire_spin_lock(&resource.spin_lock);

    if resource.active_count >= 0 {
        // Свободен или уже shared, увеличиваем счетчик
        resource.active_count += 1;
        super::spinlock::ke_release_spin_lock(&resource.spin_lock, 0);
        return true;
    }

    super::spinlock::ke_release_spin_lock(&resource.spin_lock, 0);

    if !wait {
        return false;
    }

    // TODO: Ожидание
    false
}

/// ExReleaseResourceLite - освобождает ресурс
pub fn ex_release_resource_lite(resource: &mut ERESOURCE) {
    let _old_irql = super::spinlock::ke_acquire_spin_lock(&resource.spin_lock);

    if resource.active_count < 0 {
        // Exclusive
        resource.active_count = 0;
    } else if resource.active_count > 0 {
        // Shared
        resource.active_count -= 1;
    }

    // TODO: Разбудить ожидающих если resource.active_count == 0

    super::spinlock::ke_release_spin_lock(&resource.spin_lock, 0);
}
