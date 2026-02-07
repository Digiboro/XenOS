//! PCI Bus Enumeration
//!
//! Перечисление устройств на PCI шине и создание PDO.

use crate::config::{
    PciDeviceInfo, PCI_INTERRUPT_LINE, PCI_INTERRUPT_PIN,
    pci_device_present, pci_is_multi_function, pci_is_bridge,
    pci_read_config_byte, pci_read_all_bars, PCI_SECONDARY_BUS,
};
use crate::types::*;
use core::ptr;

/// Максимальное количество устройств на шине
pub const PCI_MAX_DEVICES: u8 = 32;
/// Максимальное количество функций на устройство
pub const PCI_MAX_FUNCTIONS: u8 = 8;

// =============================================================================
// Bus Enumeration
// =============================================================================

/// Callback для enumeration
pub type EnumerateCallback = unsafe fn(info: &PciDeviceInfo, context: PVOID) -> bool;

/// Enumerate все устройства на указанной шине
///
/// # Arguments
/// * `bus` - номер шины для enumeration
/// * `callback` - вызывается для каждого найденного устройства
/// * `context` - передаётся в callback
///
/// # Returns
/// Количество найденных устройств
pub unsafe fn pci_enumerate_bus(
    bus: u8,
    callback: EnumerateCallback,
    context: PVOID,
) -> u32 {
    let mut count = 0u32;

    for device in 0..PCI_MAX_DEVICES {
        // Проверяем function 0
        if !pci_device_present(bus, device, 0) {
            continue;
        }

        // Получаем информацию о function 0
        if let Some(info) = PciDeviceInfo::read(bus, device, 0) {
            if !callback(&info, context) {
                return count;
            }
            count += 1;
        }

        // Проверяем остальные функции если multi-function
        if pci_is_multi_function(bus, device) {
            for function in 1..PCI_MAX_FUNCTIONS {
                if let Some(info) = PciDeviceInfo::read(bus, device, function) {
                    if !callback(&info, context) {
                        return count;
                    }
                    count += 1;
                }
            }
        }
    }

    count
}

/// Enumerate все шины рекурсивно (для PCI-to-PCI bridges)
///
/// Начинает с bus 0 и рекурсивно сканирует secondary buses.
pub unsafe fn pci_enumerate_all(
    callback: EnumerateCallback,
    context: PVOID,
) -> u32 {
    pci_enumerate_bus_recursive(0, callback, context)
}

unsafe fn pci_enumerate_bus_recursive(
    bus: u8,
    callback: EnumerateCallback,
    context: PVOID,
) -> u32 {
    let mut count = 0u32;

    for device in 0..PCI_MAX_DEVICES {
        if !pci_device_present(bus, device, 0) {
            continue;
        }

        let max_functions = if pci_is_multi_function(bus, device) {
            PCI_MAX_FUNCTIONS
        } else {
            1
        };

        for function in 0..max_functions {
            if let Some(info) = PciDeviceInfo::read(bus, device, function) {
                if !callback(&info, context) {
                    return count;
                }
                count += 1;

                // Если это bridge, enumerate secondary bus
                if pci_is_bridge(bus, device, function) {
                    let secondary_bus = pci_read_config_byte(
                        bus,
                        device,
                        function,
                        PCI_SECONDARY_BUS,
                    );
                    if secondary_bus != 0 && secondary_bus != 0xFF {
                        count += pci_enumerate_bus_recursive(secondary_bus, callback, context);
                    }
                }
            }
        }
    }

    count
}

// =============================================================================
// PDO Creation
// =============================================================================

/// Создаёт PDO для PCI устройства
pub unsafe fn pci_create_pdo(
    driver_object: PDRIVER_OBJECT,
    parent_fdo: PDEVICE_OBJECT,
    info: &PciDeviceInfo,
) -> PDEVICE_OBJECT {
    let mut pdo: PDEVICE_OBJECT = ptr::null_mut();

    let status = IoCreateDevice(
        driver_object,
        core::mem::size_of::<PCI_PDO_EXTENSION>() as u32,
        ptr::null(), // Без имени
        FILE_DEVICE_BUS_EXTENDER,
        0,
        0,
        &mut pdo,
    );

    if status != STATUS_SUCCESS || pdo.is_null() {
        return ptr::null_mut();
    }

    // Инициализируем PDO extension
    let pdo_ext = (*pdo).device_extension as *mut PCI_PDO_EXTENSION;
    ptr::write_bytes(pdo_ext, 0, core::mem::size_of::<PCI_PDO_EXTENSION>());

    (*pdo_ext).common.is_fdo = 0;
    (*pdo_ext).common.self_device = pdo;
    (*pdo_ext).parent_fdo = parent_fdo;

    // PCI location
    (*pdo_ext).bus_number = info.bus;
    (*pdo_ext).device_number = info.device;
    (*pdo_ext).function_number = info.function;

    // PCI IDs
    (*pdo_ext).vendor_id = info.vendor_id;
    (*pdo_ext).device_id = info.device_id;
    (*pdo_ext).subsystem_vendor_id = info.subsystem_vendor_id;
    (*pdo_ext).subsystem_id = info.subsystem_id;

    // Class codes
    (*pdo_ext).base_class = info.base_class;
    (*pdo_ext).sub_class = info.sub_class;
    (*pdo_ext).programming_interface = info.prog_if;
    (*pdo_ext).revision_id = info.revision_id;
    (*pdo_ext).header_type = info.header_type;

    (*pdo_ext).present = 1;
    (*pdo_ext).reported = 0;
    (*pdo_ext).started = 0;

    // Читаем interrupt info
    (*pdo_ext).interrupt_line = pci_read_config_byte(info.bus, info.device, info.function, PCI_INTERRUPT_LINE);
    (*pdo_ext).interrupt_pin = pci_read_config_byte(info.bus, info.device, info.function, PCI_INTERRUPT_PIN);

    // Читаем BAR информацию
    let bars = pci_read_all_bars(info.bus, info.device, info.function, info.header_type);
    let mut num_bars = 0u8;
    
    for (i, bar) in bars.iter().enumerate() {
        if bar.size > 0 {
            (*pdo_ext).bars[i].index = bar.index;
            (*pdo_ext).bars[i].base_address = bar.base_address;
            (*pdo_ext).bars[i].size = bar.size;
            (*pdo_ext).bars[i].is_memory = if bar.is_memory { 1 } else { 0 };
            (*pdo_ext).bars[i].is_64bit = if bar.is_64bit { 1 } else { 0 };
            (*pdo_ext).bars[i].is_prefetchable = if bar.is_prefetchable { 1 } else { 0 };
            (*pdo_ext).bars[i].assigned = 0; // Будет назначен в START_DEVICE
            num_bars = (i + 1) as u8;
        }
    }
    (*pdo_ext).num_bars = num_bars;

    // Устройство готово
    (*pdo).flags &= !DO_DEVICE_INITIALIZING;

    pdo
}

// =============================================================================
// Device ID Generation
// =============================================================================

/// Формирует Device ID строку для PCI устройства
///
/// Формат: PCI\VEN_xxxx&DEV_yyyy&SUBSYS_zzzzzzzz&REV_rr
pub fn format_device_id(info: &PciDeviceInfo, buffer: &mut [u16]) -> usize {
    // PCI\VEN_xxxx&DEV_yyyy
    let prefix = "PCI\\VEN_";
    let mut pos = 0;

    // Copy prefix
    for c in prefix.bytes() {
        if pos >= buffer.len() {
            return pos;
        }
        buffer[pos] = c as u16;
        pos += 1;
    }

    // Vendor ID (4 hex digits)
    pos = append_hex_u16(buffer, pos, info.vendor_id);

    // &DEV_
    for c in "&DEV_".bytes() {
        if pos >= buffer.len() {
            return pos;
        }
        buffer[pos] = c as u16;
        pos += 1;
    }

    // Device ID (4 hex digits)
    pos = append_hex_u16(buffer, pos, info.device_id);

    // Null terminator
    if pos < buffer.len() {
        buffer[pos] = 0;
        pos += 1;
    }

    pos
}

/// Формирует Hardware IDs (multi-sz) для PCI устройства
///
/// Включает:
/// - PCI\VEN_xxxx&DEV_yyyy&SUBSYS_zzzzzzzz&REV_rr
/// - PCI\VEN_xxxx&DEV_yyyy&SUBSYS_zzzzzzzz
/// - PCI\VEN_xxxx&DEV_yyyy&REV_rr
/// - PCI\VEN_xxxx&DEV_yyyy
/// - PCI\VEN_xxxx&CC_ccss
/// - PCI\VEN_xxxx&CC_ccsspp
/// - PCI\CC_ccsspp
/// - PCI\CC_ccss
pub fn format_hardware_ids(info: &PciDeviceInfo, buffer: &mut [u16]) -> usize {
    let mut pos = 0;

    // Simplified: just the main ID for now
    // PCI\VEN_xxxx&DEV_yyyy
    let ids = [
        // Most specific first
        format_args_to_buffer(
            buffer,
            pos,
            "PCI\\VEN_",
            info.vendor_id,
            "&DEV_",
            info.device_id,
        ),
    ];

    for end_pos in ids {
        if end_pos > pos && end_pos < buffer.len() {
            pos = end_pos;
            // Add null between strings
            if pos < buffer.len() {
                buffer[pos] = 0;
                pos += 1;
            }
        }
    }

    // Double null terminator for multi-sz
    if pos < buffer.len() {
        buffer[pos] = 0;
        pos += 1;
    }

    pos
}

/// Формирует Instance ID для PCI устройства
///
/// Формат: BB&DD&FF (bus, device, function)
pub fn format_instance_id(info: &PciDeviceInfo, buffer: &mut [u16]) -> usize {
    let mut pos = 0;

    // Bus (2 hex digits)
    pos = append_hex_u8(buffer, pos, info.bus);

    if pos < buffer.len() {
        buffer[pos] = '&' as u16;
        pos += 1;
    }

    // Device (2 hex digits)
    pos = append_hex_u8(buffer, pos, info.device);

    if pos < buffer.len() {
        buffer[pos] = '&' as u16;
        pos += 1;
    }

    // Function (2 hex digits)
    pos = append_hex_u8(buffer, pos, info.function);

    // Null terminator
    if pos < buffer.len() {
        buffer[pos] = 0;
        pos += 1;
    }

    pos
}

// =============================================================================
// Helper Functions
// =============================================================================

fn append_hex_u16(buffer: &mut [u16], mut pos: usize, value: u16) -> usize {
    const HEX_CHARS: &[u8] = b"0123456789ABCDEF";

    for i in (0..4).rev() {
        if pos >= buffer.len() {
            break;
        }
        let nibble = ((value >> (i * 4)) & 0xF) as usize;
        buffer[pos] = HEX_CHARS[nibble] as u16;
        pos += 1;
    }

    pos
}

fn append_hex_u8(buffer: &mut [u16], mut pos: usize, value: u8) -> usize {
    const HEX_CHARS: &[u8] = b"0123456789ABCDEF";

    for i in (0..2).rev() {
        if pos >= buffer.len() {
            break;
        }
        let nibble = ((value >> (i * 4)) & 0xF) as usize;
        buffer[pos] = HEX_CHARS[nibble] as u16;
        pos += 1;
    }

    pos
}

fn format_args_to_buffer(
    buffer: &mut [u16],
    mut pos: usize,
    prefix: &str,
    vendor: u16,
    mid: &str,
    device: u16,
) -> usize {
    // prefix
    for c in prefix.bytes() {
        if pos >= buffer.len() {
            return pos;
        }
        buffer[pos] = c as u16;
        pos += 1;
    }

    // vendor (4 hex)
    pos = append_hex_u16(buffer, pos, vendor);

    // mid
    for c in mid.bytes() {
        if pos >= buffer.len() {
            return pos;
        }
        buffer[pos] = c as u16;
        pos += 1;
    }

    // device (4 hex)
    pos = append_hex_u16(buffer, pos, device);

    pos
}

/// Формирует Compatible IDs (multi-sz) для PCI устройства
///
/// Включает:
/// - PCI\VEN_xxxx&CC_ccsspp (vendor + class + subclass + progif)
/// - PCI\VEN_xxxx&CC_ccss (vendor + class + subclass)
/// - PCI\CC_ccsspp (class + subclass + progif)
/// - PCI\CC_ccss (class + subclass)
pub fn format_compatible_ids(info: &PciDeviceInfo, buffer: &mut [u16]) -> usize {
    let mut pos = 0;
    
    // PCI\VEN_xxxx&CC_ccsspp
    pos = append_str(buffer, pos, "PCI\\VEN_");
    pos = append_hex_u16(buffer, pos, info.vendor_id);
    pos = append_str(buffer, pos, "&CC_");
    pos = append_hex_u8(buffer, pos, info.base_class);
    pos = append_hex_u8(buffer, pos, info.sub_class);
    pos = append_hex_u8(buffer, pos, info.prog_if);
    if pos < buffer.len() {
        buffer[pos] = 0;
        pos += 1;
    }
    
    // PCI\VEN_xxxx&CC_ccss
    pos = append_str(buffer, pos, "PCI\\VEN_");
    pos = append_hex_u16(buffer, pos, info.vendor_id);
    pos = append_str(buffer, pos, "&CC_");
    pos = append_hex_u8(buffer, pos, info.base_class);
    pos = append_hex_u8(buffer, pos, info.sub_class);
    if pos < buffer.len() {
        buffer[pos] = 0;
        pos += 1;
    }
    
    // PCI\CC_ccsspp
    pos = append_str(buffer, pos, "PCI\\CC_");
    pos = append_hex_u8(buffer, pos, info.base_class);
    pos = append_hex_u8(buffer, pos, info.sub_class);
    pos = append_hex_u8(buffer, pos, info.prog_if);
    if pos < buffer.len() {
        buffer[pos] = 0;
        pos += 1;
    }
    
    // PCI\CC_ccss
    pos = append_str(buffer, pos, "PCI\\CC_");
    pos = append_hex_u8(buffer, pos, info.base_class);
    pos = append_hex_u8(buffer, pos, info.sub_class);
    if pos < buffer.len() {
        buffer[pos] = 0;
        pos += 1;
    }
    
    // Double null terminator for multi-sz
    if pos < buffer.len() {
        buffer[pos] = 0;
        pos += 1;
    }
    
    pos
}

fn append_str(buffer: &mut [u16], mut pos: usize, s: &str) -> usize {
    for c in s.bytes() {
        if pos >= buffer.len() {
            return pos;
        }
        buffer[pos] = c as u16;
        pos += 1;
    }
    pos
}

