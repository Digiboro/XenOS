//! Модуль сборки компонентов XenOS
//!
//! Сборка управляется через `component.toml` манифесты.
//! Компоненты без манифеста игнорируются.
//!
//! Реализует команды:
//! - `cargo xtask build kernel`
//! - `cargo xtask build winload`
//! - `cargo xtask build drivers`
//! - `cargo xtask build limine`
//! - `cargo xtask build all`

mod kernel;
mod winload;
mod limine;
pub mod manifest;
pub mod components;

use anyhow::Result;
use crate::cli::{BuildArgs, BuildTarget, CleanArgs};
use crate::config::Config;
use crate::util::Paths;

/// Контекст сборки с загруженной конфигурацией
pub struct BuildContext {
    pub config: Config,
    pub paths: Paths,
    pub release: bool,
    pub verbose: bool,
}

impl BuildContext {
    /// Создает контекст сборки, загружая конфигурацию
    pub fn new(release: bool, verbose: bool) -> Result<Self> {
        let paths = Paths::new()?;

        // Загружаем конфигурацию (или defaults если файла нет)
        let config = if paths.config_file.exists() {
            Config::load(&paths.config_file)?
        } else {
            log::warn!("No .xenos-config found, using defaults");
            log::warn!("Run 'cargo xtask defconfig' to create config");
            Config::defaults()
        };

        Ok(Self {
            config,
            paths,
            release,
            verbose,
        })
    }

    /// Профиль сборки (debug/release)
    pub fn profile(&self) -> &str {
        if self.release {
            "release"
        } else {
            "debug"
        }
    }

    /// Флаг --release для cargo
    pub fn cargo_release_flag(&self) -> Option<&str> {
        if self.release {
            Some("--release")
        } else {
            None
        }
    }
}

/// Выполнение команды build
pub fn execute(args: BuildArgs) -> Result<()> {
    let ctx = BuildContext::new(args.release, args.verbose)?;

    match args.target {
        BuildTarget::Kernel => kernel::build(&ctx),
        BuildTarget::Winload => winload::build(&ctx),
        BuildTarget::Drivers => components::build_drivers(&ctx),
        BuildTarget::Driver { name } => components::build_one(&ctx, &name),
        BuildTarget::Limine => limine::build(&ctx),
        BuildTarget::All => {
            log::info!("=== Building all components ===");
            kernel::build(&ctx)?;
            winload::build(&ctx)?;
            components::build_drivers(&ctx)?;
            log::info!("=== All components built successfully ===");
            Ok(())
        }
    }
}

/// Очистка артефактов
pub fn clean(args: CleanArgs) -> Result<()> {
    let paths = Paths::new()?;

    log::info!("Cleaning cargo artifacts...");
    crate::util::run_cmd(
        std::process::Command::new("cargo")
            .arg("clean")
    )?;

    // build/ и sysroot/ очищаются всегда
        log::info!("Cleaning build/, sysroot/...");

        if paths.build_dir.exists() {
            fs_err::remove_dir_all(&paths.build_dir)?;
        }

        if paths.sysroot_dir.exists() {
            fs_err::remove_dir_all(&paths.sysroot_dir)?;
        }

    // Очистка Limine только с --all (требует перекомпиляции)
    if args.all {
        if paths.limine_dir.exists() {
            log::info!("Cleaning Limine...");
            crate::util::run_cmd(
                std::process::Command::new("git")
                    .args(["clean", "-fdx"])
                    .current_dir(&paths.limine_dir)
            )?;
        }
    }

    log::info!("Clean complete");
    Ok(())
}

