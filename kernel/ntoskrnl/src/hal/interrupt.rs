//! HAL Interrupt Management
//!
//! Функции управления прерываниями для HAL.
//!
//! NT использует пару функций для работы с hardware interrupts:
//! - HalBeginSystemInterrupt - начало обработки прерывания
//! - HalEndSystemInterrupt - конец обработки прерывания
//!
//! Источники:
//! - ReactOS: hal/halx86/apic/apic.c
//! - NT5: hal/halx86/i386/ixisrsup.c

#![allow(dead_code)]

use crate::arch::x86_64::pcr;
// Re-export APIC_SPURIOUS_VECTOR for convenience
pub use crate::hal::apic::APIC_SPURIOUS_VECTOR;
use crate::hal::apic::apic_send_eoi;
use crate::hal::ioapic::ioapic_disable_gsi;
use crate::hal::ioapic::ioapic_enable_gsi;
use crate::hal::ioapic::ioapic_vector_to_gsi;
use crate::hal::ioapic::is_ioapic_initialized;
use crate::hal::irql::KIRQL;
use crate::hal::irql::ke_get_current_irql;
use crate::hal::irql::kf_lower_irql;
use crate::hal::irql::kf_raise_irql;

// =============================================================================
// Legacy PIC Constants (for spurious detection)
// =============================================================================

/// Legacy PIC IRQ7 (могут быть spurious на legacy системах)
pub const PIC_IRQ7: u8 = 0x07;

/// Legacy PIC IRQ15 (могут быть spurious на legacy системах)
pub const PIC_IRQ15: u8 = 0x0F;

// =============================================================================
// Interrupt Mode
// =============================================================================

/// Режим прерывания (Edge vs Level)
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InterruptMode {
    /// Level-sensitive interrupt (remains active until acknowledged by device)
    LevelSensitive = 0,
    /// Edge-triggered / Latched interrupt (single pulse)
    Latched = 1,
}

// =============================================================================
// HalBeginSystemInterrupt
// =============================================================================

/// HalBeginSystemInterrupt - начало обработки hardware interrupt
///
/// Вызывается в начале каждого hardware interrupt handler.
///
/// # Arguments
/// * `irql` - IRQL уровень для этого прерывания
/// * `vector` - номер вектора прерывания
/// * `old_irql` - указатель для сохранения предыдущего IRQL
///
/// # Returns
/// `true` если прерывание должно быть обработано
/// `false` если это spurious interrupt (не требует EOI)
///
/// # Windows x64 ABI
/// - RCX = irql
/// - RDX = vector
/// - R8 = old_irql
#[unsafe(no_mangle)]
pub extern "win64" fn hal_begin_system_interrupt(
    irql: KIRQL,
    vector: u8,
    old_irql: *mut KIRQL,
) -> bool {
    // Проверяем spurious interrupt от APIC
    if vector == APIC_SPURIOUS_VECTOR {
        // APIC spurious interrupt - не требует EOI, просто игнорируем
        return false;
    }

    // Проверяем spurious IRQ7/IRQ15 от legacy PIC (если используется)
    // На APIC системах эти векторы могут быть переназначены
    // TODO: добавить проверку ISR bits для legacy PIC если нужно

    // Сохраняем текущий IRQL
    let prev_irql = ke_get_current_irql();

    // Записываем предыдущий IRQL
    if !old_irql.is_null() {
        unsafe {
            *old_irql = prev_irql;
        }
    }

    // Debug: проверяем что новый IRQL >= текущий
    // Прерывания с более низким IRQL должны быть заблокированы
    debug_assert!(
        irql >= prev_irql,
        "HalBeginSystemInterrupt: irql {} < prev_irql {}",
        irql,
        prev_irql
    );

    // Повышаем IRQL если нужно
    if irql > prev_irql {
        kf_raise_irql(irql);
    }

    // Инкрементируем счетчик прерываний в PRCB
    unsafe {
        let prcb = pcr::get_prcb();
        if !prcb.is_null() {
            (*prcb).interrupt_count = (*prcb).interrupt_count.wrapping_add(1);
        }
    }

    true
}

// =============================================================================
// HalEndSystemInterrupt
// =============================================================================

/// HalEndSystemInterrupt - конец обработки hardware interrupt
///
/// Вызывается в конце каждого hardware interrupt handler.
///
/// # Arguments
/// * `old_irql` - IRQL для восстановления (из HalBeginSystemInterrupt)
/// * `send_eoi` - отправлять ли EOI в APIC
///
/// # Windows x64 ABI
/// - RCX = old_irql
/// - RDX = send_eoi (bool as u64)
#[unsafe(no_mangle)]
pub extern "win64" fn hal_end_system_interrupt(old_irql: KIRQL, send_eoi: bool) {
    // Отправляем EOI в APIC если требуется
    if send_eoi {
        apic_send_eoi();
    }

    // Понижаем IRQL
    // kf_lower_irql проверит pending software interrupts
    kf_lower_irql(old_irql);
}

// =============================================================================
// HalEnableSystemInterrupt
// =============================================================================

/// HalEnableSystemInterrupt - включает системное прерывание
///
/// # Arguments
/// * `vector` - номер вектора прерывания (0x30-0xFE для device interrupts)
/// * `irql` - IRQL уровень для этого прерывания
/// * `interrupt_mode` - режим прерывания (Edge/Level)
///
/// # Returns
/// `true` если успешно включено
///
/// # Windows x64 ABI
/// - RCX = vector
/// - RDX = irql
/// - R8 = interrupt_mode
#[unsafe(no_mangle)]
pub extern "win64" fn hal_enable_system_interrupt(
    vector: u8,
    _irql: KIRQL,
    _interrupt_mode: InterruptMode,
) -> bool {
    // Проверяем инициализирован ли IOAPIC
    if !is_ioapic_initialized() {
        // IOAPIC не инициализирован - предполагаем что вектор уже настроен
        return true;
    }

    // Ищем GSI для этого вектора
    if let Some(gsi) = ioapic_vector_to_gsi(vector) {
        // Найден GSI - включаем в IOAPIC
        ioapic_enable_gsi(gsi);
        return true;
    }

    // GSI не найден - вектор может быть software или APIC-only
    // (APC, DPC, IPI, Timer, etc.)
    true
}

// =============================================================================
// HalDisableSystemInterrupt
// =============================================================================

/// HalDisableSystemInterrupt - отключает системное прерывание
///
/// # Arguments
/// * `vector` - номер вектора прерывания
///
/// # Windows x64 ABI
/// - RCX = vector
#[unsafe(no_mangle)]
pub extern "win64" fn hal_disable_system_interrupt(vector: u8) {
    // Проверяем инициализирован ли IOAPIC
    if !is_ioapic_initialized() {
        return;
    }

    // Ищем GSI для этого вектора
    if let Some(gsi) = ioapic_vector_to_gsi(vector) {
        // Найден GSI - отключаем в IOAPIC
        ioapic_disable_gsi(gsi);
    }
}

// =============================================================================
// HalGetInterruptVector (placeholder for drivers)
// =============================================================================

/// HalGetInterruptVector - получает вектор для устройства
///
/// Используется драйверами для получения вектора прерывания.
///
/// # Arguments
/// * `bus_type` - тип шины (ISA, PCI, etc.)
/// * `bus_number` - номер шины
/// * `bus_interrupt_level` - уровень прерывания на шине (IRQ number)
/// * `bus_interrupt_vector` - вектор прерывания на шине
/// * `irql` - указатель для получения IRQL
/// * `affinity` - указатель для получения affinity mask
///
/// # Returns
/// Системный вектор прерывания
#[unsafe(no_mangle)]
pub extern "win64" fn hal_get_interrupt_vector(
    _bus_type: u32,
    _bus_number: u32,
    bus_interrupt_level: u32,
    _bus_interrupt_vector: u32,
    irql: *mut KIRQL,
    _affinity: *mut u64,
) -> u32 {
    // Простая реализация: ISA IRQ -> Vector mapping
    // Vector = 0x30 + IRQ (для устройств)
    let isa_irq = bus_interrupt_level as u8;
    let vector = 0x30u8.saturating_add(isa_irq);

    // Вычисляем IRQL для устройства
    // Device IRQL = DISPATCH_LEVEL + 1 + (15 - IRQ/16)
    // Упрощенно: все device IRQs на уровне 3-12
    let device_irql = if isa_irq < 16 {
        // ISA IRQs: IRQL = 12 - (irq / 2)
        (12 - (isa_irq / 2)).max(3)
    } else {
        // Higher IRQs: фиксированный IRQL
        3
    };

    if !irql.is_null() {
        unsafe {
            *irql = device_irql;
        }
    }

    vector as u32
}

// =============================================================================
// HalRequestIpi (placeholder for SMP)
// =============================================================================

/// HalRequestIpi - отправляет IPI (Inter-Processor Interrupt)
///
/// Используется для уведомления других процессоров.
///
/// # Arguments
/// * `target_processors` - битовая маска целевых процессоров
///
/// # Note
/// Текущая реализация - stub для uniprocessor системы.
/// SMP реализация будет использовать APIC ICR.
#[unsafe(no_mangle)]
pub extern "win64" fn hal_request_ipi(_target_processors: u64) {
    // TODO: SMP implementation
    // Для uniprocessor - ничего не делаем
}

// =============================================================================
// Spurious Interrupt Check
// =============================================================================

/// Проверяет является ли прерывание spurious
///
/// APIC может генерировать spurious interrupts в определенных условиях:
/// - Race condition между маскированием и доставкой
/// - Electrical noise на interrupt lines
#[inline]
pub fn is_spurious_interrupt(vector: u8) -> bool {
    vector == APIC_SPURIOUS_VECTOR
}

/// Проверяет In-Service Register для spurious detection
///
/// Для level-triggered interrupts можно проверить ISR bit
pub fn check_isr_for_vector(vector: u8) -> bool {
    // ISR registers: 0x100-0x170 (8 registers, 32 bits each)
    // ISR base = 0x100, каждый регистр на +0x10
    let isr_index = (vector as u32) / 32;
    let isr_bit = (vector as u32) % 32;

    if isr_index >= 8 {
        return false;
    }

    // Читаем ISR регистр напрямую через MMIO
    // ISR[n] = 0x100 + n * 0x10
    let isr_offset = 0x100 + (isr_index * 0x10);

    use crate::hal::apic::LOCAL_APIC_BASE;

    let isr_value =
        unsafe { core::ptr::read_volatile((LOCAL_APIC_BASE + isr_offset as u64) as *const u32) };

    (isr_value & (1 << isr_bit)) != 0
}
