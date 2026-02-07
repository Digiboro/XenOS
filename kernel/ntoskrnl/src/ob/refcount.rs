//! Object Reference Counting
//!
//! Управление счетчиками ссылок на объекты.
//!
//! Источники:
//! - ReactOS: ob/obref.c

use core::sync::atomic::Ordering;

use super::header::*;
use super::init::OBP_REAPER_LIST;
use super::types::*;
use crate::nt::NTSTATUS;
use crate::nt::PVOID;
use crate::nt::STATUS_ACCESS_DENIED;
use crate::nt::STATUS_INVALID_HANDLE;
use crate::nt::STATUS_OBJECT_TYPE_MISMATCH;
use crate::nt::STATUS_SUCCESS;

// =============================================================================
// ObReferenceObject / ObfReferenceObject
// =============================================================================

/// Увеличивает счетчик ссылок на объект
///
/// # Safety
/// Object должен быть валидным указателем на объект
#[inline]
pub unsafe fn ob_reference_object(object: PVOID) -> isize {
    unsafe { obf_reference_object(object) }
}

/// Fast-call версия ObReferenceObject
#[inline]
pub unsafe fn obf_reference_object(object: PVOID) -> isize {
    debug_assert!(!object.is_null());

    unsafe {
        let header = object_to_object_header(object);
        let new_count = (*header).pointer_count.fetch_add(1, Ordering::AcqRel) + 1;

        new_count
    }
}

/// Безопасный reference - возвращает false если счетчик уже 0
pub unsafe fn ob_reference_object_safe(object: PVOID) -> bool {
    if object.is_null() {
        return false;
    }

    unsafe {
        let header = object_to_object_header(object);

        loop {
            let old_value = (*header).pointer_count.load(Ordering::Acquire);
            if old_value == 0 {
                return false;
            }

            match (*header).pointer_count.compare_exchange(
                old_value,
                old_value + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return true,
                Err(_) => continue, // Retry
            }
        }
    }
}

/// Увеличивает счетчик на указанное значение
#[inline]
pub unsafe fn ob_reference_object_ex(object: PVOID, count: isize) -> isize {
    unsafe {
        let header = object_to_object_header(object);
        (*header).pointer_count.fetch_add(count, Ordering::AcqRel) + count
    }
}

// =============================================================================
// ObDereferenceObject / ObfDereferenceObject
// =============================================================================

/// Уменьшает счетчик ссылок на объект
///
/// Если счетчик достигает 0, объект будет удален.
///
/// # Safety
/// Object должен быть валидным указателем на объект
#[inline]
pub unsafe fn ob_dereference_object(object: PVOID) {
    unsafe {
        obf_dereference_object(object);
    }
}

/// Fast-call версия ObDereferenceObject
pub unsafe fn obf_dereference_object(object: PVOID) -> isize {
    debug_assert!(!object.is_null());

    unsafe {
        let header = object_to_object_header(object);

        // Проверка на некорректное состояние
        let pointer_count = (*header).pointer_count.load(Ordering::Acquire);
        let handle_count = (*header).handle_count();

        if pointer_count < handle_count {
            // Misbehaving object - не уменьшаем счетчик
            return pointer_count;
        }

        let new_count = (*header).pointer_count.fetch_sub(1, Ordering::AcqRel) - 1;

        if new_count == 0 {
            // Счетчик хэндлов тоже должен быть 0
            debug_assert!(handle_count == 0);

            // Удаляем объект
            obp_delete_object_deferred(header);
        }

        new_count
    }
}

/// Уменьшает счетчик на указанное значение
pub unsafe fn ob_dereference_object_ex(object: PVOID, count: isize) -> isize {
    unsafe {
        let header = object_to_object_header(object);
        let new_count = (*header).pointer_count.fetch_sub(count, Ordering::AcqRel) - count;

        if new_count == 0 {
            obp_delete_object_deferred(header);
        }

        new_count
    }
}

/// Dereference с отложенным удалением (всегда через work item)
pub unsafe fn ob_dereference_object_defer_delete(object: PVOID) {
    unsafe {
        let header = object_to_object_header(object);
        let new_count = (*header).pointer_count.fetch_sub(1, Ordering::AcqRel) - 1;

        if new_count == 0 {
            obp_defer_object_deletion(header);
        }
    }
}

// =============================================================================
// ObReferenceObjectByPointer
// =============================================================================

/// Ссылка на объект по указателю с проверкой типа
pub unsafe fn ob_reference_object_by_pointer(
    object: PVOID,
    _desired_access: u32,
    object_type: *mut OBJECT_TYPE,
    access_mode: u8, // KPROCESSOR_MODE
) -> NTSTATUS {
    if object.is_null() {
        return STATUS_INVALID_HANDLE;
    }

    unsafe {
        let header = object_to_object_header(object);

        // Проверяем тип, если указан (кроме kernel mode для symlinks)
        if !object_type.is_null() {
            if (*header).type_ptr != object_type {
                // В kernel mode для symbolic links разрешаем несовпадение типа
                let is_symbolic_link =
                    (*header).type_ptr == super::init::obp_get_symbolic_link_object_type();

                if access_mode != 0 || !is_symbolic_link {
                    // 0 = KernelMode
                    return STATUS_OBJECT_TYPE_MISMATCH;
                }
            }
        }

        // Увеличиваем счетчик ссылок
        (*header).pointer_count.fetch_add(1, Ordering::AcqRel);

        STATUS_SUCCESS
    }
}

// =============================================================================
// ObReferenceObjectByHandle
// =============================================================================

/// Информация о хэндле
#[repr(C)]
pub struct OBJECT_HANDLE_INFORMATION {
    pub handle_attributes: u32,
    pub granted_access: u32,
}

/// Ссылка на объект по хэндлу
///
/// # Arguments
/// * `handle` - Хэндл объекта
/// * `desired_access` - Запрашиваемые права доступа
/// * `object_type` - Ожидаемый тип объекта (или null для любого)
/// * `access_mode` - KernelMode (0) или UserMode (1)
/// * `object` - Выходной указатель на объект
/// * `handle_information` - Опциональная информация о хэндле
pub unsafe fn ob_reference_object_by_handle(
    handle: PVOID, // HANDLE
    desired_access: u32,
    object_type: *mut OBJECT_TYPE,
    access_mode: u8,
    object: *mut PVOID,
    handle_information: *mut OBJECT_HANDLE_INFORMATION,
) -> NTSTATUS {
    unsafe {
        if object.is_null() {
            return STATUS_INVALID_HANDLE;
        }

        unsafe {
            *object = core::ptr::null_mut();
        }

        // Специальные хэндлы (pseudo-handles)
        let handle_value = handle as isize;

        if handle_value < 0 {
            // NtCurrentProcess = -1, NtCurrentThread = -2
            const NT_CURRENT_PROCESS: isize = -1;
            const NT_CURRENT_THREAD: isize = -2;

            let obj = match handle_value {
                NT_CURRENT_PROCESS => {
                    // Возвращаем текущий процесс
                    let process = unsafe { crate::ps::ps_get_current_process() };
                    if process.is_null() {
                        return STATUS_INVALID_HANDLE;
                    }
                    process as PVOID
                },
                NT_CURRENT_THREAD => {
                    // Возвращаем текущий поток
                    let thread = unsafe { crate::ps::ps_get_current_thread() };
                    if thread.is_null() {
                        return STATUS_INVALID_HANDLE;
                    }
                    thread as PVOID
                },
                _ => return STATUS_INVALID_HANDLE,
            };

            unsafe {
                // Проверяем тип объекта если указан
                if !object_type.is_null() {
                    let header = object_to_object_header(obj);
                    if (*header).type_ptr != object_type {
                        return STATUS_OBJECT_TYPE_MISMATCH;
                    }
                }

                // Reference объект
                let header = object_to_object_header(obj);
                (*header).pointer_count.fetch_add(1, Ordering::AcqRel);

                // Возвращаем объект
                *object = obj;

                // Заполняем информацию о хэндле
                if !handle_information.is_null() {
                    (*handle_information).handle_attributes = 0;
                    (*handle_information).granted_access = 0xFFFFFFFF; // Full access для pseudo-handles
                }
            }

            return STATUS_SUCCESS;
        }

        // Обычные хэндлы: HANDLE_TABLE (kernel/process). Pointer-handle больше не является нормальным путём (Этап 5).
        if handle.is_null() {
            return STATUS_INVALID_HANDLE;
        }

        // Для корректной семантики `OBJECT_HANDLE_INFORMATION` нам нужны и attributes handle.
        const OBP_HANDLE_FLAG_KERNEL: usize = 0x1;
        let is_kernel_handle = ((handle as usize) & OBP_HANDLE_FLAG_KERNEL) != 0;

        let table = if is_kernel_handle {
            super::init::OBP_KERNEL_HANDLE_TABLE.load(Ordering::Acquire)
        } else {
            let process = crate::ps::ps_get_current_process();
            if process.is_null() {
                return STATUS_INVALID_HANDLE;
            }
            (*process).object_table.load(Ordering::Acquire)
        };
        if table.is_null() {
            return STATUS_INVALID_HANDLE;
        }

        let Some((obj, granted_access, handle_attrs)) =
            crate::ex::handle::ex_map_handle_to_pointer_full(table, handle as usize)
        else {
            return STATUS_INVALID_HANDLE;
        };

        unsafe {
            let header = object_to_object_header(obj);

            // Проверяем тип
            if !object_type.is_null() && (*header).type_ptr != object_type {
                return STATUS_OBJECT_TYPE_MISMATCH;
            }

            // Проверяем доступ (упрощенно)
            if access_mode != 0 {
                // UserMode
                if (granted_access & desired_access) != desired_access {
                    return STATUS_ACCESS_DENIED;
                }
            }

            // Reference объект
            (*header).pointer_count.fetch_add(1, Ordering::AcqRel);

            // Заполняем информацию о хэндле
            if !handle_information.is_null() {
                (*handle_information).handle_attributes = handle_attrs;
                (*handle_information).granted_access = granted_access;
            }

            *object = obj;
            STATUS_SUCCESS
        }
    }
}

// =============================================================================
// Handle table helpers
// =============================================================================

/// Пытается получить объект по handle из таблицы текущего процесса.
///
/// Возвращает `None`, если handle не найден или таблица недоступна.
pub unsafe fn obp_map_handle_to_object(handle: PVOID) -> Option<PVOID> {
    unsafe { obp_map_handle_to_object_ex(handle).map(|(o, _)| o) }
}

/// Расширенный вариант: возвращает объект и granted_access из HANDLE_TABLE.
pub unsafe fn obp_map_handle_to_object_ex(handle: PVOID) -> Option<(PVOID, u32)> {
    unsafe {
        const OBP_HANDLE_FLAG_KERNEL: usize = 0x1;
        let is_kernel_handle = ((handle as usize) & OBP_HANDLE_FLAG_KERNEL) != 0;

        let table = if is_kernel_handle {
            super::init::OBP_KERNEL_HANDLE_TABLE.load(Ordering::Acquire)
        } else {
            let process = crate::ps::ps_get_current_process();
            if process.is_null() {
                return None;
            }

            // Получаем HANDLE_TABLE процесса
            (*process).object_table.load(Ordering::Acquire)
        };
        if table.is_null() {
            return None;
        }

        let (obj, access) =
            match crate::ex::handle::ex_map_handle_to_pointer_ex(table, handle as usize) {
                Some(v) => v,
                None => return None,
            };

        if obj.is_null() {
            None
        } else {
            Some((obj, access))
        }
    }
}

/// Удаляет handle из таблицы текущего процесса (если она есть).
pub unsafe fn obp_destroy_handle(handle: PVOID) -> bool {
    unsafe {
        const OBP_HANDLE_FLAG_KERNEL: usize = 0x1;
        let is_kernel_handle = ((handle as usize) & OBP_HANDLE_FLAG_KERNEL) != 0;

        if is_kernel_handle {
            let table = super::init::OBP_KERNEL_HANDLE_TABLE.load(Ordering::Acquire);
            if table.is_null() {
                return false;
            }
            return crate::ex::handle::ex_destroy_handle(table, handle as usize);
        }

        let process = crate::ps::ps_get_current_process();
        if process.is_null() {
            return false;
        }

        let table = (*process).object_table.load(Ordering::Acquire);
        if table.is_null() {
            return false;
        }

        crate::ex::handle::ex_destroy_handle(table, handle as usize)
    }
}

// =============================================================================
// Internal Functions
// =============================================================================

/// Добавляет объект в список для отложенного удаления
unsafe fn obp_defer_object_deletion(header: *mut OBJECT_HEADER) {
    unsafe {
        loop {
            let entry = OBP_REAPER_LIST.load(Ordering::Acquire);
            (*header).handle_count_or_next_to_free.next_to_free = entry;

            match OBP_REAPER_LIST.compare_exchange(
                entry,
                header,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    // Если список был пуст, ставим work item в очередь
                    if entry.is_null() {
                        // Ставим reaper work item в очередь.
                        //
                        // Важно: один и тот же WORK_QUEUE_ITEM нельзя ставить в очередь повторно,
                        // поэтому делаем это только на переходе списка из пустого состояния.
                        let work_item = &mut *super::init::OBP_REAPER_WORK_ITEM.get();
                        crate::ex::ex_queue_work_item(
                            work_item,
                            crate::ex::WORK_QUEUE_TYPE::DelayedWorkQueue,
                        );
                    }
                    break;
                },
                Err(_) => continue,
            }
        }
    }
}

/// Удаляет объект (немедленно или отложенно)
unsafe fn obp_delete_object_deferred(header: *mut OBJECT_HEADER) {
    unsafe {
        // Проверяем, можно ли удалять немедленно
        // TODO: проверка KeAreAllApcsDisabled()

        // Пока всегда отложенное удаление
        obp_defer_object_deletion(header);
    }
}

/// Удаляет объект (вызывается из reaper work item)
pub unsafe fn obp_delete_object(object: PVOID, _called_from_worker: bool) {
    unsafe {
        let header = object_to_object_header(object);
        let object_type = (*header).type_ptr;

        // Вызываем delete procedure если есть
        if !object_type.is_null() {
            if let Some(delete_proc) = (*object_type).type_info.delete_procedure {
                delete_proc(object);
            }
        }

        // Освобождаем security descriptor
        if !(*header).security_descriptor.is_null() {
            crate::se::sd::obp_release_security_descriptor((*header).security_descriptor);
            (*header).security_descriptor = core::ptr::null_mut();
        }

        // Освобождаем OBJECT_CREATE_INFORMATION, если он ещё присутствует
        if ((*header).flags & super::types::OB_FLAG_CREATE_INFO) != 0 {
            let ci = (*header).object_create_info_or_quota.object_create_info;
            if !ci.is_null() {
                crate::ex::pool::ex_free_pool_with_tag(ci as PVOID, u32::from_le_bytes(*b"IfbO"));
                (*header).object_create_info_or_quota.object_create_info = core::ptr::null_mut();
            }
            (*header).flags &= !super::types::OB_FLAG_CREATE_INFO;
        }

        // Освобождаем имя
        if let Some(name_info) = (*header).name_info_mut() {
            if !name_info.name.buffer.is_null() {
                crate::ex::pool::ex_free_pool(name_info.name.buffer as PVOID);
                name_info.name.buffer = core::ptr::null_mut();
            }
        }

        // Уменьшаем счетчик объектов типа
        if !object_type.is_null() {
            (*object_type).total_number_of_objects -= 1;
        }

        // Вычисляем начало аллокации
        let header_location = obp_find_header_location(header);

        // Освобождаем память
        let tag = if !object_type.is_null() {
            (*object_type).key
        } else {
            0
        };
        crate::ex::pool::ex_free_pool_with_tag(header_location, tag);
    }
}

/// Находит начало аллокации по header
unsafe fn obp_find_header_location(header: *mut OBJECT_HEADER) -> PVOID {
    unsafe {
        let mut location = header as *mut u8;

        // Creator info сразу перед header
        if ((*header).flags & OB_FLAG_CREATOR_INFO) != 0 {
            location = location.sub(core::mem::size_of::<OBJECT_HEADER_CREATOR_INFO>());
        }

        // Name info
        if (*header).name_info_offset != 0 {
            let name_start = (header as *mut u8).sub((*header).name_info_offset as usize);
            if (name_start as usize) < (location as usize) {
                location = name_start;
            }
        }

        // Handle info
        if (*header).handle_info_offset != 0 {
            let handle_start = (header as *mut u8).sub((*header).handle_info_offset as usize);
            if (handle_start as usize) < (location as usize) {
                location = handle_start;
            }
        }

        // Quota info
        if (*header).quota_info_offset != 0 {
            let quota_start = (header as *mut u8).sub((*header).quota_info_offset as usize);
            if (quota_start as usize) < (location as usize) {
                location = quota_start;
            }
        }

        location as PVOID
    }
}
