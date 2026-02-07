//! Запуск QEMU

use anyhow::{Result, Context};
use std::process::Command;
use crate::config::Config;
use crate::util::Paths;
use crate::cli::{RunArgs, TestArgs};

/// Порты для GDB
const GDB_PORT: u16 = 1234;

/// Запуск QEMU в обычном режиме
pub fn run(paths: &Paths, config: &Config, args: &RunArgs) -> Result<()> {
    let mut qemu_args = build_base_args(paths, config);

    // QEMU debug flags если указаны
    if let Some(ref debug) = args.qemu_debug {
        qemu_args.extend(["-d".into(), debug.clone()]);
        qemu_args.extend(["-D".into(), "qemu.log".into()]);
    }

    run_qemu(&qemu_args)
}

/// Запуск QEMU в режиме GDB отладки
pub fn run_gdb(paths: &Paths, config: &Config, args: &RunArgs) -> Result<()> {
    log::info!("=== GDB Debug Mode ===");
    log::info!("    gdbstub:  tcp://127.0.0.1:{}", GDB_PORT);
    log::info!("");
    log::info!("Kernel stopped (-S). Connect GDB to continue.");

    let mut qemu_args = build_base_args(paths, config);

    // GDB stub
    qemu_args.extend([
        "-gdb".into(),
        format!("tcp::{}", GDB_PORT),
        "-S".into(), // Остановить на старте
    ]);

    // QEMU debug flags если указаны
    if let Some(ref debug) = args.qemu_debug {
        qemu_args.extend(["-d".into(), debug.clone()]);
        qemu_args.extend(["-D".into(), "qemu.log".into()]);
    }

    run_qemu(&qemu_args)
}

/// Запуск QEMU в тестовом режиме
pub fn run_test(paths: &Paths, config: &Config, _args: &TestArgs) -> Result<()> {
    let mut qemu_args = build_base_args(paths, config);

    // Тестовый режим: без дисплея, serial в файл, debug exit device
    qemu_args.extend([
        "-display".into(), "none".into(),
        "-serial".into(), "file:serial.txt".into(),
        "-device".into(), "isa-debug-exit,iobase=0xf4,iosize=0x04".into(),
    ]);

    let status = Command::new("qemu-system-x86_64")
        .args(&qemu_args)
        .status()
        .context("Failed to run QEMU")?;

    let exit_code = status.code().unwrap_or(-1);

    match exit_code {
        33 => {
            log::info!("=== All tests passed ===");
            Ok(())
        }
        35 => {
            log::error!("=== TESTS FAILED ===");
            anyhow::bail!("Tests failed");
        }
        _ => {
            log::error!("=== Unexpected exit code: {} ===", exit_code);
            anyhow::bail!("Unexpected QEMU exit code: {}", exit_code);
        }
    }
}

/// Формирует базовые аргументы QEMU из конфигурации
fn build_base_args(paths: &Paths, config: &Config) -> Vec<String> {
    let mut args = vec![];

    // Базовые аргументы
    args.push("-nodefaults".into());

    // Machine type
    let machine = config.get_string("QEMU_MACHINE");
    args.extend(["-M".into(), format!("type={}", machine)]);

    // CPU
    let cpu = config.get_string("QEMU_CPU");
    args.extend(["-cpu".into(), cpu]);

    // Accelerator (skip if "none")
    let accel = config.get_string("QEMU_ACCEL");
    if accel != "none" {
        args.extend(["-accel".into(), accel]);
    }

    // Memory
    let memory = config.get_string("QEMU_MEMORY");
    args.extend(["-m".into(), memory]);

    // SMP
    let smp = config.get_int("QEMU_SMP");
    args.extend(["-smp".into(), smp.to_string()]);

    // VGA
    let vga = config.get_string("QEMU_VGA");
    args.extend(["-vga".into(), vga]);

    // OVMF
    let ovmf_code = paths.ovmf_dir.join("OVMF_CODE.fd");
    let ovmf_vars = paths.ovmf_dir.join("OVMF_VARS.fd");
    args.extend([
        "-drive".into(),
        format!("if=pflash,format=raw,readonly=on,file={}", ovmf_code.display()),
        "-drive".into(),
        format!("if=pflash,format=raw,snapshot=on,file={}", ovmf_vars.display()),
    ]);

    // Disk images
    let disk_img = paths.build_dir.join("disk.img");
    args.extend([
        "-drive".into(),
        format!("format=raw,if=none,id=disk0,file={}", disk_img.display()),
        "-device".into(), "ahci,id=ahci".into(),
        "-device".into(), "ide-hd,drive=disk0,bus=ahci.0,bootindex=0".into(),
    ]);
    
    // Второй диск (если существует)
    let disk2_img = paths.workspace_root.join("disk.img");
    if disk2_img.exists() {
        args.extend([
            "-drive".into(),
            format!("format=raw,if=none,id=disk1,file={}", disk2_img.display()),
            "-device".into(), "ide-hd,drive=disk1,bus=ahci.1".into(),
        ]);
    }

    // Monitor
    let monitor = config.get_string("QEMU_MONITOR");
    match monitor.as_str() {
        "vc" => args.extend(["-monitor".into(), "vc:1920x1080".into()]),
        "stdio" => args.extend(["-monitor".into(), "stdio".into()]),
        _ => {}
    }

    // Serial
    let serial = config.get_string("QEMU_SERIAL");
    match serial.as_str() {
        "stdio" => args.extend(["-serial".into(), "stdio".into()]),
        "file" => {
            let file = config.get_string("QEMU_SERIAL_FILE");
            args.extend(["-serial".into(), format!("file:{}", file)]);
        }
        _ => {}
    }

    // No reboot/shutdown
    args.extend(["-no-reboot".into(), "-no-shutdown".into()]);

    // Debugcon для отладочных выводов таймеров (порт 0xe9)
    args.extend(["-debugcon".into(), "file:debugcon.log".into()]);
    args.extend(["-global".into(), "isa-debugcon.iobase=0xe9".into()]);

    args
}

/// Запускает QEMU с указанными аргументами
fn run_qemu(args: &[String]) -> Result<()> {
    log::debug!("QEMU args: {:?}", args);

    let status = Command::new("qemu-system-x86_64")
        .args(args)
        .status()
        .context("Failed to run QEMU")?;

    if !status.success() {
        anyhow::bail!("QEMU exited with status: {}", status);
    }

    Ok(())
}

/// Запускает QEMU с таймаутом (для отладки)
fn run_qemu_with_timeout(args: &[String], timeout_secs: u64) -> Result<()> {
    use std::time::Duration;
    use std::process::Stdio;

    log::info!("Starting QEMU with {} second timeout...", timeout_secs);
    log::debug!("QEMU args: {:?}", args);

    let mut child = Command::new("qemu-system-x86_64")
        .args(args)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .context("Failed to spawn QEMU")?;

    // Ждем с таймаутом
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                log::info!("QEMU exited with status: {}", status);
                return Ok(());
            }
            Ok(None) => {
                // Процесс еще работает
                if start.elapsed().as_secs() >= timeout_secs {
                    log::warn!("Timeout reached, killing QEMU...");
                    let _ = child.kill();
                    let _ = child.wait();
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                anyhow::bail!("Error waiting for QEMU: {}", e);
            }
        }
    }
}

