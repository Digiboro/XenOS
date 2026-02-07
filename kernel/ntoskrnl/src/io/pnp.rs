//! Plug and Play и Power Management
//!
//! # Архитектура
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    PnP Manager                              │
//! ├─────────────────────────────────────────────────────────────┤
//! │                                                             │
//! │  Device Enumeration                                         │
//! │        │                                                    │
//! │        ▼                                                    │
//! │  ┌─────────────────┐     ┌─────────────────┐                │
//! │  │ IRP_MJ_PNP      │ ──► │ IRP_MN_*        │                │
//! │  │ IRP_MN_START    │     │ Minor functions │                │
//! │  │ IRP_MN_STOP     │     └─────────────────┘                │
//! │  │ IRP_MN_REMOVE   │                                        │
//! │  └─────────────────┘                                        │
//! │                                                             │
//! └─────────────────────────────────────────────────────────────┘
//!
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    Power Manager                            │
//! ├─────────────────────────────────────────────────────────────┤
//! │                                                             │
//! │  ┌─────────────────┐     ┌─────────────────┐                │
//! │  │ IRP_MJ_POWER    │ ──► │ IRP_MN_SET_POWER│                │
//! │  │                 │     │ IRP_MN_QUERY    │                │
//! │  │                 │     │ IRP_MN_WAIT_WAKE│                │
//! │  └─────────────────┘     └─────────────────┘                │
//! │                                                             │
//! │  Power States:                                              │
//! │    S0 (Working) ◄──► S1-S4 (Sleep) ◄──► S5 (Off)            │
//! │    D0 (Full) ◄──► D1-D2 (Low) ◄──► D3 (Off)                 │
//! │                                                             │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Отступления и упрощения
//!
//! - Упрощённая модель PnP без полного дерева устройств
//! - Нет поддержки ACPI интеграции
//! - Заглушки для большинства функций
//!
//! Источники:
//! - ReactOS: ntoskrnl/io/pnpmgr/, ntoskrnl/po/
//! - Windows XP: base/ntos/io/pnpmgr/, base/ntos/po/

use super::device::DEVICE_OBJECT;
use super::irp::io_get_current_irp_stack_location;
use super::types::*;
use crate::nt::NTSTATUS;
use crate::nt::PVOID;
use crate::nt::ULONG;
use crate::nt::USHORT;
use crate::nt::ntstatus::*;

// =============================================================================
// PnP Minor Function Codes (IRP_MN_*)
// =============================================================================

/// IRP_MN_START_DEVICE — запуск устройства
pub const IRP_MN_START_DEVICE: u8 = 0x00;
/// IRP_MN_QUERY_REMOVE_DEVICE — запрос на удаление
pub const IRP_MN_QUERY_REMOVE_DEVICE: u8 = 0x01;
/// IRP_MN_REMOVE_DEVICE — удаление устройства
pub const IRP_MN_REMOVE_DEVICE: u8 = 0x02;
/// IRP_MN_CANCEL_REMOVE_DEVICE — отмена удаления
pub const IRP_MN_CANCEL_REMOVE_DEVICE: u8 = 0x03;
/// IRP_MN_STOP_DEVICE — остановка устройства
pub const IRP_MN_STOP_DEVICE: u8 = 0x04;
/// IRP_MN_QUERY_STOP_DEVICE — запрос на остановку
pub const IRP_MN_QUERY_STOP_DEVICE: u8 = 0x05;
/// IRP_MN_CANCEL_STOP_DEVICE — отмена остановки
pub const IRP_MN_CANCEL_STOP_DEVICE: u8 = 0x06;
/// IRP_MN_QUERY_DEVICE_RELATIONS — запрос связей
pub const IRP_MN_QUERY_DEVICE_RELATIONS: u8 = 0x07;
/// IRP_MN_QUERY_INTERFACE — запрос интерфейса
pub const IRP_MN_QUERY_INTERFACE: u8 = 0x08;
/// IRP_MN_QUERY_CAPABILITIES — запрос возможностей
pub const IRP_MN_QUERY_CAPABILITIES: u8 = 0x09;
/// IRP_MN_QUERY_RESOURCES — запрос ресурсов
pub const IRP_MN_QUERY_RESOURCES: u8 = 0x0A;
/// IRP_MN_QUERY_RESOURCE_REQUIREMENTS — запрос требований
pub const IRP_MN_QUERY_RESOURCE_REQUIREMENTS: u8 = 0x0B;
/// IRP_MN_QUERY_DEVICE_TEXT — запрос текста устройства
pub const IRP_MN_QUERY_DEVICE_TEXT: u8 = 0x0C;
/// IRP_MN_FILTER_RESOURCE_REQUIREMENTS — фильтрация требований
pub const IRP_MN_FILTER_RESOURCE_REQUIREMENTS: u8 = 0x0D;
/// IRP_MN_READ_CONFIG — чтение конфигурации
pub const IRP_MN_READ_CONFIG: u8 = 0x0F;
/// IRP_MN_WRITE_CONFIG — запись конфигурации
pub const IRP_MN_WRITE_CONFIG: u8 = 0x10;
/// IRP_MN_EJECT — извлечение устройства
pub const IRP_MN_EJECT: u8 = 0x11;
/// IRP_MN_SET_LOCK — блокировка
pub const IRP_MN_SET_LOCK: u8 = 0x12;
/// IRP_MN_QUERY_ID — запрос ID
pub const IRP_MN_QUERY_ID: u8 = 0x13;
/// IRP_MN_QUERY_PNP_DEVICE_STATE — запрос состояния PnP
pub const IRP_MN_QUERY_PNP_DEVICE_STATE: u8 = 0x14;
/// IRP_MN_QUERY_BUS_INFORMATION — запрос информации о шине
pub const IRP_MN_QUERY_BUS_INFORMATION: u8 = 0x15;
/// IRP_MN_DEVICE_USAGE_NOTIFICATION — уведомление об использовании
pub const IRP_MN_DEVICE_USAGE_NOTIFICATION: u8 = 0x16;
/// IRP_MN_SURPRISE_REMOVAL — неожиданное удаление
pub const IRP_MN_SURPRISE_REMOVAL: u8 = 0x17;
/// IRP_MN_QUERY_LEGACY_BUS_INFORMATION — запрос legacy шины
pub const IRP_MN_QUERY_LEGACY_BUS_INFORMATION: u8 = 0x18;

// =============================================================================
// Power Minor Function Codes
// =============================================================================

/// IRP_MN_WAIT_WAKE — ожидание пробуждения
pub const IRP_MN_WAIT_WAKE: u8 = 0x00;
/// IRP_MN_POWER_SEQUENCE — последовательность питания
pub const IRP_MN_POWER_SEQUENCE: u8 = 0x01;
/// IRP_MN_SET_POWER — установка состояния питания
pub const IRP_MN_SET_POWER: u8 = 0x02;
/// IRP_MN_QUERY_POWER — запрос состояния питания
pub const IRP_MN_QUERY_POWER: u8 = 0x03;

// =============================================================================
// Power States
// =============================================================================

/// SYSTEM_POWER_STATE — состояния питания системы
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SYSTEM_POWER_STATE {
    PowerSystemUnspecified = 0,
    PowerSystemWorking = 1,   // S0
    PowerSystemSleeping1 = 2, // S1
    PowerSystemSleeping2 = 3, // S2
    PowerSystemSleeping3 = 4, // S3 (Suspend to RAM)
    PowerSystemHibernate = 5, // S4 (Suspend to Disk)
    PowerSystemShutdown = 6,  // S5
    PowerSystemMaximum = 7,
}

/// DEVICE_POWER_STATE — состояния питания устройства
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DEVICE_POWER_STATE {
    PowerDeviceUnspecified = 0,
    PowerDeviceD0 = 1, // Full power
    PowerDeviceD1 = 2, // Low power
    PowerDeviceD2 = 3, // Lower power
    PowerDeviceD3 = 4, // Off
    PowerDeviceMaximum = 5,
}

/// POWER_STATE — union системного и устройства состояния
#[repr(C)]
#[derive(Clone, Copy)]
pub union POWER_STATE {
    pub system_state: SYSTEM_POWER_STATE,
    pub device_state: DEVICE_POWER_STATE,
}

/// POWER_STATE_TYPE — тип состояния питания
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum POWER_STATE_TYPE {
    SystemPowerState = 0,
    DevicePowerState = 1,
}

/// POWER_ACTION — действие питания
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum POWER_ACTION {
    PowerActionNone = 0,
    PowerActionReserved = 1,
    PowerActionSleep = 2,
    PowerActionHibernate = 3,
    PowerActionShutdown = 4,
    PowerActionShutdownReset = 5,
    PowerActionShutdownOff = 6,
    PowerActionWarmEject = 7,
}

// =============================================================================
// Device Capabilities
// =============================================================================

/// DEVICE_CAPABILITIES — возможности устройства
#[repr(C)]
pub struct DEVICE_CAPABILITIES {
    pub size: USHORT,
    pub version: USHORT,
    // Битовые флаги
    pub device_d1: u32,
    pub device_d2: u32,
    pub lock_supported: u32,
    pub eject_supported: u32,
    pub removable: u32,
    pub dock_device: u32,
    pub unique_id: u32,
    pub silent_install: u32,
    pub raw_device_ok: u32,
    pub surprise_removal_ok: u32,
    pub wake_from_d0: u32,
    pub wake_from_d1: u32,
    pub wake_from_d2: u32,
    pub wake_from_d3: u32,
    pub hardware_disabled: u32,
    pub non_dynamic: u32,
    pub warm_eject_supported: u32,
    pub no_display_in_ui: u32,
    // Адрес и UINumber
    pub address: ULONG,
    pub ui_number: ULONG,
    // Маппинг состояний
    pub device_state: [DEVICE_POWER_STATE; 7], // POWER_SYSTEM_MAXIMUM
    pub system_wake: SYSTEM_POWER_STATE,
    pub device_wake: DEVICE_POWER_STATE,
    pub d1_latency: ULONG,
    pub d2_latency: ULONG,
    pub d3_latency: ULONG,
}

impl DEVICE_CAPABILITIES {
    pub const fn new() -> Self {
        Self {
            size: core::mem::size_of::<Self>() as USHORT,
            version: 1,
            device_d1: 0,
            device_d2: 0,
            lock_supported: 0,
            eject_supported: 0,
            removable: 0,
            dock_device: 0,
            unique_id: 0,
            silent_install: 0,
            raw_device_ok: 0,
            surprise_removal_ok: 0,
            wake_from_d0: 0,
            wake_from_d1: 0,
            wake_from_d2: 0,
            wake_from_d3: 0,
            hardware_disabled: 0,
            non_dynamic: 0,
            warm_eject_supported: 0,
            no_display_in_ui: 0,
            address: 0xFFFFFFFF,
            ui_number: 0xFFFFFFFF,
            device_state: [DEVICE_POWER_STATE::PowerDeviceUnspecified; 7],
            system_wake: SYSTEM_POWER_STATE::PowerSystemUnspecified,
            device_wake: DEVICE_POWER_STATE::PowerDeviceUnspecified,
            d1_latency: 0,
            d2_latency: 0,
            d3_latency: 0,
        }
    }
}

// =============================================================================
// PnP Functions
// =============================================================================

/// IoInvalidateDeviceRelations — инвалидирует связи устройства
///
/// Сообщает PnP Manager о необходимости перечислить устройства.
pub unsafe fn io_invalidate_device_relations(
    device_object: *mut DEVICE_OBJECT,
    r#type: ULONG, // DEVICE_RELATION_TYPE
) {
    // TODO: Реализовать уведомление PnP Manager
    let _ = device_object;
    let _ = r#type;
}

/// IoInvalidateDeviceState — инвалидирует состояние устройства
pub unsafe fn io_invalidate_device_state(device_object: *mut DEVICE_OBJECT) {
    // TODO: Реализовать уведомление PnP Manager
    let _ = device_object;
}

/// IoRequestDeviceEject — запрос на извлечение устройства
pub unsafe fn io_request_device_eject(physical_device_object: *mut DEVICE_OBJECT) {
    // TODO: Реализовать
    let _ = physical_device_object;
}

/// IopDefaultPnpHandler — обработчик PnP IRP по умолчанию
///
/// Передаёт IRP нижележащему драйверу или завершает с успехом.
pub unsafe extern "win64" fn iop_default_pnp_handler(
    _device_object: *mut DEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    unsafe {
        use super::irp::iof_complete_request;

        let stack = io_get_current_irp_stack_location(irp);
        let minor = (*stack).minor_function;

        match minor {
            IRP_MN_START_DEVICE
            | IRP_MN_QUERY_REMOVE_DEVICE
            | IRP_MN_QUERY_STOP_DEVICE
            | IRP_MN_CANCEL_REMOVE_DEVICE
            | IRP_MN_CANCEL_STOP_DEVICE => {
                // Успех по умолчанию
                (*irp).io_status.status = STATUS_SUCCESS;
                (*irp).io_status.information = 0;
            },
            IRP_MN_REMOVE_DEVICE | IRP_MN_STOP_DEVICE | IRP_MN_SURPRISE_REMOVAL => {
                // Успех по умолчанию
                (*irp).io_status.status = STATUS_SUCCESS;
                (*irp).io_status.information = 0;
            },
            IRP_MN_QUERY_CAPABILITIES => {
                // Возвращаем базовые возможности
                (*irp).io_status.status = STATUS_SUCCESS;
                (*irp).io_status.information = 0;
            },
            _ => {
                // Неподдерживаемые запросы — не изменяем статус
                // (IRP должен быть передан вниз по стеку)
            },
        }

        let status = (*irp).io_status.status;
        iof_complete_request(irp, 0); // IO_NO_INCREMENT
        status
    }
}

// =============================================================================
// Power Functions
// =============================================================================

/// Callback type for Power IRP completion
pub type REQUEST_POWER_COMPLETE = Option<unsafe extern "win64" fn(
    device_object: *mut DEVICE_OBJECT,
    minor_function: u8,
    power_state: POWER_STATE,
    context: PVOID,
    io_status: *mut super::types::IO_STATUS_BLOCK,
)>;

/// PoRequestPowerIrp — запрос Power IRP
///
/// Выделяет и отправляет Power IRP для изменения/запроса состояния питания.
/// 
/// # Arguments
/// * `device_object` - устройство, для которого запрашивается изменение питания
/// * `minor_function` - IRP_MN_SET_POWER или IRP_MN_QUERY_POWER
/// * `power_state` - целевое состояние питания
/// * `completion_function` - callback при завершении (может быть NULL)
/// * `context` - контекст для callback
/// * `irp` - [out] указатель для возврата IRP (может быть NULL)
///
/// # Returns
/// STATUS_PENDING при успехе (IRP отправлен), или код ошибки
pub unsafe fn po_request_power_irp(
    device_object: *mut DEVICE_OBJECT,
    minor_function: u8,
    power_state: POWER_STATE,
    completion_function: PVOID,
    context: PVOID,
    irp_out: *mut PIRP,
) -> NTSTATUS {
    unsafe {
        use super::irp::{io_allocate_irp, io_get_next_irp_stack_location, io_set_next_irp_stack_location};
        use super::types::IRP_MJ_POWER;
        
        #[cfg(feature = "trace-io")]
        {
            crate::kd::dbg_print("[POWER] PoRequestPowerIrp: device=0x");
            crate::kd::dbg_print_hex(device_object as u64);
            crate::kd::dbg_print(" minor=0x");
            crate::kd::dbg_print_hex(minor_function as u64);
            crate::kd::dbg_print("\n");
        }
        
        // Validate parameters
        if device_object.is_null() {
            #[cfg(feature = "trace-io")]
            crate::kd::dbg_print("[POWER] PoRequestPowerIrp: invalid device object\n");
            return STATUS_INVALID_PARAMETER;
        }
        
        // Validate minor function
        if minor_function != IRP_MN_SET_POWER && minor_function != IRP_MN_QUERY_POWER {
            #[cfg(feature = "trace-io")]
            crate::kd::dbg_print("[POWER] PoRequestPowerIrp: invalid minor function\n");
            return STATUS_INVALID_PARAMETER;
        }
        
        // Get top of device stack
        let top_device = super::device::io_get_attached_device(device_object);
        if top_device.is_null() {
            #[cfg(feature = "trace-io")]
            crate::kd::dbg_print("[POWER] PoRequestPowerIrp: no top device\n");
            return STATUS_INVALID_PARAMETER;
        }
        
        // Allocate IRP
        let stack_size = (*top_device).stack_size as i8;
        let irp = io_allocate_irp(stack_size, false);
        if irp.is_null() {
            #[cfg(feature = "trace-io")]
            crate::kd::dbg_print("[POWER] PoRequestPowerIrp: failed to allocate IRP\n");
            return STATUS_INSUFFICIENT_RESOURCES;
        }
        
        #[cfg(feature = "trace-io")]
        {
            crate::kd::dbg_print("[POWER] PoRequestPowerIrp: allocated IRP=0x");
            crate::kd::dbg_print_hex(irp as u64);
            crate::kd::dbg_print("\n");
        }
        
        // Initialize IRP
        (*irp).io_status.status = STATUS_NOT_SUPPORTED;
        (*irp).io_status.information = 0;
        
        // Get next stack location
        io_set_next_irp_stack_location(irp);
        let stack = io_get_next_irp_stack_location(irp);
        
        // Set up stack location for Power IRP
        (*stack).major_function = IRP_MJ_POWER;
        (*stack).minor_function = minor_function;
        (*stack).flags = 0;
        (*stack).control = 0;
        (*stack).device_object = top_device;
        (*stack).file_object = core::ptr::null_mut();
        
        // Set power parameters
        (*stack).parameters.power.system_context = 0;
        // Determine power type from minor function context
        // For device power states, we use DevicePowerState
        (*stack).parameters.power.power_type = POWER_STATE_TYPE::DevicePowerState;
        (*stack).parameters.power.power_state = power_state;
        (*stack).parameters.power.shutdown_type = POWER_ACTION::PowerActionNone;
        
        // Store completion info in IRP
        // NOTE: In a full implementation, we'd use a separate context structure
        // For now, store completion function in tail.overlay or use a different mechanism
        
        // Return IRP if caller wants it
        if !irp_out.is_null() {
            *irp_out = irp;
        }
        
        #[cfg(feature = "trace-io")]
        crate::kd::dbg_print("[POWER] PoRequestPowerIrp: sending IRP to driver...\n");
        
        // Send the IRP
        // NOTE: In NT, this is asynchronous and completion_function is called later
        // For now, we do synchronous dispatch and call completion immediately
        let status = po_call_driver(top_device, irp);
        
        #[cfg(feature = "trace-io")]
        {
            crate::kd::dbg_print("[POWER] PoRequestPowerIrp: driver returned status=0x");
            crate::kd::dbg_print_hex(status as u64);
            crate::kd::dbg_print("\n");
        }
        
        // If completion function was provided and IRP completed synchronously, call it
        if !completion_function.is_null() && status != STATUS_PENDING {
            let completion: REQUEST_POWER_COMPLETE = 
                core::mem::transmute(completion_function);
            
            if let Some(callback) = completion {
                #[cfg(feature = "trace-io")]
                crate::kd::dbg_print("[POWER] PoRequestPowerIrp: calling completion callback\n");
                
                callback(
                    device_object,
                    minor_function,
                    power_state,
                    context,
                    &mut (*irp).io_status,
                );
            }
        }
        
        // For synchronous completion, return the actual status
        // For async (STATUS_PENDING), caller should wait
        if status == STATUS_PENDING {
            status
        } else {
            // Return success if IRP completed successfully
            (*irp).io_status.status
        }
    }
}

/// PoSetPowerState — устанавливает состояние питания устройства
pub unsafe fn po_set_power_state(
    device_object: *mut DEVICE_OBJECT,
    r#type: POWER_STATE_TYPE,
    state: POWER_STATE,
) -> POWER_STATE {
    // TODO: Реализовать
    let _ = (device_object, r#type);
    state
}

/// PoCallDriver — отправляет Power IRP драйверу
///
/// Аналог IoCallDriver для Power IRP.
pub unsafe fn po_call_driver(device_object: *mut DEVICE_OBJECT, irp: PIRP) -> NTSTATUS {
    unsafe {
        // Для Power IRP используем стандартный IoCallDriver
        super::irp::io_call_driver(device_object, irp)
    }
}

/// PoStartNextPowerIrp — сигнализирует готовность к следующему Power IRP
///
/// Должен вызываться драйвером после обработки Power IRP.
pub unsafe fn po_start_next_power_irp(irp: PIRP) {
    // В упрощённой модели ничего не делаем
    let _ = irp;
}

/// IopDefaultPowerHandler — обработчик Power IRP по умолчанию
pub unsafe extern "win64" fn iop_default_power_handler(
    _device_object: *mut DEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    unsafe {
        use super::irp::iof_complete_request;

        let stack = io_get_current_irp_stack_location(irp);
        let minor = (*stack).minor_function;

        // Сигнализируем готовность к следующему IRP
        po_start_next_power_irp(irp);

        match minor {
            IRP_MN_SET_POWER | IRP_MN_QUERY_POWER => {
                // Успех по умолчанию
                (*irp).io_status.status = STATUS_SUCCESS;
                (*irp).io_status.information = 0;
            },
            IRP_MN_WAIT_WAKE => {
                // Не поддерживаем wake
                (*irp).io_status.status = STATUS_NOT_SUPPORTED;
                (*irp).io_status.information = 0;
            },
            _ => {
                (*irp).io_status.status = STATUS_SUCCESS;
                (*irp).io_status.information = 0;
            },
        }

        let status = (*irp).io_status.status;
        iof_complete_request(irp, 0);
        status
    }
}

// =============================================================================
// PnP Device State
// =============================================================================

/// PNP_DEVICE_STATE — флаги состояния PnP устройства
pub const PNP_DEVICE_DISABLED: ULONG = 0x00000001;
pub const PNP_DEVICE_DONT_DISPLAY_IN_UI: ULONG = 0x00000002;
pub const PNP_DEVICE_FAILED: ULONG = 0x00000004;
pub const PNP_DEVICE_REMOVED: ULONG = 0x00000008;
pub const PNP_DEVICE_RESOURCE_REQUIREMENTS_CHANGED: ULONG = 0x00000010;
pub const PNP_DEVICE_NOT_DISABLEABLE: ULONG = 0x00000020;

// =============================================================================
// Device Relations
// =============================================================================

/// DEVICE_RELATION_TYPE — тип связей устройств
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DEVICE_RELATION_TYPE {
    BusRelations = 0,
    EjectionRelations = 1,
    PowerRelations = 2,
    RemovalRelations = 3,
    TargetDeviceRelation = 4,
    SingleBusRelations = 5,
    TransportRelations = 6,
}

/// DEVICE_RELATIONS — список связанных устройств
#[repr(C)]
pub struct DEVICE_RELATIONS {
    pub count: ULONG,
    pub objects: [*mut DEVICE_OBJECT; 1], // Variable length array
}

// =============================================================================
// Bus Interface
// =============================================================================

/// BUS_QUERY_ID_TYPE — тип ID шины
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BUS_QUERY_ID_TYPE {
    BusQueryDeviceID = 0,
    BusQueryHardwareIDs = 1,
    BusQueryCompatibleIDs = 2,
    BusQueryInstanceID = 3,
    BusQueryDeviceSerialNumber = 4,
    BusQueryContainerID = 5,
}

/// DEVICE_TEXT_TYPE — тип текста устройства
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DEVICE_TEXT_TYPE {
    DeviceTextDescription = 0,
    DeviceTextLocationInformation = 1,
}
