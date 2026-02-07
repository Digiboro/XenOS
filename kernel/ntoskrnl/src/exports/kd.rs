//! Kernel Debugger экспорты (`Dbg*`, `Kd*`)
//!
//! Функции отладочного вывода для драйверов.

// Импортируем из root crate через полный путь
use ntoskrnl::kd;

// =============================================================================
// Basic Types
// =============================================================================

type PVOID = *mut core::ffi::c_void;
type ULONG = u32;

// =============================================================================
// DbgPrint - базовая отладочная печать
// =============================================================================

/// DbgPrint - форматированный отладочный вывод (variadic function)
///
/// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/nf-wdm-dbgprint
///
/// Эта функция variadic и требует специальной обработки.
/// Для драйверов предоставляется упрощённая версия, которая принимает
/// только format string без аргументов. Для полной поддержки variadic
/// нужен assembly wrapper.
#[unsafe(export_name = "DbgPrint")]
pub unsafe extern "win64" fn DbgPrint(format: *const u8) -> ULONG {
    if format.is_null() {
        return 0;
    }

    // Читаем C string
    let mut len = 0usize;
    while *format.add(len) != 0 && len < 4096 {
        len += 1;
    }

    if len == 0 {
        return 0;
    }

    let bytes = core::slice::from_raw_parts(format, len);
    if let Ok(s) = core::str::from_utf8(bytes) {
        kd::dbg_print(s);
        len as ULONG
    } else {
        0
    }
}

/// DbgPrintEx - расширенная отладочная печать с компонентом и уровнем
///
/// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/nf-wdm-dbgprintex
///
/// # Arguments
/// * `component_id` - ID компонента (DPFLTR_*)
/// * `level` - уровень важности (DPFLTR_ERROR_LEVEL, etc)
/// * `format` - строка формата (только string без аргументов в упрощённой версии)
#[unsafe(export_name = "DbgPrintEx")]
pub unsafe extern "win64" fn DbgPrintEx(
    component_id: ULONG,
    level: ULONG,
    format: *const u8,
) -> ULONG {
    if format.is_null() {
        return 0;
    }

    // Читаем C string
    let mut len = 0usize;
    while *format.add(len) != 0 && len < 4096 {
        len += 1;
    }

    if len == 0 {
        return 0;
    }

    let bytes = core::slice::from_raw_parts(format, len);
    if let Ok(s) = core::str::from_utf8(bytes) {
        kd::dbg_print_ex(component_id, level, s);
        len as ULONG
    } else {
        0
    }
}

// =============================================================================
// Helper Functions
// =============================================================================

/// vDbgPrintEx - вспомогательная функция для variadic вызова
///
/// Эта функция используется компилятором для реализации DbgPrintEx с variadic args.
#[unsafe(export_name = "vDbgPrintEx")]
pub unsafe extern "win64" fn vDbgPrintEx(
    component_id: ULONG,
    level: ULONG,
    format: *const u8,
    _va_list: PVOID, // va_list (не используется в упрощённой версии)
) -> ULONG {
    unsafe {
        // В полной реализации нужно парсить format string и va_list
        // Пока просто выводим format string как есть
        DbgPrintEx(component_id, level, format)
    }
}

/// vDbgPrintExWithPrefix - variadic DbgPrint с префиксом
#[unsafe(export_name = "vDbgPrintExWithPrefix")]
pub unsafe extern "win64" fn vDbgPrintExWithPrefix(
    _prefix: *const u8,
    component_id: ULONG,
    level: ULONG,
    format: *const u8,
    _va_list: PVOID,
) -> ULONG {
    unsafe {
        // Игнорируем префикс пока
        DbgPrintEx(component_id, level, format)
    }
}

