//! Boot-time screen logging (NT-style /SOS)
//!
//! Текстовый вывод на экран во время загрузки включается
//! отдельным режимом (аналог `/SOS`) и делается через
//! INBV/BOOTVID (`InbvDisplayString` / `HalDisplayString`).

#![allow(dead_code)]

use core::sync::atomic::AtomicBool;
use core::sync::atomic::Ordering;

use crate::inbv;

/// Включен ли boot-time текстовый вывод на экран (/SOS-режим).
static BOOTLOG_ENABLED: AtomicBool = AtomicBool::new(false);

/// Включить/выключить bootlog. Возвращает предыдущее состояние.
pub fn bootlog_enable(enable: bool) -> bool {
    BOOTLOG_ENABLED.swap(enable, Ordering::AcqRel)
}

#[inline]
pub fn bootlog_is_enabled() -> bool {
    BOOTLOG_ENABLED.load(Ordering::Acquire)
}

/// Инициализирует экран для bootlog (чёрный фон, белый текст).
///
/// Должно вызываться после `inbv_driver_initialize`.
pub fn bootlog_init() {
    if !bootlog_is_enabled() {
        return;
    }

    if !inbv::inbv_is_boot_driver_installed() {
        return;
    }

    // Захватываем дисплей и включаем вывод строк.
    inbv::inbv_acquire_display_ownership();

    // NOTE: используем реальный framebuffer размер; это соответствует современной
    // среде (UEFI), в отличие от фиксированного 640x480 в NT5.
    let width = inbv::inbv_get_fb_width();
    let height = inbv::inbv_get_fb_height();
    inbv::inbv_setup_display_for_debug(width, height);
}

/// Печать строки на экран в bootlog режиме.
#[inline]
pub fn bootlog_print(s: &str) {
    if !bootlog_is_enabled() {
        return;
    }
    let _ = inbv::inbv_display_string(s);
}
