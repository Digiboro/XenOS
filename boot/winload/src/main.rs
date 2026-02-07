//! winload.efi — NT-подобный загрузчик (параллельная цепочка через Limine EFI chainload)
//!
//! Шаг 1: UEFI инфраструктура (boot_context.rs)
//! Шаг 2: Чтение обязательных файлов (fs.rs)
//! Шаг 3: Парсинг SYSTEM hive (hive.rs)
//! Шаг 4: Загрузка boot drivers (loader.rs)
//! Шаг 5: Подготовка loader structures + ExitBootServices (ntcompat.rs, memory.rs)
//!
//! ВАЖНО: в целевой архитектуре этот компонент будет:
//! Limine → winload.efi → загрузка ntoskrnl.exe/драйверов → ExitBootServices → KiSystemStartup().

#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use alloc::string::ToString;

// Модули
mod boot_context;
mod boot_drivers;
mod bsod;
mod console;
mod fs;
mod loader;
mod memory;
mod ntcompat;
mod paging;
mod types;

use core::panic::PanicInfo;
use core::time::Duration;

use console::print;
use console::print_hex;
use console::print_num;
use console::println;
use types::MAX_UNICODE_NLS_SIZE;
use types::NTOSKRNL_PATH;
use types::STATUS_CANNOT_LOAD_REGISTRY_FILE;
use types::SYSTEM_HIVE_PATH;
use types::UNICODE_NLS_PATH;
use uefi::boot;
use uefi::prelude::*;

#[entry]
fn efi_main() -> Status {
    // Инициализируем uefi helpers (включая global allocator)
    let _ = uefi::helpers::init();

    // Сбрасываем консоль
    uefi::system::with_stdout(|stdout| {
        let _ = stdout.reset(false);
    });

    println("");
    println("========================================");
    println("  XenOS Loader");
    println("  Version: v0.0.1-dev");
    println("========================================");
    println("");

    // =========================================================================
    // Шаг 1: Сбор UEFI boot context
    // =========================================================================
    let ctx = boot_context::collect_boot_context();

    // Итоговая диагностика Шага 1
    println("");
    println("========================================");
    println("  Boot Context Summary");
    println("========================================");
    print("  Load Options:    ");
    if ctx.load_options_length > 0 {
        print_num(ctx.load_options_length as u64);
        println(" bytes");
    } else {
        println("none");
    }
    print("  Framebuffer:     ");
    println(if ctx.framebuffer.is_some() {
        "available"
    } else {
        "not available"
    });
    print("  Memory Map:      ");
    print_num(ctx.memory_map_entries as u64);
    println(" entries");
    print("  File System:     ");
    println(if ctx.fs_available { "OK" } else { "FAILED" });
    println("");

    // =========================================================================
    // Шаг 2: Чтение обязательных файлов
    // =========================================================================
    println("========================================");
    println("  Step 2: Loading Required Files");
    println("========================================");
    println("");

    // 2.1: Читаем SYSTEM hive
    print("[1/3] Loading SYSTEM hive: ");
    println(SYSTEM_HIVE_PATH);

    let system_hive = match fs::read_file(SYSTEM_HIVE_PATH) {
        Some(file) => {
            print("       Size: ");
            print_num(file.data.len() as u64);
            println(" bytes");
            println("       Status: OK");
            file
        },
        None => {
            println("       Status: FAILED");
            println("");
            bsod::show_bsod(
                STATUS_CANNOT_LOAD_REGISTRY_FILE,
                "{Registry File Failure}",
                SYSTEM_HIVE_PATH,
            );
        },
    };

    // Базовая валидация SYSTEM hive (должен начинаться с "regf")
    if system_hive.data.len() < 4
        || system_hive.data[0] != b'r'
        || system_hive.data[1] != b'e'
        || system_hive.data[2] != b'g'
        || system_hive.data[3] != b'f'
    {
        println("       WARNING: Invalid hive signature (expected 'regf')");
    } else {
        println("       Hive signature: OK (regf)");
    }
    println("");

    // 2.2: Читаем ntoskrnl
    print("[2/3] Loading kernel: ");
    println(NTOSKRNL_PATH);

    let ntoskrnl = match fs::read_file(NTOSKRNL_PATH) {
        Some(file) => {
            print("       Size: ");
            print_num(file.data.len() as u64);
            println(" bytes");
            println("       Status: OK");
            file
        },
        None => {
            println("       Status: FAILED - file not found");
            println("");
            bsod::show_bsod(
                types::STATUS_INVALID_IMAGE_FORMAT,
                "{Bad Image}",
                NTOSKRNL_PATH,
            );
        },
    };

    // Валидируем PE формат
    println("       Validating PE format...");
    let pe_info = pe::validate_pe(&ntoskrnl.data);

    // Загружаем PE образ ядра
    let kernel_module = if !pe_info.valid {
        println("       ERROR: Not a valid PE file!");
        bsod::show_bsod(
            types::STATUS_INVALID_IMAGE_FORMAT,
            "Invalid ntoskrnl.exe format",
            NTOSKRNL_PATH,
        );
    } else {
        print("       Machine:      0x");
        print_hex(pe_info.machine as u64);
        if pe_info.machine == 0x8664 {
            println(" (AMD64)");
        } else {
            println(" (unexpected)");
        }

        print("       Format:       ");
        if pe_info.is_pe32_plus {
            println("PE32+ (64-bit)");
        } else {
            println("PE32 (32-bit) - ERROR: 64-bit required");
            bsod::show_bsod(
                types::STATUS_INVALID_IMAGE_FORMAT,
                "ntoskrnl.exe must be PE32+ (64-bit)",
                NTOSKRNL_PATH,
            );
        }

        print("       Entry Point:  0x");
        print_hex(pe_info.entry_point as u64);
        println("");

        print("       Image Base:   0x");
        print_hex(pe_info.image_base);
        println("");

        print("       Image Size:   0x");
        print_hex(pe_info.size_of_image as u64);
        print(" (");
        print_num(pe_info.size_of_image as u64);
        println(" bytes)");

        // Загружаем PE образ в память
        // VA base для ntoskrnl = KERNEL_VA_BASE
        println("       Loading PE image...");
        match loader::load_pe_image(
            &ntoskrnl.data,
            String::from("ntoskrnl.exe"),
            String::from(NTOSKRNL_PATH),
            paging::KERNEL_VA_BASE,
        ) {
            Ok(module) => {
                print("       Phys base:    0x");
                print_hex(module.phys_base);
                println("");
                print("       VA base:      0x");
                print_hex(module.va_base);
                println("");
                print("       Entry RVA:    0x");
                print_hex(module.entry_rva as u64);
                println("");
                print("       Entry VA:     0x");
                print_hex(module.entry_point_va());
                println("");
                module
            },
            Err(e) => {
                print("       FAILED: ");
                println(match e {
                    loader::LoadError::FileTooSmall => "File too small",
                    loader::LoadError::InvalidDosSignature => "Invalid DOS signature",
                    loader::LoadError::InvalidPeSignature => "Invalid PE signature",
                    loader::LoadError::NotPe32Plus => "Not PE32+",
                    loader::LoadError::UnsupportedMachine => "Unsupported machine type",
                    loader::LoadError::AllocationFailed => "Memory allocation failed",
                    loader::LoadError::RelocationFailed => "Relocation failed",
                });
                bsod::show_bsod(
                    types::STATUS_INVALID_IMAGE_FORMAT,
                    "Failed to load ntoskrnl.exe PE image",
                    NTOSKRNL_PATH,
                );
            },
        }
    };

    println("");

    // 2.3: Читаем unicode.nls (NLS данные для Unicode)
    print("[3/3] Loading NLS data: ");
    println(UNICODE_NLS_PATH);

    let unicode_nls = match fs::read_file(UNICODE_NLS_PATH) {
        Some(file) => {
            print("       Size: ");
            print_num(file.data.len() as u64);
            println(" bytes");

            // Проверка размера
            if file.data.is_empty() {
                println("       Status: FAILED - file is empty");
                bsod::show_bsod(
                    types::STATUS_INVALID_IMAGE_FORMAT,
                    "{NLS Data Failure} - empty file",
                    UNICODE_NLS_PATH,
                );
            }

            if file.data.len() > MAX_UNICODE_NLS_SIZE {
                println("       Status: FAILED - file too large");
                bsod::show_bsod(
                    types::STATUS_INVALID_IMAGE_FORMAT,
                    "{NLS Data Failure} - file exceeds size limit",
                    UNICODE_NLS_PATH,
                );
            }

            // Базовая валидация XNLS magic (должен начинаться с "XNLS")
            if file.data.len() >= 4
                && file.data[0] == b'X'
                && file.data[1] == b'N'
                && file.data[2] == b'L'
                && file.data[3] == b'S'
            {
                println("       Format: XNLS (OK)");
            } else {
                println("       WARNING: Unknown NLS format (expected XNLS)");
            }

            println("       Status: OK");
            file
        },
        None => {
            println("       Status: FAILED - file not found");
            println("");
            println("       NOTE: unicode.nls must be generated using:");
            println("             make nls");
            bsod::show_bsod(
                types::STATUS_INVALID_IMAGE_FORMAT,
                "{NLS Data Missing}",
                UNICODE_NLS_PATH,
            );
        },
    };

    println("");
    println("[WINLOAD] Step 2 complete.");
    println("");

    // =========================================================================
    // Шаг 2.5: Загрузка зависимостей ядра (bootvid, hal, etc.)
    // =========================================================================
    println("========================================");
    println("  Step 2.5: Loading Kernel Dependencies");
    println("========================================");
    println("");

    // Создаём список загруженных модулей раньше - нужен для загрузки зависимостей
    let mut load_order_list = loader::LoadOrderList::new();

    // Сохраняем данные kernel_module перед перемещением в список
    let kernel_phys_base = kernel_module.phys_base;
    let kernel_size_of_image = kernel_module.size_of_image;
    let kernel_entry_rva = kernel_module.entry_rva;
    let kernel_va_base_saved = kernel_module.va_base;

    // Получаем зависимости ядра из его Import Directory
    let kernel_image = unsafe {
        core::slice::from_raw_parts(
            kernel_phys_base as *const u8,
            kernel_size_of_image as usize,
        )
    };
    let kernel_deps = loader::get_import_dependencies(kernel_image);

    // Добавляем ntoskrnl.exe первым в список
    load_order_list.add(kernel_module);

    // Вычисляем следующий доступный VA для зависимостей и драйверов
    let kernel_end_va = {
        let k = load_order_list.modules.first().unwrap();
        let end = k.va_base + k.size_of_image as u64;
        (end + 0xFFFF) & !0xFFFF
    };
    let mut next_module_va = kernel_end_va;

    // Загружаем каждую зависимость ядра
    print("Kernel imports ");
    print_num(kernel_deps.len() as u64);
    println(" DLL(s):");

    for dep_name in kernel_deps.iter() {
        // Пропускаем ntoskrnl (он уже загружен)
        if dep_name.eq_ignore_ascii_case("ntoskrnl") || dep_name.eq_ignore_ascii_case("NTOSKRNL") {
            continue;
        }

        // Проверяем не загружен ли уже
        if load_order_list.find_by_name(dep_name).is_some() {
            print("  [SKIP] ");
            print(dep_name);
            println(" (already loaded)");
            continue;
        }

        print("  [LOAD] ");
        print(dep_name);
        print("... ");

        // Поиск модуля в нескольких местах и с разными расширениями (как в Windows):
        // Места: \SystemRoot\system32\, \SystemRoot\system32\drivers\
        // Расширения: .dll, .sys
        let dll_name_lower = dep_name.to_lowercase();
        let search_paths = [
            // system32 - системные DLL (hal.dll, bootvid.dll)
            alloc::format!("\\XenOS\\system32\\{}.dll", dll_name_lower),
            alloc::format!("\\XenOS\\system32\\{}.sys", dll_name_lower),
            // system32\drivers - драйверы (.dll и .sys)
            alloc::format!("\\XenOS\\system32\\drivers\\{}.dll", dll_name_lower),
            alloc::format!("\\XenOS\\system32\\drivers\\{}.sys", dll_name_lower),
        ];

        let mut found = false;
        for dll_path_str in search_paths.iter() {
            let dll_path: &'static str = alloc::boxed::Box::leak(dll_path_str.clone().into_boxed_str());

            if let Some(file) = fs::read_file(dll_path) {
                // Извлекаем имя файла из пути (например, "bootvid.dll" из "\XenOS\system32\bootvid.dll")
                let file_name = dll_path_str
                    .rsplit('\\')
                    .next()
                    .unwrap_or(dll_path_str);

                // Загружаем PE образ
                match loader::load_pe_image(
                    &file.data,
                    file_name.to_string(),
                    dll_path_str.clone(),
                    next_module_va,
                ) {
                    Ok(module) => {
                        print("OK (");
                        print(dll_path);
                        println(")");

                        // Обновляем next VA
                        let module_end = module.va_base + module.size_of_image as u64;
                        next_module_va = (module_end + 0xFFFF) & !0xFFFF;

                        load_order_list.add(module);
                        found = true;
                        break;
                    }
                    Err(e) => {
                        print("FAILED at ");
                        print(dll_path);
                        print(" (");
                        print(match e {
                            loader::LoadError::FileTooSmall => "file too small",
                            loader::LoadError::InvalidDosSignature => "invalid DOS signature",
                            loader::LoadError::InvalidPeSignature => "invalid PE signature",
                            loader::LoadError::NotPe32Plus => "not PE32+",
                            loader::LoadError::UnsupportedMachine => "unsupported machine",
                            loader::LoadError::AllocationFailed => "allocation failed",
                            loader::LoadError::RelocationFailed => "relocation failed",
                        });
                        println(")");
                        found = true; // Файл найден, но не загрузился
                        break;
                    }
                }
            }
        }

        if !found {
            println("FAILED (not found in system32 or drivers)");
        }
    }

    // Сохраняем количество зависимостей ядра для правильной индексации boot drivers
    let kernel_deps_loaded = load_order_list.count() - 1; // -1 для ntoskrnl

    println("");
    print("Loaded ");
    print_num(kernel_deps_loaded as u64);
    println(" kernel dependency module(s)");
    println("");
    println("[WINLOAD] Step 2.5 complete.");
    println("");

    // =========================================================================
    // Шаг 3: Парсинг SYSTEM hive
    // =========================================================================
    println("========================================");
    println("  Step 3: Parsing SYSTEM Hive");
    println("========================================");
    println("");

    // Парсим hive с использованием библиотеки hive
    print("[1/3] Parsing hive structure...");
    let parsed_hive = match hive::Hive::new(&system_hive.data) {
        Ok(h) => {
            println(" OK");
            h
        },
        Err(_) => {
            println(" FAILED");
            bsod::show_bsod(
                STATUS_CANNOT_LOAD_REGISTRY_FILE,
                "{Registry File Failure} - Invalid hive structure",
                SYSTEM_HIVE_PATH,
            );
        },
    };

    // Определяем CurrentControlSet
    print("[2/3] Determining CurrentControlSet...");
    let current_cs = boot_drivers::get_current_control_set(&parsed_hive).unwrap_or(1);
    print(" ControlSet");
    print_num(current_cs as u64);
    println("");

    // Получаем список boot-start драйверов
    print("[3/3] Enumerating boot-start drivers...");
    let mut boot_drivers = boot_drivers::get_boot_drivers(&parsed_hive, current_cs);
    println("");

    // Сортируем по GroupOrderList + Tag
    boot_drivers::sort_boot_drivers(&parsed_hive, current_cs, &mut boot_drivers);

    print("       Found ");
    print_num(boot_drivers.len() as u64);
    println(" boot-start driver(s)");
    println("");

    // Выводим список драйверов
    if !boot_drivers.is_empty() {
        println("  Boot Driver List (sorted):");
        println("  ----------------------------");
        for (i, driver) in boot_drivers.iter().enumerate() {
            print("  ");
            print_num((i + 1) as u64);
            print(". ");
            print(&driver.name);

            if let Some(ref group) = driver.group {
                print(" [");
                print(group);
                print("]");
            }

            if driver.tag > 0 {
                print(" tag=");
                print_num(driver.tag as u64);
            }
            println("");

            if let Some(ref path) = driver.image_path {
                print("     Path: ");
                println(path);
            }
        }
        println("");
    }

    println("[WINLOAD] Step 3 complete.");
    println("");

    // =========================================================================
    // Шаг 4: Загрузка boot drivers
    // =========================================================================
    println("========================================");
    println("  Step 4: Loading Boot Drivers");
    println("========================================");
    println("");

    // load_order_list уже создан и содержит ntoskrnl + его зависимости (step 2.5)
    // Используем next_module_va для следующего драйвера
    let mut next_driver_va = next_module_va;

    // Загружаем каждый boot driver
    if boot_drivers.is_empty() {
        println("[INFO] No boot drivers to load (SYSTEM hive has no boot-start services)");
        println("");
    } else {
        for (i, driver) in boot_drivers.iter().enumerate() {
            print("[");
            print_num((i + 1) as u64);
            print("/");
            print_num(boot_drivers.len() as u64);
            print("] Loading: ");
            println(&driver.name);

            // Определяем путь к драйверу
            let image_path = match &driver.image_path {
                Some(path) => path.clone(),
                None => {
                    // По плану: требуем ImagePath, не используем дефолтный путь
                    println("       Status: SKIPPED - no ImagePath in registry");
                    println("");
                    continue;
                },
            };

            // Конвертируем путь из NT формата в UEFI
            // \SystemRoot\ -> \XenOS\
            let uefi_path = convert_nt_path_to_uefi(&image_path);

            print("       Path: ");
            println(&uefi_path);

            // Читаем файл драйвера
            // Используем leak для получения &'static str
            let path_leaked: &'static str =
                alloc::boxed::Box::leak(uefi_path.clone().into_boxed_str());

            match fs::read_file(path_leaked) {
                Some(file) => {
                    print("       Size: ");
                    print_num(file.data.len() as u64);
                    println(" bytes");

                    // Загружаем PE образ с VA = next_driver_va
                    match loader::load_pe_image(
                        &file.data,
                        driver.name.clone(),
                        uefi_path.clone(),
                        next_driver_va,
                    ) {
                        Ok(module) => {
                            print("       Phys base: 0x");
                            print_hex(module.phys_base);
                            println("");
                            print("       VA base:   0x");
                            print_hex(module.va_base);
                            println("");
                            print("       Entry VA:  0x");
                            print_hex(module.entry_point_va());
                            println("");
                            print("       Size:      ");
                            print_num(module.size_of_image as u64);
                            println(" bytes");
                            println("       Status: LOADED");

                            // Обновляем next_driver_va для следующего драйвера
                            let driver_end = module.va_base + module.size_of_image as u64;
                            next_driver_va = (driver_end + 0xFFFF) & !0xFFFF;

                            load_order_list.add(module);
                        },
                        Err(e) => {
                            print("       Status: FAILED - ");
                            match e {
                                loader::LoadError::FileTooSmall => println("file too small"),
                                loader::LoadError::InvalidDosSignature => {
                                    println("invalid DOS signature")
                                },
                                loader::LoadError::InvalidPeSignature => {
                                    println("invalid PE signature")
                                },
                                loader::LoadError::NotPe32Plus => println("not PE32+"),
                                loader::LoadError::UnsupportedMachine => {
                                    println("unsupported machine type")
                                },
                                loader::LoadError::AllocationFailed => println("allocation failed"),
                                loader::LoadError::RelocationFailed => println("relocation failed"),
                            }

                            // Проверяем ErrorControl
                            if driver.error_control >= 2 {
                                // ErrorCritical или ErrorSevere - BSOD
                                bsod::show_bsod(
                                    types::STATUS_INVALID_IMAGE_FORMAT,
                                    "{Bad System Driver Image}",
                                    &uefi_path,
                                );
                            }
                        },
                    }
                },
                None => {
                    println("       Status: FILE NOT FOUND");

                    // Проверяем ErrorControl
                    if driver.error_control >= 2 {
                        // ErrorCritical или ErrorSevere - BSOD
                        bsod::show_bsod(
                            types::STATUS_INVALID_IMAGE_FORMAT,
                            "{Boot Driver Missing}",
                            &uefi_path,
                        );
                    }
                },
            }
            println("");
        }
    }

    println("[WINLOAD] Step 4 complete.");
    print("          Loaded ");
    // -1 потому что ntoskrnl тоже в списке
    print_num((load_order_list.count() - 1) as u64);
    print(" of ");
    print_num(boot_drivers.len() as u64);
    println(" boot driver(s)");
    println("");

    // =========================================================================
    // Шаг 4.5: Резолв импортов (IAT fixups)
    // =========================================================================
    println("========================================");
    println("  Step 4.5: Resolving Imports (IAT)");
    println("========================================");
    println("");

    // Резолвим импорты для всех модулей (включая ntoskrnl если он импортирует)
    let driver_count = load_order_list.count();
    if driver_count > 0 {
        // Начинаем с 0 - разрешаем импорты ядра (если есть)
        for i in 0..driver_count {
            let module = &load_order_list.modules[i];
            
            // Для ядра показываем специальное сообщение
            if i == 0 {
                print("[KERNEL] Resolving imports for: ");
            } else {
                print("[");
                print_num(i as u64);
                print("/");
                print_num((driver_count - 1) as u64);
                print("] Resolving imports for: ");
            }
            println(&module.name);

            // Получаем mutable slice образа
            let image = unsafe {
                core::slice::from_raw_parts_mut(
                    module.phys_base as *mut u8,
                    module.size_of_image as usize,
                )
            };

            match loader::resolve_imports(module, image, &load_order_list) {
                Ok(()) => {
                    println("       Status: OK");
                },
                Err(e) => {
                    print("       Status: FAILED - ");
                    match e {
                        loader::ImportError::ModuleNotFound(name) => {
                            print("module not found: ");
                            println(&name);
                        },
                        loader::ImportError::SymbolNotFound(name) => {
                            print("symbol not found: ");
                            println(&name);
                        },
                        loader::ImportError::InvalidExportDirectory => {
                            println("invalid export directory");
                        },
                        loader::ImportError::InvalidImportDirectory => {
                            println("invalid import directory");
                        },
                    }

                    // Import resolution failure = BSOD
                    bsod::show_bsod(
                        0xC0000263, // STATUS_DRIVER_ENTRYPOINT_NOT_FOUND
                        "{Driver Import Resolution Failed}",
                        &module.name,
                    );
                },
            }
            println("");
        }
    } else {
        println("[INFO] No boot drivers to resolve imports for");
        println("");
    }

    println("[WINLOAD] Step 4.5 complete.");
    println("");

    // =========================================================================
    // Шаг 5: Подготовка loader structures + ExitBootServices
    // =========================================================================
    println("========================================");
    println("  Step 5: Preparing Loader Structures");
    println("========================================");
    println("");

    // 5.1: Получаем текущую memory map и статистику
    print("[1/4] Getting UEFI memory map...");
    let (uefi_map, map_info) = match memory::get_uefi_memory_map() {
        Some((map, info)) => {
            println(" OK");
            print("       Entries: ");
            print_num(info.entry_count as u64);
            println("");
            (map, info)
        },
        None => {
            println(" FAILED");
            bsod::show_bsod(
                0xC0000017, // STATUS_NO_MEMORY
                "{Memory Map Failure}",
                "UEFI GetMemoryMap",
            );
        },
    };

    // Статистика памяти
    let mem_stats = memory::MemoryStats::from_uefi_map(&uefi_map);
    print("       Total:   ");
    print_num(mem_stats.total_bytes() / 1024 / 1024);
    println(" MB");
    print("       Free:    ");
    print_num(mem_stats.free_bytes() / 1024 / 1024);
    println(" MB");
    println("");

    // 5.2: Конвертируем в NT формат
    print("[2/4] Converting to NT memory descriptors...");
    let mut nt_memory_list = memory::convert_uefi_memory_map(&uefi_map);
    print(" ");
    print_num(nt_memory_list.count() as u64);
    println(" descriptors");
    println("");

    // 5.3: Создаём LOADER_PARAMETER_BLOCK (используем ntldr crate)
    print("[3/4] Building LOADER_PARAMETER_BLOCK...");

    // Создаём loader block с NT-совместимой структурой
    let mut loader_block = alloc::boxed::Box::new(ntldr::LOADER_PARAMETER_BLOCK::empty());
    let loader_extension = alloc::boxed::Box::new(ntldr::LOADER_PARAMETER_EXTENSION::empty());

    // Инициализируем списки
    loader_block.MemoryDescriptorListHead.init_head();
    loader_block.LoadOrderListHead.init_head();
    loader_block.BootDriverListHead.init_head();

    // Заполняем framebuffer info
    if let Some(ref fb) = ctx.framebuffer {
        loader_block.FramebufferInfo = ntldr::LOADER_FRAMEBUFFER_INFO {
            Present: 1,
            Reserved: [0; 7],
            FrameBufferBase: fb.base,
            FrameBufferSize: fb.size as u64,
            HorizontalResolution: fb.width as u32,
            VerticalResolution: fb.height as u32,
            PixelsPerScanLine: fb.stride as u32,
            BitsPerPixel: 32,
        };
    }

    // Kernel size (base и entry будут установлены после маппинга в VA)
    loader_block.KernelSize = kernel_size_of_image;

    // Registry
    loader_block.RegistryBase = system_hive.data.as_ptr() as *mut u8;
    loader_block.RegistryLength = system_hive.data.len() as u32;

    // NLS (unicode.nls)
    // Данные уже в памяти загрузчика, передаём указатель и размер.
    // Ядро скопирует данные в kernel-owned память после парсинга XNLS.
    loader_block.UnicodeNlsBase = unicode_nls.data.as_ptr();
    loader_block.UnicodeNlsSize = unicode_nls.data.len() as u64;

    // Load Options (командная строка загрузки)
    // Передаём указатель на буфер в BootContext (который живёт до конца загрузки).
    // ВАЖНО: ctx должен оставаться живым до передачи в ядро!
    if ctx.load_options_length > 0 {
        loader_block.LoadOptions = ctx.load_options_buffer.as_ptr();
        loader_block.LoadOptionsLength = ctx.load_options_length as u32;
    }

    // Ищем подходящий регион памяти для pool
    for desc in nt_memory_list.descriptors.iter() {
        if desc.MemoryType == ntcompat::MEMORY_TYPE::LoaderFree {
            let size = desc.PageCount * 4096;
            if size >= 16 * 1024 * 1024 && loader_block.PoolBase == 0 {
                loader_block.PoolBase = desc.BasePage * 4096;
                loader_block.PoolSize = core::cmp::min(size, 64 * 1024 * 1024);
            }
        }
    }

    // ACPI RSDP
    if let Some(rsdp) = ctx.acpi_rsdp {
        loader_block.AcpiTablePhysical = rsdp;
    }

    // Связываем memory descriptors в список LoaderBlock
    // ВАЖНО: дескрипторы должны оставаться живыми до передачи в ядро!
    // Safety: ntldr::LIST_ENTRY и ntcompat::LIST_ENTRY идентичны по layout (#[repr(C)])
    unsafe {
        let head_ptr = &mut loader_block.MemoryDescriptorListHead as *mut ntldr::LIST_ENTRY;
        let head_compat = head_ptr as *mut ntcompat::LIST_ENTRY;
        nt_memory_list.link_to_head(&mut *head_compat);
    }

    // Связываем extension
    loader_block.Extension = alloc::boxed::Box::into_raw(loader_extension);

    // =========================================================================
    // 5.3.1: Формируем LoadOrderListHead и BootDriverListHead
    // =========================================================================
    // ВАЖНО: entries должны оставаться живыми до передачи в ядро!
    // Используем Vec для хранения Box'ов, потом leak'аем.

    // Создаём LOADER_MODULE_ENTRY для каждого загруженного модуля
    let mut module_entries: alloc::vec::Vec<alloc::boxed::Box<ntldr::LOADER_MODULE_ENTRY>> =
        alloc::vec::Vec::with_capacity(load_order_list.count());

    for module in load_order_list.modules.iter() {
        let entry = loader::create_module_entry(module);
        module_entries.push(entry);
    }

    // Связываем module entries в LoadOrderListHead
    for entry in module_entries.iter_mut() {
        let entry_ptr = entry.as_mut() as *mut ntldr::LOADER_MODULE_ENTRY;
        unsafe {
            loader::list_insert_tail(
                &mut loader_block.LoadOrderListHead,
                &mut (*entry_ptr).InLoadOrderLinks,
            );
        }
    }

    // Создаём BOOT_DRIVER_LIST_ENTRY для boot drivers
    let mut boot_driver_entries: alloc::vec::Vec<alloc::boxed::Box<ntldr::BOOT_DRIVER_LIST_ENTRY>> =
        alloc::vec::Vec::with_capacity(boot_drivers.len());

    // Нужен mapping между boot_drivers и module_entries
    // load_order_list содержит: ntoskrnl (0), зависимости ядра (1..N), boot drivers (N+1..)
    // kernel_deps_loaded - количество загруженных зависимостей ядра (сохранено в Step 2.5)
    let first_boot_driver_idx = 1 + kernel_deps_loaded; // ntoskrnl + зависимости
    
    for (i, driver) in boot_drivers.iter().enumerate() {
        let module_idx = first_boot_driver_idx + i;
        if module_idx >= module_entries.len() {
            break; // Драйвер не был загружен
        }

        let module_entry_ptr =
            module_entries[module_idx].as_mut() as *mut ntldr::LOADER_MODULE_ENTRY;

        let uefi_path = convert_nt_path_to_uefi(driver.image_path.as_deref().unwrap_or(""));
        let entry = loader::create_boot_driver_entry(
            &driver.name,
            &uefi_path,
            module_entry_ptr,
            current_cs,
        );
        boot_driver_entries.push(entry);
    }

    // Связываем boot driver entries в BootDriverListHead
    for entry in boot_driver_entries.iter_mut() {
        let entry_ptr = entry.as_mut() as *mut ntldr::BOOT_DRIVER_LIST_ENTRY;
        unsafe {
            loader::list_insert_tail(
                &mut loader_block.BootDriverListHead,
                &mut (*entry_ptr).Link,
            );
        }
    }

    // Leak entries чтобы они жили до передачи в ядро
    // (Vec и Box'ы будут освобождены при завершении функции,
    // но нам нужно чтобы данные остались в памяти)
    let _module_entries_leaked: alloc::vec::Vec<*mut ntldr::LOADER_MODULE_ENTRY> = module_entries
        .into_iter()
        .map(|b| alloc::boxed::Box::into_raw(b))
        .collect();
    let _boot_driver_entries_leaked: alloc::vec::Vec<*mut ntldr::BOOT_DRIVER_LIST_ENTRY> =
        boot_driver_entries
            .into_iter()
            .map(|b| alloc::boxed::Box::into_raw(b))
            .collect();

    println(" OK");
    print("       Block size:     ");
    print_num(core::mem::size_of::<ntldr::LOADER_PARAMETER_BLOCK>() as u64);
    println(" bytes");
    print("       Extension size: ");
    print_num(core::mem::size_of::<ntldr::LOADER_PARAMETER_EXTENSION>() as u64);
    println(" bytes");
    print("       Registry:       0x");
    print_hex(loader_block.RegistryBase as u64);
    print(" (");
    print_num(loader_block.RegistryLength as u64);
    println(" bytes)");
    print("       NLS data:       0x");
    print_hex(loader_block.UnicodeNlsBase as u64);
    print(" (");
    print_num(loader_block.UnicodeNlsSize);
    println(" bytes)");
    if loader_block.LoadOptionsLength > 0 {
        print("       LoadOptions:    0x");
        print_hex(loader_block.LoadOptions as u64);
        print(" (");
        print_num(loader_block.LoadOptionsLength as u64);
        println(" bytes)");
    }
    if loader_block.FramebufferInfo.Present != 0 {
        print("       Framebuffer:    0x");
        print_hex(loader_block.FramebufferInfo.FrameBufferBase);
        print(" (");
        print_num(loader_block.FramebufferInfo.HorizontalResolution as u64);
        print("x");
        print_num(loader_block.FramebufferInfo.VerticalResolution as u64);
        println(")");
    }
    if loader_block.PoolBase != 0 {
        print("       Pool:           0x");
        print_hex(loader_block.PoolBase);
        print(" (");
        print_num(loader_block.PoolSize / 1024 / 1024);
        println(" MB)");
    }
    print("       Kernel size:    ");
    print_num(loader_block.KernelSize as u64);
    println(" bytes");
    if loader_block.AcpiTablePhysical != 0 {
        print("       ACPI RSDP:      0x");
        print_hex(loader_block.AcpiTablePhysical);
        println("");
    }

    // Устанавливаем HHDM offset
    loader_block.HhdmOffset = paging::HHDM_OFFSET;
    print("       HHDM:           0x");
    print_hex(loader_block.HhdmOffset);
    println("");
    print("       MemoryDesc:     ");
    print_num(nt_memory_list.count() as u64);
    println(" descriptors linked");
    println("");

    // 5.4: Настраиваем page tables для HHDM и kernel mapping
    print("[4/6] Setting up page tables...");

    // Вычисляем максимальный физический адрес для HHDM
    let max_phys = mem_stats.max_physical_address();

    // Создаём и настраиваем page tables
    // ВАЖНО: делаем это до ExitBootServices пока работает аллокатор
    let mut page_tables = paging::PageTables::new();
    page_tables.setup(max_phys);

    println(" OK");
    print("       Max physical:   0x");
    print_hex(max_phys);
    println("");
    println("");

    // 5.5: Маппируем ядро в higher half (как Windows NT)
    print("[5/6] Mapping kernel to VA...");

    // Физический адрес и размер загруженного ядра (сохранены ранее)
    let kernel_size = kernel_size_of_image as u64;

    // Создаём маппинг: KERNEL_VA_BASE -> kernel_phys_base
    let kernel_va_base = page_tables.map_kernel(kernel_phys_base, kernel_size);

    // Вычисляем виртуальный entry point
    // Entry VA = VA base + entry RVA
    let kernel_entry_va = kernel_va_base_saved + kernel_entry_rva as u64;

    // Обновляем LoaderBlock с виртуальными адресами
    loader_block.KernelBase = kernel_va_base;
    loader_block.KernelEntry = kernel_entry_va;

    // Маппируем boot drivers в higher half VA
    // (модули начиная с индекса 1, т.к. 0 = ntoskrnl который уже замаплен)
    for module in load_order_list.modules.iter().skip(1) {
        page_tables.map_module(module.phys_base, module.va_base, module.size_of_image as u64);
    }

    let new_cr3 = page_tables.cr3_value();

    println(" OK");
    print("       Phys base:      0x");
    print_hex(kernel_phys_base);
    println("");
    print("       VA base:        0x");
    print_hex(kernel_va_base);
    println("");
    print("       VA entry:       0x");
    print_hex(kernel_entry_va);
    println("");
    print("       Boot drivers:   ");
    print_num((load_order_list.count() - 1) as u64);
    println(" mapped to VA");
    print("       New CR3:        0x");
    print_hex(new_cr3);
    println("");
    println("");

    // 5.6: ExitBootServices
    print("[6/6] Calling ExitBootServices...");

    // ВАЖНО: После ExitBootServices мы теряем доступ к:
    // - UEFI консоли (stdout)
    // - UEFI файловой системе
    // - Аллокатору через Boot Services
    //
    // Вывод после этого возможен только через:
    // - Прямую запись в framebuffer
    // - Serial port (если настроен)

    match memory::exit_boot_services() {
        Ok((final_mem_list, final_info)) => {
            // После ExitBootServices консоль недоступна!
            // UEFI Boot Services больше не работают.

            // Обновляем информацию в loader block
            let _ = final_mem_list;
            let _ = final_info;

            // =========================================================================
            // POST-EBS: Настраиваем HHDM и передаём управление ядру!
            // =========================================================================

            // Переключаем CR3 на новые page tables с HHDM
            // ВАЖНО: identity mapping должен покрывать текущий код!
            unsafe {
                paging::switch_cr3(new_cr3);
            }

            // Получаем raw pointer на loader_block
            let loader_block_ptr = alloc::boxed::Box::into_raw(loader_block);

            // Entry point ядра: KiSystemStartup(PLOADER_PARAMETER_BLOCK) -> !
            // Используем виртуальный адрес entry point (kernel_entry_va)
            // Приводим к типу функции с Windows x64 ABI
            type KiSystemStartupFn =
                unsafe extern "win64" fn(*const ntldr::LOADER_PARAMETER_BLOCK) -> !;

            let ki_system_startup: KiSystemStartupFn =
                unsafe { core::mem::transmute(kernel_entry_va as *const ()) };

            // Вызываем ядро!
            // Это точка невозврата - управление больше не вернётся в winload
            unsafe {
                ki_system_startup(loader_block_ptr);
            }

            // Сюда мы никогда не попадём
        },
        Err(err_msg) => {
            // ExitBootServices failed - консоль ещё доступна
            println(" FAILED");
            print("       Error: ");
            println(err_msg);
            println("");

            // Продолжаем работу без EBS для отладки
            println("[WARNING] ExitBootServices failed, continuing without it");
            println("          (This is expected in some UEFI environments)");
            println("");
        },
    }

    // =========================================================================
    // Итог (если ExitBootServices не удался)
    // =========================================================================
    println("========================================");
    println("  Summary");
    println("========================================");
    print("  SYSTEM hive:       ");
    print_num(system_hive.data.len() as u64);
    println(" bytes");
    print("  CurrentControlSet: ");
    print_num(current_cs as u64);
    println("");
    print("  Boot drivers:      ");
    print_num(boot_drivers.len() as u64);
    print(" registered, ");
    print_num(load_order_list.count() as u64);
    println(" loaded");
    print("  ntoskrnl:          ");
    print_num(ntoskrnl.data.len() as u64);
    if pe_info.valid && pe_info.is_pe32_plus {
        println(" bytes (PE32+ AMD64)");
    } else {
        println(" bytes (ELF - will be PE later)");
    }
    print("  Memory:            ");
    print_num(mem_stats.total_bytes() / 1024 / 1024);
    print(" MB total, ");
    print_num(mem_stats.free_bytes() / 1024 / 1024);
    println(" MB free");
    print("  NT descriptors:    ");
    print_num(nt_memory_list.count() as u64);
    println("");
    println("");

    // Выводим LoadOrderList если есть загруженные модули
    if load_order_list.count() > 0 {
        println("  LoadOrderListHead:");
        println("  -------------------");
        for (i, module) in load_order_list.modules.iter().enumerate() {
            print("  ");
            print_num((i + 1) as u64);
            print(". ");
            print(&module.name);
            print(" @ 0x");
            print_hex(module.phys_base);
            println("");
        }
        println("");
    }

    println("[WINLOAD] Steps 1-5 complete. Halting.");
    println("          (Next: Step 6 - adapt ntoskrnl + call KiSystemStartup)");

    loop {
        boot::stall(Duration::from_millis(500));
    }
}

/// Конвертирует NT-style путь в UEFI-style путь.
/// \SystemRoot\System32\drivers\foo.sys -> \XenOS\system32\drivers\foo.sys
fn convert_nt_path_to_uefi(nt_path: &str) -> String {
    let mut path = nt_path.to_string();

    // Заменяем \SystemRoot\ на \XenOS\
    if path.starts_with("\\SystemRoot\\") {
        path = path.replacen("\\SystemRoot\\", "\\XenOS\\", 1);
    } else if path.starts_with("system32\\") {
        // Относительный путь
        path = String::from("\\XenOS\\") + &path;
    }

    // Приводим к нижнему регистру для путей (UEFI FAT case-insensitive)
    path
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println("");
    println("!!! PANIC in winload.efi !!!");
    // Пытаемся вывести location если есть
    if let Some(location) = info.location() {
        print("  File: ");
        println(location.file());
        print("  Line: ");
        print_num(location.line() as u64);
        println("");
    }

    loop {
        core::hint::spin_loop();
    }
}
