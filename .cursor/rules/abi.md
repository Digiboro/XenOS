---
alwaysApply: true
---

# ABI соглашения

- **Windows x64 ABI (`extern "win64"`)** - обязателен для всех публичных API и callback функций ядра
- **System V ABI (`extern "C"`)** - допустим только на стыке с Limine bootloader
- **Rust ABI (без `extern`)** - допустим для внутренних функций, не передаваемых через указатели
