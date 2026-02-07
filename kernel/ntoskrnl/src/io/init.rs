//! I/O Manager Initialization
//!
//! Модуль инициализации I/O Manager. Отвечает за создание типов объектов
//! (Device, Driver, File), директорий в namespace и загрузку boot-драйверов.
//!
//! # Архитектура
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                    I/O Manager Initialization                       │
//! ├─────────────────────────────────────────────────────────────────────┤
//! │                                                                     │
//! │  KiSystemStartup                                                    │
//! │       │                                                            │
//! │       ▼                                                            │
//! │  io_init_system(0)  ─── Phase 0 ───────────────────────────────┐   │
//! │       │                                                        │   │
//! │       │  ┌─────────────────────────────────────────────────┐   │   │
//! │       │  │ 1. iop_init_internal()                          │   │   │
//! │       │  │    - Инициализация локов                        │   │   │
//! │       │  │    - Инициализация списков                      │   │   │
//! │       │  │    - Сброс счётчиков                            │   │   │
//! │       │  │                                                 │   │   │
//! │       │  │ 2. Создание типов объектов:                     │   │   │
//! │       │  │    - IOP_DEVICE_OBJECT_TYPE ("Device")          │   │   │
//! │       │  │    - IOP_DRIVER_OBJECT_TYPE ("Driver")          │   │   │
//! │       │  │    - IOP_FILE_OBJECT_TYPE ("File")              │   │   │
//! │       │  └─────────────────────────────────────────────────┘   │   │
//! │       │                                                        │   │
//! │       ▼                                                        │   │
//! │  io_init_system(1)  ─── Phase 1 ───────────────────────────┐   │   │
//! │       │                                                    │   │   │
//! │       │  ┌─────────────────────────────────────────────┐   │   │   │
//! │       │  │ 1. iop_create_object_directories()          │   │   │   │
//! │       │  │    - \Device, \Driver, \FileSystem          │   │   │   │
//! │       │  │                                             │   │   │   │
//! │       │  │ 2. TODO: Загрузка boot драйверов            │   │   │   │
//! │       │  │ 3. TODO: Инициализация PnP                  │   │   │   │
//! │       │  └─────────────────────────────────────────────┘   │   │   │
//! │       │                                                    │   │   │
//! │       ▼                                                    │   │   │
//! │  IO_INITIALIZED = true                                     │   │   │
//! │                                                            │   │   │
//! └────────────────────────────────────────────────────────────┴───┴───┘
//! ```
//!
//! # Типы объектов
//!
//! | Тип | Процедуры | Описание |
//! |-----|-----------|----------|
//! | Device | delete | Объект устройства |
//! | Driver | delete (+ Unload) | Объект драйвера |
//! | File | close, delete, parse | Объект файла (waitable) |
//!
//! # Отступления и упрощения
//!
//! - Boot драйверы — не загружаются (требуется boot_drivers_plan)
//! - PnP — не инициализируется
//! - Fast I/O — не поддерживается
//!
//! Источники:
//! - ReactOS: ntoskrnl/io/iomgr/ioinit.c, ntoskrnl/io/iomgr/iomgr.c
//! - NT5: ntos/io/iomgr/init.c

use core::sync::atomic::AtomicBool;
use core::sync::atomic::AtomicU32;
use core::sync::atomic::Ordering;

use crate::nt::NTSTATUS;
use crate::nt::UNICODE_STRING;
use crate::nt::ntstatus::*;
use crate::ob;

// =============================================================================
// I/O Manager State
// =============================================================================

/// I/O Manager инициализирован
static IO_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Фаза инициализации
static IO_INIT_PHASE: AtomicU32 = AtomicU32::new(0);

/// Указатель на тип объекта Device
pub static mut IOP_DEVICE_OBJECT_TYPE: *mut ob::types::OBJECT_TYPE = core::ptr::null_mut();

/// Указатель на тип объекта Driver
pub static mut IOP_DRIVER_OBJECT_TYPE: *mut ob::types::OBJECT_TYPE = core::ptr::null_mut();

/// Указатель на тип объекта File
pub static mut IOP_FILE_OBJECT_TYPE: *mut ob::types::OBJECT_TYPE = core::ptr::null_mut();

/// Указатель на директорию \Device
pub static mut IOP_DEVICE_DIRECTORY: *mut ob::dir::OBJECT_DIRECTORY = core::ptr::null_mut();

/// Указатель на директорию \Driver  
pub static mut IOP_DRIVER_DIRECTORY: *mut ob::dir::OBJECT_DIRECTORY = core::ptr::null_mut();

/// Указатель на директорию \FileSystem
pub static mut IOP_FILESYSTEM_DIRECTORY: *mut ob::dir::OBJECT_DIRECTORY = core::ptr::null_mut();

/// Указатель на директорию \?? (DosDevices)
pub static mut IOP_DOSDEVICES_DIRECTORY: *mut ob::dir::OBJECT_DIRECTORY = core::ptr::null_mut();

// =============================================================================
// IoInitSystem
// =============================================================================

/// IoInitSystem - инициализация I/O Manager
///
/// Вызывается на Phase 0 и Phase 1.
///
/// # Arguments
/// * `phase` - фаза инициализации
///
/// # Returns
/// true если успешно
pub fn io_init_system(phase: u32) -> bool {
    match phase {
        0 => io_init_phase0(),
        1 => io_init_phase1(),
        _ => false,
    }
}

/// Phase 0: Создание типов объектов
fn io_init_phase0() -> bool {
    unsafe {
        // Инициализируем внутренние структуры (локи, списки, счётчики)
        super::iop::iop_init_internal();
        
        // Инициализируем VPB subsystem
        super::vpb::iop_init_vpb();

        // Создаем тип объекта Device
        IOP_DEVICE_OBJECT_TYPE = iop_create_device_object_type();
        if IOP_DEVICE_OBJECT_TYPE.is_null() {
            return false;
        }

        // Создаем тип объекта Driver
        IOP_DRIVER_OBJECT_TYPE = iop_create_driver_object_type();
        if IOP_DRIVER_OBJECT_TYPE.is_null() {
            return false;
        }

        // Создаем тип объекта File
        IOP_FILE_OBJECT_TYPE = iop_create_file_object_type();
        if IOP_FILE_OBJECT_TYPE.is_null() {
            return false;
        }

        IO_INIT_PHASE.store(1, Ordering::Release);
        true
    }
}

/// Phase 1: Инициализация драйверов
fn io_init_phase1() -> bool {
    unsafe {
        // Создаем директорию \Device
        if !iop_create_object_directories() {
            return false;
        }

        // Загрузка boot драйверов
        iop_initialize_boot_drivers();

        // Инициализация PnP Manager
        let status = super::pnpmgr::iop_initialize_plug_play_services();
        if status < 0 {
            crate::kd::dbg_print("   [IO] PnP initialization failed\n");
            // Продолжаем работу даже при ошибке PnP (для отладки)
        }

        IO_INIT_PHASE.store(2, Ordering::Release);
        IO_INITIALIZED.store(true, Ordering::Release);
        true
    }
}

// =============================================================================
// Boot Driver Initialization
// =============================================================================

/// IopInitializeBootDrivers — инициализирует boot драйверы из LoaderBlock
///
/// Итерирует по BootDriverListHead, для каждого драйвера:
/// 1. Создаёт DRIVER_OBJECT
/// 2. Устанавливает driver_start/driver_size из LdrEntry
/// 3. Вызывает DriverEntry
///
/// NT6.1 Reference: IopInitializeBootDrivers в ntos/io/iomgr/ioinit.c
unsafe fn iop_initialize_boot_drivers() {
    use crate::kd::{dbg_print, dbg_print_fail, dbg_print_hex, dbg_print_ok};
    use crate::ke::globals::ke_get_loader_block;

    let lpb = ke_get_loader_block();
    if lpb.is_null() {
        dbg_print("       [WARN] LoaderBlock is NULL, skipping boot drivers\n");
        return;
    }

    let lpb = &*lpb;

    // Проверяем что BootDriverListHead инициализирован
    let head = &lpb.BootDriverListHead as *const ntldr::LIST_ENTRY;
    if (*head).Flink.is_null() || (*head).Flink == head as *mut _ {
        dbg_print("       [INFO] BootDriverListHead is empty\n");
        return;
    }

    // Считаем драйверы
    let mut count = 0u32;
    let mut loaded = 0u32;
    let mut current = (*head).Flink;

    while current != head as *mut _ {
        count += 1;
        current = (*current).Flink;
    }

    dbg_print("       Boot drivers found: ");
    crate::kd::dbg_print_num(count as u64);
    dbg_print("\n");

    // Итерируем и инициализируем каждый драйвер
    current = (*head).Flink;

    while current != head as *mut _ {
        // Получаем BOOT_DRIVER_LIST_ENTRY из LIST_ENTRY
        // Link - первое поле в BOOT_DRIVER_LIST_ENTRY
        let boot_entry = current as *const ntldr::BOOT_DRIVER_LIST_ENTRY;

        // Получаем имя драйвера из RegistryPath (последний компонент пути)
        let reg_path_len = (*boot_entry).RegistryPathLength as usize;
        let reg_path_ptr = core::ptr::addr_of!((*boot_entry).RegistryPath) as *const u8;
        let reg_path_bytes = core::slice::from_raw_parts(reg_path_ptr, reg_path_len);
        let reg_path = core::str::from_utf8(reg_path_bytes).unwrap_or("");

        let driver_name = reg_path.rsplit('\\').next().unwrap_or(reg_path);

        dbg_print("       [");
        crate::kd::dbg_print_num((loaded + 1) as u64);
        dbg_print("/");
        crate::kd::dbg_print_num(count as u64);
        dbg_print("] ");
        dbg_print(driver_name);
        dbg_print("... ");

        // Получаем LdrEntry для информации о модуле
        let ldr_entry = (*boot_entry).LdrEntry;
        if ldr_entry.is_null() {
            dbg_print_fail();
            dbg_print(" (no LdrEntry)\n");
            current = (*current).Flink;
            continue;
        }

        // Получаем entry point
        let entry_point = (*ldr_entry).EntryPoint;
        let dll_base = (*ldr_entry).DllBase;
        let size_of_image = (*ldr_entry).SizeOfImage;

        if entry_point == 0 {
            dbg_print_fail();
            dbg_print(" (no entry point)\n");
            current = (*current).Flink;
            continue;
        }

        // Вызываем iop_load_boot_driver
        match iop_load_boot_driver(driver_name, dll_base, size_of_image, entry_point) {
            Ok(_driver_object) => {
                dbg_print_ok();
                dbg_print("\n");
                loaded += 1;
            }
            Err(status) => {
                dbg_print_fail();
                dbg_print(" (status=0x");
                dbg_print_hex(status as u64);
                dbg_print(")\n");
            }
        }

        current = (*current).Flink;
    }

    dbg_print("       Boot drivers loaded: ");
    crate::kd::dbg_print_num(loaded as u64);
    dbg_print("/");
    crate::kd::dbg_print_num(count as u64);
    dbg_print("\n");
}

/// Тестовая функция: открывает устройство \Device\XenTest через ZwCreateFile
/// и тестирует ZwWriteFile/ZwReadFile
///
/// Проверяет что IRP_MJ_CREATE/READ/WRITE доставляется драйверу.
unsafe fn iop_test_device_open() {
    use crate::kd::{dbg_print, dbg_print_fail, dbg_print_hex, dbg_print_ok};
    use crate::ob::types::OBJECT_ATTRIBUTES;
    use crate::nt::ntdef::UNICODE_STRING;
    use super::file::{iop_create_file, iop_read_write_file, IO_STATUS_BLOCK};
    
    dbg_print("       Testing ZwCreateFile on \\Device\\XenTest... ");
    
    // Имя устройства: \Device\XenTest
    static DEVICE_NAME: [u16; 16] = [
        '\\' as u16, 'D' as u16, 'e' as u16, 'v' as u16, 'i' as u16, 'c' as u16,
        'e' as u16, '\\' as u16, 'X' as u16, 'e' as u16, 'n' as u16, 'T' as u16,
        'e' as u16, 's' as u16, 't' as u16, 0u16,
    ];
    
    let device_name = UNICODE_STRING {
        length: (15 * 2) as u16, // без NUL
        maximum_length: (16 * 2) as u16,
        buffer: DEVICE_NAME.as_ptr() as *mut u16,
    };
    
    let mut obj_attr = OBJECT_ATTRIBUTES::new();
    obj_attr.length = core::mem::size_of::<OBJECT_ATTRIBUTES>() as u32;
    obj_attr.object_name = &device_name as *const _ as *mut _;
    obj_attr.attributes = 0x40; // OBJ_CASE_INSENSITIVE
    
    let mut io_status = IO_STATUS_BLOCK {
        status: 0,
        information: 0,
    };
    
    let mut handle: usize = 0;
    
    // FILE_OPEN = 1, GENERIC_READ | GENERIC_WRITE
    let status = iop_create_file(
        &mut handle,
        0xC0000000,                  // GENERIC_READ | GENERIC_WRITE
        &obj_attr,
        &mut io_status,
        core::ptr::null(),           // allocation_size
        0,                           // file_attributes
        7,                           // share_access (FILE_SHARE_READ|WRITE|DELETE)
        1,                           // create_disposition (FILE_OPEN)
        0,                           // create_options
        core::ptr::null_mut(),       // ea_buffer
        0,                           // ea_length
        0,                           // KernelMode
    );
    
    if status != 0 {
        dbg_print_fail();
        dbg_print(" status=0x");
        dbg_print_hex(status as u64);
        dbg_print("\n");
        return;
    }
    
    dbg_print_ok();
    dbg_print(" handle=0x");
    dbg_print_hex(handle as u64);
    dbg_print("\n");
    
    // Тест ZwWriteFile
    dbg_print("       Testing ZwWriteFile... ");
    
    static TEST_DATA: [u8; 12] = *b"Hello XenOS!";
    io_status.status = 0;
    io_status.information = 0;
    
    let write_status = iop_read_write_file(
        handle,
        0,                           // event
        core::ptr::null(),           // apc_routine
        core::ptr::null_mut(),       // apc_context
        &mut io_status,
        TEST_DATA.as_ptr() as *mut _,
        TEST_DATA.len() as u32,
        core::ptr::null(),           // byte_offset
        core::ptr::null(),           // key
        true,                        // is_write
        0,                           // KernelMode
    );
    
    if write_status == 0 {
        dbg_print_ok();
        dbg_print(" wrote ");
        crate::kd::dbg_print_num(io_status.information as u64);
        dbg_print(" bytes\n");
    } else {
        dbg_print_fail();
        dbg_print(" status=0x");
        dbg_print_hex(write_status as u64);
        dbg_print("\n");
    }
    
    // Тест ZwReadFile
    dbg_print("       Testing ZwReadFile... ");
    
    let mut read_buffer: [u8; 32] = [0u8; 32];
    io_status.status = 0;
    io_status.information = 0;
    
    let read_status = iop_read_write_file(
        handle,
        0,                           // event
        core::ptr::null(),           // apc_routine
        core::ptr::null_mut(),       // apc_context
        &mut io_status,
        read_buffer.as_mut_ptr() as *mut _,
        read_buffer.len() as u32,
        core::ptr::null(),           // byte_offset
        core::ptr::null(),           // key
        false,                       // is_write = false (READ)
        0,                           // KernelMode
    );
    
    if read_status == 0 {
        dbg_print_ok();
        dbg_print(" read ");
        crate::kd::dbg_print_num(io_status.information as u64);
        dbg_print(" bytes\n");
    } else {
        dbg_print_fail();
        dbg_print(" status=0x");
        dbg_print_hex(read_status as u64);
        dbg_print("\n");
    }
    
    // Закрываем handle
    crate::ob::handle::NtClose(handle);
    
    // =========================================================================
    // Тест C драйвера: \Device\XenTestC
    // =========================================================================
    
    dbg_print("       Testing ZwCreateFile on \\Device\\XenTestC... ");
    
    // Имя устройства: \Device\XenTestC
    static DEVICE_NAME_C: [u16; 17] = [
        '\\' as u16, 'D' as u16, 'e' as u16, 'v' as u16, 'i' as u16, 'c' as u16,
        'e' as u16, '\\' as u16, 'X' as u16, 'e' as u16, 'n' as u16, 'T' as u16,
        'e' as u16, 's' as u16, 't' as u16, 'C' as u16, 0u16,
    ];
    
    let device_name_c = UNICODE_STRING {
        length: (16 * 2) as u16, // без NUL
        maximum_length: (17 * 2) as u16,
        buffer: DEVICE_NAME_C.as_ptr() as *mut u16,
    };
    
    let mut obj_attr_c = OBJECT_ATTRIBUTES::new();
    obj_attr_c.length = core::mem::size_of::<OBJECT_ATTRIBUTES>() as u32;
    obj_attr_c.object_name = &device_name_c as *const _ as *mut _;
    obj_attr_c.attributes = 0x40; // OBJ_CASE_INSENSITIVE
    
    let mut io_status_c = IO_STATUS_BLOCK {
        status: 0,
        information: 0,
    };
    
    let mut handle_c: usize = 0;
    
    let status_c = iop_create_file(
        &mut handle_c,
        0xC0000000,                  // GENERIC_READ | GENERIC_WRITE
        &obj_attr_c,
        &mut io_status_c,
        core::ptr::null(),
        0,
        7,                           // FILE_SHARE_READ|WRITE|DELETE
        1,                           // FILE_OPEN
        0,
        core::ptr::null_mut(),
        0,
        0,                           // KernelMode
    );
    
    if status_c != 0 {
        dbg_print_fail();
        dbg_print(" status=0x");
        dbg_print_hex(status_c as u64);
        dbg_print("\n");
        return;
    }
    
    dbg_print_ok();
    dbg_print(" handle=0x");
    dbg_print_hex(handle_c as u64);
    dbg_print("\n");
    
    // Тест ZwWriteFile для C драйвера
    dbg_print("       Testing ZwWriteFile (C)... ");
    
    static TEST_DATA_C: [u8; 14] = *b"Hello C driver";
    io_status_c.status = 0;
    io_status_c.information = 0;
    
    let write_status_c = iop_read_write_file(
        handle_c,
        0,
        core::ptr::null(),
        core::ptr::null_mut(),
        &mut io_status_c,
        TEST_DATA_C.as_ptr() as *mut _,
        TEST_DATA_C.len() as u32,
        core::ptr::null(),
        core::ptr::null(),
        true,                        // is_write
        0,                           // KernelMode
    );
    
    if write_status_c == 0 {
        dbg_print_ok();
        dbg_print(" wrote ");
        crate::kd::dbg_print_num(io_status_c.information as u64);
        dbg_print(" bytes\n");
    } else {
        dbg_print_fail();
        dbg_print(" status=0x");
        dbg_print_hex(write_status_c as u64);
        dbg_print("\n");
    }
    
    // Тест ZwReadFile для C драйвера
    dbg_print("       Testing ZwReadFile (C)... ");
    
    let mut read_buffer_c: [u8; 32] = [0u8; 32];
    io_status_c.status = 0;
    io_status_c.information = 0;
    
    let read_status_c = iop_read_write_file(
        handle_c,
        0,
        core::ptr::null(),
        core::ptr::null_mut(),
        &mut io_status_c,
        read_buffer_c.as_mut_ptr() as *mut _,
        read_buffer_c.len() as u32,
        core::ptr::null(),
        core::ptr::null(),
        false,                       // is_write = false (READ)
        0,                           // KernelMode
    );
    
    if read_status_c == 0 {
        dbg_print_ok();
        dbg_print(" read ");
        crate::kd::dbg_print_num(io_status_c.information as u64);
        dbg_print(" bytes\n");
    } else {
        dbg_print_fail();
        dbg_print(" status=0x");
        dbg_print_hex(read_status_c as u64);
        dbg_print("\n");
    }
    
    // Тест ZwCancelIoFile (нет pending IRP, но проверяем что API работает)
    dbg_print("       Testing ZwCancelIoFile... ");
    
    io_status_c.status = 0;
    io_status_c.information = 0;
    
    let cancel_status = super::file::NtCancelIoFile(handle_c, &mut io_status_c as *mut _ as *mut _);
    
    if cancel_status == 0 {
        dbg_print_ok();
        dbg_print(" cancelled ");
        crate::kd::dbg_print_num(io_status_c.information as u64);
        dbg_print(" IRPs\n");
    } else {
        dbg_print_fail();
        dbg_print(" status=0x");
        dbg_print_hex(cancel_status as u64);
        dbg_print("\n");
    }
    
    // Закрываем handle C драйвера
    crate::ob::handle::NtClose(handle_c);
}

/// IopLoadBootDriver — загружает один boot driver
///
/// В отличие от NtLoadDriver, образ уже загружен winload'ом.
/// Нужно только создать DRIVER_OBJECT и вызвать DriverEntry.
unsafe fn iop_load_boot_driver(
    driver_name: &str,
    dll_base: u64,
    size_of_image: u32,
    entry_point: u64,
) -> Result<*mut super::driver::DRIVER_OBJECT, NTSTATUS> {
    use super::driver::DRIVER_OBJECT;
    use super::driver::DRIVER_EXTENSION;
    use super::driver::iop_invalid_device_request;
    use super::types::DO_DEVICE_INITIALIZING;
    use super::types::IO_TYPE_DRIVER;
    use super::types::IRP_MJ_MAXIMUM_FUNCTION;
    use crate::nt::CSHORT;
    use crate::ob;

    // Создаём UNICODE_STRING для имени драйвера (только имя, без \Driver\)
    let mut name_buffer = [0u16; 128];
    let mut idx = 0;

    for ch in driver_name.encode_utf16() {
        if idx < name_buffer.len() {
            name_buffer[idx] = ch;
            idx += 1;
        }
    }

    let unicode_name = UNICODE_STRING {
        length: (idx * 2) as u16,
        maximum_length: (name_buffer.len() * 2) as u16,
        buffer: name_buffer.as_mut_ptr(),
    };

    // Создаём OBJECT_ATTRIBUTES с root_directory = \Driver
    let driver_dir = iop_get_driver_directory();
    if driver_dir.is_null() {
        return Err(STATUS_OBJECT_PATH_NOT_FOUND);
    }

    let mut obj_attr = ob::types::OBJECT_ATTRIBUTES::new();
    obj_attr.length = core::mem::size_of::<ob::types::OBJECT_ATTRIBUTES>() as u32;
    obj_attr.root_directory = driver_dir as *mut core::ffi::c_void;
    obj_attr.object_name = &unicode_name as *const _ as *mut UNICODE_STRING;
    obj_attr.attributes = ob::types::OBJ_PERMANENT | ob::types::OBJ_CASE_INSENSITIVE;

    // Размер DRIVER_OBJECT + DRIVER_EXTENSION
    let ext_size = core::mem::size_of::<DRIVER_EXTENSION>();
    let object_size = DRIVER_OBJECT::SIZE + ext_size;

    // Проверяем что тип объекта Driver инициализирован
    if IOP_DRIVER_OBJECT_TYPE.is_null() {
        return Err(STATUS_OBJECT_TYPE_MISMATCH);
    }

    // Создаём объект через OB API
    let mut driver_ptr: *mut core::ffi::c_void = core::ptr::null_mut();
    let status = ob::life::ob_create_object(
        0,                              // KernelMode
        IOP_DRIVER_OBJECT_TYPE,
        &obj_attr,
        0,                              // KernelMode
        core::ptr::null_mut(),          // ParseContext
        object_size,
        0,                              // PagedPoolCharge
        0,                              // NonPagedPoolCharge
        &mut driver_ptr,
    );

    if status != STATUS_SUCCESS {
        return Err(status);
    }

    let drv = driver_ptr as *mut DRIVER_OBJECT;
    let ext = (driver_ptr as usize + DRIVER_OBJECT::SIZE) as *mut DRIVER_EXTENSION;

    // Инициализируем DRIVER_OBJECT
    (*drv).r#type = IO_TYPE_DRIVER as CSHORT;
    (*drv).size = DRIVER_OBJECT::SIZE as CSHORT;
    (*drv).flags = 0;
    (*drv).driver_extension = ext;
    (*drv).driver_start = dll_base as *mut core::ffi::c_void;
    (*drv).driver_size = size_of_image;

    // Инициализируем DRIVER_EXTENSION
    (*ext).driver_object = drv;

    // Инициализируем все dispatch функции указателем на заглушку
    for i in 0..=IRP_MJ_MAXIMUM_FUNCTION as usize {
        (*drv).major_function[i] = Some(iop_invalid_device_request);
    }

    // Вставляем объект в namespace через ob_insert_object
    let status = ob::life::ob_insert_object(
        driver_ptr,
        core::ptr::null_mut(),  // AccessState
        0,                      // DesiredAccess
        0,                      // ObjectPointerBias
        core::ptr::null_mut(),  // Object
        core::ptr::null_mut(),  // Handle
    );

    if status != STATUS_SUCCESS {
        ob::refcount::ob_dereference_object(driver_ptr);
        return Err(status);
    }

    // Копируем имя драйвера (полный путь \Driver\Name)
    let mut full_name_buffer = [0u16; 128];
    let prefix = "\\Driver\\";
    let mut full_idx = 0;
    for ch in prefix.encode_utf16() {
        if full_idx < full_name_buffer.len() {
            full_name_buffer[full_idx] = ch;
            full_idx += 1;
        }
    }
    for ch in driver_name.encode_utf16() {
        if full_idx < full_name_buffer.len() {
            full_name_buffer[full_idx] = ch;
            full_idx += 1;
        }
    }
    
    // Выделяем буфер для имени драйвера
    let name_alloc = crate::ex::pool::ex_allocate_pool_with_tag(
        crate::ex::pool::POOL_TYPE::NonPagedPool,
        full_idx * 2,
        u32::from_le_bytes(*b"DrvN"),
    );
    if !name_alloc.is_null() {
        core::ptr::copy_nonoverlapping(
            full_name_buffer.as_ptr(),
            name_alloc as *mut u16,
            full_idx,
        );
        (*drv).driver_name.buffer = name_alloc as *mut u16;
        (*drv).driver_name.length = (full_idx * 2) as u16;
        (*drv).driver_name.maximum_length = (full_idx * 2) as u16;
    }

    // Отслеживаем загрузку
    super::iop::iop_track_driver_load();

    // Вызываем DriverEntry
    // DriverEntry signature: fn(DriverObject, RegistryPath) -> NTSTATUS
    type DriverEntryFn = unsafe extern "win64" fn(
        *mut DRIVER_OBJECT,
        *mut UNICODE_STRING,
    ) -> NTSTATUS;

    let driver_entry: DriverEntryFn = core::mem::transmute(entry_point as *const ());
    let registry_path = &mut (*ext).service_key_name;

    let status = driver_entry(drv, registry_path);

    if status != STATUS_SUCCESS {
        // DriverEntry не удался — выгружаем драйвер
        super::iop::iop_track_driver_unload();
        ob::refcount::ob_dereference_object(driver_ptr);
        return Err(status);
    }

    // Снимаем флаг инициализации с устройств
    let mut device = (*drv).device_object;
    while !device.is_null() {
        (*device).flags &= !DO_DEVICE_INITIALIZING;
        device = (*device).next_device;
    }

    // Регистрируем драйвер для PnP по известным Hardware IDs
    iop_register_boot_driver_for_pnp(driver_name, drv);

    Ok(drv)
}

/// Регистрирует boot driver для PnP по известным Hardware IDs
///
/// Определяет какие Hardware IDs обрабатывает драйвер по его имени.
unsafe fn iop_register_boot_driver_for_pnp(
    driver_name: &str,
    driver_object: *mut super::driver::DRIVER_OBJECT,
) {
    use super::pnpmgr::drvload::{pnp_register_driver_for_hardware_ids, pnp_register_filter_driver, PCI_HARDWARE_IDS, AHCI_HARDWARE_IDS, DISK_HARDWARE_IDS, PARTITION_HARDWARE_IDS};

    // Проверяем имя драйвера (case insensitive) и регистрируем соответствующие HID
    let is_pci = driver_name.eq_ignore_ascii_case("pci")
        || driver_name.eq_ignore_ascii_case("pci.sys")
        || driver_name.eq_ignore_ascii_case("pci.dll");

    let is_ahci = driver_name.eq_ignore_ascii_case("ahci")
        || driver_name.eq_ignore_ascii_case("ahci.sys")
        || driver_name.eq_ignore_ascii_case("ahci.dll");

    let is_storport = driver_name.eq_ignore_ascii_case("storport")
        || driver_name.eq_ignore_ascii_case("storport.sys")
        || driver_name.eq_ignore_ascii_case("storport.dll");

    let is_msahci = driver_name.eq_ignore_ascii_case("msahci")
        || driver_name.eq_ignore_ascii_case("msahci.sys")
        || driver_name.eq_ignore_ascii_case("msahci.dll");

    let is_disk = driver_name.eq_ignore_ascii_case("disk")
        || driver_name.eq_ignore_ascii_case("disk.sys")
        || driver_name.eq_ignore_ascii_case("disk.dll");

    let is_partmgr = driver_name.eq_ignore_ascii_case("partmgr")
        || driver_name.eq_ignore_ascii_case("partmgr.sys")
        || driver_name.eq_ignore_ascii_case("partmgr.dll");

    let is_volmgr = driver_name.eq_ignore_ascii_case("volmgr")
        || driver_name.eq_ignore_ascii_case("volmgr.sys")
        || driver_name.eq_ignore_ascii_case("volmgr.dll");

    let is_mountmgr = driver_name.eq_ignore_ascii_case("mountmgr")
        || driver_name.eq_ignore_ascii_case("mountmgr.sys")
        || driver_name.eq_ignore_ascii_case("mountmgr.dll");

    if is_pci {
        // PCI Bus Driver
        pnp_register_driver_for_hardware_ids(driver_name, driver_object, PCI_HARDWARE_IDS);
    } else if is_ahci {
        // AHCI SATA Controller Driver (старый монолитный)
        pnp_register_driver_for_hardware_ids(driver_name, driver_object, AHCI_HARDWARE_IDS);
    } else if is_msahci {
        // msahci miniport - регистрируется для AHCI устройств
        // StorPort не регистрируется напрямую - он работает через miniport
        pnp_register_driver_for_hardware_ids(driver_name, driver_object, AHCI_HARDWARE_IDS);
    } else if is_storport {
        // StorPort port driver - не регистрируется для hardware IDs
        // AddDevice вызывается через miniport (msahci)
        crate::kd::dbg_print("[IO] StorPort loaded - no direct HID registration\n");
    } else if is_disk {
        // Disk Storage Class Driver
        pnp_register_driver_for_hardware_ids(driver_name, driver_object, DISK_HARDWARE_IDS);
    } else if is_partmgr {
        // Partition Manager Filter Driver - filters SCSI\Disk
        pnp_register_filter_driver(driver_name, driver_object);
    } else if is_volmgr {
        // Volume Manager - class driver for partition PDOs
        pnp_register_driver_for_hardware_ids(driver_name, driver_object, PARTITION_HARDWARE_IDS);
    } else if is_mountmgr {
        // Mount Manager - control device driver, creates \Device\MountPointManager
        // Does NOT participate in PnP device stacks, no registration needed
        crate::kd::dbg_print("[IO] MountMgr loaded - control device driver\n");
    }
}

// =============================================================================
// Object Type Creation
// =============================================================================

/// Generic mapping для типа File (упрощенная базовая версия)
///
/// Важно: это только стартовая конфигурация. Полная аутентичность требует SE слоя.
static IOP_FILE_GENERIC_MAPPING: ob::types::GENERIC_MAPPING = ob::types::GENERIC_MAPPING {
    generic_read: ob::types::STANDARD_RIGHTS_READ | super::file::FILE_GENERIC_READ,
    generic_write: ob::types::STANDARD_RIGHTS_WRITE | super::file::FILE_GENERIC_WRITE,
    generic_execute: ob::types::STANDARD_RIGHTS_EXECUTE | super::file::FILE_GENERIC_EXECUTE,
    generic_all: super::file::FILE_ALL_ACCESS,
};

/// Статические буферы имён типов (чтобы не аллоцировать в ранней фазе)
static mut IOP_DEVICE_TYPE_NAME_BUFFER: [u16; 7] = [
    b'D' as u16,
    b'e' as u16,
    b'v' as u16,
    b'i' as u16,
    b'c' as u16,
    b'e' as u16,
    0,
];
static mut IOP_DRIVER_TYPE_NAME_BUFFER: [u16; 7] = [
    b'D' as u16,
    b'r' as u16,
    b'i' as u16,
    b'v' as u16,
    b'e' as u16,
    b'r' as u16,
    0,
];
static mut IOP_FILE_TYPE_NAME_BUFFER: [u16; 5] =
    [b'F' as u16, b'i' as u16, b'l' as u16, b'e' as u16, 0];

/// Создает тип объекта Device
unsafe fn iop_create_device_object_type() -> *mut ob::types::OBJECT_TYPE {
    let mut type_name = UNICODE_STRING {
        length: 12, // 6 chars * 2 bytes
        maximum_length: 14,
        buffer: core::ptr::addr_of_mut!(IOP_DEVICE_TYPE_NAME_BUFFER) as *mut u16,
    };

    let mut type_info = ob::types::OBJECT_TYPE_INITIALIZER::new();
    type_info.pool_type = ob::types::NON_PAGED_POOL;
    type_info.valid_access_mask = super::file::FILE_ALL_ACCESS as u32;
    type_info.generic_mapping = IOP_FILE_GENERIC_MAPPING;
    type_info.default_non_paged_pool_charge =
        core::mem::size_of::<super::device::DEVICE_OBJECT>() as u32;

    // Процедуры типа
    type_info.delete_procedure = Some(iop_delete_device);
    type_info.close_procedure = None;
    type_info.parse_procedure = None;

    let mut obj_type: *mut ob::types::OBJECT_TYPE = core::ptr::null_mut();
    let status = ob::life::ob_create_object_type(
        &mut type_name,
        &type_info,
        core::ptr::null_mut(),
        &mut obj_type,
    );

    if status == STATUS_SUCCESS {
        obj_type
    } else {
        core::ptr::null_mut()
    }
}

/// Создает тип объекта Driver
unsafe fn iop_create_driver_object_type() -> *mut ob::types::OBJECT_TYPE {
    let mut type_name = UNICODE_STRING {
        length: 12, // 6 chars * 2 bytes
        maximum_length: 14,
        buffer: core::ptr::addr_of_mut!(IOP_DRIVER_TYPE_NAME_BUFFER) as *mut u16,
    };

    let mut type_info = ob::types::OBJECT_TYPE_INITIALIZER::new();
    type_info.pool_type = ob::types::NON_PAGED_POOL;
    type_info.valid_access_mask = super::file::FILE_ALL_ACCESS as u32;
    type_info.generic_mapping = IOP_FILE_GENERIC_MAPPING;
    type_info.default_non_paged_pool_charge =
        core::mem::size_of::<super::driver::DRIVER_OBJECT>() as u32;

    type_info.delete_procedure = Some(iop_delete_driver);

    let mut obj_type: *mut ob::types::OBJECT_TYPE = core::ptr::null_mut();
    let status = ob::life::ob_create_object_type(
        &mut type_name,
        &type_info,
        core::ptr::null_mut(),
        &mut obj_type,
    );

    if status == STATUS_SUCCESS {
        obj_type
    } else {
        core::ptr::null_mut()
    }
}

/// Создает тип объекта File
unsafe fn iop_create_file_object_type() -> *mut ob::types::OBJECT_TYPE {
    let mut type_name = UNICODE_STRING {
        length: 8, // 4 chars * 2 bytes
        maximum_length: 10,
        buffer: core::ptr::addr_of_mut!(IOP_FILE_TYPE_NAME_BUFFER) as *mut u16,
    };

    let mut type_info = ob::types::OBJECT_TYPE_INITIALIZER::new();
    type_info.pool_type = ob::types::NON_PAGED_POOL;
    type_info.valid_access_mask = super::file::FILE_ALL_ACCESS as u32;
    type_info.generic_mapping = IOP_FILE_GENERIC_MAPPING;
    type_info.default_non_paged_pool_charge =
        core::mem::size_of::<super::file::FILE_OBJECT>() as u32;

    // File objects: важно иметь close callback на “последний handle”.
    type_info.maintain_handle_count = true;
    type_info.close_procedure = Some(iop_close_file);
    type_info.delete_procedure = Some(iop_delete_file);
    type_info.parse_procedure = Some(iop_parse_file);

    // File objects waitable => предоставляем default wait object
    type_info.use_default_object = true;

    let mut obj_type: *mut ob::types::OBJECT_TYPE = core::ptr::null_mut();
    let status = ob::life::ob_create_object_type(
        &mut type_name,
        &type_info,
        core::ptr::null_mut(),
        &mut obj_type,
    );

    if status == STATUS_SUCCESS {
        obj_type
    } else {
        core::ptr::null_mut()
    }
}

// =============================================================================
// Object Procedures
// =============================================================================

/// Процедура удаления Device Object
unsafe extern "win64" fn iop_delete_device(object: crate::nt::PVOID) {
    unsafe {
        // Device objects удаляются через IoDeleteDevice
        // Здесь просто освобождаем память
        let device = object as super::types::PDEVICE_OBJECT;
        if !device.is_null() {
            super::device::io_delete_device(device);
        }
    }
}

/// Процедура удаления Driver Object
unsafe extern "win64" fn iop_delete_driver(object: crate::nt::PVOID) {
    unsafe {
        use crate::ex::pool::ex_free_pool_with_tag;

        let driver = object as super::types::PDRIVER_OBJECT;
        if !driver.is_null() {
            // Вызываем Unload если есть
            if let Some(unload) = (*driver).driver_unload {
                unload(driver);
            }
            ex_free_pool_with_tag(object, u32::from_le_bytes(*b"Driv"));
        }
    }
}

/// Процедура удаления File Object
unsafe extern "win64" fn iop_delete_file(object: crate::nt::PVOID) {
    unsafe {
        let file = object as super::types::PFILE_OBJECT;
        if !file.is_null() {
            super::file::iop_delete_file_object(file);
        }
    }
}

/// Процедура закрытия File Object (CloseProcedure)
///
/// Вызывается Object Manager при закрытии handle.
/// Отправляет IRP_MJ_CLEANUP драйверу при закрытии последнего handle процесса.
///
/// NT6.1: IopCloseFile в ntos/io/iomgr/close.c
unsafe extern "win64" fn iop_close_file(
    _process: crate::nt::PVOID,
    object: crate::nt::PVOID,
    _granted_access: u32,
    process_handle_count: crate::nt::ULONG,
    _system_handle_count: crate::nt::ULONG,
) {
    let file = object as super::types::PFILE_OBJECT;
    if file.is_null() {
        return;
    }

    // Отправляем IRP_MJ_CLEANUP когда процесс закрывает последний handle
    // (process_handle_count == 1 означает что это последний handle процесса)
    if process_handle_count == 1 {
        // Проверяем что cleanup ещё не выполнялся
        if ((*file).flags & super::types::FO_CLEANUP_COMPLETE) == 0 {
            let _ = super::file::iop_send_cleanup_irp(file);
        }
    }
}

/// Процедура парсинга для File Object (ParseProcedure)
///
/// Вызывается Object Manager при относительном открытии через FILE_OBJECT.
/// Например: NtCreateFile с RootDirectory = handle на открытый файл/директорию.
///
/// NT6.1: IopParseFile в ntos/io/iomgr/parse.c
unsafe extern "win64" fn iop_parse_file(
    object: crate::nt::PVOID,
    _object_type: *mut ob::types::OBJECT_TYPE,
    _access_state: crate::nt::PVOID,
    _access_mode: u8,
    _attributes: crate::nt::ULONG,
    _complete_name: *mut crate::nt::ntdef::UNICODE_STRING,
    remaining_name: *mut crate::nt::ntdef::UNICODE_STRING,
    context: crate::nt::PVOID,
    _security_qos: crate::nt::PVOID,
    found_object: *mut crate::nt::PVOID,
) -> NTSTATUS {
    use super::file::{
        iop_create_file_object, iop_delete_file_object, iop_send_create_irp,
        FILE_OBJECT, IO_STATUS_BLOCK,
    };
    use super::types::FO_SYNCHRONOUS_IO;

    // Родительский FILE_OBJECT (из RootDirectory handle)
    let related_file = object as *mut FILE_OBJECT;
    if related_file.is_null() || found_object.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    // Получаем устройство из родительского FILE_OBJECT
    let device_object = (*related_file).device_object;
    if device_object.is_null() {
        return STATUS_INVALID_DEVICE_REQUEST;
    }

    // Создаём новый FILE_OBJECT для открываемого файла
    let file_object = iop_create_file_object(device_object);
    if file_object.is_null() {
        return STATUS_INSUFFICIENT_RESOURCES;
    }

    // Устанавливаем RelatedFileObject - драйвер использует его для относительного открытия
    (*file_object).related_file_object = related_file;

    // Копируем FileName из remaining_name
    if !remaining_name.is_null() && (*remaining_name).length > 0 {
        (*file_object).file_name.length = (*remaining_name).length;
        (*file_object).file_name.maximum_length = (*remaining_name).maximum_length;
        (*file_object).file_name.buffer = (*remaining_name).buffer;
    }

    // Устанавливаем синхронный режим по умолчанию
    (*file_object).flags |= FO_SYNCHRONOUS_IO;

    // Получаем context с параметрами создания (если передан)
    // Context содержит desired_access, share_access, create_disposition, create_options
    let (desired_access, share_access, create_disposition, create_options) = if !context.is_null() {
        let ctx = context as *const IopCreateContext;
        (
            (*ctx).desired_access,
            (*ctx).share_access,
            (*ctx).create_disposition,
            (*ctx).create_options,
        )
    } else {
        // Значения по умолчанию
        (0xC0000000u32, 7u32, 1u32, 0u32) // GENERIC_READ|WRITE, FILE_SHARE_ALL, FILE_OPEN
    };

    // Создаём IO_STATUS_BLOCK для IRP
    let mut io_status = IO_STATUS_BLOCK {
        status: 0,
        information: 0,
    };

    // Отправляем IRP_MJ_CREATE драйверу
    let status = iop_send_create_irp(
        device_object,
        file_object,
        desired_access,
        share_access,
        create_disposition,
        create_options,
        &mut io_status,
    );

    if status != STATUS_SUCCESS && status != STATUS_PENDING {
        // CREATE не удался — освобождаем FILE_OBJECT
        iop_delete_file_object(file_object);
        return status;
    }

    // Возвращаем созданный FILE_OBJECT
    *found_object = file_object as crate::nt::PVOID;
    STATUS_SUCCESS
}

/// Контекст для IopParseFile с параметрами создания файла
#[repr(C)]
struct IopCreateContext {
    desired_access: u32,
    share_access: u32,
    create_disposition: u32,
    create_options: u32,
}

// =============================================================================
// Object Directories
// =============================================================================

/// Константы имён директорий I/O Manager
static IOP_DEVICE_DIR_NAME: [u16; 7] = [
    b'D' as u16,
    b'e' as u16,
    b'v' as u16,
    b'i' as u16,
    b'c' as u16,
    b'e' as u16,
    0,
];
static IOP_DRIVER_DIR_NAME: [u16; 7] = [
    b'D' as u16,
    b'r' as u16,
    b'i' as u16,
    b'v' as u16,
    b'e' as u16,
    b'r' as u16,
    0,
];
static IOP_FILESYSTEM_DIR_NAME: [u16; 11] = [
    b'F' as u16,
    b'i' as u16,
    b'l' as u16,
    b'e' as u16,
    b'S' as u16,
    b'y' as u16,
    b's' as u16,
    b't' as u16,
    b'e' as u16,
    b'm' as u16,
    0,
];
static IOP_DOSDEVICES_DIR_NAME: [u16; 3] = [
    b'?' as u16,
    b'?' as u16,
    0,
];

/// IopCreateObjectDirectories — создаёт директории I/O namespace
///
/// Создаёт `\Device`, `\Driver`, `\FileSystem` в корневой директории.
/// Сохраняет указатели на директории в глобальные переменные.
/// Вызывается в Phase 1 инициализации I/O Manager.
unsafe fn iop_create_object_directories() -> bool {
    unsafe {
        use crate::ob::init::obp_create_named_directory;

        // Создаём \Device
        let device_name = UNICODE_STRING {
            length: 12, // 6 chars * 2 bytes
            maximum_length: 14,
            buffer: IOP_DEVICE_DIR_NAME.as_ptr() as *mut u16,
        };
        match obp_create_named_directory(core::ptr::null_mut(), &device_name) {
            Ok(dir) => IOP_DEVICE_DIRECTORY = dir,
            Err(_) => return false,
        }

        // Создаём \Driver
        let driver_name = UNICODE_STRING {
            length: 12, // 6 chars * 2 bytes
            maximum_length: 14,
            buffer: IOP_DRIVER_DIR_NAME.as_ptr() as *mut u16,
        };
        match obp_create_named_directory(core::ptr::null_mut(), &driver_name) {
            Ok(dir) => IOP_DRIVER_DIRECTORY = dir,
            Err(_) => return false,
        }

        // Создаём \FileSystem
        let filesystem_name = UNICODE_STRING {
            length: 20, // 10 chars * 2 bytes
            maximum_length: 22,
            buffer: IOP_FILESYSTEM_DIR_NAME.as_ptr() as *mut u16,
        };
        match obp_create_named_directory(core::ptr::null_mut(), &filesystem_name) {
            Ok(dir) => IOP_FILESYSTEM_DIRECTORY = dir,
            Err(_) => return false,
        }

        // Открываем \?? (DosDevices) - уже создан в OB Phase 1
        let dosdevices_name = UNICODE_STRING {
            length: 4, // 2 chars * 2 bytes
            maximum_length: 6,
            buffer: IOP_DOSDEVICES_DIR_NAME.as_ptr() as *mut u16,
        };
        
        // Открываем existing directory
        use crate::ob::types::OBJECT_ATTRIBUTES;
        let mut obj_attr = OBJECT_ATTRIBUTES::new();
        obj_attr.length = core::mem::size_of::<OBJECT_ATTRIBUTES>() as u32;
        obj_attr.root_directory = core::ptr::null_mut(); // Root namespace
        obj_attr.object_name = &dosdevices_name as *const _ as *mut UNICODE_STRING;
        obj_attr.attributes = crate::ob::types::OBJ_CASE_INSENSITIVE;
        
        let mut handle: crate::nt::PVOID = core::ptr::null_mut();
        let status = crate::ob::dir::nt_open_directory_object(
            &mut handle,
            crate::ob::types::DIRECTORY_ALL_ACCESS,
            &mut obj_attr,
        );
        
        if status == STATUS_SUCCESS && !handle.is_null() {
            // Получаем объект из handle
            let mut dir_obj: crate::nt::PVOID = core::ptr::null_mut();
            let ref_status = crate::ob::refcount::ob_reference_object_by_handle(
                handle,
                0, // no specific access needed
                crate::ob::init::obp_get_directory_object_type(),
                0, // KernelMode
                &mut dir_obj,
                core::ptr::null_mut(),
            );
            
            if ref_status == STATUS_SUCCESS {
                IOP_DOSDEVICES_DIRECTORY = dir_obj as *mut ob::dir::OBJECT_DIRECTORY;
            }
            
            crate::ob::handle::NtClose(handle as usize);
        }
        
        // \?? directory опционален для boot scenario, продолжаем даже если не открылся
        if IOP_DOSDEVICES_DIRECTORY.is_null() {
            crate::kd::dbg_print("   [IO] WARNING: Could not open \\?? directory\n");
        }

        true
    }
}

/// Возвращает указатель на директорию \?? (DosDevices)
#[inline]
pub fn iop_get_dosdevices_directory() -> *mut ob::dir::OBJECT_DIRECTORY {
    unsafe { IOP_DOSDEVICES_DIRECTORY }
}

/// Возвращает указатель на директорию \Device
#[inline]
pub fn iop_get_device_directory() -> *mut ob::dir::OBJECT_DIRECTORY {
    unsafe { IOP_DEVICE_DIRECTORY }
}

/// Возвращает указатель на директорию \Driver
#[inline]
pub fn iop_get_driver_directory() -> *mut ob::dir::OBJECT_DIRECTORY {
    unsafe { IOP_DRIVER_DIRECTORY }
}

// =============================================================================
// Query Functions
// =============================================================================

/// Проверяет инициализирован ли I/O Manager
#[inline]
pub fn io_is_initialized() -> bool {
    IO_INITIALIZED.load(Ordering::Acquire)
}

/// Возвращает фазу инициализации
#[inline]
pub fn io_get_init_phase() -> u32 {
    IO_INIT_PHASE.load(Ordering::Acquire)
}
