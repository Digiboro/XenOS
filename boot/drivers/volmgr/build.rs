fn main() {
    // Указываем линкеру использовать DriverEntry как entry point
    println!("cargo:rustc-link-arg=/ENTRY:DriverEntry");
}

