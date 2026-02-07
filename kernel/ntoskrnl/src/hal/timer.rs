//! Timer Abstraction Layer
//!
//! Унифицированный интерфейс для работы с таймерами.
//! Автоматически выбирает лучший доступный источник.
//!
//! Источники:
//! - Clock: APIC Timer (обязателен)
//! - Performance counter: TSC (invariant) > HPET
//!
//! Источники:
//! - ReactOS: hal/halx86/generic/timer.c
//! - Intel SDM Vol. 3B, Chapter 17 (TSC)

#![allow(dead_code)]

use core::sync::atomic::AtomicBool;
use core::sync::atomic::AtomicU8;
use core::sync::atomic::AtomicU64;
use core::sync::atomic::Ordering;

// =============================================================================
// Timer Source Types
// =============================================================================

/// Источник performance counter
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PerfCounterSource {
    /// Не инициализирован
    None = 0,
    /// HPET
    Hpet = 1,
    /// TSC (Time Stamp Counter)
    Tsc = 2,
}

// =============================================================================
// TSC Support
// =============================================================================

/// TSC частота в Hz (после калибровки)
static TSC_FREQUENCY: AtomicU64 = AtomicU64::new(0);

/// TSC инвариантный (не меняется при изменении частоты CPU)
static TSC_INVARIANT: AtomicBool = AtomicBool::new(false);

/// TSC инициализирован
static TSC_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Проверяет поддержку TSC
pub fn is_tsc_supported() -> bool {
    use crate::arch::x86_64::cpu::cpuid;
    use crate::arch::x86_64::cpu::cpuid_features_edx;
    let result = cpuid(1, 0);
    (result.edx & cpuid_features_edx::TSC) != 0
}

/// Проверяет является ли TSC инвариантным
///
/// Invariant TSC не меняет частоту при:
/// - Изменении частоты CPU (SpeedStep, Turbo Boost)
/// - Переходе в C-states
///
/// Детектируется через CPUID 0x80000007, EDX bit 8
pub fn is_tsc_invariant() -> bool {
    use crate::arch::x86_64::cpu::cpuid;

    let max_ext = cpuid(0x80000000, 0).eax;
    if max_ext < 0x80000007 {
        return false;
    }

    let result = cpuid(0x80000007, 0);
    (result.edx & (1 << 8)) != 0
}

/// Читает TSC
#[inline]
pub fn read_tsc() -> u64 {
    crate::arch::x86_64::msr::rdtsc()
}

/// Калибрует TSC используя PIT как эталон
///
/// # Safety
/// Должен вызываться при инициализации с работающим PIT
pub unsafe fn tsc_calibrate() {
    const CALIBRATION_MS: u32 = 50;

    let tsc_start = read_tsc();
    super::pit::ke_stall_execution_processor(CALIBRATION_MS * 1000);
    let tsc_end = read_tsc();

    let tsc_elapsed = tsc_end - tsc_start;
    let frequency = (tsc_elapsed * 1000) / CALIBRATION_MS as u64;

    TSC_FREQUENCY.store(frequency, Ordering::Release);
    TSC_INVARIANT.store(is_tsc_invariant(), Ordering::Release);
    TSC_INITIALIZED.store(true, Ordering::Release);
}

/// Возвращает частоту TSC в Hz
#[inline]
pub fn tsc_get_frequency() -> u64 {
    TSC_FREQUENCY.load(Ordering::Acquire)
}

/// Проверяет инициализирован ли TSC
#[inline]
pub fn is_tsc_initialized() -> bool {
    TSC_INITIALIZED.load(Ordering::Acquire)
}

/// Проверяет можно ли использовать TSC для performance counter
pub fn is_tsc_usable_for_perf() -> bool {
    is_tsc_initialized() && TSC_INVARIANT.load(Ordering::Acquire)
}

/// Запрашивает performance counter через TSC
pub fn tsc_query_performance_counter() -> (u64, u64) {
    if !is_tsc_initialized() {
        return (0, 0);
    }
    (read_tsc(), TSC_FREQUENCY.load(Ordering::Acquire))
}

// =============================================================================
// Timer State
// =============================================================================

/// Текущий источник performance counter
static PERF_SOURCE: AtomicU8 = AtomicU8::new(PerfCounterSource::None as u8);

/// Возвращает текущий источник performance counter
#[inline]
pub fn get_perf_counter_source() -> PerfCounterSource {
    match PERF_SOURCE.load(Ordering::Acquire) {
        1 => PerfCounterSource::Hpet,
        2 => PerfCounterSource::Tsc,
        _ => PerfCounterSource::None,
    }
}

/// Устанавливает источник performance counter
pub fn set_perf_counter_source(source: PerfCounterSource) {
    PERF_SOURCE.store(source as u8, Ordering::Release);
}

// =============================================================================
// Unified Timer Interface
// =============================================================================

/// HalQueryTickCount - возвращает количество тиков с момента запуска
#[inline]
pub fn hal_query_tick_count() -> u64 {
    super::apic::apic_query_tick_count()
}

/// HalQueryTimeIncrement - возвращает интервал между тиками в 100ns
#[inline]
pub fn hal_query_time_increment() -> u32 {
    super::apic::apic_query_time_increment()
}

/// HalQueryPerformanceCounter - унифицированный высокоточный счетчик
///
/// Выбирает лучший источник: TSC > HPET
///
/// # Arguments
/// * `frequency` - если не null, записывает частоту в Hz
///
/// # Returns
/// Текущее значение счетчика
pub fn hal_query_perf_counter(frequency: *mut u64) -> u64 {
    let (counter, freq) = match get_perf_counter_source() {
        PerfCounterSource::Tsc => tsc_query_performance_counter(),
        PerfCounterSource::Hpet => super::hpet::hpet_query_performance_counter(),
        PerfCounterSource::None => (0, 0),
    };

    if !frequency.is_null() {
        unsafe {
            *frequency = freq;
        }
    }
    counter
}

// =============================================================================
// Timer Status
// =============================================================================

/// Полный статус подсистемы таймеров
#[derive(Debug, Clone)]
pub struct TimerStatus {
    pub perf_source: PerfCounterSource,
    pub tsc_supported: bool,
    pub tsc_invariant: bool,
    pub tsc_frequency: u64,
    pub apic_available: bool,
    pub apic_frequency: u64,
    pub hpet_available: bool,
    pub hpet_frequency: u64,
    pub tick_count: u64,
    pub time_increment: u32,
}

/// Возвращает полный статус подсистемы таймеров
pub fn get_timer_status() -> TimerStatus {
    TimerStatus {
        perf_source: get_perf_counter_source(),
        tsc_supported: is_tsc_supported(),
        tsc_invariant: TSC_INVARIANT.load(Ordering::Acquire),
        tsc_frequency: TSC_FREQUENCY.load(Ordering::Acquire),
        apic_available: super::apic::is_apic_initialized(),
        apic_frequency: super::apic::apic_get_timer_frequency(),
        hpet_available: super::hpet::is_hpet_initialized(),
        hpet_frequency: super::hpet::hpet_get_frequency(),
        tick_count: hal_query_tick_count(),
        time_increment: hal_query_time_increment(),
    }
}

/// Возвращает строковое описание источника perf counter
pub fn perf_source_name(source: PerfCounterSource) -> &'static str {
    match source {
        PerfCounterSource::None => "None",
        PerfCounterSource::Hpet => "HPET",
        PerfCounterSource::Tsc => "TSC",
    }
}
