//! Object Handle Management
//!
//! Управление хэндлами объектов.
//!
//! Источники:
//! - ReactOS: ob/obhandle.c

use super::dir::OBJECT_DIRECTORY;
use super::dir::OBP_LOOKUP_CONTEXT;
use super::dir::obp_lookup_entry_directory;
use super::dir::obp_release_lookup_context;
use super::header::*;
use super::refcount::*;
use super::types::*;
use crate::nt::NTSTATUS;
use crate::nt::PVOID;
use crate::nt::STATUS_INSUFFICIENT_RESOURCES;
use crate::nt::STATUS_INVALID_HANDLE;
use crate::nt::STATUS_SUCCESS;
use crate::nt::ULONG;
use crate::nt::UNICODE_STRING;
#[inline]
fn obp_objattrs_to_handle_flags(attributes: ULONG) -> u32 {
    let mut flags: u32 = 0;
    if (attributes & (OBJ_INHERIT as ULONG)) != 0 {
        flags |= crate::ex::handle::HANDLE_FLAG_INHERIT;
    }
    if (attributes & (OBJ_PROTECT_CLOSE as ULONG)) != 0 {
        flags |= crate::ex::handle::HANDLE_FLAG_PROTECT_FROM_CLOSE;
    }
    flags
}

#[inline]
fn obp_handleattrs_obj_to_handle_flags(handle_attributes: ULONG) -> u32 {
    // Публичные NT API принимают HandleAttributes как OBJ_* маску.
    // В HANDLE_TABLE_ENTRY храним HANDLE_FLAG_*.
    obp_objattrs_to_handle_flags(handle_attributes & (OBJ_HANDLE_ATTRIBUTES as ULONG))
}

/// NtDuplicateObject
///
/// Флаги `options` (Windows NT):
/// - `DUPLICATE_CLOSE_SOURCE` — закрыть исходный handle после успешного дублирования.
/// - `DUPLICATE_SAME_ACCESS` — игнорировать `desired_access`, использовать granted_access исходного handle.
/// - `DUPLICATE_SAME_ATTRIBUTES` — игнорировать `handle_attributes`, копировать атрибуты исходного handle.
///
/// Примечание: handle attributes хранятся в `HANDLE_TABLE_ENTRY.handle_attributes`.
pub const DUPLICATE_CLOSE_SOURCE: ULONG = 0x00000001;
pub const DUPLICATE_SAME_ACCESS: ULONG = 0x00000002;
pub const DUPLICATE_SAME_ATTRIBUTES: ULONG = 0x00000004;

// =============================================================================
// NtClose
// =============================================================================

/// NtClose
///
/// Закрывает хэндл объекта.
///
/// # Arguments
/// * `handle` - хэндл для закрытия
///
/// # Returns
/// * `STATUS_SUCCESS` - хэндл закрыт успешно
/// * `STATUS_INVALID_HANDLE` - невалидный хэндл
/// * `STATUS_HANDLE_NOT_CLOSABLE` - хэндл защищён от закрытия
#[unsafe(no_mangle)]
pub extern "win64" fn NtClose(handle: usize) -> NTSTATUS {
    nt_close(handle as PVOID)
}

/// Внутренняя функция закрытия хэндла
fn nt_close(handle: PVOID) -> NTSTATUS {
    if handle.is_null() {
        return STATUS_INVALID_HANDLE;
    }

    unsafe {
        // Попытка закрыть pseudo-handles запрещена
        if (handle as isize) < 0 {
            return STATUS_INVALID_HANDLE;
        }

        // Этап 5: закрываем только реальные хэндлы из HANDLE_TABLE.
        const OBP_HANDLE_FLAG_KERNEL: usize = 0x1;
        let is_kernel_handle = ((handle as usize) & OBP_HANDLE_FLAG_KERNEL) != 0;

        let table = if is_kernel_handle {
            super::init::OBP_KERNEL_HANDLE_TABLE.load(core::sync::atomic::Ordering::Acquire)
        } else {
            let process = crate::ps::ps_get_current_process();
            if process.is_null() {
                core::ptr::null_mut()
            } else {
                (*process)
                    .object_table
                    .load(core::sync::atomic::Ordering::Acquire)
            }
        };
        if table.is_null() {
            return STATUS_INVALID_HANDLE;
        }

        let Some((object, granted_access, handle_attrs)) =
            crate::ex::handle::ex_map_handle_to_pointer_full(table, handle as usize)
        else {
            return STATUS_INVALID_HANDLE;
        };

        // HANDLE_FLAG_PROTECT_FROM_CLOSE
        if (handle_attrs & crate::ex::handle::HANDLE_FLAG_PROTECT_FROM_CLOSE) != 0 {
            return crate::nt::STATUS_HANDLE_NOT_CLOSABLE;
        }

        let header = object_to_object_header(object);

        // Удаляем handle из таблицы
        if !crate::ex::handle::ex_destroy_handle(table, handle as usize) {
            return STATUS_INVALID_HANDLE;
        }

        // Уменьшаем handle count (пока хранится в header как integer)
        (*header).handle_count_or_next_to_free.handle_count -= 1;

        // Вызываем close procedure
        let object_type = (*header).type_ptr;
        if !object_type.is_null() {
            if let Some(close_proc) = (*object_type).type_info.close_procedure {
                close_proc(
                    core::ptr::null_mut(), // Process
                    object,
                    granted_access,
                    0, // ProcessHandleCount
                    0, // SystemHandleCount
                );
            }

            // Уменьшаем счетчик хэндлов типа
            (*object_type).total_number_of_handles -= 1;
        }

        // Dereference объект
        ob_dereference_object(object);
        STATUS_SUCCESS
    }
}

// =============================================================================
// ObOpenObjectByName
// =============================================================================

/// Открывает объект по имени
pub fn ob_open_object_by_name(
    object_attributes: *mut OBJECT_ATTRIBUTES,
    object_type: *mut OBJECT_TYPE,
    access_mode: u8,
    passed_access_state: PVOID,
    desired_access: u32,
    _parse_context: PVOID,
    handle: *mut PVOID,
) -> NTSTATUS {
    use crate::se::access_state::{ACCESS_STATE, AUX_ACCESS_DATA, se_create_access_state, se_delete_access_state};

    if object_attributes.is_null() || handle.is_null() {
        return STATUS_INVALID_HANDLE;
    }

    unsafe {
        *handle = core::ptr::null_mut();

        let attrs = &*object_attributes;

        // Проверяем имя
        if attrs.object_name.is_null() {
            return STATUS_INVALID_HANDLE;
        }

        // Lookup объекта в namespace (path-walk)
        let mut found_object: PVOID = core::ptr::null_mut();
        let status = obp_lookup_object_name(attrs, &mut found_object);
        if status != STATUS_SUCCESS {
            return status;
        }

        // Получаем generic mapping от типа
        let generic_mapping = if !object_type.is_null() {
            &(*object_type).type_info.generic_mapping as *const _
        } else {
            core::ptr::null()
        };

        // Используем переданный access_state или создаём локальный
        let mut local_access_state = ACCESS_STATE::new();
        let mut local_aux_data = AUX_ACCESS_DATA::new();
        let mut using_local = false;

        let access_state = if !passed_access_state.is_null() {
            passed_access_state
        } else {
            // Создаём локальный access state
            let st = se_create_access_state(
                &mut local_access_state,
                &mut local_aux_data,
                desired_access,
                generic_mapping,
            );
            if st != STATUS_SUCCESS {
                ob_dereference_object(found_object);
                return st;
            }
            using_local = true;
            &mut local_access_state as *mut ACCESS_STATE as PVOID
        };

        // Создание handle с access check
        let result = ob_open_object_by_pointer(
            found_object,
            attrs.attributes,
            access_state,
            desired_access,
            object_type,
            access_mode,
            handle,
        );

        // Освобождаем локальный access state если создавали
        if using_local {
            se_delete_access_state(&mut local_access_state);
        }

        result
    }
}

// =============================================================================
// ObOpenObjectByPointer
// =============================================================================

/// Открывает объект по указателю (создает хэндл)
pub fn ob_open_object_by_pointer(
    object: PVOID,
    attributes: ULONG,
    passed_access_state: PVOID,
    desired_access: u32,
    object_type: *mut OBJECT_TYPE,
    access_mode: u8,
    handle: *mut PVOID,
) -> NTSTATUS {
    if object.is_null() || handle.is_null() {
        return STATUS_INVALID_HANDLE;
    }

    unsafe {
        *handle = core::ptr::null_mut();

        let header = object_to_object_header(object);

        // Проверяем тип
        if !object_type.is_null() && (*header).type_ptr != object_type {
            return crate::nt::STATUS_OBJECT_TYPE_MISMATCH;
        }

        // === Access Check (Этап I) ===
        // Для UserMode и если есть SD - проверяем доступ
        let mut granted_access = desired_access;
        
        if access_mode != 0 {
            // UserMode - нужна проверка
            let obj_type = (*header).type_ptr;
            let sd = (*header).security_descriptor as *const crate::nt::SECURITY_DESCRIPTOR;
            
            // Если type требует security или есть SD - проверяем
            let security_required = if !obj_type.is_null() {
                (*obj_type).type_info.security_required
            } else {
                false
            };
            
            if !sd.is_null() || security_required {
                let status = obp_check_object_access(
                    object,
                    passed_access_state,
                    desired_access,
                    access_mode,
                    &mut granted_access,
                );
                
                if status != STATUS_SUCCESS {
                    return status;
                }
            }
        }

        // Таблица хэндлов: kernel или процессная
        const OBP_HANDLE_FLAG_KERNEL: usize = 0x1;
        let kernel_handle = (attributes & OBJ_KERNEL_HANDLE) != 0;
        let mut table = if kernel_handle {
            super::init::OBP_KERNEL_HANDLE_TABLE.load(core::sync::atomic::Ordering::Acquire)
        } else {
            let process = crate::ps::ps_get_current_process();
            if process.is_null() {
                core::ptr::null_mut()
            } else {
                (*process)
                    .object_table
                    .load(core::sync::atomic::Ordering::Acquire)
            }
        };
        // Если таблицы процесса нет, а мы в KernelMode — деградируем в kernel handle table,
        // чтобы не возвращать pointer-handle на ранних фазах.
        if table.is_null() && access_mode == 0 {
            table =
                super::init::OBP_KERNEL_HANDLE_TABLE.load(core::sync::atomic::Ordering::Acquire);
        }

        // Handle удерживает ссылку на объект
        ob_reference_object(object);

        // HANDLE_TABLE обязателен для “нормального” пути (Этап 5).
        if table.is_null() {
            ob_dereference_object(object);
            return STATUS_INSUFFICIENT_RESOURCES;
        }

        let mut created_table = table;
        let mut created_handle = crate::ex::handle::INVALID_HANDLE_VALUE;

        // Атрибуты handle: OBJ_* -> HANDLE_FLAG_*.
        let handle_flags = obp_objattrs_to_handle_flags(attributes);
        // Используем granted_access (после access check) а не desired_access
        let h = crate::ex::handle::ex_create_handle_ex(table, object, granted_access, handle_flags);
        if h == crate::ex::handle::INVALID_HANDLE_VALUE {
            created_table = core::ptr::null_mut();
            ob_dereference_object(object);
            return STATUS_INSUFFICIENT_RESOURCES;
        }

        created_handle = h;
        *handle = ((h | if kernel_handle {
            OBP_HANDLE_FLAG_KERNEL
        } else {
            0
        }) as usize) as PVOID;

        // Увеличиваем handle count
        (*header).handle_count_or_next_to_free.handle_count += 1;

        // Увеличиваем счетчик хэндлов типа
        let obj_type = (*header).type_ptr;
        if !obj_type.is_null() {
            (*obj_type).total_number_of_handles += 1;
            if (*obj_type).total_number_of_handles > (*obj_type).high_water_number_of_handles {
                (*obj_type).high_water_number_of_handles = (*obj_type).total_number_of_handles;
            }
        }

        // Вызываем open procedure
        let obj_type = (*header).type_ptr;
        if !obj_type.is_null() {
            if let Some(open_proc) = (*obj_type).type_info.open_procedure {
                let status = open_proc(
                    OB_OPEN_REASON_OPEN,   // OpenReason
                    core::ptr::null_mut(), // Process
                    object,
                    granted_access, // Используем granted_access
                    (*header).handle_count() as ULONG,
                );
                if status != STATUS_SUCCESS {
                    // Откатываем изменения
                    (*header).handle_count_or_next_to_free.handle_count -= 1;
                    (*obj_type).total_number_of_handles -= 1;

                    if !created_table.is_null()
                        && created_handle != crate::ex::handle::INVALID_HANDLE_VALUE
                    {
                        let _ = crate::ex::handle::ex_destroy_handle(created_table, created_handle);
                    }
                    ob_dereference_object(object);
                    *handle = core::ptr::null_mut();
                    return status;
                }
            }
        }

        STATUS_SUCCESS
    }
}

// =============================================================================
// Internal: path-walk
// =============================================================================

/// Минимальный ObpLookupObjectName.
///
/// Поддерживает:
/// - абсолютные пути вида `\\A\\B\\C`
/// - относительные пути относительно `OBJECT_ATTRIBUTES.root_directory` (если задан)
/// - разыменование `SymbolicLink` (с лимитом глубины), если не указан `OBJ_OPENLINK`
///
unsafe fn obp_lookup_object_name(attrs: &OBJECT_ATTRIBUTES, out_object: *mut PVOID) -> NTSTATUS {
    unsafe {
        use crate::ex::pool::ex_free_pool_with_tag;

        if out_object.is_null() || attrs.object_name.is_null() {
            return crate::nt::STATUS_INVALID_PARAMETER;
        }

        *out_object = core::ptr::null_mut();

        let full = &*attrs.object_name;
        if full.buffer.is_null() || full.length == 0 {
            return crate::nt::STATUS_OBJECT_NAME_INVALID;
        }

        // Выбираем стартовую директорию
        let mut start_dir: *mut OBJECT_DIRECTORY = core::ptr::null_mut();
        let mut i = 0usize;
        let mut referenced_root_dir_obj: PVOID = core::ptr::null_mut();

        let u16_len = (full.length / 2) as usize;
        let buf = core::slice::from_raw_parts(full.buffer, u16_len);

        // Абсолютный путь?
        if buf.get(0) == Some(&(b'\\' as u16)) {
            start_dir = super::init::obp_get_root_directory();
            // Пропускаем ведущие '\\'
            while i < u16_len && buf[i] == b'\\' as u16 {
                i += 1;
            }
        } else {
            // Относительный: root_directory если задан, иначе — корень.
            start_dir = if !attrs.root_directory.is_null() {
                // root_directory — HANDLE на директорию (в нашем ядре может быть и pointer-handle).
                let mut dir_obj: PVOID = core::ptr::null_mut();
                let st = super::refcount::ob_reference_object_by_handle(
                    attrs.root_directory,
                    DIRECTORY_TRAVERSE,
                    super::init::obp_get_directory_object_type(),
                    0,
                    &mut dir_obj,
                    core::ptr::null_mut(),
                );
                if st != STATUS_SUCCESS {
                    return st;
                }
                // После lookup мы не сохраняем ссылку — освобождаем перед возвратом.
                referenced_root_dir_obj = dir_obj;
                dir_obj as *mut OBJECT_DIRECTORY
            } else {
                super::init::obp_get_root_directory()
            };
        }

        if start_dir.is_null() {
            if !referenced_root_dir_obj.is_null() {
                super::refcount::ob_dereference_object(referenced_root_dir_obj);
            }
            return crate::nt::STATUS_OBJECT_PATH_NOT_FOUND;
        }

        // Разбор с возможностью перезапуска на симлинке
        let mut depth: u32 = 0;
        let mut owned_buf: *mut u16 = core::ptr::null_mut();
        let _owned_bytes: usize = 0;
        let owned_tag: u32 = u32::from_le_bytes(*b"pLbO"); // 'ObLp'

        // Текущий view на буфер
        let mut cur_buf_ptr = full.buffer;
        let mut cur_len_u16 = u16_len;
        let mut cur_index = i;
        let mut cur_start_dir = start_dir;

        loop {
            if depth > 8 {
                // Слишком много разыменований симлинков (защита от циклов)
                if !owned_buf.is_null() {
                    ex_free_pool_with_tag(owned_buf as PVOID, owned_tag);
                }
                if !referenced_root_dir_obj.is_null() {
                    super::refcount::ob_dereference_object(referenced_root_dir_obj);
                }
                return crate::nt::STATUS_OBJECT_NAME_INVALID;
            }

            let cur_slice = core::slice::from_raw_parts(cur_buf_ptr, cur_len_u16);
            let mut dir = cur_start_dir;

            // Если остаток пуст — это открытие директории (текущей)
            if cur_index >= cur_len_u16 {
                if !owned_buf.is_null() {
                    ex_free_pool_with_tag(owned_buf as PVOID, owned_tag);
                }
                if !referenced_root_dir_obj.is_null() {
                    super::refcount::ob_dereference_object(referenced_root_dir_obj);
                }
                *out_object = dir as PVOID;
                return STATUS_SUCCESS;
            }

            // Пропускаем повторные '\\'
            while cur_index < cur_len_u16 && cur_slice[cur_index] == b'\\' as u16 {
                cur_index += 1;
            }
            if cur_index >= cur_len_u16 {
                if !owned_buf.is_null() {
                    ex_free_pool_with_tag(owned_buf as PVOID, owned_tag);
                }
                if !referenced_root_dir_obj.is_null() {
                    super::refcount::ob_dereference_object(referenced_root_dir_obj);
                }
                *out_object = dir as PVOID;
                return STATUS_SUCCESS;
            }

            // Walk по сегментам
            let mut seg_start = cur_index;
            loop {
                // Ищем конец сегмента
                while cur_index < cur_len_u16 && cur_slice[cur_index] != b'\\' as u16 {
                    cur_index += 1;
                }
                let seg_len = cur_index - seg_start;
                if seg_len == 0 {
                    // Пустой сегмент
                    if !owned_buf.is_null() {
                        ex_free_pool_with_tag(owned_buf as PVOID, owned_tag);
                    }
                    if !referenced_root_dir_obj.is_null() {
                        super::refcount::ob_dereference_object(referenced_root_dir_obj);
                    }
                    return crate::nt::STATUS_OBJECT_NAME_INVALID;
                }

                let seg = UNICODE_STRING {
                    length: (seg_len * 2) as u16,
                    maximum_length: (seg_len * 2) as u16,
                    buffer: unsafe { cur_buf_ptr.add(seg_start) },
                };

                // Lookup в текущей директории
                let mut ctx = OBP_LOOKUP_CONTEXT::new();
                let found =
                    obp_lookup_entry_directory(dir, &seg, attrs.attributes, false, &mut ctx);
                if !found || ctx.object.is_null() {
                    obp_release_lookup_context(&mut ctx);
                    if !owned_buf.is_null() {
                        ex_free_pool_with_tag(owned_buf as PVOID, owned_tag);
                    }
                    if !referenced_root_dir_obj.is_null() {
                        super::refcount::ob_dereference_object(referenced_root_dir_obj);
                    }
                    // Если после сегмента нет ничего кроме разделителей — это "имя не найдено",
                    // иначе "путь не найден".
                    let mut t = cur_index;
                    while t < cur_len_u16 && cur_slice[t] == b'\\' as u16 {
                        t += 1;
                    }
                    return if t >= cur_len_u16 {
                        crate::nt::STATUS_OBJECT_NAME_NOT_FOUND
                    } else {
                        crate::nt::STATUS_OBJECT_PATH_NOT_FOUND
                    };
                }

                let obj = ctx.object;
                obp_release_lookup_context(&mut ctx);

                // Последний сегмент?
                let is_last = cur_index >= cur_len_u16;
                if is_last {
                    if !owned_buf.is_null() {
                        ex_free_pool_with_tag(owned_buf as PVOID, owned_tag);
                    }
                    if !referenced_root_dir_obj.is_null() {
                        super::refcount::ob_dereference_object(referenced_root_dir_obj);
                    }
                    *out_object = obj;
                    return STATUS_SUCCESS;
                }

                // Есть remaining — обрабатываем тип
                let obj_header = object_to_object_header(obj);
                let obj_type = (*obj_header).type_ptr;

                // Directory: продолжаем walk
                if obj_type == super::init::obp_get_directory_object_type() {
                    dir = obj as *mut OBJECT_DIRECTORY;
                    // Пропускаем '\\' и продолжаем
                    while cur_index < cur_len_u16 && cur_slice[cur_index] == b'\\' as u16 {
                        cur_index += 1;
                    }
                    if cur_index >= cur_len_u16 {
                        if !owned_buf.is_null() {
                            ex_free_pool_with_tag(owned_buf as PVOID, owned_tag);
                        }
                        if !referenced_root_dir_obj.is_null() {
                            super::refcount::ob_dereference_object(referenced_root_dir_obj);
                        }
                        *out_object = dir as PVOID;
                        return STATUS_SUCCESS;
                    }
                    seg_start = cur_index;
                    continue;
                }

                // Parse procedure (в т.ч. symbolic link)
                if !obj_type.is_null() {
                    if let Some(parse_proc) = (*obj_type).type_info.parse_procedure {
                        // Остаток пути = начиная с текущего cur_index (включая '\\' если он там)
                        let rem_start = cur_index;
                        let rem_len_u16 = cur_len_u16 - rem_start;

                        let mut complete_name = UNICODE_STRING {
                            length: (cur_len_u16 * 2) as u16,
                            maximum_length: (cur_len_u16 * 2) as u16,
                            buffer: cur_buf_ptr,
                        };
                        let mut remaining_name = UNICODE_STRING {
                            length: (rem_len_u16 * 2) as u16,
                            maximum_length: (rem_len_u16 * 2) as u16,
                            buffer: unsafe { cur_buf_ptr.add(rem_start) },
                        };

                        let mut reparse_start: PVOID = core::ptr::null_mut();
                        let st = parse_proc(
                            obj, // parse_object
                            obj_type,
                            core::ptr::null_mut(), // access_state
                            0,                     // KernelMode
                            attrs.attributes,
                            &mut complete_name,
                            &mut remaining_name,
                            dir as PVOID, // context: директория, где найден объект
                            core::ptr::null_mut(), // security_qos
                            &mut reparse_start,
                        );

                        if st == crate::nt::STATUS_REPARSE {
                            // Парсер вернул новый путь в remaining_name.buffer (выделенный в pool).
                            // Освобождаем предыдущий owned, если был.
                            if !owned_buf.is_null() {
                                ex_free_pool_with_tag(owned_buf as PVOID, owned_tag);
                            }
                            owned_buf = remaining_name.buffer;

                            cur_buf_ptr = remaining_name.buffer;
                            cur_len_u16 = (remaining_name.length / 2) as usize;
                            cur_index = 0;
                            cur_start_dir = reparse_start as *mut OBJECT_DIRECTORY;

                            // Если абсолютный — пропускаем ведущие '\\'
                            if !cur_buf_ptr.is_null()
                                && cur_len_u16 != 0
                                && unsafe { *cur_buf_ptr } == b'\\' as u16
                            {
                                cur_start_dir = super::init::obp_get_root_directory();
                                while cur_index < cur_len_u16
                                    && unsafe { *cur_buf_ptr.add(cur_index) } == b'\\' as u16
                                {
                                    cur_index += 1;
                                }
                            }

                            depth += 1;
                            break; // restart outer loop
                        }

                        if st == STATUS_SUCCESS {
                            // Парсер мог вернуть непосредственный объект (например OBJ_OPENLINK).
                            // В нашем пути remaining был непустой, поэтому считаем это ошибкой пути.
                            if !owned_buf.is_null() {
                                ex_free_pool_with_tag(owned_buf as PVOID, owned_tag);
                            }
                            if !referenced_root_dir_obj.is_null() {
                                super::refcount::ob_dereference_object(referenced_root_dir_obj);
                            }
                            return crate::nt::STATUS_OBJECT_PATH_NOT_FOUND;
                        }

                        // Ошибка парсинга
                        if !owned_buf.is_null() {
                            ex_free_pool_with_tag(owned_buf as PVOID, owned_tag);
                        }
                        if !referenced_root_dir_obj.is_null() {
                            super::refcount::ob_dereference_object(referenced_root_dir_obj);
                        }
                        return st;
                    }
                }

                // Не директория и не симлинк, а remaining есть
                if !owned_buf.is_null() {
                    ex_free_pool_with_tag(owned_buf as PVOID, owned_tag);
                }
                if !referenced_root_dir_obj.is_null() {
                    super::refcount::ob_dereference_object(referenced_root_dir_obj);
                }
                return crate::nt::STATUS_OBJECT_PATH_NOT_FOUND;
            }
        }
    }
}

// =============================================================================
// ObDuplicateObject
// =============================================================================

/// Дублирует хэндл объекта
pub fn ob_duplicate_object(
    source_process: PVOID,
    source_handle: PVOID,
    target_process: PVOID,
    target_handle: *mut PVOID,
    desired_access: u32,
    handle_attributes: ULONG,
    options: ULONG,
    _access_mode: u8,
) -> NTSTATUS {
    use core::sync::atomic::Ordering;

    if target_handle.is_null() {
        return crate::nt::STATUS_INVALID_PARAMETER;
    }

    unsafe {
        *target_handle = core::ptr::null_mut();
    }

    // Опции
    let same_access = (options & DUPLICATE_SAME_ACCESS) != 0;
    let close_source = (options & DUPLICATE_CLOSE_SOURCE) != 0;
    let _same_attrs = (options & DUPLICATE_SAME_ATTRIBUTES) != 0;
    let requested_handle_attributes = handle_attributes;

    // Определяем таблицу источника/цели
    unsafe fn get_table_for_process(process: PVOID) -> *mut crate::ex::handle::HANDLE_TABLE {
        unsafe {
            if process.is_null() {
                // NULL -> текущий процесс
                let p = crate::ps::ps_get_current_process();
                if p.is_null() {
                    return core::ptr::null_mut();
                }
                return (*p)
                    .object_table
                    .load(core::sync::atomic::Ordering::Acquire);
            }
            // Параметр приходит как PVOID, ожидаем `EPROCESS*`.
            let p = process as *mut crate::ps::process::EPROCESS;
            (*p).object_table
                .load(core::sync::atomic::Ordering::Acquire)
        }
    }

    // Kernel handle определяется нашим флагом в младшем бите.
    const OBP_HANDLE_FLAG_KERNEL: usize = 0x1;
    let source_is_kernel_handle = ((source_handle as usize) & OBP_HANDLE_FLAG_KERNEL) != 0;

    let src_table = if source_is_kernel_handle {
        super::init::OBP_KERNEL_HANDLE_TABLE.load(Ordering::Acquire)
    } else {
        unsafe { get_table_for_process(source_process) }
    };

    if src_table.is_null() {
        return STATUS_INVALID_HANDLE;
    }

    // Получаем объект и исходный granted_access.
    let (object, src_granted_access, src_handle_attributes) =
        match crate::ex::handle::ex_map_handle_to_pointer_full(src_table, source_handle as usize) {
            Some(v) => v,
            None => return STATUS_INVALID_HANDLE,
        };

    if object.is_null() {
        return STATUS_INVALID_HANDLE;
    }

    // Определяем granted_access и attributes нового handle.
    let new_granted_access = if same_access {
        src_granted_access
    } else {
        desired_access
    };
    let new_handle_attributes = if _same_attrs {
        src_handle_attributes
    } else {
        obp_handleattrs_obj_to_handle_flags(requested_handle_attributes)
    };

    // Таблица цели: если источник был kernel handle — сохраняем kernel семантику, иначе — таблица target процесса.
    let dst_table = if source_is_kernel_handle {
        super::init::OBP_KERNEL_HANDLE_TABLE.load(Ordering::Acquire)
    } else {
        unsafe { get_table_for_process(target_process) }
    };

    if dst_table.is_null() {
        return STATUS_INSUFFICIENT_RESOURCES;
    }

    unsafe {
        let header = object_to_object_header(object);

        // Новый handle удерживает ссылку на объект.
        ob_reference_object(object);

        let h = crate::ex::handle::ex_create_handle_ex(
            dst_table,
            object,
            new_granted_access,
            new_handle_attributes,
        );
        if h == crate::ex::handle::INVALID_HANDLE_VALUE {
            ob_dereference_object(object);
            return STATUS_INSUFFICIENT_RESOURCES;
        }

        // Возвращаем handle с флагом kernel, если нужно.
        let out_h = (h | if source_is_kernel_handle {
            OBP_HANDLE_FLAG_KERNEL
        } else {
            0
        }) as PVOID;
        *target_handle = out_h;

        // Увеличиваем handle_count и counters типа
        (*header).handle_count_or_next_to_free.handle_count += 1;
        let obj_type = (*header).type_ptr;
        if !obj_type.is_null() {
            (*obj_type).total_number_of_handles += 1;
            if (*obj_type).total_number_of_handles > (*obj_type).high_water_number_of_handles {
                (*obj_type).high_water_number_of_handles = (*obj_type).total_number_of_handles;
            }

            // Open procedure на новый handle (аналогично ObOpenObjectByPointer).
            if let Some(open_proc) = (*obj_type).type_info.open_procedure {
                let st = open_proc(
                    OB_OPEN_REASON_DUPLICATE,
                    target_process,
                    object,
                    new_granted_access,
                    (*header).handle_count() as ULONG,
                );
                if st != STATUS_SUCCESS {
                    // Rollback: уничтожаем созданный handle + счетчики + reference
                    (*header).handle_count_or_next_to_free.handle_count -= 1;
                    (*obj_type).total_number_of_handles -= 1;
                    let _ = crate::ex::handle::ex_destroy_handle(dst_table, h);
                    ob_dereference_object(object);
                    *target_handle = core::ptr::null_mut();
                    return st;
                }
            }
        }

        // DUPLICATE_CLOSE_SOURCE: закрываем исходный handle в его таблице (с корректным decrement).
        if close_source {
            let src_header = header;
            let src_obj_type = (*src_header).type_ptr;

            // Удаляем handle entry
            if !crate::ex::handle::ex_destroy_handle(src_table, source_handle as usize) {
                // Если не получилось — это не должно ломать созданный target handle.
                // Возвращаем успех, как практический компромисс (семантика будет уточняться).
                return STATUS_SUCCESS;
            }

            // handle_count и counters
            (*src_header).handle_count_or_next_to_free.handle_count -= 1;
            if !src_obj_type.is_null() {
                if let Some(close_proc) = (*src_obj_type).type_info.close_procedure {
                    close_proc(source_process, object, src_granted_access, 0, 0);
                }
                (*src_obj_type).total_number_of_handles -= 1;
            }

            // Снимаем ссылку, которую удерживал исходный handle
            ob_dereference_object(object);
        }
    }

    STATUS_SUCCESS
}

/// NtDuplicateObject
///
/// Минимальная обертка над `ObDuplicateObject`.
pub fn nt_duplicate_object(
    source_process_handle: PVOID,
    source_handle: PVOID,
    target_process_handle: PVOID,
    target_handle: *mut PVOID,
    desired_access: u32,
    handle_attributes: ULONG,
    options: ULONG,
) -> NTSTATUS {
    // Пока SE не реализован, упрощаем: получаем EPROCESS* через ObReferenceObjectByHandle
    // и передаем в ObDuplicateObject.
    unsafe {
        let mut src_proc_obj: PVOID = core::ptr::null_mut();
        let mut dst_proc_obj: PVOID = core::ptr::null_mut();

        // Тип процесса: из PS (создается в phase0).
        let process_type =
            crate::ps::init::PS_PROCESS_TYPE.load(core::sync::atomic::Ordering::Acquire);
        if process_type.is_null() {
            return STATUS_INSUFFICIENT_RESOURCES;
        }

        // Source process
        let st = super::refcount::ob_reference_object_by_handle(
            source_process_handle,
            crate::ps::types::PROCESS_DUP_HANDLE,
            process_type,
            0, // KernelMode
            &mut src_proc_obj,
            core::ptr::null_mut(),
        );
        if st != STATUS_SUCCESS {
            return st;
        }

        // Target process
        let st = super::refcount::ob_reference_object_by_handle(
            target_process_handle,
            crate::ps::types::PROCESS_DUP_HANDLE,
            process_type,
            0, // KernelMode
            &mut dst_proc_obj,
            core::ptr::null_mut(),
        );
        if st != STATUS_SUCCESS {
            super::refcount::ob_dereference_object(src_proc_obj);
            return st;
        }

        let st = ob_duplicate_object(
            src_proc_obj,
            source_handle,
            dst_proc_obj,
            target_handle,
            desired_access,
            handle_attributes,
            options,
            0, // KernelMode
        );

        super::refcount::ob_dereference_object(dst_proc_obj);
        super::refcount::ob_dereference_object(src_proc_obj);
        st
    }
}

// =============================================================================
// Internal: Access Check
// =============================================================================

/// ObpCheckObjectAccess - внутренняя функция проверки доступа к объекту
///
/// Вызывает SeAccessCheck для проверки прав доступа.
///
/// # Источник
/// ReactOS: ob/obhandle.c ObpCheckObjectAccess
unsafe fn obp_check_object_access(
    object: PVOID,
    passed_access_state: PVOID,
    desired_access: u32,
    access_mode: u8,
    granted_access: *mut u32,
) -> NTSTATUS {
    use crate::nt::{
        STATUS_ACCESS_DENIED, PRIVILEGE_SET,
        MAXIMUM_ALLOWED,
    };
    use crate::se::{
        access_state::{ACCESS_STATE, AUX_ACCESS_DATA, se_create_access_state, se_delete_access_state},
        accesschk::se_access_check,
    };

    let header = object_to_object_header(object);
    let sd = (*header).security_descriptor as *const crate::nt::SECURITY_DESCRIPTOR;
    let obj_type = (*header).type_ptr;

    // Если нет SD и тип не требует security - разрешаем всё
    if sd.is_null() {
        if obj_type.is_null() || !(*obj_type).type_info.security_required {
            *granted_access = desired_access;
            return STATUS_SUCCESS;
        }
        // Тип требует security но SD нет - запрещаем
        return STATUS_ACCESS_DENIED;
    }

    // Получаем generic mapping от типа
    let default_mapping = super::types::GENERIC_MAPPING::new();
    let generic_mapping: &super::types::GENERIC_MAPPING = if !obj_type.is_null() {
        &(*obj_type).type_info.generic_mapping
    } else {
        &default_mapping
    };

    // Используем переданный ACCESS_STATE или создаём временный
    let mut local_access_state = ACCESS_STATE::new();
    let mut local_aux_data = AUX_ACCESS_DATA::new();
    let mut using_local_state = false;

    let access_state = if !passed_access_state.is_null() {
        passed_access_state as *mut ACCESS_STATE
    } else {
        // Создаём локальный access state
        let status = se_create_access_state(
            &mut local_access_state,
            &mut local_aux_data,
            desired_access,
            generic_mapping,
        );
        if status != STATUS_SUCCESS {
            return status;
        }
        using_local_state = true;
        &mut local_access_state as *mut ACCESS_STATE
    };

    // Выполняем access check
    let mut privileges: *mut PRIVILEGE_SET = core::ptr::null_mut();
    let mut access_status: NTSTATUS = STATUS_ACCESS_DENIED;
    let mut local_granted: u32 = 0;

    let result = se_access_check(
        sd,
        &mut (*access_state).subject_security_context,
        0, // not locked
        desired_access,
        (*access_state).previously_granted_access,
        &mut privileges,
        generic_mapping,
        access_mode as i32,
        &mut local_granted,
        &mut access_status,
    );

    // Обновляем access state
    if result {
        (*access_state).previously_granted_access = local_granted;
        (*access_state).remaining_desired_access &= !local_granted;
        *granted_access = local_granted;
    }

    // Освобождаем локальный access state если создавали
    if using_local_state {
        se_delete_access_state(&mut local_access_state);
    }

    if result {
        STATUS_SUCCESS
    } else {
        access_status
    }
}
