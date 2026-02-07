//! KD Data - глобальные данные подсистемы отладки
//!
//! Источники:
//! - ReactOS: kd64/kddata.c, kd/kdio.c

use core::sync::atomic::AtomicBool;
use core::sync::atomic::AtomicU32;
use core::sync::atomic::Ordering;

use crate::ke::spinlock::HIGH_LEVEL;
use crate::ke::spinlock::KSPIN_LOCK;
use crate::ke::spinlock::ke_acquire_spin_lock_raise_to;
use crate::ke::spinlock::ke_release_spin_lock;
use crate::ke::spinlock::ke_try_to_acquire_spin_lock_raise_to;

// =============================================================================
// Debugger State
// =============================================================================

/// Отладчик включен
pub static KD_DEBUGGER_ENABLED: AtomicBool = AtomicBool::new(true);

/// Отладчик отсутствует (нет подключения)
pub static KD_DEBUGGER_NOT_PRESENT: AtomicBool =
    AtomicBool::new(!cfg!(feature = "kd-force-present"));

/// Отладчик полностью отключен (boot option)
pub static KD_PITCH_DEBUGGER: AtomicBool = AtomicBool::new(false);

// =============================================================================
// Boot/Platform configuration
// =============================================================================

/// Физический адрес RSDP (ACPI), переданный загрузчиком.
///
/// Нужен для конфигурации KD serial через таблицу SPCR.
static KD_ACPI_RSDP_PHYSICAL: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// Установить физический адрес RSDP (ACPI).
#[inline]
pub fn kd_set_acpi_rsdp_physical(addr: u64) {
    KD_ACPI_RSDP_PHYSICAL.store(addr, Ordering::Release);
}

/// Получить физический адрес RSDP (ACPI).
#[inline]
pub fn kd_get_acpi_rsdp_physical() -> u64 {
    KD_ACPI_RSDP_PHYSICAL.load(Ordering::Acquire)
}

// =============================================================================
// Debug Mode Flags
// =============================================================================

/// Режимы отладки (какие провайдеры включены)
/// Используем атомарные переменные для безопасного доступа
pub struct KdpDebugMode;

impl KdpDebugMode {
    /// Вывод на экран включен
    pub fn screen() -> bool {
        KDP_DEBUG_MODE_SCREEN.load(Ordering::Relaxed)
    }

    /// Вывод в COM порт включен
    pub fn serial() -> bool {
        KDP_DEBUG_MODE_SERIAL.load(Ordering::Relaxed)
    }

    /// Запись в файл включена
    pub fn file() -> bool {
        KDP_DEBUG_MODE_FILE.load(Ordering::Relaxed)
    }

    /// Проверяет, включен ли хотя бы один провайдер
    pub fn any_enabled() -> bool {
        Self::screen() || Self::serial() || Self::file()
    }

    /// Установить режим screen
    pub fn set_screen(enabled: bool) {
        KDP_DEBUG_MODE_SCREEN.store(enabled, Ordering::Release);
    }

    /// Установить режим serial
    pub fn set_serial(enabled: bool) {
        KDP_DEBUG_MODE_SERIAL.store(enabled, Ordering::Release);
    }

    /// Установить режим file
    pub fn set_file(enabled: bool) {
        KDP_DEBUG_MODE_FILE.store(enabled, Ordering::Release);
    }
}

/// Screen mode flag (включается с /SOS)
static KDP_DEBUG_MODE_SCREEN: AtomicBool = AtomicBool::new(false);
/// Serial mode flag (включается с /DEBUG)
static KDP_DEBUG_MODE_SERIAL: AtomicBool = AtomicBool::new(false);
/// File mode flag
static KDP_DEBUG_MODE_FILE: AtomicBool = AtomicBool::new(false);

// =============================================================================
// Serial Port Configuration
// =============================================================================

/// Номер COM порта для отладки (1-4)
pub static SERIAL_PORT_NUMBER: AtomicU32 = AtomicU32::new(1);

/// Базовый адрес COM порта
pub const COM1_BASE: u16 = 0x3F8;
pub const COM2_BASE: u16 = 0x2F8;
pub const COM3_BASE: u16 = 0x3E8;
pub const COM4_BASE: u16 = 0x2E8;

/// Baud rate по умолчанию (115200)
pub const DEFAULT_DEBUG_BAUD_RATE: u32 = 115200;

/// Текущий baud rate
pub static SERIAL_BAUD_RATE: AtomicU32 = AtomicU32::new(DEFAULT_DEBUG_BAUD_RATE);

// =============================================================================
// Circular Buffer (для отложенного вывода и WinDbg .logfile)
// =============================================================================

/// Размер circular buffer по умолчанию (4KB)
pub const KD_DEFAULT_LOG_BUFFER_SIZE: usize = 4096;

/// Circular buffer для логов
pub static mut KD_PRINT_CIRCULAR_BUFFER: [u8; KD_DEFAULT_LOG_BUFFER_SIZE] =
    [0; KD_DEFAULT_LOG_BUFFER_SIZE];

/// Позиция записи в circular buffer
pub static KD_PRINT_WRITE_POSITION: AtomicU32 = AtomicU32::new(0);

/// Размер данных в буфере
pub static KD_PRINT_BUFFER_USED: AtomicU32 = AtomicU32::new(0);

/// Счетчик переполнений буфера
pub static KD_PRINT_ROLLOVER_COUNT: AtomicU32 = AtomicU32::new(0);

// =============================================================================
// Circular Buffer Writer (NT5/ReactOS-style)
// =============================================================================

/// Spin lock для записи в circular buffer.
///
/// В NT это защищено `KdpPrintSpinLock` и захватывается на HIGH_LEVEL,
/// чтобы исключить deadlock при реэнтерабельных `DbgPrint` из IRQ на том же CPU.
static KD_PRINT_LOCK: KSPIN_LOCK = KSPIN_LOCK::new();

/// Захватывает `KD_PRINT_LOCK` на HIGH_LEVEL.
///
/// # Возвращает
/// Предыдущий IRQL (для восстановления).
#[inline]
fn kd_print_acquire_lock() -> crate::hal::irql::KIRQL {
    ke_acquire_spin_lock_raise_to(&KD_PRINT_LOCK, HIGH_LEVEL)
}

/// Пытается захватить `KD_PRINT_LOCK` на HIGH_LEVEL (аварийный путь).
#[inline]
fn kd_print_try_acquire_lock() -> Option<crate::hal::irql::KIRQL> {
    ke_try_to_acquire_spin_lock_raise_to(&KD_PRINT_LOCK, HIGH_LEVEL)
}

#[inline]
fn kd_print_release_lock(old_irql: crate::hal::irql::KIRQL) {
    ke_release_spin_lock(&KD_PRINT_LOCK, old_irql);
}

/// Записывает строку в `KD_PRINT_CIRCULAR_BUFFER`.
///
/// Аутентично NT5/ReactOS:
/// - логирование выполняется независимо от `KdDebuggerNotPresent`
/// - длина ограничивается 512 байт
pub fn kd_log_dbg_print_bytes(bytes: &[u8]) {
    // NT ограничивает debug print строку 512 байтами.
    let mut len = bytes.len();
    if len > 512 {
        len = 512;
    }
    if len == 0 {
        return;
    }

    // NB: в обычном режиме используем блокирующий захват (как NT/ReactOS).
    // Для аварийных контекстов есть `kd_log_dbg_print_bytes_best_effort`.
    let old_irql = kd_print_acquire_lock();
    unsafe {
        let size = KD_DEFAULT_LOG_BUFFER_SIZE;
        let buf_ptr = core::ptr::addr_of_mut!(KD_PRINT_CIRCULAR_BUFFER).cast::<u8>();

        let mut write_pos = KD_PRINT_WRITE_POSITION.load(Ordering::Relaxed) as usize;
        if write_pos >= size {
            write_pos %= size;
        }

        // Пишем с учётом wrap-around.
        let first = core::cmp::min(len, size - write_pos);
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), buf_ptr.add(write_pos), first);
        if first < len {
            let second = len - first;
            core::ptr::copy_nonoverlapping(bytes.as_ptr().add(first), buf_ptr, second);
            write_pos = second;
            // rollover
            KD_PRINT_ROLLOVER_COUNT.fetch_add(1, Ordering::Relaxed);
        } else {
            write_pos += first;
            if write_pos == size {
                write_pos = 0;
                KD_PRINT_ROLLOVER_COUNT.fetch_add(1, Ordering::Relaxed);
            }
        }

        KD_PRINT_WRITE_POSITION.store(write_pos as u32, Ordering::Relaxed);

        // Used bytes (best-effort): в NT это размер буфера, здесь держим "занято" до size.
        let used = KD_PRINT_BUFFER_USED.load(Ordering::Relaxed) as usize;
        let new_used = core::cmp::min(size, used.saturating_add(len));
        KD_PRINT_BUFFER_USED.store(new_used as u32, Ordering::Relaxed);
    }
    kd_print_release_lock(old_irql);
}

/// Best-effort запись в circular buffer.
///
/// Используется в аварийных контекстах (bugcheck/panic/двойной fault),
/// где бесконечный spin на lock недопустим.
pub fn kd_log_dbg_print_bytes_best_effort(bytes: &[u8]) {
    let mut len = bytes.len();
    if len > 512 {
        len = 512;
    }
    if len == 0 {
        return;
    }

    let Some(old_irql) = kd_print_try_acquire_lock() else {
        // Отступление: в аварийном режиме допускаем потерю части лога.
        return;
    };

    unsafe {
        let size = KD_DEFAULT_LOG_BUFFER_SIZE;
        let buf_ptr = core::ptr::addr_of_mut!(KD_PRINT_CIRCULAR_BUFFER).cast::<u8>();

        let mut write_pos = KD_PRINT_WRITE_POSITION.load(Ordering::Relaxed) as usize;
        if write_pos >= size {
            write_pos %= size;
        }

        let first = core::cmp::min(len, size - write_pos);
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), buf_ptr.add(write_pos), first);
        if first < len {
            let second = len - first;
            core::ptr::copy_nonoverlapping(bytes.as_ptr().add(first), buf_ptr, second);
            write_pos = second;
            KD_PRINT_ROLLOVER_COUNT.fetch_add(1, Ordering::Relaxed);
        } else {
            write_pos += first;
            if write_pos == size {
                write_pos = 0;
                KD_PRINT_ROLLOVER_COUNT.fetch_add(1, Ordering::Relaxed);
            }
        }

        KD_PRINT_WRITE_POSITION.store(write_pos as u32, Ordering::Relaxed);
        let used = KD_PRINT_BUFFER_USED.load(Ordering::Relaxed) as usize;
        let new_used = core::cmp::min(size, used.saturating_add(len));
        KD_PRINT_BUFFER_USED.store(new_used as u32, Ordering::Relaxed);
    }

    kd_print_release_lock(old_irql);
}

/// Удобный враппер для `&str`.
#[inline]
pub fn kd_log_dbg_print_str(s: &str) {
    kd_log_dbg_print_bytes(s.as_bytes());
}

/// Best-effort враппер для `&str`.
#[inline]
pub fn kd_log_dbg_print_str_best_effort(s: &str) {
    kd_log_dbg_print_bytes_best_effort(s.as_bytes());
}

// =============================================================================
// Debug Filter Levels (DPFLTR_*_LEVEL)
// =============================================================================

/// Уровень ERROR - всегда выводится
pub const DPFLTR_ERROR_LEVEL: u32 = 0;
/// Уровень WARNING
pub const DPFLTR_WARNING_LEVEL: u32 = 1;
/// Уровень TRACE
pub const DPFLTR_TRACE_LEVEL: u32 = 2;
/// Уровень INFO
pub const DPFLTR_INFO_LEVEL: u32 = 3;

/// Маска для уровней (бит 31)
pub const DPFLTR_MASK: u32 = 0x80000000;

// =============================================================================
// Debug Filter Component IDs (DPFLTR_*_ID)
// =============================================================================

/// Системный компонент
pub const DPFLTR_SYSTEM_ID: u32 = 0;
/// SMSS
pub const DPFLTR_SMSS_ID: u32 = 1;
/// Setup
pub const DPFLTR_SETUP_ID: u32 = 2;
/// NTFS
pub const DPFLTR_NTFS_ID: u32 = 3;
/// FSTUB
pub const DPFLTR_FSTUB_ID: u32 = 4;
/// Crashdump
pub const DPFLTR_CRASHDUMP_ID: u32 = 5;

/// ACPI
pub const DPFLTR_ACPI_ID: u32 = 24;

/// PCI
pub const DPFLTR_PCI_ID: u32 = 65;

/// Memory Manager
pub const DPFLTR_MM_ID: u32 = 101;

/// Default (для неизвестных компонентов)
pub const DPFLTR_DEFAULT_ID: u32 = 100;

/// Максимальный ID компонента
pub const DPFLTR_MAX_ID: u32 = 150;

// =============================================================================
// Component Filter Masks
// =============================================================================

/// Базовая маска для DbgPrint без ComponentId (WIN2000 mask)
/// Бит 0 = ERROR level включен по умолчанию
pub static KD_WIN2000_MASK: AtomicU32 = AtomicU32::new(1);

/// Маска по умолчанию для неизвестных компонентов
pub static KD_DEFAULT_MASK: AtomicU32 = AtomicU32::new(0);

// =============================================================================
// Helper Functions
// =============================================================================

/// Получить базовый адрес COM порта по номеру
pub fn get_com_port_base(port_number: u32) -> u16 {
    match port_number {
        1 => COM1_BASE,
        2 => COM2_BASE,
        3 => COM3_BASE,
        4 => COM4_BASE,
        _ => COM1_BASE,
    }
}

/// Проверить, включен ли отладчик
#[inline]
pub fn kd_debugger_enabled() -> bool {
    KD_DEBUGGER_ENABLED.load(Ordering::Relaxed) && !KD_PITCH_DEBUGGER.load(Ordering::Relaxed)
}

/// Проверить, присутствует ли отладчик
#[inline]
pub fn kd_debugger_not_present() -> bool {
    KD_DEBUGGER_NOT_PRESENT.load(Ordering::Relaxed)
}

// =============================================================================
// Load Options Parsing
// =============================================================================

/// Инициализирует режимы KD на основе Load Options из командной строки загрузчика.
///
/// Поддерживаемые параметры:
/// - `/DEBUG` - включает вывод в serial порт
/// - `/SOS` - включает отображение лога загрузки на экран
///
/// # Safety
/// `load_options` должен быть валидным указателем на строку LoadOptions,
/// `load_options_length` - её длина в байтах.
///
/// # Returns
/// Кортеж (debug_found, sos_found) - найденные параметры
pub unsafe fn kd_init_from_load_options(
    load_options: *const u8,
    load_options_length: u32,
) -> (bool, bool) {
    let mut debug_found = false;
    let mut sos_found = false;

    if !load_options.is_null() && load_options_length > 0 {
        let options_len = load_options_length as usize;
        let options = unsafe { core::slice::from_raw_parts(load_options, options_len) };

        debug_found = find_load_option(options, b"/DEBUG");
        sos_found = find_load_option(options, b"/SOS");
    }

    // Устанавливаем режимы
    if debug_found {
        KdpDebugMode::set_serial(true);
        // Также помечаем отладчик как присутствующий для /DEBUG
        KD_DEBUGGER_NOT_PRESENT.store(false, Ordering::Release);
    }

    if sos_found {
        KdpDebugMode::set_screen(true);
    }

    (debug_found, sos_found)
}

/// Ищет ключ в Load Options (case-insensitive для ASCII).
/// Ключ должен быть отдельным словом (ограничен пробелами или началом/концом строки).
fn find_load_option(options: &[u8], key: &[u8]) -> bool {
    if key.is_empty() || options.len() < key.len() {
        return false;
    }

    // Скользящее окно по строке
    let mut i = 0;
    while i <= options.len() - key.len() {
        // Пропускаем пробелы
        if options[i] == b' ' {
            i += 1;
            continue;
        }

        // Проверяем совпадение ключа (case-insensitive)
        let mut matched = true;
        for j in 0..key.len() {
            let a = options[i + j].to_ascii_uppercase();
            let b = key[j].to_ascii_uppercase();
            if a != b {
                matched = false;
                break;
            }
        }

        if matched {
            // Проверяем что это отдельное слово
            let at_start = i == 0 || options[i - 1] == b' ';
            let at_end = i + key.len() == options.len()
                || options[i + key.len()] == b' '
                || options[i + key.len()] == b'\0';

            if at_start && at_end {
                return true;
            }
        }

        // Переходим к следующему слову
        while i < options.len() && options[i] != b' ' && options[i] != b'\0' {
            i += 1;
        }
    }

    false
}
