//! Component manifest parser (component.toml)
//!
//! Каждый компонент системы (kernel, bootloader, driver, library, application)
//! имеет `component.toml` с метаданными для сборки и установки.
//!
//! Если `component.toml` отсутствует — компонент не собирается.

use anyhow::{Result, Context};
use serde::Deserialize;
use std::path::Path;

/// Манифест компонента (component.toml)
#[derive(Debug, Deserialize)]
pub struct ComponentManifest {
    pub package: PackageInfo,
    #[serde(default)]
    pub build: BuildInfo,
}

/// Тип компонента
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ComponentType {
    /// Ядро системы (ntoskrnl.exe)
    Kernel,
    /// UEFI загрузчик (winload.efi)
    Bootloader,
    /// Драйвер (.dll/.sys)
    Driver,
    /// Библиотека (.dll)
    Library,
    /// Приложение (.exe)
    Application,
}

impl ComponentType {
    /// Возвращает cargo target для данного типа
    pub fn cargo_target(&self) -> &'static str {
        use crate::util::targets;
        match self {
            ComponentType::Kernel => targets::KERNEL,
            ComponentType::Bootloader => targets::UEFI,
            ComponentType::Driver => targets::DLL,
            ComponentType::Library => targets::DLL,
            ComponentType::Application => targets::DLL, // TODO: отдельный target для exe
        }
    }
    
    /// Профиль сборки по умолчанию
    pub fn default_profile(&self) -> &'static str {
        match self {
            ComponentType::Kernel => "debug",      // kernel может быть debug
            ComponentType::Bootloader => "debug",  // bootloader тоже
            ComponentType::Driver => "release",    // драйверы всегда release
            ComponentType::Library => "release",
            ComponentType::Application => "release",
        }
    }
    
    /// Нужен ли build-std
    pub fn needs_build_std(&self) -> bool {
        match self {
            ComponentType::Kernel => true,
            ComponentType::Bootloader => true,
            ComponentType::Driver => true,
            ComponentType::Library => true,
            ComponentType::Application => true, // пока все нуждаются
        }
    }
}

/// Основная информация о компоненте
#[derive(Debug, Deserialize)]
pub struct PackageInfo {
    /// Имя компонента (например "bootvid", "ntoskrnl")
    pub name: String,
    
    /// Тип компонента
    #[serde(rename = "type")]
    pub component_type: ComponentType,
    
    /// Включена ли сборка (по умолчанию true)
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    
    /// Путь назначения с placeholders
    /// Например: "%SystemRoot%\\system32\\bootvid.dll"
    pub destination: String,
}

fn default_enabled() -> bool {
    true
}

/// Настройки сборки
#[derive(Debug, Default, Deserialize)]
pub struct BuildInfo {
    /// Cargo features для включения
    #[serde(default)]
    pub features: Vec<String>,
    
    /// Условные features (включаются если соответствующий config включен)
    /// Формат: { "TRACE_STORAGE" = "storage-trace" }
    #[serde(default)]
    pub conditional_features: std::collections::HashMap<String, String>,
    
    /// Переопределение профиля сборки (debug/release)
    /// Если не указан — используется default для типа
    pub profile: Option<String>,
    
    /// Переопределение target
    /// Если не указан — используется default для типа
    pub target: Option<String>,
}

impl ComponentManifest {
    /// Имя файла манифеста
    pub const FILENAME: &'static str = "component.toml";
    
    /// Загружает манифест из component.toml
    pub fn load(component_dir: &Path) -> Result<Self> {
        let manifest_path = component_dir.join(Self::FILENAME);
        let content = fs_err::read_to_string(&manifest_path)
            .with_context(|| format!("Failed to read {}", manifest_path.display()))?;
        
        let manifest: ComponentManifest = toml::from_str(&content)
            .with_context(|| format!("Failed to parse {}", manifest_path.display()))?;
        
        Ok(manifest)
    }
    
    /// Проверяет существует ли манифест
    pub fn exists(component_dir: &Path) -> bool {
        component_dir.join(Self::FILENAME).exists()
    }
    
    /// Cargo target для сборки
    pub fn cargo_target(&self) -> &str {
        self.build.target.as_deref()
            .unwrap_or_else(|| self.package.component_type.cargo_target())
    }
    
    /// Профиль сборки (debug/release)
    pub fn profile(&self) -> &str {
        self.build.profile.as_deref()
            .unwrap_or_else(|| self.package.component_type.default_profile())
    }
    
    /// Нужен ли --release флаг
    pub fn is_release(&self) -> bool {
        self.profile() == "release"
    }
    
    /// Разрешает placeholders в пути назначения
    /// 
    /// Поддерживаемые placeholders:
    /// - %SystemRoot% -> XenOS
    /// - %EFI% -> EFI
    /// - %Boot% -> boot
    pub fn resolve_destination(&self) -> String {
        self.package.destination
            .replace("%SystemRoot%", "XenOS")
            .replace("%EFI%", "EFI")
            .replace("%Boot%", "boot")
            .replace('\\', "/")  // Нормализуем слэши для Unix
    }
    
    /// Возвращает относительный путь в sysroot
    /// Например: "XenOS/system32/bootvid.dll"
    pub fn sysroot_path(&self) -> String {
        self.resolve_destination()
    }
    
    /// Возвращает директорию назначения (без имени файла)
    pub fn destination_dir(&self) -> String {
        let resolved = self.resolve_destination();
        if let Some(pos) = resolved.rfind('/') {
            resolved[..pos].to_string()
        } else {
            resolved
        }
    }
    
    /// Возвращает имя файла из destination
    pub fn destination_filename(&self) -> String {
        let resolved = self.resolve_destination();
        if let Some(pos) = resolved.rfind('/') {
            resolved[pos + 1..].to_string()
        } else {
            resolved
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_resolve_destination() {
        let toml = r#"
            [package]
            name = "bootvid"
            type = "driver"
            destination = "%SystemRoot%\\system32\\bootvid.dll"
        "#;
        
        let manifest: ComponentManifest = toml::from_str(toml).unwrap();
        
        assert_eq!(manifest.resolve_destination(), "XenOS/system32/bootvid.dll");
        assert_eq!(manifest.destination_dir(), "XenOS/system32");
        assert_eq!(manifest.destination_filename(), "bootvid.dll");
        assert!(manifest.package.enabled);
    }
    
    #[test]
    fn test_disabled_component() {
        let toml = r#"
            [package]
            name = "test"
            type = "driver"
            enabled = false
            destination = "%SystemRoot%\\system32\\test.dll"
        "#;
        
        let manifest: ComponentManifest = toml::from_str(toml).unwrap();
        assert!(!manifest.package.enabled);
    }
}
