//! Сборка загрузчика winload.efi

use anyhow::Result;
use std::process::Command;
use super::BuildContext;
use crate::util::{run_cmd, targets};

/// Собирает winload.efi
pub fn build(ctx: &BuildContext) -> Result<()> {
    log::info!("=== Building winload.efi ===");

    let mut cmd = Command::new("cargo");
    cmd.arg("build")
        .arg("-p").arg("winload")
        .arg("--target").arg(targets::UEFI)
        // Build std library from source for custom target
        .arg("-Zbuild-std=core,alloc")
        .arg("-Zbuild-std-features=compiler-builtins-mem");

    if let Some(flag) = ctx.cargo_release_flag() {
        cmd.arg(flag);
    }

    run_cmd(&mut cmd)?;

    log::info!("Winload built: {}", ctx.paths.winload_artifact(ctx.release).display());
    Ok(())
}

