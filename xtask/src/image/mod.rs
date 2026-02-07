//! Модуль создания образов
//!
//! Реализует:
//! - `cargo xtask image disk` - создание GPT образа с ESP
//! - `cargo xtask image sysroot` - полная сборка sysroot
//! - `cargo xtask image iso` - создание гибридного ISO

mod disk;
mod sysroot;
mod iso;

use anyhow::Result;
use crate::cli::{ImageArgs, ImageTarget};

/// Выполнение команды image
pub fn execute(args: ImageArgs) -> Result<()> {
    match args.target {
        ImageTarget::Disk => disk::create(args.release),
        ImageTarget::Sysroot => sysroot::build(args.release),
        ImageTarget::Iso => iso::create(args.release),
    }
}

