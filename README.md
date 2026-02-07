```
██╗  ██╗███████╗███╗  ██╗ ██████╗ ███████╗
╚██╗██╔╝██╔════╝████╗ ██║██╔═══██╗██╔════╝
 ╚███╔╝ █████╗  ██╔██╗██║██║   ██║███████╗
 ██╔██╗ ██╔══╝  ██║╚██╗██║██║   ██║╚════██║
██╔╝ ██╗███████╗██║ ╚████║╚██████╔╝███████║
╚═╝  ╚═╝╚══════╝╚═╝  ╚═══╝ ╚═════╝ ╚══════╝

            XenOS v0.0.1-dev
     NT-compatible Operating System
```

## Quick Start

```bash
# Clone with submodules
git clone --recursive https://github.com/XenOS/XenCore.git
cd XenCore

# Create default configuration
cargo xtask defconfig

# Build and run
cargo xtask run
```

## Build Commands

### Building Components

```bash
cargo xtask build kernel          # Build kernel ntoskrnl.exe
cargo xtask build winload         # Build bootloader winload.efi
cargo xtask build drivers         # Build all boot drivers
cargo xtask build driver <name>   # Build specific driver
cargo xtask build limine          # Build Limine bootloader
cargo xtask build all             # Build all components

# Flags:
#   -r, --release    Release mode
#   -v, --verbose    Verbose output
```

### Generating Artifacts

```bash
cargo xtask generate hive         # Generate SYSTEM registry hive
cargo xtask generate nls          # Generate unicode.nls
cargo xtask generate all          # Generate all artifacts
```

### Creating Images

```bash
cargo xtask image disk            # Create GPT image with ESP
cargo xtask image sysroot         # Full sysroot build into image
cargo xtask image iso             # Create hybrid BIOS/UEFI ISO

# Flags:
#   -r, --release    Release mode
```

### Running and Debugging

```bash
cargo xtask run                   # Run in QEMU
cargo xtask run --release         # Release mode
cargo xtask run --xendbg          # XenDbg mode (debugcon + GDB)
cargo xtask test                  # Run tests in QEMU
```

### Configuration

```bash
cargo xtask defconfig             # Create default .xenos-config
cargo xtask menuconfig           # Interactive configuration (TUI)
```

### Cleanup

```bash
cargo xtask clean                 # Clean cargo artifacts
cargo xtask clean --all           # Full cleanup (including build/, sysroot/, Limine)
```

## Configuration

The `.xenos-config` file controls build parameters:

```toml
[config]
BUILD_MODE = "debug"              # debug | release | test

# Kernel Features
KERNEL_ALLOC = true               # Kernel allocator
KERNEL_AML = true                 # ACPI AML interpreter

# Debugging
DEBUG_BOOT_SOS = false            # Verbose boot (/SOS)
DEBUG_KD_FORCE = false            # Force kernel debugger

# Tracing (debugcon port 0xE9)
TRACE_ENABLE = false              # Enable tracing
TRACE_SCHED = false               # Scheduler tracing
TRACE_PS = false                  # Process/thread tracing
TRACE_MM = false                  # Memory manager tracing
TRACE_OB = false                  # Object manager tracing
TRACE_IO = false                  # I/O manager tracing

# QEMU
QEMU_MACHINE = "q35"              # q35 | pc
QEMU_CPU = "max"                  # max | base
QEMU_MEMORY = "1G"
QEMU_VGA = "std"                  # none | std | cirrus | vmware | virtio
```

## Project Structure

```
XenCore/
├── kernel/
│   └── ntoskrnl/           # Kernel (ntoskrnl.exe)
├── boot/
│   ├── winload/            # UEFI bootloader (winload.efi)
│   └── drivers/            # Boot drivers
│       ├── bootvid/        # Boot Video Driver
│       └── test-driver/    # Test driver
├── libs/
│   ├── ntldr/              # Shared loader structures
│   └── hive/               # Registry hive library
├── base/                   # System components (future)
│   ├── dll/
│   ├── apps/
│   └── services/
├── vendor/                 # External dependencies (submodules)
│   ├── uefi_rs/            # UEFI library
│   ├── limine/             # Limine bootloader
│   ├── pci_types/
│   └── spinning_top/
├── xtask/                  # Build system
├── config/                 # Configuration (Kconfig.toml)
├── assets/                 # Resources (fonts, hive templates)
└── ovmf/                   # UEFI firmware for QEMU
```

## Requirements

### System Utilities

- `qemu-system-x86_64` - Emulator
- `sgdisk` (gdisk) - GPT partitioning
- `mtools` (mcopy, mmd, mformat) - FAT32 operations
- `xorriso` - ISO creation
- `curl` - Download UCD for NLS

### Rust Toolchain

- Rust nightly (see `rust-toolchain.toml`)
- Target: `x86_64-unknown-uefi`
- LLVM toolchain for Limine (macOS: `brew install llvm`)

## License

MIT License
