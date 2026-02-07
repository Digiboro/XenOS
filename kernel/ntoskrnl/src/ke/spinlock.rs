//! KSPIN_LOCK - спинлок ядра
//!
//! Источники:
//! - NT5: inc/ke.h, ke/spinlock.c
//! - ReactOS: include/ndk/ketypes.h, ke/spinlock.c

#![allow(non_camel_case_types)]
#![allow(dead_code)]

use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;

// Реэкспорт IRQL констант из hal::irql (единственный источник истины)
pub use crate::hal::irql::{
    APC_LEVEL, CLOCK_LEVEL, DEVICE_IRQL_BASE, DISPATCH_LEVEL, HIGH_LEVEL, IPI_LEVEL, KIRQL,
    LOW_LEVEL, PASSIVE_LEVEL, POWER_LEVEL, PROFILE_LEVEL,
};

/// KSPIN_LOCK - примитив синхронизации ядра
///
/// На x86_64 это атомарное значение (0 = свободен, !0 = занят)
/// Спинлок должен захватываться только на уровне DISPATCH_LEVEL или выше
#[repr(transparent)]
pub struct KSPIN_LOCK {
    lock: AtomicUsize,
}

impl KSPIN_LOCK {
    /// Создает новый незанятый спинлок
    pub const fn new() -> Self {
        Self {
            lock: AtomicUsize::new(0),
        }
    }

    /// Инициализирует спинлок (для совместимости с C API)
    #[inline]
    pub fn initialize(&mut self) {
        self.lock.store(0, Ordering::Release);
    }

    /// Пытается захватить спинлок
    ///
    /// Возвращает true если захват успешен
    #[inline]
    pub fn try_acquire(&self) -> bool {
        self.lock
            .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
    }

    /// Захватывает спинлок (блокирующий)
    ///
    /// Активно ожидает пока спинлок не станет доступен
    #[inline]
    pub fn acquire(&self) {
        while !self.try_acquire() {
            // Используем pause для оптимизации на гипертрединге
            crate::arch::x86_64::cpu::yield_processor();
        }
    }

    /// Освобождает спинлок
    #[inline]
    pub fn release(&self) {
        self.lock.store(0, Ordering::Release);
    }

    /// Проверяет занят ли спинлок
    #[inline]
    pub fn is_locked(&self) -> bool {
        self.lock.load(Ordering::Relaxed) != 0
    }
}

impl Default for KSPIN_LOCK {
    fn default() -> Self {
        Self::new()
    }
}

// Safety: KSPIN_LOCK можно безопасно передавать между потоками
unsafe impl Send for KSPIN_LOCK {}
unsafe impl Sync for KSPIN_LOCK {}

/// KeInitializeSpinLock - инициализирует спинлок
///
/// NT API совместимая функция
#[inline]
pub fn ke_initialize_spin_lock(spin_lock: &mut KSPIN_LOCK) {
    spin_lock.initialize();
}

/// KeAcquireSpinLock - захватывает спинлок и поднимает IRQL до DISPATCH_LEVEL
///
/// Возвращает предыдущий IRQL который нужно восстановить при освобождении
#[inline]
pub fn ke_acquire_spin_lock(spin_lock: &KSPIN_LOCK) -> KIRQL {
    // Поднимаем IRQL до DISPATCH_LEVEL
    let old_irql = crate::hal::kf_raise_irql(crate::hal::DISPATCH_LEVEL);

    spin_lock.acquire();
    old_irql
}

/// KeReleaseSpinLock - освобождает спинлок и восстанавливает IRQL
#[inline]
pub fn ke_release_spin_lock(spin_lock: &KSPIN_LOCK, old_irql: KIRQL) {
    spin_lock.release();

    // Восстанавливаем предыдущий IRQL
    crate::hal::kf_lower_irql(old_irql);
}

/// KeTryToAcquireSpinLock - пытается захватить спинлок
///
/// Возвращает true и предыдущий IRQL если захват успешен
#[inline]
pub fn ke_try_to_acquire_spin_lock(spin_lock: &KSPIN_LOCK) -> Option<KIRQL> {
    // Поднимаем IRQL до DISPATCH_LEVEL
    let old_irql = crate::hal::kf_raise_irql(crate::hal::DISPATCH_LEVEL);

    if spin_lock.try_acquire() {
        Some(old_irql)
    } else {
        // Восстанавливаем IRQL если не удалось захватить
        crate::hal::kf_lower_irql(old_irql);
        None
    }
}

// =============================================================================
// Обобщенные acquire/try для произвольного IRQL (NT/ReactOS KD-паттерн)
// =============================================================================

/// KeAcquireSpinLockRaiseTo - захватывает спинлок и поднимает IRQL до заданного уровня.
///
/// Используется для путей, которые могут вызываться на любом IRQL и должны быть
/// защищены от реэнтерабельности через прерывания на текущем CPU.
///
/// Паттерн соответствует ReactOS/NT KD:
/// - сначала ждём, пока lock станет свободным (на текущем IRQL),
/// - затем поднимаем IRQL (обычно HIGH_LEVEL),
/// - затем пытаемся захватить lock,
/// - если не удалось (гонка), восстанавливаем IRQL и повторяем.
///
/// # Returns
/// Предыдущий IRQL (для восстановления при освобождении).
#[inline]
pub fn ke_acquire_spin_lock_raise_to(spin_lock: &KSPIN_LOCK, raise_to: KIRQL) -> KIRQL {
    debug_assert!(
        raise_to >= DISPATCH_LEVEL,
        "KeAcquireSpinLockRaiseTo: raise_to ({}) < DISPATCH_LEVEL",
        raise_to
    );

    loop {
        // 1) Ждем, пока lock станет свободен, не поднимая IRQL.
        while spin_lock.is_locked() {
            crate::arch::x86_64::cpu::yield_processor();
        }

        // 2) Поднимаем IRQL (может быть no-op если уже выше).
        let old_irql = crate::hal::kf_raise_irql(raise_to);

        // 3) Пытаемся захватить lock.
        if spin_lock.try_acquire() {
            return old_irql;
        }

        // 4) Гонка: кто-то успел захватить lock. Восстанавливаем IRQL и повторяем.
        crate::hal::kf_lower_irql(old_irql);
    }
}

/// KeTryToAcquireSpinLockRaiseTo - пытается захватить спинлок с поднятием IRQL до заданного уровня.
///
/// В случае неуспеха возвращает None и восстанавливает IRQL.
///
/// NOTE: используем pre-check `is_locked()` чтобы не поднимать IRQL заведомо зря.
#[inline]
pub fn ke_try_to_acquire_spin_lock_raise_to(
    spin_lock: &KSPIN_LOCK,
    raise_to: KIRQL,
) -> Option<KIRQL> {
    debug_assert!(
        raise_to >= DISPATCH_LEVEL,
        "KeTryToAcquireSpinLockRaiseTo: raise_to ({}) < DISPATCH_LEVEL",
        raise_to
    );

    if spin_lock.is_locked() {
        return None;
    }

    let old_irql = crate::hal::kf_raise_irql(raise_to);
    if spin_lock.try_acquire() {
        Some(old_irql)
    } else {
        crate::hal::kf_lower_irql(old_irql);
        None
    }
}

/// KeAcquireSpinLockAtDpcLevel - захватывает спинлок уже на DISPATCH_LEVEL
///
/// Используется когда IRQL уже поднят (не меняет IRQL)
#[inline]
pub fn ke_acquire_spin_lock_at_dpc_level(spin_lock: &KSPIN_LOCK) {
    spin_lock.acquire();
}

/// KeReleaseSpinLockFromDpcLevel - освобождает спинлок на DISPATCH_LEVEL
///
/// Не меняет IRQL
#[inline]
pub fn ke_release_spin_lock_from_dpc_level(spin_lock: &KSPIN_LOCK) {
    spin_lock.release();
}

/// KSPIN_LOCK_QUEUE - элемент очереди для queued spinlocks
///
/// Более эффективные спинлоки для высококонкурентного доступа
#[repr(C)]
#[derive(Clone, Copy)]
pub struct KSPIN_LOCK_QUEUE {
    pub next: *mut KSPIN_LOCK_QUEUE,
    pub lock: *mut KSPIN_LOCK,
}

impl KSPIN_LOCK_QUEUE {
    pub const fn new() -> Self {
        Self {
            next: core::ptr::null_mut(),
            lock: core::ptr::null_mut(),
        }
    }
}

impl Default for KSPIN_LOCK_QUEUE {
    fn default() -> Self {
        Self::new()
    }
}

/// Индексы для per-processor queued spinlocks (из KPRCB)
#[repr(usize)]
#[derive(Clone, Copy, Debug)]
pub enum LockQueueNumber {
    LockQueueDispatcherLock = 0,
    LockQueueContextSwapLock = 1,
    LockQueuePfnLock = 2,
    LockQueueSystemSpaceLock = 3,
    LockQueueVacbLock = 4,
    LockQueueMasterLock = 5,
    LockQueueNonPagedPoolLock = 6,
    LockQueueIoCancelLock = 7,
    LockQueueWorkQueueLock = 8,
    LockQueueIoVpbLock = 9,
    LockQueueIoDatabaseLock = 10,
    LockQueueIoCompletionLock = 11,
    LockQueueNtfsStructLock = 12,
    LockQueueAfdWorkQueueLock = 13,
    LockQueueBcbLock = 14,
    LockQueueMmNonPagedPoolLock = 15,
    LockQueueMaximumLock = 16,
}

/// KLOCK_QUEUE_HANDLE - handle для queued spinlock операций
#[repr(C)]
pub struct KLOCK_QUEUE_HANDLE {
    pub lock_queue: KSPIN_LOCK_QUEUE,
    pub old_irql: KIRQL,
}

impl KLOCK_QUEUE_HANDLE {
    pub const fn new() -> Self {
        Self {
            lock_queue: KSPIN_LOCK_QUEUE::new(),
            old_irql: PASSIVE_LEVEL,
        }
    }
}

impl Default for KLOCK_QUEUE_HANDLE {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// PRCB Lock функции (для планировщика)
// =============================================================================

use crate::arch::x86_64::pcr::KPRCB;
use crate::ke::thread::KTHREAD;

/// KiAcquirePrcbLock - захватывает PRCB lock с повышением IRQL до DISPATCH_LEVEL
///
/// Используется для защиты ready queues и других структур PRCB.
/// Соответствует ReactOS KiAcquirePrcbLock.
///
/// # Returns
/// Предыдущий IRQL для восстановления
#[inline]
pub unsafe fn ki_acquire_prcb_lock(prcb: *mut KPRCB) -> KIRQL {
    unsafe {
        let old_irql = crate::hal::kf_raise_irql(DISPATCH_LEVEL);

        // Spin-wait loop
        loop {
            if (*prcb).prcb_lock.try_acquire() {
                break;
            }

            // Busy-wait с pause для энергоэффективности
            while (*prcb).prcb_lock.is_locked() {
                crate::arch::x86_64::cpu::yield_processor();
            }
        }

        old_irql
    }
}

/// KiReleasePrcbLock - освобождает PRCB lock и понижает IRQL
#[inline]
pub unsafe fn ki_release_prcb_lock(prcb: *mut KPRCB, old_irql: KIRQL) {
    unsafe {
        (*prcb).prcb_lock.release();
        crate::hal::kf_lower_irql(old_irql);
    }
}

/// KiAcquirePrcbLockAtDpcLevel - захватывает PRCB lock без изменения IRQL
///
/// Используется когда IRQL уже поднят до DISPATCH_LEVEL.
#[inline]
pub unsafe fn ki_acquire_prcb_lock_at_dpc_level(prcb: *mut KPRCB) {
    unsafe {
        loop {
            if (*prcb).prcb_lock.try_acquire() {
                break;
            }

            while (*prcb).prcb_lock.is_locked() {
                crate::arch::x86_64::cpu::yield_processor();
            }
        }
    }
}

/// KiReleasePrcbLockFromDpcLevel - освобождает PRCB lock без изменения IRQL
#[inline]
pub unsafe fn ki_release_prcb_lock_from_dpc_level(prcb: *mut KPRCB) {
    unsafe {
        (*prcb).prcb_lock.release();
    }
}

// =============================================================================
// Thread Lock функции (для изменения приоритета и т.д.)
// =============================================================================

/// KiAcquireThreadLock - захватывает lock потока
///
/// Используется при изменении приоритета, состояния и других полей потока.
/// Соответствует ReactOS KiAcquireThreadLock.
#[inline]
pub unsafe fn ki_acquire_thread_lock(thread: *mut KTHREAD) {
    unsafe {
        loop {
            if (*thread).thread_lock.try_acquire() {
                break;
            }

            while (*thread).thread_lock.is_locked() {
                crate::arch::x86_64::cpu::yield_processor();
            }
        }
    }
}

/// KiReleaseThreadLock - освобождает lock потока
#[inline]
pub unsafe fn ki_release_thread_lock(thread: *mut KTHREAD) {
    unsafe {
        (*thread).thread_lock.release();
    }
}

/// KiTryToAcquireThreadLock - пытается захватить lock потока без блокировки
///
/// # Returns
/// true если lock захвачен, false если занят
#[inline]
pub unsafe fn ki_try_to_acquire_thread_lock(thread: *mut KTHREAD) -> bool {
    unsafe { (*thread).thread_lock.try_acquire() }
}

// =============================================================================
// RAII Guard for Spin Lock
// =============================================================================

/// KSpinLockGuard - RAII guard для автоматического освобождения спинлока
///
/// Автоматически захватывает спинлок при создании и освобождает при drop.
/// Также управляет IRQL (поднимает до DISPATCH_LEVEL при захвате,
/// восстанавливает при освобождении).
pub struct KSpinLockGuard {
    lock: *mut KSPIN_LOCK,
    old_irql: KIRQL,
}

impl KSpinLockGuard {
    /// Создает guard, захватывая спинлок
    ///
    /// # Safety
    /// Pointer должен быть валидным и жить дольше чем guard.
    #[inline]
    pub unsafe fn new(lock: *mut KSPIN_LOCK) -> Self {
        unsafe {
            let old_irql = ke_acquire_spin_lock(&*lock);
            Self { lock, old_irql }
        }
    }

    /// Создает guard, захватывая спинлок уже на DISPATCH_LEVEL
    ///
    /// # Safety
    /// - Pointer должен быть валидным
    /// - Вызывающий код уже должен быть на DISPATCH_LEVEL
    #[inline]
    pub unsafe fn new_at_dispatch(lock: *mut KSPIN_LOCK) -> Self {
        unsafe {
            ke_acquire_spin_lock_at_dpc_level(&*lock);
            Self {
                lock,
                old_irql: DISPATCH_LEVEL,
            }
        }
    }
}

impl Drop for KSpinLockGuard {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            ke_release_spin_lock(&*self.lock, self.old_irql);
        }
    }
}

// KSpinLockGuard не может быть Send (нельзя освобождать на другом CPU)
// но Sync не нужен т.к. guard создается и уничтожается на одном потоке
