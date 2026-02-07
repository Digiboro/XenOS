//! CMOS RTC - Real Time Clock
//!
//! Чтение реального времени из CMOS RTC через порты 0x70/0x71.
//!
//! # Архитектура
//!
//! ```text
//!     ┌─────────────────────────────────────────────────────────────────┐
//!     │                      CMOS RTC (MC146818)                        │
//!     │                                                                 │
//!     │   Port 0x70 ─► Address Register (выбор регистра)               │
//!     │   Port 0x71 ─► Data Register (чтение/запись данных)            │
//!     │                                                                 │
//!     │   Регистры:                                                     │
//!     │     0x00 - Seconds      0x04 - Hours        0x08 - Month       │
//!     │     0x02 - Minutes      0x07 - Day          0x09 - Year        │
//!     │     0x0A - Status A     0x0B - Status B     0x32 - Century     │
//!     └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Формат данных
//!
//! RTC может хранить данные в BCD или binary формате (определяется битом 2 в Status B).
//! Часы могут быть в 12-часовом или 24-часовом формате (определяется битом 1 в Status B).
//!
//! Источники:
//! - ReactOS: hal/halx86/generic/cmos.c
//! - OSDev Wiki: CMOS

use super::portio::read_port_uchar;
use super::portio::write_port_uchar;

// =============================================================================
// CMOS Ports and Registers
// =============================================================================

/// Порт адреса CMOS (запись номера регистра)
const CMOS_ADDRESS_PORT: u16 = 0x70;
/// Порт данных CMOS (чтение/запись значения)
const CMOS_DATA_PORT: u16 = 0x71;

/// Регистр секунд
const RTC_SECONDS: u8 = 0x00;
/// Регистр минут
const RTC_MINUTES: u8 = 0x02;
/// Регистр часов
const RTC_HOURS: u8 = 0x04;
/// Регистр дня месяца
const RTC_DAY: u8 = 0x07;
/// Регистр месяца
const RTC_MONTH: u8 = 0x08;
/// Регистр года (2 цифры)
const RTC_YEAR: u8 = 0x09;
/// Регистр века (не стандартизирован, но обычно 0x32)
const RTC_CENTURY: u8 = 0x32;
/// Status Register A (бит 7 = update in progress)
const RTC_STATUS_A: u8 = 0x0A;
/// Status Register B (бит 1 = 24h mode, бит 2 = binary mode)
const RTC_STATUS_B: u8 = 0x0B;

/// Бит "update in progress" в Status A
const RTC_UIP: u8 = 0x80;
/// Бит "24-hour mode" в Status B
const RTC_24H: u8 = 0x02;
/// Бит "binary mode" в Status B
const RTC_BINARY: u8 = 0x04;

// =============================================================================
// TIME_FIELDS - структура времени NT
// =============================================================================

/// TIME_FIELDS - структура для представления времени в NT
///
/// Используется в RtlTimeToTimeFields / RtlTimeFieldsToTime
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct TIME_FIELDS {
    /// Год (1601-30827)
    pub year: u16,
    /// Месяц (1-12)
    pub month: u16,
    /// День месяца (1-31)
    pub day: u16,
    /// Час (0-23)
    pub hour: u16,
    /// Минута (0-59)
    pub minute: u16,
    /// Секунда (0-59)
    pub second: u16,
    /// Миллисекунда (0-999)
    pub milliseconds: u16,
    /// День недели (0=Sunday, 6=Saturday)
    pub weekday: u16,
}

// =============================================================================
// CMOS Read Functions
// =============================================================================

/// Читает байт из CMOS регистра
#[inline]
fn cmos_read(register: u8) -> u8 {
    // NMI disable bit (0x80) не трогаем — оставляем как есть
    write_port_uchar(CMOS_ADDRESS_PORT, register);
    read_port_uchar(CMOS_DATA_PORT)
}

/// Конвертирует BCD в binary
#[inline]
fn bcd_to_binary(bcd: u8) -> u8 {
    (bcd & 0x0F) + ((bcd >> 4) * 10)
}

/// Ждёт пока RTC не будет в процессе обновления
fn rtc_wait_not_updating() {
    // Ждём пока бит UIP не станет 0
    // Таймаут ~1 секунда (RTC обновляется раз в секунду)
    for _ in 0..1_000_000 {
        if (cmos_read(RTC_STATUS_A) & RTC_UIP) == 0 {
            return;
        }
        // Небольшая пауза
        core::hint::spin_loop();
    }
}

// =============================================================================
// HalQueryRealTimeClock
// =============================================================================

/// HalQueryRealTimeClock — читает текущее время из CMOS RTC
///
/// # Arguments
/// * `time_fields` - указатель на структуру TIME_FIELDS для заполнения
///
/// # Returns
/// `true` если чтение успешно
///
/// # Safety
/// `time_fields` должен быть валидным указателем
pub unsafe fn hal_query_real_time_clock(time_fields: *mut TIME_FIELDS) -> bool {
    unsafe {
        if time_fields.is_null() {
            return false;
        }

        // Ждём окончания обновления RTC
        rtc_wait_not_updating();

        // Читаем Status B для определения формата
        let status_b = cmos_read(RTC_STATUS_B);
        let is_binary = (status_b & RTC_BINARY) != 0;
        let is_24h = (status_b & RTC_24H) != 0;

        // Читаем все регистры времени
        let mut seconds = cmos_read(RTC_SECONDS);
        let mut minutes = cmos_read(RTC_MINUTES);
        let mut hours = cmos_read(RTC_HOURS);
        let mut day = cmos_read(RTC_DAY);
        let mut month = cmos_read(RTC_MONTH);
        let mut year = cmos_read(RTC_YEAR);
        let mut century = cmos_read(RTC_CENTURY);

        // Конвертируем из BCD если нужно
        if !is_binary {
            seconds = bcd_to_binary(seconds);
            minutes = bcd_to_binary(minutes);
            // Для часов в 12-часовом формате бит 7 = PM
            let pm = (hours & 0x80) != 0;
            hours = bcd_to_binary(hours & 0x7F);
            if !is_24h && pm {
                hours = (hours % 12) + 12;
            }
            day = bcd_to_binary(day);
            month = bcd_to_binary(month);
            year = bcd_to_binary(year);
            century = bcd_to_binary(century);
        } else if !is_24h {
            // Binary mode, но 12-часовой формат
            let pm = (hours & 0x80) != 0;
            hours &= 0x7F;
            if pm {
                hours = (hours % 12) + 12;
            }
        }

        // Century может быть 0 если регистр не поддерживается
        // В этом случае предполагаем 20xx для year < 80, иначе 19xx
        let full_year = if century > 0 {
            (century as u16) * 100 + (year as u16)
        } else if year < 80 {
            2000 + (year as u16)
        } else {
            1900 + (year as u16)
        };

        // Заполняем структуру
        (*time_fields).year = full_year;
        (*time_fields).month = month as u16;
        (*time_fields).day = day as u16;
        (*time_fields).hour = hours as u16;
        (*time_fields).minute = minutes as u16;
        (*time_fields).second = seconds as u16;
        (*time_fields).milliseconds = 0; // RTC не даёт миллисекунды
        (*time_fields).weekday = 0; // Вычислим позже если нужно

        true
    }
}

// =============================================================================
// Time Conversion
// =============================================================================

/// Количество дней в месяцах (не високосный год)
const DAYS_IN_MONTH: [u16; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

/// Проверяет является ли год високосным
#[inline]
fn is_leap_year(year: u16) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// Количество дней от 1 Jan 1601 до 1 Jan указанного года
fn days_from_1601_to_year(year: u16) -> u64 {
    if year < 1601 {
        return 0;
    }

    let y = (year - 1601) as u64;

    // Формула: дни = годы * 365 + високосные годы
    // Високосные: каждый 4-й, минус каждый 100-й, плюс каждый 400-й
    // Считаем от 1600 (последний год перед 1601)
    let leap_years = (y + 3) / 4 - (y + 99) / 100 + (y + 399) / 400;

    y * 365 + leap_years
}

/// Конвертирует TIME_FIELDS в NT time (100ns интервалы от 1 Jan 1601)
pub fn time_fields_to_time(tf: &TIME_FIELDS) -> i64 {
    if tf.year < 1601 || tf.month < 1 || tf.month > 12 || tf.day < 1 || tf.day > 31 {
        return 0;
    }

    // Дни от 1601 до начала года
    let mut days = days_from_1601_to_year(tf.year);

    // Добавляем дни месяцев
    for m in 0..(tf.month - 1) as usize {
        days += DAYS_IN_MONTH[m] as u64;
        // Февраль в високосный год
        if m == 1 && is_leap_year(tf.year) {
            days += 1;
        }
    }

    // Добавляем дни (день 1 = 0 дней)
    days += (tf.day - 1) as u64;

    // Конвертируем в 100ns интервалы
    // 1 день = 24 * 60 * 60 * 10_000_000 = 864_000_000_000 (100ns units)
    const TICKS_PER_SECOND: i64 = 10_000_000;
    const TICKS_PER_MINUTE: i64 = 60 * TICKS_PER_SECOND;
    const TICKS_PER_HOUR: i64 = 60 * TICKS_PER_MINUTE;
    const TICKS_PER_DAY: i64 = 24 * TICKS_PER_HOUR;

    let time = (days as i64) * TICKS_PER_DAY
        + (tf.hour as i64) * TICKS_PER_HOUR
        + (tf.minute as i64) * TICKS_PER_MINUTE
        + (tf.second as i64) * TICKS_PER_SECOND
        + (tf.milliseconds as i64) * 10_000;

    time
}

/// Форматирует TIME_FIELDS в строку "YYYY-MM-DD HH:MM:SS"
///
/// # Arguments
/// * `tf` - структура времени
/// * `buf` - буфер для записи (минимум 20 байт)
///
/// # Returns
/// Количество записанных байтов (19 для полного формата)
pub fn format_time_fields_to_buf(tf: &TIME_FIELDS, buf: &mut [u8]) -> usize {
    if buf.len() < 20 {
        return 0;
    }

    // Обнуляем буфер
    buf[..20].fill(0);

    // Форматируем вручную
    let mut pos = 0;

    // Year (4 digits)
    pos += format_u16_to_buf(buf, pos, tf.year, 4);
    buf[pos] = b'-';
    pos += 1;

    // Month (2 digits)
    pos += format_u16_to_buf(buf, pos, tf.month, 2);
    buf[pos] = b'-';
    pos += 1;

    // Day (2 digits)
    pos += format_u16_to_buf(buf, pos, tf.day, 2);
    buf[pos] = b' ';
    pos += 1;

    // Hour (2 digits)
    pos += format_u16_to_buf(buf, pos, tf.hour, 2);
    buf[pos] = b':';
    pos += 1;

    // Minute (2 digits)
    pos += format_u16_to_buf(buf, pos, tf.minute, 2);
    buf[pos] = b':';
    pos += 1;

    // Second (2 digits)
    pos += format_u16_to_buf(buf, pos, tf.second, 2);

    pos // 19 байт: "YYYY-MM-DD HH:MM:SS"
}

/// Форматирует TIME_FIELDS в строку "YYYY-MM-DD HH:MM:SS"
///
/// Удобная обёртка возвращающая &str из переданного буфера
#[inline]
pub fn format_time_fields<'a>(tf: &TIME_FIELDS, buf: &'a mut [u8]) -> &'a str {
    let len = format_time_fields_to_buf(tf, buf);
    // SAFETY: format_time_fields_to_buf записывает только ASCII цифры и разделители
    unsafe { core::str::from_utf8_unchecked(&buf[..len]) }
}

/// Форматирует u16 с ведущими нулями в буфер начиная с позиции offset
fn format_u16_to_buf(buf: &mut [u8], offset: usize, value: u16, width: usize) -> usize {
    let mut v = value;
    let mut digits = [0u8; 5]; // max 65535
    let mut count = 0;

    // Генерируем цифры справа налево
    loop {
        digits[count] = b'0' + (v % 10) as u8;
        v /= 10;
        count += 1;
        if v == 0 {
            break;
        }
    }

    // Добавляем ведущие нули
    let padding = if width > count { width - count } else { 0 };
    for i in 0..padding {
        buf[offset + i] = b'0';
    }

    // Копируем цифры в правильном порядке
    for i in 0..count {
        buf[offset + padding + i] = digits[count - 1 - i];
    }

    padding + count
}
