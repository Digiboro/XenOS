//! I/O Manager - Подсистема ввода-вывода
//!
//! I/O Manager управляет драйверами, устройствами, файлами и IRP.
//!
//! Источники:
//! - ReactOS: ntoskrnl/io/iomgr/
//! - NT5: io/iomgr/
//!
//! Основные компоненты:
//! - DEVICE_OBJECT - объект устройства
//! - DRIVER_OBJECT - объект драйвера
//! - FILE_OBJECT - объект файла
//! - IRP - I/O Request Packet

#![allow(dead_code)]
#![allow(non_camel_case_types)]

pub mod device;
pub mod driver;
pub mod file;
pub mod init;
pub mod interface;
pub mod interrupt;
pub mod iop;
pub mod irp;
pub mod pnp;
pub mod pnpmgr;
pub mod resources;
pub mod types;
pub mod vpb;

pub use device::*;
pub use driver::*;
pub use file::*;
pub use init::*;
pub use interface::*;
pub use interrupt::*;
pub use iop::*;
pub use irp::*;
pub use pnp::*;
pub use pnpmgr::*;
pub use resources::*;
pub use types::*;
