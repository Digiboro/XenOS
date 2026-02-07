//! Подсистема системных вызовов (Syscalls)
//!
//! Реализация NT-подобной системы обработки системных вызовов для x86_64.
//!
//! # Архитектура
//!
//! ```text
//!                        User Mode
//!     ┌─────────────────────────────────────────────┐
//!     │   ntdll.dll                                 │
//!     │   ┌────────────────────┐                    │
//!     │   │ NtCreateFile stub  │                    │
//!     │   │   mov eax, 0x55    │ ← Номер сервиса   │
//!     │   │   mov r10, rcx     │ ← Arg1 в r10      │
//!     │   │   syscall          │                    │
//!     │   └────────────────────┘                    │
//!     └───────────────────│─────────────────────────┘
//!                         │ SYSCALL
//!     ════════════════════│════════════════════════════
//!                         ▼ Kernel Mode
//!     ┌─────────────────────────────────────────────┐
//!     │  KiSystemCall64 (ASM entrypoint)           │
//!     │   • swapgs                                  │
//!     │   • Переключение на kernel stack            │
//!     │   • Сохранение volatile регистров          │
//!     │   • Установка PreviousMode = UserMode       │
//!     │   • Вызов ki_system_service_dispatch()     │
//!     └───────────────────│─────────────────────────┘
//!                         │
//!     ┌───────────────────▼─────────────────────────┐
//!     │  ki_system_service_dispatch()              │
//!     │   • Проверка service_id < limit             │
//!     │   • Lookup в KSERVICE_TABLE_DESCRIPTOR     │
//!     │   • Вызов Nt* функции                       │
//!     │   • Возврат NTSTATUS в RAX                  │
//!     └─────────────────────────────────────────────┘
//! ```
//!
//! # Соглашение о вызове (x86_64 Windows ABI)
//!
//! При инструкции `SYSCALL`:
//! - `RAX` = номер системного вызова (service_id)
//! - `RCX` = return address (сохраняется CPU в RCX при SYSCALL)
//! - `R10` = первый аргумент (переносится из RCX в user-mode stub)
//! - `RDX` = второй аргумент
//! - `R8`  = третий аргумент
//! - `R9`  = четвёртый аргумент
//! - Остальные аргументы — на user stack
//!
//! # Таблица сервисов (SSDT)
//!
//! XenOS использует собственную нумерацию сервисов, не совместимую с Windows.
//! Номера стабильны в рамках одной версии и согласованы с user-mode библиотеками.
//!
//! Источники:
//! - NT6.1: ke/amd64/trap.asm, ke/amd64/systable.asm
//! - ReactOS: ke/amd64/stubs.c, ke/amd64/trap.S

#![allow(dead_code)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

use crate::nt::NTSTATUS;
use crate::nt::PVOID;
use crate::nt::ULONG;
use crate::nt::ntstatus::*;

// =============================================================================
// Режим процессора (KPROCESSOR_MODE)
// =============================================================================

/// KPROCESSOR_MODE — режим процессора
///
/// Определяет контекст выполнения: ядро или пользовательский режим.
/// Используется для валидации указателей в системных вызовах.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KPROCESSOR_MODE {
    /// Kernel mode — доверенный код ядра
    KernelMode = 0,
    /// User mode — недоверенный пользовательский код
    UserMode = 1,
}

impl From<u8> for KPROCESSOR_MODE {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::KernelMode,
            _ => Self::UserMode,
        }
    }
}

impl From<KPROCESSOR_MODE> for u8 {
    fn from(mode: KPROCESSOR_MODE) -> u8 {
        mode as u8
    }
}

// =============================================================================
// Тип функции системного вызова
// =============================================================================

/// Тип функции системного вызова
///
/// Все Nt* функции имеют сигнатуру `fn(...) -> NTSTATUS`.
/// Количество аргументов варьируется (0-17 в Windows).
///
/// Диспетчер передаёт аргументы через указатель на массив u64,
/// где каждый элемент — значение регистра или слот на стеке.
pub type PKSYSTEM_SERVICE = unsafe extern "win64" fn(
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
    // Аргументы 5+ передаются на стеке в SYSCALL frame
) -> NTSTATUS;

// =============================================================================
// KSERVICE_TABLE_DESCRIPTOR
// =============================================================================

/// KSERVICE_TABLE_DESCRIPTOR — дескриптор таблицы системных сервисов
///
/// NT использует две таблицы:
/// - SSDT (KeServiceDescriptorTable) — ntoskrnl сервисы (Nt*)
/// - SSSDT (KeServiceDescriptorTableShadow) — win32k сервисы (NtUser*, NtGdi*)
///
/// XenOS на данном этапе использует только SSDT.
#[repr(C)]
pub struct KSERVICE_TABLE_DESCRIPTOR {
    /// Указатель на таблицу указателей функций
    pub base: *const PKSYSTEM_SERVICE,

    /// Указатель на таблицу количества аргументов (опционально)
    /// Каждый байт = количество 8-байтных аргументов для соответствующего сервиса
    pub argument_table: *const u8,

    /// Количество сервисов в таблице (limit)
    pub limit: ULONG,

    /// Количество вызовов сервисов (для статистики)
    pub number: *mut ULONG,
}

// Safety: KSERVICE_TABLE_DESCRIPTOR содержит только указатели на статические данные
unsafe impl Sync for KSERVICE_TABLE_DESCRIPTOR {}
unsafe impl Send for KSERVICE_TABLE_DESCRIPTOR {}

impl KSERVICE_TABLE_DESCRIPTOR {
    /// Создаёт пустой дескриптор
    pub const fn empty() -> Self {
        Self {
            base: core::ptr::null(),
            argument_table: core::ptr::null(),
            limit: 0,
            number: core::ptr::null_mut(),
        }
    }

    /// Создаёт дескриптор из таблицы
    pub const fn new(
        base: *const PKSYSTEM_SERVICE,
        argument_table: *const u8,
        limit: ULONG,
    ) -> Self {
        Self {
            base,
            argument_table,
            limit,
            number: core::ptr::null_mut(),
        }
    }
}

// =============================================================================
// Номера системных сервисов (XenOS-специфичные)
// =============================================================================

/// Номера системных сервисов XenOS
///
/// ВАЖНО: Эти номера — контракт между ядром и user-mode библиотеками XenOS.
/// Изменение номеров требует обновления ntdll и пересборки всего user-mode.
pub mod service_numbers {
    // =========================================================================
    // I/O Services (0x00 - 0x1F)
    // =========================================================================

    /// NtCreateFile
    pub const NT_CREATE_FILE: u32 = 0x00;
    /// NtOpenFile
    pub const NT_OPEN_FILE: u32 = 0x01;
    /// NtReadFile
    pub const NT_READ_FILE: u32 = 0x02;
    /// NtWriteFile
    pub const NT_WRITE_FILE: u32 = 0x03;
    /// NtClose
    pub const NT_CLOSE: u32 = 0x04;
    /// NtDeviceIoControlFile
    pub const NT_DEVICE_IO_CONTROL_FILE: u32 = 0x05;
    /// NtFsControlFile
    pub const NT_FS_CONTROL_FILE: u32 = 0x06;
    /// NtCancelIoFile
    pub const NT_CANCEL_IO_FILE: u32 = 0x07;
    /// NtCancelIoFileEx
    pub const NT_CANCEL_IO_FILE_EX: u32 = 0x08;
    /// NtQueryInformationFile
    pub const NT_QUERY_INFORMATION_FILE: u32 = 0x09;
    /// NtSetInformationFile
    pub const NT_SET_INFORMATION_FILE: u32 = 0x0A;

    // =========================================================================
    // Memory Services (0x20 - 0x3F)
    // =========================================================================

    /// NtCreateSection
    pub const NT_CREATE_SECTION: u32 = 0x20;
    /// NtOpenSection
    pub const NT_OPEN_SECTION: u32 = 0x21;
    /// NtMapViewOfSection
    pub const NT_MAP_VIEW_OF_SECTION: u32 = 0x22;
    /// NtUnmapViewOfSection
    pub const NT_UNMAP_VIEW_OF_SECTION: u32 = 0x23;
    /// NtAllocateVirtualMemory
    pub const NT_ALLOCATE_VIRTUAL_MEMORY: u32 = 0x24;
    /// NtFreeVirtualMemory
    pub const NT_FREE_VIRTUAL_MEMORY: u32 = 0x25;

    // =========================================================================
    // Process/Thread Services (0x40 - 0x5F)
    // =========================================================================

    /// NtCreateProcess
    pub const NT_CREATE_PROCESS: u32 = 0x40;
    /// NtTerminateProcess
    pub const NT_TERMINATE_PROCESS: u32 = 0x41;
    /// NtCreateThread
    pub const NT_CREATE_THREAD: u32 = 0x42;
    /// NtTerminateThread
    pub const NT_TERMINATE_THREAD: u32 = 0x43;

    // =========================================================================
    // Object Services (0x60 - 0x7F)
    // =========================================================================

    /// NtQueryObject
    pub const NT_QUERY_OBJECT: u32 = 0x60;
    /// NtSetInformationObject
    pub const NT_SET_INFORMATION_OBJECT: u32 = 0x61;
    /// NtDuplicateObject
    pub const NT_DUPLICATE_OBJECT: u32 = 0x62;

    // =========================================================================
    // Максимальный номер сервиса
    // =========================================================================

    /// Максимальный номер сервиса + 1 (размер таблицы)
    pub const MAX_SERVICE_NUMBER: u32 = 0x100;
}

// =============================================================================
// STATUS_INVALID_SYSTEM_SERVICE
// =============================================================================

/// STATUS_INVALID_SYSTEM_SERVICE — неверный номер системного сервиса
///
/// Возвращается когда service_id >= limit таблицы.
pub const STATUS_INVALID_SYSTEM_SERVICE: NTSTATUS = 0xC000001Cu32 as i32;

// =============================================================================
// Таблица системных сервисов (SSDT)
// =============================================================================

/// Глобальная таблица сервисов NT (SSDT)
///
/// Инициализируется в ki_initialize_system_service_table()
pub static mut KI_SERVICE_TABLE: KSERVICE_TABLE_DESCRIPTOR = KSERVICE_TABLE_DESCRIPTOR::empty();

/// Таблица указателей на функции сервисов
///
/// Размер = MAX_SERVICE_NUMBER
static mut SERVICE_TABLE: [PKSYSTEM_SERVICE; service_numbers::MAX_SERVICE_NUMBER as usize] =
    [stub_not_implemented; service_numbers::MAX_SERVICE_NUMBER as usize];

/// Таблица количества аргументов для каждого сервиса
///
/// Пока не используется, но сохранена для совместимости с NT.
static SERVICE_ARGUMENT_TABLE: [u8; service_numbers::MAX_SERVICE_NUMBER as usize] =
    [0; service_numbers::MAX_SERVICE_NUMBER as usize];

// =============================================================================
// Заглушка для нереализованных сервисов
// =============================================================================

/// Заглушка для нереализованного сервиса
///
/// Возвращает STATUS_NOT_IMPLEMENTED.
unsafe extern "win64" fn stub_not_implemented(
    _arg1: u64,
    _arg2: u64,
    _arg3: u64,
    _arg4: u64,
) -> NTSTATUS {
    STATUS_NOT_IMPLEMENTED
}

// =============================================================================
// PreviousMode API
// =============================================================================

/// Получает PreviousMode текущего потока
///
/// KE_GET_PREVIOUS_MODE / KeGetPreviousMode
///
/// Возвращает режим процессора для текущего системного вызова:
/// - UserMode — вызов из пользовательского кода (через SYSCALL)
/// - KernelMode — внутренний вызов из ядра
///
/// # Safety
/// Требует корректно настроенный GS (KPCR).
#[inline]
pub unsafe fn ke_get_previous_mode() -> KPROCESSOR_MODE {
    unsafe {
        use crate::arch::x86_64::pcr::get_prcb;
        use crate::ke::thread::KTHREAD;

        let prcb = get_prcb();
        if prcb.is_null() {
            return KPROCESSOR_MODE::KernelMode;
        }

        let thread = (*prcb).current_thread as *mut KTHREAD;
        if thread.is_null() {
            return KPROCESSOR_MODE::KernelMode;
        }

        KPROCESSOR_MODE::from((*thread).previous_mode)
    }
}

/// Устанавливает PreviousMode текущего потока
///
/// KE_SET_PREVIOUS_MODE
///
/// ВНИМАНИЕ: Использовать только в контролируемых местах:
/// - При входе в syscall handler
/// - При внутренних Zw* вызовах
///
/// # Safety
/// Требует корректно настроенный GS (KPCR).
#[inline]
pub unsafe fn ke_set_previous_mode(mode: KPROCESSOR_MODE) {
    unsafe {
        use crate::arch::x86_64::pcr::get_prcb;
        use crate::ke::thread::KTHREAD;

        let prcb = get_prcb();
        if prcb.is_null() {
            return;
        }

        let thread = (*prcb).current_thread as *mut KTHREAD;
        if thread.is_null() {
            return;
        }

        (*thread).previous_mode = mode as u8;
    }
}

// =============================================================================
// Инициализация таблицы сервисов
// =============================================================================

/// Инициализирует таблицу системных сервисов
///
/// Вызывается при инициализации ядра.
/// Регистрирует все реализованные Nt* функции в SSDT.
pub unsafe fn ki_initialize_system_service_table() {
    unsafe {
        use service_numbers::*;

        // =========================================================================
        // I/O сервисы
        // =========================================================================

        SERVICE_TABLE[NT_CREATE_FILE as usize] = NtCreateFile_wrapper;
        SERVICE_TABLE[NT_OPEN_FILE as usize] = NtOpenFile_wrapper;
        SERVICE_TABLE[NT_READ_FILE as usize] = NtReadFile_wrapper;
        SERVICE_TABLE[NT_WRITE_FILE as usize] = NtWriteFile_wrapper;
        SERVICE_TABLE[NT_CLOSE as usize] = NtClose_wrapper;
        SERVICE_TABLE[NT_DEVICE_IO_CONTROL_FILE as usize] = NtDeviceIoControlFile_wrapper;
        SERVICE_TABLE[NT_FS_CONTROL_FILE as usize] = NtFsControlFile_wrapper;
        SERVICE_TABLE[NT_CANCEL_IO_FILE as usize] = NtCancelIoFile_wrapper;
        SERVICE_TABLE[NT_CANCEL_IO_FILE_EX as usize] = NtCancelIoFileEx_wrapper;

        // =========================================================================
        // Memory сервисы
        // =========================================================================

        SERVICE_TABLE[NT_CREATE_SECTION as usize] = NtCreateSection_wrapper;
        SERVICE_TABLE[NT_MAP_VIEW_OF_SECTION as usize] = NtMapViewOfSection_wrapper;
        SERVICE_TABLE[NT_UNMAP_VIEW_OF_SECTION as usize] = NtUnmapViewOfSection_wrapper;

        // =========================================================================
        // Process/Thread сервисы
        // =========================================================================

        SERVICE_TABLE[NT_TERMINATE_THREAD as usize] = NtTerminateThread_wrapper;
        SERVICE_TABLE[NT_TERMINATE_PROCESS as usize] = NtTerminateProcess_wrapper;
        SERVICE_TABLE[NT_CREATE_THREAD as usize] = NtCreateThread_wrapper;
        // TODO: NT_CREATE_PROCESS (требует MM/Section support)

        // =========================================================================
        // Настраиваем глобальный дескриптор
        // =========================================================================

        // Используем addr_of! для безопасного получения указателя на static
        KI_SERVICE_TABLE = KSERVICE_TABLE_DESCRIPTOR::new(
            core::ptr::addr_of!(SERVICE_TABLE) as *const PKSYSTEM_SERVICE,
            core::ptr::addr_of!(SERVICE_ARGUMENT_TABLE) as *const u8,
            MAX_SERVICE_NUMBER,
        );
    }
}

// =============================================================================
// Диспетчер системных вызовов
// =============================================================================

/// Диспетчер системных вызовов
///
/// KiSystemServiceDispatch / KiSystemService
///
/// Выполняет маршрутизацию системного вызова по номеру сервиса.
/// Вызывается из ASM entrypoint (KiSystemCall64) после настройки стека.
///
/// # Arguments
/// * `service_id` — номер системного сервиса (из RAX)
/// * `arg1` — первый аргумент (из R10, перенесён из RCX в user stub)
/// * `arg2` — второй аргумент (из RDX)
/// * `arg3` — третий аргумент (из R8)
/// * `arg4` — четвёртый аргумент (из R9)
///
/// # Returns
/// NTSTATUS результат системного вызова (возвращается в RAX)
///
/// # Коды ошибок
/// * `STATUS_INVALID_SYSTEM_SERVICE` — неверный номер сервиса
/// * `STATUS_NOT_IMPLEMENTED` — сервис зарегистрирован как заглушка
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn ki_system_service_dispatch(
    service_id: u32,
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
) -> NTSTATUS {
    unsafe {
        // Проверяем валидность номера сервиса
        if service_id >= KI_SERVICE_TABLE.limit {
            return STATUS_INVALID_SYSTEM_SERVICE;
        }

        // Получаем указатель на функцию
        let service_fn = *KI_SERVICE_TABLE.base.add(service_id as usize);

        // Вызываем функцию сервиса
        let status = service_fn(arg1, arg2, arg3, arg4);
        status
    }
}

/// Диспетчер для kernel-mode вызовов (Zw* API)
///
/// Устанавливает PreviousMode = KernelMode и вызывает диспетчер.
/// Используется для внутренних вызовов ядра к Nt* функциям.
///
/// # Safety
/// Должен вызываться только из kernel mode.
#[inline]
pub unsafe fn zw_system_service_dispatch(
    service_id: u32,
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
) -> NTSTATUS {
    unsafe {
        // Сохраняем текущий режим
        let old_mode = ke_get_previous_mode();

        // Устанавливаем kernel mode
        ke_set_previous_mode(KPROCESSOR_MODE::KernelMode);

        // Вызываем диспетчер
        let status = ki_system_service_dispatch(service_id, arg1, arg2, arg3, arg4);

        // Восстанавливаем режим
        ke_set_previous_mode(old_mode);

        status
    }
}

// =============================================================================
// Wrappers для I/O сервисов
// =============================================================================

// Эти обёртки нужны для преобразования типов аргументов.
// Реальные Nt* функции имеют типизированные аргументы.

/// Wrapper для NtCreateFile
unsafe extern "win64" fn NtCreateFile_wrapper(
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
) -> NTSTATUS {
    // arg1 = FileHandle*
    // arg2 = DesiredAccess
    // arg3 = ObjectAttributes*
    // arg4 = IoStatusBlock*
    // Остальные аргументы на стеке — пока не обрабатываем
    crate::io::file::NtCreateFile(
        arg1 as *mut usize,    // FileHandle
        arg2 as u32,           // DesiredAccess
        arg3 as PVOID,         // ObjectAttributes
        arg4 as PVOID,         // IoStatusBlock
        core::ptr::null(),     // AllocationSize
        0,                     // FileAttributes
        0,                     // ShareAccess
        0,                     // CreateDisposition
        0,                     // CreateOptions
        core::ptr::null_mut(), // EaBuffer
        0,                     // EaLength
    )
}

/// Wrapper для NtOpenFile
unsafe extern "win64" fn NtOpenFile_wrapper(
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
) -> NTSTATUS {
    crate::io::file::NtOpenFile(
        arg1 as *mut usize, // FileHandle
        arg2 as u32,        // DesiredAccess
        arg3 as PVOID,      // ObjectAttributes
        arg4 as PVOID,      // IoStatusBlock
        0,                  // ShareAccess
        0,                  // OpenOptions
    )
}

/// Wrapper для NtReadFile
unsafe extern "win64" fn NtReadFile_wrapper(
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
) -> NTSTATUS {
    crate::io::file::NtReadFile(
        arg1 as usize,         // FileHandle
        arg2 as usize,         // Event
        core::ptr::null(),     // ApcRoutine
        core::ptr::null_mut(), // ApcContext
        arg3 as PVOID,         // IoStatusBlock
        arg4 as PVOID,         // Buffer
        0,                     // Length
        core::ptr::null(),     // ByteOffset
        core::ptr::null(),     // Key
    )
}

/// Wrapper для NtWriteFile
unsafe extern "win64" fn NtWriteFile_wrapper(
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
) -> NTSTATUS {
    crate::io::file::NtWriteFile(
        arg1 as usize,         // FileHandle
        arg2 as usize,         // Event
        core::ptr::null(),     // ApcRoutine
        core::ptr::null_mut(), // ApcContext
        arg3 as PVOID,         // IoStatusBlock
        arg4 as PVOID,         // Buffer
        0,                     // Length
        core::ptr::null(),     // ByteOffset
        core::ptr::null(),     // Key
    )
}

/// Wrapper для NtClose
unsafe extern "win64" fn NtClose_wrapper(
    arg1: u64,
    _arg2: u64,
    _arg3: u64,
    _arg4: u64,
) -> NTSTATUS {
    crate::ob::handle::NtClose(arg1 as usize)
}

/// Wrapper для NtDeviceIoControlFile
unsafe extern "win64" fn NtDeviceIoControlFile_wrapper(
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
) -> NTSTATUS {
    crate::io::file::NtDeviceIoControlFile(
        arg1 as usize,         // FileHandle
        arg2 as usize,         // Event
        core::ptr::null(),     // ApcRoutine
        core::ptr::null_mut(), // ApcContext
        arg3 as PVOID,         // IoStatusBlock
        arg4 as u32,           // IoControlCode
        core::ptr::null_mut(), // InputBuffer
        0,                     // InputBufferLength
        core::ptr::null_mut(), // OutputBuffer
        0,                     // OutputBufferLength
    )
}

/// Wrapper для NtFsControlFile
unsafe extern "win64" fn NtFsControlFile_wrapper(
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
) -> NTSTATUS {
    crate::io::file::NtFsControlFile(
        arg1 as usize,         // FileHandle
        arg2 as usize,         // Event
        core::ptr::null(),     // ApcRoutine
        core::ptr::null_mut(), // ApcContext
        arg3 as PVOID,         // IoStatusBlock
        arg4 as u32,           // FsControlCode
        core::ptr::null_mut(), // InputBuffer
        0,                     // InputBufferLength
        core::ptr::null_mut(), // OutputBuffer
        0,                     // OutputBufferLength
    )
}

/// Wrapper для NtCancelIoFile
unsafe extern "win64" fn NtCancelIoFile_wrapper(
    arg1: u64,
    arg2: u64,
    _arg3: u64,
    _arg4: u64,
) -> NTSTATUS {
    crate::io::file::NtCancelIoFile(
        arg1 as usize, // FileHandle
        arg2 as PVOID, // IoStatusBlock
    )
}

/// Wrapper для NtCancelIoFileEx
unsafe extern "win64" fn NtCancelIoFileEx_wrapper(
    arg1: u64,
    arg2: u64,
    arg3: u64,
    _arg4: u64,
) -> NTSTATUS {
    crate::io::file::NtCancelIoFileEx(
        arg1 as usize, // FileHandle
        arg2 as PVOID, // IoRequestToCancel
        arg3 as PVOID, // IoStatusBlock
    )
}

// =============================================================================
// Wrappers для Memory сервисов
// =============================================================================

/// Wrapper для NtCreateSection
unsafe extern "win64" fn NtCreateSection_wrapper(
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
) -> NTSTATUS {
    crate::mm::section::NtCreateSection(
        arg1 as *mut usize,                                        // SectionHandle
        arg2 as u32,                                               // DesiredAccess
        arg3 as *const crate::ob::types::OBJECT_ATTRIBUTES,        // ObjectAttributes
        arg4 as *const crate::nt::LARGE_INTEGER,                   // MaximumSize
        0,                                                         // SectionPageProtection
        0,                                                         // AllocationAttributes
        0,                                                         // FileHandle
    )
}

/// Wrapper для NtMapViewOfSection
unsafe extern "win64" fn NtMapViewOfSection_wrapper(
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
) -> NTSTATUS {
    crate::mm::section::NtMapViewOfSection(
        arg1 as usize,         // SectionHandle
        arg2 as usize,         // ProcessHandle
        arg3 as *mut PVOID,    // BaseAddress
        arg4 as usize,         // ZeroBits
        0,                     // CommitSize
        core::ptr::null_mut(), // SectionOffset
        core::ptr::null_mut(), // ViewSize
        0,                     // InheritDisposition
        0,                     // AllocationType
        0,                     // Win32Protect
    )
}

/// Wrapper для NtUnmapViewOfSection
unsafe extern "win64" fn NtUnmapViewOfSection_wrapper(
    arg1: u64,
    arg2: u64,
    _arg3: u64,
    _arg4: u64,
) -> NTSTATUS {
    crate::mm::section::NtUnmapViewOfSection(
        arg1 as usize, // ProcessHandle
        arg2 as PVOID, // BaseAddress
    )
}

// =============================================================================
// PS (Process/Thread) Wrappers
// =============================================================================

/// Wrapper для NtTerminateThread
///
/// arg1 = ThreadHandle
/// arg2 = ExitStatus
unsafe extern "win64" fn NtTerminateThread_wrapper(
    arg1: u64,
    arg2: u64,
    _arg3: u64,
    _arg4: u64,
) -> NTSTATUS {
    unsafe {
        crate::ps::NtTerminateThread(
            arg1 as usize,    // ThreadHandle
            arg2 as NTSTATUS, // ExitStatus
        )
    }
}

/// Wrapper для NtTerminateProcess
///
/// arg1 = ProcessHandle
/// arg2 = ExitStatus
unsafe extern "win64" fn NtTerminateProcess_wrapper(
    arg1: u64,
    arg2: u64,
    _arg3: u64,
    _arg4: u64,
) -> NTSTATUS {
    unsafe {
        crate::ps::NtTerminateProcess(
            arg1 as usize,    // ProcessHandle
            arg2 as NTSTATUS, // ExitStatus
        )
    }
}

/// Wrapper для NtCreateThread
///
/// arg1 = ThreadHandle (out)
/// arg2 = DesiredAccess
/// arg3 = ObjectAttributes
/// arg4 = ProcessHandle
/// Остальные аргументы на стеке (не поддерживаются в текущей реализации)
unsafe extern "win64" fn NtCreateThread_wrapper(
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
) -> NTSTATUS {
    unsafe {
        // NtCreateThread требует 8 аргументов, но текущий диспетчер поддерживает только 4
        // Передаём что можем, остальное NULL
        crate::ps::NtCreateThread(
            arg1 as *mut usize,    // ThreadHandle
            arg2 as u32,           // DesiredAccess
            arg3 as PVOID,         // ObjectAttributes
            arg4 as usize,         // ProcessHandle
            core::ptr::null_mut(), // ClientId (не передаётся)
            core::ptr::null_mut(), // ThreadContext (не передаётся)
            core::ptr::null_mut(), // InitialTeb (не передаётся)
            0,                     // CreateSuspended
        )
    }
}

// =============================================================================
// Проверка указателей (ProbeForRead/ProbeForWrite)
// =============================================================================

/// Минимально допустимый адрес user space
const MM_USER_PROBE_ADDRESS: u64 = 0x0000_7FFF_FFFF_0000;

/// Максимальный размер для probe
const MM_MAX_PROBE_SIZE: usize = 0x7FFF_0000;

/// ProbeForRead — проверяет указатель на чтение
///
/// Выполняет базовую проверку user-mode указателя:
/// - Не NULL
/// - В пределах user space (< MM_USER_PROBE_ADDRESS)
/// - Выровнен
///
/// # Arguments
/// * `address` — адрес для проверки
/// * `length` — длина данных
/// * `alignment` — требуемое выравнивание (1, 2, 4, 8)
///
/// # Returns
/// * `Ok(())` — указатель валиден
/// * `Err(STATUS_ACCESS_VIOLATION)` — невалидный указатель
///
/// TODO: ВАЖНО: Это упрощённая проверка. Полноценный fault-safe copy
/// требует поддержки MM (обработка page fault при копировании).
#[inline]
pub unsafe fn probe_for_read(
    address: PVOID,
    length: usize,
    alignment: usize,
) -> Result<(), NTSTATUS> {
    unsafe {
        // Проверяем что вызов из user mode
        if ke_get_previous_mode() == KPROCESSOR_MODE::KernelMode {
            return Ok(()); // Kernel mode — не проверяем
        }

        let addr = address as u64;

        // NULL check
        if addr == 0 {
            return Err(STATUS_ACCESS_VIOLATION);
        }

        // Alignment check
        if alignment > 1 && (addr & (alignment as u64 - 1)) != 0 {
            return Err(STATUS_ACCESS_VIOLATION);
        }

        // Overflow check
        if length > MM_MAX_PROBE_SIZE {
            return Err(STATUS_ACCESS_VIOLATION);
        }

        // User space bounds check
        let end_addr = addr.saturating_add(length as u64);
        if end_addr > MM_USER_PROBE_ADDRESS {
            return Err(STATUS_ACCESS_VIOLATION);
        }

        Ok(())
    }
}

/// ProbeForWrite — проверяет указатель на запись
///
/// TODO: Аналогично ProbeForRead, но для записи.
/// В полной реализации также проверяет что страница writable.
#[inline]
pub unsafe fn probe_for_write(
    address: PVOID,
    length: usize,
    alignment: usize,
) -> Result<(), NTSTATUS> {
    unsafe {
        // Пока используем ту же логику что и для чтения
        probe_for_read(address, length, alignment)
    }
}

// =============================================================================
// Инициализация SYSCALL MSRs
// =============================================================================

/// Инициализирует SYSCALL MSRs для текущего процессора
///
/// Настраивает:
/// - IA32_EFER.SCE — включает SYSCALL/SYSRET
/// - IA32_STAR — CS селекторы для SYSCALL/SYSRET
/// - IA32_LSTAR — адрес KiSystemCall64
/// - IA32_FMASK — маска флагов (очищает IF и TF при SYSCALL)
///
/// # Safety
/// Должна вызываться при инициализации каждого процессора.
pub unsafe fn ki_initialize_syscall() {
    unsafe {
        use crate::arch::x86_64::gdt::selector;
        use crate::arch::x86_64::msr::efer;
        use crate::arch::x86_64::msr::msr_addr;
        use crate::arch::x86_64::msr::rdmsr;
        use crate::arch::x86_64::msr::wrmsr;

        // 1. Включаем SCE (System Call Extensions) в EFER
        let efer_value = rdmsr(msr_addr::IA32_EFER);
        wrmsr(msr_addr::IA32_EFER, efer_value | efer::SCE);

        // 2. Настраиваем STAR
        // STAR[47:32] = SYSCALL CS (kernel)
        // STAR[63:48] = SYSRET CS base (user, +16 для CS, +8 для SS)
        //
        // При SYSCALL: CS = STAR[47:32], SS = STAR[47:32] + 8
        // При SYSRET:  CS = STAR[63:48] + 16, SS = STAR[63:48] + 8
        //
        // Для XenOS GDT:
        // - KGDT64_R0_CODE (0x10) — kernel CS
        // - KGDT64_R3_DATA (0x28) — база для user mode
        //   При SYSRET: CS = 0x28 + 16 = 0x38 (должен быть KGDT64_R3_CODE = 0x30... корректируем)
        //
        // Корректный расчёт для NT GDT layout:
        // SYSRET base = 0x20 (KGDT64_R3_CMCODE - 16)
        // SYSRET CS = 0x20 + 16 = 0x30 (KGDT64_R3_CODE) ✓
        // SYSRET SS = 0x20 + 8 = 0x28 (KGDT64_R3_DATA) ✓

        let syscall_cs = selector::KGDT64_R0_CODE as u64; // 0x10
        let sysret_base = 0x20u64; // KGDT64_R3_CMCODE

        let star_value = (sysret_base << 48) | (syscall_cs << 32);
        wrmsr(msr_addr::IA32_STAR, star_value);

        // 3. Настраиваем LSTAR — адрес KiSystemCall64
        let syscall_entry = super::syscall_asm::get_syscall_entry_point();
        wrmsr(msr_addr::IA32_LSTAR, syscall_entry);

        // 4. Настраиваем CSTAR — для compatibility mode (32-bit)
        // Пока используем тот же обработчик (или можно установить заглушку)
        wrmsr(msr_addr::IA32_CSTAR, syscall_entry);

        // 5. Настраиваем FMASK — маска флагов при SYSCALL
        // Очищаем IF (0x200) и TF (0x100) чтобы:
        // - Отключить прерывания при входе в ядро
        // - Отключить single-step для предотвращения утечки информации
        let fmask = 0x200 | 0x100; // IF | TF
        wrmsr(msr_addr::IA32_FMASK, fmask);
    }
}

/// Проверяет инициализирован ли SYSCALL для текущего процессора
pub unsafe fn ki_is_syscall_initialized() -> bool {
    unsafe {
        use crate::arch::x86_64::msr::efer;
        use crate::arch::x86_64::msr::msr_addr;
        use crate::arch::x86_64::msr::rdmsr;

        let efer_value = rdmsr(msr_addr::IA32_EFER);
        (efer_value & efer::SCE) != 0
    }
}
