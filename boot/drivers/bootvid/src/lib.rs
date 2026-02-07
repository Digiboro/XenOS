//! BOOTVID - Boot Video Driver
//!
//! Отдельный драйвер для вывода текста и графики на экран во время загрузки ОС.
//! Реализация аналогична Windows NT bootvid.dll / ReactOS bootvid.
//!
//! # Архитектура
//!
//! ```text
//! INBV (ntoskrnl)
//!     │
//!     │ VidInitialize, VidDisplayString, ...
//!     v
//! BOOTVID
//!     ├── exports/        - PE-экспорты (VidInitialize, VidDisplayString, ...)
//!     ├── vid.rs          - Public Vid* API (внутренняя реализация)
//!     ├── display.rs      - DisplayCharacter, DoScroll
//!     ├── framebuffer.rs  - Linear Framebuffer backend
//!     └── font/           - Bitmap font data
//! ```

#![no_std]
#![allow(dead_code)]

use core::panic::PanicInfo;

/// Panic handler для bootvid.dll
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

/// DllMain для bootvid.dll (требуется для cdylib)
#[unsafe(no_mangle)]
#[allow(non_snake_case)]
pub extern "system" fn DllMain(_: *const u8, _: u32, _: *const u8) -> bool {
    true
}

/// Entry point для DLL (требуется линкером как _DllMainCRTStartup)
#[unsafe(export_name = "_DllMainCRTStartup")]
#[allow(non_snake_case)]
pub extern "system" fn DllMainCRTStartup(hinst: *const u8, reason: u32, reserved: *const u8) -> bool {
    DllMain(hinst, reason, reserved)
}

/// _fltused для FPU support (требуется линкером)
#[unsafe(no_mangle)]
pub static _fltused: i32 = 0;

pub mod display;
pub mod exports;
pub mod font;
pub mod framebuffer;
pub mod palette;
pub mod vid;

pub use framebuffer::*;
pub use palette::*;
pub use vid::*;

// =============================================================================
// Константы
// =============================================================================

/// Ширина символа в пикселях
pub const BOOTCHAR_WIDTH: usize = 8;

/// Высота символа в пикселях  
pub const BOOTCHAR_HEIGHT: usize = 14;

/// Количество глифов в шрифте (ASCII + Box Drawing + Block Elements)
pub const BOOTCHAR_COUNT: usize = 416;

/// Размер данных шрифта в байтах
pub const FONT_DATA_SIZE: usize = BOOTCHAR_COUNT * BOOTCHAR_HEIGHT;

// Re-export Unicode функций для удобства
pub use display::display_unicode_char;
pub use display::display_unicode_char_at;
pub use font::char_to_glyph_index;
pub use font::get_unicode_char_bitmap;
