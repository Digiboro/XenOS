//! Типы данных для winload
//!
//! Структуры для хранения boot context и загруженных файлов.

use alloc::vec::Vec;

// =============================================================================
// Константы путей к файлам
// =============================================================================

/// Путь к SYSTEM hive на ESP
pub const SYSTEM_HIVE_PATH: &str = "\\XenOS\\system32\\config\\SYSTEM";

/// Путь к ядру на ESP
pub const NTOSKRNL_PATH: &str = "\\boot\\ntoskrnl.exe";

/// Путь к unicode.nls (NLS данные для Unicode) на ESP
pub const UNICODE_NLS_PATH: &str = "\\XenOS\\system32\\unicode.nls";

// =============================================================================
// NTSTATUS коды для BSOD
// =============================================================================

/// STATUS_CANNOT_LOAD_REGISTRY_FILE (0xC0000218)
pub const STATUS_CANNOT_LOAD_REGISTRY_FILE: u32 = 0xC0000218;

/// STATUS_INVALID_IMAGE_FORMAT (0xC000007B)
pub const STATUS_INVALID_IMAGE_FORMAT: u32 = 0xC000007B;

/// STATUS_NO_MEMORY (0xC0000017)
pub const STATUS_NO_MEMORY: u32 = 0xC0000017;

/// Максимальный размер unicode.nls (защитный лимит: 2 MB)
pub const MAX_UNICODE_NLS_SIZE: usize = 2 * 1024 * 1024;

// =============================================================================
// Структуры данных
// =============================================================================

/// Максимальный размер строки Load Options (NT-style: 256 символов)
pub const MAX_LOAD_OPTIONS_LENGTH: usize = 256;

/// Контекст загрузки, собранный из UEFI Boot Services.
/// На следующих шагах будет расширен до полноценного LOADER_PARAMETER_BLOCK.
pub struct BootContext {
    /// Командная строка загрузки (Load Options, ASCII).
    /// Буфер фиксированного размера, null-terminated.
    pub load_options_buffer: [u8; MAX_LOAD_OPTIONS_LENGTH],
    /// Длина строки Load Options (без null-терминатора).
    pub load_options_length: usize,

    /// Параметры framebuffer (GOP).
    pub framebuffer: Option<FramebufferInfo>,

    /// Количество записей в UEFI memory map.
    pub memory_map_entries: usize,

    /// Флаг успешного открытия файловой системы.
    pub fs_available: bool,

    /// Физический адрес ACPI RSDP.
    pub acpi_rsdp: Option<u64>,
}

impl BootContext {
    /// Проверяет наличие ключа в Load Options (case-insensitive).
    /// Ключ должен начинаться с '/' (например, "/DEBUG").
    pub fn has_load_option(&self, key: &str) -> bool {
        if self.load_options_length == 0 {
            return false;
        }
        // Простой поиск подстроки (case-insensitive для ASCII)
        let options = &self.load_options_buffer[..self.load_options_length];
        let key_bytes = key.as_bytes();
        if key_bytes.is_empty() || options.len() < key_bytes.len() {
            return false;
        }
        // Скользящее окно
        for i in 0..=(options.len() - key_bytes.len()) {
            let mut found = true;
            for j in 0..key_bytes.len() {
                let a = options[i + j].to_ascii_uppercase();
                let b = key_bytes[j].to_ascii_uppercase();
                if a != b {
                    found = false;
                    break;
                }
            }
            if found {
                // Проверяем что это отдельное слово (пробел/начало/конец)
                let at_start = i == 0 || options[i - 1] == b' ';
                let at_end = i + key_bytes.len() == options.len()
                    || options[i + key_bytes.len()] == b' '
                    || options[i + key_bytes.len()] == b'\0';
                if at_start && at_end {
                    return true;
                }
            }
        }
        false
    }
}

/// Информация о framebuffer из GOP.
#[derive(Debug, Clone, Copy)]
pub struct FramebufferInfo {
    /// Физический адрес framebuffer.
    pub base: u64,
    /// Размер framebuffer в байтах.
    pub size: usize,
    /// Ширина в пикселях.
    pub width: usize,
    /// Высота в пикселях.
    pub height: usize,
    /// Stride (количество пикселей на строку, включая padding).
    pub stride: usize,
}

/// Загруженный файл в памяти.
pub struct LoadedFile {
    /// Данные файла.
    pub data: Vec<u8>,
    /// Путь к файлу (для диагностики).
    #[allow(dead_code)]
    pub path: &'static str,
}