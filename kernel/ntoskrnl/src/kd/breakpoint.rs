//! Debug breakpoints (DbgBreakPoint / DbgBreakPointWithStatus)
//!
//! NT/ReactOS: реализуется как `int 3` (см. ReactOS `sdk/lib/rtl/amd64/debug_asm.S`).

#![allow(dead_code)]

/// DBG_STATUS_* коды (ReactOS `xdk/ketypes.h`)
pub mod dbg_status {
    pub const DBG_STATUS_CONTROL_C: u32 = 1;
    pub const DBG_STATUS_SYSRQ: u32 = 2;
    pub const DBG_STATUS_BUGCHECK_FIRST: u32 = 3;
    pub const DBG_STATUS_BUGCHECK_SECOND: u32 = 4;
    pub const DBG_STATUS_FATAL: u32 = 5;
    pub const DBG_STATUS_DEBUG_CONTROL: u32 = 6;
    pub const DBG_STATUS_WORKER: u32 = 7;
}

/// DbgBreakPoint — программный breakpoint.
#[unsafe(no_mangle)]
pub extern "win64" fn DbgBreakPoint() {
    unsafe { core::arch::asm!("int3", options(nomem, nostack, preserves_flags)) };
}

/// DbgBreakPointWithStatus — breakpoint с кодом статуса (RCX).
///
/// NOTE: как и в ReactOS, статус напрямую не используется инструкцией `int3`,
/// но нужен по ABI/семантике (KD может прочитать значение регистра/стека).
#[unsafe(no_mangle)]
pub extern "win64" fn DbgBreakPointWithStatus(_status: u32) {
    unsafe { core::arch::asm!("int3", options(nomem, nostack, preserves_flags)) };
}
