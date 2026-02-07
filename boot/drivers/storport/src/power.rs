//! Power Management для StorPort
//!
//! Фаза 2.5: Корректная обработка Power IRP согласно NT 6.1 модели.

use crate::types::*;
use crate::imports::ntoskrnl::*;
use crate::miniport::*;
use crate::{storport_print, storport_print_hex};
use core::ptr;

// =============================================================================
// Power IRP Dispatch
// =============================================================================

/// Главный Power dispatch handler для StorPort
/// 
/// Power IRP требуют специальной обработки:
/// - Использовать PoCallDriver вместо IoCallDriver
/// - Вызывать PoStartNextPowerIrp для старых драйверов
/// - Правильно обрабатывать IRP_MN_SET_POWER и IRP_MN_QUERY_POWER
pub unsafe fn storport_power_dispatch(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    unsafe {
        let stack = IoGetCurrentIrpStackLocation(irp);
        let minor = (*stack).minor_function;
        
        #[cfg(feature = "storage-trace")]
        {
            storport_print("[STORPORT] Power IRP: minor=0x");
            storport_print_hex(minor as u64);
            storport_print("\n");
        }
        
        let ext = (*device_object).device_extension as *mut STORPORT_DEVICE_EXTENSION;
        
        // Проверяем FDO или PDO
        let is_fdo = !ext.is_null() && (*ext).signature == STORPORT_DEVICE_EXTENSION_SIGNATURE;
        
        match minor {
            IRP_MN_SET_POWER => handle_set_power(device_object, irp, ext, is_fdo),
            IRP_MN_QUERY_POWER => handle_query_power(device_object, irp, ext, is_fdo),
            IRP_MN_WAIT_WAKE => handle_wait_wake(device_object, irp, ext, is_fdo),
            _ => {
                // Для остальных - просто передаём вниз
                if is_fdo {
                    // FDO: используем PoCallDriver
                    PoStartNextPowerIrp(irp);
                    IoSkipCurrentIrpStackLocation(irp);
                    PoCallDriver((*ext).lower_device, irp)
                } else {
                    // PDO: завершаем успешно
                    PoStartNextPowerIrp(irp);
                    (*irp).io_status.status = STATUS_SUCCESS;
                    IoCompleteRequest(irp, IO_NO_INCREMENT);
                    STATUS_SUCCESS
                }
            }
        }
    }
}

/// IRP_MN_SET_POWER
/// 
/// Устанавливает состояние питания устройства или системы.
unsafe fn handle_set_power(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
    ext: *mut STORPORT_DEVICE_EXTENSION,
    is_fdo: bool,
) -> NTSTATUS {
    unsafe {
        let stack = IoGetCurrentIrpStackLocation(irp);
        
        // Power parameters из IO_STACK_LOCATION.Parameters.Power
        // Layout: Type (ULONG), State (union POWER_STATE)
        let params_ptr = &(*stack).parameters as *const _ as *const u8;
        let power_type = core::ptr::read(params_ptr as *const ULONG);
        let power_state = core::ptr::read(params_ptr.add(4) as *const ULONG);
        
        #[cfg(feature = "storage-trace")]
        {
            storport_print("[STORPORT] SET_POWER: type=");
            storport_print_hex(power_type as u64);
            storport_print(" state=");
            storport_print_hex(power_state as u64);
            storport_print("\n");
        }
        
        if is_fdo {
            // FDO: обрабатываем в зависимости от типа
            if power_type == DEVICE_POWER_STATE {
                // Device power state change
                handle_device_power_state(ext, power_state);
            }
            // System power state - просто передаём вниз
            
            PoStartNextPowerIrp(irp);
            IoSkipCurrentIrpStackLocation(irp);
            PoCallDriver((*ext).lower_device, irp)
        } else {
            // PDO: завершаем успешно
            PoStartNextPowerIrp(irp);
            (*irp).io_status.status = STATUS_SUCCESS;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            STATUS_SUCCESS
        }
    }
}

/// IRP_MN_QUERY_POWER
/// 
/// Запрос на изменение состояния питания.
/// Драйвер может отклонить если не готов к переходу.
unsafe fn handle_query_power(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
    ext: *mut STORPORT_DEVICE_EXTENSION,
    is_fdo: bool,
) -> NTSTATUS {
    unsafe {
        #[cfg(feature = "storage-trace")]
        storport_print("[STORPORT] QUERY_POWER\n");
        
        if is_fdo {
            // FDO: проверяем можем ли перейти в новое состояние
            // Для простоты всегда разрешаем
            
            PoStartNextPowerIrp(irp);
            IoSkipCurrentIrpStackLocation(irp);
            PoCallDriver((*ext).lower_device, irp)
        } else {
            // PDO: всегда разрешаем
            PoStartNextPowerIrp(irp);
            (*irp).io_status.status = STATUS_SUCCESS;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            STATUS_SUCCESS
        }
    }
}

/// IRP_MN_WAIT_WAKE
/// 
/// Запрос на пробуждение системы при событии на устройстве.
/// StorPort обычно не поддерживает, просто отклоняем.
unsafe fn handle_wait_wake(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
    ext: *mut STORPORT_DEVICE_EXTENSION,
    is_fdo: bool,
) -> NTSTATUS {
    unsafe {
        #[cfg(feature = "storage-trace")]
        storport_print("[STORPORT] WAIT_WAKE (not supported)\n");
        
        if is_fdo {
            // Передаём вниз - пусть hardware решает
            PoStartNextPowerIrp(irp);
            IoSkipCurrentIrpStackLocation(irp);
            PoCallDriver((*ext).lower_device, irp)
        } else {
            // PDO: не поддерживаем
            PoStartNextPowerIrp(irp);
            (*irp).io_status.status = STATUS_NOT_SUPPORTED;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            STATUS_NOT_SUPPORTED
        }
    }
}

/// Обработка изменения device power state
/// 
/// Состояния:
/// - D0: полностью включён
/// - D1-D2: intermediate states (редко используются)
/// - D3: выключен
unsafe fn handle_device_power_state(
    ext: *mut STORPORT_DEVICE_EXTENSION,
    new_state: ULONG,
) {
    unsafe {
        if ext.is_null() {
            return;
        }
        
        #[cfg(feature = "storage-trace")]
        {
            storport_print("[STORPORT] Device power state -> D");
            storport_print_hex(new_state as u64);
            storport_print("\n");
        }
        
        match new_state {
            POWER_DEVICE_D0 => {
                // Переход в D0 (Working) - возобновляем работу
                // Если есть HwAdapterControl - вызываем с ScsiRestartAdapter
                if let Some(hw_adapter_control) = (*ext).hw_init_data.HwAdapterControl {
                    #[cfg(feature = "storage-trace")]
                    storport_print("[STORPORT] Calling HwAdapterControl(RESTART)\n");
                    
                    hw_adapter_control(
                        (*ext).miniport_device_extension,
                        SCSI_ADAPTER_CONTROL_RESTART,
                        ptr::null_mut(),
                    );
                }
            }
            POWER_DEVICE_D3 => {
                // Переход в D3 (Off) - останавливаем адаптер
                // Сначала отменяем все pending запросы
                if (*ext).in_flight_count > 0 {
                    #[cfg(feature = "storage-trace")]
                    storport_print("[STORPORT] Cancelling requests before D3\n");
                    (*ext).cancel_all_pending();
                }
                
                // Вызываем HwAdapterControl(STOP)
                if let Some(hw_adapter_control) = (*ext).hw_init_data.HwAdapterControl {
                    #[cfg(feature = "storage-trace")]
                    storport_print("[STORPORT] Calling HwAdapterControl(STOP)\n");
                    
                    hw_adapter_control(
                        (*ext).miniport_device_extension,
                        SCSI_ADAPTER_CONTROL_STOP,
                        ptr::null_mut(),
                    );
                }
            }
            _ => {
                // D1, D2 - промежуточные состояния, обычно не используются
                #[cfg(feature = "storage-trace")]
                storport_print("[STORPORT] Intermediate power state (ignored)\n");
            }
        }
    }
}

// =============================================================================
// Power State Constants
// =============================================================================

/// Power state types
pub const SYSTEM_POWER_STATE: ULONG = 0;
pub const DEVICE_POWER_STATE: ULONG = 1;

/// Device power states
pub const POWER_DEVICE_UNSPECIFIED: ULONG = 0;
pub const POWER_DEVICE_D0: ULONG = 1; // Full power
pub const POWER_DEVICE_D1: ULONG = 2; // Light sleep
pub const POWER_DEVICE_D2: ULONG = 3; // Medium sleep
pub const POWER_DEVICE_D3: ULONG = 4; // Power off

/// System power states
pub const POWER_SYSTEM_UNSPECIFIED: ULONG = 0;
pub const POWER_SYSTEM_WORKING: ULONG = 1;       // S0
pub const POWER_SYSTEM_SLEEPING1: ULONG = 2;     // S1
pub const POWER_SYSTEM_SLEEPING2: ULONG = 3;     // S2
pub const POWER_SYSTEM_SLEEPING3: ULONG = 4;     // S3 (Standby)
pub const POWER_SYSTEM_HIBERNATE: ULONG = 5;     // S4 (Hibernate)
pub const POWER_SYSTEM_SHUTDOWN: ULONG = 6;      // S5 (Shutdown)

