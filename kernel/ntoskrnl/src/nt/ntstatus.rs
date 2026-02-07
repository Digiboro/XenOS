//! NTSTATUS коды и макросы
//!
//! Источники:
//! - NT5: inc/ntstatus.h
//! - ReactOS: include/ndk/ntstatus.h

use super::LONG;

/// NTSTATUS - код возврата NT функций
///
/// Формат:
/// - Биты 31-30: Severity (00=Success, 01=Info, 10=Warning, 11=Error)
/// - Бит 29: Customer (1=пользовательский код)
/// - Бит 28: Reserved
/// - Биты 27-16: Facility
/// - Биты 15-0: Code
pub type NTSTATUS = LONG;

/// Проверка успешности NTSTATUS
#[inline]
pub const fn nt_success(status: NTSTATUS) -> bool {
    status >= 0
}

/// Проверка информационного статуса
#[inline]
pub const fn nt_information(status: NTSTATUS) -> bool {
    ((status as u32) >> 30) == 1
}

/// Проверка предупреждения
#[inline]
pub const fn nt_warning(status: NTSTATUS) -> bool {
    ((status as u32) >> 30) == 2
}

/// Проверка ошибки
#[inline]
pub const fn nt_error(status: NTSTATUS) -> bool {
    ((status as u32) >> 30) == 3
}

// =============================================================================
// Success codes (0x00000000 - 0x3FFFFFFF)
// =============================================================================

/// Операция выполнена успешно
pub const STATUS_SUCCESS: NTSTATUS = 0x00000000;

/// Ожидание объекта 0
pub const STATUS_WAIT_0: NTSTATUS = 0x00000000;

/// Объект был abandoned
pub const STATUS_ABANDONED_WAIT_0: NTSTATUS = 0x00000080;

/// Мьютекс был abandoned (владелец завершился без освобождения)
pub const STATUS_ABANDONED: NTSTATUS = 0x00000080;

/// Ожидание было прервано user-mode APC
pub const STATUS_USER_APC: NTSTATUS = 0x000000C0;

/// Ожидание было прервано kernel-mode APC
pub const STATUS_KERNEL_APC: NTSTATUS = 0x00000100;

/// Ожидание истекло по таймауту
pub const STATUS_TIMEOUT: NTSTATUS = 0x00000102;

/// Ожидание было прервано
pub const STATUS_ALERTED: NTSTATUS = 0x00000101;

/// STATUS_PENDING — операция ещё не завершена (async I/O)
pub const STATUS_PENDING: NTSTATUS = 0x00000103;

/// Требуется перепарсить имя (используется при парсинге symbolic link)
pub const STATUS_REPARSE: NTSTATUS = 0x00000104;

// =============================================================================
// Information codes (0x40000000 - 0x7FFFFFFF)
// =============================================================================

/// Объект существует (при создании)
pub const STATUS_OBJECT_NAME_EXISTS: NTSTATUS = 0x40000000;

/// Поток был suspended
pub const STATUS_THREAD_WAS_SUSPENDED: NTSTATUS = 0x40000001;

/// Ключ реестра уже существует
pub const STATUS_KEY_HAS_CHILDREN: NTSTATUS = 0x40000002;

// =============================================================================
// Warning codes (0x80000000 - 0xBFFFFFFF)
// =============================================================================

/// Предупреждение: буфер переполнен
pub const STATUS_BUFFER_OVERFLOW: NTSTATUS = 0x80000005u32 as i32;

/// Больше нет записей
pub const STATUS_NO_MORE_ENTRIES: NTSTATUS = 0x8000001Au32 as i32;

// =============================================================================
// Error codes (0xC0000000 - 0xFFFFFFFF)
// =============================================================================

/// Неудачная операция
pub const STATUS_UNSUCCESSFUL: NTSTATUS = 0xC0000001u32 as i32;

/// Функция не реализована
pub const STATUS_NOT_IMPLEMENTED: NTSTATUS = 0xC0000002u32 as i32;

/// Неверная информация
pub const STATUS_INVALID_INFO_CLASS: NTSTATUS = 0xC0000003u32 as i32;

/// Размер информации неверный
pub const STATUS_INFO_LENGTH_MISMATCH: NTSTATUS = 0xC0000004u32 as i32;

/// Нарушение доступа
pub const STATUS_ACCESS_VIOLATION: NTSTATUS = 0xC0000005u32 as i32;

/// Неверный handle
pub const STATUS_INVALID_HANDLE: NTSTATUS = 0xC0000008u32 as i32;

/// Неверный параметр
pub const STATUS_INVALID_PARAMETER: NTSTATUS = 0xC000000Du32 as i32;

/// Объект не найден
pub const STATUS_OBJECT_NAME_NOT_FOUND: NTSTATUS = 0xC0000034u32 as i32;

/// Имя объекта уже существует
pub const STATUS_OBJECT_NAME_COLLISION: NTSTATUS = 0xC0000035u32 as i32;

/// Неверный путь объекта
pub const STATUS_OBJECT_PATH_INVALID: NTSTATUS = 0xC0000039u32 as i32;

/// Путь объекта не найден
pub const STATUS_OBJECT_PATH_NOT_FOUND: NTSTATUS = 0xC000003Au32 as i32;

/// Неверный тип объекта
pub const STATUS_OBJECT_TYPE_MISMATCH: NTSTATUS = 0xC0000024u32 as i32;

/// Доступ запрещен
pub const STATUS_ACCESS_DENIED: NTSTATUS = 0xC0000022u32 as i32;

/// Буфер слишком мал
pub const STATUS_BUFFER_TOO_SMALL: NTSTATUS = 0xC0000023u32 as i32;

/// Недостаточно памяти
pub const STATUS_NO_MEMORY: NTSTATUS = 0xC0000017u32 as i32;

/// Недостаточно ресурсов
pub const STATUS_INSUFFICIENT_RESOURCES: NTSTATUS = 0xC000009Au32 as i32;

/// Конфликтующие адреса (перекрытие VAD)
pub const STATUS_CONFLICTING_ADDRESSES: NTSTATUS = 0xC0000018u32 as i32;

/// Guard page violation (нормальное исключение для stack growth)
pub const STATUS_GUARD_PAGE_VIOLATION: NTSTATUS = 0x80000001u32 as i32;

/// Ошибка при подъёме страницы (I/O error или no memory)
pub const STATUS_IN_PAGE_ERROR: NTSTATUS = 0xC0000006u32 as i32;

/// Секция не является image (попытка создать image section из не-PE файла)
pub const STATUS_SECTION_NOT_IMAGE: NTSTATUS = 0xC000007Bu32 as i32;

/// Неверная image (повреждённый PE header)
pub const STATUS_INVALID_IMAGE_FORMAT: NTSTATUS = 0xC000007Bu32 as i32;

/// Image не подходит для машины (неверная архитектура)
pub const STATUS_INVALID_IMAGE_NOT_MZ: NTSTATUS = 0xC000012Fu32 as i32;

/// Хэндл защищен от закрытия
pub const STATUS_HANDLE_NOT_CLOSABLE: NTSTATUS = 0xC0000235u32 as i32;

/// Неверное смещение устройства
pub const STATUS_INVALID_DEVICE_REQUEST: NTSTATUS = 0xC0000010u32 as i32;

/// Устройство не готово
pub const STATUS_DEVICE_NOT_READY: NTSTATUS = 0xC00000A3u32 as i32;

/// Конец файла
pub const STATUS_END_OF_FILE: NTSTATUS = 0xC0000011u32 as i32;

/// Файл не найден
pub const STATUS_NO_SUCH_FILE: NTSTATUS = 0xC000000Fu32 as i32;

/// Неверное имя файла
pub const STATUS_OBJECT_NAME_INVALID: NTSTATUS = 0xC0000033u32 as i32;

/// Уже существует
pub const STATUS_ALREADY_EXISTS: NTSTATUS = 0xC0000035u32 as i32;

/// Привилегия не удерживается
pub const STATUS_PRIVILEGE_NOT_HELD: NTSTATUS = 0xC0000061u32 as i32;

/// Процесс завершается
pub const STATUS_PROCESS_IS_TERMINATING: NTSTATUS = 0xC000010Au32 as i32;

/// Поток не найден
pub const STATUS_THREAD_NOT_IN_PROCESS: NTSTATUS = 0xC000010Bu32 as i32;

/// Неверный CID
pub const STATUS_INVALID_CID: NTSTATUS = 0xC000000Bu32 as i32;

/// Неудачная инициализация Phase 0
pub const STATUS_PHASE0_INITIALIZATION_FAILED: NTSTATUS = 0xC0000001u32 as i32;

/// Неудачная инициализация Phase 1
pub const STATUS_PHASE1_INITIALIZATION_FAILED: NTSTATUS = 0xC0000001u32 as i32;

/// STATUS_SHARING_VIOLATION — нарушение режима совместного доступа
pub const STATUS_SHARING_VIOLATION: NTSTATUS = 0xC0000043u32 as i32;

/// STATUS_DELETE_PENDING — файл помечен для удаления
pub const STATUS_DELETE_PENDING: NTSTATUS = 0xC0000056u32 as i32;

/// STATUS_FILE_DELETED — файл удалён
pub const STATUS_FILE_DELETED: NTSTATUS = 0xC0000123u32 as i32;

/// STATUS_CANNOT_DELETE — невозможно удалить
pub const STATUS_CANNOT_DELETE: NTSTATUS = 0xC0000121u32 as i32;

/// STATUS_NOT_SUPPORTED — операция не поддерживается
pub const STATUS_NOT_SUPPORTED: NTSTATUS = 0xC00000BBu32 as i32;

/// Неверная защита страницы
pub const STATUS_INVALID_PAGE_PROTECTION: NTSTATUS = 0xC0000045u32 as i32;

/// Память не выделена (попытка decommit/free невыделенной памяти)
pub const STATUS_MEMORY_NOT_ALLOCATED: NTSTATUS = 0xC00000A0u32 as i32;

/// Невозможно удалить секцию (попытка освободить mapped view)
pub const STATUS_UNABLE_TO_DELETE_SECTION: NTSTATUS = 0xC0000022u32 as i32; // Same as ACCESS_DENIED

/// Неверный размер view секции
pub const STATUS_INVALID_VIEW_SIZE: NTSTATUS = 0xC000001Fu32 as i32;

/// Секция слишком большая
pub const STATUS_SECTION_TOO_BIG: NTSTATUS = 0xC0000040u32 as i32;

/// Неверный файл для секции
pub const STATUS_INVALID_FILE_FOR_SECTION: NTSTATUS = 0xC0000020u32 as i32;

/// Адрес не является mapped view
pub const STATUS_NOT_MAPPED_VIEW: NTSTATUS = 0xC0000019u32 as i32;

/// Commitлимит системы превышен
pub const STATUS_COMMITMENT_LIMIT: NTSTATUS = 0xC000012Du32 as i32;

// =============================================================================
// Security (SE) status codes
// =============================================================================

/// Неверный Security Descriptor
pub const STATUS_INVALID_SECURITY_DESCR: NTSTATUS = 0xC0000079u32 as i32;

/// Неверный SID
pub const STATUS_INVALID_SID: NTSTATUS = 0xC0000078u32 as i32;

/// Неверный ACL
pub const STATUS_INVALID_ACL: NTSTATUS = 0xC0000077u32 as i32;

/// Неверный owner в SD
pub const STATUS_INVALID_OWNER: NTSTATUS = 0xC000005Au32 as i32;

/// Неверная первичная группа в SD
pub const STATUS_INVALID_PRIMARY_GROUP: NTSTATUS = 0xC0000076u32 as i32;

/// Плохой уровень impersonation
pub const STATUS_BAD_IMPERSONATION_LEVEL: NTSTATUS = 0xC00000A5u32 as i32;

/// Невозможно открыть анонимный токен уровня
pub const STATUS_CANT_OPEN_ANONYMOUS: NTSTATUS = 0xC00000A6u32 as i32;

/// Невозможно выполнить impersonation
pub const STATUS_CANNOT_IMPERSONATE: NTSTATUS = 0xC000010Du32 as i32;

/// Токен уже используется
pub const STATUS_TOKEN_ALREADY_IN_USE: NTSTATUS = 0xC000015Au32 as i32;

/// Неподходящий тип токена
pub const STATUS_BAD_TOKEN_TYPE: NTSTATUS = 0xC00000A8u32 as i32;

/// Нет токена impersonation для потока
pub const STATUS_NO_TOKEN: NTSTATUS = 0xC000007Cu32 as i32;

/// Неверная группа
pub const STATUS_NO_SUCH_PRIVILEGE: NTSTATUS = 0xC0000060u32 as i32;

/// Требуется больший буфер
pub const STATUS_MORE_ENTRIES: NTSTATUS = 0x00000105;

/// Некоторые привилегии не назначены вызывающему
pub const STATUS_NOT_ALL_ASSIGNED: NTSTATUS = 0x00000106;

/// Особый аккаунт (таблица особых аккаунтов)
pub const STATUS_SPECIAL_ACCOUNT: NTSTATUS = 0xC0000124u32 as i32;

/// Нет прав доступа для аудита
pub const STATUS_NO_IMPERSONATION_TOKEN: NTSTATUS = 0xC000005Cu32 as i32;

// =============================================================================
// Facility codes
// =============================================================================

pub const FACILITY_NTWIN32: u32 = 0x7;
pub const FACILITY_DEBUGGER: u32 = 0x1;

/// Создает NTSTATUS из Win32 error code
#[inline]
pub const fn ntstatus_from_win32(error: u32) -> NTSTATUS {
    if error == 0 {
        STATUS_SUCCESS
    } else {
        ((error & 0x0000FFFF) | ((FACILITY_NTWIN32 as u32) << 16) | 0xC0000000) as i32
    }
}
