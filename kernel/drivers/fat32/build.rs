use std::env;
use std::path::PathBuf;

fn main() {
    // Получаем путь к workspace root
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let manifest_path = PathBuf::from(&manifest_dir);
    let workspace_root = manifest_path
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    
    let lib_path = workspace_root.join("build");
    
    // Добавляем путь к ntoskrnl.lib для линковки
    println!("cargo:rustc-link-search=native={}", lib_path.display());
    println!("cargo:rustc-link-lib=static=ntoskrnl");
    
    // Устанавливаем DriverEntry как entry point
    println!("cargo:rustc-link-arg=/ENTRY:DriverEntry");
    println!("cargo:rustc-link-arg=/NODEFAULTLIB");
}

