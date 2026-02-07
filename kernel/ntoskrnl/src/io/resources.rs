//! Resource Management Structures
//!
//! Структуры для описания аппаратных ресурсов устройств:
//! - CM_RESOURCE_LIST — выделенные ресурсы (передаются в IRP_MN_START_DEVICE)
//! - IO_RESOURCE_REQUIREMENTS_LIST — требования к ресурсам (QUERY_RESOURCE_REQUIREMENTS)
//!
//! Соответствует Windows NT 6.1 (Win7) для совместимости с драйверами.
//!
//! Источники:
//! - ReactOS: sdk/include/xdk/wdm.h, sdk/include/xdk/iotypes.h
//! - MSDN: CM_RESOURCE_LIST structure

use crate::nt::{PVOID, UCHAR, ULONG, USHORT};

/// Физический адрес - для ресурсов используем простой u64
pub type PHYSICAL_ADDRESS = u64;

// =============================================================================
// Interface Type
// =============================================================================

/// Тип интерфейса шины
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum INTERFACE_TYPE {
    InterfaceTypeUndefined = -1,
    Internal = 0,
    Isa = 1,
    Eisa = 2,
    MicroChannel = 3,
    TurboChannel = 4,
    PCIBus = 5,
    VMEBus = 6,
    NuBus = 7,
    PCMCIABus = 8,
    CBus = 9,
    MPIBus = 10,
    MPSABus = 11,
    ProcessorInternal = 12,
    InternalPowerBus = 13,
    PNPISABus = 14,
    PNPBus = 15,
    Vmcs = 16,
    ACPIBus = 17,
    MaximumInterfaceType = 18,
}

// =============================================================================
// CM_RESOURCE_TYPE - типы ресурсов
// =============================================================================

/// Тип ресурса (CmResourceType*)
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmResourceType {
    Null = 0,
    Port = 1,
    Interrupt = 2,
    Memory = 3,
    Dma = 4,
    DeviceSpecific = 5,
    BusNumber = 6,
    MemoryLarge = 7,
    ConfigData = 128,
    DevicePrivate = 129,
}

/// CM_RESOURCE_MEMORY_LARGE subtypes (in Flags)
pub const CM_RESOURCE_MEMORY_LARGE_40: USHORT = 0x0200;
pub const CM_RESOURCE_MEMORY_LARGE_48: USHORT = 0x0400;
pub const CM_RESOURCE_MEMORY_LARGE_64: USHORT = 0x0800;

// =============================================================================
// Resource Flags
// =============================================================================

// Memory flags
pub const CM_RESOURCE_MEMORY_READ_WRITE: USHORT = 0x0000;
pub const CM_RESOURCE_MEMORY_READ_ONLY: USHORT = 0x0001;
pub const CM_RESOURCE_MEMORY_WRITE_ONLY: USHORT = 0x0002;
pub const CM_RESOURCE_MEMORY_PREFETCHABLE: USHORT = 0x0004;
pub const CM_RESOURCE_MEMORY_COMBINEDWRITE: USHORT = 0x0008;
pub const CM_RESOURCE_MEMORY_24: USHORT = 0x0010;
pub const CM_RESOURCE_MEMORY_CACHEABLE: USHORT = 0x0020;
pub const CM_RESOURCE_MEMORY_WINDOW_DECODE: USHORT = 0x0040;
pub const CM_RESOURCE_MEMORY_BAR: USHORT = 0x0080;

// Port flags
pub const CM_RESOURCE_PORT_MEMORY: USHORT = 0x0000;
pub const CM_RESOURCE_PORT_IO: USHORT = 0x0001;
pub const CM_RESOURCE_PORT_10_BIT_DECODE: USHORT = 0x0004;
pub const CM_RESOURCE_PORT_12_BIT_DECODE: USHORT = 0x0008;
pub const CM_RESOURCE_PORT_16_BIT_DECODE: USHORT = 0x0010;
pub const CM_RESOURCE_PORT_POSITIVE_DECODE: USHORT = 0x0020;
pub const CM_RESOURCE_PORT_PASSIVE_DECODE: USHORT = 0x0040;
pub const CM_RESOURCE_PORT_WINDOW_DECODE: USHORT = 0x0080;
pub const CM_RESOURCE_PORT_BAR: USHORT = 0x0100;

// Interrupt flags
pub const CM_RESOURCE_INTERRUPT_LEVEL_SENSITIVE: USHORT = 0x0000;
pub const CM_RESOURCE_INTERRUPT_LATCHED: USHORT = 0x0001;
pub const CM_RESOURCE_INTERRUPT_MESSAGE: USHORT = 0x0002;
pub const CM_RESOURCE_INTERRUPT_POLICY_INCLUDED: USHORT = 0x0004;

// DMA flags
pub const CM_RESOURCE_DMA_8: USHORT = 0x0000;
pub const CM_RESOURCE_DMA_16: USHORT = 0x0001;
pub const CM_RESOURCE_DMA_32: USHORT = 0x0002;
pub const CM_RESOURCE_DMA_8_AND_16: USHORT = 0x0004;
pub const CM_RESOURCE_DMA_BUS_MASTER: USHORT = 0x0008;
pub const CM_RESOURCE_DMA_TYPE_A: USHORT = 0x0010;
pub const CM_RESOURCE_DMA_TYPE_B: USHORT = 0x0020;
pub const CM_RESOURCE_DMA_TYPE_F: USHORT = 0x0040;

// =============================================================================
// CM_PARTIAL_RESOURCE_DESCRIPTOR
// =============================================================================

/// Дескриптор для CmResourceTypePort
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CM_PORT_RESOURCE {
    pub start: PHYSICAL_ADDRESS,
    pub length: ULONG,
}

/// Дескриптор для CmResourceTypeInterrupt
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CM_INTERRUPT_RESOURCE {
    pub level: ULONG,
    pub vector: ULONG,
    pub affinity: usize, // KAFFINITY
}

/// Дескриптор для CmResourceTypeMemory
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CM_MEMORY_RESOURCE {
    pub start: PHYSICAL_ADDRESS,
    pub length: ULONG,
}

/// Дескриптор для CmResourceTypeMemoryLarge
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CM_MEMORY_LARGE_RESOURCE {
    pub start: PHYSICAL_ADDRESS,
    pub length_40: ULONG, // Shifted by 8, 16, or 32 depending on subtype
}

/// Дескриптор для CmResourceTypeDma
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CM_DMA_RESOURCE {
    pub channel: ULONG,
    pub port: ULONG,
    pub reserved1: ULONG,
}

/// Дескриптор для CmResourceTypeBusNumber
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CM_BUS_NUMBER_RESOURCE {
    pub start: ULONG,
    pub length: ULONG,
    pub reserved: ULONG,
}

/// Дескриптор для CmResourceTypeDevicePrivate
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CM_DEVICE_PRIVATE_RESOURCE {
    pub data: [ULONG; 3],
}

/// Union для различных типов ресурсов
#[repr(C)]
#[derive(Clone, Copy)]
pub union CM_RESOURCE_UNION {
    pub port: CM_PORT_RESOURCE,
    pub interrupt: CM_INTERRUPT_RESOURCE,
    pub memory: CM_MEMORY_RESOURCE,
    pub memory_large: CM_MEMORY_LARGE_RESOURCE,
    pub dma: CM_DMA_RESOURCE,
    pub bus_number: CM_BUS_NUMBER_RESOURCE,
    pub device_private: CM_DEVICE_PRIVATE_RESOURCE,
    pub raw: [ULONG; 3],
}

/// CM_PARTIAL_RESOURCE_DESCRIPTOR — описание одного ресурса
///
/// Используется в CM_PARTIAL_RESOURCE_LIST.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CM_PARTIAL_RESOURCE_DESCRIPTOR {
    /// Тип ресурса (CmResourceType*)
    pub r#type: UCHAR,
    /// Доля ресурса (CmResourceShareUndetermined, Shared, DeviceExclusive, DriverExclusive)
    pub share_disposition: UCHAR,
    /// Флаги, зависящие от типа ресурса
    pub flags: USHORT,
    /// Данные ресурса
    pub u: CM_RESOURCE_UNION,
}

impl CM_PARTIAL_RESOURCE_DESCRIPTOR {
    pub const fn new() -> Self {
        Self {
            r#type: 0,
            share_disposition: 0,
            flags: 0,
            u: CM_RESOURCE_UNION { raw: [0; 3] },
        }
    }

    /// Возвращает true если это memory resource
    pub fn is_memory(&self) -> bool {
        self.r#type == CmResourceType::Memory as u8 || self.r#type == CmResourceType::MemoryLarge as u8
    }

    /// Возвращает true если это port resource
    pub fn is_port(&self) -> bool {
        self.r#type == CmResourceType::Port as u8
    }

    /// Возвращает true если это interrupt resource
    pub fn is_interrupt(&self) -> bool {
        self.r#type == CmResourceType::Interrupt as u8
    }

    /// Возвращает физический адрес для memory/port ресурса
    pub fn get_physical_address(&self) -> Option<PHYSICAL_ADDRESS> {
        unsafe {
            match self.r#type {
                t if t == CmResourceType::Memory as u8 => Some(self.u.memory.start),
                t if t == CmResourceType::MemoryLarge as u8 => Some(self.u.memory_large.start),
                t if t == CmResourceType::Port as u8 => Some(self.u.port.start),
                _ => None,
            }
        }
    }

    /// Возвращает длину для memory/port ресурса
    pub fn get_length(&self) -> Option<u64> {
        unsafe {
            match self.r#type {
                t if t == CmResourceType::Memory as u8 => Some(self.u.memory.length as u64),
                t if t == CmResourceType::MemoryLarge as u8 => {
                    // Length depends on subtype (shifted)
                    let raw_len = self.u.memory_large.length_40;
                    let shift = if (self.flags & CM_RESOURCE_MEMORY_LARGE_40) != 0 {
                        8
                    } else if (self.flags & CM_RESOURCE_MEMORY_LARGE_48) != 0 {
                        16
                    } else if (self.flags & CM_RESOURCE_MEMORY_LARGE_64) != 0 {
                        32
                    } else {
                        0
                    };
                    Some((raw_len as u64) << shift)
                }
                t if t == CmResourceType::Port as u8 => Some(self.u.port.length as u64),
                _ => None,
            }
        }
    }
}

// =============================================================================
// CM_PARTIAL_RESOURCE_LIST
// =============================================================================

/// CM_PARTIAL_RESOURCE_LIST — список частичных дескрипторов ресурсов
///
/// Содержит version, revision и массив дескрипторов.
#[repr(C)]
pub struct CM_PARTIAL_RESOURCE_LIST {
    /// Версия (обычно 1)
    pub version: USHORT,
    /// Ревизия (обычно 1)
    pub revision: USHORT,
    /// Количество дескрипторов
    pub count: ULONG,
    /// Массив дескрипторов (variable length)
    pub partial_descriptors: [CM_PARTIAL_RESOURCE_DESCRIPTOR; 1],
}

impl CM_PARTIAL_RESOURCE_LIST {
    /// Возвращает итератор по дескрипторам
    pub unsafe fn descriptors(&self, count: usize) -> &[CM_PARTIAL_RESOURCE_DESCRIPTOR] {
        core::slice::from_raw_parts(self.partial_descriptors.as_ptr(), count)
    }

    /// Ищет memory ресурс с указанным индексом (0-based)
    pub unsafe fn find_memory(&self, index: usize) -> Option<&CM_PARTIAL_RESOURCE_DESCRIPTOR> {
        let mut found = 0usize;
        for desc in self.descriptors(self.count as usize) {
            if desc.is_memory() {
                if found == index {
                    return Some(desc);
                }
                found += 1;
            }
        }
        None
    }

    /// Ищет port ресурс с указанным индексом (0-based)
    pub unsafe fn find_port(&self, index: usize) -> Option<&CM_PARTIAL_RESOURCE_DESCRIPTOR> {
        let mut found = 0usize;
        for desc in self.descriptors(self.count as usize) {
            if desc.is_port() {
                if found == index {
                    return Some(desc);
                }
                found += 1;
            }
        }
        None
    }

    /// Ищет interrupt ресурс с указанным индексом (0-based)
    pub unsafe fn find_interrupt(&self, index: usize) -> Option<&CM_PARTIAL_RESOURCE_DESCRIPTOR> {
        let mut found = 0usize;
        for desc in self.descriptors(self.count as usize) {
            if desc.is_interrupt() {
                if found == index {
                    return Some(desc);
                }
                found += 1;
            }
        }
        None
    }
}

// =============================================================================
// CM_FULL_RESOURCE_DESCRIPTOR
// =============================================================================

/// CM_FULL_RESOURCE_DESCRIPTOR — полный дескриптор ресурсов для одной шины
#[repr(C)]
pub struct CM_FULL_RESOURCE_DESCRIPTOR {
    /// Тип интерфейса шины
    pub interface_type: INTERFACE_TYPE,
    /// Номер шины
    pub bus_number: ULONG,
    /// Список частичных дескрипторов
    pub partial_resource_list: CM_PARTIAL_RESOURCE_LIST,
}

// =============================================================================
// CM_RESOURCE_LIST
// =============================================================================

/// CM_RESOURCE_LIST — список ресурсов устройства
///
/// Передаётся в IRP_MN_START_DEVICE (allocated_resources, allocated_resources_translated).
#[repr(C)]
pub struct CM_RESOURCE_LIST {
    /// Количество полных дескрипторов
    pub count: ULONG,
    /// Массив полных дескрипторов (variable length)
    pub list: [CM_FULL_RESOURCE_DESCRIPTOR; 1],
}

impl CM_RESOURCE_LIST {
    /// Возвращает первый (обычно единственный) полный дескриптор
    pub fn first(&self) -> Option<&CM_FULL_RESOURCE_DESCRIPTOR> {
        if self.count > 0 {
            Some(&self.list[0])
        } else {
            None
        }
    }

    /// Возвращает partial resource list из первого дескриптора
    pub fn partial_list(&self) -> Option<&CM_PARTIAL_RESOURCE_LIST> {
        self.first().map(|full| &full.partial_resource_list)
    }
}

/// Указатель на CM_RESOURCE_LIST
pub type PCM_RESOURCE_LIST = *mut CM_RESOURCE_LIST;

// =============================================================================
// IO_RESOURCE_DESCRIPTOR - требования к ресурсам
// =============================================================================

/// IO_RESOURCE_DESCRIPTOR — описание требования к одному ресурсу
///
/// Используется в IRP_MN_QUERY_RESOURCE_REQUIREMENTS.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IO_RESOURCE_DESCRIPTOR {
    /// Опции (IO_RESOURCE_PREFERRED, IO_RESOURCE_ALTERNATIVE, etc.)
    pub option: UCHAR,
    /// Тип ресурса (CmResourceType*)
    pub r#type: UCHAR,
    /// Доля ресурса
    pub share_disposition: UCHAR,
    /// Запасной байт
    pub spare1: UCHAR,
    /// Флаги, зависящие от типа
    pub flags: USHORT,
    /// Запасные байты
    pub spare2: USHORT,
    /// Данные требования
    pub u: IO_RESOURCE_UNION,
}

/// Resource option flags
pub const IO_RESOURCE_PREFERRED: UCHAR = 0x01;
pub const IO_RESOURCE_ALTERNATIVE: UCHAR = 0x08;
pub const IO_RESOURCE_DEFAULT: UCHAR = 0x10;

/// Требование для Port ресурса
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IO_PORT_REQUIREMENT {
    /// Минимальный допустимый адрес
    pub minimum_address: PHYSICAL_ADDRESS,
    /// Максимальный допустимый адрес
    pub maximum_address: PHYSICAL_ADDRESS,
    /// Требуемое выравнивание
    pub alignment: ULONG,
    /// Требуемая длина
    pub length: ULONG,
}

/// Требование для Memory ресурса
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IO_MEMORY_REQUIREMENT {
    /// Минимальный допустимый адрес
    pub minimum_address: PHYSICAL_ADDRESS,
    /// Максимальный допустимый адрес
    pub maximum_address: PHYSICAL_ADDRESS,
    /// Требуемое выравнивание
    pub alignment: ULONG,
    /// Требуемая длина
    pub length: ULONG,
}

/// Требование для Interrupt ресурса
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IO_INTERRUPT_REQUIREMENT {
    /// Минимальный вектор
    pub minimum_vector: ULONG,
    /// Максимальный вектор
    pub maximum_vector: ULONG,
    /// Affinity mask
    pub affinity: usize, // KAFFINITY
}

/// Требование для DMA ресурса
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IO_DMA_REQUIREMENT {
    /// Минимальный канал
    pub minimum_channel: ULONG,
    /// Максимальный канал
    pub maximum_channel: ULONG,
}

/// Требование для BusNumber ресурса
#[repr(C)]
#[derive(Clone, Copy)]
pub struct IO_BUS_NUMBER_REQUIREMENT {
    /// Минимальный номер шины
    pub min_bus_number: ULONG,
    /// Максимальный номер шины
    pub max_bus_number: ULONG,
    /// Количество шин
    pub length: ULONG,
    /// Reserved
    pub reserved: ULONG,
}

/// Union для IO_RESOURCE_DESCRIPTOR
#[repr(C)]
#[derive(Clone, Copy)]
pub union IO_RESOURCE_UNION {
    pub port: IO_PORT_REQUIREMENT,
    pub memory: IO_MEMORY_REQUIREMENT,
    pub interrupt: IO_INTERRUPT_REQUIREMENT,
    pub dma: IO_DMA_REQUIREMENT,
    pub bus_number: IO_BUS_NUMBER_REQUIREMENT,
    pub raw: [u64; 4], // 32 bytes
}

impl IO_RESOURCE_DESCRIPTOR {
    pub const fn new() -> Self {
        Self {
            option: 0,
            r#type: 0,
            share_disposition: 0,
            spare1: 0,
            flags: 0,
            spare2: 0,
            u: IO_RESOURCE_UNION { raw: [0; 4] },
        }
    }

    /// Создаёт требование для memory BAR
    pub fn memory_bar(start: u64, length: u32, prefetchable: bool, is_64bit: bool) -> Self {
        let mut desc = Self::new();
        desc.option = IO_RESOURCE_PREFERRED;
        desc.r#type = CmResourceType::Memory as u8;
        desc.share_disposition = 0; // DeviceExclusive
        desc.flags = CM_RESOURCE_MEMORY_READ_WRITE | CM_RESOURCE_MEMORY_BAR;
        if prefetchable {
            desc.flags |= CM_RESOURCE_MEMORY_PREFETCHABLE;
        }

        let max_addr = if is_64bit { u64::MAX } else { 0xFFFFFFFF };

        desc.u.memory = IO_MEMORY_REQUIREMENT {
            minimum_address: start,
            maximum_address: if start != 0 {
                start.saturating_add(length as u64 - 1)
            } else {
                max_addr
            },
            alignment: length, // BAR alignment = size
            length,
        };
        desc
    }

    /// Создаёт требование для I/O port BAR
    pub fn port_bar(start: u64, length: u32) -> Self {
        let mut desc = Self::new();
        desc.option = IO_RESOURCE_PREFERRED;
        desc.r#type = CmResourceType::Port as u8;
        desc.share_disposition = 0;
        desc.flags = CM_RESOURCE_PORT_IO | CM_RESOURCE_PORT_BAR;
        desc.u.port = IO_PORT_REQUIREMENT {
            minimum_address: start,
            maximum_address: if start != 0 {
                start.saturating_add(length as u64 - 1)
            } else {
                0xFFFF
            },
            alignment: length,
            length,
        };
        desc
    }

    /// Создаёт требование для INTx interrupt
    pub fn interrupt(vector: u32, level_sensitive: bool) -> Self {
        let mut desc = Self::new();
        desc.option = IO_RESOURCE_PREFERRED;
        desc.r#type = CmResourceType::Interrupt as u8;
        desc.share_disposition = 1; // Shared
        desc.flags = if level_sensitive {
            CM_RESOURCE_INTERRUPT_LEVEL_SENSITIVE
        } else {
            CM_RESOURCE_INTERRUPT_LATCHED
        };
        desc.u.interrupt = IO_INTERRUPT_REQUIREMENT {
            minimum_vector: vector,
            maximum_vector: vector,
            affinity: !0usize, // All processors
        };
        desc
    }
}

// =============================================================================
// IO_RESOURCE_LIST
// =============================================================================

/// IO_RESOURCE_LIST — один альтернативный набор требований
#[repr(C)]
pub struct IO_RESOURCE_LIST {
    /// Версия
    pub version: USHORT,
    /// Ревизия
    pub revision: USHORT,
    /// Количество дескрипторов
    pub count: ULONG,
    /// Массив дескрипторов (variable length)
    pub descriptors: [IO_RESOURCE_DESCRIPTOR; 1],
}

// =============================================================================
// IO_RESOURCE_REQUIREMENTS_LIST
// =============================================================================

/// IO_RESOURCE_REQUIREMENTS_LIST — список требований к ресурсам
///
/// Возвращается в ответ на IRP_MN_QUERY_RESOURCE_REQUIREMENTS.
#[repr(C)]
pub struct IO_RESOURCE_REQUIREMENTS_LIST {
    /// Общий размер структуры в байтах
    pub list_size: ULONG,
    /// Тип интерфейса
    pub interface_type: INTERFACE_TYPE,
    /// Номер шины
    pub bus_number: ULONG,
    /// Номер слота
    pub slot_number: ULONG,
    /// Reserved
    pub reserved: [ULONG; 3],
    /// Количество альтернативных списков
    pub alternative_lists: ULONG,
    /// Массив списков (variable length)
    pub list: [IO_RESOURCE_LIST; 1],
}

/// Указатель на IO_RESOURCE_REQUIREMENTS_LIST
pub type PIO_RESOURCE_REQUIREMENTS_LIST = *mut IO_RESOURCE_REQUIREMENTS_LIST;

impl IO_RESOURCE_REQUIREMENTS_LIST {
    /// Вычисляет размер структуры для заданного количества дескрипторов
    pub fn size_for_descriptors(count: usize) -> usize {
        // Base size + (count - 1) * descriptor size
        // (один дескриптор уже включен в IO_RESOURCE_LIST)
        core::mem::size_of::<Self>()
            + (count.saturating_sub(1)) * core::mem::size_of::<IO_RESOURCE_DESCRIPTOR>()
    }
}

// =============================================================================
// PCI-specific resource helpers
// =============================================================================

/// Информация о PCI BAR
#[derive(Debug, Clone, Copy)]
pub struct PciBarInfo {
    /// Индекс BAR (0-5)
    pub bar_index: u8,
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
    /// Создаёт IO_RESOURCE_DESCRIPTOR для этого BAR
    pub fn to_resource_descriptor(&self) -> IO_RESOURCE_DESCRIPTOR {
        if self.is_memory {
            IO_RESOURCE_DESCRIPTOR::memory_bar(
                self.base_address,
                self.size as u32,
                self.is_prefetchable,
                self.is_64bit,
            )
        } else {
            IO_RESOURCE_DESCRIPTOR::port_bar(self.base_address, self.size as u32)
        }
    }

    /// Создаёт CM_PARTIAL_RESOURCE_DESCRIPTOR для этого BAR с назначенным адресом
    pub fn to_allocated_descriptor(&self, assigned_address: u64) -> CM_PARTIAL_RESOURCE_DESCRIPTOR {
        let mut desc = CM_PARTIAL_RESOURCE_DESCRIPTOR::new();
        desc.share_disposition = 0; // DeviceExclusive

        if self.is_memory {
            desc.r#type = CmResourceType::Memory as u8;
            desc.flags = CM_RESOURCE_MEMORY_READ_WRITE | CM_RESOURCE_MEMORY_BAR;
            if self.is_prefetchable {
                desc.flags |= CM_RESOURCE_MEMORY_PREFETCHABLE;
            }
            desc.u.memory = CM_MEMORY_RESOURCE {
                start: assigned_address,
                length: self.size as ULONG,
            };
        } else {
            desc.r#type = CmResourceType::Port as u8;
            desc.flags = CM_RESOURCE_PORT_IO | CM_RESOURCE_PORT_BAR;
            desc.u.port = CM_PORT_RESOURCE {
                start: assigned_address,
                length: self.size as ULONG,
            };
        }

        desc
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_descriptor_sizes() {
        assert_eq!(
            core::mem::size_of::<CM_PARTIAL_RESOURCE_DESCRIPTOR>(),
            16
        );
        assert_eq!(core::mem::size_of::<IO_RESOURCE_DESCRIPTOR>(), 40);
    }

    #[test]
    fn test_memory_bar_descriptor() {
        let desc = IO_RESOURCE_DESCRIPTOR::memory_bar(0xFEB00000, 0x10000, true, false);
        assert_eq!(desc.r#type, CmResourceType::Memory as u8);
        unsafe {
            assert_eq!(desc.u.memory.minimum_address, 0xFEB00000);
            assert_eq!(desc.u.memory.length, 0x10000);
        }
    }
}

