//! Сборка Limine bootloader

use anyhow::{Result, Context};
use std::process::Command;
use super::BuildContext;
use crate::util::run_cmd;

/// LLVM toolchain paths (macOS homebrew)
const LLVM_PREFIX: &str = "/opt/homebrew/opt/llvm/bin";
const LLD_PATH: &str = "/opt/homebrew/bin/ld.lld";

/// Собирает Limine bootloader
pub fn build(ctx: &BuildContext) -> Result<()> {
    log::info!("=== Building Limine bootloader ===");

    let limine_dir = &ctx.paths.limine_dir;

    // Проверяем что Limine существует
    if !limine_dir.exists() {
        anyhow::bail!("Limine not found at {}. Run 'git submodule update --init'", limine_dir.display());
    }

    // Проверяем уже собран ли
    let bootx64 = limine_dir.join("bin/BOOTX64.EFI");
    if bootx64.exists() {
        log::info!("Limine already built, skipping (use 'cargo xtask clean --all' to rebuild)");
        return Ok(());
    }

    // Bootstrap
    log::info!("Running bootstrap...");
    run_cmd(
        Command::new("./bootstrap")
            .current_dir(limine_dir)
    ).context("Limine bootstrap failed")?;

    // Configure
    log::info!("Running configure...");
    run_cmd(
        Command::new("./configure")
            .args([
                "--enable-uefi-x86-64",
                "--enable-uefi-cd",
                "--enable-bios",
                "--enable-bios-cd",
            ])
            .env("CC_FOR_TARGET", "clang")
            .env("LD_FOR_TARGET", LLD_PATH)
            .env("OBJCOPY_FOR_TARGET", format!("{}/llvm-objcopy", LLVM_PREFIX))
            .env("OBJDUMP_FOR_TARGET", format!("{}/llvm-objdump", LLVM_PREFIX))
            .env("READELF_FOR_TARGET", format!("{}/llvm-readelf", LLVM_PREFIX))
            .current_dir(limine_dir)
    ).context("Limine configure failed")?;

    // Make
    log::info!("Running make...");
    run_cmd(
        Command::new("make")
            .current_dir(limine_dir)
    ).context("Limine make failed")?;

    log::info!("Limine built successfully");
    Ok(())
}

