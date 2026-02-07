//! VID внутренняя реализация
//!
//! Внутренние функции bootvid (snake_case).
//! PE-экспорты находятся в `exports/vid.rs`.

use core::sync::atomic::AtomicBool;
use core::sync::atomic::Ordering;

use crate::display::advance_cursor;
use crate::display::carriage_return;
use crate::display::clear_screen;
use crate::display::display_character;
use crate::display::display_character_at;
use crate::display::display_unicode_char;
use crate::display::display_unicode_char_at;
use crate::display::get_back_color_bgra as display_get_back_color_bgra;
use crate::display::get_text_color as display_get_text_color;
use crate::display::init_display_state;
use crate::display::newline;
use crate::display::set_scroll_region as display_set_scroll_region;
use crate::display::set_text_color as display_set_text_color;
use crate::framebuffer::FramebufferInfo;
use crate::framebuffer::fb_fill_rect;
use crate::framebuffer::fb_get_address;
use crate::framebuffer::fb_get_height;
use crate::framebuffer::fb_get_pixels_per_row;
use crate::framebuffer::fb_get_width;
use crate::framebuffer::fb_initialize;
use crate::palette::BV_COLOR_BLACK;
use crate::palette::palette_color_to_bgra;

// =============================================================================
// Состояние инициализации
// =============================================================================

static VID_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Проверка инициализации
#[inline]
pub fn vid_is_initialized() -> bool {
    VID_INITIALIZED.load(Ordering::Acquire)
}

// =============================================================================
// vid_initialize
// =============================================================================

/// Инициализация видео драйвера
///
/// Вызывается из INBV при старте системы.
///
/// # Arguments
/// * `fb` - информация о framebuffer от загрузчика
///
/// # Returns
/// `true` если инициализация успешна
pub fn vid_initialize(fb: &FramebufferInfo) -> bool {
    if VID_INITIALIZED.load(Ordering::Acquire) {
        return true; // Уже инициализирован
    }

    // Инициализируем framebuffer backend
    if !fb_initialize(fb) {
        return false;
    }

    // Инициализируем состояние дисплея
    init_display_state();

    // Очищаем экран
    clear_screen();

    VID_INITIALIZED.store(true, Ordering::Release);
    true
}

// =============================================================================
// vid_cleanup
// =============================================================================

/// Очистка и деинициализация видео драйвера
pub fn vid_cleanup() {
    if !vid_is_initialized() {
        return;
    }

    // Очищаем экран перед выходом
    clear_screen();

    VID_INITIALIZED.store(false, Ordering::Release);
}

// =============================================================================
// vid_display_string
// =============================================================================

/// Вывод байтов как UTF-8 строки с поддержкой Unicode
pub(crate) fn vid_display_bytes(bytes: &[u8]) {
    if !vid_is_initialized() {
        return;
    }

    // Пытаемся интерпретировать как UTF-8
    if let Ok(s) = core::str::from_utf8(bytes) {
        vid_display_string_internal(s);
    } else {
        // Fallback: выводим побайтово как Latin-1
        for &c in bytes {
            match c {
                b'\n' => newline(),
                b'\r' => carriage_return(),
                b'\t' => {
                    for _ in 0..8 {
                        display_character(b' ');
                        advance_cursor();
                    }
                }
                _ => {
                    display_character(c);
                    advance_cursor();
                }
            }
        }
    }
}

/// Внутренняя функция вывода UTF-8 строки с поддержкой Unicode символов
#[inline]
fn vid_display_string_internal(s: &str) {
    for c in s.chars() {
        match c {
            '\n' => newline(),
            '\r' => carriage_return(),
            '\t' => {
                for _ in 0..8 {
                    display_character(b' ');
                    advance_cursor();
                }
            }
            _ => {
                display_unicode_char(c);
                advance_cursor();
            }
        }
    }
}

/// Вывод строки на экран в текущей позиции
///
/// Поддерживает:
/// - Unicode символы (включая Box Drawing U+2500-U+257F)
/// - Управляющие символы: `\n`, `\r`, `\t`
pub fn vid_display_string(s: &str) {
    if !vid_is_initialized() {
        return;
    }

    vid_display_string_internal(s);
}

// =============================================================================
// vid_display_string_xy
// =============================================================================

/// Вывод строки в указанной позиции (внутренняя версия с raw указателями)
///
/// Используется из PE-экспорта.
pub(crate) fn vid_display_string_xy_raw(
    bytes: &[u8],
    left: u32,
    top: u32,
    transparent: bool,
) {
    if !vid_is_initialized() {
        return;
    }

    let text_color = palette_color_to_bgra(display_get_text_color());
    let back_color = if transparent {
        None
    } else {
        Some(display_get_back_color_bgra())
    };
    let mut x = left as usize;
    let y = top as usize;

    // Пытаемся интерпретировать как UTF-8 для поддержки Unicode
    if let Ok(str_slice) = core::str::from_utf8(bytes) {
        for c in str_slice.chars() {
            match c {
                '\n' | '\r' => {}
                _ => {
                    display_unicode_char_at(c, x, y, text_color, back_color);
                    x += crate::BOOTCHAR_WIDTH;
                }
            }
        }
    } else {
        // Fallback: побайтово как Latin-1
        for &c in bytes {
            match c {
                b'\n' | b'\r' => {}
                _ => {
                    display_character_at(c, x, y, text_color, back_color);
                    x += crate::BOOTCHAR_WIDTH;
                }
            }
        }
    }
}

/// Вывод строки в указанной позиции
///
/// Поддерживает Unicode символы (включая Box Drawing).
///
/// # Arguments
/// * `s` - строка для вывода
/// * `left` - X позиция в пикселях
/// * `top` - Y позиция в пикселях
pub fn vid_display_string_xy(s: &str, left: u32, top: u32) {
    if !vid_is_initialized() {
        return;
    }

    let text_color = palette_color_to_bgra(display_get_text_color());
    let mut x = left as usize;
    let y = top as usize;

    for c in s.chars() {
        match c {
            '\n' | '\r' => {
                // Игнорируем управляющие символы в XY режиме
            }
            _ => {
                display_unicode_char_at(c, x, y, text_color, None);
                x += crate::BOOTCHAR_WIDTH;
            }
        }
    }
}

// =============================================================================
// vid_set_text_color
// =============================================================================

/// Установка цвета текста
///
/// # Arguments
/// * `color` - индекс цвета из палитры (0-15)
///
/// # Returns
/// Предыдущий цвет текста
pub fn vid_set_text_color(color: u8) -> u8 {
    display_set_text_color(color)
}

// =============================================================================
// vid_set_scroll_region
// =============================================================================

/// Установка области скроллинга
///
/// # Arguments
/// * `left`, `top` - верхний левый угол
/// * `right`, `bottom` - нижний правый угол
pub fn vid_set_scroll_region(left: u32, top: u32, right: u32, bottom: u32) {
    display_set_scroll_region(left as usize, top as usize, right as usize, bottom as usize);
}

// =============================================================================
// vid_solid_color_fill
// =============================================================================

/// Заливка прямоугольной области цветом
///
/// # Arguments
/// * `left`, `top` - верхний левый угол
/// * `right`, `bottom` - нижний правый угол (не включительно)
/// * `color` - индекс цвета из палитры (0-15)
pub fn vid_solid_color_fill(left: u32, top: u32, right: u32, bottom: u32, color: u8) {
    if !vid_is_initialized() {
        return;
    }

    let bgra = palette_color_to_bgra(color);
    fb_fill_rect(
        left as usize,
        top as usize,
        right as usize,
        bottom as usize,
        bgra,
    );
}

// =============================================================================
// vid_reset_display
// =============================================================================

/// Сброс дисплея
///
/// # Arguments
/// * `hal_reset` - если true, выполняется полный сброс (не используется для UEFI)
pub fn vid_reset_display(_hal_reset: bool) {
    if !vid_is_initialized() {
        return;
    }

    // Для UEFI/Limine просто очищаем экран и сбрасываем состояние
    let width = fb_get_width();
    let height = fb_get_height();

    // Очищаем экран черным
    fb_fill_rect(0, 0, width, height, palette_color_to_bgra(BV_COLOR_BLACK));

    // Сбрасываем состояние
    init_display_state();
}

// =============================================================================
// vid_bit_blt
// =============================================================================

/// Заголовок буфера для `VidBitBlt` в XenOS (BGRA32-only).
///
/// Контракт:
/// - `buffer` указывает на `BvBitmap32Header`, за которым сразу следует pixel data.
/// - pixels: BGRA32, построчно, stride = `delta_bytes` (если 0, то `width * 4`).
#[repr(C)]
struct BvBitmap32Header {
    magic: u32,
    width: u32,
    height: u32,
    delta_bytes: u32,
    format: u32,
}

const BV_BITMAP32_MAGIC: u32 = u32::from_le_bytes(*b"BV32");
const BV_BITMAP32_FORMAT_BGRA32: u32 = 0;

/// BitBlt - копирование bitmap на экран
///
/// # Arguments
/// * `buffer` - указатель на bitmap данные
/// * `left`, `top` - позиция на экране
pub fn vid_bit_blt(buffer: *const u8, left: u32, top: u32) {
    if !vid_is_initialized() || buffer.is_null() {
        return;
    }

    // SAFETY: читаем заголовок unaligned; при невалидном буфере просто выходим.
    let hdr = unsafe { core::ptr::read_unaligned(buffer.cast::<BvBitmap32Header>()) };
    if hdr.magic != BV_BITMAP32_MAGIC {
        return;
    }
    if hdr.format != BV_BITMAP32_FORMAT_BGRA32 {
        return;
    }
    if hdr.width == 0 || hdr.height == 0 {
        return;
    }

    let data = unsafe { buffer.add(core::mem::size_of::<BvBitmap32Header>()) };
    let delta = if hdr.delta_bytes != 0 {
        hdr.delta_bytes
    } else {
        hdr.width.saturating_mul(4)
    };
    vid_buffer_to_screen_blt(data, left, top, hdr.width, hdr.height, delta);
}

// =============================================================================
// vid_buffer_to_screen_blt
// =============================================================================

#[inline]
fn clamp_copy_region(
    left: u32,
    top: u32,
    width: u32,
    height: u32,
) -> Option<(usize, usize, usize, usize)> {
    let sw = fb_get_width();
    let sh = fb_get_height();
    if sw == 0 || sh == 0 {
        return None;
    }
    let left = left as usize;
    let top = top as usize;
    if left >= sw || top >= sh {
        return None;
    }
    let max_w = sw - left;
    let max_h = sh - top;
    let w = (width as usize).min(max_w);
    let h = (height as usize).min(max_h);
    if w == 0 || h == 0 {
        return None;
    }
    Some((left, top, w, h))
}

/// Копирование буфера на экран
///
/// # Arguments
/// * `buffer` - указатель на данные (BGRA32)
/// * `left`, `top` - позиция на экране
/// * `width`, `height` - размеры области
/// * `delta` - stride буфера в байтах
pub fn vid_buffer_to_screen_blt(
    buffer: *const u8,
    left: u32,
    top: u32,
    width: u32,
    height: u32,
    delta: u32,
) {
    if !vid_is_initialized() || buffer.is_null() {
        return;
    }

    let (left, top, mut w, h) = match clamp_copy_region(left, top, width, height) {
        Some(v) => v,
        None => return,
    };

    let fb_addr = fb_get_address();
    if fb_addr.is_null() {
        return;
    }
    let fb_stride_px = fb_get_pixels_per_row();

    // delta: bytes per row in source buffer; if 0, assume tightly packed BGRA32.
    let mut src_delta = delta as usize;
    if src_delta == 0 {
        src_delta = w * 4;
    }

    // Если stride меньше, чем нужно — клиппим по ширине.
    let max_w_by_delta = src_delta / 4;
    w = w.min(max_w_by_delta);
    if w == 0 {
        return;
    }

    unsafe {
        for row in 0..h {
            let src_row = buffer.add(row * src_delta);
            let dst_row = fb_addr.add((top + row) * fb_stride_px + left);

            for x in 0..w {
                let px = core::ptr::read_unaligned(src_row.add(x * 4).cast::<u32>());
                dst_row.add(x).write_volatile(px);
            }
        }
    }
}

// =============================================================================
// vid_screen_to_buffer_blt
// =============================================================================

/// Копирование с экрана в буфер
///
/// # Arguments
/// * `buffer` - указатель на буфер для записи
/// * `left`, `top` - позиция на экране
/// * `width`, `height` - размеры области
/// * `delta` - stride буфера в байтах
pub fn vid_screen_to_buffer_blt(
    buffer: *mut u8,
    left: u32,
    top: u32,
    width: u32,
    height: u32,
    delta: u32,
) {
    if !vid_is_initialized() || buffer.is_null() {
        return;
    }

    let (left, top, mut w, h) = match clamp_copy_region(left, top, width, height) {
        Some(v) => v,
        None => return,
    };

    let fb_addr = fb_get_address();
    if fb_addr.is_null() {
        return;
    }
    let fb_stride_px = fb_get_pixels_per_row();

    // delta: bytes per row in destination buffer; if 0, assume tightly packed BGRA32.
    let mut dst_delta = delta as usize;
    if dst_delta == 0 {
        dst_delta = w * 4;
    }

    let max_w_by_delta = dst_delta / 4;
    w = w.min(max_w_by_delta);
    if w == 0 {
        return;
    }

    unsafe {
        for row in 0..h {
            let src_row = fb_addr.add((top + row) * fb_stride_px + left);
            let dst_row = buffer.add(row * dst_delta);

            for x in 0..w {
                let px = src_row.add(x).read_volatile();
                core::ptr::write_unaligned(dst_row.add(x * 4).cast::<u32>(), px);
            }
        }
    }
}
