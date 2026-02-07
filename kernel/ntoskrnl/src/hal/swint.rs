//! Software Interrupts (APC/DPC) via self-IPI (APIC)
//!
//! Публичные точки входа (NT ABI, x64):
//! - HalRequestSoftwareInterrupt
//! - HalClearSoftwareInterrupt

use crate::arch::x86_64::pcr;
use crate::hal::apic::APC_VECTOR;
use crate::hal::apic::ApicRegister;
use crate::hal::apic::DISPATCH_VECTOR;
use crate::hal::apic::apic_read;
use crate::hal::apic::apic_write;
use crate::hal::irql::APC_LEVEL;
use crate::hal::irql::DISPATCH_LEVEL;
use crate::hal::irql::KIRQL;

fn apic_request_self_interrupt(vector: u8) {
    // Ждем пока APIC свободен (Delivery Status = 0)
    loop {
        let icr_low = apic_read(ApicRegister::Icr0);
        if (icr_low & (1 << 12)) == 0 {
            break;
        }
        crate::arch::x86_64::cpu::yield_processor();
    }

    // ICR High = 0 (dest не используется для shorthand=self)
    apic_write(ApicRegister::Icr1, 0);

    // ICR Low:
    // - vector
    // - fixed delivery
    // - level=assert
    // - shorthand=self
    let icr_low: u32 = (vector as u32) | (1 << 14) | (1 << 18);

    apic_write(ApicRegister::Icr0, icr_low);
}

/// HalRequestSoftwareInterrupt(IRQL)
///
/// NT: выставляет флаг в PRCB и генерирует self-IPI.
#[unsafe(no_mangle)]
pub extern "win64" fn hal_request_software_interrupt(irql: KIRQL) {
    let prcb = unsafe { pcr::get_prcb() };
    if prcb.is_null() {
        return;
    }

    let vector = match irql {
        APC_LEVEL => {
            unsafe {
                (*prcb).apc_interrupt_requested = 1;
            }
            APC_VECTOR
        },
        DISPATCH_LEVEL => {
            unsafe {
                (*prcb).dpc_interrupt_requested = 1;
            }
            DISPATCH_VECTOR
        },
        _ => return,
    };

    apic_request_self_interrupt(vector);
}

/// HalClearSoftwareInterrupt(IRQL)
#[unsafe(no_mangle)]
pub extern "win64" fn hal_clear_software_interrupt(irql: KIRQL) {
    let prcb = unsafe { pcr::get_prcb() };
    if prcb.is_null() {
        return;
    }

    match irql {
        APC_LEVEL => unsafe {
            (*prcb).apc_interrupt_requested = 0;
        },
        DISPATCH_LEVEL => unsafe {
            (*prcb).dpc_interrupt_requested = 0;
        },
        _ => {},
    }
}

#[inline]
pub fn is_apc_interrupt_pending() -> bool {
    let prcb = unsafe { pcr::get_prcb() };
    if prcb.is_null() {
        return false;
    }
    unsafe { (*prcb).apc_interrupt_requested != 0 }
}

#[inline]
pub fn is_dpc_interrupt_pending() -> bool {
    let prcb = unsafe { pcr::get_prcb() };
    if prcb.is_null() {
        return false;
    }
    unsafe { (*prcb).dpc_interrupt_requested != 0 }
}
