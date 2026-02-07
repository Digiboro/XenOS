//! Executive PE-экспорты (`Ex*`)
//!
//! Экспорты для boot-драйверов и других PE модулей.
//!
//! Источники:
//! - MSDN/WDK (NT6.1): https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/

#![allow(non_snake_case)]

use ntoskrnl::ex::pool::{ex_allocate_pool_with_tag, ex_free_pool_with_tag, POOL_TYPE};
use ntoskrnl::nt::ntdef::PVOID;

/// ULONG — unsigned long (NT typedef)
pub type ULONG = u32;
/// ULONG_PTR — pointer-sized unsigned
pub type ULONG_PTR = usize;

// =============================================================================
// Pool Allocation
// =============================================================================

/// ExAllocatePoolWithTag — выделяет память из пула с тегом
///
/// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/nf-wdm-exallocatepoolwithtag
#[unsafe(export_name = "ExAllocatePoolWithTag")]
pub unsafe extern "win64" fn ExAllocatePoolWithTag(
    pool_type: ULONG,
    number_of_bytes: ULONG_PTR,
    tag: ULONG,
) -> PVOID {
    let pt = match pool_type {
        0 => POOL_TYPE::NonPagedPool,
        1 => POOL_TYPE::PagedPool,
        _ => POOL_TYPE::NonPagedPool,
    };
    ex_allocate_pool_with_tag(pt, number_of_bytes, tag)
}

/// ExFreePoolWithTag — освобождает память выделенную из пула
///
/// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/nf-wdm-exfreepoolwithtag
#[unsafe(export_name = "ExFreePoolWithTag")]
pub unsafe extern "win64" fn ExFreePoolWithTag(ptr: PVOID, tag: ULONG) {
    ex_free_pool_with_tag(ptr, tag)
}

/// ExFreePool — освобождает память (legacy API)
#[unsafe(export_name = "ExFreePool")]
pub unsafe extern "win64" fn ExFreePool(ptr: PVOID) {
    ex_free_pool_with_tag(ptr, 0)
}

