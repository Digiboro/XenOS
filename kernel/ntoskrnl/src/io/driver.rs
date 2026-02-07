//! DRIVER_OBJECT - Объект драйвера
//!
//! # Архитектура
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    Driver Loading                           │
//! ├─────────────────────────────────────────────────────────────┤
//! │                                                             │
//! │  NtLoadDriver(service_path)                                 │
//! │        │                                                    │
//! │        ▼                                                    │
//! │  ┌─────────────────┐                                        │
//! │  │ IopLoadDriver   │                                        │
//! │  └────────┬────────┘                                        │
//! │           │                                                 │
//! │           ▼                                                 │
//! │  ┌─────────────────┐     ┌─────────────────┐                │
//! │  │ Create          │ ──► │ \Driver\<name>  │                │
//! │  │ DRIVER_OBJECT   │     │ in OB namespace │                │
//! │  └────────┬────────┘     └─────────────────┘                │
//! │           │                                                 │
//! │           ▼                                                 │
//! │  ┌─────────────────┐                                        │
//! │  │ DriverEntry()   │ ◄── Call driver's entry point          │
//! │  └────────┬────────┘                                        │
//! │           │                                                 │
//! │           ▼                                                 │
//! │  Driver creates devices via IoCreateDevice                  │
//! │                                                             │
//! └─────────────────────────────────────────────────────────────┘
//!
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    Driver Unloading                         │
//! ├─────────────────────────────────────────────────────────────┤
//! │                                                             │
//! │  NtUnloadDriver / IoUnloadDriver                            │
//! │        │                                                    │
//! │        ▼                                                    │
//! │  ┌─────────────────┐                                        │
//! │  │ IopUnloadDriver │                                        │
//! │  └────────┬────────┘                                        │
//! │           │                                                 │
//! │           ▼                                                 │
//! │  ┌─────────────────┐                                        │
//! │  │ DriverUnload()  │ ◄── Call driver's unload routine       │
//! │  └────────┬────────┘                                        │
//! │           │                                                 │
//! │           ▼                                                 │
//! │  Delete devices, remove from namespace, free memory         │
//! │                                                             │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Отступления и упрощения
//!
//! - Нет загрузки из PE файла (только built-in драйверы)
//! - Упрощённая работа с реестром
//! - Нет поддержки зависимостей между драйверами
//!
//! Источники:
//! - ReactOS: sdk/include/xdk/iotypes.h, ntoskrnl/io/iomgr/driver.c
//! - Windows XP: base/ntos/io/iomgr/loadunld.c

use super::types::*;
use crate::nt::CSHORT;
use crate::nt::NTSTATUS;
use crate::nt::PVOID;
use crate::nt::ULONG;
use crate::nt::ntdef::UNICODE_STRING;
use crate::nt::ntstatus::*;

// =============================================================================
// DRIVER_EXTENSION
// =============================================================================

/// DRIVER_EXTENSION - расширение объекта драйвера
#[repr(C)]
pub struct DRIVER_EXTENSION {
    /// Указатель на DRIVER_OBJECT
    pub driver_object: PDRIVER_OBJECT,
    /// Функция AddDevice (PnP)
    pub add_device: PDRIVER_ADD_DEVICE,
    /// Счетчик устройств
    pub count: ULONG,
    /// Имя сервиса (registry path)
    pub service_key_name: UNICODE_STRING,
    /// Список client notification
    pub client_driver_extension: PVOID,
    /// Указатель на файловую систему
    pub fs_filter_callbacks: PVOID,
}

impl DRIVER_EXTENSION {
    pub const fn new() -> Self {
        Self {
            driver_object: core::ptr::null_mut(),
            add_device: None,
            count: 0,
            service_key_name: UNICODE_STRING::new(),
            client_driver_extension: core::ptr::null_mut(),
            fs_filter_callbacks: core::ptr::null_mut(),
        }
    }
}

/// IO_CLIENT_EXTENSION - элемент списка расширений драйвера
///
/// Каждый драйвер может иметь несколько расширений, идентифицированных
/// по client_identification_address (обычно адрес AddDevice функции).
#[repr(C)]
pub struct IO_CLIENT_EXTENSION {
    /// Следующее расширение в списке
    pub next_extension: *mut IO_CLIENT_EXTENSION,
    /// Идентификатор клиента (обычно адрес AddDevice)
    pub client_identification_address: PVOID,
    // Данные расширения следуют непосредственно за этой структурой
}

// =============================================================================
// FAST_IO_DISPATCH
// =============================================================================

/// FAST_IO_DISPATCH - таблица Fast I/O функций
///
/// Fast I/O позволяет обойти IRP для некоторых операций
#[repr(C)]
pub struct FAST_IO_DISPATCH {
    pub size_of_fast_io_dispatch: ULONG,
    pub fast_io_check_if_possible: PVOID,
    pub fast_io_read: PVOID,
    pub fast_io_write: PVOID,
    pub fast_io_query_basic_info: PVOID,
    pub fast_io_query_standard_info: PVOID,
    pub fast_io_lock: PVOID,
    pub fast_io_unlock_single: PVOID,
    pub fast_io_unlock_all: PVOID,
    pub fast_io_unlock_all_by_key: PVOID,
    pub fast_io_device_control: PVOID,
    pub acquire_file_for_ntsection: PVOID,
    pub release_file_for_ntsection: PVOID,
    pub fast_io_detach_device: PVOID,
    pub fast_io_query_network_open_info: PVOID,
    pub acquire_for_mod_write: PVOID,
    pub mdl_read: PVOID,
    pub mdl_read_complete: PVOID,
    pub prepare_mdl_write: PVOID,
    pub mdl_write_complete: PVOID,
    pub fast_io_read_compressed: PVOID,
    pub fast_io_write_compressed: PVOID,
    pub mdl_read_complete_compressed: PVOID,
    pub mdl_write_complete_compressed: PVOID,
    pub fast_io_query_open: PVOID,
    pub release_for_mod_write: PVOID,
    pub acquire_for_cc_flush: PVOID,
    pub release_for_cc_flush: PVOID,
}

// =============================================================================
// DRIVER_OBJECT
// =============================================================================

/// DRIVER_OBJECT - объект драйвера
///
/// Представляет загруженный драйвер в системе.
/// Создается I/O Manager при загрузке драйвера.
#[repr(C)]
pub struct DRIVER_OBJECT {
    /// Тип объекта (IO_TYPE_DRIVER)
    pub r#type: CSHORT,
    /// Размер структуры
    pub size: CSHORT,
    /// Первое устройство в списке
    pub device_object: PDEVICE_OBJECT,
    /// Флаги драйвера
    pub flags: ULONG,
    /// Начало образа драйвера в памяти
    pub driver_start: PVOID,
    /// Размер образа драйвера
    pub driver_size: ULONG,
    /// Секция образа (LDR_DATA_TABLE_ENTRY)
    pub driver_section: PVOID,
    /// Расширение драйвера
    pub driver_extension: *mut DRIVER_EXTENSION,
    /// Имя драйвера
    pub driver_name: UNICODE_STRING,
    /// Путь в реестре к hardware database
    pub hardware_database: *mut UNICODE_STRING,
    /// Таблица Fast I/O
    pub fast_io_dispatch: *mut FAST_IO_DISPATCH,
    /// Функция инициализации (DriverEntry)
    pub driver_init: PDRIVER_INITIALIZE,
    /// Функция StartIo
    pub driver_start_io: PDRIVER_STARTIO,
    /// Функция выгрузки
    pub driver_unload: PDRIVER_UNLOAD,
    /// Таблица dispatch функций
    pub major_function: [PDRIVER_DISPATCH; (IRP_MJ_MAXIMUM_FUNCTION + 1) as usize],
}

impl DRIVER_OBJECT {
    /// Размер структуры
    pub const SIZE: usize = core::mem::size_of::<Self>();
}

// =============================================================================
// Driver Object Functions
// =============================================================================

/// IoAllocateDriverObjectExtension - выделяет расширение драйвера
///
/// Выделяет расширение для драйвера, идентифицированное по client_identification_address.
/// Обычно client_identification_address - это адрес AddDevice функции драйвера.
///
/// # Returns
/// * STATUS_SUCCESS - расширение выделено
/// * STATUS_OBJECT_NAME_COLLISION - расширение с таким ID уже существует
/// * STATUS_INSUFFICIENT_RESOURCES - недостаточно памяти
pub unsafe fn io_allocate_driver_object_extension(
    driver_object: PDRIVER_OBJECT,
    client_identification_address: PVOID,
    driver_object_extension_size: ULONG,
    driver_object_extension: *mut PVOID,
) -> NTSTATUS {
    unsafe {
        use crate::ex::pool::POOL_TYPE;
        use crate::ex::pool::ex_allocate_pool_with_tag;

        if driver_object.is_null() || driver_object_extension.is_null() {
            return STATUS_INVALID_PARAMETER;
        }
        
        if client_identification_address.is_null() {
            return STATUS_INVALID_PARAMETER;
        }

        let drv_ext = (*driver_object).driver_extension;
        if drv_ext.is_null() {
            return STATUS_INVALID_PARAMETER;
        }
        
        // Проверяем что расширение с таким ID ещё не существует
        let mut current = (*drv_ext).client_driver_extension as *mut IO_CLIENT_EXTENSION;
        while !current.is_null() {
            if (*current).client_identification_address == client_identification_address {
                return STATUS_OBJECT_NAME_COLLISION;
            }
            current = (*current).next_extension;
        }

        // Вычисляем общий размер: заголовок + данные расширения
        let header_size = core::mem::size_of::<IO_CLIENT_EXTENSION>();
        let total_size = header_size + driver_object_extension_size as usize;
        
        // Выделяем память для заголовка + данных
        let extension = ex_allocate_pool_with_tag(
            POOL_TYPE::NonPagedPool,
            total_size,
            u32::from_le_bytes(*b"DrvE"),
        );

        if extension.is_null() {
            return STATUS_INSUFFICIENT_RESOURCES;
        }

        // Обнуляем
        core::ptr::write_bytes(extension as *mut u8, 0, total_size);

        // Инициализируем заголовок
        let client_ext = extension as *mut IO_CLIENT_EXTENSION;
        (*client_ext).client_identification_address = client_identification_address;
        
        // Вставляем в начало списка
        (*client_ext).next_extension = (*drv_ext).client_driver_extension as *mut IO_CLIENT_EXTENSION;
        (*drv_ext).client_driver_extension = client_ext as PVOID;
        
        // Возвращаем указатель на данные (после заголовка)
        *driver_object_extension = (extension as usize + header_size) as PVOID;

        STATUS_SUCCESS
    }
}

/// IoGetDriverObjectExtension - получает расширение драйвера
///
/// Ищет расширение драйвера по client_identification_address.
///
/// # Returns
/// Указатель на данные расширения или NULL если не найдено
pub unsafe fn io_get_driver_object_extension(
    driver_object: PDRIVER_OBJECT,
    client_identification_address: PVOID,
) -> PVOID {
    unsafe {
        if driver_object.is_null() || client_identification_address.is_null() {
            return core::ptr::null_mut();
        }
        
        let drv_ext = (*driver_object).driver_extension;
        if drv_ext.is_null() {
            return core::ptr::null_mut();
        }
        
        // Ищем в списке
        let mut current = (*drv_ext).client_driver_extension as *mut IO_CLIENT_EXTENSION;
        while !current.is_null() {
            if (*current).client_identification_address == client_identification_address {
                // Возвращаем указатель на данные (после заголовка)
                let header_size = core::mem::size_of::<IO_CLIENT_EXTENSION>();
                return (current as usize + header_size) as PVOID;
            }
            current = (*current).next_extension;
        }
        
        core::ptr::null_mut()
    }
}

/// Создает DRIVER_OBJECT для встроенного драйвера
pub unsafe fn iop_create_driver_object(_driver_name: &str) -> *mut DRIVER_OBJECT {
    unsafe {
        use crate::ex::pool::POOL_TYPE;
        use crate::ex::pool::ex_allocate_pool_with_tag;

        // Вычисляем размер
        let ext_size = core::mem::size_of::<DRIVER_EXTENSION>();
        let total_size = DRIVER_OBJECT::SIZE + ext_size;

        // Выделяем память
        let driver = ex_allocate_pool_with_tag(
            POOL_TYPE::NonPagedPool,
            total_size,
            u32::from_le_bytes(*b"Driv"),
        );

        if driver.is_null() {
            return core::ptr::null_mut();
        }

        // Обнуляем
        core::ptr::write_bytes(driver as *mut u8, 0, total_size);

        let drv = driver as *mut DRIVER_OBJECT;
        let ext = (driver as usize + DRIVER_OBJECT::SIZE) as *mut DRIVER_EXTENSION;

        // Инициализируем DRIVER_OBJECT
        (*drv).r#type = IO_TYPE_DRIVER as CSHORT;
        (*drv).size = DRIVER_OBJECT::SIZE as CSHORT;
        (*drv).flags = 0;
        (*drv).driver_extension = ext;

        // Инициализируем DRIVER_EXTENSION
        (*ext).driver_object = drv;

        // Инициализируем все dispatch функции указателем на заглушку
        for i in 0..=IRP_MJ_MAXIMUM_FUNCTION as usize {
            (*drv).major_function[i] = Some(iop_invalid_device_request);
        }

        drv
    }
}

/// Заглушка для неподдерживаемых IRP
pub(super) unsafe extern "win64" fn iop_invalid_device_request(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    unsafe {
        use super::irp::iof_complete_request;

        // Завершаем IRP с ошибкой
        (*irp).io_status.status = STATUS_INVALID_DEVICE_REQUEST;
        (*irp).io_status.information = 0;
        iof_complete_request(irp, 0); // IO_NO_INCREMENT
        STATUS_INVALID_DEVICE_REQUEST
    }
}

// =============================================================================
// Driver Loading
// =============================================================================

/// IopLoadDriver — загружает драйвер и вызывает DriverEntry
///
/// Для встроенных драйверов: создаёт DRIVER_OBJECT и вызывает entry point.
/// Для внешних драйверов: загружает PE образ, создаёт объект, вызывает entry.
pub unsafe fn iop_load_driver(
    driver_name: &UNICODE_STRING,
    driver_entry: PDRIVER_INITIALIZE,
) -> Result<*mut DRIVER_OBJECT, NTSTATUS> {
    unsafe {
        if driver_entry.is_none() {
            return Err(STATUS_INVALID_PARAMETER);
        }

        // Проверяем не загружен ли уже
        let driver_dir = super::init::iop_get_driver_directory();
        if driver_dir.is_null() {
            return Err(STATUS_OBJECT_PATH_NOT_FOUND);
        }

        // Создаём DRIVER_OBJECT
        let driver_object = iop_create_driver_object_internal(driver_name)?;

        // Устанавливаем DriverEntry
        (*driver_object).driver_init = driver_entry;

        // Регистрируем в OB namespace (\Driver\<name>)
        let status = iop_register_driver_object(driver_object, driver_name);
        if status != STATUS_SUCCESS {
            iop_delete_driver_object(driver_object);
            return Err(status);
        }

        // Отслеживаем загрузку
        super::iop::iop_track_driver_load();

        // Вызываем DriverEntry
        let entry = driver_entry.unwrap();
        let registry_path = &(*(*driver_object).driver_extension).service_key_name;

        let status = entry(driver_object, registry_path as *const _ as *mut _);

        if status != STATUS_SUCCESS {
            // DriverEntry не удался — выгружаем драйвер
            super::iop::iop_track_driver_unload();
            iop_unregister_driver_object(driver_object);
            iop_delete_driver_object(driver_object);
            return Err(status);
        }

        // Снимаем флаг инициализации с устройств
        let mut device = (*driver_object).device_object;
        while !device.is_null() {
            (*device).flags &= !DO_DEVICE_INITIALIZING;
            device = (*device).next_device;
        }

        Ok(driver_object)
    }
}

/// IopCreateDriverObjectInternal — создаёт и инициализирует DRIVER_OBJECT
unsafe fn iop_create_driver_object_internal(
    driver_name: &UNICODE_STRING,
) -> Result<*mut DRIVER_OBJECT, NTSTATUS> {
    unsafe {
        use crate::ex::pool::POOL_TYPE;
        use crate::ex::pool::ex_allocate_pool_with_tag;

        // Вычисляем размеры
        let ext_size = core::mem::size_of::<DRIVER_EXTENSION>();
        let total_size = DRIVER_OBJECT::SIZE + ext_size;

        // Выделяем память
        let driver = ex_allocate_pool_with_tag(
            POOL_TYPE::NonPagedPool,
            total_size,
            u32::from_le_bytes(*b"Driv"),
        );

        if driver.is_null() {
            return Err(STATUS_INSUFFICIENT_RESOURCES);
        }

        // Обнуляем
        core::ptr::write_bytes(driver as *mut u8, 0, total_size);

        let drv = driver as *mut DRIVER_OBJECT;
        let ext = (driver as usize + DRIVER_OBJECT::SIZE) as *mut DRIVER_EXTENSION;

        // Инициализируем DRIVER_OBJECT
        (*drv).r#type = IO_TYPE_DRIVER as CSHORT;
        (*drv).size = DRIVER_OBJECT::SIZE as CSHORT;
        (*drv).flags = 0;
        (*drv).driver_extension = ext;

        // Копируем имя драйвера
        if !driver_name.buffer.is_null() && driver_name.length > 0 {
            let name_buffer = ex_allocate_pool_with_tag(
                POOL_TYPE::NonPagedPool,
                driver_name.length as usize,
                u32::from_le_bytes(*b"DrvN"),
            );

            if !name_buffer.is_null() {
                core::ptr::copy_nonoverlapping(
                    driver_name.buffer,
                    name_buffer as *mut u16,
                    driver_name.length as usize / 2,
                );
                (*drv).driver_name.buffer = name_buffer as *mut u16;
                (*drv).driver_name.length = driver_name.length;
                (*drv).driver_name.maximum_length = driver_name.length;
            }
        }

        // Инициализируем DRIVER_EXTENSION
        (*ext).driver_object = drv;

        // Инициализируем все dispatch функции заглушкой
        for i in 0..=IRP_MJ_MAXIMUM_FUNCTION as usize {
            (*drv).major_function[i] = Some(iop_invalid_device_request);
        }

        Ok(drv)
    }
}

/// IopRegisterDriverObject — регистрирует DRIVER_OBJECT в OB namespace
pub(super) unsafe fn iop_register_driver_object(
    driver_object: *mut DRIVER_OBJECT,
    driver_name: &UNICODE_STRING,
) -> NTSTATUS {
    use crate::ob;

    let driver_dir = super::init::iop_get_driver_directory();
    if driver_dir.is_null() {
        return STATUS_OBJECT_PATH_NOT_FOUND;
    }

    // Создаём объект в директории \Driver
    let mut obj_attr = ob::types::OBJECT_ATTRIBUTES::new();
    obj_attr.length = core::mem::size_of::<ob::types::OBJECT_ATTRIBUTES>() as ULONG;
    obj_attr.root_directory = driver_dir as PVOID;
    obj_attr.object_name = driver_name as *const _ as *mut UNICODE_STRING;
    obj_attr.attributes = ob::types::OBJ_PERMANENT | ob::types::OBJ_CASE_INSENSITIVE;

    // Создаём объект через OB
    let obj: PVOID = driver_object as PVOID;
    let status = ob::life::ob_insert_object(
        obj,
        core::ptr::null_mut(), // access state
        0,                     // desired access
        0,                     // object pointer bias
        core::ptr::null_mut(), // object
        core::ptr::null_mut(), // handle
    );

    status
}

/// IopUnregisterDriverObject — удаляет DRIVER_OBJECT из OB namespace
pub(super) unsafe fn iop_unregister_driver_object(driver_object: *mut DRIVER_OBJECT) {
    unsafe {
        use crate::ob;

        if driver_object.is_null() {
            return;
        }

        // Удаляем permanent флаг чтобы объект мог быть удалён при drop refcount
        let header = ob::header::object_to_object_header(driver_object as crate::nt::PVOID);
        if !header.is_null() {
            (*header).flags &= !ob::types::OB_FLAG_PERMANENT;
        }
    }
}

/// IopDeleteDriverObject — освобождает память DRIVER_OBJECT
unsafe fn iop_delete_driver_object(driver_object: *mut DRIVER_OBJECT) {
    unsafe {
        use crate::ex::pool::ex_free_pool_with_tag;

        if driver_object.is_null() {
            return;
        }

        // Освобождаем имя драйвера
        if !(*driver_object).driver_name.buffer.is_null() {
            ex_free_pool_with_tag(
                (*driver_object).driver_name.buffer as PVOID,
                u32::from_le_bytes(*b"DrvN"),
            );
        }

        // Освобождаем DRIVER_OBJECT + DRIVER_EXTENSION
        ex_free_pool_with_tag(driver_object as PVOID, u32::from_le_bytes(*b"Driv"));
    }
}

// =============================================================================
// Driver Unloading
// =============================================================================

/// IopUnloadDriver — выгружает драйвер
///
/// Вызывает DriverUnload, удаляет устройства, освобождает ресурсы.
pub unsafe fn iop_unload_driver(driver_object: *mut DRIVER_OBJECT) -> NTSTATUS {
    unsafe {
        if driver_object.is_null() {
            return STATUS_INVALID_PARAMETER;
        }

        // Проверяем можно ли выгрузить
        if (*driver_object).driver_unload.is_none() {
            return STATUS_INVALID_DEVICE_REQUEST; // Driver doesn't support unloading
        }

        // Проверяем есть ли устройства
        if !(*driver_object).device_object.is_null() {
            // Есть устройства — нельзя выгрузить пока они существуют
            // В реальной NT драйвер должен сам удалить устройства в DriverUnload
        }

        // Вызываем DriverUnload
        if let Some(unload) = (*driver_object).driver_unload {
            unload(driver_object);
        }

        // Удаляем все оставшиеся устройства
        while !(*driver_object).device_object.is_null() {
            let device = (*driver_object).device_object;
            super::device::io_delete_device(device);
        }

        // Отслеживаем выгрузку
        super::iop::iop_track_driver_unload();

        // Удаляем из namespace и освобождаем память
        iop_unregister_driver_object(driver_object);
        iop_delete_driver_object(driver_object);

        STATUS_SUCCESS
    }
}

/// IoUnloadDriver — публичный API для выгрузки драйвера по имени
pub unsafe fn io_unload_driver(driver_service_name: *const UNICODE_STRING) -> NTSTATUS {
    if driver_service_name.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    // TODO: Найти драйвер по имени и вызвать iop_unload_driver
    STATUS_NOT_IMPLEMENTED
}

// =============================================================================
// NtLoadDriver / NtUnloadDriver
// =============================================================================

/// NtLoadDriver — загружает драйвер по пути в реестре
///
/// # Arguments
/// * `driver_service_name` - путь к ключу реестра драйвера
///   (например, `\Registry\Machine\System\CurrentControlSet\Services\MyDriver`)
#[unsafe(no_mangle)]
pub extern "win64" fn NtLoadDriver(driver_service_name: *const UNICODE_STRING) -> NTSTATUS {
    if driver_service_name.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    // TODO: Полная реализация:
    // 1. Прочитать параметры из реестра (ImagePath, Start, Type, etc.)
    // 2. Загрузить PE образ
    // 3. Вызвать IopLoadDriver

    STATUS_NOT_IMPLEMENTED
}

/// NtUnloadDriver — выгружает драйвер по пути в реестре
#[unsafe(no_mangle)]
pub extern "win64" fn NtUnloadDriver(driver_service_name: *const UNICODE_STRING) -> NTSTATUS {
    if driver_service_name.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    // TODO: Найти драйвер по имени сервиса и выгрузить
    unsafe { io_unload_driver(driver_service_name) }
}

// =============================================================================
// Built-in Driver Registration
// =============================================================================

/// IoRegisterBuiltinDriver — регистрирует встроенный драйвер
///
/// Используется для драйверов, скомпилированных в ядро.
pub unsafe fn io_register_builtin_driver(
    driver_name: &UNICODE_STRING,
    driver_entry: PDRIVER_INITIALIZE,
) -> NTSTATUS {
    unsafe {
        match iop_load_driver(driver_name, driver_entry) {
            Ok(_) => STATUS_SUCCESS,
            Err(status) => status,
        }
    }
}
