//! HPET - High Precision Event Timer
//!
//! Реализация HPET для high-precision performance counter.
//!
//! Источники:
//! - IA-PC HPET Specification (Intel)
//! - OSDev Wiki: https://wiki.osdev.org/HPET
//! - ReactOS: hal/halx86/apic/hpet.c (частично)

#![allow(dead_code)]

use core::sync::atomic::AtomicBool;
use core::sync::atomic::AtomicU64;
use core::sync::atomic::Ordering;

// =============================================================================
// HPET Register Offsets
// =============================================================================

/// Регистры HPET (смещения от базового адреса)
#[repr(u64)]
#[derive(Clone, Copy, Debug)]
pub enum HpetRegister {
    /// General Capabilities and ID Register (read-only)
    /// Bits 0-7: REV_ID (revision)
    /// Bits 8-12: NUM_TIM_CAP (number of timers - 1)
    /// Bit 13: COUNT_SIZE_CAP (1 = 64-bit counter)
    /// Bit 15: LEG_RT_CAP (legacy replacement capable)
    /// Bits 16-31: VENDOR_ID
    /// Bits 32-63: COUNTER_CLK_PERIOD (femtoseconds per tick)
    GeneralCapabilities = 0x000,

    /// General Configuration Register
    /// Bit 0: ENABLE_CNF (enable main counter)
    /// Bit 1: LEG_RT_CNF (legacy replacement mapping)
    GeneralConfiguration = 0x010,

    /// General Interrupt Status Register
    GeneralInterruptStatus = 0x020,

    /// Main Counter Value Register (64-bit)
    MainCounterValue = 0x0F0,

    /// Timer 0 Configuration and Capability Register
    Timer0Config = 0x100,
    /// Timer 0 Comparator Value Register
    Timer0Comparator = 0x108,
    /// Timer 0 FSB Interrupt Route Register
    Timer0FsbRoute = 0x110,

    /// Timer 1 Configuration and Capability Register
    Timer1Config = 0x120,
    /// Timer 1 Comparator Value Register
    Timer1Comparator = 0x128,
    /// Timer 1 FSB Interrupt Route Register
    Timer1FsbRoute = 0x130,

    /// Timer 2 Configuration and Capability Register
    Timer2Config = 0x140,
    /// Timer 2 Comparator Value Register
    Timer2Comparator = 0x148,
    /// Timer 2 FSB Interrupt Route Register
    Timer2FsbRoute = 0x150,
}

// =============================================================================
// General Capabilities Register Bits
// =============================================================================

/// Маска для REV_ID (bits 0-7)
pub const HPET_CAP_REV_ID_MASK: u64 = 0xFF;

/// Маска для NUM_TIM_CAP (bits 8-12)
pub const HPET_CAP_NUM_TIM_MASK: u64 = 0x1F00;
pub const HPET_CAP_NUM_TIM_SHIFT: u64 = 8;

/// Bit 13: COUNT_SIZE_CAP (1 = 64-bit counter)
pub const HPET_CAP_COUNT_SIZE: u64 = 1 << 13;

/// Bit 15: LEG_RT_CAP (legacy replacement capable)
pub const HPET_CAP_LEG_RT: u64 = 1 << 15;

/// Маска для VENDOR_ID (bits 16-31)
pub const HPET_CAP_VENDOR_ID_MASK: u64 = 0xFFFF0000;
pub const HPET_CAP_VENDOR_ID_SHIFT: u64 = 16;

/// Маска для COUNTER_CLK_PERIOD (bits 32-63) - femtoseconds per tick
pub const HPET_CAP_PERIOD_MASK: u64 = 0xFFFFFFFF00000000;
pub const HPET_CAP_PERIOD_SHIFT: u64 = 32;

// =============================================================================
// General Configuration Register Bits
// =============================================================================

/// Bit 0: ENABLE_CNF - enable main counter
pub const HPET_CFG_ENABLE: u64 = 1 << 0;

/// Bit 1: LEG_RT_CNF - legacy replacement routing
pub const HPET_CFG_LEGACY_RT: u64 = 1 << 1;

// =============================================================================
// Timer Configuration Register Bits
// =============================================================================

/// Bit 1: Timer Interrupt Type (0 = edge, 1 = level)
pub const HPET_TIM_INT_TYPE_LEVEL: u64 = 1 << 1;

/// Bit 2: Timer Interrupt Enable
pub const HPET_TIM_INT_ENABLE: u64 = 1 << 2;

/// Bit 3: Timer Type (0 = one-shot, 1 = periodic)
pub const HPET_TIM_TYPE_PERIODIC: u64 = 1 << 3;

/// Bit 4: Periodic Interrupt Capable (read-only)
pub const HPET_TIM_PERIODIC_CAP: u64 = 1 << 4;

/// Bit 5: Timer Size (0 = 32-bit, 1 = 64-bit) (read-only)
pub const HPET_TIM_SIZE_64BIT: u64 = 1 << 5;

/// Bit 6: Timer Value Set (for periodic mode)
pub const HPET_TIM_VAL_SET: u64 = 1 << 6;

/// Bit 14: FSB Interrupt Delivery Capable (read-only)
pub const HPET_TIM_FSB_CAP: u64 = 1 << 14;

/// Bit 15: FSB Interrupt Enable
pub const HPET_TIM_FSB_ENABLE: u64 = 1 << 15;

// =============================================================================
// HPET State
// =============================================================================

/// Виртуальный базовый адрес HPET
static HPET_BASE_ADDRESS: AtomicU64 = AtomicU64::new(0);

/// Период счетчика в фемтосекундах (10^-15 секунд)
static HPET_PERIOD_FEMTO: AtomicU64 = AtomicU64::new(0);

/// Частота HPET в Hz (вычисляется из периода)
static HPET_FREQUENCY: AtomicU64 = AtomicU64::new(0);

/// HPET инициализирован
static HPET_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Количество таймеров
static HPET_NUM_TIMERS: AtomicU64 = AtomicU64::new(0);

/// 64-битный счетчик
static HPET_IS_64BIT: AtomicBool = AtomicBool::new(false);

// =============================================================================
// HPET Register Access
// =============================================================================

/// Читает регистр HPET
#[inline]
pub fn hpet_read(reg: HpetRegister) -> u64 {
    let base = HPET_BASE_ADDRESS.load(Ordering::Acquire);
    if base == 0 {
        return 0;
    }
    unsafe { core::ptr::read_volatile((base + reg as u64) as *const u64) }
}

/// Записывает регистр HPET
#[inline]
pub fn hpet_write(reg: HpetRegister, value: u64) {
    let base = HPET_BASE_ADDRESS.load(Ordering::Acquire);
    if base == 0 {
        return;
    }
    unsafe { core::ptr::write_volatile((base + reg as u64) as *mut u64, value) }
}

// =============================================================================
// HPET Detection and Initialization
// =============================================================================

/// Проверяет инициализирован ли HPET
#[inline]
pub fn is_hpet_initialized() -> bool {
    HPET_INITIALIZED.load(Ordering::Acquire)
}

/// Возвращает частоту HPET в Hz
#[inline]
pub fn hpet_get_frequency() -> u64 {
    HPET_FREQUENCY.load(Ordering::Acquire)
}

/// Возвращает период HPET в фемтосекундах
#[inline]
pub fn hpet_get_period_femto() -> u64 {
    HPET_PERIOD_FEMTO.load(Ordering::Acquire)
}

/// Инициализирует HPET
///
/// # Arguments
/// * `hpet_info` - информация из ACPI HPET таблицы
///
/// # Safety
/// Должен вызываться после инициализации MM и HHDM
pub unsafe fn hpet_initialize(hpet_info: &crate::acpi::HpetInfo) -> bool {
    // Получаем HHDM offset для преобразования физического адреса
    let hhdm_offset = crate::mm::init::mm_get_hhdm_offset() as u64;
    let hpet_phys = hpet_info.base_address as u64;
    let hpet_virt = hhdm_offset + hpet_phys;

    HPET_BASE_ADDRESS.store(hpet_virt, Ordering::Release);

    // Читаем General Capabilities
    let caps = hpet_read(HpetRegister::GeneralCapabilities);

    // Извлекаем период в фемтосекундах
    let period_femto = (caps & HPET_CAP_PERIOD_MASK) >> HPET_CAP_PERIOD_SHIFT;
    if period_femto == 0 || period_femto > 100_000_000 {
        // Невалидный период (должен быть <= 100ns = 100_000_000 fs)
        HPET_BASE_ADDRESS.store(0, Ordering::Release);
        return false;
    }
    HPET_PERIOD_FEMTO.store(period_femto, Ordering::Release);

    // Вычисляем частоту: frequency = 10^15 / period_femto
    let frequency = 1_000_000_000_000_000u64 / period_femto;
    HPET_FREQUENCY.store(frequency, Ordering::Release);

    // Количество таймеров
    let num_timers = ((caps & HPET_CAP_NUM_TIM_MASK) >> HPET_CAP_NUM_TIM_SHIFT) + 1;
    HPET_NUM_TIMERS.store(num_timers, Ordering::Release);

    // 64-битный счетчик?
    let is_64bit = (caps & HPET_CAP_COUNT_SIZE) != 0;
    HPET_IS_64BIT.store(is_64bit, Ordering::Release);

    // Останавливаем счетчик для настройки
    let mut config = hpet_read(HpetRegister::GeneralConfiguration);
    config &= !HPET_CFG_ENABLE;
    hpet_write(HpetRegister::GeneralConfiguration, config);

    // Сбрасываем счетчик в 0
    hpet_write(HpetRegister::MainCounterValue, 0);

    // Отключаем legacy replacement (мы используем APIC)
    config &= !HPET_CFG_LEGACY_RT;

    // Включаем счетчик
    config |= HPET_CFG_ENABLE;
    hpet_write(HpetRegister::GeneralConfiguration, config);

    HPET_INITIALIZED.store(true, Ordering::Release);
    true
}

// =============================================================================
// HPET Counter Reading
// =============================================================================

/// Читает текущее значение счетчика HPET
#[inline]
pub fn hpet_read_counter() -> u64 {
    hpet_read(HpetRegister::MainCounterValue)
}

/// Конвертирует тики HPET в наносекунды
#[inline]
pub fn hpet_ticks_to_nanos(ticks: u64) -> u64 {
    let period = HPET_PERIOD_FEMTO.load(Ordering::Acquire);
    if period == 0 {
        return 0;
    }
    // nanos = ticks * period_femto / 10^6
    // Используем 128-bit умножение для избежания переполнения
    let product = (ticks as u128) * (period as u128);
    (product / 1_000_000) as u64
}

/// Конвертирует наносекунды в тики HPET
#[inline]
pub fn hpet_nanos_to_ticks(nanos: u64) -> u64 {
    let period = HPET_PERIOD_FEMTO.load(Ordering::Acquire);
    if period == 0 {
        return 0;
    }
    // ticks = nanos * 10^6 / period_femto
    let product = (nanos as u128) * 1_000_000;
    (product / period as u128) as u64
}

// =============================================================================
// KeQueryPerformanceCounter Support
// =============================================================================

/// Запрашивает performance counter через HPET
///
/// Возвращает (counter_value, frequency)
pub fn hpet_query_performance_counter() -> (u64, u64) {
    if !is_hpet_initialized() {
        return (0, 0);
    }

    let counter = hpet_read_counter();
    let frequency = HPET_FREQUENCY.load(Ordering::Acquire);

    (counter, frequency)
}

// =============================================================================
// HPET Information
// =============================================================================

/// Информация о HPET для отладки
#[derive(Debug, Clone, Copy)]
pub struct HpetStatus {
    pub initialized: bool,
    pub base_address: u64,
    pub frequency_hz: u64,
    pub period_femto: u64,
    pub num_timers: u64,
    pub is_64bit: bool,
    pub current_counter: u64,
}

/// Возвращает текущий статус HPET
pub fn hpet_get_status() -> HpetStatus {
    HpetStatus {
        initialized: HPET_INITIALIZED.load(Ordering::Acquire),
        base_address: HPET_BASE_ADDRESS.load(Ordering::Acquire),
        frequency_hz: HPET_FREQUENCY.load(Ordering::Acquire),
        period_femto: HPET_PERIOD_FEMTO.load(Ordering::Acquire),
        num_timers: HPET_NUM_TIMERS.load(Ordering::Acquire),
        is_64bit: HPET_IS_64BIT.load(Ordering::Acquire),
        current_counter: if HPET_INITIALIZED.load(Ordering::Acquire) {
            hpet_read_counter()
        } else {
            0
        },
    }
}
