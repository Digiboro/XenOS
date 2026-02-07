//! KD Screen Provider
//!
//! Экранный провайдер для KD (аналог KdpScreenPrint в ReactOS).
//! Выводит debug сообщения на экран через INBV/BOOTVID.
//!
//! # Особенности
//!
//! - Буферизация строк для корректного вывода
//! - Обработка backspace (HalDisplayString не поддерживает его напрямую)
//! - Работает только когда INBV владеет дисплеем
//!
//! # Отступления и упрощения
//!
//! - Буфер строки хранится per-CPU (в NT/ReactOS это глобальная строка для screen provider).
//!   Это упрощает поддержку SMP и снижает порчу строки при параллельной печати.
//! - Для предсказуемости весь вывод на экран сериализован глобальным lock'ом
//!   (как и serial вывод в KD).

use super::data::KdpDebugMode;
use crate::hal::irql::KIRQL;
use crate::inbv;
use crate::ke::init::MAX_PROCESSORS;
use crate::ke::spinlock::HIGH_LEVEL;
use crate::ke::spinlock::KSPIN_LOCK;
use crate::ke::spinlock::ke_acquire_spin_lock_raise_to;
use crate::ke::spinlock::ke_release_spin_lock;

/// Максимальная длина строки для буферизации
const KDP_SCREEN_LINE_LENGTH: usize = 80;

/// Глобальный lock для сериализации экранного вывода.
///
/// Захватывается на HIGH_LEVEL, чтобы исключить deadlock при реэнтерабельном выводе из IRQ.
static KDP_SCREEN_LOCK: KSPIN_LOCK = KSPIN_LOCK::new();

/// Захватывает глобальный lock экранного вывода (HIGH_LEVEL).
///
/// Используется для «транзакций» (например, смена цвета + печать + восстановление),
/// чтобы исключить перемешивание со сторонними сообщениями.
pub(crate) fn kdp_screen_enter_lock() -> KIRQL {
    ke_acquire_spin_lock_raise_to(&KDP_SCREEN_LOCK, HIGH_LEVEL)
}

/// Освобождает глобальный lock экранного вывода.
pub(crate) fn kdp_screen_leave_lock(old_irql: KIRQL) {
    ke_release_spin_lock(&KDP_SCREEN_LOCK, old_irql);
}

/// Состояние буфера строки для конкретного CPU.
#[derive(Clone, Copy)]
struct KdpScreenLineState {
    buf: [u8; KDP_SCREEN_LINE_LENGTH + 1],
    len: usize,
    printed: usize,
}

impl KdpScreenLineState {
    pub const fn new() -> Self {
        Self {
            buf: [0; KDP_SCREEN_LINE_LENGTH + 1],
            len: 0,
            printed: 0,
        }
    }

    #[inline]
    pub fn reset(&mut self) {
        self.len = 0;
        self.printed = 0;
        // NB: буфер чистить не обязательно, достаточно len/printed.
        self.buf[0] = 0;
    }
}

/// Per-CPU буферы строки.
///
/// SAFETY: доступ к элементу должен быть сериализован (через `KDP_SCREEN_LOCK`)
/// или гарантированно происходить только на одном CPU без реэнтерабельности.
static mut KDP_SCREEN_PER_CPU: [KdpScreenLineState; MAX_PROCESSORS] =
    [const { KdpScreenLineState::new() }; MAX_PROCESSORS];

#[inline]
fn kdp_screen_current_cpu_index() -> usize {
    let cpu = unsafe { crate::arch::x86_64::pcr::get_processor_number() as usize };
    if cpu < MAX_PROCESSORS { cpu } else { 0 }
}

/// Flush буфера текущего CPU при уже захваченном `KDP_SCREEN_LOCK`.
pub(crate) fn kdp_screen_flush_locked() {
    let cpu = kdp_screen_current_cpu_index();
    let st = unsafe { kdp_screen_state_ptr(cpu) };
    unsafe { kdp_screen_flush_pending_locked(st) };
}

/// Печать строки при уже захваченном `KDP_SCREEN_LOCK`.
pub(crate) fn kdp_screen_print_str_locked(s: &str) {
    let cpu = kdp_screen_current_cpu_index();
    let st = unsafe { kdp_screen_state_ptr(cpu) };
    unsafe { kdp_screen_print_utf8_locked(st, s) };
}

/// Возвращает raw pointer на per-CPU состояние строки.
///
/// # Safety
/// `cpu` должен быть < MAX_PROCESSORS.
#[inline]
unsafe fn kdp_screen_state_ptr(cpu: usize) -> *mut KdpScreenLineState {
    unsafe {
        core::ptr::addr_of_mut!(KDP_SCREEN_PER_CPU)
            .cast::<KdpScreenLineState>()
            .add(cpu)
    }
}

// =============================================================================
// Инициализация
// =============================================================================

/// Инициализация KD Screen Provider
///
/// Настраивает экран для вывода debug информации.
///
/// # Arguments
/// * `width` - ширина экрана в пикселях
/// * `height` - высота экрана в пикселях
///
/// # Returns
/// `true` если инициализация успешна
pub fn kdp_screen_init(width: u32, height: u32) -> bool {
    if !KdpDebugMode::screen() {
        return false;
    }

    if !inbv::inbv_is_boot_driver_installed() {
        return false;
    }

    // Захватываем дисплей
    inbv::inbv_acquire_display_ownership();

    // Настраиваем экран для debug вывода
    inbv::inbv_setup_display_for_debug(width, height);

    // Сбрасываем буферы всех CPU (best-effort, но безопасно на раннем этапе).
    let old_irql = ke_acquire_spin_lock_raise_to(&KDP_SCREEN_LOCK, HIGH_LEVEL);
    unsafe {
        for i in 0..MAX_PROCESSORS {
            let st = kdp_screen_state_ptr(i);
            (*st).reset();
        }
    }
    ke_release_spin_lock(&KDP_SCREEN_LOCK, old_irql);

    true
}

// =============================================================================
// Основные функции
// =============================================================================

/// Вывод строки на экран
///
/// Основная функция KD screen provider.
/// Обрабатывает специальные символы:
/// - `\n` - перевод строки (flush буфера)
/// - `\r` - возврат каретки
/// - `\b` - backspace (удаление последнего символа)
///
/// # Arguments
/// * `string` - строка для вывода (байты ASCII)
pub fn kdp_screen_print(string: &[u8]) {
    if !KdpDebugMode::screen() {
        return;
    }

    // В NT/ReactOS печать возможна только когда INBV реально владеет дисплеем (OWNED),
    // а не просто "не LOST".
    if inbv::inbv_get_display_state() != inbv::InbvDisplayState::Owned {
        return;
    }

    // Глобальная сериализация вывода (см. принятые решения).
    let old_irql = ke_acquire_spin_lock_raise_to(&KDP_SCREEN_LOCK, HIGH_LEVEL);
    let cpu = kdp_screen_current_cpu_index();
    let st = unsafe { kdp_screen_state_ptr(cpu) };
    unsafe { kdp_screen_print_locked(st, string) };

    ke_release_spin_lock(&KDP_SCREEN_LOCK, old_irql);
}

/// Вывод строки на экран (string slice версия)
pub fn kdp_screen_print_str(string: &str) {
    if !KdpDebugMode::screen() {
        return;
    }

    if inbv::inbv_get_display_state() != inbv::InbvDisplayState::Owned {
        return;
    }

    let old_irql = ke_acquire_spin_lock_raise_to(&KDP_SCREEN_LOCK, HIGH_LEVEL);
    let cpu = kdp_screen_current_cpu_index();
    let st = unsafe { kdp_screen_state_ptr(cpu) };
    unsafe { kdp_screen_print_utf8_locked(st, string) };

    ke_release_spin_lock(&KDP_SCREEN_LOCK, old_irql);
}

// =============================================================================
// Внутренние функции буферизации
// =============================================================================

/// Внутренний вывод UTF-8 строки при уже захваченном `KDP_SCREEN_LOCK`.
/// 
/// Работает с целыми UTF-8 символами, не разбивая multi-byte последовательности.
unsafe fn kdp_screen_print_utf8_locked(st: *mut KdpScreenLineState, string: &str) {
    unsafe {
        for ch in string.chars() {
            match ch {
                '\n' => {
                    kdp_screen_flush_line_locked(st);
                    let _ = inbv::inbv_display_string("\n");
                },
                '\r' => {
                    // \r = начало строки. Сбрасываем буфер и печатаем \r.
                    (*st).reset();
                    let _ = inbv::inbv_display_string("\r");
                },
                '\x08' => {
                    kdp_screen_handle_backspace_locked(st);
                },
                _ => {
                    kdp_screen_add_utf8_char_locked(st, ch);
                },
            }
        }

        // Допечатываем «хвост» буфера, который ещё не был выведен.
        kdp_screen_flush_pending_locked(st);
    }
}

/// Внутренний вывод строки при уже захваченном `KDP_SCREEN_LOCK` (байтовая версия).
unsafe fn kdp_screen_print_locked(st: *mut KdpScreenLineState, string: &[u8]) {
    // Пытаемся интерпретировать как UTF-8
    if let Ok(s) = core::str::from_utf8(string) {
        kdp_screen_print_utf8_locked(st, s);
    } else {
        // Fallback для невалидного UTF-8: побайтово
        unsafe {
            for &ch in string {
                match ch {
                    b'\n' => {
                        kdp_screen_flush_line_locked(st);
                        let _ = inbv::inbv_display_string("\n");
                    },
                    b'\r' => {
                        (*st).reset();
                        let _ = inbv::inbv_display_string("\r");
                    },
                    0x08 => {
                        kdp_screen_handle_backspace_locked(st);
                    },
                    _ => {
                        kdp_screen_add_byte_locked(st, ch);
                    },
                }
            }
            kdp_screen_flush_pending_locked(st);
        }
    }
}

/// Добавить UTF-8 символ в буфер (может занимать 1-4 байта)
#[inline]
unsafe fn kdp_screen_add_utf8_char_locked(st: *mut KdpScreenLineState, ch: char) {
    unsafe {
        let mut buf = [0u8; 4];
        let encoded = ch.encode_utf8(&mut buf);
        let char_len = encoded.len();
        
        // Проверяем, поместится ли символ в буфер
        if (*st).len + char_len > KDP_SCREEN_LINE_LENGTH {
            kdp_screen_flush_line_locked(st);
        }
        
        // Копируем байты символа в буфер
        for &b in encoded.as_bytes() {
            (*st).buf[(*st).len] = b;
            (*st).len += 1;
        }
        (*st).buf[(*st).len] = 0;

        // Печатаем инкрементально после каждого символа
        kdp_screen_flush_pending_locked(st);
    }
}

/// Добавить один байт в буфер (для fallback режима)
#[inline]
unsafe fn kdp_screen_add_byte_locked(st: *mut KdpScreenLineState, ch: u8) {
    unsafe {
        if (*st).len >= KDP_SCREEN_LINE_LENGTH {
            kdp_screen_flush_line_locked(st);
        }
        (*st).buf[(*st).len] = ch;
        (*st).len += 1;
        (*st).buf[(*st).len] = 0;

        // NT/ReactOS: печатаем инкрементально
        kdp_screen_flush_pending_locked(st);
    }
}

#[inline]
unsafe fn kdp_screen_flush_pending_locked(st: *mut KdpScreenLineState) {
    unsafe {
        if (*st).printed == (*st).len {
            return;
        }

        // NB: делаем borrow явно, чтобы не попасть под `dangerous_implicit_autorefs` (Rust 2024).
        let slice = &(&(*st).buf)[(*st).printed..(*st).len];
        let s = unsafe { core::str::from_utf8_unchecked(slice) };
        let _ = inbv::inbv_display_string(s);
        (*st).printed = (*st).len;
    }
}

#[inline]
unsafe fn kdp_screen_flush_line_locked(st: *mut KdpScreenLineState) {
    unsafe {
        // Выводим то, что ещё не успели вывести, и очищаем буфер строки.
        kdp_screen_flush_pending_locked(st);
        (*st).reset();
    }
}

/// Публичный flush буфера
///
/// Используется когда нужно немедленно вывести буфер на экран,
/// например перед сменой цвета текста.
pub fn kdp_screen_flush() {
    if !KdpDebugMode::screen() {
        return;
    }

    if inbv::inbv_get_display_state() != inbv::InbvDisplayState::Owned {
        return;
    }

    let old_irql = ke_acquire_spin_lock_raise_to(&KDP_SCREEN_LOCK, HIGH_LEVEL);
    let cpu = kdp_screen_current_cpu_index();
    let st = unsafe { kdp_screen_state_ptr(cpu) };
    unsafe { kdp_screen_flush_pending_locked(st) };
    ke_release_spin_lock(&KDP_SCREEN_LOCK, old_irql);
}

#[inline]
unsafe fn kdp_screen_handle_backspace_locked(st: *mut KdpScreenLineState) {
    unsafe {
        if (*st).len == 0 {
            return;
        }

        (*st).len -= 1;
        (*st).buf[(*st).len] = 0;
        if (*st).printed > (*st).len {
            (*st).printed = (*st).len;
        }

        // ReactOS-паттерн: \r + перепечатка строки (HalDisplayString не поддерживает '\b').
        let _ = inbv::inbv_display_string("\r");
        // NB: делаем borrow явно, чтобы не попасть под `dangerous_implicit_autorefs` (Rust 2024).
        let s = unsafe { core::str::from_utf8_unchecked(&(&(*st).buf)[..(*st).len]) };
        let _ = inbv::inbv_display_string(s);
        (*st).printed = (*st).len;
    }
}

// =============================================================================
// Вспомогательные функции
// =============================================================================

/// Очистка экрана и сброс состояния
pub fn kdp_screen_clear() {
    if !inbv::inbv_is_boot_driver_installed() {
        return;
    }

    inbv::inbv_reset_display();
    let old_irql = ke_acquire_spin_lock_raise_to(&KDP_SCREEN_LOCK, HIGH_LEVEL);
    unsafe {
        for i in 0..MAX_PROCESSORS {
            let st = kdp_screen_state_ptr(i);
            (*st).reset();
        }
    }
    ke_release_spin_lock(&KDP_SCREEN_LOCK, old_irql);
}

/// Проверка готовности screen provider
pub fn kdp_screen_ready() -> bool {
    KdpDebugMode::screen()
        && inbv::inbv_is_boot_driver_installed()
        && inbv::inbv_get_display_state() == inbv::InbvDisplayState::Owned
}
