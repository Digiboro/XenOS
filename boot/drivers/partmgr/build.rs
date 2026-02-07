fn main() {
    // Указываем линкеру использовать DriverEntry как entry point
    println!("cargo:rustc-link-arg=/ENTRY:DriverEntry");
    
    // Путь к ntoskrnl.lib для линковки
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let workspace_root = std::path::Path::new(&manifest_dir)
        .parent().unwrap()
        .parent().unwrap()
        .parent().unwrap();
    
    let lib_path = workspace_root.join("build");
    println!("cargo:rustc-link-search=native={}", lib_path.display());
}

