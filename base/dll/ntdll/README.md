# NTDLL - NT Layer DLL

User-mode системная библиотека XenOS, предоставляющая stubs для системных вызовов.

## Назначение

NTDLL является низкоуровневым интерфейсом между user-mode приложениями и ядром XenOS.
Каждая функция выполняет переход в kernel mode через инструкцию `SYSCALL`.

## Архитектура

```text
User Mode Application
         │
         ▼
    ┌─────────┐
    │  ntdll  │  ← syscall stubs
    └────┬────┘
         │ SYSCALL
         ▼
    ┌──────────────┐
    │  ntoskrnl    │  ← KiSystemCall64 → Nt* handlers
    └──────────────┘
```

## Syscall Stub

Каждый stub генерируется макросом и имеет следующий формат:

```asm
NtCreateFile:
    mov r10, rcx        ; сохранить 1-й аргумент (SYSCALL затирает RCX)
    mov eax, 0x00       ; номер сервиса (service_id)
    syscall             ; переход в kernel mode
    ret                 ; возврат (результат в RAX)
```

## Соглашение о вызовах

- **ABI**: Win64 (x86_64 Windows calling convention)
- **Аргументы 1-4**: RCX, RDX, R8, R9
- **Аргументы 5+**: на стеке (RSP+0x28, RSP+0x30, ...)
- **Возврат**: NTSTATUS в RAX

## Генерация из единого источника

Список syscalls определён в `../../shared/ntapi/syscalls_list.inc.rs`:

```rust
syscall_list! {
    (crate::io::file::NtCreateFile,  0x00, 11)
    (crate::io::file::NtOpenFile,    0x01,  6)
    ...
}
```

Один файл используется для:

- **ntoskrnl**: регистрация обработчиков в SSDT
- **ntdll**: генерация syscall stubs

## Файлы

| Файл         | Описание                                     |
| ------------ | -------------------------------------------- |
| `src/lib.rs` | Макросы и генерация stubs                    |
| `version.rc` | Ресурс версии (PE metadata)                  |
| `build.rs`   | Парсинг syscalls_list, генерация .def и .lib |

## Сборка

```bash
# Через Makefile (рекомендуется)
make ntdll

# Или напрямую через cargo
cargo build --release \
    --package ntdll \
    --target ./config/targets/x86_64-xenos-dll.json \
    -Zbuild-std=core,alloc \
    -Zbuild-std-features=compiler-builtins-mem
```

При сборке `build.rs` автоматически:

1. Парсит `../../shared/ntapi/syscalls_list.inc.rs`
2. Генерирует `ntdll.def` с экспортами
3. Создаёт `ntdll.lib` (import library)

Результат:

- `target/x86_64-xenos-dll/release/ntdll.dll` - PE32+ DLL
- `target/.../build/ntdll-.../out/ntdll.def` - Экспорты (auto-generated)
- `target/.../build/ntdll-.../out/ntdll.lib` - Import library для C/C++

## Sysroot

При сборке `make sysroot` ntdll.dll копируется в:

```
sysroot/XenOS/system32/ntdll.dll
```

## Экспортируемые функции

### I/O Services (0x00 - 0x1F)

- `NtCreateFile` - создание/открытие файла
- `NtOpenFile` - открытие файла
- `NtReadFile` - чтение из файла
- `NtWriteFile` - запись в файл
- `NtClose` - закрытие handle
- `NtDeviceIoControlFile` - IOCTL для устройства
- `NtFsControlFile` - FSCTL для файловой системы
- `NtCancelIoFile` - отмена I/O операции
- `NtCancelIoFileEx` - отмена конкретной I/O операции

### Memory Services (0x20 - 0x3F)

- `NtCreateSection` - создание section object
- `NtMapViewOfSection` - отображение section в память
- `NtUnmapViewOfSection` - удаление отображения

### Process/Thread Services (0x40 - 0x5F)

- `NtTerminateProcess` - завершение процесса
- `NtCreateThread` - создание потока
- `NtTerminateThread` - завершение потока

## Совместимость

Ориентир: **Windows NT 6.1 (Windows 7)**

Сигнатуры функций соответствуют MSDN/WDK документации для NT 6.1.
