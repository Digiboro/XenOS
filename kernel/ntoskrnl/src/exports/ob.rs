//! Object Manager PE-экспорты (`Ob*`)
//!
//! Экспорты для boot-драйверов и других PE модулей.
//!
//! Источники:
//! - MSDN/WDK (NT6.1): https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/

#![allow(non_snake_case)]

use ntoskrnl::ob::refcount::{ob_reference_object, ob_dereference_object};
use ntoskrnl::nt::ntdef::PVOID;

// =============================================================================
// Object Reference Counting
// =============================================================================

/// ObReferenceObject — увеличивает счётчик ссылок объекта
///
/// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/nf-wdm-obfreferenceobject
#[unsafe(export_name = "ObReferenceObject")]
pub unsafe extern "win64" fn ObReferenceObject(object: PVOID) {
    unsafe { ob_reference_object(object); }
}

/// ObfReferenceObject — fast path версия
#[unsafe(export_name = "ObfReferenceObject")]
pub unsafe extern "win64" fn ObfReferenceObject(object: PVOID) {
    unsafe { ob_reference_object(object); }
}

/// ObDereferenceObject — уменьшает счётчик ссылок объекта
///
/// Когда счётчик достигает 0, объект удаляется.
///
/// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/nf-wdm-obdereferenceobject
#[unsafe(export_name = "ObDereferenceObject")]
pub unsafe extern "win64" fn ObDereferenceObject(object: PVOID) {
    unsafe { ob_dereference_object(object); }
}

/// ObfDereferenceObject — fast path версия
#[unsafe(export_name = "ObfDereferenceObject")]
pub unsafe extern "win64" fn ObfDereferenceObject(object: PVOID) {
    unsafe { ob_dereference_object(object); }
}

