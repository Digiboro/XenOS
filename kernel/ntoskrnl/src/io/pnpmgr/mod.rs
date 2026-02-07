//! PnP Manager — Plug and Play менеджер
//!
//! # Архитектура NT6.1 PnP
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                         PnP Manager                                  │
//! ├─────────────────────────────────────────────────────────────────────┤
//! │                                                                     │
//! │  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐              │
//! │  │ Root DevNode│───►│ ACPI DevNode│───►│ PCI DevNode │──► ...       │
//! │  │   (HTREE\   │    │   (ACPI\    │    │   (PCI\     │              │
//! │  │    ROOT\0)  │    │    _SB...)  │    │   VEN_xxx)  │              │
//! │  └─────────────┘    └─────────────┘    └─────────────┘              │
//! │         │                  │                  │                     │
//! │         ▼                  ▼                  ▼                     │
//! │  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐              │
//! │  │  Root PDO   │    │  ACPI PDO   │    │   PCI PDO   │              │
//! │  │(PnpRoot bus)│    │  (bus drv)  │    │  (function) │              │
//! │  └──────┬──────┘    └──────┬──────┘    └──────┬──────┘              │
//! │         │                  │                  │                     │
//! │    AddDevice          AddDevice          AddDevice                  │
//! │         │                  │                  │                     │
//! │         ▼                  ▼                  ▼                     │
//! │  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐              │
//! │  │  Root FDO   │    │  ACPI FDO   │    │   Func FDO  │              │
//! │  │  (bus drv)  │    │  (acpi.sys) │    │ (driver.sys)│              │
//! │  └─────────────┘    └─────────────┘    └─────────────┘              │
//! │                                                                     │
//! └─────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Модули
//!
//! - `devnode` — DEVICE_NODE структура и операции
//! - `state` — PNP_DEVNODE_STATE enum и state machine
//! - `devaction` — очередь действий PnP (enumerate, start, stop, remove)
//! - `init` — инициализация PnP Manager
//!
//! Источники:
//! - ReactOS: ntoskrnl/io/pnpmgr/
//! - Windows 7: ntos/io/pnpmgr/

#![allow(dead_code)]

pub mod arbiter;
pub mod devaction;
pub mod devnode;
pub mod drvload;
pub mod init;
pub mod pnpacpi;
pub mod state;

pub use arbiter::*;
pub use devaction::*;
pub use devnode::*;
pub use drvload::*;
pub use init::*;
pub use pnpacpi::*;
pub use state::*;

