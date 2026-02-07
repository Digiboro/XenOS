//! XenOS Win32 Test Application - Hello World
//!
//! Минимальное тестовое приложение для проверки работы user space в XenOS.
//! Использует стандартную библиотеку Rust и WinAPI.

use windows::Win32::System::SystemInformation::{
    GetVersionExW, OSVERSIONINFOW,
};

fn main() {
    println!("=====================================");
    println!("  Hello from XenOS User Space!");
    println!("=====================================");
    println!();
    
    // Получаем версию ОС через WinAPI
    let mut version_info = OSVERSIONINFOW {
        dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32,
        ..Default::default()
    };
    
    let result = unsafe { GetVersionExW(&mut version_info) };
    
    if result.is_ok() {
        println!("OS Version: {}.{}.{}",
            version_info.dwMajorVersion,
            version_info.dwMinorVersion,
            version_info.dwBuildNumber
        );
        println!("Platform ID: {}", version_info.dwPlatformId);
    } else {
        println!("Failed to get OS version");
    }
    
    println!();
    println!("Expected: XenOS (NT 6.1 compatible)");
    println!();
    println!("If you see this message, user mode works!");
    println!();
}
