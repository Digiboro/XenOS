//! Build script for PCI bus driver
//!
//! Устанавливает entry point для PE DLL и линкует с ntoskrnl.lib.

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let workspace_root = std::path::Path::new(&manifest_dir)
        .parent() // drivers
        .unwrap()
        .parent() // boot
        .unwrap()
        .parent() // XenCore
        .unwrap();

    // Путь к build/ директории с ntoskrnl.lib
    let lib_dir = workspace_root.join("build");

    // Указываем линкеру где искать библиотеки
    println!("cargo:rustc-link-search=native={}", lib_dir.display());

    // Линкуем с ntoskrnl.lib для резолва импортов
    println!("cargo:rustc-link-lib=ntoskrnl");

    // Устанавливаем DriverEntry как entry point для PE
    println!("cargo:rustc-link-arg=/ENTRY:DriverEntry");

    // Rerun if lib changes
    println!(
        "cargo:rerun-if-changed={}",
        lib_dir.join("ntoskrnl.lib").display()
    );
}

