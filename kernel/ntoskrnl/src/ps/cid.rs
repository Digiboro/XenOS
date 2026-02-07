//! Client ID (CID) Management
//!
//! Управление Client ID для процессов и потоков.
//!
//! Источники:
//! - ReactOS: ps/psmgr.c (PspCidTable)

use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;

use super::init::PSP_CID_TABLE;
use super::process::EPROCESS;
use super::thread::ETHREAD;
use crate::ex::handle::ex_create_handle_at;
use crate::ex::handle::ex_destroy_handle;
use crate::ex::handle::ex_map_handle_to_pointer;
use crate::nt::NTSTATUS;
use crate::nt::PVOID;
use crate::nt::STATUS_INVALID_CID;
use crate::nt::STATUS_SUCCESS;

// =============================================================================
// CID Allocation
// =============================================================================

/// Следующий свободный Process ID
static NEXT_PROCESS_ID: AtomicUsize = AtomicUsize::new(8); // 0=Idle, 4=System
/// Следующий свободный Thread ID
static NEXT_THREAD_ID: AtomicUsize = AtomicUsize::new(8);

/// Выделяет новый Process ID
pub fn psp_allocate_process_id() -> usize {
    // Простая реализация: инкрементируем на 4 (как в NT)
    NEXT_PROCESS_ID.fetch_add(4, Ordering::SeqCst)
}

/// Выделяет новый Thread ID
pub fn psp_allocate_thread_id() -> usize {
    // Простая реализация: инкрементируем на 4
    NEXT_THREAD_ID.fetch_add(4, Ordering::SeqCst)
}

// =============================================================================
// CID Table Operations
// =============================================================================

/// Тип объекта в CID table
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CID_OBJECT_TYPE {
    Process = 1,
    Thread = 2,
}

/// Вставляет процесс в CID table
pub fn psp_create_process_cid(process: *mut EPROCESS) -> NTSTATUS {
    if process.is_null() {
        return STATUS_INVALID_CID;
    }

    let cid_table = PSP_CID_TABLE.load(Ordering::Acquire);
    if cid_table.is_null() {
        return STATUS_INVALID_CID;
    }

    unsafe {
        let process_id = (*process).process_id();

        // В NT PspCidTable мапит PID/TID -> object через HANDLE_TABLE.
        // Для сохранения семантики “CID == ID” используем фиксированное значение handle.
        // Важно: handle 0 зарезервирован (NULL).
        if process_id == 0 {
            return STATUS_INVALID_CID;
        }
        let ok = ex_create_handle_at(
            cid_table,
            process_id,
            process as PVOID,
            CID_OBJECT_TYPE::Process as u32,
            0,
        );
        if !ok {
            return STATUS_INVALID_CID;
        }

        STATUS_SUCCESS
    }
}

/// Вставляет поток в CID table
pub fn psp_create_thread_cid(thread: *mut ETHREAD) -> NTSTATUS {
    if thread.is_null() {
        return STATUS_INVALID_CID;
    }

    let cid_table = PSP_CID_TABLE.load(Ordering::Acquire);
    if cid_table.is_null() {
        return STATUS_INVALID_CID;
    }

    unsafe {
        // Аналогично процессам: handle == TID.
        let tid = (*thread).cid.thread_id();
        if tid == 0 {
            return STATUS_INVALID_CID;
        }
        let ok = ex_create_handle_at(
            cid_table,
            tid,
            thread as PVOID,
            CID_OBJECT_TYPE::Thread as u32,
            0,
        );
        if !ok {
            return STATUS_INVALID_CID;
        }

        STATUS_SUCCESS
    }
}

/// Удаляет процесс из CID table
pub fn psp_delete_process_cid(process_id: usize) {
    let cid_table = PSP_CID_TABLE.load(Ordering::Acquire);
    if cid_table.is_null() {
        return;
    }

    // Handle value = process ID (в упрощенной реализации)
    ex_destroy_handle(cid_table, process_id);
}

/// Удаляет поток из CID table
pub fn psp_delete_thread_cid(thread_id: usize) {
    let cid_table = PSP_CID_TABLE.load(Ordering::Acquire);
    if cid_table.is_null() {
        return;
    }

    ex_destroy_handle(cid_table, thread_id);
}

// =============================================================================
// CID Lookup
// =============================================================================

/// Ищет процесс по Process ID
pub fn psp_lookup_process_by_id(process_id: usize) -> *mut EPROCESS {
    // Специальные случаи
    if process_id == 0 {
        return super::process::ps_idle_process();
    }
    if process_id == 4 {
        return super::process::ps_get_initial_system_process();
    }

    let cid_table = PSP_CID_TABLE.load(Ordering::Acquire);
    if cid_table.is_null() {
        return core::ptr::null_mut();
    }

    // Используем ex_map_handle_to_pointer
    let object = ex_map_handle_to_pointer(cid_table, process_id);
    if object.is_null() {
        return core::ptr::null_mut();
    }

    // TODO: Проверить тип объекта
    object as *mut EPROCESS
}

/// Ищет поток по Thread ID
pub fn psp_lookup_thread_by_id(thread_id: usize) -> *mut ETHREAD {
    let cid_table = PSP_CID_TABLE.load(Ordering::Acquire);
    if cid_table.is_null() {
        return core::ptr::null_mut();
    }

    let object = ex_map_handle_to_pointer(cid_table, thread_id);
    if object.is_null() {
        return core::ptr::null_mut();
    }

    // TODO: Проверить тип объекта
    object as *mut ETHREAD
}

// =============================================================================
// Public API
// =============================================================================

/// PsLookupProcessByProcessId - ищет процесс и увеличивает reference count
pub fn ps_lookup_process_by_process_id(process_id: usize, process: *mut *mut EPROCESS) -> NTSTATUS {
    if process.is_null() {
        return STATUS_INVALID_CID;
    }

    let found = psp_lookup_process_by_id(process_id);
    if found.is_null() {
        return STATUS_INVALID_CID;
    }

    unsafe {
        // Увеличиваем reference count
        crate::ob::ob_reference_object(found as crate::nt::PVOID);
        *process = found;
    }

    STATUS_SUCCESS
}

/// PsLookupThreadByThreadId - ищет поток и увеличивает reference count
pub fn ps_lookup_thread_by_thread_id(thread_id: usize, thread: *mut *mut ETHREAD) -> NTSTATUS {
    if thread.is_null() {
        return STATUS_INVALID_CID;
    }

    let found = psp_lookup_thread_by_id(thread_id);
    if found.is_null() {
        return STATUS_INVALID_CID;
    }

    unsafe {
        // Увеличиваем reference count
        crate::ob::ob_reference_object(found as crate::nt::PVOID);
        *thread = found;
    }

    STATUS_SUCCESS
}
