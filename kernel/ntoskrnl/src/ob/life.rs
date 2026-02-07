//! Object Lifecycle Management
//!
//! Создание и удаление объектов.
//!
//! Источники:
//! - ReactOS: ob/oblife.c

use core::sync::atomic::Ordering;

use super::header::*;
use super::init::OBP_DEFAULT_OBJECT;
use super::init::OBP_OBJECT_TYPES;
use super::init::OBP_TYPE_OBJECT_TYPE;
use super::types::*;
use crate::ex::pool::POOL_TYPE;
use crate::ex::pool::ex_allocate_pool_with_tag;
use crate::nt::LIST_ENTRY;
use crate::nt::NTSTATUS;
use crate::nt::PVOID;
use crate::nt::STATUS_INSUFFICIENT_RESOURCES;
use crate::nt::STATUS_INVALID_PARAMETER;
use crate::nt::STATUS_OBJECT_NAME_COLLISION;
use crate::nt::STATUS_OBJECT_NAME_EXISTS;
use crate::nt::STATUS_OBJECT_NAME_INVALID;
use crate::nt::STATUS_SUCCESS;
use crate::nt::ULONG;
use crate::nt::UNICODE_STRING;

// =============================================================================
// ObCreateObjectType
// =============================================================================

/// Создает новый тип объекта
///
/// # Arguments
/// * `type_name` - Имя типа (например, "Process", "Thread")
/// * `object_type_initializer` - Параметры инициализации типа
/// * `reserved` - Зарезервировано (должно быть null)
/// * `object_type` - Выходной указатель на созданный тип
///
/// # Returns
/// STATUS_SUCCESS или код ошибки
pub fn ob_create_object_type(
    type_name: *mut UNICODE_STRING,
    object_type_initializer: &OBJECT_TYPE_INITIALIZER,
    _reserved: PVOID,
    object_type: *mut *mut OBJECT_TYPE,
) -> NTSTATUS {
    // Проверяем параметры
    if type_name.is_null() || object_type.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    unsafe {
        let name = &*type_name;

        // Проверяем валидность имени
        if name.length == 0 || (name.length % 2) != 0 {
            return STATUS_INVALID_PARAMETER;
        }

        // Проверяем что имя не содержит разделителей
        let name_chars = core::slice::from_raw_parts(name.buffer, (name.length / 2) as usize);
        for &ch in name_chars {
            if ch == b'\\' as u16 {
                return STATUS_OBJECT_NAME_INVALID;
            }
        }

        // Проверяем инициализатор
        if object_type_initializer.length as usize
            != core::mem::size_of::<OBJECT_TYPE_INITIALIZER>()
        {
            return STATUS_INVALID_PARAMETER;
        }

        // Проверка: MaintainHandleCount требует OpenProcedure или CloseProcedure
        if object_type_initializer.maintain_handle_count
            && object_type_initializer.open_procedure.is_none()
            && object_type_initializer.close_procedure.is_none()
        {
            return STATUS_INVALID_PARAMETER;
        }

        // Получаем тип объекта "Type"
        let type_object_type = *OBP_TYPE_OBJECT_TYPE.get();

        // Вычисляем размеры
        let type_size = core::mem::size_of::<OBJECT_TYPE>();
        let header_size = core::mem::size_of::<OBJECT_HEADER>();
        let name_info_size = core::mem::size_of::<OBJECT_HEADER_NAME_INFO>();
        let creator_info_size = core::mem::size_of::<OBJECT_HEADER_CREATOR_INFO>();

        let total_size = name_info_size + creator_info_size + header_size + type_size;

        // Выбираем pool type
        let pool_type = if object_type_initializer.pool_type == NON_PAGED_POOL {
            POOL_TYPE::NonPagedPool
        } else {
            POOL_TYPE::PagedPool
        };

        // Создаем tag из имени типа
        let mut tag_bytes = [b' '; 4];
        let tag_len = core::cmp::min(name.length as usize / 2, 4);
        for i in 0..tag_len {
            tag_bytes[3 - i] = *name_chars.get(i).unwrap_or(&(b' ' as u16)) as u8;
        }
        let tag = u32::from_le_bytes(tag_bytes);

        // Выделяем память
        let ptr = ex_allocate_pool_with_tag(pool_type, total_size, tag);
        if ptr.is_null() {
            return STATUS_INSUFFICIENT_RESOURCES;
        }

        // Обнуляем память
        core::ptr::write_bytes(ptr, 0, total_size);

        // Настраиваем структуры
        let name_info_ptr = ptr as *mut OBJECT_HEADER_NAME_INFO;
        let creator_info_ptr =
            (ptr as *mut u8).add(name_info_size) as *mut OBJECT_HEADER_CREATOR_INFO;
        let header = (ptr as *mut u8).add(name_info_size + creator_info_size) as *mut OBJECT_HEADER;
        let new_type = (ptr as *mut u8).add(name_info_size + creator_info_size + header_size)
            as *mut OBJECT_TYPE;

        // Копируем имя типа
        let name_buffer_size = (name.length + 2) as usize; // +2 для null terminator
        let name_buffer = ex_allocate_pool_with_tag(
            POOL_TYPE::PagedPool,
            name_buffer_size,
            u32::from_le_bytes(*b"mNbO"),
        );

        if name_buffer.is_null() {
            crate::ex::pool::ex_free_pool_with_tag(ptr, tag);
            return STATUS_INSUFFICIENT_RESOURCES;
        }

        core::ptr::copy_nonoverlapping(
            name.buffer as *const u8,
            name_buffer as *mut u8,
            name.length as usize,
        );
        // Null terminate
        *((name_buffer as *mut u16).add((name.length / 2) as usize)) = 0;

        // Инициализируем name info
        (*name_info_ptr).query_references = 1;
        (*name_info_ptr).name.buffer = name_buffer as *mut u16;
        (*name_info_ptr).name.length = name.length;
        (*name_info_ptr).name.maximum_length = name.length + 2;

        // Инициализируем creator info
        (*creator_info_ptr).type_list = LIST_ENTRY::new();
        LIST_ENTRY::init_head(&mut (*creator_info_ptr).type_list);

        // Инициализируем header
        (*header).pointer_count = core::sync::atomic::AtomicIsize::new(1);
        (*header).handle_count_or_next_to_free.handle_count = 0;
        (*header).flags = OB_FLAG_KERNEL_MODE | OB_FLAG_PERMANENT | OB_FLAG_CREATOR_INFO;
        (*header).name_info_offset = (name_info_size + creator_info_size) as u8;
        (*header).type_ptr = type_object_type;

        // Инициализируем object type
        (*new_type).type_list = LIST_ENTRY::new();
        LIST_ENTRY::init_head(&mut (*new_type).type_list);
        (*new_type).name = (*name_info_ptr).name.clone();
        (*new_type).type_info = object_type_initializer.clone_init();
        (*new_type).key = tag;

        // Настраиваем default object
        if object_type_initializer.use_default_object {
            (*new_type).type_info.valid_access_mask |= SYNCHRONIZE;
            (*new_type).default_object = OBP_DEFAULT_OBJECT.get() as PVOID;
        }

        // Вычисляем header size для pool charges
        let header_charge = header_size
            + name_info_size
            + if object_type_initializer.maintain_handle_count {
                core::mem::size_of::<OBJECT_HEADER_HANDLE_INFO>()
            } else {
                0
            };

        if object_type_initializer.pool_type == NON_PAGED_POOL {
            (*new_type).type_info.default_non_paged_pool_charge += header_charge as ULONG;
        } else {
            (*new_type).type_info.default_paged_pool_charge += header_charge as ULONG;
        }

        // Увеличиваем счетчики типа Type
        if !type_object_type.is_null() {
            (*type_object_type).total_number_of_objects += 1;
            if (*type_object_type).total_number_of_objects
                > (*type_object_type).high_water_number_of_objects
            {
                (*type_object_type).high_water_number_of_objects =
                    (*type_object_type).total_number_of_objects;
            }

            // Устанавливаем индекс
            (*new_type).index = (*type_object_type).total_number_of_objects as u8;

            // Добавляем в массив типов
            let idx = ((*new_type).index - 1) as usize;
            if idx < OB_MAX_OBJECT_TYPES {
                let types = &mut *OBP_OBJECT_TYPES.get();
                types[idx] = new_type;
            }

            // Добавляем в список типов
            LIST_ENTRY::insert_tail(
                &mut (*type_object_type).type_list,
                &mut (*creator_info_ptr).type_list,
            );
        }

        *object_type = new_type;
        STATUS_SUCCESS
    }
}

// Вспомогательный метод для клонирования инициализатора
impl OBJECT_TYPE_INITIALIZER {
    fn clone_init(&self) -> Self {
        Self {
            length: self.length,
            object_type_flags: self.object_type_flags,
            case_insensitive: self.case_insensitive,
            unnamed_objects_only: self.unnamed_objects_only,
            use_default_object: self.use_default_object,
            security_required: self.security_required,
            maintain_handle_count: self.maintain_handle_count,
            maintain_type_list: self.maintain_type_list,
            supports_object_callbacks: self.supports_object_callbacks,
            cache_aligned: self.cache_aligned,
            _padding: self._padding,
            object_type_code: self.object_type_code,
            invalid_attributes: self.invalid_attributes,
            generic_mapping: self.generic_mapping,
            valid_access_mask: self.valid_access_mask,
            retain_access: self.retain_access,
            pool_type: self.pool_type,
            default_paged_pool_charge: self.default_paged_pool_charge,
            default_non_paged_pool_charge: self.default_non_paged_pool_charge,
            dump_procedure: self.dump_procedure,
            open_procedure: self.open_procedure,
            close_procedure: self.close_procedure,
            delete_procedure: self.delete_procedure,
            parse_procedure: self.parse_procedure,
            security_procedure: self.security_procedure,
            query_name_procedure: self.query_name_procedure,
            okay_to_close_procedure: self.okay_to_close_procedure,
            wait_object_flag_mask: self.wait_object_flag_mask,
            wait_object_flag_offset: self.wait_object_flag_offset,
            wait_object_pointer_offset: self.wait_object_pointer_offset,
        }
    }
}

// =============================================================================
// ObCreateObject
// =============================================================================

/// Создает новый объект указанного типа
///
/// # Arguments
/// * `probe_mode` - Режим проверки (KernelMode/UserMode)
/// * `object_type` - Тип создаваемого объекта
/// * `object_attributes` - Атрибуты объекта (имя, флаги и т.д.)
/// * `access_mode` - Режим доступа
/// * `parse_context` - Контекст парсинга (опционально)
/// * `object_size` - Размер тела объекта
/// * `paged_pool_charge` - Charge для paged pool (0 = default)
/// * `non_paged_pool_charge` - Charge для non-paged pool (0 = default)
/// * `object` - Выходной указатель на созданный объект
pub fn ob_create_object(
    probe_mode: u8,
    object_type: *mut OBJECT_TYPE,
    object_attributes: *const OBJECT_ATTRIBUTES,
    access_mode: u8,
    _parse_context: PVOID,
    object_size: usize,
    paged_pool_charge: ULONG,
    non_paged_pool_charge: ULONG,
    object: *mut PVOID,
) -> NTSTATUS {
    if object_type.is_null() || object.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    unsafe {
        let type_info = &(*object_type).type_info;

        // Определяем атрибуты
        let (attributes, has_name) = if !object_attributes.is_null() {
            let attrs = &*object_attributes;
            let has_name = !attrs.object_name.is_null() && (*attrs.object_name).length > 0;
            (attrs.attributes, has_name)
        } else {
            (0, false)
        };

        // Проверяем invalid attributes
        if (attributes & type_info.invalid_attributes) != 0 {
            return STATUS_INVALID_PARAMETER;
        }

        // Вычисляем размеры optional headers
        let quota_size = if (paged_pool_charge != type_info.default_paged_pool_charge)
            || (non_paged_pool_charge != type_info.default_non_paged_pool_charge)
            || ((attributes & OBJ_EXCLUSIVE) != 0)
        {
            core::mem::size_of::<OBJECT_HEADER_QUOTA_INFO>()
        } else {
            0
        };

        let handle_size = if type_info.maintain_handle_count {
            core::mem::size_of::<OBJECT_HEADER_HANDLE_INFO>()
        } else {
            0
        };

        let name_size = if has_name {
            core::mem::size_of::<OBJECT_HEADER_NAME_INFO>()
        } else {
            0
        };

        let creator_size = if type_info.maintain_type_list {
            core::mem::size_of::<OBJECT_HEADER_CREATOR_INFO>()
        } else {
            0
        };

        // Общий размер
        let header_size = core::mem::size_of::<OBJECT_HEADER>();
        let total_size =
            quota_size + handle_size + name_size + creator_size + header_size + object_size;

        // Pool type
        let pool_type = if type_info.pool_type == NON_PAGED_POOL {
            POOL_TYPE::NonPagedPool
        } else {
            POOL_TYPE::PagedPool
        };

        // Выделяем память
        let ptr = ex_allocate_pool_with_tag(pool_type, total_size, (*object_type).key);
        if ptr.is_null() {
            return STATUS_INSUFFICIENT_RESOURCES;
        }

        // Обнуляем
        core::ptr::write_bytes(ptr, 0, total_size);

        // Вычисляем указатели на структуры
        let mut current_offset = 0usize;

        let quota_info = if quota_size > 0 {
            let p = (ptr as *mut u8).add(current_offset) as *mut OBJECT_HEADER_QUOTA_INFO;
            current_offset += quota_size;
            p
        } else {
            core::ptr::null_mut()
        };

        let handle_info = if handle_size > 0 {
            let p = (ptr as *mut u8).add(current_offset) as *mut OBJECT_HEADER_HANDLE_INFO;
            current_offset += handle_size;
            p
        } else {
            core::ptr::null_mut()
        };

        let name_info = if name_size > 0 {
            let p = (ptr as *mut u8).add(current_offset) as *mut OBJECT_HEADER_NAME_INFO;
            current_offset += name_size;
            p
        } else {
            core::ptr::null_mut()
        };

        let creator_info = if creator_size > 0 {
            let p = (ptr as *mut u8).add(current_offset) as *mut OBJECT_HEADER_CREATOR_INFO;
            current_offset += creator_size;
            p
        } else {
            core::ptr::null_mut()
        };

        let header = (ptr as *mut u8).add(current_offset) as *mut OBJECT_HEADER;
        let body = (ptr as *mut u8).add(current_offset + header_size) as PVOID;

        // Инициализируем quota info
        if !quota_info.is_null() {
            let paged = if paged_pool_charge != 0 {
                paged_pool_charge
            } else {
                type_info.default_paged_pool_charge
            };
            let non_paged = if non_paged_pool_charge != 0 {
                non_paged_pool_charge
            } else {
                type_info.default_non_paged_pool_charge
            };
            (*quota_info).paged_pool_charge = paged;
            (*quota_info).non_paged_pool_charge = non_paged;
        }

        // Инициализируем handle info
        if !handle_info.is_null() {
            (*handle_info).single_entry.handle_count = 0;
        }

        // Инициализируем name info
        if !name_info.is_null() && has_name {
            let src_name = &*(*object_attributes).object_name;

            // TODO: на этом этапе не поддерживаем имена с '\\' внутри (полный path)
            // — это будет реализовано в ObpLookupObjectName/parse (Этап 4).
            // Сейчас принимаем только leaf‑имя для вставки в конкретную директорию.
            let chars =
                core::slice::from_raw_parts(src_name.buffer, (src_name.length / 2) as usize);
            for &ch in chars {
                if ch == b'\\' as u16 {
                    crate::ex::pool::ex_free_pool_with_tag(ptr, (*object_type).key);
                    return STATUS_OBJECT_NAME_INVALID;
                }
            }

            // Копируем имя
            let name_buffer_size = (src_name.length + 2) as usize;
            let name_buffer = ex_allocate_pool_with_tag(
                POOL_TYPE::PagedPool,
                name_buffer_size,
                u32::from_le_bytes(*b"mNbO"),
            );

            if !name_buffer.is_null() {
                core::ptr::copy_nonoverlapping(
                    src_name.buffer as *const u8,
                    name_buffer as *mut u8,
                    src_name.length as usize,
                );
                *((name_buffer as *mut u16).add((src_name.length / 2) as usize)) = 0;

                (*name_info).name.buffer = name_buffer as *mut u16;
                (*name_info).name.length = src_name.length;
                (*name_info).name.maximum_length = src_name.length + 2;
            }
            (*name_info).query_references = 1;
        }

        // Инициализируем creator info
        if !creator_info.is_null() {
            (*creator_info).type_list = LIST_ENTRY::new();
            LIST_ENTRY::init_head(&mut (*creator_info).type_list);
        }

        // Инициализируем header
        (*header).pointer_count = core::sync::atomic::AtomicIsize::new(1);
        (*header).handle_count_or_next_to_free.handle_count = 0;
        (*header).type_ptr = object_type;
        (*header).security_descriptor = core::ptr::null_mut();
        (*header).object_create_info_or_quota.object_create_info = core::ptr::null_mut();

        // Создаём OBJECT_CREATE_INFORMATION, чтобы ObInsertObject мог узнать root_directory и атрибуты.
        // В NT эта структура освобождается после успешной вставки.
        let mut create_info_ptr: *mut OBJECT_CREATE_INFORMATION = core::ptr::null_mut();
        if !object_attributes.is_null() {
            let info = ex_allocate_pool_with_tag(
                POOL_TYPE::NonPagedPool,
                core::mem::size_of::<OBJECT_CREATE_INFORMATION>(),
                u32::from_le_bytes(*b"IfbO"), // 'ObfI'
            ) as *mut OBJECT_CREATE_INFORMATION;
            if !info.is_null() {
                *info = OBJECT_CREATE_INFORMATION::new();
                (*info).attributes = (*object_attributes).attributes;
                (*info).root_directory = (*object_attributes).root_directory;
                (*info).probe_mode = probe_mode;
                (*info).security_descriptor = (*object_attributes).security_descriptor;
                (*info).security_qos = (*object_attributes).security_quality_of_service;
                create_info_ptr = info;
                (*header).object_create_info_or_quota.object_create_info = info;
            }
        }

        // Устанавливаем offsets
        if quota_size > 0 {
            (*header).quota_info_offset =
                (quota_size + handle_size + name_size + creator_size) as u8;
        }
        if handle_size > 0 {
            (*header).handle_info_offset = (handle_size + name_size + creator_size) as u8;
        }
        if name_size > 0 {
            (*header).name_info_offset = (name_size + creator_size) as u8;
        }

        // Устанавливаем флаги
        let mut flags = 0u8;
        if !create_info_ptr.is_null() {
            flags |= OB_FLAG_CREATE_INFO;
        }
        if creator_size > 0 {
            flags |= OB_FLAG_CREATOR_INFO;
        }
        if handle_size > 0 {
            flags |= OB_FLAG_SINGLE_PROCESS;
        }
        if (attributes & OBJ_PERMANENT) != 0 {
            flags |= OB_FLAG_PERMANENT;
        }
        if (attributes & OBJ_EXCLUSIVE) != 0 {
            flags |= OB_FLAG_EXCLUSIVE;
        }
        if access_mode == 0 {
            // KernelMode
            flags |= OB_FLAG_KERNEL_MODE;
        }
        (*header).flags = flags;

        // Увеличиваем счетчики типа
        (*object_type).total_number_of_objects += 1;
        if (*object_type).total_number_of_objects > (*object_type).high_water_number_of_objects {
            (*object_type).high_water_number_of_objects = (*object_type).total_number_of_objects;
        }

        *object = body;
        STATUS_SUCCESS
    }
}

// =============================================================================
// ObInsertObject
// =============================================================================

/// Вставляет объект в namespace и/или handle table
///
/// Примечание: с Этапа 5 `ob_insert_object` создаёт только реальные handles через `HANDLE_TABLE`.
pub fn ob_insert_object(
    object: PVOID,
    _access_state: PVOID,
    desired_access: u32,
    _object_pointer_bias: ULONG,
    new_object: *mut PVOID,
    handle: *mut PVOID, // HANDLE*
) -> NTSTATUS {
    if object.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    // Итог: по умолчанию успех. Для OBJ_OPENIF при открытии существующего вернём STATUS_OBJECT_NAME_EXISTS.
    let mut final_status: NTSTATUS = STATUS_SUCCESS;

    unsafe {
        if !new_object.is_null() {
            *new_object = core::ptr::null_mut();
        }

        // В случае OBJ_OPENIF мы можем "переключиться" на уже существующий объект.
        let mut target_object: PVOID = object;
        let mut target_header: *mut OBJECT_HEADER = object_to_object_header(object);

        // Получаем create info (если есть) ТОЛЬКО у созданного объекта (не у target в случае openif).
        let created_header: *mut OBJECT_HEADER = target_header;
        let mut created_create_info: *mut OBJECT_CREATE_INFORMATION =
            if ((*created_header).flags & OB_FLAG_CREATE_INFO) != 0 {
                (*created_header)
                    .object_create_info_or_quota
                    .object_create_info
            } else {
                core::ptr::null_mut()
            };

        // Атрибуты для namespace/handle семантики (OBJ_*), если create info отсутствует — 0.
        let attributes: u32 = if !created_create_info.is_null() {
            (*created_create_info).attributes
        } else {
            0
        };

        // =========================================================================
        // 1) Namespace insertion (только leaf‑имя в одну директорию)
        // =========================================================================
        let mut inserted_into_namespace = false;
        let mut inserted_parent_dir: *mut super::dir::OBJECT_DIRECTORY = core::ptr::null_mut();

        if let Some(name_info) = (*target_header).name_info() {
            if !name_info.name.buffer.is_null() && name_info.name.length != 0 {
                let parent_dir = if !created_create_info.is_null()
                    && !(*created_create_info).root_directory.is_null()
                {
                    (*created_create_info).root_directory as *mut super::dir::OBJECT_DIRECTORY
                } else {
                    super::init::obp_get_root_directory()
                };

                if parent_dir.is_null() {
                    // Объект не вставлен — освобождаем созданный объект.
                    super::refcount::ob_dereference_object(object);
                    return crate::nt::STATUS_OBJECT_PATH_NOT_FOUND;
                }

                // Ищем коллизию имени.
                use super::dir::OBP_LOOKUP_CONTEXT;
                use super::dir::obp_insert_entry_directory;
                use super::dir::obp_lookup_entry_directory;
                use super::dir::obp_release_lookup_context;
                
                let mut ctx = OBP_LOOKUP_CONTEXT::new();
                let found = obp_lookup_entry_directory(
                    parent_dir,
                    &name_info.name,
                    attributes,
                    true, // look_for_insertion
                    &mut ctx,
                );

                if found && !ctx.object.is_null() {
                    // Имя уже занято.
                    let open_if = (attributes & OBJ_OPENIF) != 0;
                    if !open_if {
                        obp_release_lookup_context(&mut ctx);
                        // ObInsertObject в NT на ошибке сам освобождает объект.
                        super::refcount::ob_dereference_object(object);
                        return STATUS_OBJECT_NAME_COLLISION;
                    }

                    // OBJ_OPENIF: открываем существующий объект вместо созданного.
                    let existing = ctx.object;
                    obp_release_lookup_context(&mut ctx);

                    // Освобождаем OBJECT_CREATE_INFORMATION у созданного объекта сразу, чтобы не держать NonPaged память
                    // и не допустить double-free в delete пути.
                    if !created_create_info.is_null() {
                        crate::ex::pool::ex_free_pool_with_tag(
                            created_create_info as PVOID,
                            u32::from_le_bytes(*b"IfbO"),
                        );
                        (*created_header)
                            .object_create_info_or_quota
                            .object_create_info = core::ptr::null_mut();
                        (*created_header).flags &= !OB_FLAG_CREATE_INFO;
                        created_create_info = core::ptr::null_mut();
                    }

                    // Переключаемся на существующий объект.
                    target_object = existing;
                    target_header = object_to_object_header(existing);
                    final_status = STATUS_OBJECT_NAME_EXISTS;

                    // Созданный объект больше не нужен — освобождаем “создательскую” ссылку.
                    super::refcount::ob_dereference_object(object);
                } else {
                    // Вставляем новый объект в директорию.
                    if !obp_insert_entry_directory(parent_dir, &mut ctx, target_header) {
                        obp_release_lookup_context(&mut ctx);
                        super::refcount::ob_dereference_object(object);
                        return STATUS_INSUFFICIENT_RESOURCES;
                    }
                    obp_release_lookup_context(&mut ctx);
                    inserted_into_namespace = true;
                    inserted_parent_dir = parent_dir;
                }
            }
        }

        // =========================================================================
        // 1.5) Назначение Security Descriptor (если объект новый, не OBJ_OPENIF)
        // =========================================================================
        if final_status == STATUS_SUCCESS && (*target_header).security_descriptor.is_null() {
            // Назначаем SD только для нового объекта
            let obj_type = (*target_header).type_ptr;
            let security_required = if !obj_type.is_null() {
                (*obj_type).type_info.security_required
            } else {
                false
            };

            if security_required {
                // Получаем explicit SD из create info
                let explicit_sd = if !created_create_info.is_null() {
                    (*created_create_info).security_descriptor as *const crate::nt::SECURITY_DESCRIPTOR
                } else {
                    core::ptr::null()
                };

                // Получаем parent SD (из директории если вставляли в namespace)
                let parent_sd = if inserted_into_namespace && !inserted_parent_dir.is_null() {
                    let parent_header = object_to_object_header(inserted_parent_dir as PVOID);
                    (*parent_header).security_descriptor as *const crate::nt::SECURITY_DESCRIPTOR
                } else {
                    core::ptr::null()
                };

                // Получаем generic mapping от типа
                let generic_mapping = if !obj_type.is_null() {
                    &(*obj_type).type_info.generic_mapping as *const _
                } else {
                    core::ptr::null()
                };

                // Создаём subject context для текущего потока
                let mut subject_context = crate::se::subject::SECURITY_SUBJECT_CONTEXT::new();
                crate::se::subject::se_capture_subject_context(&mut subject_context);

                // Назначаем SD
                let mut new_sd: *mut crate::nt::SECURITY_DESCRIPTOR = core::ptr::null_mut();
                let status = crate::se::sd::se_assign_security(
                    parent_sd,
                    explicit_sd,
                    &mut new_sd,
                    false, // не директория (TODO: определять из типа)
                    &mut subject_context,
                    generic_mapping,
                    crate::ex::pool::POOL_TYPE::PagedPool,
                );

                crate::se::subject::se_release_subject_context(&mut subject_context);

                if status == STATUS_SUCCESS && !new_sd.is_null() {
                    (*target_header).security_descriptor = new_sd as PVOID;
                }
                // Если не удалось назначить SD - продолжаем без него (объект всё равно создаётся)
            }
        }

        // =========================================================================
        // 2) Создание handle (через HANDLE_TABLE, с fallback на pointer-handle)
        // =========================================================================
        if !handle.is_null() {
            // Определяем таблицу: kernel handles или таблица текущего процесса
            const OBP_HANDLE_FLAG_KERNEL: usize = 0x1;
            let mut kernel_handle = (attributes & OBJ_KERNEL_HANDLE) != 0;

            let table = if kernel_handle {
                super::init::OBP_KERNEL_HANDLE_TABLE.load(Ordering::Acquire)
            } else {
                let process = crate::ps::ps_get_current_process();
                if process.is_null() {
                    core::ptr::null_mut()
                } else {
                    (*process).object_table.load(Ordering::Acquire)
                }
            };

            // Этап 5: handle обязан быть реальным. Если таблицы процесса нет, а мы в kernel контексте,
            // деградируем в kernel handle table.
            let table = if !table.is_null() {
                table
            } else if kernel_handle || new_object.is_null() {
                // В ранних фазах/внутренних вызовах без user-возврата — используем kernel handles.
                kernel_handle = true;
                super::init::OBP_KERNEL_HANDLE_TABLE.load(Ordering::Acquire)
            } else {
                core::ptr::null_mut()
            };

            if table.is_null() {
                // Откат: если объект был вставлен в namespace — удаляем entry.
                if inserted_into_namespace && !inserted_parent_dir.is_null() {
                    let _ =
                        obp_remove_namespace_entry(inserted_parent_dir, target_header, attributes);
                }
                super::refcount::ob_dereference_object(target_object);
                return STATUS_INSUFFICIENT_RESOURCES;
            }

            // Любой handle должен удерживать ссылку на объект
            super::refcount::ob_reference_object(target_object);

            let mut created_table: *mut crate::ex::handle::HANDLE_TABLE = core::ptr::null_mut();
            let mut created_handle: usize = crate::ex::handle::INVALID_HANDLE_VALUE;

            // Атрибуты handle: OBJ_* -> HANDLE_FLAG_*.
            let mut handle_flags: u32 = 0;
            if (attributes & OBJ_INHERIT) != 0 {
                handle_flags |= crate::ex::handle::HANDLE_FLAG_INHERIT;
            }
            if (attributes & OBJ_PROTECT_CLOSE) != 0 {
                handle_flags |= crate::ex::handle::HANDLE_FLAG_PROTECT_FROM_CLOSE;
            }
            let h = crate::ex::handle::ex_create_handle_ex(
                table,
                target_object,
                desired_access,
                handle_flags,
            );
            if h == crate::ex::handle::INVALID_HANDLE_VALUE {
                // Откат: если объект был вставлен в namespace — удаляем entry.
                if inserted_into_namespace && !inserted_parent_dir.is_null() {
                    let _ =
                        obp_remove_namespace_entry(inserted_parent_dir, target_header, attributes);
                }
                super::refcount::ob_dereference_object(target_object); // Снимаем ref, взятый для handle
                return STATUS_INSUFFICIENT_RESOURCES;
            }

            created_table = table;
            created_handle = h;
            *handle = ((h | if kernel_handle {
                OBP_HANDLE_FLAG_KERNEL
            } else {
                0
            }) as usize) as PVOID;

            // Увеличиваем handle count объекта (временно в заголовке как integer)
            (*target_header).handle_count_or_next_to_free.handle_count += 1;

            // Увеличиваем счетчик хэндлов типа + вызываем open callback при наличии
            let obj_type = (*target_header).type_ptr;
            if !obj_type.is_null() {
                (*obj_type).total_number_of_handles += 1;
                if (*obj_type).total_number_of_handles > (*obj_type).high_water_number_of_handles {
                    (*obj_type).high_water_number_of_handles = (*obj_type).total_number_of_handles;
                }

                if let Some(open_proc) = (*obj_type).type_info.open_procedure {
                    let st = open_proc(
                        OB_OPEN_REASON_CREATE,
                        core::ptr::null_mut(),
                        target_object,
                        desired_access,
                        (*target_header).handle_count() as ULONG,
                    );
                    if st != STATUS_SUCCESS {
                        (*target_header).handle_count_or_next_to_free.handle_count -= 1;
                        (*obj_type).total_number_of_handles -= 1;
                        // Откат: удаляем handle из таблицы (если успели создать) и снимаем ссылку.
                        if !created_table.is_null()
                            && created_handle != crate::ex::handle::INVALID_HANDLE_VALUE
                        {
                            let _ =
                                crate::ex::handle::ex_destroy_handle(created_table, created_handle);
                        }
                        super::refcount::ob_dereference_object(target_object);
                        // Откат namespace insertion если это был созданный объект
                        if inserted_into_namespace && !inserted_parent_dir.is_null() {
                            let _ = obp_remove_namespace_entry(
                                inserted_parent_dir,
                                target_header,
                                attributes,
                            );
                        }
                        return st;
                    }
                }
            }
        }

        // =========================================================================
        // 3) Завершение: освобождаем OBJECT_CREATE_INFORMATION
        // =========================================================================
        // Освобождаем create info ТОЛЬКО у созданного объекта (если мы его реально вставляли/использовали).
        if !created_create_info.is_null() && ((*created_header).flags & OB_FLAG_CREATE_INFO) != 0 {
            crate::ex::pool::ex_free_pool_with_tag(
                created_create_info as PVOID,
                u32::from_le_bytes(*b"IfbO"),
            );
            (*created_header)
                .object_create_info_or_quota
                .object_create_info = core::ptr::null_mut();
            (*created_header).flags &= !OB_FLAG_CREATE_INFO;
        }

        if !new_object.is_null() {
            *new_object = target_object;
        }

        // Если caller не запросил возврат object pointer (`new_object == NULL`),
        // освобождаем “создательскую” ссылку и оставляем объект жить за счёт handle.
        // Это приближает семантику к ObInsertObject в NT.
        if !handle.is_null() && new_object.is_null() {
            super::refcount::ob_dereference_object(target_object);
        }
    }

    final_status
}

/// Удаляет ранее вставленный объект из директории namespace (rollback путь).
unsafe fn obp_remove_namespace_entry(
    directory: *mut super::dir::OBJECT_DIRECTORY,
    header: *mut OBJECT_HEADER,
    attributes: u32,
) -> bool {
    unsafe {
        use super::dir::OBP_LOOKUP_CONTEXT;
        use super::dir::obp_delete_entry_directory;
        use super::dir::obp_lookup_entry_directory;
        use super::dir::obp_release_lookup_context;

        if directory.is_null() || header.is_null() {
            return false;
        }
        let Some(name_info) = (*header).name_info() else {
            return false;
        };

        let mut ctx = OBP_LOOKUP_CONTEXT::new();
        let found =
            obp_lookup_entry_directory(directory, &name_info.name, attributes, false, &mut ctx);
        if !found || ctx.entry.is_null() || (*ctx.entry).is_null() {
            obp_release_lookup_context(&mut ctx);
            return false;
        }
        let ok = obp_delete_entry_directory(&mut ctx);
        obp_release_lookup_context(&mut ctx);
        ok
    }
}

// =============================================================================
// Internal helpers
// =============================================================================

// NOTE: бывший `obp_insert_namespace_entry` удалён.
// Семантика коллизий (`OBJ_OPENIF` / STATUS_OBJECT_NAME_EXISTS) реализуется в `ob_insert_object`,
// потому что зависит от атрибутов `OBJECT_CREATE_INFORMATION` и должна уметь “переключиться”
// на уже существующий объект.

// =============================================================================
// ObMakeTemporaryObject
// =============================================================================

/// ObMakeTemporaryObject — убирает флаг OBJ_PERMANENT с объекта
///
/// После этого объект будет удалён когда последний handle/reference закроется.
/// Используется для удаления permanent объектов (например symbolic links).
///
/// # Arguments
/// * `object_handle` - handle объекта
///
/// # Returns
/// STATUS_SUCCESS или код ошибки
pub fn ob_make_temporary_object(object_handle: PVOID) -> NTSTATUS {
    if object_handle.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    unsafe {
        // Получаем объект по handle
        let mut object: PVOID = core::ptr::null_mut();
        let status = super::refcount::ob_reference_object_by_handle(
            object_handle,
            0,                     // Не требуем специфичных прав
            core::ptr::null_mut(), // Любой тип
            0,                     // KernelMode
            &mut object,
            core::ptr::null_mut(),
        );

        if status != STATUS_SUCCESS {
            return status;
        }

        // Убираем флаг permanent
        let header = object_to_object_header(object);
        if !header.is_null() {
            (*header).flags &= !OB_FLAG_PERMANENT;
        }

        // Освобождаем ссылку
        super::refcount::ob_dereference_object(object);

        STATUS_SUCCESS
    }
}
