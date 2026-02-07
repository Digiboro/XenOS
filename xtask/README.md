# XenOS Build System (xtask)

Система сборки XenOS на базе cargo xtask pattern.

## Архитектура

```
xtask/
├── Cargo.toml
└── src/
    ├── main.rs           # Точка входа
    ├── cli.rs            # CLI определения (clap)
    ├── util.rs           # Утилиты, пути, константы
    │
    ├── config/           # Конфигурация
    │   ├── mod.rs        # menuconfig, defconfig
    │   ├── loader.rs     # Загрузка .xenos-config
    │   └── features.rs   # Маппинг config -> Cargo features
    │
    ├── build/            # Сборка компонентов
    │   ├── mod.rs        # BuildContext, execute()
    │   ├── kernel.rs     # Сборка ntoskrnl.exe
    │   ├── winload.rs    # Сборка winload.efi
    │   ├── drivers.rs    # Сборка boot-драйверов
    │   └── limine.rs     # Сборка Limine bootloader
    │
    ├── generate/         # Генерация артефактов
    │   ├── mod.rs
    │   ├── hive.rs       # SYSTEM registry hive
    │   └── nls.rs        # unicode.nls
    │
    ├── image/            # Создание образов
    │   ├── mod.rs
    │   ├── disk.rs       # GPT образ с ESP
    │   ├── sysroot.rs    # Полная сборка sysroot
    │   └── iso.rs        # Гибридный BIOS/UEFI ISO
    │
    └── run/              # Запуск в QEMU
        ├── mod.rs
        └── qemu.rs       # QEMU аргументы, запуск
```

## Добавление новой команды

### 1. Добавить в CLI (`cli.rs`)

```rust
#[derive(Subcommand)]
pub enum Command {
    // ...existing commands...

    /// Новая команда
    NewCommand(NewCommandArgs),
}

#[derive(Args)]
pub struct NewCommandArgs {
    /// Описание аргумента
    #[arg(long)]
    pub some_flag: bool,
}
```

### 2. Создать модуль

```rust
// src/newcommand/mod.rs
use anyhow::Result;
use crate::cli::NewCommandArgs;

pub fn execute(args: NewCommandArgs) -> Result<()> {
    // Реализация
    Ok(())
}
```

### 3. Подключить в `main.rs`

```rust
mod newcommand;

match cli.command {
    // ...
    Command::NewCommand(args) => newcommand::execute(args),
}
```

## Добавление новой опции конфигурации

### 1. Добавить поле в `ConfigValues` (`config/loader.rs`)

```rust
#[allow(non_snake_case)]
pub struct ConfigValues {
    // ...
    #[serde(default)]
    pub NEW_OPTION: bool,
}
```

### 2. Добавить getter

```rust
impl Config {
    pub fn get_bool(&self, key: &str) -> bool {
        match key {
            // ...
            "NEW_OPTION" => self.config.NEW_OPTION,
            _ => false,
        }
    }
}
```

### 3. Использовать в features (`config/features.rs`)

```rust
pub fn kernel_features(&self) -> Vec<String> {
    // ...
    if self.get_bool("NEW_OPTION") {
        features.push("new-feature".into());
    }
}
```

### 4. Добавить в Kconfig.toml

```toml
[[menu.section.config]]
id = "NEW_OPTION"
type = "bool"
prompt = "New Option"
default = false
help = "Description of the new option"
```

## Добавление нового build target

### 1. Создать файл в `build/`

```rust
// src/build/newtarget.rs
use anyhow::Result;
use super::BuildContext;

pub fn build(ctx: &BuildContext) -> Result<()> {
    log::info!("=== Building new target ===");
    // ...
    Ok(())
}
```

### 2. Добавить в `BuildTarget` enum

```rust
#[derive(Subcommand)]
pub enum BuildTarget {
    // ...
    NewTarget,
}
```

### 3. Подключить в `build/mod.rs`

```rust
mod newtarget;

pub fn execute(args: BuildArgs) -> Result<()> {
    match args.target {
        // ...
        BuildTarget::NewTarget => newtarget::build(&ctx),
    }
}
```

## Зависимости

- **clap** - CLI парсер
- **tokio** - Async runtime
- **fs-err** - Улучшенные операции с файлами
- **serde/toml** - Конфигурация
- **ratatui/crossterm** - TUI для menuconfig
- **fatfs/gpt** - Работа с образами дисков
- **hive** - Генерация registry hive (внутренняя библиотека)

## Конфигурация

Файл `.xenos-config` в корне проекта содержит все настройки сборки.
Создать по умолчанию: `cargo xtask defconfig`

Структура:

```toml
[config]
BUILD_MODE = "debug"
KERNEL_ALLOC = true
KERNEL_AML = true
# ... см. полный список в docs/plan/xtask_migration_plan.md
```

## Custom Targets

- `x86_64-xenos-pe.json` - Ядро ntoskrnl.exe (PE32+)
- `x86_64-xenos-dll.json` - Драйверы .sys (PE32+ DLL)
- `x86_64-unknown-uefi` - Загрузчик winload.efi (UEFI)
