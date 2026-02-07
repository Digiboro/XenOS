//! Font System для BOOTVID
//!
//! 8x14 bitmap шрифт для boot video, совместимый с Windows NT bootvid.
//!
//! Шрифт генерируется из BDF файлов в `assets/boot/fonts/` на этапе сборки.
//!
//! Структура массива глифов:
//! - 0-255: ASCII/Latin-1
//! - 256-383: Box Drawing (U+2500-U+257F)
//! - 384-415: Block Elements (U+2580-U+259F)

mod data;

pub use data::FONT_DATA;

/// Ширина символа в пикселях
pub const FONT_WIDTH: usize = 8;

/// Высота символа в пикселях  
pub const FONT_HEIGHT: usize = 14;

/// Общее количество глифов в массиве
pub const FONT_GLYPH_COUNT: usize = 416;

// Диапазоны Unicode и соответствующие смещения в массиве глифов

/// Box Drawing символы: U+2500-U+257F
const BOX_DRAWING_START: u32 = 0x2500;
const BOX_DRAWING_END: u32 = 0x257F;
const BOX_DRAWING_GLYPH_OFFSET: usize = 256;

/// Block Elements: U+2580-U+259F
const BLOCK_ELEMENTS_START: u32 = 0x2580;
const BLOCK_ELEMENTS_END: u32 = 0x259F;
const BLOCK_ELEMENTS_GLYPH_OFFSET: usize = 384;

/// Индекс глифа-заглушки (квадрат) для неизвестных символов
const FALLBACK_GLYPH: usize = 0;

/// Преобразование Unicode codepoint в индекс глифа
///
/// # Arguments
/// * `c` - Unicode символ
///
/// # Returns
/// Индекс глифа в массиве FONT_DATA
#[inline]
pub fn char_to_glyph_index(c: char) -> usize {
    let cp = c as u32;
    
    if cp < 256 {
        // ASCII/Latin-1: прямое отображение
        cp as usize
    } else if cp >= BOX_DRAWING_START && cp <= BOX_DRAWING_END {
        // Box Drawing: U+2500-U+257F -> 256-383
        BOX_DRAWING_GLYPH_OFFSET + (cp - BOX_DRAWING_START) as usize
    } else if cp >= BLOCK_ELEMENTS_START && cp <= BLOCK_ELEMENTS_END {
        // Block Elements: U+2580-U+259F -> 384-415
        BLOCK_ELEMENTS_GLYPH_OFFSET + (cp - BLOCK_ELEMENTS_START) as usize
    } else {
        // Неизвестный символ - возвращаем заглушку
        FALLBACK_GLYPH
    }
}

/// Получить bitmap данные для глифа по индексу
///
/// # Arguments
/// * `glyph_index` - Индекс глифа (0-415)
///
/// # Returns
/// Срез из 14 байт, каждый байт = одна строка (MSB слева)
#[inline]
pub fn get_glyph_bitmap(glyph_index: usize) -> &'static [u8] {
    let idx = if glyph_index < FONT_GLYPH_COUNT {
        glyph_index
    } else {
        FALLBACK_GLYPH
    };
    let offset = idx * FONT_HEIGHT;
    &FONT_DATA[offset..offset + FONT_HEIGHT]
}

/// Получить bitmap данные символа (ASCII)
///
/// # Arguments
/// * `c` - ASCII код символа (0-255)
///
/// # Returns
/// Срез из 14 байт, каждый байт = одна строка (MSB слева)
#[inline]
pub fn get_char_bitmap(c: u8) -> &'static [u8] {
    get_glyph_bitmap(c as usize)
}

/// Получить bitmap данные для Unicode символа
///
/// # Arguments
/// * `c` - Unicode символ
///
/// # Returns
/// Срез из 14 байт, каждый байт = одна строка (MSB слева)
#[inline]
pub fn get_unicode_char_bitmap(c: char) -> &'static [u8] {
    get_glyph_bitmap(char_to_glyph_index(c))
}

/// Проверить, установлен ли пиксель в символе
///
/// # Arguments
/// * `c` - ASCII код символа
/// * `x` - X координата (0-7)
/// * `y` - Y координата (0-13)
#[inline]
pub fn is_pixel_set(c: u8, x: usize, y: usize) -> bool {
    if x >= FONT_WIDTH || y >= FONT_HEIGHT {
        return false;
    }

    let bitmap = get_char_bitmap(c);
    let row = bitmap[y];
    let mask = 0x80 >> x;
    (row & mask) != 0
}
