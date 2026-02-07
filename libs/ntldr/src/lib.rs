//! NT Loader Shared Structures
//!
//! Общие структуры для передачи данных между winload.efi и ntoskrnl.exe.
//! Основаны на Windows NT6.1 (Win7) формате с упрощениями для XenOS.

#![no_std]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

// =============================================================================
// Базовые типы
// =============================================================================

/// Стандартная двусвязная LIST_ENTRY (NT-стиль).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LIST_ENTRY {
    pub Flink: *mut LIST_ENTRY,
    pub Blink: *mut LIST_ENTRY,
}

impl LIST_ENTRY {
    pub const fn empty() -> Self {
        Self {
            Flink: core::ptr::null_mut(),
            Blink: core::ptr::null_mut(),
        }
    }

    /// Инициализирует список как пустой (указывает на себя).
    pub fn init_head(&mut self) {
        let self_ptr = self as *mut LIST_ENTRY;
        self.Flink = self_ptr;
        self.Blink = self_ptr;
    }

    /// Проверяет, пуст ли список.
    pub fn is_empty(&self) -> bool {
        let self_ptr = self as *const LIST_ENTRY;
        self.Flink as *const _ == self_ptr
    }
}

// =============================================================================
// Memory Types
// =============================================================================

/// NT Memory Types (для MemoryDescriptorList).
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MEMORY_TYPE {
    LoaderExceptionBlock = 0,
    LoaderSystemBlock = 1,
    LoaderFree = 2,
    LoaderBad = 3,
    LoaderLoadedProgram = 4,
    LoaderFirmwareTemporary = 5,
    LoaderFirmwarePermanent = 6,
    LoaderOsloaderHeap = 7,
    LoaderOsloaderStack = 8,
    LoaderSystemCode = 9,
    LoaderHalCode = 10,
    LoaderBootDriver = 11,
    LoaderConsoleInDriver = 12,
    LoaderConsoleOutDriver = 13,
    LoaderStartupDpcStack = 14,
    LoaderStartupKernelStack = 15,
    LoaderStartupPanicStack = 16,
    LoaderStartupPcrPage = 17,
    LoaderStartupPdrPage = 18,
    LoaderRegistryData = 19,
    LoaderMemoryData = 20,
    LoaderNlsData = 21,
    LoaderSpecialMemory = 22,
    LoaderBBTMemory = 23,
    LoaderReserve = 24,
    LoaderXIPRom = 25,
    LoaderHALCachedMemory = 26,
    LoaderLargePageFiller = 27,
    LoaderErrorLogMemory = 28,
    LoaderMaximum = 29,
}

/// NT Memory Allocation Descriptor.
#[repr(C)]
pub struct MEMORY_ALLOCATION_DESCRIPTOR {
    /// Связь в списке.
    pub ListEntry: LIST_ENTRY,
    /// Тип памяти.
    pub MemoryType: MEMORY_TYPE,
    /// Базовая страница (PFN).
    pub BasePage: u64,
    /// Количество страниц.
    pub PageCount: u64,
}

// =============================================================================
// Framebuffer Info
// =============================================================================

/// Информация о фреймбуфере для графического вывода.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct LOADER_FRAMEBUFFER_INFO {
    /// 1 если валиден, 0 если фреймбуфер недоступен.
    pub Present: u8,
    pub Reserved: [u8; 7],

    /// Физический адрес фреймбуфера.
    pub FrameBufferBase: u64,
    /// Размер фреймбуфера в байтах.
    pub FrameBufferSize: u64,

    /// Ширина в пикселях.
    pub HorizontalResolution: u32,
    /// Высота в пикселях.
    pub VerticalResolution: u32,
    /// Пикселей на строку (stride).
    pub PixelsPerScanLine: u32,
    /// Бит на пиксель (обычно 32).
    pub BitsPerPixel: u32,
}

impl LOADER_FRAMEBUFFER_INFO {
    pub const fn empty() -> Self {
        Self {
            Present: 0,
            Reserved: [0; 7],
            FrameBufferBase: 0,
            FrameBufferSize: 0,
            HorizontalResolution: 0,
            VerticalResolution: 0,
            PixelsPerScanLine: 0,
            BitsPerPixel: 0,
        }
    }
}

// =============================================================================
// LOADER_PARAMETER_BLOCK
// =============================================================================

/// Основная структура передачи данных от загрузчика к ядру.
///
/// Это упрощённая версия Windows LOADER_PARAMETER_BLOCK,
/// адаптированная для XenOS.
#[repr(C)]
pub struct LOADER_PARAMETER_BLOCK {
    // -------------------------------------------------------------------------
    // Идентификация
    // -------------------------------------------------------------------------
    /// Версия структуры (для эволюции формата).
    pub OsMajorVersion: u32,
    /// Минорная версия.
    pub OsMinorVersion: u32,
    /// Размер структуры в байтах.
    pub Size: u32,
    /// Reserved.
    pub Reserved: u32,

    // -------------------------------------------------------------------------
    // HHDM (Higher Half Direct Map)
    // -------------------------------------------------------------------------
    /// HHDM offset - смещение для преобразования физ. адресов в виртуальные.
    /// Вся физическая память отображена по адресу HhdmOffset + физ_адрес.
    pub HhdmOffset: u64,

    // -------------------------------------------------------------------------
    // Memory Information
    // -------------------------------------------------------------------------
    /// Указатель на список дескрипторов памяти.
    pub MemoryDescriptorListHead: LIST_ENTRY,

    // -------------------------------------------------------------------------
    // Boot Loaded Programs (kernel, HAL, boot drivers)
    // -------------------------------------------------------------------------
    /// Список загруженных модулей (ntoskrnl, hal, boot drivers).
    pub LoadOrderListHead: LIST_ENTRY,

    // -------------------------------------------------------------------------
    // Boot Drivers
    // -------------------------------------------------------------------------
    /// Список boot-start драйверов.
    pub BootDriverListHead: LIST_ENTRY,

    // -------------------------------------------------------------------------
    // Registry (SYSTEM hive)
    // -------------------------------------------------------------------------
    /// Адрес загруженного SYSTEM hive в памяти.
    pub RegistryBase: *mut u8,
    /// Размер SYSTEM hive в байтах.
    pub RegistryLength: u32,
    pub RegistryReserved: u32,

    // -------------------------------------------------------------------------
    // ACPI
    // -------------------------------------------------------------------------
    /// Физический адрес RSDP.
    pub AcpiTablePhysical: u64,

    // -------------------------------------------------------------------------
    // Framebuffer / Display
    // -------------------------------------------------------------------------
    /// Информация о фреймбуфере.
    pub FramebufferInfo: LOADER_FRAMEBUFFER_INFO,

    // -------------------------------------------------------------------------
    // Memory Pool
    // -------------------------------------------------------------------------
    /// Базовый адрес для раннего pool/heap.
    pub PoolBase: u64,
    /// Размер pool/heap в байтах.
    pub PoolSize: u64,

    // -------------------------------------------------------------------------
    // Kernel/HAL locations
    // -------------------------------------------------------------------------
    /// Базовый адрес загруженного ntoskrnl.
    pub KernelBase: u64,
    /// Entry point ядра.
    pub KernelEntry: u64,
    /// Размер образа ядра.
    pub KernelSize: u32,
    pub KernelReserved: u32,

    // -------------------------------------------------------------------------
    // NLS Data (unicode.nls)
    // -------------------------------------------------------------------------
    /// Указатель на буфер с unicode.nls (виртуальный адрес через HHDM).
    /// Загрузчик читает файл целиком и передаёт указатель.
    /// Ядро парсит XNLS формат и извлекает таблицы.
    pub UnicodeNlsBase: *const u8,
    /// Размер unicode.nls в байтах.
    pub UnicodeNlsSize: u64,

    // -------------------------------------------------------------------------
    // Load Options (командная строка загрузки)
    // -------------------------------------------------------------------------
    /// Указатель на строку Load Options (ASCII, null-terminated).
    /// Содержит ключи вида "/DEBUG", "/SOS" и т.п.
    /// В NT6.1: PSTR LoadOptions.
    pub LoadOptions: *const u8,
    /// Длина строки LoadOptions в байтах (без null-терминатора).
    /// В NT6.1: ULONG LoadOptionsLength (но считается включая null).
    pub LoadOptionsLength: u32,
    /// Reserved для выравнивания.
    pub LoadOptionsReserved: u32,

    // -------------------------------------------------------------------------
    // Extension (для будущих расширений)
    // -------------------------------------------------------------------------
    /// Указатель на LOADER_PARAMETER_EXTENSION.
    pub Extension: *mut LOADER_PARAMETER_EXTENSION,
}

impl LOADER_PARAMETER_BLOCK {
    /// Текущая версия структуры.
    pub const CURRENT_VERSION_MAJOR: u32 = 6;
    pub const CURRENT_VERSION_MINOR: u32 = 1;

    pub const fn empty() -> Self {
        Self {
            OsMajorVersion: Self::CURRENT_VERSION_MAJOR,
            OsMinorVersion: Self::CURRENT_VERSION_MINOR,
            Size: core::mem::size_of::<Self>() as u32,
            Reserved: 0,
            HhdmOffset: 0,
            MemoryDescriptorListHead: LIST_ENTRY::empty(),
            LoadOrderListHead: LIST_ENTRY::empty(),
            BootDriverListHead: LIST_ENTRY::empty(),
            RegistryBase: core::ptr::null_mut(),
            RegistryLength: 0,
            RegistryReserved: 0,
            AcpiTablePhysical: 0,
            FramebufferInfo: LOADER_FRAMEBUFFER_INFO::empty(),
            PoolBase: 0,
            PoolSize: 0,
            KernelBase: 0,
            KernelEntry: 0,
            KernelSize: 0,
            KernelReserved: 0,
            UnicodeNlsBase: core::ptr::null(),
            UnicodeNlsSize: 0,
            LoadOptions: core::ptr::null(),
            LoadOptionsLength: 0,
            LoadOptionsReserved: 0,
            Extension: core::ptr::null_mut(),
        }
    }
}

// =============================================================================
// LOADER_PARAMETER_EXTENSION
// =============================================================================

/// Расширение LOADER_PARAMETER_BLOCK для дополнительных данных.
#[repr(C)]
pub struct LOADER_PARAMETER_EXTENSION {
    /// Размер структуры.
    pub Size: u32,
    /// Profile status.
    pub Profile: u32,

    /// Firmware-specific information.
    pub FirmwareType: u32,
    /// Reserved.
    pub FirmwareReserved: u32,

    /// Физический адрес UEFI System Table (для UEFI runtime services).
    pub EfiSystemTable: u64,

    /// Физический адрес UEFI Memory Map (после ExitBootServices).
    pub EfiMemoryMap: u64,
    /// Размер UEFI Memory Map.
    pub EfiMemoryMapSize: u32,
    /// Размер одного дескриптора UEFI.
    pub EfiMemoryMapDescriptorSize: u32,
}

impl LOADER_PARAMETER_EXTENSION {
    pub const fn empty() -> Self {
        Self {
            Size: core::mem::size_of::<Self>() as u32,
            Profile: 0,
            FirmwareType: 2, // EFI
            FirmwareReserved: 0,
            EfiSystemTable: 0,
            EfiMemoryMap: 0,
            EfiMemoryMapSize: 0,
            EfiMemoryMapDescriptorSize: 0,
        }
    }
}

// =============================================================================
// Loaded Module Entry
// =============================================================================

/// Запись о загруженном модуле (в LoadOrderListHead).
/// Упрощённая версия NT LDR_DATA_TABLE_ENTRY.
#[repr(C)]
pub struct LOADER_MODULE_ENTRY {
    /// Связь в списке LoadOrderListHead.
    pub InLoadOrderLinks: LIST_ENTRY,

    /// Базовый адрес загруженного образа.
    pub DllBase: u64,
    /// Entry point модуля.
    pub EntryPoint: u64,
    /// Размер образа.
    pub SizeOfImage: u32,
    /// Reserved.
    pub Reserved: u32,

    /// Имя модуля (путь).
    /// Фиксированный буфер для простоты.
    pub FullDllName: [u8; 260],
    /// Длина имени.
    pub FullDllNameLength: u16,
    /// Reserved.
    pub NameReserved: [u8; 6],
}

impl LOADER_MODULE_ENTRY {
    pub const fn empty() -> Self {
        Self {
            InLoadOrderLinks: LIST_ENTRY::empty(),
            DllBase: 0,
            EntryPoint: 0,
            SizeOfImage: 0,
            Reserved: 0,
            FullDllName: [0; 260],
            FullDllNameLength: 0,
            NameReserved: [0; 6],
        }
    }
}

// =============================================================================
// Boot Driver Entry
// =============================================================================

/// Запись о boot-start драйвере (в BootDriverListHead).
#[repr(C)]
pub struct BOOT_DRIVER_LIST_ENTRY {
    /// Связь в списке.
    pub Link: LIST_ENTRY,

    /// Registry path (\Registry\Machine\System\CurrentControlSet\Services\Xxx).
    pub RegistryPath: [u8; 260],
    pub RegistryPathLength: u16,
    pub PathReserved: [u8; 6],

    /// File path (\SystemRoot\system32\drivers\xxx.sys).
    pub FilePath: [u8; 260],
    pub FilePathLength: u16,
    pub FileReserved: [u8; 6],

    /// Указатель на загруженный модуль (LOADER_MODULE_ENTRY).
    pub LdrEntry: *mut LOADER_MODULE_ENTRY,
}

impl BOOT_DRIVER_LIST_ENTRY {
    pub const fn empty() -> Self {
        Self {
            Link: LIST_ENTRY::empty(),
            RegistryPath: [0; 260],
            RegistryPathLength: 0,
            PathReserved: [0; 6],
            FilePath: [0; 260],
            FilePathLength: 0,
            FileReserved: [0; 6],
            LdrEntry: core::ptr::null_mut(),
        }
    }
}

// =============================================================================
// Constants
// =============================================================================

/// Размер страницы (4 KB).
pub const PAGE_SIZE: u64 = 4096;

/// Firmware type: UEFI.
pub const FIRMWARE_TYPE_UEFI: u32 = 2;
