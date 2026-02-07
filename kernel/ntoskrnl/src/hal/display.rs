//! HAL Display Functions
//!
//! HalDisplayString и связанные функции для вывода на экран.
//! Делегирует вызовы в INBV.

use crate::inbv;

/// HalDisplayString - вывод строки на экран
///
/// Стандартная HAL функция для вывода debug информации.
/// Используется при bugcheck и ранней диагностике.
///
/// # Arguments
/// * `string` - строка для вывода
pub fn hal_display_string(string: &str) {
    inbv::inbv_display_string(string);
}

/// HalDisplayStringXY - вывод строки в заданной позиции
///
/// # Arguments
/// * `string` - строка для вывода
/// * `x` - X координата в пикселях
/// * `y` - Y координата в пикселях
pub fn hal_display_string_xy(string: &str, x: u32, y: u32) {
    inbv::inbv_display_string_xy(string, x, y);
}

/// HalSetDisplayTextColor - установка цвета текста
///
/// # Arguments
/// * `color` - индекс цвета из палитры (0-15)
///
/// # Returns
/// Предыдущий цвет
pub fn hal_set_display_text_color(color: u8) -> u8 {
    inbv::inbv_set_text_color(color)
}

/// HalClearDisplay - очистка области экрана
///
/// # Arguments
/// * `left`, `top` - верхний левый угол
/// * `right`, `bottom` - нижний правый угол
/// * `color` - цвет заливки
pub fn hal_clear_display(left: u32, top: u32, right: u32, bottom: u32, color: u8) {
    inbv::inbv_solid_color_fill(left, top, right, bottom, color);
}
