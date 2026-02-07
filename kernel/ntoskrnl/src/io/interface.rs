//! Device Interface Management
//!
//! Реализация Device Interfaces для NT 6.1 совместимости.
//! Device Interfaces позволяют драйверам публиковать интерфейсы по GUID,
//! которые user-mode приложения и другие драйверы могут обнаруживать.

#![allow(static_mut_refs)]
//!
//! # Архитектура
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                      Device Interface Flow                          │
//! ├─────────────────────────────────────────────────────────────────────┤
//! │                                                                     │
//! │  1. IoRegisterDeviceInterface(PDO, GUID, ReferenceString)          │
//! │     ├─> Создаёт запись в таблице интерфейсов                       │
//! │     └─> Возвращает SymbolicLinkName (disabled состояние)           │
//! │                                                                     │
//! │  2. IoSetDeviceInterfaceState(SymLinkName, TRUE)                   │
//! │     ├─> Создаёт symbolic link \??\{GUID}#InstancePath              │
//! │     └─> Отправляет PnP notification (volume arrival)              │
//! │                                                                     │
//! │  3. IoSetDeviceInterfaceState(SymLinkName, FALSE)                  │
//! │     ├─> Удаляет symbolic link                                      │
//! │     └─> Отправляет PnP notification (volume removal)              │
//! │                                                                     │
//! └─────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # References
//! - MSDN: IoRegisterDeviceInterface
//! - MSDN: IoSetDeviceInterfaceState
//! - WDK: wdm.h, iotypes.h

use super::types::*;
use crate::ex::pool::{ex_allocate_pool_with_tag, ex_free_pool_with_tag, POOL_TYPE};
use crate::nt::LIST_ENTRY;
use crate::nt::NTSTATUS;
use crate::nt::PVOID;
use crate::nt::ULONG;
use crate::nt::ntdef::UNICODE_STRING;
use crate::nt::ntstatus::*;
use crate::nt::security::GUID;
use crate::ke::spinlock::KSPIN_LOCK;
use core::sync::atomic::{AtomicU32, Ordering};

// =============================================================================
// Constants
// =============================================================================

/// Maximum length of device interface symbolic link name
const MAX_INTERFACE_NAME_LENGTH: usize = 512;

/// Pool tag for device interface allocations: 'IoIf'
const DEVICE_INTERFACE_TAG: ULONG = 0x66496F49;

// =============================================================================
// Device Interface Entry
// =============================================================================

/// Состояние device interface
#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeviceInterfaceState {
    /// Интерфейс зарегистрирован, но не активен
    Disabled = 0,
    /// Интерфейс активен (symbolic link создан)
    Enabled = 1,
}

/// Запись о зарегистрированном device interface
#[repr(C)]
pub struct DEVICE_INTERFACE_ENTRY {
    /// Связь в глобальном списке
    pub list_entry: LIST_ENTRY,
    /// GUID класса интерфейса
    pub interface_class_guid: GUID,
    /// Physical Device Object, для которого зарегистрирован интерфейс
    pub physical_device_object: PDEVICE_OBJECT,
    /// Reference string (опциональный)
    pub reference_string: [u16; 64],
    /// Длина reference string в символах
    pub reference_string_length: u16,
    /// Текущее состояние
    pub state: DeviceInterfaceState,
    /// Symbolic link name (полный путь)
    pub symbolic_link_name: [u16; MAX_INTERFACE_NAME_LENGTH],
    /// Длина symbolic link name в байтах
    pub symbolic_link_name_length: u16,
    /// Instance number (уникальный для данного GUID)
    pub instance_number: u32,
}

impl DEVICE_INTERFACE_ENTRY {
    pub fn new() -> Self {
        Self {
            list_entry: LIST_ENTRY::new(),
            interface_class_guid: GUID::null(),
            physical_device_object: core::ptr::null_mut(),
            reference_string: [0; 64],
            reference_string_length: 0,
            state: DeviceInterfaceState::Disabled,
            symbolic_link_name: [0; MAX_INTERFACE_NAME_LENGTH],
            symbolic_link_name_length: 0,
            instance_number: 0,
        }
    }
}

pub type PDEVICE_INTERFACE_ENTRY = *mut DEVICE_INTERFACE_ENTRY;

// =============================================================================
// Global State
// =============================================================================

/// Глобальный список зарегистрированных device interfaces
static mut IOP_DEVICE_INTERFACE_LIST: LIST_ENTRY = LIST_ENTRY::new();

/// Spinlock для защиты списка
static mut IOP_DEVICE_INTERFACE_LOCK: KSPIN_LOCK = KSPIN_LOCK::new();

/// Счётчик для генерации уникальных instance numbers
static IOP_INTERFACE_INSTANCE_COUNTER: AtomicU32 = AtomicU32::new(1);

/// Флаг инициализации
static mut IOP_DEVICE_INTERFACE_INITIALIZED: bool = false;

// =============================================================================
// PnP Notification Registration Structures
// =============================================================================

/// Pool tag for notification allocations: 'PnNt'
const PNP_NOTIFICATION_TAG: ULONG = 0x744E6E50;

/// Event category for IoRegisterPlugPlayNotification
#[repr(i32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IO_NOTIFICATION_EVENT_CATEGORY {
    /// Device interface change (arrival/removal)
    EventCategoryDeviceInterfaceChange = 0,
    /// Hardware profile change
    EventCategoryHardwareProfileChange = 1,
    /// Target device change
    EventCategoryTargetDeviceChange = 2,
    /// Reserved
    EventCategoryReserved = 3,
}

/// Callback function type for PnP notifications
/// Returns STATUS_SUCCESS to continue receiving notifications
pub type PDRIVER_NOTIFICATION_CALLBACK_ROUTINE = 
    Option<unsafe extern "win64" fn(
        notification_structure: PVOID,
        context: PVOID,
    ) -> NTSTATUS>;

/// Notification registration entry
#[repr(C)]
pub struct PNP_NOTIFICATION_ENTRY {
    /// Link in global list
    pub list_entry: LIST_ENTRY,
    /// Event category
    pub event_category: IO_NOTIFICATION_EVENT_CATEGORY,
    /// Interface GUID (for EventCategoryDeviceInterfaceChange)
    pub interface_class_guid: GUID,
    /// Callback function
    pub callback_routine: PDRIVER_NOTIFICATION_CALLBACK_ROUTINE,
    /// Context passed to callback
    pub context: PVOID,
    /// Driver object that registered
    pub driver_object: PDRIVER_OBJECT,
    /// Reference count
    pub ref_count: u32,
    /// Unregistered flag
    pub unregistered: bool,
}

impl PNP_NOTIFICATION_ENTRY {
    pub fn new() -> Self {
        Self {
            list_entry: LIST_ENTRY::new(),
            event_category: IO_NOTIFICATION_EVENT_CATEGORY::EventCategoryDeviceInterfaceChange,
            interface_class_guid: GUID::null(),
            callback_routine: None,
            context: core::ptr::null_mut(),
            driver_object: core::ptr::null_mut(),
            ref_count: 1,
            unregistered: false,
        }
    }
}

pub type PPNP_NOTIFICATION_ENTRY = *mut PNP_NOTIFICATION_ENTRY;

/// Device interface change notification structure
/// Passed to callback when device interface arrives/leaves
#[repr(C)]
pub struct DEVICE_INTERFACE_CHANGE_NOTIFICATION {
    /// Structure version (1)
    pub version: u16,
    /// Structure size
    pub size: u16,
    /// Event GUID (GUID_DEVICE_INTERFACE_ARRIVAL or GUID_DEVICE_INTERFACE_REMOVAL)
    pub event: GUID,
    /// Interface class GUID
    pub interface_class_guid: GUID,
    /// Symbolic link name of the interface
    pub symbolic_link_name: *mut UNICODE_STRING,
}

/// GUID_DEVICE_INTERFACE_ARRIVAL
/// {CB3A4004-46F0-11D0-B08F-00609713053F}
pub const GUID_DEVICE_INTERFACE_ARRIVAL: GUID = GUID {
    data1: 0xCB3A4004,
    data2: 0x46F0,
    data3: 0x11D0,
    data4: [0xB0, 0x8F, 0x00, 0x60, 0x97, 0x13, 0x05, 0x3F],
};

/// GUID_DEVICE_INTERFACE_REMOVAL
/// {CB3A4005-46F0-11D0-B08F-00609713053F}
pub const GUID_DEVICE_INTERFACE_REMOVAL: GUID = GUID {
    data1: 0xCB3A4005,
    data2: 0x46F0,
    data3: 0x11D0,
    data4: [0xB0, 0x8F, 0x00, 0x60, 0x97, 0x13, 0x05, 0x3F],
};

/// Global list of PnP notification registrations
static mut IOP_PNP_NOTIFICATION_LIST: LIST_ENTRY = LIST_ENTRY::new();

/// Spinlock for notification list
static mut IOP_PNP_NOTIFICATION_LOCK: KSPIN_LOCK = KSPIN_LOCK::new();

/// Counter for notification entry IDs
static IOP_NOTIFICATION_COUNTER: AtomicU32 = AtomicU32::new(1);

// =============================================================================
// Initialization
// =============================================================================

/// Инициализирует подсистему Device Interfaces
pub unsafe fn iop_init_device_interfaces() {
    unsafe {
        if IOP_DEVICE_INTERFACE_INITIALIZED {
            return;
        }
        
        LIST_ENTRY::init_head(&mut IOP_DEVICE_INTERFACE_LIST);
        IOP_DEVICE_INTERFACE_LOCK = KSPIN_LOCK::new();
        
        // Initialize notification list
        LIST_ENTRY::init_head(&mut IOP_PNP_NOTIFICATION_LIST);
        IOP_PNP_NOTIFICATION_LOCK = KSPIN_LOCK::new();
        
        IOP_DEVICE_INTERFACE_INITIALIZED = true;
        
        #[cfg(feature = "trace-io")]
        crate::kd::dbg_print("[INTERFACE] Device interface subsystem initialized\n");
    }
}

// =============================================================================
// IoRegisterDeviceInterface
// =============================================================================

/// IoRegisterDeviceInterface - регистрирует device interface для PDO
///
/// Создаёт запись о device interface для указанного класса GUID.
/// Интерфейс создаётся в disabled состоянии.
///
/// # Arguments
/// * `physical_device_object` - PDO устройства
/// * `interface_class_guid` - GUID класса интерфейса
/// * `reference_string` - опциональная строка для различения нескольких интерфейсов
/// * `symbolic_link_name` - [out] возвращаемое имя symbolic link
///
/// # Returns
/// STATUS_SUCCESS или код ошибки
///
/// # Reference
/// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/nf-wdm-ioregisterdeviceinterface
pub unsafe fn io_register_device_interface(
    physical_device_object: PDEVICE_OBJECT,
    interface_class_guid: *const GUID,
    reference_string: *const UNICODE_STRING,
    symbolic_link_name: *mut UNICODE_STRING,
) -> NTSTATUS {
    unsafe {
        // Validate parameters
        if physical_device_object.is_null() || interface_class_guid.is_null() || symbolic_link_name.is_null() {
            return STATUS_INVALID_PARAMETER;
        }
        
        // Ensure initialized
        if !IOP_DEVICE_INTERFACE_INITIALIZED {
            iop_init_device_interfaces();
        }
        
        // Allocate entry
        let entry = ex_allocate_pool_with_tag(
            POOL_TYPE::NonPagedPool,
            core::mem::size_of::<DEVICE_INTERFACE_ENTRY>(),
            DEVICE_INTERFACE_TAG,
        ) as PDEVICE_INTERFACE_ENTRY;
        
        if entry.is_null() {
            return STATUS_INSUFFICIENT_RESOURCES;
        }
        
        // Initialize entry
        core::ptr::write(entry, DEVICE_INTERFACE_ENTRY::new());
        (*entry).interface_class_guid = *interface_class_guid;
        (*entry).physical_device_object = physical_device_object;
        (*entry).state = DeviceInterfaceState::Disabled;
        
        // Copy reference string if provided
        if !reference_string.is_null() && (*reference_string).length > 0 {
            let ref_str = &*reference_string;
            let char_count = (ref_str.length / 2).min(63) as usize;
            core::ptr::copy_nonoverlapping(
                ref_str.buffer,
                (*entry).reference_string.as_mut_ptr(),
                char_count,
            );
            (*entry).reference_string_length = char_count as u16;
        }
        
        // Generate instance number
        let instance = IOP_INTERFACE_INSTANCE_COUNTER.fetch_add(1, Ordering::SeqCst);
        (*entry).instance_number = instance;
        
        // Build symbolic link name
        // Format: \??\{GUID}#instance or \??\{GUID}#refstring#instance
        let name_len = build_interface_symbolic_link_name(
            &*interface_class_guid,
            instance,
            if (*entry).reference_string_length > 0 {
                let len = (*entry).reference_string_length as usize;
                let ptr = core::ptr::addr_of!((*entry).reference_string) as *const u16;
                Some(core::slice::from_raw_parts(ptr, len))
            } else {
                None
            },
            &mut (*entry).symbolic_link_name,
        );
        (*entry).symbolic_link_name_length = name_len;
        
        // Acquire lock and add to global list
        let _irql = crate::ke::spinlock::ke_acquire_spin_lock(&IOP_DEVICE_INTERFACE_LOCK);
        LIST_ENTRY::insert_tail(&mut IOP_DEVICE_INTERFACE_LIST, &mut (*entry).list_entry);
        crate::ke::spinlock::ke_release_spin_lock(&IOP_DEVICE_INTERFACE_LOCK, _irql);
        
        // Return symbolic link name to caller
        // Caller must have allocated buffer
        if (*symbolic_link_name).buffer.is_null() || (*symbolic_link_name).maximum_length == 0 {
            // Allocate buffer for caller
            let buffer_size = (name_len as usize + 1) * 2; // +1 for null terminator, *2 for wide chars
            let buffer = ex_allocate_pool_with_tag(
                POOL_TYPE::PagedPool,
                buffer_size,
                DEVICE_INTERFACE_TAG,
            ) as *mut u16;
            
            if buffer.is_null() {
                // Cleanup entry
                let _irql = crate::ke::spinlock::ke_acquire_spin_lock(&IOP_DEVICE_INTERFACE_LOCK);
                LIST_ENTRY::remove_entry(&mut (*entry).list_entry);
                crate::ke::spinlock::ke_release_spin_lock(&IOP_DEVICE_INTERFACE_LOCK, _irql);
                ex_free_pool_with_tag(entry as PVOID, DEVICE_INTERFACE_TAG);
                return STATUS_INSUFFICIENT_RESOURCES;
            }
            
            core::ptr::copy_nonoverlapping(
                (*entry).symbolic_link_name.as_ptr(),
                buffer,
                name_len as usize,
            );
            *buffer.add(name_len as usize) = 0; // Null terminate
            
            (*symbolic_link_name).buffer = buffer;
            (*symbolic_link_name).length = name_len * 2; // bytes
            (*symbolic_link_name).maximum_length = buffer_size as u16;
        } else {
            // Copy to provided buffer
            let copy_len = (name_len as u16).min((*symbolic_link_name).maximum_length / 2 - 1);
            core::ptr::copy_nonoverlapping(
                (*entry).symbolic_link_name.as_ptr(),
                (*symbolic_link_name).buffer,
                copy_len as usize,
            );
            (*symbolic_link_name).length = copy_len * 2;
        }
        
        STATUS_SUCCESS
    }
}

/// Builds the symbolic link name for a device interface
///
/// Format: \??\{XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX}#instance
/// or:     \??\{XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX}#refstring#instance
fn build_interface_symbolic_link_name(
    guid: &GUID,
    instance: u32,
    reference_string: Option<&[u16]>,
    buffer: &mut [u16; MAX_INTERFACE_NAME_LENGTH],
) -> u16 {
    let mut pos = 0usize;
    
    // Prefix: \??\
    let prefix: &[u16] = &[0x5C, 0x3F, 0x3F, 0x5C]; // \??\
    for &c in prefix {
        if pos < buffer.len() {
            buffer[pos] = c;
            pos += 1;
        }
    }
    
    // GUID in format {XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX}
    buffer[pos] = 0x7B; pos += 1; // {
    
    // data1 (8 hex digits)
    pos = write_hex_u32(buffer, pos, guid.data1);
    buffer[pos] = 0x2D; pos += 1; // -
    
    // data2 (4 hex digits)
    pos = write_hex_u16(buffer, pos, guid.data2);
    buffer[pos] = 0x2D; pos += 1; // -
    
    // data3 (4 hex digits)
    pos = write_hex_u16(buffer, pos, guid.data3);
    buffer[pos] = 0x2D; pos += 1; // -
    
    // data4[0..2] (4 hex digits)
    pos = write_hex_u8(buffer, pos, guid.data4[0]);
    pos = write_hex_u8(buffer, pos, guid.data4[1]);
    buffer[pos] = 0x2D; pos += 1; // -
    
    // data4[2..8] (12 hex digits)
    for i in 2..8 {
        pos = write_hex_u8(buffer, pos, guid.data4[i]);
    }
    
    buffer[pos] = 0x7D; pos += 1; // }
    buffer[pos] = 0x23; pos += 1; // #
    
    // Reference string if provided
    if let Some(ref_str) = reference_string {
        for &c in ref_str {
            if pos < buffer.len() - 16 && c != 0 {
                buffer[pos] = c;
                pos += 1;
            }
        }
        buffer[pos] = 0x23; pos += 1; // #
    }
    
    // Instance number as decimal
    pos = write_decimal_u32(buffer, pos, instance);
    
    pos as u16
}

// Helper functions for building strings
fn write_hex_u32(buffer: &mut [u16], mut pos: usize, value: u32) -> usize {
    const HEX: &[u16] = &[
        0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37,
        0x38, 0x39, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46
    ];
    for i in (0..8).rev() {
        let nibble = ((value >> (i * 4)) & 0xF) as usize;
        if pos < buffer.len() {
            buffer[pos] = HEX[nibble];
            pos += 1;
        }
    }
    pos
}

fn write_hex_u16(buffer: &mut [u16], mut pos: usize, value: u16) -> usize {
    const HEX: &[u16] = &[
        0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37,
        0x38, 0x39, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46
    ];
    for i in (0..4).rev() {
        let nibble = ((value >> (i * 4)) & 0xF) as usize;
        if pos < buffer.len() {
            buffer[pos] = HEX[nibble];
            pos += 1;
        }
    }
    pos
}

fn write_hex_u8(buffer: &mut [u16], mut pos: usize, value: u8) -> usize {
    const HEX: &[u16] = &[
        0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37,
        0x38, 0x39, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46
    ];
    let high = (value >> 4) as usize;
    let low = (value & 0xF) as usize;
    if pos < buffer.len() { buffer[pos] = HEX[high]; pos += 1; }
    if pos < buffer.len() { buffer[pos] = HEX[low]; pos += 1; }
    pos
}

/// Convert u16 to uppercase (ASCII only)
fn to_upper_u16(c: u16) -> u16 {
    if c >= 0x61 && c <= 0x7A { // 'a' to 'z'
        c - 0x20
    } else {
        c
    }
}

fn write_decimal_u32(buffer: &mut [u16], mut pos: usize, value: u32) -> usize {
    if value == 0 {
        if pos < buffer.len() {
            buffer[pos] = 0x30; // '0'
            pos += 1;
        }
        return pos;
    }
    
    // Build digits in reverse
    let mut digits = [0u16; 10];
    let mut count = 0;
    let mut v = value;
    while v > 0 {
        digits[count] = 0x30 + (v % 10) as u16;
        v /= 10;
        count += 1;
    }
    
    // Write in correct order
    while count > 0 {
        count -= 1;
        if pos < buffer.len() {
            buffer[pos] = digits[count];
            pos += 1;
        }
    }
    
    pos
}

// =============================================================================
// IoSetDeviceInterfaceState
// =============================================================================

/// IoSetDeviceInterfaceState - включает или выключает device interface
///
/// Когда интерфейс включается, создаётся symbolic link.
/// Когда выключается, symbolic link удаляется.
///
/// # Arguments
/// * `symbolic_link_name` - имя интерфейса (возвращённое IoRegisterDeviceInterface)
/// * `enable` - TRUE для включения, FALSE для выключения
///
/// # Returns
/// STATUS_SUCCESS или код ошибки
///
/// # Reference
/// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/nf-wdm-iosetdeviceinterfacestate
pub unsafe fn io_set_device_interface_state(
    symbolic_link_name: *const UNICODE_STRING,
    enable: bool,
) -> NTSTATUS {
    unsafe {
        if symbolic_link_name.is_null() {
            return STATUS_INVALID_PARAMETER;
        }
        
        let name = &*symbolic_link_name;
        if name.buffer.is_null() || name.length == 0 {
            return STATUS_INVALID_PARAMETER;
        }
        
        // Find the interface entry
        let _irql = crate::ke::spinlock::ke_acquire_spin_lock(&IOP_DEVICE_INTERFACE_LOCK);
        
        let entry = find_interface_entry_by_name(name);
        
        if entry.is_null() {
            crate::ke::spinlock::ke_release_spin_lock(&IOP_DEVICE_INTERFACE_LOCK, _irql);
            return STATUS_OBJECT_NAME_NOT_FOUND;
        }
        
        let current_state = (*entry).state;
        
        crate::ke::spinlock::ke_release_spin_lock(&IOP_DEVICE_INTERFACE_LOCK, _irql);
        
        if enable {
            if current_state == DeviceInterfaceState::Enabled {
                return STATUS_SUCCESS; // Already enabled
            }
            
            // Create symbolic link
            // The symbolic link points to the PDO's device name
            
            // Get device name from PDO
            let pdo = (*entry).physical_device_object;
            
            #[cfg(feature = "trace-io")]
            crate::kd::dbg_print("[INTERFACE] Enabling interface, getting PDO name...\n");
            
            let device_name = get_device_object_name(pdo);
            
            if device_name.buffer.is_null() {
                #[cfg(feature = "trace-io")]
                crate::kd::dbg_print("[INTERFACE] ERROR: PDO has no name (anonymous device)\n");
                return STATUS_OBJECT_NAME_NOT_FOUND;
            }
            
            #[cfg(feature = "trace-io")]
            crate::dbg_print!("[INTERFACE] PDO name length: {}\n", device_name.length);
            
            // Create the symbolic link
            // symbolic_link_name is in format "\??\{GUID}#instance"
            // We need to extract the leaf name (after \??\) for io_create_symbolic_link
            // which uses \?? as root_directory
            let link_name_leaf = extract_leaf_from_dos_path(symbolic_link_name);
            
            let status = super::device::io_create_symbolic_link(
                &link_name_leaf,
                &device_name,
            );
            
            #[cfg(feature = "trace-io")]
            crate::dbg_print!("[INTERFACE] io_create_symbolic_link returned: 0x{:08X}\n", status as u32);
            
            // Free the name buffer
            ex_free_pool_with_tag(
                device_name.buffer as PVOID,
                u32::from_le_bytes(*b"nmOI"),
            );
            
            if status == STATUS_SUCCESS {
                let interface_guid: GUID;
                
                {
                    let _irql = crate::ke::spinlock::ke_acquire_spin_lock(&IOP_DEVICE_INTERFACE_LOCK);
                    (*entry).state = DeviceInterfaceState::Enabled;
                    interface_guid = (*entry).interface_class_guid;
                    crate::ke::spinlock::ke_release_spin_lock(&IOP_DEVICE_INTERFACE_LOCK, _irql);
                }
                
                #[cfg(feature = "trace-io")]
                crate::kd::dbg_print("[INTERFACE] Interface ENABLED, sending arrival notification\n");
                
                // Send PnP notification for interface arrival
                iop_notify_device_interface_change(&interface_guid, symbolic_link_name, true);
            }
            
            status
        } else {
            if current_state == DeviceInterfaceState::Disabled {
                return STATUS_SUCCESS; // Already disabled
            }
            
            // Get interface GUID before deleting
            let interface_guid = (*entry).interface_class_guid;
            
            // Delete symbolic link
            let status = super::device::io_delete_symbolic_link(symbolic_link_name);
            
            if status == STATUS_SUCCESS || status == STATUS_OBJECT_NAME_NOT_FOUND {
                {
                    let _irql = crate::ke::spinlock::ke_acquire_spin_lock(&IOP_DEVICE_INTERFACE_LOCK);
                    (*entry).state = DeviceInterfaceState::Disabled;
                    crate::ke::spinlock::ke_release_spin_lock(&IOP_DEVICE_INTERFACE_LOCK, _irql);
                }
                
                #[cfg(feature = "trace-io")]
                crate::kd::dbg_print("[INTERFACE] Interface DISABLED, sending removal notification\n");
                
                // Send PnP notification for interface removal
                iop_notify_device_interface_change(&interface_guid, symbolic_link_name, false);
            }
            
            if status == STATUS_OBJECT_NAME_NOT_FOUND {
                return STATUS_SUCCESS; // Link already gone is OK for disable
            }
            
            status
        }
    }
}

/// Finds an interface entry by symbolic link name
unsafe fn find_interface_entry_by_name(name: &UNICODE_STRING) -> PDEVICE_INTERFACE_ENTRY {
    unsafe {
        let char_count = (name.length / 2) as usize;
        
        let mut current = IOP_DEVICE_INTERFACE_LIST.flink;
        while current != &mut IOP_DEVICE_INTERFACE_LIST as *mut LIST_ENTRY {
            let entry = get_interface_entry_from_list(current);
            
            // Compare names
            let entry_len = (*entry).symbolic_link_name_length as usize;
            if entry_len == char_count {
                let mut match_found = true;
                for i in 0..char_count {
                    let c1 = *name.buffer.add(i);
                    let c2 = (*entry).symbolic_link_name[i];
                    // Case-insensitive compare
                    if to_upper_u16(c1) != to_upper_u16(c2) {
                        match_found = false;
                        break;
                    }
                }
                if match_found {
                    return entry;
                }
            }
            
            current = (*current).flink;
        }
        
        core::ptr::null_mut()
    }
}

/// Extracts the leaf name from a DOS path like "\??\{GUID}#instance"
/// Returns a UNICODE_STRING pointing to the part after "\??\"
unsafe fn extract_leaf_from_dos_path(full_path: *const UNICODE_STRING) -> UNICODE_STRING {
    if full_path.is_null() {
        return UNICODE_STRING::new();
    }
    
    let path = &*full_path;
    if path.buffer.is_null() || path.length < 8 {
        return path.clone();
    }
    
    let chars = core::slice::from_raw_parts(path.buffer, (path.length / 2) as usize);
    
    // Look for "\??\" prefix (4 chars: \, ?, ?, \)
    // 0x5C = '\', 0x3F = '?'
    if chars.len() >= 4 
        && chars[0] == 0x5C  // '\'
        && chars[1] == 0x3F  // '?'
        && chars[2] == 0x3F  // '?'
        && chars[3] == 0x5C  // '\'
    {
        // Return string starting after "\??\"
        UNICODE_STRING {
            length: path.length - 8,  // subtract 4 chars * 2 bytes
            maximum_length: path.maximum_length - 8,
            buffer: path.buffer.add(4),  // skip 4 chars
        }
    } else {
        // No prefix found, return as is
        path.clone()
    }
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Gets the device object name from DEVICE_OBJECT
///
/// Traverses the object directory tree to construct the full path name.
/// Caller must free the returned string buffer using ex_free_pool_with_tag.
unsafe fn get_device_object_name(device: PDEVICE_OBJECT) -> UNICODE_STRING {
    use crate::ob::header::{object_to_object_header, object_header_to_name_info};
    use crate::ex::pool::{ex_allocate_pool_with_tag, POOL_TYPE};

    // 1. Calculate required length
    let mut total_len = 0usize;
    let mut current_obj = device as PVOID;
    
    // Check if device itself is valid
    if current_obj.is_null() {
         return UNICODE_STRING::new();
    }

    loop {
        let header = object_to_object_header(current_obj);
        let name_info = object_header_to_name_info(header);
        
        if name_info.is_null() {
             // Anonymous object in chain - cannot construct full path
             // Unless it's the root directory which might be anonymous in some implementations?
             // But usually Root is named in OB or we treat it as special.
             // For now, if we hit anonymous, we stop.
             break;
        }
        
        let name = &(*name_info).name;
        if name.length > 0 {
            // Add length of name + separator '\'
            total_len += (name.length as usize) + 2;
        }
        
        let dir = (*name_info).directory;
        if dir.is_null() {
            // Reached root (or object without parent)
            break;
        }
        current_obj = dir;
    }

    if total_len == 0 {
        return UNICODE_STRING::new();
    }
    
    // Allocate buffer (including null terminator)
    let buffer_size = total_len + 2;
    let buffer = ex_allocate_pool_with_tag(
        POOL_TYPE::PagedPool,
        buffer_size,
        u32::from_le_bytes(*b"nmOI"),
    ) as *mut u16;
    
    if buffer.is_null() {
        return UNICODE_STRING::new();
    }
    
    // 2. Fill buffer backwards
    let mut pos = total_len / 2; // Position in WCHARs
    current_obj = device as PVOID;
    
    // Null terminate
    *buffer.add(pos) = 0;
    
    loop {
        let header = object_to_object_header(current_obj);
        let name_info = object_header_to_name_info(header);
        
        if name_info.is_null() {
             break;
        }
        
        let name = &(*name_info).name;
        if name.length > 0 {
            let name_chars = name.length as usize / 2;
            pos -= name_chars;
            
            // Copy name
            core::ptr::copy_nonoverlapping(
                name.buffer,
                buffer.add(pos),
                name_chars,
            );
            
            // Add separator
            pos -= 1;
            *buffer.add(pos) = b'\\' as u16;
        }
        
        let dir = (*name_info).directory;
        if dir.is_null() {
            break;
        }
        current_obj = dir;
    }
    
    UNICODE_STRING {
        length: total_len as u16,
        maximum_length: buffer_size as u16,
        buffer,
    }
}

// =============================================================================
// IoGetDeviceInterfaces (optional helper)
// =============================================================================

/// Counts registered interfaces for a given GUID
pub unsafe fn io_count_device_interfaces(interface_class_guid: *const GUID) -> u32 {
    unsafe {
        if interface_class_guid.is_null() {
            return 0;
        }
        
        let _irql = crate::ke::spinlock::ke_acquire_spin_lock(&IOP_DEVICE_INTERFACE_LOCK);
        
        let mut count = 0u32;
        let mut current = IOP_DEVICE_INTERFACE_LIST.flink;
        
        while current != &mut IOP_DEVICE_INTERFACE_LIST as *mut LIST_ENTRY {
            let entry = get_interface_entry_from_list(current);
            
            if (*entry).interface_class_guid == *interface_class_guid {
                count += 1;
            }
            
            current = (*current).flink;
        }
        
        crate::ke::spinlock::ke_release_spin_lock(&IOP_DEVICE_INTERFACE_LOCK, _irql);
        
        count
    }
}

// =============================================================================
// IoRegisterPlugPlayNotification
// =============================================================================

/// IoRegisterPlugPlayNotification - регистрирует callback для PnP notifications
///
/// Позволяет драйверу получать уведомления о PnP событиях,
/// таких как появление/исчезновение device interfaces.
///
/// # Arguments
/// * `event_category` - категория событий (EventCategoryDeviceInterfaceChange)
/// * `event_category_flags` - флаги (PNPNOTIFY_DEVICE_INTERFACE_INCLUDE_EXISTING_INTERFACES)
/// * `event_category_data` - указатель на GUID интерфейса (для EventCategoryDeviceInterfaceChange)
/// * `driver_object` - объект драйвера
/// * `callback_routine` - функция callback
/// * `context` - контекст для callback
/// * `notification_entry` - [out] handle регистрации
///
/// # Returns
/// STATUS_SUCCESS или код ошибки
///
/// # Reference
/// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/nf-wdm-ioregisterplugplaynotification
pub unsafe fn io_register_plug_play_notification(
    event_category: IO_NOTIFICATION_EVENT_CATEGORY,
    event_category_flags: ULONG,
    event_category_data: PVOID,
    driver_object: PDRIVER_OBJECT,
    callback_routine: PDRIVER_NOTIFICATION_CALLBACK_ROUTINE,
    context: PVOID,
    notification_entry: *mut PVOID,
) -> NTSTATUS {
    unsafe {
        #[cfg(feature = "trace-io")]
        crate::kd::dbg_print("[INTERFACE] IoRegisterPlugPlayNotification called\n");
        
        // Validate parameters
        if callback_routine.is_none() || notification_entry.is_null() {
            #[cfg(feature = "trace-io")]
            crate::kd::dbg_print("[INTERFACE] Invalid parameter\n");
            return STATUS_INVALID_PARAMETER;
        }
        
        // Currently only support EventCategoryDeviceInterfaceChange
        if event_category != IO_NOTIFICATION_EVENT_CATEGORY::EventCategoryDeviceInterfaceChange {
            #[cfg(feature = "trace-io")]
            crate::kd::dbg_print("[INTERFACE] Unsupported event category\n");
            return STATUS_NOT_SUPPORTED;
        }
        
        // event_category_data must be a GUID pointer for device interface change
        if event_category_data.is_null() {
            #[cfg(feature = "trace-io")]
            crate::kd::dbg_print("[INTERFACE] Missing interface GUID\n");
            return STATUS_INVALID_PARAMETER;
        }
        
        let interface_guid = event_category_data as *const GUID;
        
        // Ensure initialized
        if !IOP_DEVICE_INTERFACE_INITIALIZED {
            iop_init_device_interfaces();
        }
        
        // Allocate notification entry
        let entry = ex_allocate_pool_with_tag(
            POOL_TYPE::NonPagedPool,
            core::mem::size_of::<PNP_NOTIFICATION_ENTRY>(),
            PNP_NOTIFICATION_TAG,
        ) as PPNP_NOTIFICATION_ENTRY;
        
        if entry.is_null() {
            #[cfg(feature = "trace-io")]
            crate::kd::dbg_print("[INTERFACE] Failed to allocate notification entry\n");
            return STATUS_INSUFFICIENT_RESOURCES;
        }
        
        // Initialize entry
        core::ptr::write(entry, PNP_NOTIFICATION_ENTRY::new());
        (*entry).event_category = event_category;
        (*entry).interface_class_guid = *interface_guid;
        (*entry).callback_routine = callback_routine;
        (*entry).context = context;
        (*entry).driver_object = driver_object;
        (*entry).ref_count = 1;
        (*entry).unregistered = false;
        
        // Add to global list
        let _irql = crate::ke::spinlock::ke_acquire_spin_lock(&IOP_PNP_NOTIFICATION_LOCK);
        LIST_ENTRY::insert_tail(&mut IOP_PNP_NOTIFICATION_LIST, &mut (*entry).list_entry);
        crate::ke::spinlock::ke_release_spin_lock(&IOP_PNP_NOTIFICATION_LOCK, _irql);
        
        // Return handle
        *notification_entry = entry as PVOID;
        
        #[cfg(feature = "trace-io")]
        {
            crate::kd::dbg_print("[INTERFACE] Notification registered, entry=0x");
            crate::kd::dbg_print_hex(entry as u64);
            crate::kd::dbg_print("\n");
        }
        
        // If PNPNOTIFY_DEVICE_INTERFACE_INCLUDE_EXISTING_INTERFACES is set,
        // call callback for all existing enabled interfaces of this class
        const PNPNOTIFY_DEVICE_INTERFACE_INCLUDE_EXISTING_INTERFACES: ULONG = 1;
        
        if (event_category_flags & PNPNOTIFY_DEVICE_INTERFACE_INCLUDE_EXISTING_INTERFACES) != 0 {
            #[cfg(feature = "trace-io")]
            crate::kd::dbg_print("[INTERFACE] Sending notifications for existing interfaces\n");
            
            notify_existing_interfaces(entry);
        }
        
        STATUS_SUCCESS
    }
}

/// Sends notifications for all existing enabled interfaces matching the registration
unsafe fn notify_existing_interfaces(registration: PPNP_NOTIFICATION_ENTRY) {
    unsafe {
        let _irql = crate::ke::spinlock::ke_acquire_spin_lock(&IOP_DEVICE_INTERFACE_LOCK);
        
        let target_guid = (*registration).interface_class_guid;
        let mut current = IOP_DEVICE_INTERFACE_LIST.flink;
        
        while current != &mut IOP_DEVICE_INTERFACE_LIST as *mut LIST_ENTRY {
            let interface_entry = get_interface_entry_from_list(current);
            
            // Check if this interface matches and is enabled
            if (*interface_entry).interface_class_guid == target_guid 
               && (*interface_entry).state == DeviceInterfaceState::Enabled 
            {
                #[cfg(feature = "trace-io")]
                {
                    crate::kd::dbg_print("[INTERFACE] Found existing enabled interface, calling callback\n");
                }
                
                // Build notification structure
                let mut sym_link_name = UNICODE_STRING {
                    length: (*interface_entry).symbolic_link_name_length * 2,
                    maximum_length: (*interface_entry).symbolic_link_name_length * 2 + 2,
                    buffer: (*interface_entry).symbolic_link_name.as_ptr() as *mut u16,
                };
                
                let notification = DEVICE_INTERFACE_CHANGE_NOTIFICATION {
                    version: 1,
                    size: core::mem::size_of::<DEVICE_INTERFACE_CHANGE_NOTIFICATION>() as u16,
                    event: GUID_DEVICE_INTERFACE_ARRIVAL,
                    interface_class_guid: target_guid,
                    symbolic_link_name: &mut sym_link_name,
                };
                
                // Release lock before calling callback
                crate::ke::spinlock::ke_release_spin_lock(&IOP_DEVICE_INTERFACE_LOCK, _irql);
                
                // Call callback
                if let Some(callback) = (*registration).callback_routine {
                    let _status = callback(
                        &notification as *const _ as PVOID,
                        (*registration).context,
                    );
                    
                    #[cfg(feature = "trace-io")]
                    {
                        crate::kd::dbg_print("[INTERFACE] Callback returned status=0x");
                        crate::kd::dbg_print_hex(_status as u64);
                        crate::kd::dbg_print("\n");
                    }
                }
                
                // Reacquire lock
                let _irql = crate::ke::spinlock::ke_acquire_spin_lock(&IOP_DEVICE_INTERFACE_LOCK);
            }
            
            current = (*current).flink;
        }
        
        crate::ke::spinlock::ke_release_spin_lock(&IOP_DEVICE_INTERFACE_LOCK, _irql);
    }
}

/// IoUnregisterPlugPlayNotification - отменяет регистрацию notification callback
///
/// # Arguments
/// * `notification_entry` - handle регистрации (возвращённый IoRegisterPlugPlayNotification)
///
/// # Returns
/// STATUS_SUCCESS или код ошибки
pub unsafe fn io_unregister_plug_play_notification(
    notification_entry: PVOID,
) -> NTSTATUS {
    unsafe {
        #[cfg(feature = "trace-io")]
        {
            crate::kd::dbg_print("[INTERFACE] IoUnregisterPlugPlayNotification entry=0x");
            crate::kd::dbg_print_hex(notification_entry as u64);
            crate::kd::dbg_print("\n");
        }
        
        if notification_entry.is_null() {
            return STATUS_INVALID_PARAMETER;
        }
        
        let entry = notification_entry as PPNP_NOTIFICATION_ENTRY;
        
        // Mark as unregistered
        let _irql = crate::ke::spinlock::ke_acquire_spin_lock(&IOP_PNP_NOTIFICATION_LOCK);
        
        if (*entry).unregistered {
            crate::ke::spinlock::ke_release_spin_lock(&IOP_PNP_NOTIFICATION_LOCK, _irql);
            return STATUS_SUCCESS;
        }
        
        (*entry).unregistered = true;
        
        // Remove from list
        LIST_ENTRY::remove_entry(&mut (*entry).list_entry);
        
        crate::ke::spinlock::ke_release_spin_lock(&IOP_PNP_NOTIFICATION_LOCK, _irql);
        
        // Decrement reference count and free if zero
        (*entry).ref_count = (*entry).ref_count.saturating_sub(1);
        if (*entry).ref_count == 0 {
            ex_free_pool_with_tag(entry as PVOID, PNP_NOTIFICATION_TAG);
        }
        
        STATUS_SUCCESS
    }
}

/// IoUnregisterPlugPlayNotificationEx - отменяет регистрацию с ожиданием callbacks
///
/// Более безопасная версия, которая ждёт завершения всех активных callbacks
pub unsafe fn io_unregister_plug_play_notification_ex(
    notification_entry: PVOID,
) -> NTSTATUS {
    // For now, same as basic version
    // Full implementation would wait for pending callbacks
    io_unregister_plug_play_notification(notification_entry)
}

// =============================================================================
// Internal: Notification dispatch
// =============================================================================

/// Calls all registered callbacks for device interface change
/// Called from IoSetDeviceInterfaceState
pub(crate) unsafe fn iop_notify_device_interface_change(
    interface_class_guid: &GUID,
    symbolic_link_name: *const UNICODE_STRING,
    is_arrival: bool,
) {
    unsafe {
        #[cfg(feature = "trace-io")]
        {
            crate::kd::dbg_print("[INTERFACE] iop_notify_device_interface_change: ");
            if is_arrival {
                crate::kd::dbg_print("ARRIVAL\n");
            } else {
                crate::kd::dbg_print("REMOVAL\n");
            }
        }
        
        // Build notification structure on stack
        let mut sym_link_copy = *symbolic_link_name;
        
        let notification = DEVICE_INTERFACE_CHANGE_NOTIFICATION {
            version: 1,
            size: core::mem::size_of::<DEVICE_INTERFACE_CHANGE_NOTIFICATION>() as u16,
            event: if is_arrival { GUID_DEVICE_INTERFACE_ARRIVAL } else { GUID_DEVICE_INTERFACE_REMOVAL },
            interface_class_guid: *interface_class_guid,
            symbolic_link_name: &mut sym_link_copy,
        };
        
        // Find all matching registrations and call their callbacks
        let _irql = crate::ke::spinlock::ke_acquire_spin_lock(&IOP_PNP_NOTIFICATION_LOCK);
        
        let mut current = IOP_PNP_NOTIFICATION_LIST.flink;
        let mut callbacks_to_call: [(PDRIVER_NOTIFICATION_CALLBACK_ROUTINE, PVOID); 16] = [(None, core::ptr::null_mut()); 16];
        let mut callback_count = 0usize;
        
        while current != &mut IOP_PNP_NOTIFICATION_LIST as *mut LIST_ENTRY {
            let entry = get_notification_entry_from_list(current);
            
            // Check if this registration matches
            if !(*entry).unregistered 
               && (*entry).event_category == IO_NOTIFICATION_EVENT_CATEGORY::EventCategoryDeviceInterfaceChange
               && (*entry).interface_class_guid == *interface_class_guid 
            {
                // Collect callback info (we'll call outside the lock)
                if callback_count < 16 {
                    callbacks_to_call[callback_count] = ((*entry).callback_routine, (*entry).context);
                    (*entry).ref_count += 1; // Prevent free during callback
                    callback_count += 1;
                }
            }
            
            current = (*current).flink;
        }
        
        crate::ke::spinlock::ke_release_spin_lock(&IOP_PNP_NOTIFICATION_LOCK, _irql);
        
        #[cfg(feature = "trace-io")]
        {
            crate::kd::dbg_print("[INTERFACE] Found ");
            crate::kd::dbg_print_hex(callback_count as u64);
            crate::kd::dbg_print(" registered callbacks\n");
        }
        
        // Call callbacks outside the lock
        for i in 0..callback_count {
            if let Some(callback) = callbacks_to_call[i].0 {
                #[cfg(feature = "trace-io")]
                {
                    crate::kd::dbg_print("[INTERFACE] Calling callback ");
                    crate::kd::dbg_print_hex(i as u64);
                    crate::kd::dbg_print("\n");
                }
                
                let _status = callback(
                    &notification as *const _ as PVOID,
                    callbacks_to_call[i].1,
                );
                
                #[cfg(feature = "trace-io")]
                {
                    crate::kd::dbg_print("[INTERFACE] Callback returned 0x");
                    crate::kd::dbg_print_hex(_status as u64);
                    crate::kd::dbg_print("\n");
                }
            }
        }
        
        // Decrement reference counts
        // (In a full implementation, we'd track which entries we incremented)
    }
}

/// Helper to get notification entry from list
#[inline]
unsafe fn get_notification_entry_from_list(list_ptr: *mut LIST_ENTRY) -> PPNP_NOTIFICATION_ENTRY {
    let offset = core::mem::offset_of!(PNP_NOTIFICATION_ENTRY, list_entry);
    (list_ptr as usize - offset) as PPNP_NOTIFICATION_ENTRY
}

// =============================================================================
// Well-known Device Interface GUIDs (NT 6.1)
// =============================================================================

/// GUID_DEVINTERFACE_DISK
/// {53F56307-B6BF-11D0-94F2-00A0C91EFB8B}
pub const GUID_DEVINTERFACE_DISK: GUID = GUID {
    data1: 0x53F56307,
    data2: 0xB6BF,
    data3: 0x11D0,
    data4: [0x94, 0xF2, 0x00, 0xA0, 0xC9, 0x1E, 0xFB, 0x8B],
};

/// GUID_DEVINTERFACE_VOLUME
/// {53F5630D-B6BF-11D0-94F2-00A0C91EFB8B}
pub const GUID_DEVINTERFACE_VOLUME: GUID = GUID {
    data1: 0x53F5630D,
    data2: 0xB6BF,
    data3: 0x11D0,
    data4: [0x94, 0xF2, 0x00, 0xA0, 0xC9, 0x1E, 0xFB, 0x8B],
};

/// GUID_DEVINTERFACE_PARTITION
/// {53F5630A-B6BF-11D0-94F2-00A0C91EFB8B}
pub const GUID_DEVINTERFACE_PARTITION: GUID = GUID {
    data1: 0x53F5630A,
    data2: 0xB6BF,
    data3: 0x11D0,
    data4: [0x94, 0xF2, 0x00, 0xA0, 0xC9, 0x1E, 0xFB, 0x8B],
};

/// GUID_DEVINTERFACE_STORAGEPORT
/// {2ACCFE60-C130-11D2-B082-00A0C91EFB8B}
pub const GUID_DEVINTERFACE_STORAGEPORT: GUID = GUID {
    data1: 0x2ACCFE60,
    data2: 0xC130,
    data3: 0x11D2,
    data4: [0xB0, 0x82, 0x00, 0xA0, 0xC9, 0x1E, 0xFB, 0x8B],
};

// =============================================================================
// Internal helper
// =============================================================================

/// Helper function to get containing structure from list entry
#[inline]
unsafe fn get_interface_entry_from_list(list_ptr: *mut LIST_ENTRY) -> PDEVICE_INTERFACE_ENTRY {
    let offset = core::mem::offset_of!(DEVICE_INTERFACE_ENTRY, list_entry);
    (list_ptr as usize - offset) as PDEVICE_INTERFACE_ENTRY
}

