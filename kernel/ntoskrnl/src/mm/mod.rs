//! Memory Manager (Mm)
//!
//! Управление памятью NT ядра.
//!
//! # Модули
//!
//! - `types` — базовые типы и константы
//! - `pte` — Page Table Entries, self-map
//! - `pfn` — PFN database, физические страницы
//! - `init` — инициализация MM
//! - `syspte` — System PTE allocator
//! - `hyperspace` — временные маппинги
//! - `pool` — Mm-уровень pool
//! - `virtual_mem` — виртуальная память, VAD
//! - `mdl` — Memory Descriptor Lists
//! - `mmio` — Memory-mapped I/O
//! - `hal_mmio` — HAL MMIO (APIC, IOAPIC)
//!
//! Источники:
//! - ReactOS: mm/ARM3/mminit.c, mm/ARM3/pfnlist.c, mm/ARM3/virtual.c
//! - NT5: mm/mminit.c, mm/pfnlist.c

#![allow(dead_code)]
#![allow(non_camel_case_types)]

pub mod hal_mmio;
pub mod hyperspace;
pub mod init;
pub mod mdl;
pub mod mmio;
pub mod mpw;
pub mod pagefault;
pub mod pagefile;
pub mod pfn;
pub mod pool;
pub mod pte;
pub mod section;
pub mod syspte;
pub mod types;
pub mod va_allocator;
pub mod vad;
pub mod virt_to_phys;
pub mod virtual_mem;
pub mod workingset;
pub mod zero;

pub use hal_mmio::*;
pub use hyperspace::*;
pub use init::*;
pub use mdl::*;
pub use mmio::*;
pub use mpw::*;
pub use pagefault::*;
pub use pagefile::*;
pub use pfn::*;
pub use pool::*;
pub use pte::*;
pub use section::*;
pub use syspte::*;
pub use types::*;
pub use va_allocator::*;
pub use vad::*;
pub use virt_to_phys::*;
pub use virtual_mem::*;
pub use workingset::*;
pub use zero::*;
