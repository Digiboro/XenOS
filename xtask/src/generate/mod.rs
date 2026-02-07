//! Модуль генерации артефактов
//!
//! Реализует:
//! - `cargo xtask generate hive` - генерация SYSTEM registry hive
//! - `cargo xtask generate nls` - генерация unicode.nls
//! - `cargo xtask generate all`

mod hive;
mod nls;

use anyhow::Result;
use crate::cli::{GenerateArgs, GenerateTarget};

/// Выполнение команды generate
pub fn execute(args: GenerateArgs) -> Result<()> {
    match args.target {
        GenerateTarget::Hive => hive::generate(),
        GenerateTarget::Nls => nls::generate(),
        GenerateTarget::All => {
            log::info!("=== Generating all artifacts ===");
            hive::generate()?;
            nls::generate()?;
            log::info!("=== All artifacts generated ===");
            Ok(())
        }
    }
}

