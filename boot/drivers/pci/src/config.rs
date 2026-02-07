//! PCI Configuration Space Access
//!
//! Методы доступа к PCI конфигурационному пространству:
//! - Legacy I/O (порты 0xCF8/0xCFC) для PCI 2.x
//! - ECAM (memory-mapped) для PCIe
//!
//! # PCI Configuration Space Layout
//!
//! ```text
//! Offset  Size  Description
//! ------  ----  -----------
//! 0x00    2     Vendor ID
//! 0x02    2     Device ID
//! 0x04    2     Command
//! 0x06    2     Status
//! 0x08    1     Revision ID
//! 0x09    1     Programming Interface
//! 0x0A    1     Sub Class
//! 0x0B    1     Base Class
//! 0x0C    1     Cache Line Size
//! 0x0D    1     Latency Timer
//! 0x0E    1     Header Type
//! 0x0F    1     BIST
//! 0x10-   6*4   Base Address Registers (BAR0-BAR5) - Type 0
//! 0x2C    2     Subsystem Vendor ID
//! 0x2E    2     Subsystem ID
//! 0x3C    1     Interrupt Line
//! 0x3D    1     Interrupt Pin
//! ```

use crate::types::*;
use core::arch::asm;

// =============================================================================
// I/O Ports
// =============================================================================

const PCI_CONFIG_ADDRESS: u16 = 0x0CF8;
const PCI_CONFIG_DATA: u16 = 0x0CFC;

// =============================================================================
// PCI Config Register Offsets
// =============================================================================

pub const PCI_VENDOR_ID: u8 = 0x00;
pub const PCI_DEVICE_ID: u8 = 0x02;
pub const PCI_COMMAND: u8 = 0x04;
pub const PCI_STATUS: u8 = 0x06;
pub const PCI_REVISION_ID: u8 = 0x08;
pub const PCI_PROG_IF: u8 = 0x09;
pub const PCI_SUBCLASS: u8 = 0x0A;
pub const PCI_CLASS: u8 = 0x0B;
pub const PCI_CACHE_LINE_SIZE: u8 = 0x0C;
pub const PCI_LATENCY_TIMER: u8 = 0x0D;
pub const PCI_HEADER_TYPE: u8 = 0x0E;
pub const PCI_BIST: u8 = 0x0F;
pub const PCI_BAR0: u8 = 0x10;
pub const PCI_SUBSYSTEM_VENDOR_ID: u8 = 0x2C;
pub const PCI_SUBSYSTEM_ID: u8 = 0x2E;
pub const PCI_INTERRUPT_LINE: u8 = 0x3C;
pub const PCI_INTERRUPT_PIN: u8 = 0x3D;

// Type 1 header (PCI-to-PCI bridge)
pub const PCI_PRIMARY_BUS: u8 = 0x18;
pub const PCI_SECONDARY_BUS: u8 = 0x19;
pub const PCI_SUBORDINATE_BUS: u8 = 0x1A;

// =============================================================================
// Header Type Values
// =============================================================================

pub const PCI_HEADER_TYPE_DEVICE: u8 = 0x00;
pub const PCI_HEADER_TYPE_BRIDGE: u8 = 0x01;
pub const PCI_HEADER_TYPE_CARDBUS: u8 = 0x02;
pub const PCI_HEADER_TYPE_MULTI_FUNCTION: u8 = 0x80;

// =============================================================================
// PCI Command Register bits
// =============================================================================

pub const PCI_COMMAND_IO: u16 = 0x0001;
pub const PCI_COMMAND_MEMORY: u16 = 0x0002;
pub const PCI_COMMAND_BUS_MASTER: u16 = 0x0004;
pub const PCI_COMMAND_SPECIAL: u16 = 0x0008;
pub const PCI_COMMAND_INVALIDATE: u16 = 0x0010;
pub const PCI_COMMAND_VGA_PALETTE: u16 = 0x0020;
pub const PCI_COMMAND_PARITY: u16 = 0x0040;
pub const PCI_COMMAND_WAIT: u16 = 0x0080;
pub const PCI_COMMAND_SERR: u16 = 0x0100;
pub const PCI_COMMAND_FAST_BACK: u16 = 0x0200;
pub const PCI_COMMAND_INTX_DISABLE: u16 = 0x0400;

// =============================================================================
// Special Values
// =============================================================================

/// Invalid vendor ID (device not present)
pub const PCI_INVALID_VENDOR_ID: u16 = 0xFFFF;

// =============================================================================
// I/O Port Access (inline assembly)
// =============================================================================

#[inline]
unsafe fn outl(port: u16, value: u32) {
    asm!(
        "out dx, eax",
        in("dx") port,
        in("eax") value,
        options(nostack, preserves_flags)
    );
}

#[inline]
unsafe fn inl(port: u16) -> u32 {
    let value: u32;
    asm!(
        "in eax, dx",
        in("dx") port,
        out("eax") value,
        options(nostack, preserves_flags)
    );
    value
}

#[inline]
unsafe fn outw(port: u16, value: u16) {
    asm!(
        "out dx, ax",
        in("dx") port,
        in("ax") value,
        options(nostack, preserves_flags)
    );
}

#[inline]
unsafe fn inw(port: u16) -> u16 {
    let value: u16;
    asm!(
        "in ax, dx",
        in("dx") port,
        out("ax") value,
        options(nostack, preserves_flags)
    );
    value
}

#[inline]
unsafe fn outb(port: u16, value: u8) {
    asm!(
        "out dx, al",
        in("dx") port,
        in("al") value,
        options(nostack, preserves_flags)
    );
}

#[inline]
unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    asm!(
        "in al, dx",
        in("dx") port,
        out("al") value,
        options(nostack, preserves_flags)
    );
    value
}

// =============================================================================
// PCI Config Address
// =============================================================================

/// Формирует адрес для PCI Configuration Mechanism 1
///
/// ```text
/// Bit 31     - Enable bit
/// Bits 30:24 - Reserved
/// Bits 23:16 - Bus Number
/// Bits 15:11 - Device Number
/// Bits 10:8  - Function Number
/// Bits 7:0   - Register Offset (must be DWORD aligned for CONFIG_DATA access)
/// ```
#[inline]
fn make_pci_address(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    0x80000000 // Enable bit
        | ((bus as u32) << 16)
        | ((device as u32) << 11)
        | ((function as u32) << 8)
        | ((offset as u32) & 0xFC) // DWORD aligned
}

// =============================================================================
// Legacy PCI Config Access (I/O ports)
// =============================================================================

/// Читает 8-bit значение из PCI config space
pub unsafe fn pci_read_config_byte(bus: u8, device: u8, function: u8, offset: u8) -> u8 {
    let address = make_pci_address(bus, device, function, offset);
    outl(PCI_CONFIG_ADDRESS, address);

    // Читаем DWORD и извлекаем нужный байт
    let data = inl(PCI_CONFIG_DATA);
    let shift = (offset & 3) * 8;
    ((data >> shift) & 0xFF) as u8
}

/// Читает 16-bit значение из PCI config space
pub unsafe fn pci_read_config_word(bus: u8, device: u8, function: u8, offset: u8) -> u16 {
    let address = make_pci_address(bus, device, function, offset);
    outl(PCI_CONFIG_ADDRESS, address);

    // Читаем DWORD и извлекаем нужное слово
    let data = inl(PCI_CONFIG_DATA);
    let shift = (offset & 2) * 8;
    ((data >> shift) & 0xFFFF) as u16
}

/// Читает 32-bit значение из PCI config space
pub unsafe fn pci_read_config_dword(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    let address = make_pci_address(bus, device, function, offset);
    outl(PCI_CONFIG_ADDRESS, address);
    inl(PCI_CONFIG_DATA)
}

/// Записывает 8-bit значение в PCI config space
pub unsafe fn pci_write_config_byte(bus: u8, device: u8, function: u8, offset: u8, value: u8) {
    let address = make_pci_address(bus, device, function, offset);
    outl(PCI_CONFIG_ADDRESS, address);

    // Read-modify-write
    let mut data = inl(PCI_CONFIG_DATA);
    let shift = (offset & 3) * 8;
    let mask = !(0xFF << shift);
    data = (data & mask) | ((value as u32) << shift);
    outl(PCI_CONFIG_DATA, data);
}

/// Записывает 16-bit значение в PCI config space
pub unsafe fn pci_write_config_word(bus: u8, device: u8, function: u8, offset: u8, value: u16) {
    let address = make_pci_address(bus, device, function, offset);
    outl(PCI_CONFIG_ADDRESS, address);

    // Read-modify-write
    let mut data = inl(PCI_CONFIG_DATA);
    let shift = (offset & 2) * 8;
    let mask = !(0xFFFF << shift);
    data = (data & mask) | ((value as u32) << shift);
    outl(PCI_CONFIG_DATA, data);
}

/// Записывает 32-bit значение в PCI config space
pub unsafe fn pci_write_config_dword(bus: u8, device: u8, function: u8, offset: u8, value: u32) {
    let address = make_pci_address(bus, device, function, offset);
    outl(PCI_CONFIG_ADDRESS, address);
    outl(PCI_CONFIG_DATA, value);
}

// =============================================================================
// Device Detection
// =============================================================================

/// Проверяет наличие устройства по заданному адресу
pub unsafe fn pci_device_present(bus: u8, device: u8, function: u8) -> bool {
    let vendor = pci_read_config_word(bus, device, function, PCI_VENDOR_ID);
    vendor != PCI_INVALID_VENDOR_ID
}

/// Проверяет, является ли устройство multi-function
pub unsafe fn pci_is_multi_function(bus: u8, device: u8) -> bool {
    let header_type = pci_read_config_byte(bus, device, 0, PCI_HEADER_TYPE);
    (header_type & PCI_HEADER_TYPE_MULTI_FUNCTION) != 0
}

/// Возвращает header type устройства (без multi-function bit)
pub unsafe fn pci_get_header_type(bus: u8, device: u8, function: u8) -> u8 {
    let header_type = pci_read_config_byte(bus, device, function, PCI_HEADER_TYPE);
    header_type & 0x7F
}

/// Проверяет, является ли устройство PCI-to-PCI bridge
pub unsafe fn pci_is_bridge(bus: u8, device: u8, function: u8) -> bool {
    pci_get_header_type(bus, device, function) == PCI_HEADER_TYPE_BRIDGE
}

// =============================================================================
// Device Info
// =============================================================================

/// Информация о PCI устройстве
#[derive(Clone, Copy, Debug, Default)]
pub struct PciDeviceInfo {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub base_class: u8,
    pub sub_class: u8,
    pub prog_if: u8,
    pub revision_id: u8,
    pub header_type: u8,
    pub subsystem_vendor_id: u16,
    pub subsystem_id: u16,
}

impl PciDeviceInfo {
    /// Читает информацию о PCI устройстве
    pub unsafe fn read(bus: u8, device: u8, function: u8) -> Option<Self> {
        if !pci_device_present(bus, device, function) {
            return None;
        }

        let vendor_id = pci_read_config_word(bus, device, function, PCI_VENDOR_ID);
        let device_id = pci_read_config_word(bus, device, function, PCI_DEVICE_ID);
        let revision_id = pci_read_config_byte(bus, device, function, PCI_REVISION_ID);
        let prog_if = pci_read_config_byte(bus, device, function, PCI_PROG_IF);
        let sub_class = pci_read_config_byte(bus, device, function, PCI_SUBCLASS);
        let base_class = pci_read_config_byte(bus, device, function, PCI_CLASS);
        let header_type = pci_read_config_byte(bus, device, function, PCI_HEADER_TYPE) & 0x7F;

        let (subsystem_vendor_id, subsystem_id) = if header_type == PCI_HEADER_TYPE_DEVICE {
            (
                pci_read_config_word(bus, device, function, PCI_SUBSYSTEM_VENDOR_ID),
                pci_read_config_word(bus, device, function, PCI_SUBSYSTEM_ID),
            )
        } else {
            (0, 0)
        };

        Some(PciDeviceInfo {
            bus,
            device,
            function,
            vendor_id,
            device_id,
            base_class,
            sub_class,
            prog_if,
            revision_id,
            header_type,
            subsystem_vendor_id,
            subsystem_id,
        })
    }
}

// =============================================================================
// BAR (Base Address Register) Operations
// =============================================================================

/// BAR type bits
pub const PCI_BAR_TYPE_MASK: u32 = 0x00000001;
pub const PCI_BAR_TYPE_IO: u32 = 0x00000001;
pub const PCI_BAR_TYPE_MEMORY: u32 = 0x00000000;

/// Memory BAR bits
pub const PCI_BAR_MEMORY_TYPE_MASK: u32 = 0x00000006;
pub const PCI_BAR_MEMORY_32BIT: u32 = 0x00000000;
pub const PCI_BAR_MEMORY_1MB: u32 = 0x00000002; // Below 1MB (obsolete)
pub const PCI_BAR_MEMORY_64BIT: u32 = 0x00000004;
pub const PCI_BAR_MEMORY_PREFETCH: u32 = 0x00000008;

/// Маска для получения адреса из memory BAR
pub const PCI_BAR_MEMORY_ADDR_MASK: u32 = 0xFFFFFFF0;
/// Маска для получения адреса из I/O BAR  
pub const PCI_BAR_IO_ADDR_MASK: u32 = 0xFFFFFFFC;

/// Информация о PCI BAR
#[derive(Debug, Clone, Copy, Default)]
pub struct PciBarInfo {
    /// Индекс BAR (0-5)
    pub index: u8,
    /// Это memory BAR (иначе I/O)
    pub is_memory: bool,
    /// Это 64-bit BAR
    pub is_64bit: bool,
    /// Prefetchable (только для memory)
    pub is_prefetchable: bool,
    /// Базовый адрес
    pub base_address: u64,
    /// Размер BAR
    pub size: u64,
}

impl PciBarInfo {
    /// Возвращает true если BAR не используется (размер 0)
    pub fn is_unused(&self) -> bool {
        self.size == 0
    }
}

/// Количество BAR'ов для Type 0 header (device)
pub const PCI_TYPE0_NUM_BARS: usize = 6;
/// Количество BAR'ов для Type 1 header (bridge)  
pub const PCI_TYPE1_NUM_BARS: usize = 2;

/// Определяет размер и тип BAR
///
/// Используется стандартная процедура BAR sizing:
/// 1. Сохраняем текущее значение BAR
/// 2. Записываем 0xFFFFFFFF
/// 3. Читаем маску (незаписываемые биты остаются 0)
/// 4. Вычисляем размер из маски
/// 5. Восстанавливаем оригинальное значение
pub unsafe fn pci_probe_bar(
    bus: u8,
    device: u8,
    function: u8,
    bar_index: u8,
) -> PciBarInfo {
    let bar_offset = PCI_BAR0 + (bar_index * 4);
    
    // Отключаем декодирование I/O и memory во время probe
    let cmd = pci_read_config_word(bus, device, function, PCI_COMMAND);
    pci_write_config_word(
        bus, device, function, PCI_COMMAND,
        cmd & !(PCI_COMMAND_IO | PCI_COMMAND_MEMORY)
    );
    
    // Читаем текущее значение BAR
    let original = pci_read_config_dword(bus, device, function, bar_offset);
    
    // Определяем тип BAR
    let is_io = (original & PCI_BAR_TYPE_MASK) == PCI_BAR_TYPE_IO;
    
    if is_io {
        // I/O BAR
        pci_write_config_dword(bus, device, function, bar_offset, 0xFFFFFFFF);
        let mask = pci_read_config_dword(bus, device, function, bar_offset);
        pci_write_config_dword(bus, device, function, bar_offset, original);
        
        // Восстанавливаем command
        pci_write_config_word(bus, device, function, PCI_COMMAND, cmd);
        
        let size_mask = mask & PCI_BAR_IO_ADDR_MASK;
        if size_mask == 0 || size_mask == 0xFFFFFFFC {
            return PciBarInfo::default();
        }
        
        let size = ((!size_mask) + 1) & 0xFFFF;
        
        PciBarInfo {
            index: bar_index,
            is_memory: false,
            is_64bit: false,
            is_prefetchable: false,
            base_address: (original & PCI_BAR_IO_ADDR_MASK) as u64,
            size: size as u64,
        }
    } else {
        // Memory BAR
        let is_64bit = (original & PCI_BAR_MEMORY_TYPE_MASK) == PCI_BAR_MEMORY_64BIT;
        let is_prefetchable = (original & PCI_BAR_MEMORY_PREFETCH) != 0;
        
        // Probe low 32 bits
        pci_write_config_dword(bus, device, function, bar_offset, 0xFFFFFFFF);
        let mask_low = pci_read_config_dword(bus, device, function, bar_offset);
        pci_write_config_dword(bus, device, function, bar_offset, original);
        
        let (base_address, size) = if is_64bit {
            // 64-bit BAR: probe high 32 bits too
            let bar_offset_high = bar_offset + 4;
            let original_high = pci_read_config_dword(bus, device, function, bar_offset_high);
            
            pci_write_config_dword(bus, device, function, bar_offset_high, 0xFFFFFFFF);
            let mask_high = pci_read_config_dword(bus, device, function, bar_offset_high);
            pci_write_config_dword(bus, device, function, bar_offset_high, original_high);
            
            let base = ((original_high as u64) << 32) | ((original & PCI_BAR_MEMORY_ADDR_MASK) as u64);
            let mask = ((mask_high as u64) << 32) | ((mask_low & PCI_BAR_MEMORY_ADDR_MASK) as u64);
            
            let size = if mask == 0 || mask == 0xFFFFFFFF_FFFFFFF0 {
                0
            } else {
                (!mask).wrapping_add(1)
            };
            
            (base, size)
        } else {
            // 32-bit BAR
            let base = (original & PCI_BAR_MEMORY_ADDR_MASK) as u64;
            let size_mask = mask_low & PCI_BAR_MEMORY_ADDR_MASK;
            
            let size = if size_mask == 0 || size_mask == 0xFFFFFFF0 {
                0
            } else {
                ((!size_mask) + 1) as u64
            };
            
            (base, size)
        };
        
        // Восстанавливаем command
        pci_write_config_word(bus, device, function, PCI_COMMAND, cmd);
        
        if size == 0 {
            return PciBarInfo::default();
        }
        
        PciBarInfo {
            index: bar_index,
            is_memory: true,
            is_64bit,
            is_prefetchable,
            base_address,
            size,
        }
    }
}

/// Читает все BAR'ы устройства
pub unsafe fn pci_read_all_bars(
    bus: u8,
    device: u8,
    function: u8,
    header_type: u8,
) -> [PciBarInfo; 6] {
    let mut bars = [PciBarInfo::default(); 6];
    
    let num_bars = if header_type == PCI_HEADER_TYPE_BRIDGE {
        PCI_TYPE1_NUM_BARS
    } else {
        PCI_TYPE0_NUM_BARS
    };
    
    let mut i = 0;
    while i < num_bars {
        let bar = pci_probe_bar(bus, device, function, i as u8);
        bars[i] = bar;
        
        // 64-bit BAR занимает 2 слота
        if bar.is_64bit {
            i += 2;
        } else {
            i += 1;
        }
    }
    
    bars
}

/// Программирует BAR с указанным адресом
pub unsafe fn pci_program_bar(
    bus: u8,
    device: u8,
    function: u8,
    bar_index: u8,
    address: u64,
    is_64bit: bool,
) {
    let bar_offset = PCI_BAR0 + (bar_index * 4);
    
    // Записываем low 32 bits (сохраняя флаги типа)
    let original = pci_read_config_dword(bus, device, function, bar_offset);
    let type_bits = original & 0x0F; // Сохраняем биты типа
    let new_value = ((address as u32) & 0xFFFFFFF0) | type_bits;
    pci_write_config_dword(bus, device, function, bar_offset, new_value);
    
    // Для 64-bit BAR записываем high 32 bits
    if is_64bit {
        let bar_offset_high = bar_offset + 4;
        pci_write_config_dword(bus, device, function, bar_offset_high, (address >> 32) as u32);
    }
}

/// Включает I/O space, Memory space и Bus Master для устройства
pub unsafe fn pci_enable_device(bus: u8, device: u8, function: u8) {
    let cmd = pci_read_config_word(bus, device, function, PCI_COMMAND);
    pci_write_config_word(
        bus, device, function, PCI_COMMAND,
        cmd | PCI_COMMAND_IO | PCI_COMMAND_MEMORY | PCI_COMMAND_BUS_MASTER
    );
}

/// Отключает I/O space и Memory space для устройства
pub unsafe fn pci_disable_device(bus: u8, device: u8, function: u8) {
    let cmd = pci_read_config_word(bus, device, function, PCI_COMMAND);
    pci_write_config_word(
        bus, device, function, PCI_COMMAND,
        cmd & !(PCI_COMMAND_IO | PCI_COMMAND_MEMORY | PCI_COMMAND_BUS_MASTER)
    );
}

