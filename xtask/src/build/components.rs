//! Универсальная сборка компонентов
//!
//! Все компоненты (kernel, bootloader, drivers, libraries, applications)
//! обнаруживаются автоматически по наличию `component.toml` манифеста.
//!
//! Если `component.toml` отсутствует — компонент игнорируется.
//! Если `enabled = false` — компонент пропускается.

use anyhow::{Result, bail, Context};
use std::path::{Path, PathBuf};
use std::process::Command;
use super::BuildContext;
use super::manifest::{ComponentManifest, ComponentType};
use crate::util::{run_cmd, ensure_dir};

/// Директории для поиска компонентов
const COMPONENT_LOCATIONS: &[&str] = &[
    // Ядро и загрузчик
    "kernel/ntoskrnl",
    "boot/winload",
    // Драйверы
    "boot/drivers",
    "kernel/drivers",
    // Библиотеки (будущее)
    "libs",
    // Приложения (будущее)
    "usermode",
];

/// Результат обнаружения компонентов
pub struct DiscoveredComponent {
    pub path: PathBuf,
    pub manifest: ComponentManifest,
}

/// Обнаруживает все компоненты с component.toml
pub fn discover_all(ctx: &BuildContext) -> Result<Vec<DiscoveredComponent>> {
    let mut components = Vec::new();

    for location in COMPONENT_LOCATIONS {
        let loc_path = ctx.paths.workspace_root.join(location);
        
        if !loc_path.exists() {
            continue;
        }

        // Проверяем, это директория с component.toml или директория с поддиректориями
        if ComponentManifest::exists(&loc_path) {
            // Это сам компонент (например kernel/ntoskrnl)
            if let Ok(manifest) = ComponentManifest::load(&loc_path) {
                components.push(DiscoveredComponent {
                    path: loc_path,
                    manifest,
                });
            }
        } else if loc_path.is_dir() {
            // Это директория с компонентами (например boot/drivers)
            for entry in fs_err::read_dir(&loc_path)? {
                let entry = entry?;
                let component_dir = entry.path();
                
                if !component_dir.is_dir() {
                    continue;
                }

                if ComponentManifest::exists(&component_dir) {
                    if let Ok(manifest) = ComponentManifest::load(&component_dir) {
                        components.push(DiscoveredComponent {
                            path: component_dir,
                            manifest,
                        });
                    }
                }
            }
        }
    }

    Ok(components)
}

/// Собирает все включенные компоненты
pub fn build_all(ctx: &BuildContext) -> Result<()> {
    log::info!("=== Discovering components ===");
    
    let components = discover_all(ctx)?;
    let enabled: Vec<_> = components.iter()
        .filter(|c| c.manifest.package.enabled)
        .collect();
    
    log::info!("Found {} component(s), {} enabled", components.len(), enabled.len());

    // Группируем по типу для правильного порядка сборки
    // Порядок: kernel -> bootloader -> drivers -> libraries -> applications
    let order = [
        ComponentType::Kernel,
        ComponentType::Bootloader,
        ComponentType::Driver,
        ComponentType::Library,
        ComponentType::Application,
    ];

    for component_type in order {
        let of_type: Vec<_> = enabled.iter()
            .filter(|c| c.manifest.package.component_type == component_type)
            .collect();
        
        if of_type.is_empty() {
            continue;
        }

        log::info!("=== Building {:?}s ({}) ===", component_type, of_type.len());
        
        for component in of_type {
            build_component(ctx, component)?;
        }
    }

    Ok(())
}

/// Собирает конкретный компонент по имени
pub fn build_one(ctx: &BuildContext, name: &str) -> Result<()> {
    let components = discover_all(ctx)?;
    
    let component = components.iter()
        .find(|c| c.manifest.package.name == name)
        .ok_or_else(|| anyhow::anyhow!("Component '{}' not found", name))?;
    
    if !component.manifest.package.enabled {
        bail!("Component '{}' is disabled in component.toml", name);
    }
    
    build_component(ctx, component)
}

/// Собирает один компонент
fn build_component(ctx: &BuildContext, component: &DiscoveredComponent) -> Result<()> {
    let manifest = &component.manifest;
    let name = &manifest.package.name;
    
    log::info!("Building: {}", name);

    // Проверяем наличие Cargo.toml (Rust компонент)
    let cargo_toml = component.path.join("Cargo.toml");
    
    if cargo_toml.exists() {
        build_rust_component(ctx, &component.path, manifest)?;
    } else {
        // C/другой компонент - только копируем если собран
        copy_prebuilt_component(ctx, &component.path, manifest)?;
    }

    Ok(())
}

/// Собирает Rust компонент
fn build_rust_component(ctx: &BuildContext, component_dir: &Path, manifest: &ComponentManifest) -> Result<()> {
    let mut cmd = Command::new("cargo");
    cmd.current_dir(&ctx.paths.workspace_root)  // Нужно для поиска target .json файлов
        .arg("build")
        .arg("--manifest-path")
        .arg(component_dir.join("Cargo.toml"))
        .arg("--target").arg(manifest.cargo_target());

    // Профиль сборки
    if manifest.is_release() {
        cmd.arg("--release");
    }

    // Build-std если нужен
    if manifest.package.component_type.needs_build_std() {
        cmd.arg("-Zbuild-std=core,alloc")
            .arg("-Zbuild-std-features=compiler-builtins-mem");
    }

    // Собираем features
    let mut features: Vec<String> = manifest.build.features.clone();
    
    // Условные features
    for (config_key, feature) in &manifest.build.conditional_features {
        if ctx.config.get_bool(config_key) {
            features.push(feature.clone());
        }
    }
    
    if !features.is_empty() {
        cmd.arg("--features").arg(features.join(","));
    }

    // Добавляем путь к .lib файлам
    let lib_path = ctx.paths.workspace_root.join("build");
    cmd.env("RUSTFLAGS", format!("-L {}", lib_path.display()));

    run_cmd(&mut cmd)?;

    // Копируем в sysroot
    copy_to_sysroot(ctx, component_dir, manifest)?;

    Ok(())
}

/// Копирует собранный компонент в sysroot
fn copy_to_sysroot(ctx: &BuildContext, component_dir: &Path, manifest: &ComponentManifest) -> Result<()> {
    let name = &manifest.package.name;
    
    // Rust заменяет дефисы на подчёркивания
    let file_name = name.replace('-', "_");
    
    // Определяем расширение и ищем файл
    // cargo_target() возвращает "x86_64-xenos-dll.json", но директория без .json
    let target_name = manifest.cargo_target().trim_end_matches(".json");
    let target_dir = ctx.paths.target_dir.join(target_name);
    let profile_dir = target_dir.join(manifest.profile());
    
    // Пробуем разные расширения
    let extensions = match manifest.package.component_type {
        ComponentType::Kernel => vec!["exe"],
        ComponentType::Bootloader => vec!["efi"],
        ComponentType::Driver => vec!["dll", "sys"],
        ComponentType::Library => vec!["dll"],
        ComponentType::Application => vec!["exe"],
    };

    let mut src_path = None;
    for ext in extensions {
        let path = profile_dir.join(format!("{}.{}", file_name, ext));
        if path.exists() {
            src_path = Some(path);
            break;
        }
    }

    let src_path = src_path.ok_or_else(|| {
        anyhow::anyhow!("Built artifact not found for '{}' in {}", name, profile_dir.display())
    })?;

    // Создаём директорию назначения
    let dest_dir = ctx.paths.sysroot_dir.join(manifest.destination_dir());
    ensure_dir(&dest_dir)?;

    // Копируем с именем из манифеста
    let dest_path = ctx.paths.sysroot_dir.join(manifest.sysroot_path());
    fs_err::copy(&src_path, &dest_path)
        .with_context(|| format!("Failed to copy {} to {}", src_path.display(), dest_path.display()))?;
    
    log::info!("  -> {}", dest_path.display());

    Ok(())
}

/// Копирует предварительно собранный компонент (C/другой)
fn copy_prebuilt_component(ctx: &BuildContext, component_dir: &Path, manifest: &ComponentManifest) -> Result<()> {
    let name = &manifest.package.name;
    let file_name = name.replace('-', "_");
    
    // Ищем в build/ директории компонента
    let build_dir = component_dir.join("build");
    
    let extensions = ["sys", "dll", "exe", "efi"];
    let mut src_path = None;
    
    for ext in extensions {
        let path = build_dir.join(format!("{}.{}", file_name, ext));
        if path.exists() {
            src_path = Some(path);
            break;
        }
    }

    let Some(src_path) = src_path else {
        log::info!("  Prebuilt '{}' not found (build manually)", name);
        return Ok(());
    };

    // Создаём директорию назначения
    let dest_dir = ctx.paths.sysroot_dir.join(manifest.destination_dir());
    ensure_dir(&dest_dir)?;

    // Копируем
    let dest_path = ctx.paths.sysroot_dir.join(manifest.sysroot_path());
    fs_err::copy(&src_path, &dest_path)?;
    
    log::info!("  Prebuilt -> {}", dest_path.display());

    Ok(())
}

// === Обратная совместимость с drivers.rs ===

/// Собирает все драйверы (алиас для build_all с фильтром)
pub fn build_drivers(ctx: &BuildContext) -> Result<()> {
    log::info!("=== Building drivers ===");
    
    let components = discover_all(ctx)?;
    let drivers: Vec<_> = components.iter()
        .filter(|c| c.manifest.package.component_type == ComponentType::Driver)
        .filter(|c| c.manifest.package.enabled)
        .collect();
    
    log::info!("Found {} driver(s)", drivers.len());
    
    for driver in drivers {
        build_component(ctx, driver)?;
    }

    Ok(())
}

/// Собирает ядро
pub fn build_kernel(ctx: &BuildContext) -> Result<()> {
    build_one(ctx, "ntoskrnl")
}

/// Собирает загрузчик
pub fn build_bootloader(ctx: &BuildContext) -> Result<()> {
    build_one(ctx, "winload")
}

