//! NTDLL - User-mode системная библиотека
//!
//! Предоставляет stubs для системных вызовов XenOS.
//! Каждый stub выполняет переход в kernel mode через инструкцию SYSCALL.
//!
//! # Архитектура
//!
//! ```text
//! User Mode (ntdll.dll)              Kernel Mode (ntoskrnl.exe)
//!
//! NtCreateFile:                      KiSystemCall64:
//!   mov r10, rcx    ─┐                 swapgs
//!   mov eax, 0x00    │                 ... build KTRAP_FRAME ...
//!   syscall ─────────┴──────────────→  call SERVICE_TABLE[eax]
//!   ret              ←──────────────── sysretq
//!                                       │
//!                                       └→ NtCreateFile (kernel)
//! ```
//!
//! # Соглашение о вызовах
//!
//! - Аргументы 1-4: RCX, RDX, R8, R9 (стандартный Win64 ABI)
//! - RCX копируется в R10 перед SYSCALL (т.к. SYSCALL затирает RCX адресом возврата)
//! - Номер сервиса (service_id) в EAX
//! - Результат (NTSTATUS) возвращается в RAX
//!
//! # Источники
//!
//! - MSDN (NT6.1): System Call Dispatching
//! - ReactOS: dll/ntdll/include/ntdll.h

#![no_std]

use core::arch::naked_asm;
use core::panic::PanicInfo;

// =============================================================================
// DLL Entry Point
// =============================================================================

/// DLL Entry Point (required for PE DLL)
///
/// Вызывается загрузчиком при загрузке/выгрузке DLL.
/// Для ntdll минимальная реализация - просто возвращаем TRUE.
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn _DllMainCRTStartup(
    _hinstdll: *mut core::ffi::c_void,
    _fdwreason: u32,
    _lpvreserved: *mut core::ffi::c_void,
) -> i32 {
    1 // TRUE
}

// =============================================================================
// Panic handler (required for no_std cdylib)
// =============================================================================

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

// =============================================================================
// Типы NT
// =============================================================================

/// NTSTATUS - код возврата NT функций
pub type NTSTATUS = i32;

// =============================================================================
// Макрос генерации syscall stubs
// =============================================================================

/// Генерирует syscall stub для Nt* функции
///
/// # Параметры
///
/// - `$name` - имя функции (например, NtCreateFile)
/// - `$id` - номер сервиса (service_id)
/// - `$argc` - количество аргументов (для документации)
///
/// # Генерируемый код
///
/// ```asm
/// $name:
///     mov r10, rcx    ; сохранить 1-й аргумент (SYSCALL затирает RCX)
///     mov eax, $id    ; номер сервиса
///     syscall         ; переход в kernel mode
///     ret             ; возврат в caller
/// ```
macro_rules! syscall_stub {
    ($name:ident, $id:expr, $argc:expr) => {
        /// Системный вызов (stub)
        ///
        /// Аргументов: $argc
        #[unsafe(naked)]
        #[unsafe(no_mangle)]
        pub unsafe extern "win64" fn $name() -> NTSTATUS {
            naked_asm!(
                "mov r10, rcx",      // сохранить 1-й аргумент
                "mov eax, {id}",     // номер сервиса
                "syscall",           // переход в kernel
                "ret",               // возврат
                id = const $id,
            )
        }
    };
}

// =============================================================================
// Syscall stubs из сгенерированного файла
// =============================================================================

/// Макрос syscall_list! для генерации stubs из include файла
///
/// Формат: (NtName, syscall_id, argc)
macro_rules! syscall_list {
    ( $( ($name:ident, $id:expr, $argc:expr) )* ) => {
        $( syscall_stub!($name, $id, $argc); )*
    };
}

// Сгенерированный файл из ntapi.toml
include!(concat!(env!("OUT_DIR"), "/syscalls_ntdll.inc.rs"));
