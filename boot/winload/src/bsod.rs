//! BSOD-подобный экран ошибки
//!
//! Показывает экран ошибки в стиле Windows NT при критических сбоях загрузки.

use core::time::Duration;

use uefi::boot;

use crate::console::print;
use crate::console::print_hex;
use crate::console::println;

/// Показывает BSOD-подобный экран ошибки и останавливает загрузку.
pub fn show_bsod(stop_code: u32, message: &str, file_path: &str) -> ! {
    // Очищаем экран (синий фон эмулируем через текст)
    println("");
    println("****************************************************************");
    println("*                                                              *");
    println("*  A problem has been detected and XenOS loader has stopped.  *");
    println("*                                                              *");
    println("****************************************************************");
    println("");
    print("*** STOP: 0x");
    print_hex(stop_code as u64);
    println("");
    println("");
    print("  ");
    println(message);
    println("");
    print("  File: ");
    println(file_path);
    println("");
    println("If this is the first time you've seen this error screen,");
    println("verify that the boot image contains all required files.");
    println("");
    println("Technical information:");
    print("  NTSTATUS: 0x");
    print_hex(stop_code as u64);
    println("");

    // Бесконечный цикл
    loop {
        boot::stall(Duration::from_secs(1));
    }
}
