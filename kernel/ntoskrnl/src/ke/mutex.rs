//! KMUTEX и FAST_MUTEX - мьютексы ядра
//!
//! Источники:
//! - NT5: inc/ke.h, ke/mutntobj.c
//! - ReactOS: include/ndk/ketypes.h, ke/mutntobj.c

#![allow(non_camel_case_types)]
#![allow(dead_code)]

use core::sync::atomic::AtomicI32;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;

use super::event::DISPATCHER_HEADER;
use super::event::MUTANT_OBJECT;
use super::spinlock::KIRQL;
use crate::nt::LIST_ENTRY;
use crate::nt::LONG;
use crate::nt::PVOID;
use crate::nt::UCHAR;

/// KMUTANT (KMUTEX) - мьютекс ядра
///
/// Мьютекс может быть рекурсивно захвачен одним потоком.
/// Автоматически освобождается при завершении потока (abandoned).
#[repr(C)]
pub struct KMUTANT {
    pub header: DISPATCHER_HEADER,
    /// Список мьютексов потока (для отслеживания владения)
    pub mutant_list_entry: LIST_ENTRY,
    /// Поток-владелец (PKTHREAD)
    pub owner_thread: PVOID,
    /// Флаг abandoned
    pub abandoned: UCHAR,
    /// APC disable count
    pub apc_disable: UCHAR,
}

/// KMUTEX - синоним для KMUTANT
pub type KMUTEX = KMUTANT;

impl KMUTANT {
    /// Создает новый мьютекс (неинициализированный)
    pub const fn new() -> Self {
        Self {
            header: DISPATCHER_HEADER::new(MUTANT_OBJECT, 0),
            mutant_list_entry: LIST_ENTRY::new(),
            owner_thread: core::ptr::null_mut(),
            abandoned: 0,
            apc_disable: 0,
        }
    }

    /// Проверяет свободен ли мьютекс
    #[inline]
    pub fn is_signaled(&self) -> bool {
        self.header.signal_state.load(Ordering::Acquire) > 0
    }

    /// Проверяет принадлежит ли мьютекс указанному потоку
    #[inline]
    pub fn is_owned_by(&self, thread: PVOID) -> bool {
        self.owner_thread == thread
    }
}

impl Default for KMUTANT {
    fn default() -> Self {
        Self::new()
    }
}

/// KeInitializeMutex - инициализирует мьютекс
///
/// # Arguments
/// * `mutex` - указатель на мьютекс
/// * `level` - уровень (не используется в современных NT)
pub fn ke_initialize_mutex(mutex: &mut KMUTANT, level: u32) {
    let _ = level; // Не используется

    mutex.header.r#type = MUTANT_OBJECT;
    mutex.header.size = (core::mem::size_of::<KMUTANT>() / 4) as u8;
    mutex.header.flags = 0;
    mutex.header.reserved = 0;
    mutex.header.signal_state.store(1, Ordering::Release); // Свободен
    mutex.owner_thread = core::ptr::null_mut();
    mutex.abandoned = 0;
    mutex.apc_disable = 0;

    unsafe {
        mutex.header.init_wait_list();
        LIST_ENTRY::init_head(&mut mutex.mutant_list_entry as *mut LIST_ENTRY);
    }
}

/// KeReleaseMutex - освобождает мьютекс
///
/// Возвращает предыдущее состояние
pub fn ke_release_mutex(mutex: &mut KMUTANT, wait: bool) -> LONG {
    let _ = wait;

    // Захватываем dispatcher lock
    let old_irql = super::spinlock::ke_acquire_spin_lock(&super::sched::KI_DISPATCHER_LOCK);

    let previous = mutex.header.signal_state.load(Ordering::Acquire);

    // Увеличиваем signal_state (при 0 мьютекс занят, >0 свободен)
    let new_state = mutex.header.signal_state.fetch_add(1, Ordering::AcqRel) + 1;

    // Если мьютекс стал свободен (state > 0), очищаем владельца и пробуждаем потоки
    if new_state > 0 {
        mutex.owner_thread = core::ptr::null_mut();

        // Пробуждаем ожидающие потоки
        // Safety: header указывает на валидный DISPATCHER_HEADER внутри mutex
        unsafe {
            let header = &mutex.header as *const super::event::DISPATCHER_HEADER
                as *mut super::event::DISPATCHER_HEADER;
            super::sched::ki_wait_test(header, 1); // MUTEX_INCREMENT = 1
        }
    }

    // Освобождаем dispatcher lock
    super::spinlock::ke_release_spin_lock(&super::sched::KI_DISPATCHER_LOCK, old_irql);

    previous
}

/// KeReadStateMutex - читает состояние мьютекса
pub fn ke_read_state_mutex(mutex: &KMUTANT) -> LONG {
    mutex.header.signal_state.load(Ordering::Acquire)
}

// =============================================================================
// FAST_MUTEX - быстрый мьютекс
// =============================================================================

/// FAST_MUTEX - облегченный мьютекс для использования на IRQL < DISPATCH_LEVEL
///
/// Не может быть захвачен рекурсивно.
/// Более эффективен чем KMUTEX для простых случаев.
#[repr(C)]
pub struct FAST_MUTEX {
    /// Счетчик (1 = свободен, 0 = занят, <0 = занят + есть ожидающие)
    pub count: AtomicI32,
    /// Поток-владелец
    pub owner: PVOID,
    /// Счетчик конкуренции
    pub contention: AtomicUsize,
    /// Событие для ожидания
    pub event: super::event::KEVENT,
    /// Старый IRQL
    pub old_irql: KIRQL,
}

impl FAST_MUTEX {
    /// Создает новый быстрый мьютекс
    pub const fn new() -> Self {
        Self {
            count: AtomicI32::new(1),
            owner: core::ptr::null_mut(),
            contention: AtomicUsize::new(0),
            event: super::event::KEVENT::new(),
            old_irql: 0,
        }
    }

    /// Проверяет свободен ли мьютекс
    #[inline]
    pub fn is_free(&self) -> bool {
        self.count.load(Ordering::Acquire) > 0
    }
}

impl Default for FAST_MUTEX {
    fn default() -> Self {
        Self::new()
    }
}

/// ExInitializeFastMutex - инициализирует быстрый мьютекс
pub fn ex_initialize_fast_mutex(mutex: &mut FAST_MUTEX) {
    mutex.count.store(1, Ordering::Release);
    mutex.owner = core::ptr::null_mut();
    mutex.contention.store(0, Ordering::Release);

    // Инициализируем событие как SynchronizationEvent
    super::event::ke_initialize_event(
        &mut mutex.event,
        super::event::EVENT_TYPE::SynchronizationEvent,
        false,
    );
}

/// ExAcquireFastMutex - захватывает быстрый мьютекс
///
/// Поднимает IRQL до APC_LEVEL
pub fn ex_acquire_fast_mutex(mutex: &mut FAST_MUTEX) {
    // Поднимаем IRQL до APC_LEVEL
    let old_irql = crate::hal::kf_raise_irql(crate::hal::APC_LEVEL);
    mutex.old_irql = old_irql;

    // Атомарно уменьшаем счетчик
    let old_count = mutex.count.fetch_sub(1, Ordering::AcqRel);

    if old_count <= 0 {
        // Мьютекс был занят, нужно ждать
        mutex.contention.fetch_add(1, Ordering::Relaxed);
        // Ожидание требует scheduler - пока спинлок
        while mutex.count.load(Ordering::Acquire) < 0 {
            crate::arch::x86_64::cpu::yield_processor();
        }
    }

    // Сохраняем текущий поток как владельца
    mutex.owner = unsafe { crate::ps::ps_get_current_thread() as PVOID };
}

/// ExReleaseFastMutex - освобождает быстрый мьютекс
pub fn ex_release_fast_mutex(mutex: &mut FAST_MUTEX) {
    let old_irql = mutex.old_irql;
    mutex.owner = core::ptr::null_mut();

    // Атомарно увеличиваем счетчик
    let old_count = mutex.count.fetch_add(1, Ordering::AcqRel);

    if old_count < 0 {
        // Были ожидающие потоки, разбудить одного
        super::event::ke_set_event(&mutex.event, 0, false);
    }

    // Восстанавливаем IRQL
    crate::hal::kf_lower_irql(old_irql);
}

/// ExTryToAcquireFastMutex - пытается захватить быстрый мьютекс
///
/// Возвращает true если захват успешен
pub fn ex_try_to_acquire_fast_mutex(mutex: &mut FAST_MUTEX) -> bool {
    // Поднимаем IRQL до APC_LEVEL
    let old_irql = crate::hal::kf_raise_irql(crate::hal::APC_LEVEL);

    // Пытаемся атомарно изменить 1 -> 0
    if mutex
        .count
        .compare_exchange(1, 0, Ordering::AcqRel, Ordering::Relaxed)
        .is_ok()
    {
        // Сохраняем владельца и старый IRQL
        mutex.old_irql = old_irql;
        mutex.owner = unsafe { crate::ps::ps_get_current_thread() as PVOID };
        true
    } else {
        // Не удалось захватить - восстанавливаем IRQL
        crate::hal::kf_lower_irql(old_irql);
        false
    }
}

// =============================================================================
// EX_PUSH_LOCK - push-lock (облегченный RW lock)
// =============================================================================

/// EX_PUSH_LOCK - очень легковесная блокировка
///
/// Поддерживает shared (read) и exclusive (write) режимы.
/// Размер всего один указатель.
#[repr(transparent)]
pub struct EX_PUSH_LOCK {
    value: AtomicUsize,
}

// Биты EX_PUSH_LOCK
const EX_PUSH_LOCK_LOCKED: usize = 0x1;
const EX_PUSH_LOCK_WAITING: usize = 0x2;
const EX_PUSH_LOCK_WAKING: usize = 0x4;
const EX_PUSH_LOCK_MULTIPLE_SHARED: usize = 0x8;
const EX_PUSH_LOCK_SHARE_INC: usize = 0x10;

impl EX_PUSH_LOCK {
    /// Создает новый push-lock
    pub const fn new() -> Self {
        Self {
            value: AtomicUsize::new(0),
        }
    }

    /// Проверяет свободен ли lock
    #[inline]
    pub fn is_free(&self) -> bool {
        self.value.load(Ordering::Acquire) == 0
    }

    /// Проверяет захвачен ли эксклюзивно
    #[inline]
    pub fn is_locked_exclusive(&self) -> bool {
        let v = self.value.load(Ordering::Acquire);
        (v & EX_PUSH_LOCK_LOCKED) != 0 && (v & EX_PUSH_LOCK_MULTIPLE_SHARED) == 0
    }
}

impl Default for EX_PUSH_LOCK {
    fn default() -> Self {
        Self::new()
    }
}

/// ExInitializePushLock - инициализирует push-lock
pub fn ex_initialize_push_lock(push_lock: &mut EX_PUSH_LOCK) {
    push_lock.value.store(0, Ordering::Release);
}

/// ExAcquirePushLockExclusive - захватывает push-lock эксклюзивно
pub fn ex_acquire_push_lock_exclusive(push_lock: &EX_PUSH_LOCK) {
    // Простая реализация: спин пока не получим эксклюзивный доступ
    loop {
        let current = push_lock.value.load(Ordering::Acquire);
        if current == 0 {
            // Пытаемся захватить
            if push_lock
                .value
                .compare_exchange(0, EX_PUSH_LOCK_LOCKED, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                return;
            }
        }
        crate::arch::x86_64::cpu::yield_processor();
    }
}

/// ExReleasePushLockExclusive - освобождает эксклюзивный push-lock
pub fn ex_release_push_lock_exclusive(push_lock: &EX_PUSH_LOCK) {
    push_lock.value.store(0, Ordering::Release);
    // TODO: Разбудить ожидающих если есть
}

/// ExAcquirePushLockShared - захватывает push-lock для чтения
pub fn ex_acquire_push_lock_shared(push_lock: &EX_PUSH_LOCK) {
    loop {
        let current = push_lock.value.load(Ordering::Acquire);

        // Если не заблокирован эксклюзивно
        if (current & EX_PUSH_LOCK_LOCKED) == 0 || (current & EX_PUSH_LOCK_MULTIPLE_SHARED) != 0 {
            let new_value = if current == 0 {
                EX_PUSH_LOCK_LOCKED | EX_PUSH_LOCK_MULTIPLE_SHARED | EX_PUSH_LOCK_SHARE_INC
            } else {
                current + EX_PUSH_LOCK_SHARE_INC
            };

            if push_lock
                .value
                .compare_exchange(current, new_value, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                return;
            }
        }
        crate::arch::x86_64::cpu::yield_processor();
    }
}

/// ExReleasePushLockShared - освобождает shared push-lock
pub fn ex_release_push_lock_shared(push_lock: &EX_PUSH_LOCK) {
    let current = push_lock
        .value
        .fetch_sub(EX_PUSH_LOCK_SHARE_INC, Ordering::AcqRel);
    let new_value = current - EX_PUSH_LOCK_SHARE_INC;

    // Если это был последний shared reader, очищаем флаги
    if new_value == (EX_PUSH_LOCK_LOCKED | EX_PUSH_LOCK_MULTIPLE_SHARED) {
        let _ = push_lock
            .value
            .compare_exchange(new_value, 0, Ordering::AcqRel, Ordering::Relaxed);
    }
}
