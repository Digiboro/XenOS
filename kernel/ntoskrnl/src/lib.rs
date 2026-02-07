//! XenOS NT-совместимое ядро

#![no_std]
#![feature(abi_x86_interrupt)]
#![feature(allocator_api)]

extern crate alloc;

// PE-импорты из внешних модулей (bootvid.dll, hal.dll, ...)
pub mod imports;

// Базовые типы NT
pub mod nt;

// Архитектурно-зависимый код
pub mod arch;

// Подсистемы ядра
pub mod ex; // Executive - pool, work items, handles
pub mod hal; // HAL - Hardware Abstraction Layer
pub mod ke; // Kernel - базовые функции, синхронизация
pub mod mm; // Memory Manager
pub mod ob; // Object Manager
pub mod ps; // Process Manager

// Подсистемы ядра (продолжение)
pub mod acpi; // ACPI - Advanced Configuration and Power Interface
pub mod bootlog; // Boot-time screen logging (/SOS)
pub mod inbv; // In-Boot Video - экранный вывод
pub mod io; // I/O Manager
pub mod kd; // Kernel Debugger - DbgPrint, KdPrint
pub mod pe_stubs; // PE/COFF runtime stubs (__chkstk, _fltused)

// Kernel Test Framework (только с feature test-kernel)
#[cfg(feature = "test-kernel")]
pub mod test;

// Подсистемы (будут добавлены по мере реализации)
pub mod se; // Security Reference Monitor
pub mod rtl; // Runtime Library (минимальный слой)
