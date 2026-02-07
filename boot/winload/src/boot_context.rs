//! Сбор UEFI Boot Context
//!
//! Получение информации из UEFI Boot Services:
//! - Load Options (cmdline)
//! - GOP (Graphics Output Protocol)
//! - Memory Map
//! - File System
//! - ACPI RSDP

use uefi::boot;
use uefi::mem::memory_map::MemoryMap;
use uefi::proto::console::gop::GraphicsOutput;
use uefi::proto::loaded_image::LoadedImage;
use uefi::table::cfg;

use crate::console::print;
use crate::console::print_hex;
use crate::console::print_num;
use crate::console::println;
use crate::types::BootContext;
use crate::types::FramebufferInfo;
use crate::types::MAX_LOAD_OPTIONS_LENGTH;

/// Результат получения Load Options.
struct LoadOptionsResult {
    /// Буфер с ASCII строкой (null-terminated).
    buffer: [u8; MAX_LOAD_OPTIONS_LENGTH],
    /// Длина строки (без null-терминатора).
    length: usize,
}

/// Получение Load Options (cmdline) и конвертация UTF-16 → ASCII.
fn get_load_options() -> LoadOptionsResult {
    let mut result = LoadOptionsResult {
        buffer: [0u8; MAX_LOAD_OPTIONS_LENGTH],
        length: 0,
    };

    let image_handle = boot::image_handle();

    // Открываем LoadedImage protocol
    let loaded_image = match boot::open_protocol_exclusive::<LoadedImage>(image_handle) {
        Ok(li) => li,
        Err(_) => {
            println("  Load Options: (protocol error)");
            return result;
        },
    };

    // Получаем load options как raw bytes
    // Используем load_options_as_bytes вместо load_options_as_cstr16,
    // т.к. некоторые загрузчики (limine) не включают null в LoadOptionsSize
    let raw_bytes = match loaded_image.load_options_as_bytes() {
        Some(bytes) if !bytes.is_empty() => bytes,
        _ => {
            println("  Load Options: (none)");
            return result;
        },
    };

    // Печатаем размер
    print("  Load Options size: ");
    print_num(raw_bytes.len() as u64);
    println(" bytes");

    // Интерпретируем как UTF-16LE и конвертируем в ASCII
    // raw_bytes содержит UTF-16LE данные (2 байта на символ)
    let u16_len = raw_bytes.len() / 2;
    if u16_len == 0 {
        println("  Load Options: (empty)");
        return result;
    }

    // Печатаем оригинал через UEFI stdout
    print("  Load Options (UTF-16): ");
    // Выводим посимвольно
    for i in 0..u16_len {
        let lo = raw_bytes[i * 2] as u16;
        let hi = raw_bytes.get(i * 2 + 1).copied().unwrap_or(0) as u16;
        let ch16 = lo | (hi << 8);
        if ch16 == 0 {
            break;
        }
        // Печатаем как UEFI символ
        let buf: [u16; 2] = [ch16, 0];
        let cstr = unsafe { uefi::CStr16::from_u16_with_nul_unchecked(&buf) };
    uefi::system::with_stdout(|stdout| {
            let _ = stdout.output_string(cstr);
    });
    }
    println("");

    // Конвертируем UTF-16 → ASCII (только ASCII-подмножество)
    // Это безопасно для ключей типа "/DEBUG", "/SOS"
    let mut out_idx = 0;
    for i in 0..u16_len {
        let lo = raw_bytes[i * 2] as u16;
        let hi = raw_bytes.get(i * 2 + 1).copied().unwrap_or(0) as u16;
        let ch16 = lo | (hi << 8);

        if ch16 == 0 {
            break; // null-terminator
        }
        if out_idx >= MAX_LOAD_OPTIONS_LENGTH - 1 {
            break; // оставляем место для null
        }
        // ASCII: только 0x00-0x7F
        if ch16 <= 0x7F {
            result.buffer[out_idx] = ch16 as u8;
            out_idx += 1;
        } else {
            // Non-ASCII символ — заменяем на '?'
            result.buffer[out_idx] = b'?';
            out_idx += 1;
        }
    }
    result.buffer[out_idx] = 0; // null-terminate
    result.length = out_idx;

    // Печатаем сконвертированный результат
    print("  Load Options (ASCII): ");
    if result.length > 0 {
        // Печатаем посимвольно
        for i in 0..result.length {
            let ch = result.buffer[i];
            if ch >= 0x20 && ch < 0x7F {
                print_char(ch);
            }
        }
    } else {
        print("(empty)");
    }
    println("");

    result
}

/// Печать одного ASCII символа.
fn print_char(ch: u8) {
    // Используем UEFI stdout напрямую
    let buf: [u16; 2] = [ch as u16, 0];
    let cstr = unsafe { uefi::CStr16::from_u16_with_nul_unchecked(&buf) };
    uefi::system::with_stdout(|stdout| {
        let _ = stdout.output_string(cstr);
    });
}

/// Получение параметров GOP (framebuffer).
fn get_gop_info() -> Option<FramebufferInfo> {
    // Ищем handle с GOP protocol
    let gop_handle = boot::get_handle_for_protocol::<GraphicsOutput>().ok()?;

    // Открываем GOP protocol с GetProtocol (не exclusive!)
    // Exclusive блокирует консольный вывод firmware.
    let gop = unsafe {
        boot::open_protocol::<GraphicsOutput>(
            boot::OpenProtocolParams {
                handle: gop_handle,
                agent: boot::image_handle(),
                controller: None,
            },
            boot::OpenProtocolAttributes::GetProtocol,
        )
        .ok()?
    };

    let mode_info = gop.current_mode_info();
    let (width, height) = mode_info.resolution();
    let stride = mode_info.stride();

    let mut gop = gop;
    let mut fb = gop.frame_buffer();
    let base = fb.as_mut_ptr() as u64;
    let size = fb.size();

    Some(FramebufferInfo {
        base,
        size,
        width,
        height,
        stride,
    })
}

/// Получение количества записей в UEFI Memory Map.
fn get_memory_map_info() -> usize {
    // Получаем memory map (выделяет память через UEFI allocator)
    match boot::memory_map(boot::MemoryType::LOADER_DATA) {
        Ok(map) => map.entries().count(),
        Err(_) => 0,
    }
}

/// Проверка доступности файловой системы.
fn check_filesystem() -> bool {
    let image_handle = boot::image_handle();

    // Пытаемся получить файловую систему образа
    match boot::get_image_file_system(image_handle) {
        Ok(_fs) => true,
        Err(_) => false,
    }
}

/// Получение физического адреса ACPI RSDP из UEFI Configuration Table.
fn get_acpi_rsdp() -> Option<u64> {
    // Получаем System Table
    let st = uefi::system::with_config_table(|config_tables| {
        // Ищем ACPI 2.0 RSDP (предпочтительно)
        for entry in config_tables {
            if entry.guid == cfg::ACPI2_GUID {
                return Some(entry.address as u64);
            }
        }
        // Fallback на ACPI 1.0
        for entry in config_tables {
            if entry.guid == cfg::ACPI_GUID {
                return Some(entry.address as u64);
            }
        }
        None
    });
    st
}

/// Собираем весь boot context.
pub fn collect_boot_context() -> BootContext {
    println("[WINLOAD] Collecting UEFI boot context...");
    println("");

    // 1. Load Options
    println("[1/5] Reading Load Options...");
    let load_opts = get_load_options();

    // 2. GOP / Framebuffer
    println("[2/5] Querying GOP (Graphics Output Protocol)...");
    let framebuffer = get_gop_info();
    if let Some(ref fb) = framebuffer {
        print("  Framebuffer: ");
        print_hex(fb.base);
        println("");
        print("  Resolution:  ");
        print_num(fb.width as u64);
        print("x");
        print_num(fb.height as u64);
        println("");
        print("  Stride:      ");
        print_num(fb.stride as u64);
        println(" pixels/row");
        print("  Size:        ");
        print_num(fb.size as u64);
        println(" bytes");
    } else {
        println("  GOP not available");
    }

    // 3. Memory Map
    println("[3/5] Querying UEFI Memory Map...");
    let memory_map_entries = get_memory_map_info();
    print("  Entries:     ");
    print_num(memory_map_entries as u64);
    println("");

    // 4. ACPI RSDP
    println("[4/5] Looking for ACPI RSDP...");
    let acpi_rsdp = get_acpi_rsdp();
    if let Some(rsdp) = acpi_rsdp {
        print("  RSDP:        ");
        print_hex(rsdp);
        println("");
    } else {
        println("  RSDP not found (ACPI unavailable)");
    }

    // 5. File System
    println("[5/5] Checking File System access...");
    let fs_available = check_filesystem();
    if fs_available {
        println("  ESP filesystem: OK");
    } else {
        println("  ESP filesystem: FAILED");
    }

    println("");
    println("[WINLOAD] Boot context collected successfully.");

    BootContext {
        load_options_buffer: load_opts.buffer,
        load_options_length: load_opts.length,
        framebuffer,
        memory_map_entries,
        fs_available,
        acpi_rsdp,
    }
}
