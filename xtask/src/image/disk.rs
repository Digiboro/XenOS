//! Создание GPT образа диска с ESP разделом

use anyhow::{Result, Context};
use std::process::Command;
use crate::util::{Paths, ensure_dir, run_cmd};

const DISK_SIZE_MB: u32 = 128;
const ESP_OFFSET: u64 = 1048576; // 1MB offset, sector 2048

/// Создает GPT образ диска с ESP разделом
pub fn create(_release: bool) -> Result<()> {
    log::info!("=== Creating GPT disk image ({} MB) ===", DISK_SIZE_MB);

    let paths = Paths::new()?;
    ensure_dir(&paths.build_dir)?;

    let disk_img = paths.build_dir.join("disk.img");

    // Удаляем старый образ
    if disk_img.exists() {
        fs_err::remove_file(&disk_img)?;
    }

    // Создаем пустой файл нужного размера
    run_cmd(
        Command::new("dd")
            .args([
                "if=/dev/zero",
                &format!("of={}", disk_img.display()),
                "bs=1M",
                &format!("count={}", DISK_SIZE_MB),
            ])
            .stderr(std::process::Stdio::null())
    ).context("Failed to create disk image")?;

    // Создаем GPT таблицу с ESP разделом
    run_cmd(
        Command::new("sgdisk")
            .args([
                "--clear",
                "--new=1:2048:0",
                "--typecode=1:EF00",
                "--change-name=1:ESP",
            ])
            .arg(&disk_img)
            .stdout(std::process::Stdio::null())
    ).context("Failed to create GPT partition table")?;

    // Форматируем ESP как FAT32
    run_cmd(
        Command::new("mformat")
            .args([
                "-i",
                &format!("{}@@{}", disk_img.display(), ESP_OFFSET),
                "-F",
                "-v", "XENOS",
                "::",
            ])
    ).context("Failed to format ESP partition")?;

    log::info!("GPT disk image created: {}", disk_img.display());
    Ok(())
}

/// Путь к образу диска
pub fn disk_image_path(paths: &Paths) -> std::path::PathBuf {
    paths.build_dir.join("disk.img")
}

/// Offset ESP раздела
pub fn esp_offset() -> u64 {
    ESP_OFFSET
}

