---
alwaysApply: true
---

# Именование функций

- При создании аналогов внутренних функций ядра Windows NT (например, `PspExitThread`) используем snake_case для соответствия конвенциям Rust (например, `psp_exit_thread`)
- В docstring функции обязательно указываем оригинальное имя функции Windows NT (например, `/// PspExitThread`)
- Для публичных функций и syscalls используем оригинальное имя функции Windows NT в PascalCase (например, `pub fn PspExitThread`), крайне важно сохранить совместимость с WinNT API.
