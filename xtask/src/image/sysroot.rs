//! Сборка полного sysroot и копирование в образ диска

use anyhow::{Result, Context};
use std::process::Command;
use crate::util::{Paths, ensure_dir, run_cmd};
use crate::build;
use crate::build::manifest::ComponentManifest;
use crate::generate;
use crate::cli::{BuildArgs, BuildTarget, GenerateArgs, GenerateTarget};
use super::disk;

/// Директории с драйверами
const DRIVER_LOCATIONS: &[&str] = &[
    "boot/drivers",
    "kernel/drivers",
];

/// Директории ESP для создания
const ESP_DIRS: &[&str] = &[
    "::/EFI",
    "::/EFI/BOOT",
    "::/boot",
    "::/XenOS",
    "::/XenOS/system32",
    "::/XenOS/system32/config",
    "::/XenOS/system32/drivers",
];

/// Собирает полный sysroot и копирует в образ диска
pub fn build(release: bool) -> Result<()> {
    log::info!("=== Building sysroot ===");

    let paths = Paths::new()?;

    // 1. Собираем все компоненты
    log::info!("Building all components...");
    build::execute(BuildArgs {
        target: BuildTarget::All,
        release,
        verbose: false,
    })?;

    // 2. Генерируем артефакты
    log::info!("Generating artifacts...");
    generate::execute(GenerateArgs {
        target: GenerateTarget::All,
    })?;

    // 3. Создаем образ диска
    disk::create(release)?;

    // 4. Копируем файлы в локальный sysroot
    copy_to_local_sysroot(&paths, release)?;

    // 5. Копируем файлы в образ диска
    copy_to_disk_image(&paths, release)?;

    log::info!("=== Sysroot built successfully ===");
    Ok(())
}

/// Копирует файлы в локальный sysroot/
fn copy_to_local_sysroot(paths: &Paths, release: bool) -> Result<()> {
    log::info!("Copying files to local sysroot...");

    // EFI/BOOT/
    let efi_boot = paths.sysroot_dir.join("EFI/BOOT");
    ensure_dir(&efi_boot)?;

    // Limine BOOTX64.EFI
    let limine_efi = paths.limine_dir.join("bin/BOOTX64.EFI");
    if limine_efi.exists() {
        fs_err::copy(&limine_efi, efi_boot.join("BOOTX64.EFI"))?;
    }

    // winload.efi
    let winload = paths.winload_artifact(release);
    if winload.exists() {
        fs_err::copy(&winload, efi_boot.join("winload.efi"))?;
    }

    // boot/
    let boot_dir = paths.sysroot_dir.join("boot");
    ensure_dir(&boot_dir)?;

    // ntoskrnl.exe
    let kernel = paths.kernel_artifact(release);
    if kernel.exists() {
        fs_err::copy(&kernel, boot_dir.join("ntoskrnl.exe"))?;
    }

    // limine.conf
    let limine_conf_src = paths.workspace_root.join("config/limine/limine.conf");
    if limine_conf_src.exists() {
        fs_err::copy(&limine_conf_src, paths.sysroot_dir.join("limine.conf"))?;
    }

    Ok(())
}

/// Копирует файлы в образ диска (ESP)
fn copy_to_disk_image(paths: &Paths, _release: bool) -> Result<()> {
    log::info!("Copying files to disk image...");

    let disk_img = disk::disk_image_path(paths);
    let esp_offset = disk::esp_offset();

    let mcopy_target = format!("{}@@{}", disk_img.display(), esp_offset);

    // Создаем директории в образе
    for dir in ESP_DIRS {
        let _ = Command::new("mmd")
            .args(["-i", &mcopy_target, dir])
            .output();
    }

    // Копируем системные файлы
    let files = [
        (paths.sysroot_dir.join("EFI/BOOT/BOOTX64.EFI"), "::/EFI/BOOT"),
        (paths.sysroot_dir.join("EFI/BOOT/winload.efi"), "::/EFI/BOOT"),
        (paths.sysroot_dir.join("boot/ntoskrnl.exe"), "::/boot"),
        (paths.sysroot_dir.join("limine.conf"), "::"),
        (paths.sysroot_dir.join("XenOS/system32/config/SYSTEM"), "::/XenOS/system32/config"),
        (paths.sysroot_dir.join("XenOS/system32/unicode.nls"), "::/XenOS/system32"),
    ];

    for (src, dest) in files {
        if src.exists() {
            run_cmd(
                Command::new("mcopy")
                    .args(["-i", &mcopy_target, "-o"])
                    .arg(&src)
                    .arg(dest)
            ).with_context(|| format!("Failed to copy {} to image", src.display()))?;
        }
    }

    // Копируем драйверы согласно их манифестам
    copy_drivers_to_image(paths, &mcopy_target)?;

    log::info!("Files copied to disk image");
    Ok(())
}

/// Копирует драйверы в образ диска согласно их driver.toml манифестам
fn copy_drivers_to_image(paths: &Paths, mcopy_target: &str) -> Result<()> {
    for location in DRIVER_LOCATIONS {
        let drivers_dir = paths.workspace_root.join(location);
        if !drivers_dir.exists() {
            continue;
        }

        for entry in fs_err::read_dir(&drivers_dir)? {
            let entry = entry?;
            let driver_dir = entry.path();
            
            if !driver_dir.is_dir() {
                continue;
            }

            // Проверяем наличие манифеста
            if !ComponentManifest::exists(&driver_dir) {
                continue;
            }

            let manifest = match ComponentManifest::load(&driver_dir) {
                Ok(m) => m,
                Err(e) => {
                    log::warn!("Failed to load manifest for {}: {}", driver_dir.display(), e);
                    continue;
                }
            };
            
            // Пропускаем отключенные компоненты
            if !manifest.package.enabled {
                continue;
            }

            // Путь к файлу в sysroot
            let sysroot_path = paths.sysroot_dir.join(manifest.sysroot_path());
            
            if !sysroot_path.exists() {
                // Компонент не был собран (например, C драйвер без .sys)
                continue;
            }

            // Путь в образе (формат ::/path/to/dir)
            let image_dir = format!("::{}", manifest.destination_dir());
            
            run_cmd(
                Command::new("mcopy")
                    .args(["-i", mcopy_target, "-o"])
                    .arg(&sysroot_path)
                    .arg(&image_dir)
            ).with_context(|| format!("Failed to copy {} to image", sysroot_path.display()))?;
            
            log::info!("  Copied: {} -> {}", manifest.package.name, manifest.destination_dir());
        }
    }

    Ok(())
}
