//! KD - Kernel Debugger subsystem
//!
//! Подсистема отладочного вывода ядра.
//!
//! Архитектура:
//! - DbgPrint/DbgPrintEx - публичный API
//! - KdpPrint - внутренняя функция печати
//! - KD Providers - модульные провайдеры вывода (Serial, Screen, File)
//!
//! Источники:
//! - ReactOS: kd/kdio.c, kd64/kdprint.c, kd64/kddata.c
//! - NT5: kd64/

#![allow(dead_code)]

pub mod breakpoint;
pub mod data;
pub mod print;
pub mod screen;
pub mod serial;

pub use breakpoint::*;
pub use data::*;
pub use print::*;
pub use screen::*;
pub use serial::*;
