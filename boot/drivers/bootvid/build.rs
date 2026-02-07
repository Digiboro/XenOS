//! Build script для генерации bitmap шрифта из BDF
//!
//! Генерирует src/font/data.rs с массивом FONT_DATA
//!
//! Структура массива:
//! - 0-255: ASCII/Latin-1 символы
//! - 256-383: Box Drawing (U+2500-U+257F)
//! - 384-447: Block Elements (U+2580-U+259F)

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::Path;

// Параметры шрифта
const FONT_WIDTH: usize = 8;
const FONT_HEIGHT: usize = 14;

/// Базовое количество символов ASCII/Latin-1
const ASCII_COUNT: usize = 256;

/// Box Drawing символы: U+2500-U+257F (128 символов)
const BOX_DRAWING_START: usize = 0x2500;
const BOX_DRAWING_COUNT: usize = 128;
const BOX_DRAWING_GLYPH_OFFSET: usize = 256;

/// Block Elements: U+2580-U+259F (32 символа)
const BLOCK_ELEMENTS_START: usize = 0x2580;
const BLOCK_ELEMENTS_COUNT: usize = 32;
const BLOCK_ELEMENTS_GLYPH_OFFSET: usize = 384;

/// Общее количество глифов в массиве
const TOTAL_GLYPH_COUNT: usize = ASCII_COUNT + BOX_DRAWING_COUNT + BLOCK_ELEMENTS_COUNT; // 416

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();

    // Путь к BDF шрифту Terminus
    // CARGO_MANIFEST_DIR = .../boot/drivers/bootvid
    // parent() -> .../boot/drivers
    // parent() -> .../boot  
    // parent() -> .../ (корень проекта)
    let font_path = Path::new(&manifest_dir)
        .parent()
        .unwrap() // boot/drivers
        .parent()
        .unwrap() // boot
        .parent()
        .unwrap() // project root
        .join("assets")
        .join("boot")
        .join("fonts")
        .join("ter-u14b.bdf");

    // Путь для сгенерированного файла
    let dest_path = Path::new(&manifest_dir).join("src/font/data.rs");

    println!("cargo:rerun-if-changed={}", font_path.display());
    println!("cargo:rerun-if-changed=build.rs");

    let font_data = if font_path.exists() {
        // Генерируем шрифт из BDF
        generate_font_from_bdf(&font_path)
    } else {
        // Используем fallback шрифт
        eprintln!(
            "Warning: {} not found, using fallback font",
            font_path.display()
        );
        generate_fallback_font()
    };

    // Генерируем Rust код
    let rust_code = generate_rust_code(&font_data);

    fs::write(&dest_path, rust_code).expect("Failed to write font data");
}

/// Парсинг BDF файла и извлечение bitmap данных
fn generate_font_from_bdf(font_path: &Path) -> Vec<u8> {
    let content = fs::read_to_string(font_path).expect("Failed to read BDF file");

    // Парсим все глифы из BDF
    let glyphs = parse_bdf(&content);

    // Создаём массив для всех символов
    let mut data = vec![0u8; TOTAL_GLYPH_COUNT * FONT_HEIGHT];

    // Заполняем placeholder для неизвестных символов (квадрат)
    let placeholder: [u8; 14] = [
        0x00, 0x00, 0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00, 0x00, 0x00,
    ];
    
    // Placeholder для контрольных символов ASCII
    for c in 0..ASCII_COUNT {
        if c < 32 || c == 127 {
            let offset = c * FONT_HEIGHT;
            data[offset..offset + FONT_HEIGHT].copy_from_slice(&placeholder);
        }
    }

    // Копируем глифы с маппингом Unicode -> позиция в массиве
    for (encoding, glyph_data) in &glyphs {
        let glyph_index = unicode_to_glyph_index(*encoding);
        
        if let Some(idx) = glyph_index {
            let offset = idx * FONT_HEIGHT;
            let copy_len = glyph_data.len().min(FONT_HEIGHT);
            data[offset..offset + copy_len].copy_from_slice(&glyph_data[..copy_len]);
        }
    }

    // Статистика для отладки
    let mut ascii_count = 0;
    let mut box_count = 0;
    let mut block_count = 0;
    
    for (encoding, _) in &glyphs {
        if *encoding < ASCII_COUNT {
            ascii_count += 1;
        } else if *encoding >= BOX_DRAWING_START && *encoding < BOX_DRAWING_START + BOX_DRAWING_COUNT {
            box_count += 1;
        } else if *encoding >= BLOCK_ELEMENTS_START && *encoding < BLOCK_ELEMENTS_START + BLOCK_ELEMENTS_COUNT {
            block_count += 1;
        }
    }
    
    eprintln!("Font statistics: ASCII={}, Box Drawing={}, Block Elements={}", 
              ascii_count, box_count, block_count);

    data
}

/// Преобразование Unicode codepoint в индекс глифа
fn unicode_to_glyph_index(codepoint: usize) -> Option<usize> {
    if codepoint < ASCII_COUNT {
        // ASCII/Latin-1: 0-255 -> позиция 0-255
        Some(codepoint)
    } else if codepoint >= BOX_DRAWING_START && codepoint < BOX_DRAWING_START + BOX_DRAWING_COUNT {
        // Box Drawing: U+2500-U+257F -> позиция 256-383
        Some(BOX_DRAWING_GLYPH_OFFSET + (codepoint - BOX_DRAWING_START))
    } else if codepoint >= BLOCK_ELEMENTS_START && codepoint < BLOCK_ELEMENTS_START + BLOCK_ELEMENTS_COUNT {
        // Block Elements: U+2580-U+259F -> позиция 384-415
        Some(BLOCK_ELEMENTS_GLYPH_OFFSET + (codepoint - BLOCK_ELEMENTS_START))
    } else {
        None
    }
}

/// Структура для хранения данных глифа
struct BdfGlyph {
    encoding: usize,
    bitmap: Vec<u8>,
}

/// Парсинг BDF файла
fn parse_bdf(content: &str) -> HashMap<usize, Vec<u8>> {
    let mut glyphs = HashMap::new();
    let mut current_encoding: Option<usize> = None;
    let mut current_bitmap: Vec<u8> = Vec::new();
    let mut in_bitmap = false;

    for line in content.lines() {
        let line = line.trim();

        if line.starts_with("ENCODING ") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                current_encoding = parts[1].parse().ok();
            }
        } else if line == "BITMAP" {
            in_bitmap = true;
            current_bitmap.clear();
        } else if line == "ENDCHAR" {
            if let Some(enc) = current_encoding {
                if !current_bitmap.is_empty() {
                    glyphs.insert(enc, current_bitmap.clone());
                }
            }
            in_bitmap = false;
            current_encoding = None;
            current_bitmap.clear();
        } else if in_bitmap && !line.is_empty() {
            // Парсим hex строку
            if let Ok(byte) = u8::from_str_radix(line, 16) {
                current_bitmap.push(byte);
            } else if line.len() >= 2 {
                // Может быть 2-байтовая строка, берём первый байт
                if let Ok(byte) = u8::from_str_radix(&line[0..2], 16) {
                    current_bitmap.push(byte);
                }
            }
        }
    }

    glyphs
}

/// Fallback шрифт если BDF не найден
fn generate_fallback_font() -> Vec<u8> {
    let mut data = vec![0u8; TOTAL_GLYPH_COUNT * FONT_HEIGHT];

    // Placeholder для неизвестных символов
    let placeholder: [u8; 14] = [
        0x00, 0x00, 0x7E, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x42, 0x7E, 0x00, 0x00, 0x00,
    ];

    for c in 0..ASCII_COUNT {
        if c < 32 || c >= 127 {
            let offset = c * FONT_HEIGHT;
            data[offset..offset + FONT_HEIGHT].copy_from_slice(&placeholder);
        }
    }

    data
}

/// Генерация Rust кода с данными шрифта
fn generate_rust_code(font_data: &[u8]) -> String {
    let mut code = String::new();

    code.push_str("//! Сгенерированные данные шрифта\n");
    code.push_str("//!\n");
    code.push_str("//! Автоматически сгенерировано build.rs из ter-u14b.bdf\n");
    code.push_str("//! НЕ РЕДАКТИРОВАТЬ ВРУЧНУЮ!\n");
    code.push_str("//!\n");
    code.push_str("//! Структура массива:\n");
    code.push_str("//! - 0-255: ASCII/Latin-1\n");
    code.push_str("//! - 256-383: Box Drawing (U+2500-U+257F)\n");
    code.push_str("//! - 384-415: Block Elements (U+2580-U+259F)\n\n");

    code.push_str(&format!("/// Bitmap данные шрифта 8x14 (Terminus)\n"));
    code.push_str("///\n");
    code.push_str(&format!(
        "/// {} глифов * {} строк = {} байт\n",
        TOTAL_GLYPH_COUNT,
        FONT_HEIGHT,
        TOTAL_GLYPH_COUNT * FONT_HEIGHT
    ));
    code.push_str("/// Каждый байт = одна строка символа (MSB = левый пиксель)\n");
    code.push_str("#[rustfmt::skip]\n");
    code.push_str(&format!(
        "pub static FONT_DATA: [u8; {}] = [\n",
        TOTAL_GLYPH_COUNT * FONT_HEIGHT
    ));

    // ASCII/Latin-1 (0-255)
    code.push_str("    // =========== ASCII/Latin-1 (0-255) ===========\n");
    for c in 0..ASCII_COUNT {
        let offset = c * FONT_HEIGHT;
        code.push_str(&format!("    // Glyph {} (0x{:02X})", c, c));

        if c >= 32 && c < 127 {
            let ch = c as u8 as char;
            if ch == '\\' {
                code.push_str(" '\\\\'");
            } else if ch == '\'' {
                code.push_str(" '\\''");
            } else {
                code.push_str(&format!(" '{}'", ch));
            }
        }
        code.push('\n');

        code.push_str("    ");
        for row in 0..FONT_HEIGHT {
            code.push_str(&format!("0x{:02X}, ", font_data[offset + row]));
        }
        code.push('\n');
    }

    // Box Drawing (256-383, соответствует U+2500-U+257F)
    code.push_str("\n    // =========== Box Drawing U+2500-U+257F (glyph 256-383) ===========\n");
    for i in 0..BOX_DRAWING_COUNT {
        let glyph_idx = BOX_DRAWING_GLYPH_OFFSET + i;
        let unicode = BOX_DRAWING_START + i;
        let offset = glyph_idx * FONT_HEIGHT;
        
        let char_repr = char::from_u32(unicode as u32).unwrap_or('?');
        code.push_str(&format!(
            "    // Glyph {} (U+{:04X}) '{}'\n",
            glyph_idx, unicode, char_repr
        ));

        code.push_str("    ");
        for row in 0..FONT_HEIGHT {
            code.push_str(&format!("0x{:02X}, ", font_data[offset + row]));
        }
        code.push('\n');
    }

    // Block Elements (384-415, соответствует U+2580-U+259F)
    code.push_str("\n    // =========== Block Elements U+2580-U+259F (glyph 384-415) ===========\n");
    for i in 0..BLOCK_ELEMENTS_COUNT {
        let glyph_idx = BLOCK_ELEMENTS_GLYPH_OFFSET + i;
        let unicode = BLOCK_ELEMENTS_START + i;
        let offset = glyph_idx * FONT_HEIGHT;
        
        let char_repr = char::from_u32(unicode as u32).unwrap_or('?');
        code.push_str(&format!(
            "    // Glyph {} (U+{:04X}) '{}'\n",
            glyph_idx, unicode, char_repr
        ));

        code.push_str("    ");
        for row in 0..FONT_HEIGHT {
            code.push_str(&format!("0x{:02X}, ", font_data[offset + row]));
        }
        code.push('\n');
    }

    code.push_str("];\n");

    code
}
