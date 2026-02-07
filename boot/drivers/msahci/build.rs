fn main() {
    // Указываем линкеру использовать DriverEntry как entry point
    println!("cargo:rustc-link-arg=/ENTRY:DriverEntry");
    // Линкуем с ntoskrnl для DbgPrint и других kernel exports
    println!("cargo:rustc-link-lib=ntoskrnl");
}

