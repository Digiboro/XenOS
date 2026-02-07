//! HAL - Hardware Abstraction Layer
//!
//! Абстракция аппаратного обеспечения для ядра NT.
//!
//! В оригинальной NT HAL - отдельный модуль (hal.dll).
//! В нашей реализации он встроен в ntoskrnl для простоты.
//!
//! Источники:
//! - ReactOS: hal/halx86/generic/, hal/halx86/apic/
//! - NT5: hal/halx86/

#![allow(dead_code)]
#![allow(non_camel_case_types)]

pub mod apic;
pub mod display;
pub mod hpet;
pub mod init;
pub mod interrupt;
pub mod ioapic;
pub mod irql;
pub mod pic;
pub mod pit;
pub mod portio;
pub mod processor;
pub mod rtc;
pub mod swint;
pub mod timer;

pub use apic::*;
pub use display::*;
pub use hpet::*;
pub use init::*;
pub use interrupt::*;
pub use ioapic::*;
pub use irql::*;
pub use pic::*;
pub use pit::*;
pub use portio::*;
pub use processor::*;
pub use rtc::*;
pub use swint::*;
pub use timer::*;
