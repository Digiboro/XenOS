//! Bugcheck callback API (KeRegisterBugCheckCallback)
//!
//! Цель: NT5-совместимая регистрация bugcheck callbacks и их вызов.
//! Источники:
//! - NT5: `inc/ke.h`, `ke/bugcheck.c`
//! - ReactOS: `ntoskrnl/ke/bug.c`

#![allow(dead_code)]

use crate::hal::irql::HIGH_LEVEL;
use crate::hal::irql::KIRQL;
use crate::hal::irql::{self};
use crate::nt::BOOLEAN;
use crate::nt::FALSE;
use crate::nt::LIST_ENTRY;
use crate::nt::PVOID;
use crate::nt::TRUE;
use crate::nt::ULONG;
use crate::nt::ULONG_PTR;

/// KBUGCHECK_BUFFER_DUMP_STATE (NT5 `ke.h`)
#[allow(non_camel_case_types)]
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KBUGCHECK_BUFFER_DUMP_STATE {
    BufferEmpty = 0,
    BufferInserted = 1,
    BufferStarted = 2,
    BufferFinished = 3,
    BufferIncomplete = 4,
}

/// PKBUGCHECK_CALLBACK_ROUTINE (NT5): `VOID (*)(PVOID Buffer, ULONG Length)`
#[allow(non_camel_case_types)]
pub type PKBUGCHECK_CALLBACK_ROUTINE = Option<extern "win64" fn(buffer: PVOID, length: ULONG)>;

/// KBUGCHECK_CALLBACK_RECORD (NT5 `ke.h`)
#[repr(C)]
pub struct KBUGCHECK_CALLBACK_RECORD {
    pub entry: LIST_ENTRY,
    pub callback_routine: PKBUGCHECK_CALLBACK_ROUTINE,
    pub buffer: PVOID,
    pub length: ULONG,
    pub component: *mut u8, // PUCHAR
    pub checksum: ULONG_PTR,
    pub state: u8, // KBUGCHECK_BUFFER_DUMP_STATE
}

impl KBUGCHECK_CALLBACK_RECORD {
    /// Эквивалент `KeInitializeCallbackRecord`: `State = BufferEmpty`.
    pub fn initialize(&mut self) {
        self.state = KBUGCHECK_BUFFER_DUMP_STATE::BufferEmpty as u8;
    }
}

#[inline]
fn checksum(rec: &KBUGCHECK_CALLBACK_RECORD) -> ULONG_PTR {
    (rec.callback_routine.map(|f| f as usize).unwrap_or(0) as ULONG_PTR)
        .wrapping_add(rec.buffer as usize)
        .wrapping_add(rec.length as usize)
        .wrapping_add(rec.component as usize)
}

#[inline]
unsafe fn bugcheck_list_head() -> *mut LIST_ENTRY {
    unsafe { crate::ke::init::ki_bugcheck_callback_list_head() }
}

/// Вызывает callbacks (аналог `KiDoBugCheckCallbacks` в ReactOS).
///
/// # Safety
/// Должно вызываться в bugcheck контексте (IRQL=HIGH_LEVEL, interrupts off).
pub unsafe fn ki_do_bugcheck_callbacks() {
    let list_head = unsafe { bugcheck_list_head() };
    if list_head.is_null() {
        return;
    }

    // Убедимся, что список хотя бы инициализирован (как в ReactOS: flink/blink != NULL).
    if unsafe { (*list_head).flink.is_null() || (*list_head).blink.is_null() } {
        return;
    }

    let mut last = list_head;
    let mut next = unsafe { (*list_head).flink };
    while next != list_head {
        let record = unsafe { crate::containing_record!(next, KBUGCHECK_CALLBACK_RECORD, entry) };

        // Минимальная валидация цепочки (по мотивам ReactOS).
        if unsafe { (*record).entry.blink } != last {
            return;
        }

        let cs = unsafe { checksum(&*record) };
        let state = unsafe { (*record).state };

        if state == (KBUGCHECK_BUFFER_DUMP_STATE::BufferInserted as u8)
            && unsafe { (*record).checksum } == cs
        {
            unsafe { (*record).state = KBUGCHECK_BUFFER_DUMP_STATE::BufferStarted as u8 };

            // В NT/ReactOS вызов обернут SEH. В Rust-sehless окружении делаем прямой вызов.
            if let Some(cb) = unsafe { (*record).callback_routine } {
                cb(unsafe { (*record).buffer }, unsafe { (*record).length });
                unsafe { (*record).state = KBUGCHECK_BUFFER_DUMP_STATE::BufferFinished as u8 };
            } else {
                unsafe { (*record).state = KBUGCHECK_BUFFER_DUMP_STATE::BufferIncomplete as u8 };
            }
        }

        last = next;
        next = unsafe { (*next).flink };
    }
}

/// KeRegisterBugCheckCallback (NT5 ABI).
#[unsafe(no_mangle)]
pub extern "win64" fn KeRegisterBugCheckCallback(
    callback_record: *mut KBUGCHECK_CALLBACK_RECORD,
    callback_routine: PKBUGCHECK_CALLBACK_ROUTINE,
    buffer: PVOID,
    length: ULONG,
    component: *mut u8,
) -> BOOLEAN {
    if callback_record.is_null() {
        return FALSE;
    }

    let mut old: KIRQL = 0;
    irql::ke_raise_irql(HIGH_LEVEL, &mut old as *mut KIRQL);

    let ok = unsafe {
        if (*callback_record).state == (KBUGCHECK_BUFFER_DUMP_STATE::BufferEmpty as u8) {
            (*callback_record).length = length;
            (*callback_record).buffer = buffer;
            (*callback_record).component = component;
            (*callback_record).callback_routine = callback_routine;
            (*callback_record).checksum = checksum(&*callback_record);
            (*callback_record).state = KBUGCHECK_BUFFER_DUMP_STATE::BufferInserted as u8;

            LIST_ENTRY::insert_tail(
                bugcheck_list_head(),
                core::ptr::addr_of_mut!((*callback_record).entry),
            );
            true
        } else {
            false
        }
    };

    irql::ke_lower_irql(old);
    if ok { TRUE } else { FALSE }
}

/// KeDeregisterBugCheckCallback (NT5 ABI).
#[unsafe(no_mangle)]
pub extern "win64" fn KeDeregisterBugCheckCallback(
    callback_record: *mut KBUGCHECK_CALLBACK_RECORD,
) -> BOOLEAN {
    if callback_record.is_null() {
        return FALSE;
    }

    let mut old: KIRQL = 0;
    irql::ke_raise_irql(HIGH_LEVEL, &mut old as *mut KIRQL);

    let ok = unsafe {
        if (*callback_record).state == (KBUGCHECK_BUFFER_DUMP_STATE::BufferInserted as u8) {
            (*callback_record).state = KBUGCHECK_BUFFER_DUMP_STATE::BufferEmpty as u8;
            LIST_ENTRY::remove_entry(core::ptr::addr_of_mut!((*callback_record).entry));
            true
        } else {
            false
        }
    };

    irql::ke_lower_irql(old);
    if ok { TRUE } else { FALSE }
}
