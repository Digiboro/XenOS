//! Build script для ntdll
//!
//! - Вызывает spec2def.py для генерации ntdll.def и syscalls_ntdll.inc.rs из ntapi.toml
//! - Генерирует ntdll.lib (import library) из ntdll.def
//! - Указывает линкеру использовать .def для создания Export Directory
//! - Старается быть идемпотентным: не перегенерирует файлы без изменений

use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));

    // Путь к единому источнику правды - ntapi.toml
    let ntapi_toml = manifest_dir.join("../../libs/ntapi/ntapi.toml");
    println!("cargo:rerun-if-changed={}", ntapi_toml.display());

    // Путь к генератору spec2def.py
    let spec2def = manifest_dir.join("../../../tools/spec2def/spec2def.py");
    println!("cargo:rerun-if-changed={}", spec2def.display());

    // Версионный ресурс PE (version.rc -> version.res)
    let version_rc = manifest_dir.join("version.rc");
    println!("cargo:rerun-if-changed={}", version_rc.display());
    println!("cargo:rerun-if-env-changed=LLVM_RC");
    compile_version_resource(&version_rc, &out_dir);

    // Генерируем артефакты из ntapi.toml
    let def_file = out_dir.join("ntdll.def");
    let stubs_file = out_dir.join("syscalls_ntdll.inc.rs");
    let def_changed = generate_from_ntapi_toml(&ntapi_toml, &spec2def, &def_file, &stubs_file);

    // Линкеру: использовать .def для Export Directory
    if def_file.exists() {
        println!("cargo:rustc-link-arg=/DEF:{}", def_file.display());
    }

    // import library
    println!("cargo:rerun-if-env-changed=LLVM_DLLTOOL");
    generate_import_lib(&def_file, &out_dir, def_changed);
}

// =============================================================================
// Инструменты
// =============================================================================

/// Ищет исполняемый файл в PATH (учитывает .exe на Windows).
fn which_in_path(tool: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    let tool_os: OsString = tool.into();
    for dir in env::split_paths(&path) {
        let candidate = dir.join(&tool_os);
        if candidate.is_file() {
            return Some(candidate);
        }
        if cfg!(windows) && !tool.ends_with(".exe") {
            let candidate = dir.join(format!("{tool}.exe"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Возвращает путь к инструменту:
/// - сначала из переменной окружения (если задана),
/// - потом через PATH,
/// - потом по набору типовых абсолютных путей,
/// - иначе по имени (в надежде, что ОС найдёт позже).
fn resolve_tool(env_var: &str, default_name: &str, common_paths: &[&str]) -> PathBuf {
    if let Some(p) = env::var_os(env_var).filter(|v| !v.is_empty()).map(PathBuf::from) {
        return p;
    }
    if let Some(p) = which_in_path(default_name) {
        return p;
    }
    for p in common_paths {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return pb;
        }
    }
    PathBuf::from(default_name)
}

/// Единый список «типовых» путей к LLVM tools.
fn llvm_common_paths(tool_basename: &str) -> Vec<String> {
    let exe = if cfg!(windows) { ".exe" } else { "" };
    let t = format!("{tool_basename}{exe}");
    vec![
        // macOS Homebrew (Apple Silicon)
        format!("/opt/homebrew/opt/llvm/bin/{t}"),
        // macOS Homebrew (Intel)
        format!("/usr/local/opt/llvm/bin/{t}"),
        // Linux
        format!("/usr/bin/{t}"),
        format!("/usr/local/bin/{t}"),
        // Windows LLVM installer
        format!(r"C:\Program Files\LLVM\bin\{t}"),
    ]
}

// =============================================================================
// version.rc -> version.res
// =============================================================================

fn compile_version_resource(version_rc: &Path, out_dir: &Path) {
    if !version_rc.exists() {
        return;
    }

    let res_file = out_dir.join("version.res");

    let paths = llvm_common_paths("llvm-rc");
    let paths_ref: Vec<&str> = paths.iter().map(|s| s.as_str()).collect();
    let llvm_rc = resolve_tool("LLVM_RC", "llvm-rc", &paths_ref);

    // llvm-rc /fo output.res -- input.rc
    let status = Command::new(&llvm_rc)
        .args([
            "/fo",
            &res_file.to_string_lossy(),
            "--",
            &version_rc.to_string_lossy(),
        ])
        .status();

    match status {
        Ok(s) if s.success() => {
            println!("cargo:rustc-link-arg={}", res_file.display());
        }
        Ok(_) => {
            println!(
                "cargo:warning=llvm-rc failed to compile version.rc (tool: {})",
                llvm_rc.display()
            );
        }
        Err(e) => {
            println!(
                "cargo:warning=llvm-rc not found/failed to execute (tool: {}, err: {}), skipping version info",
                llvm_rc.display(),
                e
            );
        }
    }
}

// =============================================================================
// ntapi.toml -> ntdll.def + syscalls_ntdll.inc.rs
// =============================================================================

/// Вызывает spec2def.py для генерации артефактов из ntapi.toml.
/// Возвращает true, если файлы изменились.
fn generate_from_ntapi_toml(
    ntapi_toml: &Path,
    spec2def: &Path,
    def_file: &Path,
    stubs_file: &Path,
) -> bool {
    if !ntapi_toml.exists() {
        println!("cargo:warning=ntapi.toml not found: {}", ntapi_toml.display());
        return false;
    }

    if !spec2def.exists() {
        println!("cargo:warning=spec2def.py not found: {}", spec2def.display());
        return false;
    }

    // Сохраняем старое содержимое для проверки изменений
    let old_def = fs::read(def_file).ok();
    let old_stubs = fs::read(stubs_file).ok();

    // Вызываем spec2def.py
    let python = resolve_python();
    let status = Command::new(&python)
        .args([
            spec2def.to_string_lossy().as_ref(),
            "--toml",
            ntapi_toml.to_string_lossy().as_ref(),
            "--ntdll-def",
            def_file.to_string_lossy().as_ref(),
            "--ntdll-stubs",
            stubs_file.to_string_lossy().as_ref(),
        ])
        .status();

    match status {
        Ok(s) if s.success() => {
            // Проверяем изменились ли файлы
            let new_def = fs::read(def_file).ok();
            let new_stubs = fs::read(stubs_file).ok();

            let def_changed = old_def != new_def;
            let stubs_changed = old_stubs != new_stubs;

            if def_changed {
                println!("cargo:warning=Generated ntdll.def from ntapi.toml");
            }
            if stubs_changed {
                println!("cargo:warning=Generated syscalls_ntdll.inc.rs from ntapi.toml");
            }

            def_changed || stubs_changed
        }
        Ok(_) => {
            println!("cargo:warning=spec2def.py failed");
            false
        }
        Err(e) => {
            println!("cargo:warning=Failed to run spec2def.py: {}", e);
            false
        }
    }
}

/// Находит Python интерпретатор
fn resolve_python() -> PathBuf {
    // Проверяем переменную окружения
    if let Some(p) = env::var_os("PYTHON").filter(|v| !v.is_empty()).map(PathBuf::from) {
        return p;
    }

    // Пробуем python3, затем python
    for name in &["python3", "python"] {
        if let Some(p) = which_in_path(name) {
            return p;
        }
    }

    // Fallback
    PathBuf::from("python3")
}

// =============================================================================
// ntdll.def -> ntdll.lib (import lib)
// =============================================================================

fn generate_import_lib(def_file: &Path, out_dir: &Path, def_changed: bool) {
    let lib_file = out_dir.join("ntdll.lib");

    if !def_file.exists() {
        println!("cargo:warning=ntdll.def not found, skipping import lib generation");
        return;
    }

    // Если .def не изменился и .lib уже есть — пропускаем.
    if !def_changed && lib_file.exists() {
        return;
    }

    let paths = llvm_common_paths("llvm-dlltool");
    let paths_ref: Vec<&str> = paths.iter().map(|s| s.as_str()).collect();
    let dlltool = resolve_tool("LLVM_DLLTOOL", "llvm-dlltool", &paths_ref);

    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_else(|_| "x86_64".to_string());
    let machine = match target_arch.as_str() {
        "x86_64" => "i386:x86-64",
        "x86" | "i686" => "i386",
        other => {
            println!(
                "cargo:warning=Unknown target arch '{other}' for import lib, using x86-64 machine by default"
            );
            "i386:x86-64"
        }
    };

    // Не удаляем файл заранее: пусть инструмент перезапишет сам, иначе ломаем инкрементальность.
    let status = Command::new(&dlltool)
        .args([
            "-d",
            &def_file.to_string_lossy(),
            "-l",
            &lib_file.to_string_lossy(),
            "-m",
            machine,
        ])
        .status();

    match status {
        Ok(s) if s.success() => {
            println!("cargo:warning=Generated ntdll.lib at {}", lib_file.display());
        }
        Ok(_) => {
            println!(
                "cargo:warning=llvm-dlltool failed to generate ntdll.lib (tool: {})",
                dlltool.display()
            );
        }
        Err(e) => {
            println!(
                "cargo:warning=llvm-dlltool not found/failed to execute (tool: {}, err: {}), skipping import lib",
                dlltool.display(),
                e
            );
        }
    }
}
