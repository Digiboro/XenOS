//! Внутренние функции отображения
//!
//! DisplayCharacter, DoScroll, SetPixel и другие низкоуровневые операции.

use core::sync::atomic::AtomicBool;
use core::sync::atomic::AtomicU32;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;

use crate::BOOTCHAR_HEIGHT;
use crate::BOOTCHAR_WIDTH;
use crate::font::FONT_DATA;
use crate::font::FONT_HEIGHT;
use crate::font::char_to_glyph_index;
use crate::framebuffer::fb_copy_rect;
use crate::framebuffer::fb_fill_rect;
use crate::framebuffer::fb_get_height;
use crate::framebuffer::fb_get_width;
use crate::framebuffer::fb_is_initialized;
use crate::framebuffer::fb_set_pixel;
use crate::palette::BV_COLOR_BLACK;
use crate::palette::DEFAULT_BACK_COLOR;
use crate::palette::DEFAULT_TEXT_COLOR;
use crate::palette::palette_color_to_bgra;

// =============================================================================
// Глобальное состояние дисплея
// =============================================================================

/// Текущий цвет текста (индекс палитры)
static VIDP_TEXT_COLOR: AtomicU32 = AtomicU32::new(DEFAULT_TEXT_COLOR as u32);

/// Текущий цвет фона (индекс палитры)
static VIDP_BACK_COLOR: AtomicU32 = AtomicU32::new(DEFAULT_BACK_COLOR as u32);

/// BGRA override для цвета фона текста (XenOS extension; нужен когда фон не из палитры)
static VIDP_BACK_COLOR_BGRA_OVERRIDE: AtomicU32 = AtomicU32::new(0);
static VIDP_BACK_COLOR_BGRA_OVERRIDE_ENABLED: AtomicBool = AtomicBool::new(false);

/// Текущая X позиция курсора (в пикселях)
static VIDP_CURRENT_X: AtomicUsize = AtomicUsize::new(0);

/// Текущая Y позиция курсора (в пикселях)
static VIDP_CURRENT_Y: AtomicUsize = AtomicUsize::new(0);

/// Область скроллинга - левая граница
static VIDP_SCROLL_LEFT: AtomicUsize = AtomicUsize::new(0);

/// Область скроллинга - верхняя граница  
static VIDP_SCROLL_TOP: AtomicUsize = AtomicUsize::new(0);

/// Область скроллинга - правая граница
static VIDP_SCROLL_RIGHT: AtomicUsize = AtomicUsize::new(0);

/// Область скроллинга - нижняя граница
static VIDP_SCROLL_BOTTOM: AtomicUsize = AtomicUsize::new(0);

// =============================================================================
// Getters/Setters для состояния
// =============================================================================

/// Получить текущий цвет текста
#[inline]
pub fn get_text_color() -> u8 {
    VIDP_TEXT_COLOR.load(Ordering::Relaxed) as u8
}

/// Установить цвет текста, вернуть предыдущий
#[inline]
pub fn set_text_color(color: u8) -> u8 {
    let prev = VIDP_TEXT_COLOR.swap(color as u32, Ordering::Relaxed);
    prev as u8
}

/// Получить текущий цвет фона
#[inline]
pub fn get_back_color() -> u8 {
    VIDP_BACK_COLOR.load(Ordering::Relaxed) as u8
}

/// Установить цвет фона
#[inline]
pub fn set_back_color(color: u8) {
    VIDP_BACK_COLOR.store(color as u32, Ordering::Relaxed);
}

/// Установить BGRA override для цвета фона текста (используется вместо палитры).
pub fn set_back_color_bgra_override(color: u32) {
    VIDP_BACK_COLOR_BGRA_OVERRIDE.store(color, Ordering::Relaxed);
    VIDP_BACK_COLOR_BGRA_OVERRIDE_ENABLED.store(true, Ordering::Relaxed);
}

/// Сбросить BGRA override и вернуться к палитровому back color.
pub fn clear_back_color_bgra_override() {
    VIDP_BACK_COLOR_BGRA_OVERRIDE_ENABLED.store(false, Ordering::Relaxed);
}

/// Эффективный BGRA back color (учитывает override).
#[inline]
pub fn get_back_color_bgra() -> u32 {
    if VIDP_BACK_COLOR_BGRA_OVERRIDE_ENABLED.load(Ordering::Relaxed) {
        VIDP_BACK_COLOR_BGRA_OVERRIDE.load(Ordering::Relaxed)
    } else {
        palette_color_to_bgra(get_back_color())
    }
}

/// Получить текущую X позицию
#[inline]
pub fn get_current_x() -> usize {
    VIDP_CURRENT_X.load(Ordering::Relaxed)
}

/// Получить текущую Y позицию
#[inline]
pub fn get_current_y() -> usize {
    VIDP_CURRENT_Y.load(Ordering::Relaxed)
}

/// Установить текущую позицию
#[inline]
pub fn set_current_position(x: usize, y: usize) {
    VIDP_CURRENT_X.store(x, Ordering::Relaxed);
    VIDP_CURRENT_Y.store(y, Ordering::Relaxed);
}

/// Получить область скроллинга
#[inline]
pub fn get_scroll_region() -> (usize, usize, usize, usize) {
    (
        VIDP_SCROLL_LEFT.load(Ordering::Relaxed),
        VIDP_SCROLL_TOP.load(Ordering::Relaxed),
        VIDP_SCROLL_RIGHT.load(Ordering::Relaxed),
        VIDP_SCROLL_BOTTOM.load(Ordering::Relaxed),
    )
}

/// Установить область скроллинга
pub fn set_scroll_region(left: usize, top: usize, right: usize, bottom: usize) {
    VIDP_SCROLL_LEFT.store(left, Ordering::Relaxed);
    VIDP_SCROLL_TOP.store(top, Ordering::Relaxed);
    VIDP_SCROLL_RIGHT.store(right, Ordering::Relaxed);
    VIDP_SCROLL_BOTTOM.store(bottom, Ordering::Relaxed);
}

/// Инициализация состояния дисплея
pub fn init_display_state() {
    let width = fb_get_width();
    let height = fb_get_height();

    VIDP_TEXT_COLOR.store(DEFAULT_TEXT_COLOR as u32, Ordering::Relaxed);
    VIDP_BACK_COLOR.store(DEFAULT_BACK_COLOR as u32, Ordering::Relaxed);
    VIDP_BACK_COLOR_BGRA_OVERRIDE_ENABLED.store(false, Ordering::Relaxed);
    VIDP_CURRENT_X.store(0, Ordering::Relaxed);
    VIDP_CURRENT_Y.store(0, Ordering::Relaxed);

    // Scroll region = весь экран
    VIDP_SCROLL_LEFT.store(0, Ordering::Relaxed);
    VIDP_SCROLL_TOP.store(0, Ordering::Relaxed);
    VIDP_SCROLL_RIGHT.store(width, Ordering::Relaxed);
    VIDP_SCROLL_BOTTOM.store(height, Ordering::Relaxed);
}

// =============================================================================
// Отображение символов
// =============================================================================

/// Отобразить символ в указанной позиции
///
/// # Arguments
/// * `c` - ASCII код символа
/// * `x` - X позиция в пикселях
/// * `y` - Y позиция в пикселях
/// * `text_color` - цвет текста (BGRA)
/// * `back_color` - цвет фона (BGRA), если None - прозрачный фон
pub fn display_character_at(c: u8, x: usize, y: usize, text_color: u32, back_color: Option<u32>) {
    if !fb_is_initialized() {
        return;
    }

    // Получаем bitmap данные символа
    let char_index = c as usize;
    let font_offset = char_index * BOOTCHAR_HEIGHT;

    // Рисуем символ построчно
    for row in 0..BOOTCHAR_HEIGHT {
        let font_row = FONT_DATA[font_offset + row];
        let py = y + row;

        for col in 0..BOOTCHAR_WIDTH {
            let px = x + col;

            // Бит установлен = пиксель текста
            let bit_mask = 0x80 >> col;
            if (font_row & bit_mask) != 0 {
                fb_set_pixel(px, py, text_color);
            } else if let Some(bg) = back_color {
                fb_set_pixel(px, py, bg);
            }
        }
    }
}

/// Отобразить символ в текущей позиции с текущими цветами
pub fn display_character(c: u8) {
    let x = get_current_x();
    let y = get_current_y();
    let text_color = palette_color_to_bgra(get_text_color());
    let back_color = Some(get_back_color_bgra());

    display_character_at(c, x, y, text_color, back_color);
}

/// Отобразить Unicode символ в указанной позиции
///
/// Поддерживает ASCII, Latin-1, Box Drawing (U+2500-U+257F) и Block Elements (U+2580-U+259F)
///
/// # Arguments
/// * `c` - Unicode символ
/// * `x` - X позиция в пикселях
/// * `y` - Y позиция в пикселях
/// * `text_color` - цвет текста (BGRA)
/// * `back_color` - цвет фона (BGRA), если None - прозрачный фон
pub fn display_unicode_char_at(c: char, x: usize, y: usize, text_color: u32, back_color: Option<u32>) {
    if !fb_is_initialized() {
        return;
    }

    // Получаем индекс глифа для Unicode символа
    let glyph_index = char_to_glyph_index(c);
    let font_offset = glyph_index * FONT_HEIGHT;

    // Рисуем символ построчно
    for row in 0..BOOTCHAR_HEIGHT {
        let font_row = FONT_DATA[font_offset + row];
        let py = y + row;

        for col in 0..BOOTCHAR_WIDTH {
            let px = x + col;

            // Бит установлен = пиксель текста
            let bit_mask = 0x80 >> col;
            if (font_row & bit_mask) != 0 {
                fb_set_pixel(px, py, text_color);
            } else if let Some(bg) = back_color {
                fb_set_pixel(px, py, bg);
            }
        }
    }
}

/// Отобразить Unicode символ в текущей позиции с текущими цветами
pub fn display_unicode_char(c: char) {
    let x = get_current_x();
    let y = get_current_y();
    let text_color = palette_color_to_bgra(get_text_color());
    let back_color = Some(get_back_color_bgra());

    display_unicode_char_at(c, x, y, text_color, back_color);
}

// =============================================================================
// Скроллинг
// =============================================================================

/// Скроллинг экрана на одну строку вверх
pub fn do_scroll() {
    if !fb_is_initialized() {
        return;
    }

    let (left, top, right, bottom) = get_scroll_region();

    if right <= left || bottom <= top {
        return;
    }

    let scroll_height = bottom - top;
    if scroll_height <= BOOTCHAR_HEIGHT {
        // Область слишком мала для скроллинга
        return;
    }

    let width = right - left;
    let copy_height = scroll_height - BOOTCHAR_HEIGHT;

    // Копируем область вверх
    fb_copy_rect(
        left,
        top + BOOTCHAR_HEIGHT, // source
        left,
        top, // destination
        width,
        copy_height,
    );

    // Очищаем нижнюю строку
    let clear_top = bottom - BOOTCHAR_HEIGHT;
    let back_color = get_back_color_bgra();
    fb_fill_rect(left, clear_top, right, bottom, back_color);
}

/// Перевод строки с возможным скроллингом
pub fn newline() {
    let (left, _top, _right, bottom) = get_scroll_region();
    let y = get_current_y();

    // Переход на начало следующей строки
    let new_y = y + BOOTCHAR_HEIGHT;

    if new_y + BOOTCHAR_HEIGHT > bottom {
        // Нужен скроллинг
        do_scroll();
        // Позиция остается на последней строке
        set_current_position(left, bottom - BOOTCHAR_HEIGHT);
    } else {
        set_current_position(left, new_y);
    }
}

/// Возврат каретки
pub fn carriage_return() {
    let (left, _top, _right, _bottom) = get_scroll_region();
    let y = get_current_y();
    set_current_position(left, y);
}

/// Продвинуть курсор после символа
pub fn advance_cursor() {
    let (_left, _top, right, _bottom) = get_scroll_region();
    let x = get_current_x();
    let y = get_current_y();

    let new_x = x + BOOTCHAR_WIDTH;

    if new_x + BOOTCHAR_WIDTH > right {
        // Переход на новую строку
        newline();
    } else {
        set_current_position(new_x, y);
    }
}

// =============================================================================
// Вспомогательные функции
// =============================================================================

/// Очистка экрана
pub fn clear_screen() {
    if !fb_is_initialized() {
        return;
    }

    let width = fb_get_width();
    let height = fb_get_height();
    let back_color = palette_color_to_bgra(BV_COLOR_BLACK);

    fb_fill_rect(0, 0, width, height, back_color);
    set_current_position(0, 0);
}

/// Очистка scroll region
pub fn clear_scroll_region() {
    let (left, top, right, bottom) = get_scroll_region();
    let back_color = get_back_color_bgra();

    fb_fill_rect(left, top, right, bottom, back_color);
}
