//! Build script для ntoskrnl
//!
//! - Собирает asm-файлы (.S) для x86_64
//! - Генерирует ntoskrnl.lib (import library) из ntoskrnl.def
//! - Указывает линкеру использовать .def для создания Export Directory

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let target = env::var("TARGET").unwrap_or_default();
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // Указываем линкеру использовать .def файл для создания Export Directory
    // Это необходимо для того чтобы boot-драйверы могли импортировать функции из ntoskrnl.exe
    let def_file = manifest_dir.join("ntoskrnl.def");
    if def_file.exists() {
        println!("cargo:rustc-link-arg=/DEF:{}", def_file.display());
        println!("cargo:rerun-if-changed=ntoskrnl.def");
    }

    // Генерируем import library из .def файла
    generate_import_lib(&manifest_dir, &out_dir);

    // Добавляем путь к import libraries динамически загружаемых драйверов
    // bootvid.lib генерируется xtask до сборки ядра
    let build_dir = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.join("build"));

    if let Some(build_dir) = build_dir {
        if build_dir.exists() {
            println!("cargo:rustc-link-search=native={}", build_dir.display());
            println!("cargo:rerun-if-changed={}/bootvid.lib", build_dir.display());
        }
    }

    // Собираем ASM только для x86_64 таргетов
    if target.starts_with("x86_64") {
        println!("cargo:rerun-if-changed=src/arch/x86_64/asm/ctxswitch.S");
        println!("cargo:rerun-if-changed=src/arch/x86_64/asm/boot.S");
        println!("cargo:rerun-if-changed=src/arch/x86_64/asm/interrupt.S");

        // Список ASM файлов для сборки
        let asm_files = [
            ("ctxswitch.S", "ctxswitch.o"),
            ("boot.S", "boot.o"),
            ("interrupt.S", "interrupt.o"),
        ];

        // Определяем путь к clang
        let clang = env::var("CLANG").unwrap_or_else(|_| {
            if std::path::Path::new("/opt/homebrew/opt/llvm/bin/clang").exists() {
                "/opt/homebrew/opt/llvm/bin/clang".to_string()
            } else {
                "clang".to_string()
            }
        });

        let mut obj_files = Vec::new();

        // Собираем каждый .S файл в .o
        for (src_name, obj_name) in &asm_files {
            let src_file = manifest_dir.join("src/arch/x86_64/asm").join(src_name);
            let obj_file = out_dir.join(obj_name);

            // Удаляем старый файл если есть
            let _ = std::fs::remove_file(&obj_file);

            // Собираем .S в .o с COFF форматом для Windows target
            let status = Command::new(&clang)
                .args([
                    "-target",
                    "x86_64-pc-windows-msvc",
                    "-c",
                    "-x",
                    "assembler-with-cpp",
                    "-nostdinc",
                    "-ffreestanding",
                    "-o",
                ])
                .arg(&obj_file)
                .arg(&src_file)
                .status()
                .expect(&format!("Failed to run clang for {}", src_name));

            if !status.success() {
                panic!("clang failed to compile {}", src_name);
            }

            // Проверяем что .o файл создан
            if !obj_file.exists() {
                panic!("clang did not create object file for {}", src_name);
            }

            obj_files.push(obj_file);
        }

        // Создаём COFF архив со всеми объектными файлами
        let lib_file = out_dir.join("asm.lib");
        let _ = std::fs::remove_file(&lib_file);

        let llvm_lib = if std::path::Path::new("/opt/homebrew/opt/llvm/bin/llvm-lib").exists() {
            "/opt/homebrew/opt/llvm/bin/llvm-lib".to_string()
        } else {
            "llvm-lib".to_string()
        };

        // llvm-lib использует синтаксис: llvm-lib /out:file.lib file1.o file2.o ...
        let out_arg = format!("/out:{}", lib_file.display());
        let status = Command::new(&llvm_lib)
            .arg(&out_arg)
            .args(&obj_files)
            .status()
            .expect("Failed to run llvm-lib");

        if !status.success() {
            panic!("llvm-lib failed to create archive");
        }

        // Указываем cargo где искать библиотеку
        println!("cargo:rustc-link-search=native={}", out_dir.display());
        println!("cargo:rustc-link-lib=static=asm");
    }
}

/// Генерирует import library (ntoskrnl.lib) из ntoskrnl.def
///
/// Import library используется для линковки драйверов на C/C++ и внешних
/// инструментов с ядром. Содержит заглушки для всех экспортируемых функций.
///
/// Выходной файл: $OUT_DIR/ntoskrnl.lib
fn generate_import_lib(manifest_dir: &PathBuf, out_dir: &PathBuf) {
    let def_file = manifest_dir.join("ntoskrnl.def");
    let lib_file = out_dir.join("ntoskrnl.lib");

    // Проверяем наличие .def файла
    if !def_file.exists() {
        println!("cargo:warning=ntoskrnl.def not found, skipping import lib generation");
        return;
    }

    // Определяем путь к llvm-dlltool
    let dlltool = if std::path::Path::new("/opt/homebrew/opt/llvm/bin/llvm-dlltool").exists() {
        "/opt/homebrew/opt/llvm/bin/llvm-dlltool".to_string()
    } else {
        "llvm-dlltool".to_string()
    };

    // Удаляем старый .lib если есть
    let _ = std::fs::remove_file(&lib_file);

    // Генерируем import library:
    // llvm-dlltool -d ntoskrnl.def -l ntoskrnl.lib -m i386:x86-64
    let status = Command::new(&dlltool)
        .args([
            "-d",
            &def_file.to_string_lossy(),
            "-l",
            &lib_file.to_string_lossy(),
            "-m",
            "i386:x86-64",
        ])
        .status();

    match status {
        Ok(s) if s.success() => {
            println!(
                "cargo:warning=Generated ntoskrnl.lib at {}",
                lib_file.display()
            );
        },
        Ok(_) => {
            println!("cargo:warning=llvm-dlltool failed to generate ntoskrnl.lib");
        },
        Err(e) => {
            println!(
                "cargo:warning=llvm-dlltool not found ({}), skipping import lib",
                e
            );
        },
    }
}
