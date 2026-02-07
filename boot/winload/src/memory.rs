//! Работа с памятью и конвертация UEFI memory map в NT формат

use uefi::boot;
use uefi::mem::memory_map::MemoryMap;
use uefi::mem::memory_map::MemoryMapOwned;

use crate::ntcompat::MEMORY_TYPE;
use crate::ntcompat::MemoryDescriptorList;

/// Размер страницы (4KB)
pub const PAGE_SIZE: u64 = 4096;

/// Конвертирует UEFI memory type в NT MEMORY_TYPE.
fn uefi_type_to_nt(uefi_type: uefi::boot::MemoryType) -> MEMORY_TYPE {
    use uefi::boot::MemoryType as UefiMem;

    match uefi_type {
        UefiMem::RESERVED => MEMORY_TYPE::LoaderFirmwarePermanent,
        UefiMem::LOADER_CODE => MEMORY_TYPE::LoaderOsloaderHeap,
        UefiMem::LOADER_DATA => MEMORY_TYPE::LoaderOsloaderHeap,
        UefiMem::BOOT_SERVICES_CODE => MEMORY_TYPE::LoaderFirmwareTemporary,
        UefiMem::BOOT_SERVICES_DATA => MEMORY_TYPE::LoaderFirmwareTemporary,
        UefiMem::RUNTIME_SERVICES_CODE => MEMORY_TYPE::LoaderFirmwarePermanent,
        UefiMem::RUNTIME_SERVICES_DATA => MEMORY_TYPE::LoaderFirmwarePermanent,
        UefiMem::CONVENTIONAL => MEMORY_TYPE::LoaderFree,
        UefiMem::UNUSABLE => MEMORY_TYPE::LoaderBad,
        UefiMem::ACPI_RECLAIM => MEMORY_TYPE::LoaderFirmwareTemporary,
        UefiMem::ACPI_NON_VOLATILE => MEMORY_TYPE::LoaderFirmwarePermanent,
        UefiMem::MMIO => MEMORY_TYPE::LoaderFirmwarePermanent,
        UefiMem::MMIO_PORT_SPACE => MEMORY_TYPE::LoaderFirmwarePermanent,
        UefiMem::PAL_CODE => MEMORY_TYPE::LoaderFirmwarePermanent,
        UefiMem::PERSISTENT_MEMORY => MEMORY_TYPE::LoaderFree,
        _ => MEMORY_TYPE::LoaderSpecialMemory,
    }
}

/// Конвертирует UEFI memory map в список NT дескрипторов.
pub fn convert_uefi_memory_map(uefi_map: &MemoryMapOwned) -> MemoryDescriptorList {
    let mut list = MemoryDescriptorList::new();

    for entry in uefi_map.entries() {
        let nt_type = uefi_type_to_nt(entry.ty);
        let base_page = entry.phys_start / PAGE_SIZE;
        let page_count = entry.page_count;

        list.add(nt_type, base_page, page_count);
    }

    list
}

/// Информация о UEFI memory map для сохранения после ExitBootServices.
#[derive(Debug)]
pub struct UefiMemoryMapInfo {
    /// Количество записей.
    pub entry_count: usize,
    /// Размер одного дескриптора.
    pub descriptor_size: usize,
    /// Версия дескрипторов.
    pub descriptor_version: u32,
}

/// Получает текущую UEFI memory map.
/// Возвращает (MemoryMapOwned, UefiMemoryMapInfo).
pub fn get_uefi_memory_map() -> Option<(MemoryMapOwned, UefiMemoryMapInfo)> {
    let map = boot::memory_map(boot::MemoryType::LOADER_DATA).ok()?;

    let entry_count = map.entries().count();

    // Получаем дополнительную информацию
    // В uefi-rs нет прямого доступа к descriptor_size, используем размер структуры
    let descriptor_size = core::mem::size_of::<uefi::mem::memory_map::MemoryDescriptor>();

    let info = UefiMemoryMapInfo {
        entry_count,
        descriptor_size,
        descriptor_version: 1, // UEFI spec версия
    };

    Some((map, info))
}

/// Выполняет ExitBootServices.
///
/// После успешного вызова UEFI Boot Services более недоступны!
///
/// Возвращает финальную memory map (уже после EBS) и NT-совместимый список дескрипторов.
pub fn exit_boot_services() -> Result<(MemoryDescriptorList, UefiMemoryMapInfo), &'static str> {
    // Получаем memory map до ExitBootServices для конвертации
    let (pre_map, info) = get_uefi_memory_map().ok_or("Failed to get initial memory map")?;

    // Конвертируем в NT формат до ExitBootServices
    let nt_descriptors = convert_uefi_memory_map(&pre_map);

    // Вызываем ExitBootServices
    // ВАЖНО: После этого вызова мы теряем доступ к Boot Services!
    //
    // uefi-rs exit_boot_services возвращает MemoryMapOwned напрямую (не Result).
    // Он внутри обрабатывает повторы если memory map изменился.
    let _final_map = unsafe { boot::exit_boot_services(Some(boot::MemoryType::LOADER_DATA)) };

    // Успешно! Boot Services больше недоступны
    Ok((nt_descriptors, info))
}

/// Находит регион памяти подходящий для раннего heap.
/// Ищет большой блок CONVENTIONAL памяти.
pub fn find_heap_region(map: &MemoryMapOwned, min_size: u64) -> Option<(u64, u64)> {
    let min_pages = (min_size + PAGE_SIZE - 1) / PAGE_SIZE;

    let mut best_base = 0u64;
    let mut best_size = 0u64;

    for entry in map.entries() {
        if entry.ty == boot::MemoryType::CONVENTIONAL {
            let size_bytes = entry.page_count * PAGE_SIZE;

            // Предпочитаем память выше 1MB но ниже 4GB для совместимости
            if entry.phys_start >= 0x100000
                && entry.phys_start < 0x100000000
                && entry.page_count >= min_pages
            {
                if size_bytes > best_size {
                    best_base = entry.phys_start;
                    best_size = size_bytes;
                }
            }
        }
    }

    if best_size >= min_size {
        Some((best_base, best_size))
    } else {
        None
    }
}

/// Статистика по типам памяти.
#[derive(Debug, Default)]
pub struct MemoryStats {
    pub total_pages: u64,
    pub free_pages: u64,
    pub firmware_pages: u64,
    pub loader_pages: u64,
    pub reserved_pages: u64,
    /// Максимальный физический адрес (конец последнего региона)
    pub max_address: u64,
}

impl MemoryStats {
    pub fn from_uefi_map(map: &MemoryMapOwned) -> Self {
        let mut stats = Self::default();

        for entry in map.entries() {
            stats.total_pages += entry.page_count;

            // Вычисляем максимальный адрес
            let region_end = entry.phys_start + entry.page_count * PAGE_SIZE;
            if region_end > stats.max_address {
                stats.max_address = region_end;
            }

            match entry.ty {
                boot::MemoryType::CONVENTIONAL | boot::MemoryType::PERSISTENT_MEMORY => {
                    stats.free_pages += entry.page_count;
                },
                boot::MemoryType::LOADER_CODE | boot::MemoryType::LOADER_DATA => {
                    stats.loader_pages += entry.page_count;
                },
                boot::MemoryType::RUNTIME_SERVICES_CODE
                | boot::MemoryType::RUNTIME_SERVICES_DATA
                | boot::MemoryType::ACPI_NON_VOLATILE
                | boot::MemoryType::RESERVED => {
                    stats.firmware_pages += entry.page_count;
                },
                boot::MemoryType::BOOT_SERVICES_CODE | boot::MemoryType::BOOT_SERVICES_DATA => {
                    // Boot services memory - станет free после ExitBootServices
                    stats.firmware_pages += entry.page_count;
                },
                _ => {
                    stats.reserved_pages += entry.page_count;
                },
            }
        }

        stats
    }

    pub fn total_bytes(&self) -> u64 {
        self.total_pages * PAGE_SIZE
    }

    pub fn free_bytes(&self) -> u64 {
        self.free_pages * PAGE_SIZE
    }

    /// Максимальный физический адрес памяти в системе.
    pub fn max_physical_address(&self) -> u64 {
        self.max_address
    }
}
