//! PnP ACPI Integration — ACPI Bus Enumerator
//!
//! Этот модуль реализует интеграцию PnP Manager с ACPI namespace.
//! Он работает как "ACPI bus enumerator", создавая PDO для устройств,
//! обнаруженных в ACPI namespace.
//!
//! # Архитектура
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                      ACPI Bus Enumerator                        │
//! ├─────────────────────────────────────────────────────────────────┤
//! │                                                                 │
//! │  AML Namespace                     PnP Device Tree              │
//! │  ─────────────                     ────────────────             │
//! │                                                                 │
//! │  \_SB (System Bus)  ─────────►  ACPI Root DevNode               │
//! │    │                                  │                         │
//! │    ├── PCI0 (_HID=PNP0A03)   ───►  PDO: ACPI\PNP0A03            │
//! │    │     └── VGA (_ADR=0x20000)     │                           │
//! │    │                                ▼                           │
//! │    ├── LNKA (_HID=PNP0C0F)   ───►  PDO: ACPI\PNP0C0F            │
//! │    │                                                            │
//! │    └── HPET (_HID=PNP0103)   ───►  PDO: ACPI\PNP0103            │
//! │                                                                 │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Поддерживаемые ACPI методы
//!
//! - `_HID` — Hardware ID (ACPI\XXXXXXXX или ACPI\PNPXXXX)
//! - `_CID` — Compatible IDs
//! - `_UID` — Unique ID (для различения одинаковых устройств)
//! - `_ADR` — Address (для PCI-style адресации)
//! - `_STA` — Device Status
//! - `_INI` — Device Initialization
//!
//! # Важные HID
//!
//! - PNP0A03: PCI Host Bridge
//! - PNP0A08: PCI Express Host Bridge
//! - PNP0C01: System Board
//! - PNP0C02: PnP Motherboard Resources
//! - PNP0C0F: PCI Interrupt Link Device
//! - PNP0103: HPET
//!
//! Источники:
//! - ReactOS: drivers/bus/acpi/acpica/
//! - Windows: acpi.sys
//! - ACPI Spec 6.4+

#![allow(dead_code)]

pub mod device;
pub mod driver;
pub mod enumerate;
pub mod ids;
pub mod irp;

pub use device::*;
pub use driver::*;
pub use enumerate::*;
pub use ids::*;
pub use irp::*;

