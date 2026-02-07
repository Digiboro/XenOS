//! PE-импорты из bootvid.dll
//!
//! Стабы функций, структуры и константы для работы с boot video driver.
//! IAT патчится winload'ом при загрузке.
//!
//! # Источники
//! - ReactOS: bootvid/vga.c
//! - Windows NT 6.1 bootvid.dll

use core::ptr::null_mut;

// =============================================================================
// FramebufferInfo - информация о framebuffer от загрузчика
// =============================================================================

/// Информация о framebuffer (передается из Limine/winload)
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FramebufferInfo {
    /// Адрес framebuffer в памяти
    pub address: *mut u32,

    /// Ширина в пикселях
    pub width: usize,

    /// Высота в пикселях
    pub height: usize,

    /// Pitch (байт на строку)
    pub pitch: usize,

    /// Bits per pixel (обычно 32)
    pub bpp: u8,
}

impl FramebufferInfo {
    /// Создать пустую структуру
    pub const fn empty() -> Self {
        Self {
            address: null_mut(),
            width: 0,
            height: 0,
            pitch: 0,
            bpp: 0,
        }
    }

    /// Проверка валидности
    pub fn is_valid(&self) -> bool {
        !self.address.is_null() && self.width > 0 && self.height > 0 && self.bpp >= 24
    }

    /// Количество пикселей на строку (pitch / 4 для 32bpp)
    pub fn pixels_per_row(&self) -> usize {
        self.pitch / 4
    }
}

unsafe impl Send for FramebufferInfo {}
unsafe impl Sync for FramebufferInfo {}

// =============================================================================
// Константы шрифта
// =============================================================================

/// Ширина символа в пикселях
pub const BOOTCHAR_WIDTH: usize = 8;

/// Высота символа в пикселях
pub const BOOTCHAR_HEIGHT: usize = 14;

// =============================================================================
// Цветовые константы (совместимы с Windows NT bootvid)
// =============================================================================

pub const BV_COLOR_BLACK: u8 = 0;
pub const BV_COLOR_BLUE: u8 = 1;
pub const BV_COLOR_GREEN: u8 = 2;
pub const BV_COLOR_CYAN: u8 = 3;
pub const BV_COLOR_RED: u8 = 4;
pub const BV_COLOR_MAGENTA: u8 = 5;
/// В NT/VGA это "brown" (тёмно-жёлтый). Оставляем имя YELLOW для совместимости.
pub const BV_COLOR_YELLOW: u8 = 6;
pub const BV_COLOR_LIGHT_GRAY: u8 = 7;
pub const BV_COLOR_DARK_GRAY: u8 = 8;
pub const BV_COLOR_LIGHT_BLUE: u8 = 9;
pub const BV_COLOR_LIGHT_GREEN: u8 = 10;
pub const BV_COLOR_LIGHT_CYAN: u8 = 11;
pub const BV_COLOR_LIGHT_RED: u8 = 12;
pub const BV_COLOR_LIGHT_MAGENTA: u8 = 13;
pub const BV_COLOR_LIGHT_YELLOW: u8 = 14;
pub const BV_COLOR_WHITE: u8 = 15;

// =============================================================================
// PE-импорты из bootvid.dll
// =============================================================================

#[link(name = "bootvid")]
unsafe extern "win64" {
    /// VidInitialize - инициализация видео драйвера
    ///
    /// # Arguments
    /// * `fb` - указатель на FramebufferInfo
    ///
    /// # Returns
    /// `true` если инициализация успешна
    #[link_name = "VidInitialize"]
    pub fn vid_initialize(fb: *const FramebufferInfo) -> bool;

    /// VidCleanup - деинициализация видео драйвера
    #[link_name = "VidCleanup"]
    pub fn vid_cleanup();

    /// VidDisplayString - вывод строки в текущей позиции
    ///
    /// # Arguments
    /// * `s` - указатель на строку (UTF-8)
    /// * `len` - длина строки в байтах
    #[link_name = "VidDisplayString"]
    pub fn vid_display_string(s: *const u8, len: usize);

    /// VidDisplayStringXY - вывод строки в указанной позиции
    ///
    /// # Arguments
    /// * `s` - указатель на строку (UTF-8)
    /// * `len` - длина строки в байтах
    /// * `left` - X позиция в пикселях
    /// * `top` - Y позиция в пикселях
    /// * `transparent` - прозрачный фон
    #[link_name = "VidDisplayStringXY"]
    pub fn vid_display_string_xy(s: *const u8, len: usize, left: u32, top: u32, transparent: bool);

    /// VidSetTextColor - установка цвета текста
    ///
    /// # Arguments
    /// * `color` - индекс цвета из палитры (0-15)
    ///
    /// # Returns
    /// Предыдущий цвет текста
    #[link_name = "VidSetTextColor"]
    pub fn vid_set_text_color(color: u32) -> u32;

    /// VidSetScrollRegion - установка области прокрутки
    #[link_name = "VidSetScrollRegion"]
    pub fn vid_set_scroll_region(left: u32, top: u32, right: u32, bottom: u32);

    /// VidSolidColorFill - заливка области цветом
    #[link_name = "VidSolidColorFill"]
    pub fn vid_solid_color_fill(left: u32, top: u32, right: u32, bottom: u32, color: u32);

    /// VidResetDisplay - сброс дисплея
    #[link_name = "VidResetDisplay"]
    pub fn vid_reset_display(hal_reset: bool);

    /// VidBitBlt - копирование bitmap на экран
    #[link_name = "VidBitBlt"]
    pub fn vid_bit_blt(buffer: *const u8, left: u32, top: u32);

    /// VidBufferToScreenBlt - копирование буфера на экран
    #[link_name = "VidBufferToScreenBlt"]
    pub fn vid_buffer_to_screen_blt(
        buffer: *const u8,
        left: u32,
        top: u32,
        width: u32,
        height: u32,
        delta: u32,
    );

    /// VidScreenToBufferBlt - копирование с экрана в буфер
    #[link_name = "VidScreenToBufferBlt"]
    pub fn vid_screen_to_buffer_blt(
        buffer: *mut u8,
        left: u32,
        top: u32,
        width: u32,
        height: u32,
        delta: u32,
    );

    // =========================================================================
    // XenOS Extensions (не NT-совместимые)
    // =========================================================================

    /// VidSolidColorFillBgra - заливка области прямым BGRA32 цветом
    #[link_name = "VidSolidColorFillBgra"]
    pub fn vid_solid_color_fill_bgra(left: u32, top: u32, right: u32, bottom: u32, bgra: u32);

    /// VidSetTextBackgroundBgra - установка цвета фона текста прямым BGRA32
    #[link_name = "VidSetTextBackgroundBgra"]
    pub fn vid_set_text_background_bgra(bgra: u32);

    /// VidGetFramebufferWidth - получить ширину framebuffer
    #[link_name = "VidGetFramebufferWidth"]
    pub fn vid_get_framebuffer_width() -> u32;

    /// VidGetFramebufferHeight - получить высоту framebuffer
    #[link_name = "VidGetFramebufferHeight"]
    pub fn vid_get_framebuffer_height() -> u32;
}

// =============================================================================
// Safe Rust wrappers
// =============================================================================

/// Инициализация bootvid
///
/// # Safety
/// Вызывать только после загрузки bootvid.dll и патчинга IAT.
pub unsafe fn initialize(fb: &FramebufferInfo) -> bool {
    vid_initialize(fb as *const FramebufferInfo)
}

/// Очистка bootvid
pub unsafe fn cleanup() {
    vid_cleanup()
}

/// Вывод строки в текущей позиции
pub unsafe fn display_string(s: &str) {
    vid_display_string(s.as_ptr(), s.len())
}

/// Вывод строки в указанной позиции
pub unsafe fn display_string_xy(s: &str, left: u32, top: u32, transparent: bool) {
    vid_display_string_xy(s.as_ptr(), s.len(), left, top, transparent)
}

/// Установка цвета текста
pub unsafe fn set_text_color(color: u8) -> u8 {
    vid_set_text_color(color as u32) as u8
}

/// Установка области прокрутки
pub unsafe fn set_scroll_region(left: u32, top: u32, right: u32, bottom: u32) {
    vid_set_scroll_region(left, top, right, bottom)
}

/// Заливка области цветом
pub unsafe fn solid_color_fill(left: u32, top: u32, right: u32, bottom: u32, color: u8) {
    vid_solid_color_fill(left, top, right, bottom, color as u32)
}

/// Сброс дисплея
pub unsafe fn reset_display(hal_reset: bool) {
    vid_reset_display(hal_reset)
}

/// BitBlt
pub unsafe fn bit_blt(buffer: *const u8, left: u32, top: u32) {
    vid_bit_blt(buffer, left, top)
}

/// Копирование буфера на экран
pub unsafe fn buffer_to_screen_blt(
    buffer: *const u8,
    left: u32,
    top: u32,
    width: u32,
    height: u32,
    delta: u32,
) {
    vid_buffer_to_screen_blt(buffer, left, top, width, height, delta)
}

/// Копирование с экрана в буфер
pub unsafe fn screen_to_buffer_blt(
    buffer: *mut u8,
    left: u32,
    top: u32,
    width: u32,
    height: u32,
    delta: u32,
) {
    vid_screen_to_buffer_blt(buffer, left, top, width, height, delta)
}

// =============================================================================
// XenOS Extensions
// =============================================================================

/// Заливка области прямым BGRA32 цветом (минуя палитру)
pub unsafe fn solid_color_fill_bgra(left: u32, top: u32, right: u32, bottom: u32, bgra: u32) {
    vid_solid_color_fill_bgra(left, top, right, bottom, bgra)
}

/// Установка цвета фона текста прямым BGRA32
pub unsafe fn set_text_background_bgra(bgra: u32) {
    vid_set_text_background_bgra(bgra)
}

/// Получить ширину framebuffer
pub unsafe fn get_framebuffer_width() -> u32 {
    vid_get_framebuffer_width()
}

/// Получить высоту framebuffer
pub unsafe fn get_framebuffer_height() -> u32 {
    vid_get_framebuffer_height()
}

