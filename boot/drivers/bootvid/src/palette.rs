//! Цветовая палитра BOOTVID
//!
//! 16 цветов CGA-style, используемых для boot video.
//! Цвета в формате BGRA (32bpp) для linear framebuffer.

// =============================================================================
// Индексы цветов (совместимы с Windows NT bootvid)
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
// Палитра в формате BGRA (0xAARRGGBB для little-endian -> 0xBBGGRRAA в памяти)
// =============================================================================

/// Палитра из 16 цветов в формате 0xAARRGGBB
///
/// Индексы соответствуют константам BV_COLOR_*
pub static BOOTVID_PALETTE: [u32; 16] = [
    0xFF000000, // 0  - Black
    0xFF0000AA, // 1  - Blue (dark)
    0xFF00AA00, // 2  - Green (dark)
    0xFF00AAAA, // 3  - Cyan (dark)
    0xFFAA0000, // 4  - Red (dark)
    0xFFAA00AA, // 5  - Magenta (dark)
    0xFFAA5500, // 6  - Brown (dark) (NT/VGA "yellow")
    0xFFAAAAAA, // 7  - Light Gray
    0xFF555555, // 8  - Dark Gray
    0xFF5555FF, // 9  - Light Blue
    0xFF55FF55, // 10 - Light Green
    0xFF55FFFF, // 11 - Light Cyan
    0xFFFF5555, // 12 - Light Red
    0xFFFF55FF, // 13 - Light Magenta
    0xFFFFFF55, // 14 - Light Yellow
    0xFFFFFFFF, // 15 - White
];

/// Преобразование индекса цвета в BGRA значение
#[inline]
pub fn palette_color_to_bgra(color_index: u8) -> u32 {
    let index = (color_index & 0x0F) as usize;
    BOOTVID_PALETTE[index]
}

/// Цвет текста по умолчанию
pub const DEFAULT_TEXT_COLOR: u8 = BV_COLOR_WHITE;

/// Цвет фона по умолчанию
pub const DEFAULT_BACK_COLOR: u8 = BV_COLOR_BLACK;
