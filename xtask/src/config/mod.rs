//! Модуль конфигурации XenOS
//!
//! Реализует:
//! - Загрузку/сохранение .xenos-config
//! - Парсинг Kconfig.toml
//! - Menuconfig TUI
//! - Преобразование конфигурации в Cargo features

mod features;
mod loader;
mod tui;

pub use loader::Config;

use anyhow::Result;
use crate::util::Paths;

/// Запуск интерактивного menuconfig (TUI)
pub fn menuconfig() -> Result<()> {
    log::info!("Starting menuconfig...");

    let mut app = tui::MenuConfigApp::new()?;
    app.run()?;

    log::info!("Configuration complete");
    Ok(())
}

/// Создание конфигурации по умолчанию
pub fn defconfig() -> Result<()> {
    let paths = Paths::new()?;

    log::info!("Creating default configuration...");

    let config = Config::defaults();
    config.save(&paths.config_file)?;

    log::info!("Configuration saved to {}", paths.config_file.display());
    Ok(())
}

