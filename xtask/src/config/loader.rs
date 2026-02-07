//! Загрузка и сохранение конфигурации

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

// =============================================================================
// Kconfig Schema
// =============================================================================

/// Kconfig.toml structure
#[derive(Debug, Clone, Deserialize)]
pub struct Kconfig {
    pub metadata: KconfigMetadata,
    #[serde(rename = "menu", default)]
    pub menus: Vec<KconfigMenu>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct KconfigMetadata {
    pub version: u32,
    pub description: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct KconfigMenu {
    pub id: String,
    pub prompt: String,
    #[serde(default)]
    pub configs: Vec<KconfigItem>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct KconfigItem {
    pub id: String,
    #[serde(rename = "type")]
    pub item_type: String,
    pub prompt: String,
    #[serde(default)]
    pub default: Option<toml::Value>,
    #[serde(default)]
    pub depends: Option<String>,
    #[serde(default)]
    pub visible_if: Option<String>,
    #[serde(default)]
    pub help: Option<String>,
    #[serde(default)]
    pub choices: Option<Vec<String>>,
    #[serde(default)]
    pub range: Option<Vec<i32>>,
}

impl Kconfig {
    /// Load Kconfig from file
    pub fn load(path: &Path) -> Result<Self> {
        let content = fs_err::read_to_string(path)
            .with_context(|| format!("Failed to read Kconfig: {}", path.display()))?;

        // Parse directly - TOML handles [[menu]] with configs arrays correctly
        toml::from_str(&content)
            .with_context(|| format!("Failed to parse Kconfig: {}", path.display()))
    }
}

// =============================================================================
// Runtime Config
// =============================================================================

/// Конфигурация сборки XenOS (runtime config)
/// Объединяем config wrapper для совместимости с TOML
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(flatten)]
    inner: ConfigValues,
}

impl std::ops::Deref for Config {
    type Target = ConfigValues;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl std::ops::DerefMut for Config {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

/// Значения конфигурации
/// Используем UPPER_CASE для соответствия формату .xenos-config
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(non_snake_case)]
pub struct ConfigValues {
    // Build
    #[serde(default = "default_build_mode")]
    pub BUILD_MODE: String,

    // Kernel Features
    #[serde(default = "default_true")]
    pub KERNEL_ALLOC: bool,
    #[serde(default = "default_true")]
    pub KERNEL_AML: bool,

    // Debugging
    #[serde(default)]
    pub DEBUG_KD_FORCE: bool,

    // Tracing
    #[serde(default)]
    pub TRACE_ENABLE: bool,
    #[serde(default)]
    pub TRACE_CALLS: bool,
    #[serde(default)]
    pub TRACE_SCHED: bool,
    #[serde(default)]
    pub TRACE_PS: bool,
    #[serde(default)]
    pub TRACE_MM: bool,
    #[serde(default)]
    pub TRACE_OB: bool,
    #[serde(default)]
    pub TRACE_IO: bool,
    #[serde(default)]
    pub TRACE_STORAGE: bool,

    // Testing
    #[serde(default)]
    pub TEST_KERNEL: bool,
    #[serde(default)]
    pub TEST_PS: bool,

    // QEMU
    #[serde(default = "default_qemu_machine")]
    pub QEMU_MACHINE: String,
    #[serde(default = "default_qemu_cpu")]
    pub QEMU_CPU: String,
    #[serde(default = "default_qemu_accel")]
    pub QEMU_ACCEL: String,
    #[serde(default = "default_qemu_memory")]
    pub QEMU_MEMORY: String,
    #[serde(default = "default_one")]
    pub QEMU_SMP: i32,
    #[serde(default = "default_qemu_display")]
    pub QEMU_DISPLAY: String,
    #[serde(default = "default_qemu_vga")]
    pub QEMU_VGA: String,
    #[serde(default = "default_qemu_monitor")]
    pub QEMU_MONITOR: String,
    #[serde(default = "default_qemu_serial")]
    pub QEMU_SERIAL: String,
    #[serde(default = "default_serial_file")]
    pub QEMU_SERIAL_FILE: String,

    // Build Options
    #[serde(default = "default_true")]
    pub BUILD_DRIVERS: bool,
    #[serde(default)]
    pub BUILD_ISO: bool,
    #[serde(default)]
    pub BUILD_LIMINE: bool,
}

// Default value functions for serde
fn default_build_mode() -> String { "debug".into() }
fn default_true() -> bool { true }
fn default_one() -> i32 { 1 }
fn default_qemu_machine() -> String { "q35".into() }
fn default_qemu_cpu() -> String { "max".into() }
fn default_qemu_accel() -> String { "none".into() }
fn default_qemu_memory() -> String { "1G".into() }
fn default_qemu_display() -> String { "sdl".into() }
fn default_qemu_vga() -> String { "std".into() }
fn default_qemu_monitor() -> String { "vc".into() }
fn default_qemu_serial() -> String { "stdio".into() }
fn default_serial_file() -> String { "serial.txt".into() }

impl Default for ConfigValues {
    fn default() -> Self {
        Self {
            BUILD_MODE: default_build_mode(),
            KERNEL_ALLOC: default_true(),
            KERNEL_AML: default_true(),
            DEBUG_KD_FORCE: false,
            TRACE_ENABLE: false,
            TRACE_CALLS: false,
            TRACE_SCHED: false,
            TRACE_PS: false,
            TRACE_MM: false,
            TRACE_OB: false,
            TRACE_IO: false,
            TRACE_STORAGE: false,
            TEST_KERNEL: false,
            TEST_PS: false,
            QEMU_MACHINE: default_qemu_machine(),
            QEMU_CPU: default_qemu_cpu(),
            QEMU_ACCEL: default_qemu_accel(),
            QEMU_MEMORY: default_qemu_memory(),
            QEMU_SMP: default_one(),
            QEMU_DISPLAY: default_qemu_display(),
            QEMU_VGA: default_qemu_vga(),
            QEMU_MONITOR: default_qemu_monitor(),
            QEMU_SERIAL: default_qemu_serial(),
            QEMU_SERIAL_FILE: default_serial_file(),
            BUILD_DRIVERS: default_true(),
            BUILD_ISO: false,
            BUILD_LIMINE: false,
        }
    }
}

impl Config {
    /// Загружает конфигурацию из файла
    pub fn load(path: &Path) -> Result<Self> {
        let content = fs_err::read_to_string(path)
            .with_context(|| format!("Failed to read config: {}", path.display()))?;

        toml::from_str(&content)
            .with_context(|| format!("Failed to parse config: {}", path.display()))
    }

    /// Загружает конфигурацию или возвращает defaults
    pub fn load_or_default(path: &Path) -> Result<Self> {
        if path.exists() {
            Self::load(path)
        } else {
            Ok(Self::defaults())
        }
    }

    /// Сохраняет конфигурацию в файл
    pub fn save(&self, path: &Path) -> Result<()> {
        let content = toml::to_string_pretty(self).context("Failed to serialize config")?;

        // Добавляем header
        let header = r#"# XenOS Build Configuration
# Generated by: cargo xtask defconfig
#
# WARNING: Do not edit manually unless you know what you're doing.
# Use 'cargo xtask menuconfig' for interactive configuration.

"#;

        fs_err::write(path, format!("{}{}", header, content))
            .with_context(|| format!("Failed to write config: {}", path.display()))
    }

    /// Создает конфигурацию по умолчанию
    pub fn defaults() -> Self {
        Self {
            inner: ConfigValues::default(),
        }
    }

    /// Получает bool значение
    pub fn get_bool(&self, key: &str) -> bool {
        match key {
            "KERNEL_ALLOC" => self.inner.KERNEL_ALLOC,
            "KERNEL_AML" => self.inner.KERNEL_AML,
            "DEBUG_KD_FORCE" => self.inner.DEBUG_KD_FORCE,
            "TRACE_ENABLE" => self.inner.TRACE_ENABLE,
            "TRACE_CALLS" => self.inner.TRACE_CALLS,
            "TRACE_SCHED" => self.inner.TRACE_SCHED,
            "TRACE_PS" => self.inner.TRACE_PS,
            "TRACE_MM" => self.inner.TRACE_MM,
            "TRACE_OB" => self.inner.TRACE_OB,
            "TRACE_IO" => self.inner.TRACE_IO,
            "TRACE_STORAGE" => self.inner.TRACE_STORAGE,
            "TEST_KERNEL" => self.inner.TEST_KERNEL,
            "TEST_PS" => self.inner.TEST_PS,
            "BUILD_DRIVERS" => self.inner.BUILD_DRIVERS,
            "BUILD_ISO" => self.inner.BUILD_ISO,
            "BUILD_LIMINE" => self.inner.BUILD_LIMINE,
            _ => false,
        }
    }

    /// Получает string значение
    pub fn get_string(&self, key: &str) -> String {
        match key {
            "BUILD_MODE" => self.inner.BUILD_MODE.clone(),
            "QEMU_MACHINE" => self.inner.QEMU_MACHINE.clone(),
            "QEMU_CPU" => self.inner.QEMU_CPU.clone(),
            "QEMU_ACCEL" => self.inner.QEMU_ACCEL.clone(),
            "QEMU_MEMORY" => self.inner.QEMU_MEMORY.clone(),
            "QEMU_DISPLAY" => self.inner.QEMU_DISPLAY.clone(),
            "QEMU_VGA" => self.inner.QEMU_VGA.clone(),
            "QEMU_MONITOR" => self.inner.QEMU_MONITOR.clone(),
            "QEMU_SERIAL" => self.inner.QEMU_SERIAL.clone(),
            "QEMU_SERIAL_FILE" => self.inner.QEMU_SERIAL_FILE.clone(),
            _ => String::new(),
        }
    }

    /// Получает int значение
    pub fn get_int(&self, key: &str) -> i32 {
        match key {
            "QEMU_SMP" => self.inner.QEMU_SMP,
            _ => 0,
        }
    }
}

