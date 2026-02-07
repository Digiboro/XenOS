//! NT-совместимые структуры для передачи в ядро
//!
//! Структуры соответствуют Windows NT6.1 (Windows 7) layout.
//! Используются для формирования LOADER_PARAMETER_BLOCK.

#![allow(dead_code)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

use alloc::boxed::Box;
use alloc::vec::Vec;

// =============================================================================
// NT List Entry (двусвязный список)
// =============================================================================

/// LIST_ENTRY - стандартная структура двусвязного списка NT.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct LIST_ENTRY {
    pub Flink: *mut LIST_ENTRY,
    pub Blink: *mut LIST_ENTRY,
}

impl LIST_ENTRY {
    pub const fn empty() -> Self {
        LIST_ENTRY {
            Flink: core::ptr::null_mut(),
            Blink: core::ptr::null_mut(),
        }
    }

    /// Инициализирует пустой список (указывает сам на себя).
    pub fn init_head(&mut self) {
        self.Flink = self as *mut LIST_ENTRY;
        self.Blink = self as *mut LIST_ENTRY;
    }

    /// Проверяет, пуст ли список.
    pub fn is_empty(&self) -> bool {
        self.Flink == self as *const LIST_ENTRY as *mut LIST_ENTRY
    }
}

// =============================================================================
// Memory Types
// =============================================================================

/// Типы памяти NT (MEMORY_TYPE enum).
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

/// MEMORY_ALLOCATION_DESCRIPTOR - дескриптор региона памяти.
#[repr(C)]
#[derive(Debug)]
pub struct MEMORY_ALLOCATION_DESCRIPTOR {
    pub ListEntry: LIST_ENTRY,
    pub MemoryType: MEMORY_TYPE,
    pub BasePage: u64, // PFN (Page Frame Number)
    pub PageCount: u64,
}

impl MEMORY_ALLOCATION_DESCRIPTOR {
    pub fn new(memory_type: MEMORY_TYPE, base_page: u64, page_count: u64) -> Self {
        MEMORY_ALLOCATION_DESCRIPTOR {
            ListEntry: LIST_ENTRY::empty(),
            MemoryType: memory_type,
            BasePage: base_page,
            PageCount: page_count,
        }
    }
}

// =============================================================================
// Loader Parameter Block (Win7 compatible)
// =============================================================================

/// LOADER_PARAMETER_BLOCK - основная структура передачи данных от загрузчика ядру.
/// Layout соответствует Windows 7 / NT6.1.
#[repr(C)]
pub struct LOADER_PARAMETER_BLOCK {
    // Версия структуры для XenOS
    pub OsMajorVersion: u32,
    pub OsMinorVersion: u32,
    pub Size: u32,
    pub OsLoaderSecurityVersion: u32,

    // Списки
    pub LoadOrderListHead: LIST_ENTRY,
    pub MemoryDescriptorListHead: LIST_ENTRY,
    pub BootDriverListHead: LIST_ENTRY,
    pub EarlyLaunchListHead: LIST_ENTRY,
    pub CoreDriverListHead: LIST_ENTRY,
    pub CoreExtensionsDriverListHead: LIST_ENTRY,
    pub TpmCoreDriverListHead: LIST_ENTRY,

    // Ядро и HAL
    pub KernelStack: u64,
    pub Prcb: u64,
    pub Process: u64,
    pub Thread: u64,

    // Registry
    pub RegistryLength: u32,
    pub RegistryBase: *mut u8,

    // Configuration root (CONFIGURATION_COMPONENT_DATA)
    pub ConfigurationRoot: *mut u8,

    // ARC paths
    pub ArcBootDeviceName: *const u8,
    pub ArcHalDeviceName: *const u8,
    pub NtBootPathName: *const u8,
    pub NtHalPathName: *const u8,
    pub LoadOptions: *const u8,

    // NLS data
    pub NlsData: *mut NLS_DATA_BLOCK,

    // ARC disk info
    pub ArcDiskInformation: *mut u8,

    // Extension
    pub Extension: *mut LOADER_PARAMETER_EXTENSION,

    // Firmware information
    pub FirmwareInformation: FIRMWARE_INFORMATION_LOADER_BLOCK,
}

impl LOADER_PARAMETER_BLOCK {
    pub fn new() -> Box<Self> {
        let mut block = Box::new(Self {
            OsMajorVersion: 6, // NT 6.x
            OsMinorVersion: 1, // NT 6.1 (Win7)
            Size: core::mem::size_of::<Self>() as u32,
            OsLoaderSecurityVersion: 0,

            LoadOrderListHead: LIST_ENTRY::empty(),
            MemoryDescriptorListHead: LIST_ENTRY::empty(),
            BootDriverListHead: LIST_ENTRY::empty(),
            EarlyLaunchListHead: LIST_ENTRY::empty(),
            CoreDriverListHead: LIST_ENTRY::empty(),
            CoreExtensionsDriverListHead: LIST_ENTRY::empty(),
            TpmCoreDriverListHead: LIST_ENTRY::empty(),

            KernelStack: 0,
            Prcb: 0,
            Process: 0,
            Thread: 0,

            RegistryLength: 0,
            RegistryBase: core::ptr::null_mut(),

            ConfigurationRoot: core::ptr::null_mut(),

            ArcBootDeviceName: core::ptr::null(),
            ArcHalDeviceName: core::ptr::null(),
            NtBootPathName: core::ptr::null(),
            NtHalPathName: core::ptr::null(),
            LoadOptions: core::ptr::null(),

            NlsData: core::ptr::null_mut(),
            ArcDiskInformation: core::ptr::null_mut(),
            Extension: core::ptr::null_mut(),

            FirmwareInformation: FIRMWARE_INFORMATION_LOADER_BLOCK::new_uefi(),
        });

        // Инициализируем списки
        block.LoadOrderListHead.init_head();
        block.MemoryDescriptorListHead.init_head();
        block.BootDriverListHead.init_head();
        block.EarlyLaunchListHead.init_head();
        block.CoreDriverListHead.init_head();
        block.CoreExtensionsDriverListHead.init_head();
        block.TpmCoreDriverListHead.init_head();

        block
    }
}

impl Default for LOADER_PARAMETER_BLOCK {
    fn default() -> Self {
        Self {
            OsMajorVersion: 6,
            OsMinorVersion: 1,
            Size: core::mem::size_of::<Self>() as u32,
            OsLoaderSecurityVersion: 0,

            LoadOrderListHead: LIST_ENTRY::empty(),
            MemoryDescriptorListHead: LIST_ENTRY::empty(),
            BootDriverListHead: LIST_ENTRY::empty(),
            EarlyLaunchListHead: LIST_ENTRY::empty(),
            CoreDriverListHead: LIST_ENTRY::empty(),
            CoreExtensionsDriverListHead: LIST_ENTRY::empty(),
            TpmCoreDriverListHead: LIST_ENTRY::empty(),

            KernelStack: 0,
            Prcb: 0,
            Process: 0,
            Thread: 0,

            RegistryLength: 0,
            RegistryBase: core::ptr::null_mut(),

            ConfigurationRoot: core::ptr::null_mut(),

            ArcBootDeviceName: core::ptr::null(),
            ArcHalDeviceName: core::ptr::null(),
            NtBootPathName: core::ptr::null(),
            NtHalPathName: core::ptr::null(),
            LoadOptions: core::ptr::null(),

            NlsData: core::ptr::null_mut(),
            ArcDiskInformation: core::ptr::null_mut(),
            Extension: core::ptr::null_mut(),

            FirmwareInformation: FIRMWARE_INFORMATION_LOADER_BLOCK::new_uefi(),
        }
    }
}

// =============================================================================
// Loader Parameter Extension
// =============================================================================

/// LOADER_PARAMETER_EXTENSION - расширенные данные загрузчика.
#[repr(C)]
pub struct LOADER_PARAMETER_EXTENSION {
    pub Size: u32,
    pub Profile: PROFILE_PARAMETER_BLOCK,

    pub EmInfFileImage: *mut u8,
    pub EmInfFileSize: u32,

    pub TriageDumpBlock: *mut u8,

    // Загрузчик установил следующие поля
    pub HeadlessLoaderBlock: *mut u8,
    pub SMBiosEPSHeader: *mut u8,
    pub DrvDBImage: *mut u8,
    pub DrvDBSize: u32,
    pub DrvDBPatchImage: *mut u8,
    pub DrvDBPatchSize: u32,

    pub NetworkLoaderBlock: *mut u8,

    pub FirmwareDescriptorListHead: LIST_ENTRY,

    // ACPI tables
    pub AcpiTable: *mut u8,
    pub AcpiTableSize: u32,

    // Processor information
    pub LoaderPerformanceData: *mut u8,

    // Boot application persistence
    pub BootApplicationPersistentData: LIST_ENTRY,

    // Winload-specific (указатели на внутренние данные winload)
    pub WmdTestResult: *mut u8,
    pub BootIdentifier: [u8; 16], // GUID

    // Misc
    pub ResumePages: u32,
    pub DumpHeader: *mut u8,

    // Memory caching attributes
    pub MemoryCachingRequirementsCount: u32,
    pub MemoryCachingRequirements: *mut u8,

    // Boot entropy
    pub BootEntropyResult: BOOT_ENTROPY_LDR_RESULT,

    // Processor count
    pub ProcessorCounterFrequency: u64,

    // Hypervisor
    pub HypervisorEnforcedCodeIntegrity: u32,

    // Hardware configuration
    pub HardwareConfigurationId: u64,

    // Various flags
    pub BootFlags: u32,
    pub InternalBootFlags: u32,

    // Pointers to attached devices
    pub AttachedHives: LIST_ENTRY,

    // Memory ranges
    pub MemoryReserves: LIST_ENTRY,
}

impl LOADER_PARAMETER_EXTENSION {
    pub fn new() -> Box<Self> {
        let mut ext = Box::new(Self {
            Size: core::mem::size_of::<Self>() as u32,
            Profile: PROFILE_PARAMETER_BLOCK::empty(),

            EmInfFileImage: core::ptr::null_mut(),
            EmInfFileSize: 0,
            TriageDumpBlock: core::ptr::null_mut(),
            HeadlessLoaderBlock: core::ptr::null_mut(),
            SMBiosEPSHeader: core::ptr::null_mut(),
            DrvDBImage: core::ptr::null_mut(),
            DrvDBSize: 0,
            DrvDBPatchImage: core::ptr::null_mut(),
            DrvDBPatchSize: 0,
            NetworkLoaderBlock: core::ptr::null_mut(),

            FirmwareDescriptorListHead: LIST_ENTRY::empty(),

            AcpiTable: core::ptr::null_mut(),
            AcpiTableSize: 0,
            LoaderPerformanceData: core::ptr::null_mut(),
            BootApplicationPersistentData: LIST_ENTRY::empty(),
            WmdTestResult: core::ptr::null_mut(),
            BootIdentifier: [0; 16],
            ResumePages: 0,
            DumpHeader: core::ptr::null_mut(),
            MemoryCachingRequirementsCount: 0,
            MemoryCachingRequirements: core::ptr::null_mut(),
            BootEntropyResult: BOOT_ENTROPY_LDR_RESULT::empty(),
            ProcessorCounterFrequency: 0,
            HypervisorEnforcedCodeIntegrity: 0,
            HardwareConfigurationId: 0,
            BootFlags: 0,
            InternalBootFlags: 0,
            AttachedHives: LIST_ENTRY::empty(),
            MemoryReserves: LIST_ENTRY::empty(),
        });

        // Инициализируем списки
        ext.FirmwareDescriptorListHead.init_head();
        ext.BootApplicationPersistentData.init_head();
        ext.AttachedHives.init_head();
        ext.MemoryReserves.init_head();

        ext
    }
}

impl Default for LOADER_PARAMETER_EXTENSION {
    fn default() -> Self {
        Self {
            Size: core::mem::size_of::<Self>() as u32,
            Profile: PROFILE_PARAMETER_BLOCK::empty(),
            EmInfFileImage: core::ptr::null_mut(),
            EmInfFileSize: 0,
            TriageDumpBlock: core::ptr::null_mut(),
            HeadlessLoaderBlock: core::ptr::null_mut(),
            SMBiosEPSHeader: core::ptr::null_mut(),
            DrvDBImage: core::ptr::null_mut(),
            DrvDBSize: 0,
            DrvDBPatchImage: core::ptr::null_mut(),
            DrvDBPatchSize: 0,
            NetworkLoaderBlock: core::ptr::null_mut(),
            FirmwareDescriptorListHead: LIST_ENTRY::empty(),
            AcpiTable: core::ptr::null_mut(),
            AcpiTableSize: 0,
            LoaderPerformanceData: core::ptr::null_mut(),
            BootApplicationPersistentData: LIST_ENTRY::empty(),
            WmdTestResult: core::ptr::null_mut(),
            BootIdentifier: [0; 16],
            ResumePages: 0,
            DumpHeader: core::ptr::null_mut(),
            MemoryCachingRequirementsCount: 0,
            MemoryCachingRequirements: core::ptr::null_mut(),
            BootEntropyResult: BOOT_ENTROPY_LDR_RESULT::empty(),
            ProcessorCounterFrequency: 0,
            HypervisorEnforcedCodeIntegrity: 0,
            HardwareConfigurationId: 0,
            BootFlags: 0,
            InternalBootFlags: 0,
            AttachedHives: LIST_ENTRY::empty(),
            MemoryReserves: LIST_ENTRY::empty(),
        }
    }
}

// =============================================================================
// Supporting structures
// =============================================================================

/// NLS_DATA_BLOCK - данные NLS (National Language Support).
#[repr(C)]
#[derive(Debug, Default)]
pub struct NLS_DATA_BLOCK {
    pub AnsiCodePageData: *mut u8,
    pub OemCodePageData: *mut u8,
    pub UnicodeCaseTableData: *mut u8,
}

/// PROFILE_PARAMETER_BLOCK
#[repr(C)]
#[derive(Debug, Default)]
pub struct PROFILE_PARAMETER_BLOCK {
    pub Status: u16,
    pub Reserved: u16,
    pub DockingState: u32,
    pub Capabilities: u32,
    pub DockID: u32,
    pub SerialNumber: u32,
}

impl PROFILE_PARAMETER_BLOCK {
    pub const fn empty() -> Self {
        Self {
            Status: 0,
            Reserved: 0,
            DockingState: 0,
            Capabilities: 0,
            DockID: 0,
            SerialNumber: 0,
        }
    }
}

/// BOOT_ENTROPY_LDR_RESULT
#[repr(C)]
#[derive(Debug)]
pub struct BOOT_ENTROPY_LDR_RESULT {
    pub MaxEntropyCached: u32,
    pub EntropyCount: u32,
    pub Buffer: [u8; 64],
}

impl BOOT_ENTROPY_LDR_RESULT {
    pub const fn empty() -> Self {
        Self {
            MaxEntropyCached: 0,
            EntropyCount: 0,
            Buffer: [0; 64],
        }
    }
}

/// Firmware type
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FIRMWARE_TYPE {
    Unknown = 0,
    Bios = 1,
    Uefi = 2,
    Max = 3,
}

/// FIRMWARE_INFORMATION_LOADER_BLOCK
#[repr(C)]
pub struct FIRMWARE_INFORMATION_LOADER_BLOCK {
    pub FirmwareTypeUefi: u32, // FIRMWARE_TYPE as u32
    pub EfiRuntimeUseIum: u32,
    pub EfiRuntimePageProtectionSupported: u32,

    // EFI specific
    pub EfiSystemTable: u64, // Физический адрес EFI_SYSTEM_TABLE
    pub EfiMemoryMap: u64,   // Физический адрес UEFI memory map
    pub EfiMemoryMapSize: u32,
    pub EfiMemoryMapDescriptorSize: u32,
    pub EfiMemoryMapDescriptorVersion: u32,
}

impl FIRMWARE_INFORMATION_LOADER_BLOCK {
    pub fn new_uefi() -> Self {
        Self {
            FirmwareTypeUefi: FIRMWARE_TYPE::Uefi as u32,
            EfiRuntimeUseIum: 0,
            EfiRuntimePageProtectionSupported: 0,
            EfiSystemTable: 0,
            EfiMemoryMap: 0,
            EfiMemoryMapSize: 0,
            EfiMemoryMapDescriptorSize: 0,
            EfiMemoryMapDescriptorVersion: 0,
        }
    }
}

impl Default for FIRMWARE_INFORMATION_LOADER_BLOCK {
    fn default() -> Self {
        Self::new_uefi()
    }
}

// =============================================================================
// Boot Driver List Entry
// =============================================================================

/// BOOT_DRIVER_LIST_ENTRY - запись о boot driver.
#[repr(C)]
pub struct BOOT_DRIVER_LIST_ENTRY {
    pub Link: LIST_ENTRY,
    pub FilePath: UNICODE_STRING,
    pub RegistryPath: UNICODE_STRING,
    pub LdrEntry: *mut LDR_DATA_TABLE_ENTRY,
    pub LoadCount: u32,
}

/// LDR_DATA_TABLE_ENTRY - запись о загруженном модуле.
#[repr(C)]
pub struct LDR_DATA_TABLE_ENTRY {
    pub InLoadOrderLinks: LIST_ENTRY,
    pub InMemoryOrderLinks: LIST_ENTRY,
    pub InInitializationOrderLinks: LIST_ENTRY,
    pub DllBase: *mut u8,
    pub EntryPoint: *mut u8,
    pub SizeOfImage: u32,
    pub FullDllName: UNICODE_STRING,
    pub BaseDllName: UNICODE_STRING,
    pub Flags: u32,
    pub LoadCount: u16,
    pub TlsIndex: u16,
    pub HashLinks: LIST_ENTRY,
    pub TimeDateStamp: u32,
    pub EntryPointActivationContext: *mut u8,
    pub Lock: *mut u8,
    pub DdagNode: *mut u8,
    pub NodeModuleLink: LIST_ENTRY,
    pub LoadContext: *mut u8,
    pub ParentDllBase: *mut u8,
    pub SwitchBackContext: *mut u8,
}

/// UNICODE_STRING
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct UNICODE_STRING {
    pub Length: u16,
    pub MaximumLength: u16,
    pub Buffer: *mut u16,
}

impl UNICODE_STRING {
    pub const fn empty() -> Self {
        Self {
            Length: 0,
            MaximumLength: 0,
            Buffer: core::ptr::null_mut(),
        }
    }
}

impl Default for UNICODE_STRING {
    fn default() -> Self {
        Self::empty()
    }
}

// =============================================================================
// Memory map builder
// =============================================================================

/// Хранилище для дескрипторов памяти.
pub struct MemoryDescriptorList {
    pub descriptors: Vec<Box<MEMORY_ALLOCATION_DESCRIPTOR>>,
}

impl MemoryDescriptorList {
    pub fn new() -> Self {
        Self {
            descriptors: Vec::new(),
        }
    }

    /// Добавляет дескриптор.
    pub fn add(&mut self, memory_type: MEMORY_TYPE, base_page: u64, page_count: u64) {
        let desc = Box::new(MEMORY_ALLOCATION_DESCRIPTOR::new(
            memory_type,
            base_page,
            page_count,
        ));
        self.descriptors.push(desc);
    }

    /// Возвращает количество дескрипторов.
    pub fn count(&self) -> usize {
        self.descriptors.len()
    }

    /// Связывает все дескрипторы в двусвязный список и возвращает head.
    pub fn link_to_head(&mut self, head: &mut LIST_ENTRY) {
        head.init_head();

        if self.descriptors.is_empty() {
            return;
        }

        // Связываем в список
        for i in 0..self.descriptors.len() {
            let entry = &mut self.descriptors[i].ListEntry as *mut LIST_ENTRY;

            unsafe {
                if i == 0 {
                    // Первый элемент
                    (*entry).Blink = head;
                    head.Flink = entry;
                } else {
                    // Связываем с предыдущим
                    let prev = &mut self.descriptors[i - 1].ListEntry as *mut LIST_ENTRY;
                    (*entry).Blink = prev;
                    (*prev).Flink = entry;
                }

                if i == self.descriptors.len() - 1 {
                    // Последний элемент
                    (*entry).Flink = head;
                    head.Blink = entry;
                }
            }
        }
    }
}

impl Default for MemoryDescriptorList {
    fn default() -> Self {
        Self::new()
    }
}
