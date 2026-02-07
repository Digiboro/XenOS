//! IRQL - Interrupt Request Level Management (AMD64)
//!
//! Цель: NT-совместимые инварианты:
//! - IRQL согласован между CR8 и PCR->Irql
//! - DPC/APC обработчики выполняются на гарантированном IRQL
//! - KfLowerIrql не исполняет dispatcher "вручную", а запрашивает SW interrupt

use crate::arch::x86_64::cpu;
use crate::arch::x86_64::pcr;

/// KIRQL - Kernel Interrupt Request Level
pub type KIRQL = u8;

pub const PASSIVE_LEVEL: KIRQL = 0;
pub const LOW_LEVEL: KIRQL = 0;
pub const APC_LEVEL: KIRQL = 1;
pub const DISPATCH_LEVEL: KIRQL = 2;
pub const SYNCH_LEVEL: KIRQL = DISPATCH_LEVEL;

pub const DEVICE_IRQL_BASE: KIRQL = 3;
pub const CLOCK_LEVEL: KIRQL = 13;
pub const IPI_LEVEL: KIRQL = 14;
/// POWER_LEVEL - power management IRQL (AMD64: alias IPI_LEVEL)
pub const POWER_LEVEL: KIRQL = IPI_LEVEL;
pub const PROFILE_LEVEL: KIRQL = 15;
pub const HIGH_LEVEL: KIRQL = 15;

#[inline]
pub fn ke_get_current_irql() -> KIRQL {
    unsafe { pcr::get_current_irql() }
}

#[inline]
pub fn kf_raise_irql(new_irql: KIRQL) -> KIRQL {
    let old_irql = unsafe { pcr::get_current_irql() };

    debug_assert!(
        new_irql >= old_irql,
        "KfRaiseIrql: new_irql ({}) < old_irql ({})",
        new_irql,
        old_irql
    );

    // DEBUG: отключено - слишком много шума
    // crate::ke::debug::debug_raw("[IRQL] Raise: ");
    // crate::ke::debug::debug_dec(old_irql as u64);
    // crate::ke::debug::debug_raw(" -> ");
    // crate::ke::debug::debug_dec(new_irql as u64);
    // crate::ke::debug::debug_raw("\n");

    unsafe {
        cpu::write_cr8(new_irql as u64);
        pcr::set_current_irql(new_irql);
    }
    old_irql
}

#[inline]
pub fn kf_lower_irql(new_irql: KIRQL) {
    let old_irql = unsafe { pcr::get_current_irql() };

    debug_assert!(
        new_irql <= old_irql,
        "KfLowerIrql: new_irql ({}) > old_irql ({})",
        new_irql,
        old_irql
    );

    // DEBUG: отключено - слишком много шума
    // crate::ke::debug::debug_raw("[IRQL] Lower: ");
    // crate::ke::debug::debug_dec(old_irql as u64);
    // crate::ke::debug::debug_raw(" -> ");
    // crate::ke::debug::debug_dec(new_irql as u64);
    // crate::ke::debug::debug_raw("\n");

    // 1) Обновляем PCR->Irql (software view)
    unsafe { pcr::set_current_irql(new_irql) };

    // 2) Понижаем CR8 (hardware view)
    unsafe { cpu::write_cr8(new_irql as u64) };

    // 3) NT-паттерн: на понижении IRQL мы НЕ исполняем dispatcher напрямую,
    // а запрашиваем доставку software interrupt (если есть pending работа).
    //
    // ВАЖНО: управление флагом IF (sti/cli) не является частью семантики IRQL.
    // В Windows `KfLowerIrql`/`KeLowerIrql` не "включают прерывания".
    // IF восстанавливается архитектурно (iret) или управляется вызывающим кодом.
    if new_irql < DISPATCH_LEVEL && old_irql >= DISPATCH_LEVEL {
        let has_pending = crate::hal::init::hal_is_dispatch_pending()
            || crate::hal::init::hal_is_dpc_pending()
            || crate::hal::swint::is_dpc_interrupt_pending();

        if has_pending {
            crate::hal::swint::hal_request_software_interrupt(DISPATCH_LEVEL);
        }
    }

    // 4) Если мы понизились ниже APC_LEVEL — разрешаем доставку APC через SW interrupt.
    if new_irql < APC_LEVEL && old_irql >= APC_LEVEL {
        // В будущем: KiCheckForKernelApcDelivery / KiCheckForUserApcDelivery.
        // Сейчас — минимальный NT-совместимый механизм: запросить APC SW interrupt
        // если он уже помечен как pending на потоке/PRCB.
        if crate::hal::swint::is_apc_interrupt_pending() {
            crate::hal::swint::hal_request_software_interrupt(APC_LEVEL);
        }
    }
}

#[inline]
pub fn ke_raise_irql(new_irql: KIRQL, old_irql: *mut KIRQL) {
    let prev = kf_raise_irql(new_irql);
    if !old_irql.is_null() {
        unsafe { *old_irql = prev };
    }
}

#[inline]
pub fn ke_lower_irql(new_irql: KIRQL) {
    kf_lower_irql(new_irql);
}

#[inline]
pub fn ke_raise_irql_to_dpc_level() -> KIRQL {
    kf_raise_irql(DISPATCH_LEVEL)
}

#[inline]
pub fn ke_raise_irql_to_synch_level() -> KIRQL {
    kf_raise_irql(SYNCH_LEVEL)
}

#[inline]
pub unsafe fn disable_interrupts() {
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack, preserves_flags));
    }
}

#[inline]
pub unsafe fn enable_interrupts() {
    unsafe {
        core::arch::asm!("sti", options(nomem, nostack, preserves_flags));
    }
}

// =============================================================================
// Пролог/эпилог software interrupt stubs (KiApcInterrupt/KiDispatchInterrupt)
// =============================================================================

/// Входит в software interrupt на указанном IRQL.
///
/// Вызывается из ASM-stub перед передачей управления Rust handler'у.
/// Должен синхронизировать CR8 и PCR->Irql и вернуть предыдущий IRQL для восстановления.
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn KiEnterSoftwareInterrupt(new_irql: KIRQL) -> KIRQL {
    let old_irql = unsafe { pcr::get_current_irql() };
    unsafe {
        cpu::write_cr8(new_irql as u64);
        pcr::set_current_irql(new_irql);
    }
    old_irql
}

/// Выходит из software interrupt и восстанавливает IRQL.
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn KiExitSoftwareInterrupt(old_irql: KIRQL) {
    unsafe {
        pcr::set_current_irql(old_irql);
        cpu::write_cr8(old_irql as u64);
    }
    // NB: Не трогаем IF. Возврат из прерывания (iretq) восстановит флаги.
}

// =============================================================================
// Пролог/эпилог hardware interrupt stubs (KiClockInterrupt и др.)
// =============================================================================

/// Входит в hardware interrupt на указанном IRQL.
///
/// Вызывается из ASM-stub перед передачей управления Rust handler'у.
/// Семантически идентичен KiEnterSoftwareInterrupt, но выделен для clarity.
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn KiEnterHardwareInterrupt(new_irql: KIRQL) -> KIRQL {
    let old_irql = unsafe { pcr::get_current_irql() };
    unsafe {
        cpu::write_cr8(new_irql as u64);
        pcr::set_current_irql(new_irql);
    }
    old_irql
}

/// Выходит из hardware interrupt и восстанавливает IRQL.
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn KiExitHardwareInterrupt(old_irql: KIRQL) {
    unsafe {
        pcr::set_current_irql(old_irql);
        cpu::write_cr8(old_irql as u64);
    }
}

// =============================================================================
// HAL helpers (legacy compatibility)
// =============================================================================

/// HalSetIrql / internal helper: устанавливает IRQL без семантики "raise/lower".
///
/// Используется в местах, где HAL/процессорный код требует прямой установки
/// (например, ранняя инициализация). В обычном коде ядра использовать
/// `kf_raise_irql` / `kf_lower_irql`.
#[inline]
pub fn hal_set_irql(new_irql: KIRQL) {
    unsafe {
        cpu::write_cr8(new_irql as u64);
        pcr::set_current_irql(new_irql);
    }
}
