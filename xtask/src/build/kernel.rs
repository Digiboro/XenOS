//! Сборка ядра ntoskrnl.exe

use anyhow::Result;
use std::process::Command;
use super::BuildContext;
use crate::util::{run_cmd, targets, ensure_dir};

/// Собирает ядро ntoskrnl.exe
pub fn build(ctx: &BuildContext) -> Result<()> {
    log::info!("=== Building kernel (ntoskrnl.exe) ===");

    // Генерируем import libraries для зависимостей ядра
    generate_import_libs(ctx)?;

    // Получаем features из конфигурации
    let features = ctx.config.kernel_features();

    if !features.is_empty() {
        log::info!("Features: {}", features.join(", "));
    }

    // Формируем команду cargo
    let mut cmd = Command::new("cargo");
    cmd.arg("build")
        .arg("-p").arg("ntoskrnl")
        .arg("--target").arg(targets::KERNEL)
        // Build std library from source for custom target
        .arg("-Zbuild-std=core,alloc")
        .arg("-Zbuild-std-features=compiler-builtins-mem");

    // Release флаг
    if let Some(flag) = ctx.cargo_release_flag() {
        cmd.arg(flag);
    }

    // Features из конфигурации
    if !features.is_empty() {
        cmd.arg("--features").arg(features.join(","));
    }

    // Запускаем сборку
    run_cmd(&mut cmd)?;

    // Копируем import library для драйверов
    copy_import_lib(ctx)?;

    log::info!("Kernel built: {}", ctx.paths.kernel_artifact(ctx.release).display());
    Ok(())
}

/// Копирует ntoskrnl.lib (import library) в build/ для драйверов
fn copy_import_lib(ctx: &BuildContext) -> Result<()> {
    ensure_dir(&ctx.paths.build_dir)?;

    let profile = ctx.profile();
    let target_dir = ctx.paths.target_dir
        .join("x86_64-xenos-pe")
        .join(profile);

    // Ищем .lib файл в build директории target
    let build_dir = target_dir.join("build");
    if build_dir.exists() {
        for entry in walkdir::WalkDir::new(&build_dir)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if path.extension().map(|e| e == "lib").unwrap_or(false)
                && path.file_name().map(|n| n.to_string_lossy().contains("ntoskrnl")).unwrap_or(false)
            {
                let dest = ctx.paths.build_dir.join("ntoskrnl.lib");
                fs_err::copy(path, &dest)?;
                log::info!("  -> {}", dest.display());
                return Ok(());
            }
        }
    }

    Ok(())
}

/// Генерирует import libraries для зависимостей ядра и драйверов
/// Эти библиотеки нужны для линковки с динамически загружаемыми компонентами.
fn generate_import_libs(ctx: &BuildContext) -> Result<()> {
    ensure_dir(&ctx.paths.build_dir)?;

    // Список .def файлов для генерации .lib
    let def_files = [
        ("boot/drivers/bootvid/bootvid.def", "bootvid.lib"),
        ("boot/drivers/storport/storport.def", "storport.lib"),
    ];

    for (def_path, lib_name) in def_files {
        let def_file = ctx.paths.workspace_root.join(def_path);
        let lib_file = ctx.paths.build_dir.join(lib_name);

        if def_file.exists() {
            generate_import_lib_from_def(&def_file, &lib_file)?;
        } else {
            log::warn!("{} not found, skipping {} generation", def_path, lib_name);
        }
    }

    Ok(())
}

/// Генерирует import library из .def файла используя llvm-dlltool
fn generate_import_lib_from_def(def_file: &std::path::Path, lib_file: &std::path::Path) -> Result<()> {
    // Определяем путь к llvm-dlltool
    let dlltool = if std::path::Path::new("/opt/homebrew/opt/llvm/bin/llvm-dlltool").exists() {
        "/opt/homebrew/opt/llvm/bin/llvm-dlltool"
    } else {
        "llvm-dlltool"
    };

    // Удаляем старый .lib если есть
    let _ = std::fs::remove_file(lib_file);

    // Генерируем import library
    let status = Command::new(dlltool)
        .args([
            "-d",
            &def_file.to_string_lossy(),
            "-l",
            &lib_file.to_string_lossy(),
            "-m",
            "i386:x86-64",
        ])
        .status()?;

    if status.success() {
        log::info!("Generated {} from {}", lib_file.display(), def_file.display());
    } else {
        anyhow::bail!("llvm-dlltool failed to generate {}", lib_file.display());
    }

    Ok(())
}

