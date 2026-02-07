//! Object Manager Tests
//!
//! Тесты для Object Manager (ob).
//! Покрывают: types, header, dir, flags, constants.

use crate::nt::UNICODE_STRING;
use crate::ob::dir::OBJECT_DIRECTORY;
use crate::ob::dir::OBJECT_DIRECTORY_ENTRY;
use crate::ob::dir::OBP_HASH_TABLE_SIZE;
use crate::ob::dir::OBP_LOOKUP_CONTEXT;
use crate::ob::dir::obp_hash_object_name;
use crate::ob::header::*;
use crate::ob::link::OBJECT_SYMBOLIC_LINK;
use crate::ob::types::*;
use crate::test::harness::KernelTest;

// =============================================================================
// Object Flags Tests - флаги объектов
// =============================================================================

/// Проверка констант флагов объектов
fn test_object_flags() {
    // Проверяем что флаги - степени двойки (битовые маски)
    assert_eq!(OB_FLAG_CREATE_INFO, 0x01);
    assert_eq!(OB_FLAG_KERNEL_MODE, 0x02);
    assert_eq!(OB_FLAG_CREATOR_INFO, 0x04);
    assert_eq!(OB_FLAG_EXCLUSIVE, 0x08);
    assert_eq!(OB_FLAG_PERMANENT, 0x10);
    assert_eq!(OB_FLAG_SECURITY, 0x20);
    assert_eq!(OB_FLAG_SINGLE_PROCESS, 0x40);
    assert_eq!(OB_FLAG_DEFER_DELETE, 0x80);

    // Проверяем что флаги не перекрываются
    let all_flags = OB_FLAG_CREATE_INFO
        | OB_FLAG_KERNEL_MODE
        | OB_FLAG_CREATOR_INFO
        | OB_FLAG_EXCLUSIVE
        | OB_FLAG_PERMANENT
        | OB_FLAG_SECURITY
        | OB_FLAG_SINGLE_PROCESS
        | OB_FLAG_DEFER_DELETE;
    assert_eq!(all_flags, 0xFF);
}

/// Проверка констант атрибутов объектов (OBJ_*)
fn test_object_attributes_flags() {
    // Проверяем значения флагов
    assert_eq!(OBJ_INHERIT, 0x00000002);
    assert_eq!(OBJ_PROTECT_CLOSE, 0x00000001);
    assert_eq!(OBJ_PERMANENT, 0x00000010);
    assert_eq!(OBJ_EXCLUSIVE, 0x00000020);
    assert_eq!(OBJ_CASE_INSENSITIVE, 0x00000040);
    assert_eq!(OBJ_OPENLINK, 0x00000100);
    assert_eq!(OBJ_KERNEL_HANDLE, 0x00000200);
    assert_eq!(OBJ_OPENIF, 0x00000080);
    assert_eq!(OBJ_FORCE_ACCESS_CHECK, 0x00000400);
    assert_eq!(OBJ_KERNEL_EXCLUSIVE, 0x00010000);

    // Проверяем OBJ_VALID_KERNEL_ATTRIBUTES содержит все kernel атрибуты
    assert!((OBJ_VALID_KERNEL_ATTRIBUTES & OBJ_PROTECT_CLOSE) != 0);
    assert!((OBJ_VALID_KERNEL_ATTRIBUTES & OBJ_INHERIT) != 0);
    assert!((OBJ_VALID_KERNEL_ATTRIBUTES & OBJ_PERMANENT) != 0);
    assert!((OBJ_VALID_KERNEL_ATTRIBUTES & OBJ_EXCLUSIVE) != 0);
    assert!((OBJ_VALID_KERNEL_ATTRIBUTES & OBJ_CASE_INSENSITIVE) != 0);
    assert!((OBJ_VALID_KERNEL_ATTRIBUTES & OBJ_KERNEL_HANDLE) != 0);

    // OBJ_HANDLE_ATTRIBUTES - комбинация атрибутов handle
    assert_eq!(OBJ_HANDLE_ATTRIBUTES, OBJ_INHERIT | OBJ_PROTECT_CLOSE);
}

/// Проверка констант OB_OPEN_REASON
fn test_open_reason_constants() {
    assert_eq!(OB_OPEN_REASON_CREATE, 0);
    assert_eq!(OB_OPEN_REASON_OPEN, 1);
    assert_eq!(OB_OPEN_REASON_DUPLICATE, 2);
    assert_eq!(OB_OPEN_REASON_INHERIT, 3);

    // Все причины уникальны
    let reasons = [
        OB_OPEN_REASON_CREATE,
        OB_OPEN_REASON_OPEN,
        OB_OPEN_REASON_DUPLICATE,
        OB_OPEN_REASON_INHERIT,
    ];
    for i in 0..reasons.len() {
        for j in (i + 1)..reasons.len() {
            assert_ne!(reasons[i], reasons[j]);
        }
    }
}

// =============================================================================
// Access Rights Tests - права доступа
// =============================================================================

/// Проверка стандартных прав доступа
fn test_standard_rights() {
    // Стандартные права
    assert_eq!(DELETE, 0x00010000);
    assert_eq!(READ_CONTROL, 0x00020000);
    assert_eq!(WRITE_DAC, 0x00040000);
    assert_eq!(WRITE_OWNER, 0x00080000);
    assert_eq!(SYNCHRONIZE, 0x00100000);

    // STANDARD_RIGHTS_* используют READ_CONTROL как базу
    assert_eq!(STANDARD_RIGHTS_READ, 0x00020000);
    assert_eq!(STANDARD_RIGHTS_WRITE, 0x00020000);
    assert_eq!(STANDARD_RIGHTS_EXECUTE, 0x00020000);

    // STANDARD_RIGHTS_ALL включает все стандартные права
    assert_eq!(STANDARD_RIGHTS_ALL, 0x001F0000);
    assert!((STANDARD_RIGHTS_ALL & DELETE) != 0);
    assert!((STANDARD_RIGHTS_ALL & READ_CONTROL) != 0);
    assert!((STANDARD_RIGHTS_ALL & WRITE_DAC) != 0);
    assert!((STANDARD_RIGHTS_ALL & WRITE_OWNER) != 0);
    assert!((STANDARD_RIGHTS_ALL & SYNCHRONIZE) != 0);
}

/// Проверка прав доступа к директории
fn test_directory_access_rights() {
    assert_eq!(DIRECTORY_QUERY, 0x0001);
    assert_eq!(DIRECTORY_TRAVERSE, 0x0002);
    assert_eq!(DIRECTORY_CREATE_OBJECT, 0x0004);
    assert_eq!(DIRECTORY_CREATE_SUBDIRECTORY, 0x0008);

    // DIRECTORY_ALL_ACCESS
    let specific = DIRECTORY_QUERY
        | DIRECTORY_TRAVERSE
        | DIRECTORY_CREATE_OBJECT
        | DIRECTORY_CREATE_SUBDIRECTORY;
    assert_eq!(specific, 0x000F);
    assert_eq!(DIRECTORY_ALL_ACCESS, STANDARD_RIGHTS_ALL | 0x000F);
}

/// Проверка прав доступа к символической ссылке
fn test_symbolic_link_access_rights() {
    assert_eq!(SYMBOLIC_LINK_QUERY, 0x0001);
    assert_eq!(
        SYMBOLIC_LINK_ALL_ACCESS,
        STANDARD_RIGHTS_ALL | SYMBOLIC_LINK_QUERY
    );
}

/// Проверка прав доступа к типу объекта
fn test_object_type_access_rights() {
    assert_eq!(OBJECT_TYPE_CREATE, 0x0001);
    assert_eq!(
        OBJECT_TYPE_ALL_ACCESS,
        STANDARD_RIGHTS_ALL | OBJECT_TYPE_CREATE
    );
}

// =============================================================================
// Pool Types Tests
// =============================================================================

/// Проверка констант pool types
fn test_pool_types() {
    assert_eq!(NON_PAGED_POOL, 0);
    assert_eq!(PAGED_POOL, 1);
    assert_ne!(NON_PAGED_POOL, PAGED_POOL);
}

// =============================================================================
// OBJECT_ATTRIBUTES Tests
// =============================================================================

/// Проверка создания OBJECT_ATTRIBUTES
fn test_object_attributes_new() {
    let attrs = OBJECT_ATTRIBUTES::new();

    // Проверяем что length правильно установлен
    assert_eq!(
        attrs.length as usize,
        core::mem::size_of::<OBJECT_ATTRIBUTES>()
    );

    // Все указатели должны быть null
    assert!(attrs.root_directory.is_null());
    assert!(attrs.object_name.is_null());
    assert!(attrs.security_descriptor.is_null());
    assert!(attrs.security_quality_of_service.is_null());

    // Атрибуты должны быть 0
    assert_eq!(attrs.attributes, 0);
}

/// Проверка Default trait для OBJECT_ATTRIBUTES
fn test_object_attributes_default() {
    let attrs = OBJECT_ATTRIBUTES::default();
    let new_attrs = OBJECT_ATTRIBUTES::new();

    assert_eq!(attrs.length, new_attrs.length);
    assert_eq!(attrs.attributes, new_attrs.attributes);
}

/// Проверка метода init для OBJECT_ATTRIBUTES
fn test_object_attributes_init() {
    let mut attrs = OBJECT_ATTRIBUTES::new();
    let dummy_ptr = 0x1000 as *mut core::ffi::c_void;

    attrs.init(
        core::ptr::null_mut(),                    // name
        OBJ_KERNEL_HANDLE | OBJ_CASE_INSENSITIVE, // attributes
        dummy_ptr,                                // root_directory
        core::ptr::null_mut(),                    // security_descriptor
    );

    // Проверяем что length правильно установлен после init
    assert_eq!(
        attrs.length as usize,
        core::mem::size_of::<OBJECT_ATTRIBUTES>()
    );

    // Проверяем что атрибуты установлены
    assert_eq!(attrs.attributes, OBJ_KERNEL_HANDLE | OBJ_CASE_INSENSITIVE);
    assert_eq!(attrs.root_directory, dummy_ptr);
}

// =============================================================================
// GENERIC_MAPPING Tests
// =============================================================================

/// Проверка создания GENERIC_MAPPING
fn test_generic_mapping_new() {
    let mapping = GENERIC_MAPPING::new();

    assert_eq!(mapping.generic_read, 0);
    assert_eq!(mapping.generic_write, 0);
    assert_eq!(mapping.generic_execute, 0);
    assert_eq!(mapping.generic_all, 0);
}

/// Проверка Default trait для GENERIC_MAPPING
fn test_generic_mapping_default() {
    let mapping = GENERIC_MAPPING::default();
    let new_mapping = GENERIC_MAPPING::new();

    assert_eq!(mapping.generic_read, new_mapping.generic_read);
    assert_eq!(mapping.generic_write, new_mapping.generic_write);
    assert_eq!(mapping.generic_execute, new_mapping.generic_execute);
    assert_eq!(mapping.generic_all, new_mapping.generic_all);
}

// =============================================================================
// OBJECT_TYPE_INITIALIZER Tests
// =============================================================================

/// Проверка создания OBJECT_TYPE_INITIALIZER
fn test_object_type_initializer_new() {
    let init = OBJECT_TYPE_INITIALIZER::new();

    // Проверяем length
    assert_eq!(
        init.length as usize,
        core::mem::size_of::<OBJECT_TYPE_INITIALIZER>()
    );

    // Все булевы флаги false
    assert!(!init.case_insensitive);
    assert!(!init.unnamed_objects_only);
    assert!(!init.use_default_object);
    assert!(!init.security_required);
    assert!(!init.maintain_handle_count);
    assert!(!init.maintain_type_list);
    assert!(!init.supports_object_callbacks);
    assert!(!init.cache_aligned);

    // Числовые поля 0
    assert_eq!(init.object_type_flags, 0);
    assert_eq!(init.object_type_code, 0);
    assert_eq!(init.invalid_attributes, 0);
    assert_eq!(init.valid_access_mask, 0);
    assert_eq!(init.retain_access, 0);
    assert_eq!(init.pool_type, 0);
    assert_eq!(init.default_paged_pool_charge, 0);
    assert_eq!(init.default_non_paged_pool_charge, 0);

    // Все callbacks None
    assert!(init.open_procedure.is_none());
    assert!(init.close_procedure.is_none());
    assert!(init.delete_procedure.is_none());
    assert!(init.parse_procedure.is_none());
    assert!(init.security_procedure.is_none());
    assert!(init.query_name_procedure.is_none());
}

/// Проверка Default trait для OBJECT_TYPE_INITIALIZER
fn test_object_type_initializer_default() {
    let init = OBJECT_TYPE_INITIALIZER::default();
    let new_init = OBJECT_TYPE_INITIALIZER::new();

    assert_eq!(init.length, new_init.length);
    assert_eq!(init.pool_type, new_init.pool_type);
}

// =============================================================================
// OBJECT_CREATE_INFORMATION Tests
// =============================================================================

/// Проверка создания OBJECT_CREATE_INFORMATION
fn test_object_create_information_new() {
    let info = OBJECT_CREATE_INFORMATION::new();

    assert_eq!(info.attributes, 0);
    assert!(info.root_directory.is_null());
    assert_eq!(info.probe_mode, 0);
    assert_eq!(info.paged_pool_charge, 0);
    assert_eq!(info.non_paged_pool_charge, 0);
    assert_eq!(info.security_descriptor_charge, 0);
    assert!(info.security_descriptor.is_null());
    assert!(info.security_qos.is_null());
}

// =============================================================================
// OBJECT_HEADER Tests
// =============================================================================

/// Проверка создания OBJECT_HEADER
fn test_object_header_new() {
    let header = OBJECT_HEADER::new();

    // Начальный pointer_count должен быть 1
    assert_eq!(header.pointer_count(), 1);

    // Handle count должен быть 0
    assert_eq!(header.handle_count(), 0);

    // Флаги должны быть 0
    assert_eq!(header.flags, 0);

    // Offsets должны быть 0 (нет optional headers)
    assert_eq!(header.name_info_offset, 0);
    assert_eq!(header.handle_info_offset, 0);
    assert_eq!(header.quota_info_offset, 0);

    // Указатели null
    assert!(header.security_descriptor.is_null());
    assert!(header.type_ptr.is_null());
}

/// Проверка метода body() для OBJECT_HEADER
fn test_object_header_body() {
    let header = OBJECT_HEADER::new();
    let body = header.body();

    // body должен указывать сразу после header
    let header_ptr = &header as *const OBJECT_HEADER as usize;
    let body_ptr = body as usize;

    assert_eq!(body_ptr, header_ptr + core::mem::size_of::<OBJECT_HEADER>());
}

/// Проверка методов name_info/handle_info/quota_info когда нет optional headers
fn test_object_header_no_optional_info() {
    let header = OBJECT_HEADER::new();

    // При offset=0 методы должны возвращать None
    assert!(header.name_info().is_none());
    assert!(header.handle_info().is_none());
    assert!(header.quota_info().is_none());
    assert!(header.creator_info().is_none());
}

// =============================================================================
// Header Info Structures Tests
// =============================================================================

/// Проверка создания OBJECT_HEADER_NAME_INFO
fn test_object_header_name_info_new() {
    let name_info = OBJECT_HEADER_NAME_INFO::new();

    assert!(name_info.directory.is_null());
    assert!(name_info.name.buffer.is_null());
    assert_eq!(name_info.name.length, 0);
    assert_eq!(name_info.query_references, 1);
}

/// Проверка создания OBJECT_HEADER_CREATOR_INFO
fn test_object_header_creator_info_new() {
    let creator_info = OBJECT_HEADER_CREATOR_INFO::new();

    assert_eq!(creator_info.creator_back_trace_index, 0);
    assert_eq!(creator_info.reserved, 0);
    assert!(creator_info.creator_unique_process.is_null());
}

/// Проверка создания OBJECT_HEADER_HANDLE_INFO
fn test_object_header_handle_info_new() {
    let handle_info = OBJECT_HEADER_HANDLE_INFO::new();

    assert!(handle_info.single_entry.process.is_null());
    assert_eq!(handle_info.single_entry.handle_count, 0);
}

/// Проверка создания OBJECT_HEADER_QUOTA_INFO
fn test_object_header_quota_info_new() {
    let quota_info = OBJECT_HEADER_QUOTA_INFO::new();

    assert_eq!(quota_info.paged_pool_charge, 0);
    assert_eq!(quota_info.non_paged_pool_charge, 0);
    assert_eq!(quota_info.security_descriptor_charge, 0);
    assert!(quota_info.exclusive_process.is_null());
}

// =============================================================================
// OBJECT_DIRECTORY Tests
// =============================================================================

/// Проверка создания OBJECT_DIRECTORY
fn test_object_directory_new() {
    let dir = OBJECT_DIRECTORY::new();

    // Все buckets должны быть null
    for bucket in &dir.hash_buckets {
        assert!(bucket.is_null());
    }

    assert!(dir.device_map.is_null());
    assert_eq!(dir.session_id, 0);
    assert!(dir.namespace_entry.is_null());
    assert_eq!(dir.flags, 0);
}

/// Проверка константы OBP_HASH_TABLE_SIZE
fn test_hash_table_size() {
    // NT использует 37 buckets (простое число)
    assert_eq!(OBP_HASH_TABLE_SIZE, 37);
}

/// Проверка создания OBJECT_DIRECTORY_ENTRY
fn test_object_directory_entry_new() {
    let entry = OBJECT_DIRECTORY_ENTRY::new();

    assert!(entry.chain_link.is_null());
    assert!(entry.object.is_null());
    assert_eq!(entry.hash_value, 0);
}

// =============================================================================
// OBP_LOOKUP_CONTEXT Tests
// =============================================================================

/// Проверка создания OBP_LOOKUP_CONTEXT
fn test_lookup_context_new() {
    let ctx = OBP_LOOKUP_CONTEXT::new();

    assert!(ctx.directory.is_null());
    assert!(ctx.object.is_null());
    assert!(ctx.entry.is_null());
    assert_eq!(ctx.hash_value, 0);
    assert_eq!(ctx.directory_lock_mode, 0);
}

/// Проверка метода init для OBP_LOOKUP_CONTEXT
fn test_lookup_context_init() {
    let mut ctx = OBP_LOOKUP_CONTEXT {
        directory: 0x1000 as *mut OBJECT_DIRECTORY,
        object: 0x2000 as *mut core::ffi::c_void,
        entry: 0x3000 as *mut *mut OBJECT_DIRECTORY_ENTRY,
        hash_value: 12345,
        directory_lock_mode: 2,
    };

    ctx.init();

    // После init все должно быть сброшено
    assert!(ctx.directory.is_null());
    assert!(ctx.object.is_null());
    assert!(ctx.entry.is_null());
    assert_eq!(ctx.hash_value, 0);
    assert_eq!(ctx.directory_lock_mode, 0);
}

// =============================================================================
// Hash Function Tests
// =============================================================================

/// Проверка hash функции для пустого имени
fn test_hash_empty_name() {
    let empty_name = UNICODE_STRING::new();
    let hash = obp_hash_object_name(&empty_name);

    assert_eq!(hash, 0);
}

/// Проверка hash функции - одинаковые строки дают одинаковый hash
fn test_hash_consistency() {
    // "Test" в UTF-16
    static TEST_NAME: [u16; 5] = [b'T' as u16, b'e' as u16, b's' as u16, b't' as u16, 0];

    let name = UNICODE_STRING {
        length: 8, // 4 chars * 2 bytes
        maximum_length: 10,
        buffer: TEST_NAME.as_ptr() as *mut u16,
    };

    let hash1 = obp_hash_object_name(&name);
    let hash2 = obp_hash_object_name(&name);

    // Hash должен быть детерминистичным
    assert_eq!(hash1, hash2);

    // Hash должен быть ненулевым для непустой строки
    assert_ne!(hash1, 0);
}

/// Проверка hash функции - case insensitivity (hash всегда upcase)
fn test_hash_case_insensitive() {
    // "Test" в UTF-16
    static TEST_UPPER: [u16; 5] = [b'T' as u16, b'E' as u16, b'S' as u16, b'T' as u16, 0];
    static TEST_LOWER: [u16; 5] = [b't' as u16, b'e' as u16, b's' as u16, b't' as u16, 0];
    static TEST_MIXED: [u16; 5] = [b'T' as u16, b'e' as u16, b'S' as u16, b't' as u16, 0];

    let upper_name = UNICODE_STRING {
        length: 8,
        maximum_length: 10,
        buffer: TEST_UPPER.as_ptr() as *mut u16,
    };

    let lower_name = UNICODE_STRING {
        length: 8,
        maximum_length: 10,
        buffer: TEST_LOWER.as_ptr() as *mut u16,
    };

    let mixed_name = UNICODE_STRING {
        length: 8,
        maximum_length: 10,
        buffer: TEST_MIXED.as_ptr() as *mut u16,
    };

    let hash_upper = obp_hash_object_name(&upper_name);
    let hash_lower = obp_hash_object_name(&lower_name);
    let hash_mixed = obp_hash_object_name(&mixed_name);

    // Все варианты должны давать одинаковый hash (т.к. hash всегда upcase)
    assert_eq!(hash_upper, hash_lower);
    assert_eq!(hash_upper, hash_mixed);
}

/// Проверка hash функции - разные строки дают разные hash (обычно)
fn test_hash_different_strings() {
    static NAME_A: [u16; 2] = [b'A' as u16, 0];
    static NAME_B: [u16; 2] = [b'B' as u16, 0];

    let name_a = UNICODE_STRING {
        length: 2,
        maximum_length: 4,
        buffer: NAME_A.as_ptr() as *mut u16,
    };

    let name_b = UNICODE_STRING {
        length: 2,
        maximum_length: 4,
        buffer: NAME_B.as_ptr() as *mut u16,
    };

    let hash_a = obp_hash_object_name(&name_a);
    let hash_b = obp_hash_object_name(&name_b);

    // Разные строки должны (почти всегда) давать разные hash
    assert_ne!(hash_a, hash_b);
}

/// Проверка hash функции для типичных имен NT
fn test_hash_typical_names() {
    // Типичные имена объектов NT
    static PROCESS: [u16; 8] = [
        b'P' as u16,
        b'r' as u16,
        b'o' as u16,
        b'c' as u16,
        b'e' as u16,
        b's' as u16,
        b's' as u16,
        0,
    ];
    static THREAD: [u16; 7] = [
        b'T' as u16,
        b'h' as u16,
        b'r' as u16,
        b'e' as u16,
        b'a' as u16,
        b'd' as u16,
        0,
    ];
    static DIRECTORY: [u16; 10] = [
        b'D' as u16,
        b'i' as u16,
        b'r' as u16,
        b'e' as u16,
        b'c' as u16,
        b't' as u16,
        b'o' as u16,
        b'r' as u16,
        b'y' as u16,
        0,
    ];

    let process_name = UNICODE_STRING {
        length: 14,
        maximum_length: 16,
        buffer: PROCESS.as_ptr() as *mut u16,
    };

    let thread_name = UNICODE_STRING {
        length: 12,
        maximum_length: 14,
        buffer: THREAD.as_ptr() as *mut u16,
    };

    let directory_name = UNICODE_STRING {
        length: 18,
        maximum_length: 20,
        buffer: DIRECTORY.as_ptr() as *mut u16,
    };

    // Все должны давать ненулевой hash
    assert_ne!(obp_hash_object_name(&process_name), 0);
    assert_ne!(obp_hash_object_name(&thread_name), 0);
    assert_ne!(obp_hash_object_name(&directory_name), 0);

    // И все должны быть разными
    let h1 = obp_hash_object_name(&process_name);
    let h2 = obp_hash_object_name(&thread_name);
    let h3 = obp_hash_object_name(&directory_name);

    assert_ne!(h1, h2);
    assert_ne!(h1, h3);
    assert_ne!(h2, h3);
}

// =============================================================================
// OBJECT_SYMBOLIC_LINK Tests
// =============================================================================

/// Проверка создания OBJECT_SYMBOLIC_LINK
fn test_object_symbolic_link_new() {
    let link = OBJECT_SYMBOLIC_LINK::new();

    assert_eq!(link.creation_time.quad_part, 0);
    assert!(link.link_target.buffer.is_null());
    assert_eq!(link.link_target.length, 0);
    assert_eq!(link.dos_device_drive_index, 0);
    assert!(link.link_target_object.is_null());
    assert_eq!(link.link_target_remaining_access, 0);
    assert_eq!(link.flags, 0);
    assert_eq!(link.access_mask, 0);
}

// =============================================================================
// Size Tests - проверка размеров структур
// =============================================================================

/// Проверка размеров ключевых структур
fn test_structure_sizes() {
    // OBJECT_ATTRIBUTES должен быть 48 байт на x64 (6 полей по 8 байт в среднем)
    // Точный размер зависит от выравнивания
    assert!(core::mem::size_of::<OBJECT_ATTRIBUTES>() >= 24);

    // GENERIC_MAPPING - 4 ULONG = 16 байт
    assert_eq!(core::mem::size_of::<GENERIC_MAPPING>(), 16);

    // OBJECT_HEADER - проверяем что не слишком маленький
    assert!(core::mem::size_of::<OBJECT_HEADER>() >= 32);

    // OBJECT_DIRECTORY - массив указателей + дополнительные поля
    assert!(
        core::mem::size_of::<OBJECT_DIRECTORY>()
            >= OBP_HASH_TABLE_SIZE * core::mem::size_of::<usize>()
    );
}

// =============================================================================
// Реестр тестов OB
// =============================================================================

/// Все тесты Object Manager
pub static OB_TESTS: &[KernelTest] = &[
    // Object Flags tests
    KernelTest {
        name: "object_flags",
        module: "ob::types",
        test_fn: test_object_flags,
    },
    KernelTest {
        name: "object_attributes_flags",
        module: "ob::types",
        test_fn: test_object_attributes_flags,
    },
    KernelTest {
        name: "open_reason_constants",
        module: "ob::types",
        test_fn: test_open_reason_constants,
    },
    // Access Rights tests
    KernelTest {
        name: "standard_rights",
        module: "ob::types",
        test_fn: test_standard_rights,
    },
    KernelTest {
        name: "directory_access_rights",
        module: "ob::types",
        test_fn: test_directory_access_rights,
    },
    KernelTest {
        name: "symbolic_link_access_rights",
        module: "ob::types",
        test_fn: test_symbolic_link_access_rights,
    },
    KernelTest {
        name: "object_type_access_rights",
        module: "ob::types",
        test_fn: test_object_type_access_rights,
    },
    KernelTest {
        name: "pool_types",
        module: "ob::types",
        test_fn: test_pool_types,
    },
    // OBJECT_ATTRIBUTES tests
    KernelTest {
        name: "object_attributes_new",
        module: "ob::types",
        test_fn: test_object_attributes_new,
    },
    KernelTest {
        name: "object_attributes_default",
        module: "ob::types",
        test_fn: test_object_attributes_default,
    },
    KernelTest {
        name: "object_attributes_init",
        module: "ob::types",
        test_fn: test_object_attributes_init,
    },
    // GENERIC_MAPPING tests
    KernelTest {
        name: "generic_mapping_new",
        module: "ob::types",
        test_fn: test_generic_mapping_new,
    },
    KernelTest {
        name: "generic_mapping_default",
        module: "ob::types",
        test_fn: test_generic_mapping_default,
    },
    // OBJECT_TYPE_INITIALIZER tests
    KernelTest {
        name: "object_type_initializer_new",
        module: "ob::types",
        test_fn: test_object_type_initializer_new,
    },
    KernelTest {
        name: "object_type_initializer_default",
        module: "ob::types",
        test_fn: test_object_type_initializer_default,
    },
    // OBJECT_CREATE_INFORMATION tests
    KernelTest {
        name: "object_create_information_new",
        module: "ob::types",
        test_fn: test_object_create_information_new,
    },
    // OBJECT_HEADER tests
    KernelTest {
        name: "object_header_new",
        module: "ob::header",
        test_fn: test_object_header_new,
    },
    KernelTest {
        name: "object_header_body",
        module: "ob::header",
        test_fn: test_object_header_body,
    },
    KernelTest {
        name: "object_header_no_optional_info",
        module: "ob::header",
        test_fn: test_object_header_no_optional_info,
    },
    // Header Info tests
    KernelTest {
        name: "object_header_name_info_new",
        module: "ob::header",
        test_fn: test_object_header_name_info_new,
    },
    KernelTest {
        name: "object_header_creator_info_new",
        module: "ob::header",
        test_fn: test_object_header_creator_info_new,
    },
    KernelTest {
        name: "object_header_handle_info_new",
        module: "ob::header",
        test_fn: test_object_header_handle_info_new,
    },
    KernelTest {
        name: "object_header_quota_info_new",
        module: "ob::header",
        test_fn: test_object_header_quota_info_new,
    },
    // OBJECT_DIRECTORY tests
    KernelTest {
        name: "object_directory_new",
        module: "ob::dir",
        test_fn: test_object_directory_new,
    },
    KernelTest {
        name: "hash_table_size",
        module: "ob::dir",
        test_fn: test_hash_table_size,
    },
    KernelTest {
        name: "object_directory_entry_new",
        module: "ob::dir",
        test_fn: test_object_directory_entry_new,
    },
    // OBP_LOOKUP_CONTEXT tests
    KernelTest {
        name: "lookup_context_new",
        module: "ob::dir",
        test_fn: test_lookup_context_new,
    },
    KernelTest {
        name: "lookup_context_init",
        module: "ob::dir",
        test_fn: test_lookup_context_init,
    },
    // Hash Function tests
    KernelTest {
        name: "hash_empty_name",
        module: "ob::dir",
        test_fn: test_hash_empty_name,
    },
    KernelTest {
        name: "hash_consistency",
        module: "ob::dir",
        test_fn: test_hash_consistency,
    },
    KernelTest {
        name: "hash_case_insensitive",
        module: "ob::dir",
        test_fn: test_hash_case_insensitive,
    },
    KernelTest {
        name: "hash_different_strings",
        module: "ob::dir",
        test_fn: test_hash_different_strings,
    },
    KernelTest {
        name: "hash_typical_names",
        module: "ob::dir",
        test_fn: test_hash_typical_names,
    },
    // OBJECT_SYMBOLIC_LINK tests
    KernelTest {
        name: "object_symbolic_link_new",
        module: "ob::link",
        test_fn: test_object_symbolic_link_new,
    },
    // Structure size tests
    KernelTest {
        name: "structure_sizes",
        module: "ob::layout",
        test_fn: test_structure_sizes,
    },
];
