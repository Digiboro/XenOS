//! Power Manager PE-экспорты (`Po*`)
//!
//! Экспорты для boot-драйверов и других PE модулей.
//!
//! Источники:
//! - MSDN/WDK (NT6.1): https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/

#![allow(non_snake_case)]

use ntoskrnl::io::pnp::{
    po_call_driver, po_start_next_power_irp, po_request_power_irp, po_set_power_state,
    POWER_STATE, POWER_STATE_TYPE,
};
use ntoskrnl::io::types::{PDEVICE_OBJECT, PIRP};
use ntoskrnl::nt::ntstatus::NTSTATUS;
use ntoskrnl::nt::ntdef::PVOID;

// =============================================================================
// Power IRP Handling
// =============================================================================

/// PoCallDriver — вызывает dispatch routine для Power IRP
///
/// Аналог IoCallDriver, но для Power IRP.
///
/// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-pocalldriver
#[unsafe(export_name = "PoCallDriver")]
pub unsafe extern "win64" fn PoCallDriver(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    unsafe { po_call_driver(device_object, irp) }
}

/// PoStartNextPowerIrp — сигнализирует готовность к следующему Power IRP
///
/// Должен вызываться драйвером после обработки Power IRP.
///
/// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-postartnextpowerirp
#[unsafe(export_name = "PoStartNextPowerIrp")]
pub unsafe extern "win64" fn PoStartNextPowerIrp(irp: PIRP) {
    unsafe { po_start_next_power_irp(irp) }
}

/// PoRequestPowerIrp — запрашивает Power IRP для устройства
///
/// Выделяет и отправляет Power IRP для изменения состояния питания.
///
/// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/nf-wdm-porequestpowerirp
#[unsafe(export_name = "PoRequestPowerIrp")]
pub unsafe extern "win64" fn PoRequestPowerIrp(
    device_object: PDEVICE_OBJECT,
    minor_function: u8,
    power_state: POWER_STATE,
    completion_function: PVOID,
    context: PVOID,
    irp: *mut PIRP,
) -> NTSTATUS {
    unsafe {
        po_request_power_irp(
            device_object,
            minor_function,
            power_state,
            completion_function,
            context,
            irp,
        )
    }
}

/// PoSetPowerState — устанавливает текущее состояние питания устройства
///
/// Информирует Power Manager о текущем состоянии питания устройства.
///
/// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-posetpowerstate
#[unsafe(export_name = "PoSetPowerState")]
pub unsafe extern "win64" fn PoSetPowerState(
    device_object: PDEVICE_OBJECT,
    power_type: POWER_STATE_TYPE,
    power_state: POWER_STATE,
) -> POWER_STATE {
    unsafe { po_set_power_state(device_object, power_type, power_state) }
}
