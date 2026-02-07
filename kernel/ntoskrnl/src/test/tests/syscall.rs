//! Тесты подсистемы системных вызовов
//!
//! Тестирует:
//! - Диспетчер syscall (ki_system_service_dispatch)
//! - Таблица сервисов (KSERVICE_TABLE_DESCRIPTOR)
//! - Модель PreviousMode
//! - Проверку указателей (ProbeForRead/Write)
//! - Конкретные syscall заглушки

use super::super::harness::KernelTest;
use crate::ke::syscall::*;
use crate::nt::ntstatus::*;

// =============================================================================
// Тесты диспетчера
// =============================================================================

/// Тест: невалидный номер сервиса возвращает STATUS_INVALID_SYSTEM_SERVICE
fn test_invalid_service_id() {
    unsafe {
        // Инициализируем таблицу если ещё не инициализирована
        ki_initialize_system_service_table();

        // Вызываем с невалидным номером (больше MAX_SERVICE_NUMBER)
        let status = ki_system_service_dispatch(0xFFFF, 0, 0, 0, 0);

        assert_eq!(
            status, STATUS_INVALID_SYSTEM_SERVICE,
            "Ожидается STATUS_INVALID_SYSTEM_SERVICE для невалидного service_id"
        );
    }
}

/// Тест: нереализованный сервис возвращает STATUS_NOT_IMPLEMENTED
fn test_not_implemented_service() {
    unsafe {
        ki_initialize_system_service_table();

        // Вызываем NtCreateFile (0x00) - это заглушка
        let status = ki_system_service_dispatch(
            service_numbers::NT_CREATE_FILE,
            0, // FileHandle (NULL - вызовет INVALID_PARAMETER)
            0, // DesiredAccess
            0, // ObjectAttributes
            0, // IoStatusBlock (NULL - вызовет INVALID_PARAMETER)
        );

        // Так как FileHandle и IoStatusBlock NULL, ожидаем INVALID_PARAMETER
        assert_eq!(
            status, STATUS_INVALID_PARAMETER,
            "Ожидается STATUS_INVALID_PARAMETER для NULL обязательных параметров"
        );
    }
}

/// Тест: NtCreateFile с NULL ObjectAttributes возвращает INVALID_PARAMETER
fn test_stub_service_returns_not_implemented() {
    unsafe {
        ki_initialize_system_service_table();

        // Создаём фиктивные буферы на стеке
        let mut file_handle: usize = 0;
        let mut io_status = crate::io::file::IO_STATUS_BLOCK::new();

        // Вызываем NtCreateFile с валидными указателями, но NULL ObjectAttributes
        let status = ki_system_service_dispatch(
            service_numbers::NT_CREATE_FILE,
            &mut file_handle as *mut usize as u64,
            0, // DesiredAccess
            0, // ObjectAttributes (NULL - теперь вызывает INVALID_PARAMETER)
            &mut io_status as *mut _ as u64,
        );

        // NtCreateFile реализован: NULL ObjectAttributes возвращает INVALID_PARAMETER
        assert_eq!(
            status, STATUS_INVALID_PARAMETER,
            "Ожидается STATUS_INVALID_PARAMETER для NtCreateFile с NULL ObjectAttributes"
        );
    }
}

// =============================================================================
// Тесты PreviousMode
// =============================================================================

/// Тест: ke_get_previous_mode возвращает KernelMode по умолчанию
fn test_previous_mode_default() {
    unsafe {
        // В kernel-mode тестах PreviousMode должен быть KernelMode
        let mode = ke_get_previous_mode();

        assert_eq!(
            mode,
            KPROCESSOR_MODE::KernelMode,
            "По умолчанию PreviousMode должен быть KernelMode"
        );
    }
}

/// Тест: ke_set_previous_mode корректно меняет режим
fn test_previous_mode_set() {
    unsafe {
        // Сохраняем текущий режим
        let original_mode = ke_get_previous_mode();

        // Устанавливаем UserMode
        ke_set_previous_mode(KPROCESSOR_MODE::UserMode);
        assert_eq!(
            ke_get_previous_mode(),
            KPROCESSOR_MODE::UserMode,
            "После set должен быть UserMode"
        );

        // Устанавливаем KernelMode обратно
        ke_set_previous_mode(KPROCESSOR_MODE::KernelMode);
        assert_eq!(
            ke_get_previous_mode(),
            KPROCESSOR_MODE::KernelMode,
            "После set должен быть KernelMode"
        );

        // Восстанавливаем оригинальный режим
        ke_set_previous_mode(original_mode);
    }
}

/// Тест: zw_system_service_dispatch вызывает с KernelMode
fn test_zw_dispatch_kernel_mode() {
    unsafe {
        ki_initialize_system_service_table();

        // Сохраняем текущий режим
        let original_mode = ke_get_previous_mode();

        // Устанавливаем UserMode
        ke_set_previous_mode(KPROCESSOR_MODE::UserMode);

        // zw_* должен временно переключить в KernelMode
        // Используем невалидный сервис чтобы просто проверить что вызов проходит
        let status = zw_system_service_dispatch(0xFFFF, 0, 0, 0, 0);

        // Должен вернуть INVALID_SYSTEM_SERVICE
        assert_eq!(status, STATUS_INVALID_SYSTEM_SERVICE);

        // PreviousMode должен быть восстановлен в UserMode
        assert_eq!(
            ke_get_previous_mode(),
            KPROCESSOR_MODE::UserMode,
            "zw_* должен восстановить PreviousMode после вызова"
        );

        // Восстанавливаем оригинальный режим
        ke_set_previous_mode(original_mode);
    }
}

// =============================================================================
// Тесты проверки указателей
// =============================================================================

/// Тест: probe_for_read с NULL возвращает ACCESS_VIOLATION в UserMode
fn test_probe_for_read_null() {
    unsafe {
        let original_mode = ke_get_previous_mode();

        // Устанавливаем UserMode для проверки
        ke_set_previous_mode(KPROCESSOR_MODE::UserMode);

        let result = probe_for_read(core::ptr::null_mut(), 8, 1);
        assert!(result.is_err(), "NULL указатель должен вызвать ошибку");
        assert_eq!(result.unwrap_err(), STATUS_ACCESS_VIOLATION);

        ke_set_previous_mode(original_mode);
    }
}

/// Тест: probe_for_read в KernelMode пропускает проверку
fn test_probe_for_read_kernel_mode_skip() {
    unsafe {
        let original_mode = ke_get_previous_mode();

        // Устанавливаем KernelMode
        ke_set_previous_mode(KPROCESSOR_MODE::KernelMode);

        // В KernelMode даже NULL указатель "проходит" (не проверяется)
        let result = probe_for_read(core::ptr::null_mut(), 8, 1);
        assert!(result.is_ok(), "В KernelMode проверка должна пропускаться");

        ke_set_previous_mode(original_mode);
    }
}

/// Тест: probe_for_read с kernel address возвращает ACCESS_VIOLATION
fn test_probe_for_read_kernel_address() {
    unsafe {
        let original_mode = ke_get_previous_mode();

        ke_set_previous_mode(KPROCESSOR_MODE::UserMode);

        // Адрес в kernel space (выше MM_USER_PROBE_ADDRESS)
        let kernel_addr = 0xFFFF_8000_0000_0000u64 as *mut core::ffi::c_void;
        let result = probe_for_read(kernel_addr, 8, 1);
        assert!(result.is_err(), "Kernel address должен вызвать ошибку");
        assert_eq!(result.unwrap_err(), STATUS_ACCESS_VIOLATION);

        ke_set_previous_mode(original_mode);
    }
}

/// Тест: probe_for_read с невыровненным адресом возвращает ACCESS_VIOLATION
fn test_probe_for_read_misaligned() {
    unsafe {
        let original_mode = ke_get_previous_mode();

        ke_set_previous_mode(KPROCESSOR_MODE::UserMode);

        // Невыровненный адрес (требуется выравнивание на 8)
        let misaligned_addr = 0x1001u64 as *mut core::ffi::c_void;
        let result = probe_for_read(misaligned_addr, 8, 8);
        assert!(result.is_err(), "Невыровненный адрес должен вызвать ошибку");
        assert_eq!(result.unwrap_err(), STATUS_ACCESS_VIOLATION);

        ke_set_previous_mode(original_mode);
    }
}

// =============================================================================
// Тесты probe_for_write
// =============================================================================

/// Тест: probe_for_write с NULL возвращает ACCESS_VIOLATION в UserMode
fn test_probe_for_write_null() {
    unsafe {
        let original_mode = ke_get_previous_mode();

        ke_set_previous_mode(KPROCESSOR_MODE::UserMode);

        let result = probe_for_write(core::ptr::null_mut(), 8, 1);
        assert!(result.is_err(), "NULL указатель должен вызвать ошибку");
        assert_eq!(result.unwrap_err(), STATUS_ACCESS_VIOLATION);

        ke_set_previous_mode(original_mode);
    }
}

/// Тест: probe_for_write в KernelMode пропускает проверку
fn test_probe_for_write_kernel_mode_skip() {
    unsafe {
        let original_mode = ke_get_previous_mode();

        ke_set_previous_mode(KPROCESSOR_MODE::KernelMode);

        // В KernelMode даже NULL указатель "проходит"
        let result = probe_for_write(core::ptr::null_mut(), 8, 1);
        assert!(result.is_ok(), "В KernelMode проверка должна пропускаться");

        ke_set_previous_mode(original_mode);
    }
}

/// Тест: probe_for_write с kernel address возвращает ACCESS_VIOLATION
fn test_probe_for_write_kernel_address() {
    unsafe {
        let original_mode = ke_get_previous_mode();

        ke_set_previous_mode(KPROCESSOR_MODE::UserMode);

        let kernel_addr = 0xFFFF_8000_0000_0000u64 as *mut core::ffi::c_void;
        let result = probe_for_write(kernel_addr, 8, 1);
        assert!(result.is_err(), "Kernel address должен вызвать ошибку");
        assert_eq!(result.unwrap_err(), STATUS_ACCESS_VIOLATION);

        ke_set_previous_mode(original_mode);
    }
}

// =============================================================================
// Тесты таблицы сервисов
// =============================================================================

/// Тест: таблица сервисов имеет корректный лимит
fn test_service_table_limit() {
    unsafe {
        ki_initialize_system_service_table();

        // Проверяем что лимит > 0
        let limit = KI_SERVICE_TABLE.limit;
        assert!(limit > 0, "Лимит таблицы сервисов должен быть > 0");

        // Проверяем что лимит не превышает MAX_SERVICE_NUMBER
        assert!(
            limit <= service_numbers::MAX_SERVICE_NUMBER,
            "Лимит не должен превышать MAX_SERVICE_NUMBER"
        );
    }
}

/// Тест: все I/O сервисы зарегистрированы
fn test_io_services_registered() {
    unsafe {
        ki_initialize_system_service_table();

        // Проверяем что I/O сервисы в пределах лимита
        assert!(
            service_numbers::NT_CREATE_FILE < KI_SERVICE_TABLE.limit,
            "NtCreateFile должен быть в пределах таблицы"
        );
        assert!(
            service_numbers::NT_OPEN_FILE < KI_SERVICE_TABLE.limit,
            "NtOpenFile должен быть в пределах таблицы"
        );
        assert!(
            service_numbers::NT_READ_FILE < KI_SERVICE_TABLE.limit,
            "NtReadFile должен быть в пределах таблицы"
        );
        assert!(
            service_numbers::NT_WRITE_FILE < KI_SERVICE_TABLE.limit,
            "NtWriteFile должен быть в пределах таблицы"
        );
        assert!(
            service_numbers::NT_CLOSE < KI_SERVICE_TABLE.limit,
            "NtClose должен быть в пределах таблицы"
        );
    }
}

/// Тест: базовый адрес таблицы не NULL
fn test_service_table_base_not_null() {
    unsafe {
        ki_initialize_system_service_table();

        assert!(
            !KI_SERVICE_TABLE.base.is_null(),
            "Базовый адрес таблицы сервисов не должен быть NULL"
        );
    }
}

// =============================================================================
// Тесты конкретных syscall заглушек
// =============================================================================

/// Тест: NtClose с невалидным handle возвращает INVALID_HANDLE
fn test_nt_close_invalid_handle() {
    unsafe {
        ki_initialize_system_service_table();

        // Вызываем NtClose с невалидным handle (0xDEADBEEF)
        let status = ki_system_service_dispatch(
            service_numbers::NT_CLOSE,
            0xDEADBEEF, // Невалидный handle
            0,
            0,
            0,
        );

        // Ожидаем INVALID_HANDLE (handle не существует в таблице)
        assert_eq!(
            status, STATUS_INVALID_HANDLE,
            "NtClose с невалидным handle должен вернуть STATUS_INVALID_HANDLE"
        );
    }
}

/// Тест: NtReadFile с NULL IoStatusBlock возвращает INVALID_PARAMETER
fn test_nt_read_file_null_iostatus() {
    unsafe {
        ki_initialize_system_service_table();

        let status = ki_system_service_dispatch(
            service_numbers::NT_READ_FILE,
            0, // FileHandle
            0, // Event
            0, // ApcRoutine
            0, // IoStatusBlock (NULL)
        );

        assert_eq!(
            status, STATUS_INVALID_PARAMETER,
            "NtReadFile с NULL IoStatusBlock должен вернуть INVALID_PARAMETER"
        );
    }
}

/// Тест: NtWriteFile с NULL IoStatusBlock возвращает INVALID_PARAMETER
fn test_nt_write_file_null_iostatus() {
    unsafe {
        ki_initialize_system_service_table();

        let status = ki_system_service_dispatch(
            service_numbers::NT_WRITE_FILE,
            0, // FileHandle
            0, // Event
            0, // ApcRoutine
            0, // IoStatusBlock (NULL)
        );

        assert_eq!(
            status, STATUS_INVALID_PARAMETER,
            "NtWriteFile с NULL IoStatusBlock должен вернуть INVALID_PARAMETER"
        );
    }
}

/// Тест: NtDeviceIoControlFile с NULL IoStatusBlock возвращает INVALID_PARAMETER
fn test_nt_device_ioctl_null_iostatus() {
    unsafe {
        ki_initialize_system_service_table();

        let status = ki_system_service_dispatch(
            service_numbers::NT_DEVICE_IO_CONTROL_FILE,
            0, // FileHandle
            0, // Event
            0, // ApcRoutine
            0, // IoStatusBlock (NULL)
        );

        assert_eq!(
            status, STATUS_INVALID_PARAMETER,
            "NtDeviceIoControlFile с NULL IoStatusBlock должен вернуть INVALID_PARAMETER"
        );
    }
}

// =============================================================================
// Тесты граничных значений
// =============================================================================

/// Тест: probe_for_read с адресом на границе user space
fn test_probe_for_read_user_space_boundary() {
    unsafe {
        let original_mode = ke_get_previous_mode();

        ke_set_previous_mode(KPROCESSOR_MODE::UserMode);

        // Адрес чуть ниже границы kernel space (должен пройти проверку диапазона)
        // MM_USER_PROBE_ADDRESS = 0x7FFF_FFFF_0000
        let boundary_addr = 0x7FFF_FFFE_0000u64 as *mut core::ffi::c_void;
        let result = probe_for_read(boundary_addr, 8, 1);
        // Этот адрес в user space, проверка диапазона должна пройти
        assert!(
            result.is_ok(),
            "Адрес в user space должен пройти проверку диапазона"
        );

        ke_set_previous_mode(original_mode);
    }
}

/// Тест: probe_for_read с размером 0
fn test_probe_for_read_zero_size() {
    unsafe {
        let original_mode = ke_get_previous_mode();

        ke_set_previous_mode(KPROCESSOR_MODE::UserMode);

        // Проверка с размером 0 должна пройти (нечего проверять)
        let addr = 0x1000u64 as *mut core::ffi::c_void;
        let result = probe_for_read(addr, 0, 1);
        assert!(result.is_ok(), "Проверка с размером 0 должна пройти");

        ke_set_previous_mode(original_mode);
    }
}

/// Тест: probe_for_read с выравниванием 1 (без требований)
fn test_probe_for_read_alignment_one() {
    unsafe {
        let original_mode = ke_get_previous_mode();

        ke_set_previous_mode(KPROCESSOR_MODE::UserMode);

        // Любой адрес должен пройти проверку выравнивания при alignment=1
        let odd_addr = 0x1001u64 as *mut core::ffi::c_void;
        let result = probe_for_read(odd_addr, 8, 1);
        assert!(result.is_ok(), "Выравнивание 1 не должно отклонять адреса");

        ke_set_previous_mode(original_mode);
    }
}

// =============================================================================
// Тесты MSR инициализации
// =============================================================================

/// Тест: ki_is_syscall_initialized работает
fn test_syscall_initialized_check() {
    unsafe {
        // Просто проверяем что функция вызывается без падения
        let _initialized = ki_is_syscall_initialized();
        // Не проверяем конкретное значение - зависит от фазы инициализации
    }
}

/// Тест: повторная инициализация таблицы сервисов безопасна
fn test_service_table_reinit_safe() {
    unsafe {
        // Первая инициализация
        ki_initialize_system_service_table();
        let limit1 = KI_SERVICE_TABLE.limit;

        // Повторная инициализация
        ki_initialize_system_service_table();
        let limit2 = KI_SERVICE_TABLE.limit;

        // Лимит должен остаться тем же
        assert_eq!(
            limit1, limit2,
            "Повторная инициализация не должна менять таблицу"
        );
    }
}

// =============================================================================
// Тесты ASM ABI (эмуляция вызовов как из ntdll.dll)
// =============================================================================

/// Тестовый wrapper для вызова диспетчера через ASM с syscall ABI
///
/// Эмулирует то, как ntdll.dll будет вызывать syscall:
/// - RAX = номер сервиса
/// - R10 = arg1 (перемещён из RCX в ntdll stub)
/// - RDX = arg2
/// - R8  = arg3
/// - R9  = arg4
///
/// Примечание: реальный `syscall` из kernel mode невозможен,
/// поэтому мы эмулируем только ABI, вызывая диспетчер напрямую.
#[inline(never)]
unsafe fn syscall_asm_test(
    service_id: u32,
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
) -> NTSTATUS {
    let result: i32;

    // Эмулируем syscall ABI:
    // В реальном ntdll: mov r10, rcx; mov eax, <id>; syscall
    // Мы формируем регистры так же, но вызываем ki_system_service_dispatch
    core::arch::asm!(
        // Сохраняем callee-saved регистры которые портим
        "push rbx",
        "push rbp",

        // Формируем syscall ABI в регистрах:
        // eax = service_id (уже в ecx, перемещаем)
        // r10 = arg1 (уже в rdx при вызове этой функции)
        // rdx = arg2
        // r8 = arg3
        // r9 = arg4

        // Аргументы уже в правильных регистрах благодаря Win64 ABI:
        // ecx = service_id, rdx = arg1, r8 = arg2, r9 = arg3
        // arg4 на стеке [rsp + 0x28]

        // Перемещаем согласно syscall ABI -> Win64 ABI для вызова диспетчера:
        // ki_system_service_dispatch(service_id, arg1, arg2, arg3, arg4)
        // ecx = service_id
        // rdx = arg1
        // r8 = arg2
        // r9 = arg3
        // [rsp+0x20] = arg4

        // arg4 уже на стеке после push'ей, но нужен shadow space
        "sub rsp, 0x28",           // shadow space (32 bytes) + alignment
        "mov [rsp + 0x20], r10",   // arg4 (был передан в r10 из-за перестановки)

        // Вызываем диспетчер
        "call {dispatch}",

        "add rsp, 0x28",

        "pop rbp",
        "pop rbx",

        dispatch = sym ki_system_service_dispatch,
        // Входные регистры согласно Win64 ABI вызова этой функции:
        in("ecx") service_id,
        in("rdx") arg1,
        in("r8") arg2,
        in("r9") arg3,
        in("r10") arg4,  // временно через r10
        lateout("eax") result,
        // Clobbers
        out("r11") _,
        clobber_abi("win64"),
    );

    result
}

/// Тест: ASM вызов с невалидным service_id
fn test_asm_invalid_service_id() {
    unsafe {
        ki_initialize_system_service_table();

        let status = syscall_asm_test(0xFFFF, 0, 0, 0, 0);

        assert_eq!(
            status, STATUS_INVALID_SYSTEM_SERVICE,
            "ASM: невалидный service_id должен вернуть INVALID_SYSTEM_SERVICE"
        );
    }
}

/// Тест: ASM вызов NtCreateFile с NULL параметрами
fn test_asm_nt_create_file_null() {
    unsafe {
        ki_initialize_system_service_table();

        let status = syscall_asm_test(
            service_numbers::NT_CREATE_FILE,
            0, // FileHandle (NULL)
            0, // DesiredAccess
            0, // ObjectAttributes
            0, // IoStatusBlock (NULL)
        );

        assert_eq!(
            status, STATUS_INVALID_PARAMETER,
            "ASM: NtCreateFile с NULL должен вернуть INVALID_PARAMETER"
        );
    }
}

/// Тест: ASM вызов NtClose с невалидным handle
fn test_asm_nt_close_invalid() {
    unsafe {
        ki_initialize_system_service_table();

        let status = syscall_asm_test(
            service_numbers::NT_CLOSE,
            0xDEADBEEF, // Невалидный handle
            0,
            0,
            0,
        );

        assert_eq!(
            status, STATUS_INVALID_HANDLE,
            "ASM: NtClose с невалидным handle должен вернуть INVALID_HANDLE"
        );
    }
}

/// Тест: ASM вызов NtCreateFile с NULL ObjectAttributes
fn test_asm_stub_not_implemented() {
    unsafe {
        ki_initialize_system_service_table();

        let mut file_handle: u64 = 0;
        let mut io_status = crate::io::file::IO_STATUS_BLOCK::new();

        let status = syscall_asm_test(
            service_numbers::NT_CREATE_FILE,
            &mut file_handle as *mut u64 as u64,
            0, // DesiredAccess
            0, // ObjectAttributes (NULL)
            &mut io_status as *mut _ as u64,
        );

        // NtCreateFile реализован: NULL ObjectAttributes -> INVALID_PARAMETER
        assert_eq!(
            status, STATUS_INVALID_PARAMETER,
            "ASM: NtCreateFile с NULL ObjectAttributes должен вернуть INVALID_PARAMETER"
        );
    }
}

/// Тест: ASM - многократные вызовы подряд (проверка стабильности)
fn test_asm_multiple_calls() {
    unsafe {
        ki_initialize_system_service_table();

        for i in 0..10 {
            let status = syscall_asm_test(0xFFFF, i, 0, 0, 0);
            assert_eq!(
                status, STATUS_INVALID_SYSTEM_SERVICE,
                "ASM: итерация {} должна вернуть INVALID_SYSTEM_SERVICE",
                i
            );
        }
    }
}

/// Тест: ASM вызов NtReadFile
fn test_asm_nt_read_file() {
    unsafe {
        ki_initialize_system_service_table();

        // NULL IoStatusBlock
        let status = syscall_asm_test(
            service_numbers::NT_READ_FILE,
            0, // FileHandle
            0, // Event
            0, // ApcRoutine
            0, // IoStatusBlock (NULL)
        );

        assert_eq!(
            status, STATUS_INVALID_PARAMETER,
            "ASM: NtReadFile с NULL IoStatusBlock должен вернуть INVALID_PARAMETER"
        );
    }
}

// =============================================================================
// Тест полного ntdll-style stub (эмуляция mov r10, rcx; mov eax, id)
// =============================================================================

/// Эмулирует ntdll stub: NtXxx передаёт первый аргумент через RCX,
/// а stub делает `mov r10, rcx` перед `syscall`
#[inline(never)]
unsafe fn ntdll_style_stub(
    service_id: u32,
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
) -> NTSTATUS {
    let result: i32;

    // Эмуляция ntdll stub:
    // NTAPI NtFoo(arg1, arg2, arg3, arg4) {
    //     mov r10, rcx   ; arg1 -> r10
    //     mov eax, <id>
    //     syscall        ; -> ki_system_service_dispatch
    //     ret
    // }
    core::arch::asm!(
        // ntdll делает: mov r10, rcx (arg1 уже в rcx по Win64 ABI)
        "mov r10, rcx",

        // mov eax, service_id (передаём через rsi который не используется Win64)
        "mov eax, esi",

        // Теперь формируем Win64 call к диспетчеру:
        // ecx = service_id
        // rdx = arg1 (из r10)
        // r8 = arg2
        // r9 = arg3
        // [rsp+0x20] = arg4
        "mov ecx, eax",       // service_id
        "mov rdx, r10",       // arg1
        // r8, r9 уже содержат arg2, arg3

        "sub rsp, 0x28",
        "mov [rsp + 0x20], rdi",  // arg4 (передан через rdi)

        "call {dispatch}",

        "add rsp, 0x28",

        dispatch = sym ki_system_service_dispatch,
        in("esi") service_id,     // service_id через esi
        in("rcx") arg1,           // arg1 в rcx (ntdll передаёт так)
        in("r8") arg2,
        in("r9") arg3,
        in("rdi") arg4,           // arg4 через rdi
        lateout("eax") result,
        out("r10") _,
        out("r11") _,
        out("rdx") _,
        clobber_abi("win64"),
    );

    result
}

/// Тест: ntdll-style stub с NtClose
fn test_ntdll_stub_nt_close() {
    unsafe {
        ki_initialize_system_service_table();

        let status = ntdll_style_stub(
            service_numbers::NT_CLOSE,
            0xBADCAFE, // invalid handle (передаётся как arg1 через rcx)
            0,
            0,
            0,
        );

        assert_eq!(
            status, STATUS_INVALID_HANDLE,
            "ntdll-style: NtClose должен вернуть INVALID_HANDLE"
        );
    }
}

/// Тест: ntdll-style stub с NtCreateFile (NULL ObjectAttributes)
fn test_ntdll_stub_nt_create_file() {
    unsafe {
        ki_initialize_system_service_table();

        let mut handle: u64 = 0;
        let mut iostatus = crate::io::file::IO_STATUS_BLOCK::new();

        let status = ntdll_style_stub(
            service_numbers::NT_CREATE_FILE,
            &mut handle as *mut _ as u64,   // FileHandle
            0,                              // DesiredAccess
            0,                              // ObjectAttributes (NULL)
            &mut iostatus as *mut _ as u64, // IoStatusBlock
        );

        // NtCreateFile реализован: NULL ObjectAttributes -> INVALID_PARAMETER
        assert_eq!(
            status, STATUS_INVALID_PARAMETER,
            "ntdll-style: NtCreateFile с NULL ObjectAttributes должен вернуть INVALID_PARAMETER"
        );
    }
}

// =============================================================================
// Реестр тестов
// =============================================================================

pub static SYSCALL_TESTS: &[KernelTest] = &[
    // Тесты диспетчера
    KernelTest {
        name: "invalid_service_id",
        module: "syscall",
        test_fn: test_invalid_service_id,
    },
    KernelTest {
        name: "not_implemented_service",
        module: "syscall",
        test_fn: test_not_implemented_service,
    },
    KernelTest {
        name: "stub_service_returns_not_implemented",
        module: "syscall",
        test_fn: test_stub_service_returns_not_implemented,
    },
    // Тесты PreviousMode
    KernelTest {
        name: "previous_mode_default",
        module: "syscall",
        test_fn: test_previous_mode_default,
    },
    KernelTest {
        name: "previous_mode_set",
        module: "syscall",
        test_fn: test_previous_mode_set,
    },
    KernelTest {
        name: "zw_dispatch_kernel_mode",
        module: "syscall",
        test_fn: test_zw_dispatch_kernel_mode,
    },
    // Тесты probe_for_read
    KernelTest {
        name: "probe_for_read_null",
        module: "syscall",
        test_fn: test_probe_for_read_null,
    },
    KernelTest {
        name: "probe_for_read_kernel_mode_skip",
        module: "syscall",
        test_fn: test_probe_for_read_kernel_mode_skip,
    },
    KernelTest {
        name: "probe_for_read_kernel_address",
        module: "syscall",
        test_fn: test_probe_for_read_kernel_address,
    },
    KernelTest {
        name: "probe_for_read_misaligned",
        module: "syscall",
        test_fn: test_probe_for_read_misaligned,
    },
    // Тесты probe_for_write
    KernelTest {
        name: "probe_for_write_null",
        module: "syscall",
        test_fn: test_probe_for_write_null,
    },
    KernelTest {
        name: "probe_for_write_kernel_mode_skip",
        module: "syscall",
        test_fn: test_probe_for_write_kernel_mode_skip,
    },
    KernelTest {
        name: "probe_for_write_kernel_address",
        module: "syscall",
        test_fn: test_probe_for_write_kernel_address,
    },
    // Тесты таблицы сервисов
    KernelTest {
        name: "service_table_limit",
        module: "syscall",
        test_fn: test_service_table_limit,
    },
    KernelTest {
        name: "io_services_registered",
        module: "syscall",
        test_fn: test_io_services_registered,
    },
    KernelTest {
        name: "service_table_base_not_null",
        module: "syscall",
        test_fn: test_service_table_base_not_null,
    },
    // Тесты конкретных syscalls
    KernelTest {
        name: "nt_close_invalid_handle",
        module: "syscall",
        test_fn: test_nt_close_invalid_handle,
    },
    KernelTest {
        name: "nt_read_file_null_iostatus",
        module: "syscall",
        test_fn: test_nt_read_file_null_iostatus,
    },
    KernelTest {
        name: "nt_write_file_null_iostatus",
        module: "syscall",
        test_fn: test_nt_write_file_null_iostatus,
    },
    KernelTest {
        name: "nt_device_ioctl_null_iostatus",
        module: "syscall",
        test_fn: test_nt_device_ioctl_null_iostatus,
    },
    // Тесты граничных значений
    KernelTest {
        name: "probe_for_read_user_space_boundary",
        module: "syscall",
        test_fn: test_probe_for_read_user_space_boundary,
    },
    KernelTest {
        name: "probe_for_read_zero_size",
        module: "syscall",
        test_fn: test_probe_for_read_zero_size,
    },
    KernelTest {
        name: "probe_for_read_alignment_one",
        module: "syscall",
        test_fn: test_probe_for_read_alignment_one,
    },
    // Тесты инициализации
    KernelTest {
        name: "syscall_initialized_check",
        module: "syscall",
        test_fn: test_syscall_initialized_check,
    },
    KernelTest {
        name: "service_table_reinit_safe",
        module: "syscall",
        test_fn: test_service_table_reinit_safe,
    },
    // Тесты ASM ABI (эмуляция ntdll)
    KernelTest {
        name: "asm_invalid_service_id",
        module: "syscall",
        test_fn: test_asm_invalid_service_id,
    },
    KernelTest {
        name: "asm_nt_create_file_null",
        module: "syscall",
        test_fn: test_asm_nt_create_file_null,
    },
    KernelTest {
        name: "asm_nt_close_invalid",
        module: "syscall",
        test_fn: test_asm_nt_close_invalid,
    },
    KernelTest {
        name: "asm_stub_not_implemented",
        module: "syscall",
        test_fn: test_asm_stub_not_implemented,
    },
    KernelTest {
        name: "asm_multiple_calls",
        module: "syscall",
        test_fn: test_asm_multiple_calls,
    },
    KernelTest {
        name: "asm_nt_read_file",
        module: "syscall",
        test_fn: test_asm_nt_read_file,
    },
    // Тесты ntdll-style stub
    KernelTest {
        name: "ntdll_stub_nt_close",
        module: "syscall",
        test_fn: test_ntdll_stub_nt_close,
    },
    KernelTest {
        name: "ntdll_stub_nt_create_file",
        module: "syscall",
        test_fn: test_ntdll_stub_nt_create_file,
    },
];
