//! KD Print - функции отладочного вывода
//!
//! DbgPrint, DbgPrintEx, KdPrint!
//!
//! Источники:
//! - ReactOS: sdk/lib/rtl/debug.c
//! - ReactOS: kd64/kdprint.c

use core::fmt::Write;
use core::fmt::{self};

// =============================================================================
// Цветовые константы (реэкспорт из imports::bootvid)
// =============================================================================
pub use crate::imports::bootvid::{
    BV_COLOR_BLACK, BV_COLOR_BLUE, BV_COLOR_CYAN, BV_COLOR_DARK_GRAY, BV_COLOR_GREEN,
    BV_COLOR_LIGHT_BLUE, BV_COLOR_LIGHT_CYAN, BV_COLOR_LIGHT_GRAY, BV_COLOR_LIGHT_GREEN,
    BV_COLOR_LIGHT_MAGENTA, BV_COLOR_LIGHT_RED, BV_COLOR_LIGHT_YELLOW, BV_COLOR_MAGENTA,
    BV_COLOR_RED, BV_COLOR_WHITE, BV_COLOR_YELLOW,
};

use super::data::*;
use super::screen::*;
use super::serial::*;
use crate::hal::irql::KIRQL;
use crate::inbv;
use crate::ke::spinlock::HIGH_LEVEL;
use crate::ke::spinlock::KSPIN_LOCK;
use crate::ke::spinlock::ke_acquire_spin_lock_raise_to;
use crate::ke::spinlock::ke_release_spin_lock;

// =============================================================================
// Глобальный spinlock для атомарного вывода
// =============================================================================

/// Глобальный lock для сериализации форматированного вывода.
///
/// Захватывается в `dbg_print_args` ДО форматирования, чтобы весь вывод
/// форматированной строки был атомарным и не перемешивался с выводом
/// других потоков/CPU.
static KD_PRINT_LOCK: KSPIN_LOCK = KSPIN_LOCK::new();

// =============================================================================
// Internal Print Implementation
// =============================================================================

/// Внутренняя функция вывода строки через все провайдеры
fn kd_io_print_string(s: &str) {
    // NT5/ReactOS: всегда логируем DbgPrint в circular buffer.
    // Вывод "наружу" зависит от наличия подключенного отладчика.
    kd_log_dbg_print_str(s);

    // Проверяем режим отладки (какие провайдеры активны)
    if !KdpDebugMode::any_enabled() {
        return;
    }

    // Serial провайдер (KD transport backend)
    // Требует подключенного отладчика (NT-поведение)
    if KdpDebugMode::serial() {
        if kd_debugger_enabled() && !kd_debugger_not_present() {
            kdp_serial_print_str(s);
        }
    }

    // Screen провайдер (/SOS режим)
    // Работает независимо от наличия отладчика - это вывод на экран для пользователя,
    // аналог NT /SOS (HalDisplayString/InbvDisplayString)
    if KdpDebugMode::screen() {
        kdp_screen_print_str(s);
    }

    // File провайдер (TODO)
    // if KdpDebugMode::file() {
    //     kdp_file_print(s);
    // }
}

/// Внутренняя функция вывода строки при уже захваченном `KD_PRINT_LOCK`.
///
/// Используется из `DbgPrintWriter` для атомарного форматированного вывода.
fn kd_io_print_string_locked(s: &str) {
    // Логируем в circular buffer
    kd_log_dbg_print_str(s);

    if !KdpDebugMode::any_enabled() {
        return;
    }

    // Serial - используем _locked версию (без внутреннего spinlock)
    if KdpDebugMode::serial() {
        if kd_debugger_enabled() && !kd_debugger_not_present() {
            kdp_serial_print_str_locked(s);
        }
    }

    // Screen - используем _locked версию
    if KdpDebugMode::screen() {
        kdp_screen_print_str_locked(s);
    }
}

/// Проверка debug filter state
///
/// Возвращает true если сообщение с данным ComponentId и Level должно быть выведено
pub fn nt_query_debug_filter_state(component_id: u32, level: u32) -> bool {
    use core::sync::atomic::Ordering;

    // Получаем маску для компонента
    let mask = if component_id == u32::MAX {
        // DbgPrint без ComponentId использует WIN2000 mask
        KD_WIN2000_MASK.load(Ordering::Relaxed)
    } else if component_id < DPFLTR_MAX_ID {
        // TODO: Реализовать таблицу масок для каждого компонента
        // Пока используем default mask
        KD_DEFAULT_MASK.load(Ordering::Relaxed)
    } else {
        KD_DEFAULT_MASK.load(Ordering::Relaxed)
    };

    // Преобразуем Level в битовую маску если нужно
    let level_mask = if level < 32 {
        1u32 << level
    } else {
        level & !DPFLTR_MASK
    };

    // Проверяем: глобальная маска (WIN2000) OR маска компонента
    let win2000_mask = KD_WIN2000_MASK.load(Ordering::Relaxed);

    (win2000_mask & level_mask) != 0 || (mask & level_mask) != 0
}

// =============================================================================
// DbgPrint Writer (для fmt::Write trait)
// =============================================================================

/// Writer для форматированного вывода с синхронизацией.
///
/// Захватывает `KD_PRINT_LOCK` при создании и освобождает при уничтожении,
/// обеспечивая атомарный вывод всей форматированной строки.
struct DbgPrintWriter {
    old_irql: KIRQL,
}

impl DbgPrintWriter {
    /// Создаёт writer, захватывая глобальный lock.
    fn new() -> Self {
        let old_irql = ke_acquire_spin_lock_raise_to(&KD_PRINT_LOCK, HIGH_LEVEL);
        Self { old_irql }
    }
}

impl Drop for DbgPrintWriter {
    fn drop(&mut self) {
        ke_release_spin_lock(&KD_PRINT_LOCK, self.old_irql);
    }
}

impl Write for DbgPrintWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        kd_io_print_string_locked(s);
        Ok(())
    }
}

// =============================================================================
// Public API - DbgPrint functions
// =============================================================================

/// DbgPrint - базовая функция отладочного вывода
///
/// Эквивалент DbgPrint() в Windows NT.
/// Всегда выводит на уровне DPFLTR_ERROR_LEVEL.
///
/// Вывод синхронизирован глобальным lock'ом для предотвращения
/// перемешивания с выводом других потоков.
pub fn dbg_print(s: &str) {
    // DbgPrint использует ComponentId = MAXULONG и Level = ERROR
    if nt_query_debug_filter_state(u32::MAX, DPFLTR_ERROR_LEVEL) {
        let old_irql = ke_acquire_spin_lock_raise_to(&KD_PRINT_LOCK, HIGH_LEVEL);
        kd_io_print_string_locked(s);
        ke_release_spin_lock(&KD_PRINT_LOCK, old_irql);
    }
}

/// DbgPrintEx - расширенная функция отладочного вывода
///
/// Позволяет указать компонент и уровень для фильтрации.
pub fn dbg_print_ex(component_id: u32, level: u32, s: &str) {
    if nt_query_debug_filter_state(component_id, level) {
        let old_irql = ke_acquire_spin_lock_raise_to(&KD_PRINT_LOCK, HIGH_LEVEL);
        kd_io_print_string_locked(s);
        ke_release_spin_lock(&KD_PRINT_LOCK, old_irql);
    }
}

/// Форматированный DbgPrint (использует alloc)
#[cfg(feature = "alloc")]
pub fn dbg_print_fmt(args: fmt::Arguments) {
    use alloc::string::ToString;
    if nt_query_debug_filter_state(u32::MAX, DPFLTR_ERROR_LEVEL) {
        let s = args.to_string();
        let old_irql = ke_acquire_spin_lock_raise_to(&KD_PRINT_LOCK, HIGH_LEVEL);
        kd_io_print_string_locked(&s);
        ke_release_spin_lock(&KD_PRINT_LOCK, old_irql);
    }
}

/// Форматированный вывод без аллокации (пишет напрямую)
///
/// Lock захватывается ДО форматирования и освобождается ПОСЛЕ,
/// что гарантирует атомарный вывод всей строки.
pub fn dbg_print_args(args: fmt::Arguments) {
    if nt_query_debug_filter_state(u32::MAX, DPFLTR_ERROR_LEVEL) {
        let mut writer = DbgPrintWriter::new();
        let _ = writer.write_fmt(args);
        // Lock освобождается при drop'е writer'а
    }
}

// =============================================================================
// Convenience Functions
// =============================================================================

/// Вывести строку с новой строкой
pub fn dbg_println(s: &str) {
    dbg_print(s);
    dbg_print("\n");
}

// =============================================================================
// Colored Output Functions
// =============================================================================

/// Установить цвет текста для screen провайдера
///
/// # Arguments
/// * `color` - индекс цвета из палитры (0-15)
///
/// # Returns
/// Предыдущий цвет
#[inline]
pub fn dbg_set_color(color: u8) -> u8 {
    inbv::inbv_set_text_color(color)
}

/// Принудительный flush буфера screen провайдера
///
/// Используется перед сменой цвета текста, чтобы буферизованный текст
/// был выведен с текущим цветом.
#[inline]
pub fn dbg_flush() {
    super::screen::kdp_screen_flush();
}

/// Вывести строку указанным цветом (цвет восстанавливается после)
pub fn dbg_print_colored(s: &str, color: u8) {
    // Сохраняем семантику DbgPrint: фильтрация + логирование всегда.
    if !nt_query_debug_filter_state(u32::MAX, DPFLTR_ERROR_LEVEL) {
        return;
    }

    // 1) Логируем всегда (NT-поведение).
    kd_log_dbg_print_str(s);

    // 2) Если провайдеры отключены — всё.
    if !KdpDebugMode::any_enabled() {
        return;
    }

    // 3) Serial (KD transport) — как обычно, без цвета.
    if KdpDebugMode::serial() {
        if kd_debugger_enabled() && !kd_debugger_not_present() {
            kdp_serial_print_str(s);
        }
    }

    // 4) Screen (/SOS) — делаем «цветовую транзакцию» под глобальным screen lock,
    // чтобы чужой вывод не “украл” наш цвет.
    if KdpDebugMode::screen() && inbv::inbv_get_display_state() == inbv::InbvDisplayState::Owned {
        let old_irql = super::screen::kdp_screen_enter_lock();

        // Flush буфера ПЕРЕД сменой цвета (чтобы предыдущий текст вывелся текущим цветом).
        super::screen::kdp_screen_flush_locked();
        let prev_color = dbg_set_color(color);

        // Печатаем строку напрямую в screen provider (lock уже держим).
        super::screen::kdp_screen_print_str_locked(s);
        super::screen::kdp_screen_flush_locked();

        dbg_set_color(prev_color);
        super::screen::kdp_screen_leave_lock(old_irql);
    }
}

/// Вывести "OK" зелёным цветом
pub fn dbg_print_ok() {
    dbg_print_colored("OK", BV_COLOR_LIGHT_GREEN);
}

/// Вывести "FAILED" красным цветом
pub fn dbg_print_fail() {
    dbg_print_colored("FAILED", BV_COLOR_LIGHT_RED);
}

/// Вывести "SKIPPED" жёлтым цветом
pub fn dbg_print_skip() {
    dbg_print_colored("SKIPPED", BV_COLOR_LIGHT_YELLOW);
}

/// Вывести "WARNING" жёлтым цветом
pub fn dbg_print_warn(msg: &str) {
    dbg_print_colored("[WARNING] ", BV_COLOR_LIGHT_YELLOW);
    dbg_print(msg);
}

/// Вывести "ERROR" красным цветом
pub fn dbg_print_error(msg: &str) {
    dbg_print_colored("[ERROR] ", BV_COLOR_LIGHT_RED);
    dbg_print(msg);
}

/// Вывести число (через все провайдеры)
pub fn dbg_print_num(n: u64) {
    // Конвертируем число в строку
    let mut buf = [0u8; 20];
    let s = format_u64(n, &mut buf);
    dbg_print(s);
}

/// Вывести число в hex (зелёным цветом, без префикса 0x)
pub fn dbg_print_hex(n: u64) {
    // Конвертируем число в hex строку
    let mut buf = [0u8; 16];
    let s = format_hex(n, &mut buf);
    dbg_print_colored(s, BV_COLOR_LIGHT_GREEN);
}

/// Форматирование u64 в строку (без аллокации)
fn format_u64(n: u64, buf: &mut [u8; 20]) -> &str {
    if n == 0 {
        return "0";
    }

    let mut num = n;
    let mut pos = buf.len();

    while num > 0 && pos > 0 {
        pos -= 1;
        buf[pos] = b'0' + (num % 10) as u8;
        num /= 10;
    }

    // SAFETY: мы записали только ASCII цифры
    unsafe { core::str::from_utf8_unchecked(&buf[pos..]) }
}

/// Форматирование u64 в hex строку (без аллокации, без префикса 0x)
fn format_hex(n: u64, buf: &mut [u8; 16]) -> &str {
    const HEX_CHARS: &[u8; 16] = b"0123456789ABCDEF";

    // Заполняем hex цифры
    for i in 0..16 {
        let nibble = ((n >> (60 - i * 4)) & 0xF) as usize;
        buf[i] = HEX_CHARS[nibble];
    }

    // SAFETY: мы записали только ASCII символы
    unsafe { core::str::from_utf8_unchecked(&buf[..]) }
}

/// Вывести размер в человекочитаемом формате
pub fn dbg_print_size(bytes: u64) {
    if bytes >= 1024 * 1024 * 1024 {
        dbg_print_num(bytes / (1024 * 1024 * 1024));
        dbg_print(" GB");
    } else if bytes >= 1024 * 1024 {
        dbg_print_num(bytes / (1024 * 1024));
        dbg_print(" MB");
    } else if bytes >= 1024 {
        dbg_print_num(bytes / 1024);
        dbg_print(" KB");
    } else {
        dbg_print_num(bytes);
        dbg_print(" B");
    }
}

// =============================================================================
// KdPrint Macros
// =============================================================================

/// KdPrint! - макрос для отладочного вывода
///
/// В release сборках раскрывается в пустую операцию.
///
/// # Пример
/// ```
/// kd_print!("Hello, World!\n");
/// kd_print!("Value: {}\n", 42);
/// ```
#[macro_export]
macro_rules! kd_print {
    ($($arg:tt)*) => {
        #[cfg(debug_assertions)]
        {
            $crate::kd::dbg_print_args(format_args!($($arg)*));
        }
    };
}

/// KdPrintEx! - макрос с указанием компонента и уровня
#[macro_export]
macro_rules! kd_print_ex {
    ($component:expr, $level:expr, $($arg:tt)*) => {
        #[cfg(debug_assertions)]
        {
            if $crate::kd::nt_query_debug_filter_state($component, $level) {
                $crate::kd::dbg_print_args(format_args!($($arg)*));
            }
        }
    };
}

/// DbgPrint! - всегда активный макрос (даже в release)
#[macro_export]
macro_rules! dbg_print {
    ($($arg:tt)*) => {
        $crate::kd::dbg_print_args(format_args!($($arg)*));
    };
}

// =============================================================================
// KD Initialization
// =============================================================================

/// Инициализация KD подсистемы
pub fn kd_init_system() {
    // Инициализируем serial провайдер:
    // - конфигурация берётся из ACPI SPCR (если доступна)
    // - иначе используем дефолт COM1/115200
    serial_init_from_spcr_or_default();
}
