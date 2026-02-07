//! Создание гибридного BIOS/UEFI ISO образа

use anyhow::{Result, Context};
use std::process::Command;
use crate::util::{Paths, ensure_dir, run_cmd};

/// Создает гибридный BIOS/UEFI ISO образ
pub fn create(release: bool) -> Result<()> {
    log::info!("=== Creating hybrid ISO image ===");

    let paths = Paths::new()?;

    // Сначала собираем sysroot
    super::sysroot::build(release)?;

    let iso_dir = paths.build_dir.join("iso_root");
    let iso_img = paths.build_dir.join("xenos_dev.iso");

    ensure_dir(&iso_dir)?;

    // Копируем файлы в iso_root
    copy_iso_files(&paths, &iso_dir)?;

    // Создаем ISO с xorriso
    run_cmd(
        Command::new("xorriso")
            .args([
                "-as", "mkisofs",
                "-R", "-r", "-J",
                "-b", "boot/limine-bios-cd.bin",
                "-no-emul-boot",
                "-boot-load-size", "4",
                "-boot-info-table",
                "-hfsplus",
                "-apm-block-size", "2048",
                "--efi-boot", "boot/limine-uefi-cd.bin",
                "-efi-boot-part",
                "--efi-boot-image",
                "--protective-msdos-label",
            ])
            .arg(&iso_dir)
            .arg("-o")
            .arg(&iso_img)
    ).context("Failed to create ISO with xorriso")?;

    // Устанавливаем Limine BIOS на ISO
    let limine_install = paths.limine_dir.join("bin/limine");
    if limine_install.exists() {
        run_cmd(
            Command::new(&limine_install)
                .args(["bios-install"])
                .arg(&iso_img)
        ).context("Failed to install Limine BIOS on ISO")?;
    }

    log::info!("ISO image created: {}", iso_img.display());
    Ok(())
}

/// Копирует файлы в iso_root/
fn copy_iso_files(paths: &Paths, iso_dir: &std::path::Path) -> Result<()> {
    // EFI/BOOT/
    let efi_boot = iso_dir.join("EFI/BOOT");
    ensure_dir(&efi_boot)?;

    // boot/
    let boot_dir = iso_dir.join("boot");
    ensure_dir(&boot_dir)?;

    // XenOS/system32/config/
    let config_dir = iso_dir.join("XenOS/system32/config");
    ensure_dir(&config_dir)?;

    // Копируем из sysroot
    let copies = [
        (paths.sysroot_dir.join("EFI/BOOT/BOOTX64.EFI"), efi_boot.join("BOOTX64.EFI")),
        (paths.sysroot_dir.join("EFI/BOOT/winload.efi"), efi_boot.join("winload.efi")),
        (paths.sysroot_dir.join("boot/ntoskrnl.exe"), boot_dir.join("ntoskrnl.exe")),
        (paths.sysroot_dir.join("limine.conf"), iso_dir.join("limine.conf")),
        (paths.sysroot_dir.join("XenOS/system32/config/SYSTEM"), config_dir.join("SYSTEM")),
        (paths.sysroot_dir.join("XenOS/system32/unicode.nls"), iso_dir.join("XenOS/system32/unicode.nls")),
    ];

    for (src, dest) in copies {
        if src.exists() {
            if let Some(parent) = dest.parent() {
                ensure_dir(parent)?;
            }
            fs_err::copy(&src, &dest)?;
        }
    }

    // Копируем Limine файлы для ISO
    let limine_files = [
        ("bin/limine-bios.sys", "boot/limine-bios.sys"),
        ("bin/limine-bios-cd.bin", "boot/limine-bios-cd.bin"),
        ("bin/limine-uefi-cd.bin", "boot/limine-uefi-cd.bin"),
    ];

    for (src, dest) in limine_files {
        let src_path = paths.limine_dir.join(src);
        let dest_path = iso_dir.join(dest);
        if src_path.exists() {
            fs_err::copy(&src_path, &dest_path)?;
        }
    }

    Ok(())
}

