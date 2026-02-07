//! PE Loader для boot drivers и ядра
//!
//! Загружает PE32+ образы в память, применяет relocations,
//! формирует записи LoadedModule (аналог LDR_DATA_TABLE_ENTRY).

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

use uefi::boot;

// Типы и функции из PE библиотеки
use pe::{
    read_u16, read_u32, read_u64, PeImage,
    IMAGE_DIRECTORY_ENTRY_BASERELOC, IMAGE_DIRECTORY_ENTRY_EXPORT, IMAGE_DIRECTORY_ENTRY_IMPORT,
    IMAGE_FILE_MACHINE_AMD64, IMAGE_ORDINAL_FLAG64, IMAGE_REL_BASED_ABSOLUTE, IMAGE_REL_BASED_DIR64,
    IMAGE_SCN_CNT_CODE, IMAGE_SCN_MEM_EXECUTE, IMAGE_SCN_MEM_READ, IMAGE_SCN_MEM_WRITE,
};

// =============================================================================
// Структуры загруженного модуля
// =============================================================================

/// Информация о загруженном модуле (аналог LDR_DATA_TABLE_ENTRY).
/// Используется для построения LoadOrderListHead.
#[derive(Debug)]
pub struct LoadedModule {
    /// Физический базовый адрес образа (куда скопирован через AllocatePages).
    /// Используется для доступа к образу до переключения на kernel page tables.
    pub phys_base: u64,

    /// Виртуальный базовый адрес (VA, по которому образ будет исполняться).
    /// Используется для relocations и export/import resolution.
    pub va_base: u64,

    /// Entry point RVA (относительно базы).
    pub entry_rva: u32,

    /// Размер образа в памяти.
    pub size_of_image: u32,

    /// Количество страниц, выделенных под образ.
    pub page_count: usize,

    /// Имя модуля (без пути).
    pub name: String,

    /// Полный путь к модулю.
    pub full_path: String,

    /// Флаги (для совместимости с NT).
    pub flags: u32,

    /// Checksum из PE header.
    pub checksum: u32,

    /// TimeDateStamp из PE header.
    pub time_date_stamp: u32,
}

impl LoadedModule {
    /// Возвращает физический адрес entry point.
    #[inline]
    pub fn entry_point_phys(&self) -> u64 {
        self.phys_base + self.entry_rva as u64
    }

    /// Возвращает виртуальный адрес entry point.
    #[inline]
    pub fn entry_point_va(&self) -> u64 {
        self.va_base + self.entry_rva as u64
    }

    /// Возвращает slice образа в физической памяти.
    ///
    /// # Safety
    /// Caller должен гарантировать, что phys_base и size_of_image валидны.
    pub unsafe fn image_slice(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(self.phys_base as *const u8, self.size_of_image as usize)
        }
    }

    /// Возвращает mutable slice образа в физической памяти.
    ///
    /// # Safety
    /// Caller должен гарантировать, что phys_base и size_of_image валидны,
    /// и что нет других ссылок на эту память.
    pub unsafe fn image_slice_mut(&self) -> &mut [u8] {
        unsafe {
            core::slice::from_raw_parts_mut(self.phys_base as *mut u8, self.size_of_image as usize)
        }
    }
}

/// Информация о секции PE файла.
#[derive(Debug, Clone)]
pub struct SectionInfo {
    /// Имя секции (до 8 символов).
    pub name: [u8; 8],
    /// Virtual size (размер в памяти).
    pub virtual_size: u32,
    /// Virtual address (RVA).
    pub virtual_address: u32,
    /// Size of raw data (размер в файле).
    pub size_of_raw_data: u32,
    /// Pointer to raw data (offset в файле).
    pub pointer_to_raw_data: u32,
    /// Characteristics (флаги).
    pub characteristics: u32,
}

/// Результат парсинга PE заголовков.
#[derive(Debug)]
pub struct PeHeaders {
    /// Machine type.
    pub machine: u16,
    /// Number of sections.
    pub number_of_sections: u16,
    /// TimeDateStamp.
    pub time_date_stamp: u32,
    /// Size of optional header.
    pub size_of_optional_header: u16,
    /// Characteristics.
    pub characteristics: u16,

    // Optional header (PE32+)
    /// Magic (0x20B for PE32+).
    pub magic: u16,
    /// Entry point RVA.
    pub address_of_entry_point: u32,
    /// Image base.
    pub image_base: u64,
    /// Section alignment.
    pub section_alignment: u32,
    /// File alignment.
    pub file_alignment: u32,
    /// Size of image.
    pub size_of_image: u32,
    /// Size of headers.
    pub size_of_headers: u32,
    /// Checksum.
    pub checksum: u32,
    /// Number of RVA and sizes (data directories count).
    pub number_of_rva_and_sizes: u32,

    /// Data directories (RVA, Size pairs).
    pub data_directories: Vec<(u32, u32)>,

    /// Sections.
    pub sections: Vec<SectionInfo>,

    /// Offset к первой секции в файле.
    pub first_section_offset: usize,
}

/// Ошибка загрузки PE.
#[derive(Debug)]
pub enum LoadError {
    /// Файл слишком маленький.
    FileTooSmall,
    /// Неверная DOS сигнатура.
    InvalidDosSignature,
    /// Неверная PE сигнатура.
    InvalidPeSignature,
    /// Не PE32+ формат.
    NotPe32Plus,
    /// Неподдерживаемая архитектура.
    UnsupportedMachine,
    /// Ошибка аллокации памяти.
    AllocationFailed,
    /// Ошибка применения relocations.
    RelocationFailed,
}

// =============================================================================
// Парсинг PE заголовков
// =============================================================================

/// Парсит PE заголовки без загрузки образа.
/// Использует pe::PeImage для базового парсинга и извлекает данные в локальную структуру.
pub fn parse_pe_headers(data: &[u8]) -> Result<PeHeaders, LoadError> {
    // Используем PeImage из pe crate для парсинга
    let pe = PeImage::parse(data).map_err(|e| match e {
        pe::PeError::BufferTooSmall => LoadError::FileTooSmall,
        pe::PeError::InvalidDosSignature => LoadError::InvalidDosSignature,
        pe::PeError::InvalidPeSignature => LoadError::InvalidPeSignature,
        _ => LoadError::FileTooSmall,
    })?;

    // Получаем file header
    let file_header = pe.file_header().map_err(|_| LoadError::FileTooSmall)?;

    // Проверяем архитектуру
    if file_header.machine != IMAGE_FILE_MACHINE_AMD64 {
        return Err(LoadError::UnsupportedMachine);
    }

    // Проверяем что это PE32+
    if !pe.is_pe32_plus().map_err(|_| LoadError::FileTooSmall)? {
        return Err(LoadError::NotPe32Plus);
    }

    // Получаем optional header (PE32+)
    let opt_header = pe.optional_header_64().map_err(|_| LoadError::NotPe32Plus)?;

    // Читаем data directories
    let mut data_directories = Vec::new();
    for i in 0..opt_header.number_of_rva_and_sizes as usize {
        match pe.data_directory(i) {
            Ok(dd) => data_directories.push((dd.virtual_address, dd.size)),
            Err(_) => data_directories.push((0, 0)),
        }
    }

    // Читаем секции
    let mut sections = Vec::new();
    for section in pe.sections().map_err(|_| LoadError::FileTooSmall)? {
        sections.push(SectionInfo {
            name: section.name,
            virtual_size: section.virtual_size,
            virtual_address: section.virtual_address,
            size_of_raw_data: section.size_of_raw_data,
            pointer_to_raw_data: section.pointer_to_raw_data,
            characteristics: section.characteristics,
        });
    }

    Ok(PeHeaders {
        machine: file_header.machine,
        number_of_sections: file_header.number_of_sections,
        time_date_stamp: file_header.time_date_stamp,
        size_of_optional_header: file_header.size_of_optional_header,
        characteristics: file_header.characteristics,
        magic: opt_header.magic,
        address_of_entry_point: opt_header.address_of_entry_point,
        image_base: opt_header.image_base,
        section_alignment: opt_header.section_alignment,
        file_alignment: opt_header.file_alignment,
        size_of_image: opt_header.size_of_image,
        size_of_headers: opt_header.size_of_headers,
        checksum: opt_header.checksum,
        number_of_rva_and_sizes: opt_header.number_of_rva_and_sizes,
        data_directories,
        sections,
        first_section_offset: pe.sections_offset().map_err(|_| LoadError::FileTooSmall)?,
    })
}

// =============================================================================
// Загрузка PE образа
// =============================================================================

/// Загружает PE образ в память.
///
/// # Аргументы
/// * `data` - сырые данные PE файла
/// * `name` - имя модуля
/// * `full_path` - полный путь к файлу
/// * `va_base` - целевой виртуальный адрес (для relocations)
///
/// # Возвращает
/// LoadedModule с загруженным образом или ошибку.
pub fn load_pe_image(
    data: &[u8],
    name: String,
    full_path: String,
    va_base: u64,
) -> Result<LoadedModule, LoadError> {
    // Парсим заголовки
    let headers = parse_pe_headers(data)?;

    // Аллоцируем память под образ с выравниванием на 4KB страницу
    // Используем UEFI AllocatePages для гарантированного выравнивания
    let image_size = headers.size_of_image as usize;
    let page_count = (image_size + 4095) / 4096;

    let image_alloc = boot::allocate_pages(
        boot::AllocateType::AnyPages,
        boot::MemoryType::LOADER_DATA,
        page_count,
    )
    .map_err(|_| LoadError::AllocationFailed)?;

    // Создаём slice из выделенной памяти
    let image: &mut [u8] =
        unsafe { core::slice::from_raw_parts_mut(image_alloc.as_ptr(), image_size) };

    // Очищаем память
    image.fill(0);

    // Копируем заголовки
    let headers_size = headers.size_of_headers as usize;
    if headers_size > data.len() || headers_size > image_size {
        return Err(LoadError::FileTooSmall);
    }
    image[..headers_size].copy_from_slice(&data[..headers_size]);

    // Копируем секции
    for section in &headers.sections {
        let src_offset = section.pointer_to_raw_data as usize;
        let dst_offset = section.virtual_address as usize;
        let copy_size = section.size_of_raw_data as usize;

        if src_offset + copy_size > data.len() {
            continue; // Секция может быть пустой или некорректной
        }

        if dst_offset + copy_size > image_size {
            continue; // Секция выходит за пределы образа
        }

        image[dst_offset..dst_offset + copy_size]
            .copy_from_slice(&data[src_offset..src_offset + copy_size]);

        // Заполняем оставшееся место нулями (для bss-подобных секций)
        let virtual_size = section.virtual_size as usize;
        if virtual_size > copy_size && dst_offset + virtual_size <= image_size {
            image[dst_offset + copy_size..dst_offset + virtual_size].fill(0);
        }
    }

    // Физический базовый адрес загруженного образа
    let phys_base = image_alloc.as_ptr() as u64;

    // Применяем relocations под целевой VA
    let delta = va_base as i64 - headers.image_base as i64;
    if delta != 0 {
        apply_relocations(image, &headers, delta)?;
    }

    Ok(LoadedModule {
        phys_base,
        va_base,
        entry_rva: headers.address_of_entry_point,
        size_of_image: headers.size_of_image,
        page_count,
        name,
        full_path,
        flags: 0,
        checksum: headers.checksum,
        time_date_stamp: headers.time_date_stamp,
    })
}

// =============================================================================
// Base Relocations
// =============================================================================

/// Применяет base relocations к загруженному образу.
fn apply_relocations(image: &mut [u8], headers: &PeHeaders, delta: i64) -> Result<(), LoadError> {
    // Получаем directory entry для relocations
    if headers.data_directories.len() <= IMAGE_DIRECTORY_ENTRY_BASERELOC {
        return Ok(()); // Нет relocations - OK
    }

    let (reloc_rva, reloc_size) = headers.data_directories[IMAGE_DIRECTORY_ENTRY_BASERELOC];

    if reloc_rva == 0 || reloc_size == 0 {
        return Ok(()); // Нет relocations - OK
    }

    let mut offset = reloc_rva as usize;
    let end_offset = offset + reloc_size as usize;

    // Relocation block format:
    // +0: VirtualAddress (4 bytes) - base RVA для этого блока
    // +4: SizeOfBlock (4 bytes) - размер блока включая header
    // +8: TypeOffset entries (each 2 bytes) - type (4 bits) + offset (12 bits)

    while offset + 8 <= end_offset && offset + 8 <= image.len() {
        let block_rva = read_u32(image, offset).ok_or(LoadError::RelocationFailed)?;
        let block_size = read_u32(image, offset + 4).ok_or(LoadError::RelocationFailed)?;

        if block_size < 8 {
            break; // Некорректный блок
        }

        let num_entries = (block_size as usize - 8) / 2;

        for i in 0..num_entries {
            let entry_offset = offset + 8 + i * 2;
            if entry_offset + 2 > image.len() {
                break;
            }

            let entry = read_u16(image, entry_offset).ok_or(LoadError::RelocationFailed)?;
            let reloc_type = (entry >> 12) as u16;
            let reloc_offset = (entry & 0x0FFF) as u32;

            let target_rva = block_rva + reloc_offset;
            let target_offset = target_rva as usize;

            match reloc_type {
                IMAGE_REL_BASED_ABSOLUTE => {
                    // Ничего не делаем - padding
                },
                IMAGE_REL_BASED_DIR64 => {
                    // 64-bit relocation
                    if target_offset + 8 <= image.len() {
                        let value =
                            read_u64(image, target_offset).ok_or(LoadError::RelocationFailed)?;
                        let new_value = (value as i64 + delta) as u64;
                        image[target_offset..target_offset + 8]
                            .copy_from_slice(&new_value.to_le_bytes());
                    }
                },
                _ => {
                    // Неподдерживаемый тип relocation - пропускаем
                },
            }
        }

        offset += block_size as usize;
    }

    Ok(())
}

// =============================================================================
// Список загруженных модулей
// =============================================================================

/// Список загруженных модулей (аналог LoadOrderListHead).
pub struct LoadOrderList {
    /// Модули в порядке загрузки.
    pub modules: Vec<LoadedModule>,
}

impl LoadOrderList {
    /// Создаёт пустой список.
    pub fn new() -> Self {
        LoadOrderList {
            modules: Vec::new(),
        }
    }

    /// Добавляет модуль в список.
    pub fn add(&mut self, module: LoadedModule) {
        self.modules.push(module);
    }

    /// Возвращает количество загруженных модулей.
    pub fn count(&self) -> usize {
        self.modules.len()
    }

    /// Ищет модуль по имени (case-insensitive).
    pub fn find_by_name(&self, name: &str) -> Option<&LoadedModule> {
        let name_upper = name.to_uppercase();
        self.modules
            .iter()
            .find(|m| m.name.to_uppercase() == name_upper)
    }
}

impl Default for LoadOrderList {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Вспомогательные функции для отладки
// =============================================================================

impl SectionInfo {
    /// Возвращает имя секции как строку.
    pub fn name_str(&self) -> &str {
        let len = self.name.iter().position(|&c| c == 0).unwrap_or(8);
        core::str::from_utf8(&self.name[..len]).unwrap_or("???")
    }

    /// Проверяет, является ли секция исполняемой.
    #[allow(dead_code)]
    pub fn is_executable(&self) -> bool {
        self.characteristics & IMAGE_SCN_MEM_EXECUTE != 0
            || self.characteristics & IMAGE_SCN_CNT_CODE != 0
    }

    /// Проверяет, является ли секция записываемой.
    #[allow(dead_code)]
    pub fn is_writable(&self) -> bool {
        self.characteristics & IMAGE_SCN_MEM_WRITE != 0
    }

    /// Проверяет, является ли секция читаемой.
    #[allow(dead_code)]
    pub fn is_readable(&self) -> bool {
        self.characteristics & IMAGE_SCN_MEM_READ != 0
    }
}

// =============================================================================
// Export Index (для резолва импортов)
// =============================================================================

/// Ошибка резолва импортов.
#[derive(Debug)]
pub enum ImportError {
    /// Модуль-экспортёр не найден.
    ModuleNotFound(String),
    /// Символ не найден в экспортах.
    SymbolNotFound(String),
    /// Некорректный формат export directory.
    InvalidExportDirectory,
    /// Некорректный формат import directory.
    InvalidImportDirectory,
}

/// Индекс экспортов модуля для быстрого резолва.
///
/// Хранит ссылки на export directory и позволяет резолвить
/// символы по имени или ordinal.
pub struct ExportIndex<'a> {
    /// Ссылка на загруженный модуль
    module: &'a LoadedModule,

    /// Slice образа в памяти (физический адрес)
    image: &'a [u8],

    /// Export Directory RVA
    export_dir_rva: u32,

    /// Export Directory Size
    export_dir_size: u32,

    // Кешированные поля из IMAGE_EXPORT_DIRECTORY
    /// Base ordinal
    ordinal_base: u32,
    /// Количество функций в EAT
    number_of_functions: u32,
    /// Количество именованных экспортов
    number_of_names: u32,
    /// RVA таблицы адресов (EAT)
    address_of_functions: u32,
    /// RVA таблицы имён
    address_of_names: u32,
    /// RVA таблицы ординалов
    address_of_name_ordinals: u32,
}

impl<'a> ExportIndex<'a> {
    /// Создаёт индекс экспортов для модуля.
    ///
    /// Возвращает None если модуль не имеет export directory.
    pub fn new(module: &'a LoadedModule, image: &'a [u8]) -> Option<Self> {
        // Парсим PE headers чтобы получить data directories
        let headers = parse_pe_headers(image).ok()?;

        // Получаем export directory
        if headers.data_directories.len() <= IMAGE_DIRECTORY_ENTRY_EXPORT {
            return None;
        }

        let (export_dir_rva, export_dir_size) =
            headers.data_directories[IMAGE_DIRECTORY_ENTRY_EXPORT];
        if export_dir_rva == 0 || export_dir_size == 0 {
            return None;
        }

        // Читаем IMAGE_EXPORT_DIRECTORY
        let dir_offset = export_dir_rva as usize;
        if dir_offset + 40 > image.len() {
            return None;
        }

        // IMAGE_EXPORT_DIRECTORY layout:
        // +0x00: Characteristics (4)
        // +0x04: TimeDateStamp (4)
        // +0x08: MajorVersion (2)
        // +0x0A: MinorVersion (2)
        // +0x0C: Name (4) - RVA to DLL name
        // +0x10: Base (4) - Ordinal base
        // +0x14: NumberOfFunctions (4)
        // +0x18: NumberOfNames (4)
        // +0x1C: AddressOfFunctions (4) - RVA to EAT
        // +0x20: AddressOfNames (4) - RVA to name pointers
        // +0x24: AddressOfNameOrdinals (4) - RVA to ordinal table

        let ordinal_base = read_u32(image, dir_offset + 0x10)?;
        let number_of_functions = read_u32(image, dir_offset + 0x14)?;
        let number_of_names = read_u32(image, dir_offset + 0x18)?;
        let address_of_functions = read_u32(image, dir_offset + 0x1C)?;
        let address_of_names = read_u32(image, dir_offset + 0x20)?;
        let address_of_name_ordinals = read_u32(image, dir_offset + 0x24)?;

        Some(ExportIndex {
            module,
            image,
            export_dir_rva,
            export_dir_size,
            ordinal_base,
            number_of_functions,
            number_of_names,
            address_of_functions,
            address_of_names,
            address_of_name_ordinals,
        })
    }

    /// Резолвит экспорт по имени.
    ///
    /// Возвращает VA экспортированной функции или None.
    pub fn resolve_by_name(&self, name: &str) -> Option<u64> {
        // Бинарный поиск в таблице имён
        let names_table = self.address_of_names as usize;
        let ordinals_table = self.address_of_name_ordinals as usize;
        let eat = self.address_of_functions as usize;

        let mut low = 0usize;
        let mut high = self.number_of_names as usize;

        while low < high {
            let mid = (low + high) / 2;

            // Читаем RVA имени
            let name_rva_offset = names_table + mid * 4;
            let name_rva = read_u32(self.image, name_rva_offset)? as usize;

            // Читаем строку имени
            let export_name = self.read_cstring(name_rva)?;

            match export_name.as_str().cmp(name) {
                core::cmp::Ordering::Equal => {
                    // Нашли! Читаем ordinal
                    let ordinal_offset = ordinals_table + mid * 2;
                    let ordinal = read_u16(self.image, ordinal_offset)? as usize;

                    // Читаем RVA функции из EAT
                    let func_rva_offset = eat + ordinal * 4;
                    let func_rva = read_u32(self.image, func_rva_offset)?;

                    // Проверяем на forwarder (RVA внутри export directory)
                    if func_rva >= self.export_dir_rva
                        && func_rva < self.export_dir_rva + self.export_dir_size
                    {
                        // Forwarder - пока не поддерживаем
                        return None;
                    }

                    // Возвращаем VA = va_base + rva
                    return Some(self.module.va_base + func_rva as u64);
                },
                core::cmp::Ordering::Less => {
                    low = mid + 1;
                },
                core::cmp::Ordering::Greater => {
                    high = mid;
                },
            }
        }

        None
    }

    /// Резолвит экспорт по ordinal.
    ///
    /// Возвращает VA экспортированной функции или None.
    pub fn resolve_by_ordinal(&self, ordinal: u32) -> Option<u64> {
        // Вычисляем индекс в EAT
        if ordinal < self.ordinal_base {
            return None;
        }
        let index = (ordinal - self.ordinal_base) as usize;

        if index >= self.number_of_functions as usize {
            return None;
        }

        // Читаем RVA функции из EAT
        let eat = self.address_of_functions as usize;
        let func_rva_offset = eat + index * 4;
        let func_rva = read_u32(self.image, func_rva_offset)?;

        // Проверяем на forwarder
        if func_rva >= self.export_dir_rva && func_rva < self.export_dir_rva + self.export_dir_size
        {
            // Forwarder - пока не поддерживаем
            return None;
        }

        // Возвращаем VA = va_base + rva
        Some(self.module.va_base + func_rva as u64)
    }

    /// Читает null-terminated строку из образа.
    fn read_cstring(&self, offset: usize) -> Option<String> {
        if offset >= self.image.len() {
            return None;
        }

        let mut end = offset;
        while end < self.image.len() && self.image[end] != 0 {
            end += 1;
        }

        String::from_utf8(self.image[offset..end].to_vec()).ok()
    }
}

// =============================================================================
// Import Resolver (IAT Fixups)
// =============================================================================

/// Получает список DLL-зависимостей из Import Directory PE-файла.
///
/// Используется для загрузки зависимых модулей перед резолвом импортов.
///
/// # Аргументы
/// * `image` - slice образа PE-файла
///
/// # Возвращает
/// Vec<String> с именами DLL (без расширения, uppercase)
pub fn get_import_dependencies(image: &[u8]) -> Vec<String> {
    let mut deps = Vec::new();

    // Парсим PE headers
    let headers = match parse_pe_headers(image) {
        Ok(h) => h,
        Err(_) => return deps,
    };

    // Получаем import directory
    if headers.data_directories.len() <= IMAGE_DIRECTORY_ENTRY_IMPORT {
        return deps;
    }

    let (import_dir_rva, import_dir_size) = headers.data_directories[IMAGE_DIRECTORY_ENTRY_IMPORT];
    if import_dir_rva == 0 || import_dir_size == 0 {
        return deps;
    }

    // Проходим по IMAGE_IMPORT_DESCRIPTOR[]
    let mut desc_offset = import_dir_rva as usize;

    loop {
        if desc_offset + 20 > image.len() {
            break;
        }

        let name_rva = read_u32(image, desc_offset + 12).unwrap_or(0);
        let first_thunk = read_u32(image, desc_offset + 16).unwrap_or(0);

        // Нулевой дескриптор = конец
        if name_rva == 0 && first_thunk == 0 {
            break;
        }

        // Читаем имя DLL
        if let Some(dll_name) = read_cstring_at(image, name_rva as usize) {
            // Нормализуем: uppercase, без расширения
            let dll_name_upper = dll_name.to_uppercase();
            let dll_base_name = dll_name_upper
                .strip_suffix(".DLL")
                .or_else(|| dll_name_upper.strip_suffix(".EXE"))
                .or_else(|| dll_name_upper.strip_suffix(".SYS"))
                .unwrap_or(&dll_name_upper)
                .to_string();

            // Добавляем если ещё не добавлен
            if !deps.contains(&dll_base_name) {
                deps.push(dll_base_name);
            }
        }

        desc_offset += 20;
    }

    deps
}

/// Резолвит импорты модуля и патчит IAT.
///
/// # Аргументы
/// * `module` - модуль, для которого резолвим импорты
/// * `image` - mutable slice образа модуля в физической памяти
/// * `loaded_modules` - список загруженных модулей для поиска экспортёров
///
/// # Возвращает
/// Ok(()) если все импорты резолвлены, или ImportError.
pub fn resolve_imports(
    module: &LoadedModule,
    image: &mut [u8],
    loaded_modules: &LoadOrderList,
) -> Result<(), ImportError> {
    // Парсим PE headers
    let headers = parse_pe_headers(image).map_err(|_| ImportError::InvalidImportDirectory)?;

    // Получаем import directory
    if headers.data_directories.len() <= IMAGE_DIRECTORY_ENTRY_IMPORT {
        return Ok(()); // Нет импортов - OK
    }

    let (import_dir_rva, import_dir_size) = headers.data_directories[IMAGE_DIRECTORY_ENTRY_IMPORT];
    if import_dir_rva == 0 || import_dir_size == 0 {
        return Ok(()); // Нет импортов - OK
    }

    // Проходим по IMAGE_IMPORT_DESCRIPTOR[]
    let mut desc_offset = import_dir_rva as usize;

    // IMAGE_IMPORT_DESCRIPTOR layout (20 bytes):
    // +0x00: OriginalFirstThunk (4) - RVA to INT (Import Name Table)
    // +0x04: TimeDateStamp (4)
    // +0x08: ForwarderChain (4)
    // +0x0C: Name (4) - RVA to DLL name
    // +0x10: FirstThunk (4) - RVA to IAT

    loop {
        if desc_offset + 20 > image.len() {
            break;
        }

        let original_first_thunk = read_u32(image, desc_offset).unwrap_or(0);
        let name_rva = read_u32(image, desc_offset + 12).unwrap_or(0);
        let first_thunk = read_u32(image, desc_offset + 16).unwrap_or(0);

        // Проверяем на нулевой дескриптор (конец списка)
        if name_rva == 0 && first_thunk == 0 {
            break;
        }

        // Читаем имя DLL
        let dll_name =
            read_cstring_at(image, name_rva as usize).ok_or(ImportError::InvalidImportDirectory)?;

        // Убираем расширение для поиска
        let dll_name_upper = dll_name.to_uppercase();
        let dll_base_name = dll_name_upper
            .strip_suffix(".DLL")
            .or_else(|| dll_name_upper.strip_suffix(".EXE"))
            .or_else(|| dll_name_upper.strip_suffix(".SYS"))
            .unwrap_or(&dll_name_upper);

        // Ищем модуль-экспортёр
        let exporter = loaded_modules
            .find_by_name(dll_base_name)
            .or_else(|| loaded_modules.find_by_name(&dll_name))
            .ok_or_else(|| ImportError::ModuleNotFound(dll_name.clone()))?;

        // Создаём ExportIndex для экспортёра
        let exporter_image = unsafe { exporter.image_slice() };
        let export_index = ExportIndex::new(exporter, exporter_image)
            .ok_or(ImportError::InvalidExportDirectory)?;

        // Выбираем источник имён: INT или IAT
        let int_rva = if original_first_thunk != 0 {
            original_first_thunk
        } else {
            first_thunk
        };

        // Проходим по thunks
        let mut thunk_idx = 0usize;
        loop {
            let int_offset = int_rva as usize + thunk_idx * 8;
            let iat_offset = first_thunk as usize + thunk_idx * 8;

            if int_offset + 8 > image.len() || iat_offset + 8 > image.len() {
                break;
            }

            let thunk = read_u64(image, int_offset).unwrap_or(0);

            // Нулевой thunk = конец списка
            if thunk == 0 {
                break;
            }

            // Резолвим импорт
            let resolved_va = if (thunk & IMAGE_ORDINAL_FLAG64) != 0 {
                // Import by ordinal
                let ordinal = (thunk & 0xFFFF) as u32;
                export_index.resolve_by_ordinal(ordinal).ok_or_else(|| {
                    ImportError::SymbolNotFound(alloc::format!("ordinal {}", ordinal))
                })?
            } else {
                // Import by name
                // thunk = RVA to IMAGE_IMPORT_BY_NAME
                let hint_name_rva = thunk as usize;
                if hint_name_rva + 2 >= image.len() {
                    return Err(ImportError::InvalidImportDirectory);
                }

                // IMAGE_IMPORT_BY_NAME: Hint (2 bytes) + Name (null-terminated)
                let symbol_name = read_cstring_at(image, hint_name_rva + 2)
                    .ok_or(ImportError::InvalidImportDirectory)?;

                export_index
                    .resolve_by_name(&symbol_name)
                    .ok_or_else(|| ImportError::SymbolNotFound(symbol_name))?
            };

            // Записываем resolved VA в IAT
            image[iat_offset..iat_offset + 8].copy_from_slice(&resolved_va.to_le_bytes());

            thunk_idx += 1;
        }

        desc_offset += 20;
    }

    Ok(())
}

/// Читает null-terminated строку из образа.
fn read_cstring_at(image: &[u8], offset: usize) -> Option<String> {
    if offset >= image.len() {
        return None;
    }

    let mut end = offset;
    while end < image.len() && image[end] != 0 {
        end += 1;
    }

    String::from_utf8(image[offset..end].to_vec()).ok()
}

// =============================================================================
// LPB List Entry Creation
// =============================================================================

/// Создаёт LOADER_MODULE_ENTRY из LoadedModule.
///
/// Выделяет память через UEFI и заполняет структуру.
/// Возвращает Box для управления временем жизни.
pub fn create_module_entry(module: &LoadedModule) -> alloc::boxed::Box<ntldr::LOADER_MODULE_ENTRY> {
    let mut entry = alloc::boxed::Box::new(ntldr::LOADER_MODULE_ENTRY::empty());

    entry.DllBase = module.va_base;
    entry.EntryPoint = module.entry_point_va();
    entry.SizeOfImage = module.size_of_image;

    // Копируем имя файла (до 259 символов + NUL)
    let name_bytes = module.full_path.as_bytes();
    let copy_len = name_bytes.len().min(259);
    entry.FullDllName[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
    entry.FullDllNameLength = copy_len as u16;

    entry
}

/// Создаёт BOOT_DRIVER_LIST_ENTRY для boot драйвера.
///
/// # Arguments
/// * `driver_info` - информация о драйвере из registry
/// * `module_entry` - указатель на связанный LOADER_MODULE_ENTRY
/// * `control_set` - номер ControlSet для формирования registry path
pub fn create_boot_driver_entry(
    driver_name: &str,
    driver_path: &str,
    module_entry: *mut ntldr::LOADER_MODULE_ENTRY,
    control_set: u32,
) -> alloc::boxed::Box<ntldr::BOOT_DRIVER_LIST_ENTRY> {
    let mut entry = alloc::boxed::Box::new(ntldr::BOOT_DRIVER_LIST_ENTRY::empty());

    // Registry path: \Registry\Machine\System\ControlSet00N\Services\DriverName
    let registry_path = alloc::format!(
        "\\Registry\\Machine\\System\\ControlSet{:03}\\Services\\{}",
        control_set, driver_name
    );
    let reg_bytes = registry_path.as_bytes();
    let reg_len = reg_bytes.len().min(259);
    entry.RegistryPath[..reg_len].copy_from_slice(&reg_bytes[..reg_len]);
    entry.RegistryPathLength = reg_len as u16;

    // File path
    let path_bytes = driver_path.as_bytes();
    let path_len = path_bytes.len().min(259);
    entry.FilePath[..path_len].copy_from_slice(&path_bytes[..path_len]);
    entry.FilePathLength = path_len as u16;

    // Link to module entry
    entry.LdrEntry = module_entry;

    entry
}

/// Связывает LIST_ENTRY в список (вставка в конец).
///
/// # Safety
/// Caller должен гарантировать валидность указателей.
pub unsafe fn list_insert_tail(head: *mut ntldr::LIST_ENTRY, entry: *mut ntldr::LIST_ENTRY) {
    unsafe {
        let blink = (*head).Blink;
        (*entry).Flink = head;
        (*entry).Blink = blink;
        (*blink).Flink = entry;
        (*head).Blink = entry;
    }
}
