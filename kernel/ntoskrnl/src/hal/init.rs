//! HAL Initialization
//!
//! Инициализация Hardware Abstraction Layer.
//!
//! Источники:
//! - ReactOS: hal/halx86/generic/halinit.c

use core::sync::atomic::AtomicU32;
use core::sync::atomic::Ordering;

use super::apic;
use super::hpet;
use super::ioapic;
use super::pic;
use super::processor;
use super::timer;

// =============================================================================
// HAL State
// =============================================================================

/// Фаза инициализации HAL
static HAL_INIT_PHASE: AtomicU32 = AtomicU32::new(0);

/// Флаг успешной инициализации
static HAL_INITIALIZED: AtomicU32 = AtomicU32::new(0);

// =============================================================================
// HalInitSystem
// =============================================================================

/// HalInitSystem - главная функция инициализации HAL
///
/// Вызывается в Phase 0 и Phase 1.
///
/// # Arguments
/// * `phase` - фаза инициализации (0 или 1)
///
/// # Returns
/// true если инициализация успешна
pub fn hal_init_system(phase: u32) -> bool {
    match phase {
        0 => hal_init_phase0(),
        1 => hal_init_phase1(),
        _ => false,
    }
}

/// Phase 0: Ранняя инициализация
///
/// - Инициализация PIC
/// - Базовая настройка таймера
fn hal_init_phase0() -> bool {
    // Инициализируем PIC
    pic::halp_initialize_legacy_pics();

    // Инициализируем процессор
    processor::hal_initialize_processor(0);

    HAL_INIT_PHASE.store(1, Ordering::Release);
    true
}

/// Phase 1: Основная инициализация
///
/// - Калибровка TSC
/// - Инициализация APIC Timer
/// - Инициализация IOAPIC
/// - Выбор источника performance counter
fn hal_init_phase1() -> bool {
    // 1. Калибруем TSC пока PIT доступен
    if timer::is_tsc_supported() {
        unsafe {
            timer::tsc_calibrate();
        }
    }

    // 2. Инициализируем APIC Timer (обязателен)
    if !apic::is_apic_supported() || !apic::is_apic_enabled() {
        panic!("APIC required but not available");
    }

    unsafe {
        apic::apic_map_local_apic();
        apic::apic_initialize_local_apic(0);
        apic::apic_disable_legacy_pic();
        apic::apic_calibrate_timer();
        apic::apic_initialize_clock(apic::APIC_DEFAULT_INTERVAL_100NS);
    }

    // 3. Инициализируем IOAPIC
    // Используем MADT конфигурацию если доступна, иначе стандартный адрес
    const IOAPIC_DEFAULT_PHYS: u64 = 0xFEC00000;
    const IOAPIC_DEFAULT_GSI_BASE: u32 = 0;

    // Проверяем что HAL MMIO инициализирован
    if crate::mm::hal_mmio::is_hal_mmio_initialized() {
        let (ioapic_addr, gsi_base) = ioapic::ioapic_get_madt_config()
            .unwrap_or((IOAPIC_DEFAULT_PHYS, IOAPIC_DEFAULT_GSI_BASE));

        unsafe {
            if ioapic::ioapic_initialize(ioapic_addr, gsi_base) {
                // Применяем ISA IRQ overrides из ACPI MADT
                ioapic::ioapic_apply_madt_overrides();
            }
        }
    }

    // 4. Выбираем performance counter source: TSC > HPET
    if timer::is_tsc_usable_for_perf() {
        timer::set_perf_counter_source(timer::PerfCounterSource::Tsc);
    } else if hpet::is_hpet_initialized() {
        timer::set_perf_counter_source(timer::PerfCounterSource::Hpet);
    }

    // HAL Phase 1 завершен - IRQL управляется вызывающим кодом.
    // В NT Phase 1 инициализация обычно выполняется в system thread на PASSIVE_LEVEL,
    // поэтому HAL не должен требовать фиксированного IRQL здесь.

    HAL_INIT_PHASE.store(2, Ordering::Release);
    HAL_INITIALIZED.store(1, Ordering::Release);

    true
}

// =============================================================================
// HAL Query Functions
// =============================================================================

/// Возвращает фазу инициализации HAL
#[inline]
pub fn hal_get_init_phase() -> u32 {
    HAL_INIT_PHASE.load(Ordering::Acquire)
}

/// Проверяет инициализирован ли HAL
#[inline]
pub fn is_hal_initialized() -> bool {
    HAL_INITIALIZED.load(Ordering::Acquire) != 0
}

/// Проверяет инициализирован ли HPET
#[inline]
pub fn hal_is_hpet_available() -> bool {
    hpet::is_hpet_initialized()
}

// =============================================================================
// HPET Initialization
// =============================================================================

/// Инициализирует HPET из ACPI информации
///
/// Должна вызываться после инициализации ACPI и MM.
///
/// # Arguments
/// * `hpet_info` - информация из ACPI HPET таблицы
///
/// # Returns
/// true если HPET успешно инициализирован
pub fn hal_initialize_hpet(hpet_info: &crate::acpi::HpetInfo) -> bool {
    unsafe { hpet::hpet_initialize(hpet_info) }
}

// =============================================================================
// Software Interrupts
// =============================================================================

// Software interrupts (APC/DPC) реализованы в hal/swint.rs
// Используют PRCB флаги и self-IPI вместо глобальных AtomicU32

/// Флаг отложенного dispatch (quantum end, context switch)
/// Используется совместно с PRCB.quantum_end
static PENDING_DISPATCH: AtomicU32 = AtomicU32::new(0);

/// Проверяет есть ли отложенный APC
#[inline]
pub fn hal_is_apc_pending() -> bool {
    super::swint::is_apc_interrupt_pending()
}

/// Проверяет есть ли отложенный DPC
#[inline]
pub fn hal_is_dpc_pending() -> bool {
    super::swint::is_dpc_interrupt_pending()
}

/// Проверяет есть ли отложенный dispatch (quantum end)
#[inline]
pub fn hal_is_dispatch_pending() -> bool {
    PENDING_DISPATCH.load(Ordering::Acquire) != 0
}

/// Устанавливает флаг отложенного dispatch
#[inline]
pub fn hal_request_dispatch() {
    PENDING_DISPATCH.store(1, Ordering::Release);
}

/// Очищает флаг отложенного dispatch
#[inline]
pub fn hal_clear_dispatch() {
    PENDING_DISPATCH.store(0, Ordering::Release);
}

// =============================================================================
// Interrupt Handlers Support
// =============================================================================

/// HalBeginSystemInterrupt - начало обработки hardware interrupt
///
/// # Arguments
/// * `irql` - IRQL прерывания
/// * `vector` - номер вектора
/// * `old_irql` - указатель для сохранения предыдущего IRQL
///
/// # Returns
/// true если прерывание должно быть обработано
pub fn hal_begin_system_interrupt(irql: u8, vector: u8, old_irql: *mut u8) -> bool {
    // Проверяем spurious interrupt для IRQ7/IRQ15
    if let Some(irq) = pic::vector_to_irq(vector) {
        if pic::hal_is_spurious_irq(irq) {
            return false;
        }
    }

    // Сохраняем и повышаем IRQL
    let prev_irql = super::irql::kf_raise_irql(irql);
    if !old_irql.is_null() {
        unsafe {
            *old_irql = prev_irql;
        }
    }

    true
}

/// HalEndSystemInterrupt - завершение обработки hardware interrupt
///
/// # Arguments
/// * `old_irql` - IRQL для восстановления
pub fn hal_end_system_interrupt(old_irql: u8) {
    // Отправляем EOI
    // Вектор уже известен обработчику, который должен вызвать hal_end_of_interrupt

    // Понижаем IRQL
    super::irql::kf_lower_irql(old_irql);
}

// =============================================================================
// HAL Interrupt Registration
// =============================================================================

/// Тип callback функции для hardware interrupt
pub type HalInterruptCallback = fn(vector: u8, context: *mut core::ffi::c_void);

/// Зарегистрированные обработчики прерываний
static mut INTERRUPT_HANDLERS: [Option<(HalInterruptCallback, *mut core::ffi::c_void)>; 256] =
    [None; 256];

/// Регистрирует обработчик прерывания
///
/// # Safety
/// Не thread-safe, должен вызываться при инициализации
pub unsafe fn hal_register_interrupt_handler(
    vector: u8,
    handler: HalInterruptCallback,
    context: *mut core::ffi::c_void,
) {
    unsafe {
        INTERRUPT_HANDLERS[vector as usize] = Some((handler, context));
    }
}

/// Вызывает зарегистрированный обработчик
pub fn hal_dispatch_interrupt(vector: u8) {
    unsafe {
        if let Some((handler, context)) = INTERRUPT_HANDLERS[vector as usize] {
            handler(vector, context);
        }
    }
}
