//! VID PE-экспорты
//!
//! Публичные PE-экспорты bootvid для INBV.
//! Каждая функция — thin-wrapper над внутренней реализацией в `vid.rs`.
//!
//! # Архитектура
//!
//! ```text
//!        (внешний модуль)
//!            INBV
//!              │   imports (IAT)
//!              ▼
//!  ┌──────────────────────────────┐
//!  │     bootvid.dll exports      │  <── Этот модуль
//!  │       VidInitialize / ...    │
//!  └───────────────┬──────────────┘
//!                  │ thin-wrapper
//!                  ▼
//!  ┌──────────────────────────────┐
//!  │    внутренние реализации     │  <── vid.rs
//!  │    vid_initialize / ...      │
//!  └──────────────────────────────┘
//! ```
//!
//! # Источники
//! - ReactOS: bootvid/vga.c (VidInitialize, VidDisplayString, ...)
//! - Windows NT 6.1 bootvid.dll exports

use crate::framebuffer::FramebufferInfo;
use crate::vid;

// =============================================================================
// VidInitialize
// =============================================================================

/// VidInitialize
///
/// Инициализация видео драйвера.
///
/// # Arguments
/// * `fb` - указатель на FramebufferInfo
///
/// # Returns
/// `true` если инициализация успешна
#[unsafe(export_name = "VidInitialize")]
pub extern "win64" fn VidInitialize(fb: *const FramebufferInfo) -> bool {
    if fb.is_null() {
        return false;
    }
    // SAFETY: вызывающий гарантирует валидность указателя
    let fb = unsafe { &*fb };
    vid::vid_initialize(fb)
}

// =============================================================================
// VidCleanup
// =============================================================================

/// VidCleanup
///
/// Очистка и деинициализация видео драйвера.
#[unsafe(export_name = "VidCleanup")]
pub extern "win64" fn VidCleanup() {
    vid::vid_cleanup()
}

// =============================================================================
// VidDisplayString
// =============================================================================

/// VidDisplayString
///
/// Вывод строки в текущей позиции.
///
/// # Arguments
/// * `s` - указатель на строку (UTF-8)
/// * `len` - длина строки в байтах
#[unsafe(export_name = "VidDisplayString")]
pub extern "win64" fn VidDisplayString(s: *const u8, len: usize) {
    if s.is_null() || len == 0 {
        return;
    }
    // SAFETY: вызывающий гарантирует валидность буфера на чтение
    let bytes = unsafe { core::slice::from_raw_parts(s, len) };
    vid::vid_display_bytes(bytes)
}

// =============================================================================
// VidDisplayStringXY
// =============================================================================

/// VidDisplayStringXY
///
/// Вывод строки в указанной позиции.
///
/// # Arguments
/// * `s` - указатель на строку (UTF-8)
/// * `len` - длина строки в байтах
/// * `left` - X позиция в пикселях
/// * `top` - Y позиция в пикселях
/// * `transparent` - прозрачный фон
#[unsafe(export_name = "VidDisplayStringXY")]
pub extern "win64" fn VidDisplayStringXY(
    s: *const u8,
    len: usize,
    left: u32,
    top: u32,
    transparent: bool,
) {
    if s.is_null() || len == 0 {
        return;
    }
    // SAFETY: вызывающий гарантирует валидность буфера на чтение
    let bytes = unsafe { core::slice::from_raw_parts(s, len) };
    vid::vid_display_string_xy_raw(bytes, left, top, transparent)
}

// =============================================================================
// VidSetTextColor
// =============================================================================

/// VidSetTextColor
///
/// Установка цвета текста.
///
/// # Arguments
/// * `color` - индекс цвета из палитры (0-15)
///
/// # Returns
/// Предыдущий цвет текста
#[unsafe(export_name = "VidSetTextColor")]
pub extern "win64" fn VidSetTextColor(color: u32) -> u32 {
    vid::vid_set_text_color(color as u8) as u32
}

// =============================================================================
// VidSetScrollRegion
// =============================================================================

/// VidSetScrollRegion
///
/// Установка области прокрутки.
///
/// # Arguments
/// * `left`, `top` - верхний левый угол
/// * `right`, `bottom` - нижний правый угол
#[unsafe(export_name = "VidSetScrollRegion")]
pub extern "win64" fn VidSetScrollRegion(left: u32, top: u32, right: u32, bottom: u32) {
    vid::vid_set_scroll_region(left, top, right, bottom)
}

// =============================================================================
// VidSolidColorFill
// =============================================================================

/// VidSolidColorFill
///
/// Заливка прямоугольной области цветом.
///
/// # Arguments
/// * `left`, `top` - верхний левый угол
/// * `right`, `bottom` - нижний правый угол
/// * `color` - индекс цвета из палитры (0-15)
#[unsafe(export_name = "VidSolidColorFill")]
pub extern "win64" fn VidSolidColorFill(left: u32, top: u32, right: u32, bottom: u32, color: u32) {
    vid::vid_solid_color_fill(left, top, right, bottom, color as u8)
}

// =============================================================================
// VidResetDisplay
// =============================================================================

/// VidResetDisplay
///
/// Сброс дисплея.
///
/// # Arguments
/// * `hal_reset` - если true, выполняется полный сброс (не используется для UEFI)
#[unsafe(export_name = "VidResetDisplay")]
pub extern "win64" fn VidResetDisplay(hal_reset: bool) {
    vid::vid_reset_display(hal_reset)
}

// =============================================================================
// VidBitBlt
// =============================================================================

/// VidBitBlt
///
/// BitBlt - копирование bitmap на экран.
///
/// # Arguments
/// * `buffer` - указатель на bitmap данные
/// * `left`, `top` - позиция на экране
#[unsafe(export_name = "VidBitBlt")]
pub extern "win64" fn VidBitBlt(buffer: *const u8, left: u32, top: u32) {
    vid::vid_bit_blt(buffer, left, top)
}

// =============================================================================
// VidBufferToScreenBlt
// =============================================================================

/// VidBufferToScreenBlt
///
/// Копирование буфера на экран.
///
/// # Arguments
/// * `buffer` - указатель на данные (BGRA32)
/// * `left`, `top` - позиция на экране
/// * `width`, `height` - размеры области
/// * `delta` - stride буфера в байтах
#[unsafe(export_name = "VidBufferToScreenBlt")]
pub extern "win64" fn VidBufferToScreenBlt(
    buffer: *const u8,
    left: u32,
    top: u32,
    width: u32,
    height: u32,
    delta: u32,
) {
    vid::vid_buffer_to_screen_blt(buffer, left, top, width, height, delta)
}

// =============================================================================
// VidScreenToBufferBlt
// =============================================================================

/// VidScreenToBufferBlt
///
/// Копирование с экрана в буфер.
///
/// # Arguments
/// * `buffer` - указатель на буфер для записи
/// * `left`, `top` - позиция на экране
/// * `width`, `height` - размеры области
/// * `delta` - stride буфера в байтах
#[unsafe(export_name = "VidScreenToBufferBlt")]
pub extern "win64" fn VidScreenToBufferBlt(
    buffer: *mut u8,
    left: u32,
    top: u32,
    width: u32,
    height: u32,
    delta: u32,
) {
    vid::vid_screen_to_buffer_blt(buffer, left, top, width, height, delta)
}

// =============================================================================
// XenOS Extensions (не NT-совместимые)
// =============================================================================

/// VidSolidColorFillBgra
///
/// Заливка области прямым BGRA32 цветом (минуя палитру).
/// XenOS extension для кастомных тем.
#[unsafe(export_name = "VidSolidColorFillBgra")]
pub extern "win64" fn VidSolidColorFillBgra(
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
    bgra: u32,
) {
    crate::framebuffer::fb_fill_rect(
        left as usize,
        top as usize,
        right as usize,
        bottom as usize,
        bgra,
    )
}

/// VidSetTextBackgroundBgra
///
/// Установка цвета фона текста прямым BGRA32 (минуя палитру).
/// XenOS extension для кастомных тем.
#[unsafe(export_name = "VidSetTextBackgroundBgra")]
pub extern "win64" fn VidSetTextBackgroundBgra(bgra: u32) {
    crate::display::set_back_color_bgra_override(bgra)
}

/// VidGetFramebufferWidth
///
/// Получить ширину framebuffer.
#[unsafe(export_name = "VidGetFramebufferWidth")]
pub extern "win64" fn VidGetFramebufferWidth() -> u32 {
    crate::framebuffer::fb_get_width() as u32
}

/// VidGetFramebufferHeight
///
/// Получить высоту framebuffer.
#[unsafe(export_name = "VidGetFramebufferHeight")]
pub extern "win64" fn VidGetFramebufferHeight() -> u32 {
    crate::framebuffer::fb_get_height() as u32
}
