//! CLI определения для xtask
//!
//! Использует clap для парсинга аргументов командной строки.

use clap::{Parser, Subcommand, Args};

/// XenOS Build System
#[derive(Parser)]
#[command(name = "xtask", about = "XenOS build system and task runner")]
#[command(version, author)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Сборка компонентов
    Build(BuildArgs),

    /// Генерация артефактов (hive, nls)
    Generate(GenerateArgs),

    /// Создание образов (disk, sysroot, iso)
    Image(ImageArgs),

    /// Запуск в QEMU
    Run(RunArgs),

    /// Запуск тестов в QEMU
    Test(TestArgs),

    /// Очистка артефактов сборки
    Clean(CleanArgs),

    /// Интерактивная конфигурация (TUI)
    Menuconfig,

    /// Применить конфигурацию по умолчанию
    Defconfig,
}

// =============================================================================
// Build
// =============================================================================

#[derive(Args)]
pub struct BuildArgs {
    #[command(subcommand)]
    pub target: BuildTarget,

    /// Release режим сборки
    #[arg(long, short = 'r')]
    pub release: bool,

    /// Verbose вывод
    #[arg(long, short = 'v')]
    pub verbose: bool,
}

#[derive(Subcommand)]
pub enum BuildTarget {
    /// Сборка ядра ntoskrnl.exe
    Kernel,

    /// Сборка загрузчика winload.efi
    Winload,

    /// Сборка всех boot-драйверов
    Drivers,

    /// Сборка конкретного драйвера
    Driver {
        /// Имя драйвера
        name: String,
    },

    /// Сборка Limine bootloader
    Limine,

    /// Сборка всех компонентов
    All,
}

// =============================================================================
// Generate
// =============================================================================

#[derive(Args)]
pub struct GenerateArgs {
    #[command(subcommand)]
    pub target: GenerateTarget,
}

#[derive(Subcommand)]
pub enum GenerateTarget {
    /// Генерация SYSTEM registry hive
    Hive,

    /// Генерация unicode.nls (скачивает UCD если нужно)
    Nls,

    /// Генерация всех артефактов
    All,
}

// =============================================================================
// Image
// =============================================================================

#[derive(Args)]
pub struct ImageArgs {
    #[command(subcommand)]
    pub target: ImageTarget,

    /// Release режим
    #[arg(long, short = 'r')]
    pub release: bool,
}

#[derive(Subcommand)]
pub enum ImageTarget {
    /// Создание пустого GPT образа с ESP
    Disk,

    /// Полная сборка sysroot в образ
    Sysroot,

    /// Создание гибридного BIOS/UEFI ISO
    Iso,
}

// =============================================================================
// Run
// =============================================================================

#[derive(Args)]
pub struct RunArgs {
    /// Release режим
    #[arg(long, short = 'r')]
    pub release: bool,

    /// Режим GDB отладки (GDB stub, остановка на старте)
    #[arg(long)]
    pub gdb: bool,

    /// QEMU debug flags (-d)
    #[arg(long)]
    pub qemu_debug: Option<String>,

    /// Verbose вывод
    #[arg(long, short = 'v')]
    pub verbose: bool,
}

// =============================================================================
// Test
// =============================================================================

#[derive(Args)]
pub struct TestArgs {
    /// Конкретный тест для запуска
    pub test_name: Option<String>,

    /// Verbose вывод
    #[arg(long, short = 'v')]
    pub verbose: bool,
}

// =============================================================================
// Clean
// =============================================================================

#[derive(Args)]
pub struct CleanArgs {
    /// Полная очистка (включая Limine)
    #[arg(long)]
    pub all: bool,
}

