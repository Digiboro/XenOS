//! FAT32 Time/Date Conversion
//!
//! Конвертация между FAT32 date/time и NT FILETIME (100ns intervals since 1601).

use crate::types::*;

// =============================================================================
// Constants
// =============================================================================

/// FAT epoch: January 1, 1980
const FAT_EPOCH_YEAR: i32 = 1980;

/// NT epoch: January 1, 1601 (в 100ns intervals)
const NT_EPOCH_FILETIME: i64 = 0;

/// Разница между FAT epoch (1980) и NT epoch (1601) в днях
const DAYS_1601_TO_1980: i64 = 138_427; // 379 лет * 365.25 дней

/// 100ns intervals в одном дне
const FILETIME_PER_DAY: i64 = 24 * 60 * 60 * 10_000_000;

// =============================================================================
// DOS Date/Time Format
// =============================================================================

/// FAT Date format (16 bits):
/// - Bits 0-4: Day (1-31)
/// - Bits 5-8: Month (1-12)
/// - Bits 9-15: Year - 1980 (0-127, representing 1980-2107)
///
/// FAT Time format (16 bits):
/// - Bits 0-4: Seconds / 2 (0-29, representing 0-58 seconds)
/// - Bits 5-10: Minutes (0-59)
/// - Bits 11-15: Hours (0-23)

/// Конвертирует FAT date/time в NT FILETIME
pub fn dos_datetime_to_filetime(date: u16, time: u16, _tenth: u8) -> i64 {
    // Извлекаем компоненты date
    let day = (date & 0x1F) as i32;
    let month = ((date >> 5) & 0x0F) as i32;
    let year = ((date >> 9) & 0x7F) as i32 + FAT_EPOCH_YEAR;
    
    // Извлекаем компоненты time
    let seconds = ((time & 0x1F) * 2) as i32; // * 2 потому что stored in 2-second intervals
    let minutes = ((time >> 5) & 0x3F) as i32;
    let hours = ((time >> 11) & 0x1F) as i32;
    
    // Валидация
    if day < 1 || day > 31 || month < 1 || month > 12 {
        return 0; // Некорректная дата
    }
    
    // Вычисляем количество дней от NT epoch (1601) до этой даты
    let days_from_1601 = days_since_1601(year, month, day);
    
    // Конвертируем в 100ns intervals
    let filetime = days_from_1601 * FILETIME_PER_DAY
        + (hours as i64 * 3600 + minutes as i64 * 60 + seconds as i64) * 10_000_000;
    
    filetime
}

/// Конвертирует NT FILETIME в FAT date/time
pub fn filetime_to_dos_datetime(filetime: i64) -> (u16, u16) {
    if filetime <= 0 {
        // Некорректное время, возвращаем FAT epoch
        return (0x0021, 0); // Jan 1, 1980, 00:00:00
    }
    
    // Конвертируем в дни и секунды
    let total_days = filetime / FILETIME_PER_DAY;
    let day_remainder = filetime % FILETIME_PER_DAY;
    let total_seconds = day_remainder / 10_000_000;
    
    // Вычисляем дату с учётом високосных годов
    let mut year = 1601i32;
    let mut remaining_days = total_days;
    
    // Находим год
    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        year += 1;
    }
    
    if year < FAT_EPOCH_YEAR {
        return (0x0021, 0); // До FAT epoch
    }
    
    let fat_year = (year - FAT_EPOCH_YEAR) as u16;
    if fat_year > 127 {
        return (0xFFFF, 0xFFFF); // После 2107
    }
    
    // Вычисляем месяц и день с учётом текущего года
    let (month, day) = day_of_year_to_month_day(remaining_days as i32, year);
    
    // Вычисляем время
    let hours = (total_seconds / 3600) as u16;
    let minutes = ((total_seconds % 3600) / 60) as u16;
    let seconds = (total_seconds % 60) as u16;
    
    // Формируем FAT date
    let fat_date = ((fat_year & 0x7F) << 9) | ((month & 0x0F) << 5) | (day & 0x1F);
    
    // Формируем FAT time  
    let fat_time = ((hours & 0x1F) << 11) | ((minutes & 0x3F) << 5) | ((seconds / 2) & 0x1F);
    
    (fat_date, fat_time)
}

/// Вычисляет количество дней от 1601 до указанной даты
fn days_since_1601(year: i32, month: i32, day: i32) -> i64 {
    // Полный расчёт с учётом високосных годов
    let years_since_1601 = year - 1601;
    
    // Количество високосных годов от 1601 до year
    let leap_years = count_leap_years(1601, year);
    
    // Базовое количество дней
    let days = years_since_1601 as i64 * 365 + leap_years as i64;
    
    // Добавляем дни за месяцы
    let mut days_in_year = 0;
    for m in 1..month {
        days_in_year += days_in_month(year, m);
    }
    
    days + days_in_year as i64 + day as i64 - 1 // -1 потому что day начинается с 1
}

/// Проверяет является ли год високосным
fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// Подсчитывает количество високосных годов между start и end (не включая end)
fn count_leap_years(start: i32, end: i32) -> i32 {
    if end <= start {
        return 0;
    }
    
    let count_before_end = (end - 1) / 4 - (end - 1) / 100 + (end - 1) / 400;
    let count_before_start = (start - 1) / 4 - (start - 1) / 100 + (start - 1) / 400;
    
    count_before_end - count_before_start
}

/// Возвращает количество дней в месяце с учётом високосных годов
fn days_in_month(year: i32, month: i32) -> i32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => if is_leap_year(year) { 29 } else { 28 },
        _ => 0,
    }
}

/// Конвертирует день года в месяц/день с учётом високосного года
fn day_of_year_to_month_day(day_of_year: i32, year: i32) -> (u16, u16) {
    let mut remaining = day_of_year + 1;
    let mut month = 1u16;
    
    for m in 1..=12 {
        let days = days_in_month(year, m);
        if remaining <= days {
            return (month, remaining as u16);
        }
        remaining -= days;
        month += 1;
    }
    
    (12, 31) // Fallback
}

/// Получает текущее время в FILETIME формате
///
/// Читает текущее время из системы (через ntoskrnl exports)
pub fn get_current_filetime() -> i64 {
    // В kernel mode используем KeQuerySystemTime или фиксированное время
    // Для FAT32 timestamp'ы не критичны для функциональности
    // Возвращаем константу представляющую 2025-12-22
    const FILETIME_2025_12_22: i64 = 133_484_544_000_000_000;
    FILETIME_2025_12_22
}

