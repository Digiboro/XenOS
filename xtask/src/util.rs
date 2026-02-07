//! Утилиты для xtask
//!
//! Пути, константы, вспомогательные функции.

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Пути проекта
pub struct Paths {
    /// Корень workspace
    pub workspace_root: PathBuf,
    /// Директория target/
    pub target_dir: PathBuf,
    /// Директория build/ (артефакты сборки)
    pub build_dir: PathBuf,
    /// Директория sysroot/
    pub sysroot_dir: PathBuf,
    /// Файл конфигурации .xenos-config
    pub config_file: PathBuf,
    /// Файл определений Kconfig.toml
    pub kconfig_file: PathBuf,
    /// Директория с OVMF файлами
    pub ovmf_dir: PathBuf,
    /// Директория vendor/limine
    pub limine_dir: PathBuf,
}

impl Paths {
    /// Создает структуру путей, находя корень workspace
    pub fn new() -> Result<Self> {
        let workspace_root = find_workspace_root()?;

        Ok(Self {
            target_dir: workspace_root.join("target"),
            build_dir: workspace_root.join("build"),
            sysroot_dir: workspace_root.join("sysroot"),
            config_file: workspace_root.join(".xenos-config"),
            kconfig_file: workspace_root.join("config/Kconfig.toml"),
            ovmf_dir: workspace_root.join("ovmf"),
            limine_dir: workspace_root.join("vendor/limine"),
            workspace_root,
        })
    }

    /// Путь к target для конкретного таргета
    pub fn target_for(&self, target: &str) -> PathBuf {
        self.target_dir.join(target)
    }

    /// Путь к артефакту ядра
    pub fn kernel_artifact(&self, release: bool) -> PathBuf {
        let profile = if release { "release" } else { "debug" };
        self.target_dir
            .join("x86_64-xenos-pe")
            .join(profile)
            .join("ntoskrnl.exe")
    }

    /// Путь к артефакту winload
    pub fn winload_artifact(&self, release: bool) -> PathBuf {
        let profile = if release { "release" } else { "debug" };
        self.target_dir
            .join("x86_64-xenos-uefi")
            .join(profile)
            .join("winload.efi")
    }
}

/// Константы для custom targets
pub mod targets {
    /// Custom target для ядра (PE32+)
    pub const KERNEL: &str = "x86_64-xenos-pe.json";
    /// Custom target для DLL/драйверов (PE32+)
    pub const DLL: &str = "x86_64-xenos-dll.json";
    /// Custom target для UEFI приложений
    pub const UEFI: &str = "x86_64-xenos-uefi.json";
}

/// Находит корень workspace (директория с корневым Cargo.toml)
fn find_workspace_root() -> Result<PathBuf> {
    let output = Command::new("cargo")
        .args(["locate-project", "--workspace", "--message-format=plain"])
        .output()
        .context("Failed to run cargo locate-project")?;

    if !output.status.success() {
        bail!("cargo locate-project failed");
    }

    let path = String::from_utf8(output.stdout)
        .context("Invalid UTF-8 in cargo output")?;

    let cargo_toml = PathBuf::from(path.trim());
    cargo_toml
        .parent()
        .map(|p| p.to_path_buf())
        .context("Failed to get workspace root")
}

/// Запуск команды с выводом в консоль
pub fn run_cmd(cmd: &mut Command) -> Result<()> {
    log::debug!("Running: {:?}", cmd);

    let status = cmd
        .status()
        .with_context(|| format!("Failed to execute {:?}", cmd))?;

    if !status.success() {
        bail!("Command failed with status: {}", status);
    }

    Ok(())
}

/// Запуск команды с захватом вывода
pub fn run_cmd_output(cmd: &mut Command) -> Result<String> {
    log::debug!("Running: {:?}", cmd);

    let output = cmd
        .output()
        .with_context(|| format!("Failed to execute {:?}", cmd))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Command failed: {}", stderr);
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Проверка наличия команды в PATH
pub fn has_command(name: &str) -> bool {
    which::which(name).is_ok()
}

/// Создание директории если не существует
pub fn ensure_dir(path: &Path) -> Result<()> {
    if !path.exists() {
        fs_err::create_dir_all(path)
            .with_context(|| format!("Failed to create directory: {}", path.display()))?;
    }
    Ok(())
}

