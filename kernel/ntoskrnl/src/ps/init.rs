//! Process Manager Initialization
//!
//! Инициализация Process Manager.
//!
//! Источники:
//! - ReactOS: ps/psmgr.c

use core::sync::atomic::AtomicPtr;
use core::sync::atomic::AtomicU32;
use core::sync::atomic::Ordering;

use super::process::*;
use super::types::*;
use crate::ex::pool::POOL_TYPE;
use crate::ex::pool::ex_allocate_pool_with_tag;
use crate::nt::LIST_ENTRY;
use crate::nt::NTSTATUS;
use crate::nt::PVOID;
use crate::nt::STATUS_SUCCESS;
use crate::nt::UNICODE_STRING;
use crate::ob::OBJ_KERNEL_EXCLUSIVE;
use crate::ob::OBJECT_TYPE;
use crate::ob::OBJECT_TYPE_INITIALIZER;
use crate::ob::ob_create_object_type;

// =============================================================================
// Global Object Types
// =============================================================================

/// Process object type
pub static PS_PROCESS_TYPE: AtomicPtr<OBJECT_TYPE> = AtomicPtr::new(core::ptr::null_mut());

/// Thread object type
pub static PS_THREAD_TYPE: AtomicPtr<OBJECT_TYPE> = AtomicPtr::new(core::ptr::null_mut());

/// Job object type
pub static PS_JOB_TYPE: AtomicPtr<OBJECT_TYPE> = AtomicPtr::new(core::ptr::null_mut());

/// Initialization phase
pub static PSP_INITIALIZATION_PHASE: AtomicU32 = AtomicU32::new(0);

// =============================================================================
// CID Table
// =============================================================================

/// CID (Client ID) Handle Table
pub static PSP_CID_TABLE: AtomicPtr<crate::ex::handle::HANDLE_TABLE> =
    AtomicPtr::new(core::ptr::null_mut());

// =============================================================================
// PsInitSystem
// =============================================================================

/// Инициализирует Process Manager
///
/// Вызывается в Phase 0 и Phase 1
pub fn ps_init_system(phase: u32) -> bool {
    match phase {
        0 => psp_init_phase0(),
        1 => psp_init_phase1(),
        _ => false,
    }
}

/// Phase 0: Ранняя инициализация
///
/// PspInitPhase0
///
/// - Создает типы объектов Process и Thread
/// - Создает CID table
/// - Создает System Process
/// - Создает Phase 1 поток инициализации (NT way)
///
/// Источники:
/// - NT6.1: ps/psmgr.c (PspInitPhase0)
/// - ReactOS: ntoskrnl/ps/psmgr.c
fn psp_init_phase0() -> bool {
    // Создаем тип объекта Process
    if !create_process_object_type() {
        return false;
    }

    // Создаем тип объекта Thread
    if !create_thread_object_type() {
        return false;
    }

    // Инициализируем список активных процессов
    unsafe {
        let head = PS_ACTIVE_PROCESS_HEAD.get();
        LIST_ENTRY::init_head(head);
    }

    // Создаем CID table
    if !create_cid_table() {
        return false;
    }

    // Idle Process уже создан в ke::idle::ki_initialize_idle_process() до Executive init
    // Здесь только устанавливаем имя (если нужно)
    unsafe {
        let idle_process = PS_IDLE_PROCESS.load(Ordering::Acquire);
        if idle_process.is_null() {
            // Idle Process не был создан - ошибка порядка инициализации
            crate::kd::dbg_print("[PS] ERROR: Idle Process not initialized!\n");
            return false;
        }
        // Устанавливаем имя (NT pattern)
        (*idle_process).set_image_name(b"Idle");
    }

    // Создаем System Process
    if !create_system_process() {
        return false;
    }

    // =========================================================================
    // NT way: Создаем Phase 1 поток инициализации в конце PspInitPhase0
    // В NT это делается внутри ps_init_system(0), а не в main.rs
    // =========================================================================
    unsafe {
        let loader_block = crate::ke::globals::ke_get_loader_block();
        if loader_block.is_null() {
            crate::kd::dbg_print("[PS] ERROR: LoaderBlock is NULL, cannot create Phase1 thread\n");
            return false;
        }

        // Создаем Phase1 поток напрямую с phase1_initialization (без обёртки)
        let status = crate::ps::ps_create_system_thread(
            core::ptr::null_mut(),                  // thread_handle (не нужен)
            0,                                      // desired_access
            core::ptr::null_mut(),                  // object_attributes
            0,                                      // process_handle (System Process)
            core::ptr::null_mut(),                  // client_id (не нужен)
            Some(crate::ex::phase1_initialization), // start_routine - напрямую!
            loader_block as PVOID,                  // start_context = LoaderBlock
        );

        if status != STATUS_SUCCESS {
            crate::kd::dbg_print("[PS] ERROR: Failed to create Phase1 thread\n");
            return false;
        }
    }

    PSP_INITIALIZATION_PHASE.store(1, Ordering::Release);
    true
}

/// Phase 1: Основная инициализация
///
/// - Финализация System Process
/// - Назначение System token
/// - Вставка System Process в CID table (отложено из Phase 0 из-за IRQL)
fn psp_init_phase1() -> bool {
    // Дополнительная инициализация System Process
    unsafe {
        let system_process = PS_INITIAL_SYSTEM_PROCESS.load(Ordering::Acquire);
        if !system_process.is_null() {
            // Устанавливаем имя
            (*system_process).set_image_name(b"System");

            // Назначаем System token для System Process
            // SE должна быть инициализирована к этому моменту
            crate::se::se_assign_system_token_to_system_process(system_process);

            // Вставляем System Process в CID table
            // В Phase 0 это было невозможно из-за высокого IRQL
            let cid_status = super::cid::psp_create_process_cid(system_process);
            if cid_status != STATUS_SUCCESS {
                crate::kd_print!(
                    "[PS] Warning: failed to insert System Process into CID table: 0x{:08X}\n",
                    cid_status
                );
                // Продолжаем - System Process (PID 4) обрабатывается как special case в lookup
            }
        }
    }

    PSP_INITIALIZATION_PHASE.store(2, Ordering::Release);
    true
}

// =============================================================================
// Object Type Creation
// =============================================================================

/// Статический буфер для имени "Process"
static mut PROCESS_TYPE_NAME_BUFFER: [u16; 8] = [
    b'P' as u16,
    b'r' as u16,
    b'o' as u16,
    b'c' as u16,
    b'e' as u16,
    b's' as u16,
    b's' as u16,
    0,
];

/// Статический буфер для имени "Thread"
static mut THREAD_TYPE_NAME_BUFFER: [u16; 7] = [
    b'T' as u16,
    b'h' as u16,
    b'r' as u16,
    b'e' as u16,
    b'a' as u16,
    b'd' as u16,
    0,
];

/// Создает тип объекта Process
fn create_process_object_type() -> bool {
    // Создаем UNICODE_STRING вручную
    // Safety: PROCESS_TYPE_NAME_BUFFER - статический мутабельный буфер
    let mut type_name = UNICODE_STRING {
        length: 14, // 7 chars * 2 bytes
        maximum_length: 16,
        buffer: core::ptr::addr_of_mut!(PROCESS_TYPE_NAME_BUFFER) as *mut u16,
    };

    let mut type_info = OBJECT_TYPE_INITIALIZER::new();
    type_info.object_type_flags = OBJ_KERNEL_EXCLUSIVE as u16;
    type_info.generic_mapping = PSP_PROCESS_MAPPING;
    type_info.valid_access_mask = PROCESS_ALL_ACCESS;
    type_info.pool_type = POOL_TYPE::NonPagedPool as u32;
    type_info.default_non_paged_pool_charge = core::mem::size_of::<EPROCESS>() as u32;
    type_info.close_procedure = Some(psp_process_close);
    type_info.delete_procedure = Some(psp_process_delete);

    let mut process_type: *mut OBJECT_TYPE = core::ptr::null_mut();
    let status = ob_create_object_type(
        &raw mut type_name,
        &type_info,
        core::ptr::null_mut(),
        &raw mut process_type,
    );

    if status == STATUS_SUCCESS && !process_type.is_null() {
        PS_PROCESS_TYPE.store(process_type, Ordering::Release);
        true
    } else {
        false
    }
}

/// Создает тип объекта Thread
fn create_thread_object_type() -> bool {
    // Safety: THREAD_TYPE_NAME_BUFFER - статический мутабельный буфер
    let mut type_name = UNICODE_STRING {
        length: 12, // 6 chars * 2 bytes
        maximum_length: 14,
        buffer: core::ptr::addr_of_mut!(THREAD_TYPE_NAME_BUFFER) as *mut u16,
    };

    let mut type_info = OBJECT_TYPE_INITIALIZER::new();
    type_info.object_type_flags = OBJ_KERNEL_EXCLUSIVE as u16;
    type_info.generic_mapping = PSP_THREAD_MAPPING;
    type_info.valid_access_mask = THREAD_ALL_ACCESS;
    type_info.pool_type = POOL_TYPE::NonPagedPool as u32;
    type_info.default_non_paged_pool_charge = core::mem::size_of::<super::thread::ETHREAD>() as u32;
    type_info.close_procedure = Some(psp_thread_close);
    type_info.delete_procedure = Some(psp_thread_delete);

    let mut thread_type: *mut OBJECT_TYPE = core::ptr::null_mut();
    let status = ob_create_object_type(
        &raw mut type_name,
        &type_info,
        core::ptr::null_mut(),
        &raw mut thread_type,
    );

    if status == STATUS_SUCCESS && !thread_type.is_null() {
        PS_THREAD_TYPE.store(thread_type, Ordering::Release);
        true
    } else {
        false
    }
}

// =============================================================================
// CID Table
// =============================================================================

/// Создает CID Handle Table
fn create_cid_table() -> bool {
    match crate::ex::handle::ex_create_handle_table(core::ptr::null_mut()) {
        Some(table) => {
            PSP_CID_TABLE.store(table, Ordering::Release);
            true
        },
        None => false,
    }
}

// =============================================================================
// Process Creation
// =============================================================================

// create_idle_process() удалён - Idle Process создаётся в ke::idle::ki_initialize_idle_process()

/// Создает System Process
fn create_system_process() -> bool {
    // Выделяем память для System Process
    let process = ex_allocate_pool_with_tag(
        POOL_TYPE::NonPagedPool,
        core::mem::size_of::<EPROCESS>(),
        u32::from_le_bytes(*b"Proc"),
    ) as *mut EPROCESS;

    if process.is_null() {
        return false;
    }

    unsafe {
        // Инициализируем нулями
        core::ptr::write_bytes(process, 0, 1);

        // Базовая инициализация
        *process = EPROCESS::new();

        // System process имеет PID 4
        (*process).set_process_id(4);
        (*process).set_image_name(b"System");
        (*process).pcb.base_priority = 8;
        (*process).priority_class = PROCESS_PRIORITY_CLASS_NORMAL;

        // Инициализируем списки
        LIST_ENTRY::init_head(&raw mut (*process).pcb.thread_list_head);
        LIST_ENTRY::init_head(&raw mut (*process).thread_list_head);
        LIST_ENTRY::init_head(&raw mut (*process).active_process_links);

        // Добавляем в список активных процессов
        let head = PS_ACTIVE_PROCESS_HEAD.get();
        // Insert at head
        (*process).active_process_links.flink = (*head).flink;
        (*process).active_process_links.blink = head as *mut LIST_ENTRY;
        if !(*head).flink.is_null() {
            (*(*head).flink).blink = &raw mut (*process).active_process_links;
        }
        (*head).flink = &raw mut (*process).active_process_links;
        if (*head).blink.is_null() || (*head).blink == head as *mut LIST_ENTRY {
            (*head).blink = &raw mut (*process).active_process_links;
        }

        // Создаем handle table для процесса (путь с поддержкой наследования, пока без родителя).
        if let Some(handle_table) = crate::ex::handle::ex_create_handle_table(process as PVOID) {
            (*process)
                .object_table
                .store(handle_table, Ordering::Release);
        }
    }

    PS_INITIAL_SYSTEM_PROCESS.store(process, Ordering::Release);
    true
}

/// PspInheritHandles
///
/// Наследование handles по `HANDLE_FLAG_INHERIT` (как в NT).
/// На этом этапе:
/// - переносим только inheritable handles из таблицы родителя,
/// - сохраняем значения хэндлов (создаем в child table тем же index),
/// - копируем granted_access и handle_attributes (HANDLE_FLAG_*),
/// - вызываем open_procedure с OB_OPEN_REASON_INHERIT.
pub unsafe fn psp_inherit_handles(
    child_process: *mut EPROCESS,
    parent_process: *mut EPROCESS,
) -> NTSTATUS {
    unsafe {
        use crate::ex::handle::HANDLE_TABLE;
        use crate::nt::STATUS_INSUFFICIENT_RESOURCES;
        use crate::nt::STATUS_INVALID_PARAMETER;
        use crate::nt::STATUS_SUCCESS;
        use crate::nt::ULONG;

        if child_process.is_null() || parent_process.is_null() {
            return STATUS_INVALID_PARAMETER;
        }

        let parent_table = (*parent_process).object_table.load(Ordering::Acquire);
        let child_table = (*child_process).object_table.load(Ordering::Acquire);
        if parent_table.is_null() || child_table.is_null() {
            return STATUS_INSUFFICIENT_RESOURCES;
        }

        // Проходим по таблице родителя и дублируем только inheritable handles.
        let size = (*parent_table).table_size.load(Ordering::Acquire);
        for index in 0..size {
            let handle = HANDLE_TABLE::index_to_handle(index);
            let Some((obj, access, attrs)) =
                crate::ex::handle::ex_map_handle_to_pointer_full(parent_table, handle)
            else {
                continue;
            };

            if (attrs & crate::ex::handle::HANDLE_FLAG_INHERIT) == 0 {
                continue;
            }

            // Создаем handle в child table с тем же значением.
            // Важно: handle удерживает ссылку на объект.
            crate::ob::refcount::ob_reference_object(obj);

            if !crate::ex::handle::ex_create_handle_at(child_table, handle, obj, access, attrs) {
                crate::ob::refcount::ob_dereference_object(obj);
                return STATUS_INSUFFICIENT_RESOURCES;
            }

            // Увеличиваем handle_count и counters типа + open callback.
            let header = crate::ob::header::object_to_object_header(obj);
            (*header).handle_count_or_next_to_free.handle_count += 1;

            let obj_type = (*header).type_ptr;
            if !obj_type.is_null() {
                (*obj_type).total_number_of_handles += 1;
                if (*obj_type).total_number_of_handles > (*obj_type).high_water_number_of_handles {
                    (*obj_type).high_water_number_of_handles = (*obj_type).total_number_of_handles;
                }

                if let Some(open_proc) = (*obj_type).type_info.open_procedure {
                    let st = open_proc(
                        crate::ob::types::OB_OPEN_REASON_INHERIT,
                        child_process as PVOID,
                        obj,
                        access,
                        (*header).handle_count() as ULONG,
                    );
                    if st != STATUS_SUCCESS {
                        // Откат: счетчики, handle, ссылка
                        (*header).handle_count_or_next_to_free.handle_count -= 1;
                        (*obj_type).total_number_of_handles -= 1;
                        let _ = crate::ex::handle::ex_destroy_handle(child_table, handle);
                        crate::ob::refcount::ob_dereference_object(obj);
                        return st;
                    }
                }
            }
        }

        STATUS_SUCCESS
    }
}

// =============================================================================
// Object Callbacks
// =============================================================================

/// Close procedure для Process
///
/// PspProcessClose (NT internal name)
/// Вызывается при закрытии handle на процесс.
/// Текущая реализация минимальна - расширить при добавлении handle tracking.
unsafe extern "win64" fn psp_process_close(
    _process: PVOID,
    _object: PVOID,
    _granted_access: u32,
    _process_handle_count: u32,
    _system_handle_count: u32,
) {
    // При закрытии handle на процесс особых действий не требуется
    // в текущей минимальной реализации.
    // TODO: При добавлении debug port, exception port и др. -
    // добавить соответствующий cleanup здесь.
}

/// Delete procedure для Process
///
/// PspProcessDelete (NT internal name)
/// Вызывается когда reference count процесса достигает нуля.
/// Освобождает все ресурсы процесса.
unsafe extern "win64" fn psp_process_delete(object: PVOID) {
    unsafe {
        if object.is_null() {
            return;
        }

        let process = object as *mut super::process::EPROCESS;

        // 1. Удаляем из CID table
        let pid = (*process).process_id();
        if pid != 0 {
            // Idle process (PID 0) не в CID table
            super::cid::psp_delete_process_cid(pid);
        }

        // 2. Удаляем из active process list
        // NOTE: К моменту delete поток уже должен быть удалён из списка в exit path,
        // но проверяем на всякий случай
        let links = &raw mut (*process).active_process_links;
        if !(*links).flink.is_null() && !(*links).blink.is_null() {
            // Безопасное удаление из двусвязного списка
            if (*links).flink != links as *mut _ && (*links).blink != links as *mut _ {
                (*(*links).blink).flink = (*links).flink;
                (*(*links).flink).blink = (*links).blink;
                // Обнуляем указатели для безопасности
                (*links).flink = core::ptr::null_mut();
                (*links).blink = core::ptr::null_mut();
            }
        }

        // 3. Освобождаем handle table
        let handle_table = (*process)
            .object_table
            .swap(core::ptr::null_mut(), Ordering::AcqRel);
        if !handle_table.is_null() {
            crate::ex::handle::ex_destroy_handle_table(handle_table);
        }

        // 4. Освобождаем token (когда будет SE подсистема)
        // let token = (*process).token.swap(core::ptr::null_mut(), Ordering::AcqRel);
        // if !token.is_null() { ObDereferenceObject(token); }

        // NOTE: Память самого EPROCESS будет освобождена Object Manager
        // после возврата из этой функции
    }
}

/// Close procedure для Thread
///
/// PspThreadClose (NT internal name)
/// Вызывается при закрытии handle на поток.
unsafe extern "win64" fn psp_thread_close(
    _process: PVOID,
    _object: PVOID,
    _granted_access: u32,
    _process_handle_count: u32,
    _system_handle_count: u32,
) {
    // При закрытии handle на поток особых действий не требуется
    // в текущей минимальной реализации.
}

/// Delete procedure для Thread
///
/// PspThreadDelete (NT internal name)
/// Вызывается когда reference count потока достигает нуля.
/// Освобождает все ресурсы потока.
unsafe extern "win64" fn psp_thread_delete(object: PVOID) {
    unsafe {
        if object.is_null() {
            return;
        }

        let thread = object as *mut super::thread::ETHREAD;

        // 1. Удаляем из CID table
        let tid = (*thread).cid.thread_id();
        if tid != 0 {
            super::cid::psp_delete_thread_cid(tid);
        }

        // 2. Удаляем из списка потоков процесса
        let kthread = &raw mut (*thread).tcb;
        let thread_list = &raw mut (*kthread).thread_list_entry;
        if !(*thread_list).flink.is_null() && !(*thread_list).blink.is_null() {
            if (*thread_list).flink != thread_list as *mut _
                && (*thread_list).blink != thread_list as *mut _
            {
                (*(*thread_list).blink).flink = (*thread_list).flink;
                (*(*thread_list).flink).blink = (*thread_list).blink;
                (*thread_list).flink = core::ptr::null_mut();
                (*thread_list).blink = core::ptr::null_mut();
            }
        }

        // 3. Освобождаем kernel stack
        let stack_limit = (*kthread).stack_limit;
        if !stack_limit.is_null() {
            crate::ex::pool::ex_free_pool_with_tag(stack_limit, u32::from_le_bytes(*b"Kstk"));
            // Обнуляем для безопасности
            (*kthread).stack_limit = core::ptr::null_mut();
            (*kthread).initial_stack = core::ptr::null_mut();
            (*kthread).kernel_stack = core::ptr::null_mut();
        }

        // 4. Освобождаем state_save_area (XSAVE)
        // NOTE: state_save_area находится на стеке, который уже освободили выше
        (*kthread).state_save_area = core::ptr::null_mut();

        // 5. Декремент active_threads в процессе
        let process = (*thread).thread_process.load(Ordering::Acquire);
        if !process.is_null() {
            (*process).active_threads.fetch_sub(1, Ordering::AcqRel);
        }

        // NOTE: Память самого ETHREAD будет освобождена Object Manager
    }
}

// =============================================================================
// Query Functions
// =============================================================================

/// Возвращает фазу инициализации
#[inline]
pub fn psp_get_initialization_phase() -> u32 {
    PSP_INITIALIZATION_PHASE.load(Ordering::Acquire)
}

/// Возвращает тип объекта Process
#[inline]
pub fn ps_process_type() -> *mut OBJECT_TYPE {
    PS_PROCESS_TYPE.load(Ordering::Acquire)
}

/// Возвращает тип объекта Thread
#[inline]
pub fn ps_thread_type() -> *mut OBJECT_TYPE {
    PS_THREAD_TYPE.load(Ordering::Acquire)
}
