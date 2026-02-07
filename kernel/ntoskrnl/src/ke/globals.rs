//! KE Globals - глобальные переменные ядра
//!
//! Источники:
//! - NT5: ke/amd64/ctxswap.asm, ke/thrdschd.c
//! - ReactOS: ke/thrdschd.c

#![allow(dead_code)]

use core::sync::atomic::AtomicPtr;
use core::sync::atomic::AtomicU64;
use core::sync::atomic::Ordering;

use ntldr::LOADER_PARAMETER_BLOCK;

use crate::arch::x86_64::pcr::KPRCB;

/// Максимальное количество процессоров
pub const MAX_PROCESSORS: usize = 64;

/// KiProcessorBlock - массив указателей на PRCB всех процессоров
///
/// Инициализируется при старте каждого процессора.
/// KiProcessorBlock[N] содержит указатель на PRCB процессора N.
static mut KI_PROCESSOR_BLOCK: [*mut KPRCB; MAX_PROCESSORS] =
    [core::ptr::null_mut(); MAX_PROCESSORS];

/// Получает указатель на PRCB процессора по номеру
///
/// # Safety
/// Вызывающий должен гарантировать что процессор инициализирован
#[inline]
pub unsafe fn ki_processor_block(processor: usize) -> *mut KPRCB {
    unsafe {
        if processor < MAX_PROCESSORS {
            KI_PROCESSOR_BLOCK[processor]
        } else {
            core::ptr::null_mut()
        }
    }
}

/// Устанавливает указатель на PRCB процессора
///
/// # Safety
/// Должен вызываться только при инициализации процессора
#[inline]
pub unsafe fn ki_set_processor_block(processor: usize, prcb: *mut KPRCB) {
    unsafe {
        if processor < MAX_PROCESSORS {
            KI_PROCESSOR_BLOCK[processor] = prcb;
        }
    }
}

/// Количество активных процессоров
pub static KE_NUMBER_PROCESSORS: AtomicU64 = AtomicU64::new(1);

// =============================================================================
// KeLoaderBlock - указатель на LOADER_PARAMETER_BLOCK
// =============================================================================

/// KeLoaderBlock - глобальный указатель на LOADER_PARAMETER_BLOCK
///
/// В NT используется для доступа к информации загрузчика из разных подсистем.
/// Устанавливается в KiSystemStartup, валиден до конца Phase 1 инициализации.
static KE_LOADER_BLOCK: AtomicPtr<LOADER_PARAMETER_BLOCK> = AtomicPtr::new(core::ptr::null_mut());

/// Устанавливает указатель на LoaderBlock
///
/// # Safety
/// Должен вызываться один раз в начале KiSystemStartup
pub unsafe fn ke_set_loader_block(lpb: *const LOADER_PARAMETER_BLOCK) {
    KE_LOADER_BLOCK.store(lpb as *mut _, Ordering::Release);
}

/// Возвращает указатель на LoaderBlock
///
/// Валиден только до конца Phase 1 инициализации.
#[inline]
pub fn ke_get_loader_block() -> *const LOADER_PARAMETER_BLOCK {
    KE_LOADER_BLOCK.load(Ordering::Acquire)
}

/// Получает количество активных процессоров
#[inline]
pub fn ke_query_active_processor_count() -> u32 {
    KE_NUMBER_PROCESSORS.load(Ordering::Relaxed) as u32
}

/// LOW_REALTIME_PRIORITY - граница real-time приоритетов
///
/// Потоки с приоритетом >= LOW_REALTIME_PRIORITY считаются real-time
/// и не подвержены priority decay и некоторым другим механизмам.
pub const LOW_REALTIME_PRIORITY: i32 = 16;

/// MAXIMUM_PRIORITY - максимальный приоритет
pub const MAXIMUM_PRIORITY: i32 = 32;

/// PRIORITY_LEVELS - количество уровней приоритета
pub const PRIORITY_LEVELS: usize = 32;
