//! Модуль запуска в QEMU
//!
//! Реализует:
//! - `cargo xtask run` - запуск в QEMU
//! - `cargo xtask run --gdb` - режим отладки с GDB
//! - `cargo xtask test` - запуск тестов

mod qemu;

use anyhow::Result;
use crate::cli::{RunArgs, TestArgs};
use crate::config::Config;
use crate::util::Paths;
use crate::image;
use crate::cli::{ImageArgs, ImageTarget};

/// Выполнение команды run
pub fn execute(args: RunArgs) -> Result<()> {
    log::info!("=== Starting XenOS in QEMU ===");

    let paths = Paths::new()?;

    // Загружаем конфигурацию
    let config = if paths.config_file.exists() {
        Config::load(&paths.config_file)?
    } else {
        Config::defaults()
    };

    // ПРИНУДИТЕЛЬНО пересобираем ядро (для отладки таймеров)
    log::info!("Force rebuilding kernel...");
    std::process::Command::new("cargo")
        .arg("clean")
        .arg("-p")
        .arg("ntoskrnl")
        .status()?;

    // Всегда пересобираем sysroot перед запуском
    // (cargo сам сделает инкрементальную сборку если ничего не изменилось)
        log::info!("Building sysroot...");
        image::execute(ImageArgs {
            target: ImageTarget::Sysroot,
            release: args.release,
        })?;

    // Запускаем QEMU
    if args.gdb {
        qemu::run_gdb(&paths, &config, &args)
    } else {
        qemu::run(&paths, &config, &args)
    }
}

/// Выполнение команды test
pub fn execute_test(args: TestArgs) -> Result<()> {
    log::info!("=== Running kernel tests in QEMU ===");

    let paths = Paths::new()?;

    // Создаем конфигурацию для тестов
    let mut config = if paths.config_file.exists() {
        Config::load(&paths.config_file)?
    } else {
        Config::defaults()
    };

    // Включаем test-kernel feature
    config.TEST_KERNEL = true;

    // Собираем sysroot с test features
    image::execute(ImageArgs {
        target: ImageTarget::Sysroot,
        release: false,
    })?;

    // Запускаем QEMU в тестовом режиме
    qemu::run_test(&paths, &config, &args)
}

