//! Process/Thread Query Functions
//!
//! Функции запроса информации о процессах и потоках.
//!
//! Источники:
//! - ReactOS: ps/query.c

use super::process::*;
use super::types::*;
use crate::nt::NTSTATUS;
use crate::nt::PVOID;
use crate::nt::STATUS_INFO_LENGTH_MISMATCH;
use crate::nt::STATUS_INVALID_PARAMETER;
use crate::nt::STATUS_SUCCESS;

// =============================================================================
// NtQueryInformationProcess
// =============================================================================

/// Запрашивает информацию о процессе
pub fn nt_query_information_process(
    process_handle: PVOID,
    process_information_class: PROCESSINFOCLASS,
    process_information: PVOID,
    process_information_length: u32,
    return_length: *mut u32,
) -> NTSTATUS {
    if process_information.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    // Получаем процесс из handle
    let process = get_process_from_handle(process_handle);
    if process.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    match process_information_class {
        PROCESSINFOCLASS::ProcessBasicInformation => {
            if process_information_length < core::mem::size_of::<PROCESS_BASIC_INFORMATION>() as u32
            {
                return STATUS_INFO_LENGTH_MISMATCH;
            }

            unsafe {
                let info = process_information as *mut PROCESS_BASIC_INFORMATION;
                (*info).exit_status = (*process)
                    .exit_status
                    .load(core::sync::atomic::Ordering::Acquire)
                    as i32;
                (*info).peb_base_address = (*process).peb;
                (*info).affinity_mask = (*process)
                    .pcb
                    .active_processors
                    .load(core::sync::atomic::Ordering::Acquire);
                (*info).base_priority = (*process).pcb.base_priority as i32;
                (*info).unique_process_id = (*process).process_id();
                (*info).inherited_from_unique_process_id =
                    (*process).inherited_from_unique_process_id;

                if !return_length.is_null() {
                    *return_length = core::mem::size_of::<PROCESS_BASIC_INFORMATION>() as u32;
                }
            }

            STATUS_SUCCESS
        },

        PROCESSINFOCLASS::ProcessHandleCount => {
            if process_information_length < 4 {
                return STATUS_INFO_LENGTH_MISMATCH;
            }

            unsafe {
                let handle_table = (*process)
                    .object_table
                    .load(core::sync::atomic::Ordering::Acquire);
                let count = if handle_table.is_null() {
                    0
                } else {
                    (*handle_table)
                        .handle_count
                        .load(core::sync::atomic::Ordering::Acquire)
                };

                *(process_information as *mut u32) = count as u32;

                if !return_length.is_null() {
                    *return_length = 4;
                }
            }

            STATUS_SUCCESS
        },

        PROCESSINFOCLASS::ProcessSessionInformation => {
            if process_information_length < 4 {
                return STATUS_INFO_LENGTH_MISMATCH;
            }

            unsafe {
                *(process_information as *mut u32) = (*process).session_id;

                if !return_length.is_null() {
                    *return_length = 4;
                }
            }

            STATUS_SUCCESS
        },

        _ => {
            // Неподдерживаемый класс информации
            STATUS_INVALID_PARAMETER
        },
    }
}

// =============================================================================
// NtQueryInformationThread
// =============================================================================

/// Запрашивает информацию о потоке
pub fn nt_query_information_thread(
    thread_handle: PVOID,
    thread_information_class: THREADINFOCLASS,
    thread_information: PVOID,
    thread_information_length: u32,
    return_length: *mut u32,
) -> NTSTATUS {
    if thread_information.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    // Получаем поток из handle
    let thread = get_thread_from_handle(thread_handle);
    if thread.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    match thread_information_class {
        THREADINFOCLASS::ThreadBasicInformation => {
            // THREAD_BASIC_INFORMATION
            #[repr(C)]
            struct ThreadBasicInfo {
                exit_status: i32,
                teb_base_address: PVOID,
                client_id: CLIENT_ID,
                affinity_mask: usize,
                priority: i32,
                base_priority: i32,
            }

            if thread_information_length < core::mem::size_of::<ThreadBasicInfo>() as u32 {
                return STATUS_INFO_LENGTH_MISMATCH;
            }

            unsafe {
                let info = thread_information as *mut ThreadBasicInfo;
                (*info).exit_status = (*thread)
                    .exit_status
                    .load(core::sync::atomic::Ordering::Acquire);
                (*info).teb_base_address = (*thread).tcb.teb;
                (*info).client_id = (*thread).cid;
                (*info).affinity_mask = (*thread).tcb.affinity as usize;
                (*info).priority = (*thread).tcb.priority as i32;
                (*info).base_priority = (*thread).tcb.base_priority as i32;

                if !return_length.is_null() {
                    *return_length = core::mem::size_of::<ThreadBasicInfo>() as u32;
                }
            }

            STATUS_SUCCESS
        },

        THREADINFOCLASS::ThreadQuerySetWin32StartAddress => {
            if thread_information_length < 8 {
                return STATUS_INFO_LENGTH_MISMATCH;
            }

            unsafe {
                *(thread_information as *mut PVOID) = (*thread).win32_start_address;

                if !return_length.is_null() {
                    *return_length = 8;
                }
            }

            STATUS_SUCCESS
        },

        _ => STATUS_INVALID_PARAMETER,
    }
}

// =============================================================================
// Handle Resolution
// =============================================================================

/// Получает процесс из handle
fn get_process_from_handle(handle: PVOID) -> *mut EPROCESS {
    let handle_value = handle as isize;

    // NtCurrentProcess pseudo-handle
    if handle_value == NT_CURRENT_PROCESS {
        return unsafe { ps_get_current_process() };
    }

    // TODO: Полная реализация через ObReferenceObjectByHandle
    // Пока простая заглушка
    core::ptr::null_mut()
}

/// Получает поток из handle
fn get_thread_from_handle(handle: PVOID) -> *mut super::thread::ETHREAD {
    let handle_value = handle as isize;

    // NtCurrentThread pseudo-handle
    if handle_value == NT_CURRENT_THREAD {
        return unsafe { ps_get_current_thread() as *mut super::thread::ETHREAD };
    }

    // TODO: Полная реализация через ObReferenceObjectByHandle
    core::ptr::null_mut()
}

// =============================================================================
// Convenience Functions
// =============================================================================

/// Возвращает количество активных процессов
pub fn ps_get_process_count() -> usize {
    let mut count = 0usize;

    unsafe {
        let head = PS_ACTIVE_PROCESS_HEAD.get();
        let mut entry = (*head).flink;

        while !entry.is_null() && entry != head as *mut crate::nt::LIST_ENTRY {
            count += 1;
            entry = (*entry).flink;
        }
    }

    count
}

/// Итератор по активным процессам
pub struct ProcessIterator {
    current: *mut crate::nt::LIST_ENTRY,
    head: *mut crate::nt::LIST_ENTRY,
}

impl ProcessIterator {
    pub fn new() -> Self {
        unsafe {
            let head = PS_ACTIVE_PROCESS_HEAD.get();
            Self {
                current: (*head).flink,
                head: head as *mut crate::nt::LIST_ENTRY,
            }
        }
    }
}

impl Iterator for ProcessIterator {
    type Item = *mut EPROCESS;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current.is_null() || self.current == self.head {
            return None;
        }

        unsafe {
            // CONTAINING_RECORD equivalent
            let offset = core::mem::offset_of!(EPROCESS, active_process_links);
            let process = (self.current as *mut u8).sub(offset) as *mut EPROCESS;

            self.current = (*self.current).flink;

            Some(process)
        }
    }
}
