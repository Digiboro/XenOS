//! NLS (National Language Support) - парсер XNLS v1
//!
//! Парсит файл unicode.nls в формате XNLS v1 и инициализирует runtime таблицы.
//! Вызывается на раннем этапе загрузки ядра до инициализации Object Manager.

use ntldr::LOADER_PARAMETER_BLOCK;

use crate::kd::dbg_print;
use crate::nt::NTSTATUS;

// =============================================================================
// Константы формата XNLS v1
// =============================================================================

/// Magic заголовка XNLS
const XNLS_MAGIC: [u8; 4] = *b"XNLS";

/// Версия формата v1
const XNLS_VERSION: u16 = 1;

/// Размер заголовка XNLS v1
const XNLS_HEADER_SIZE: usize = 48;

/// Размер записи TOC
const XNLS_TOC_ENTRY_SIZE: usize = 32;

/// Chunk ID: UUP1 (simple upper mapping BMP)
const CHUNK_ID_UUP1: [u8; 4] = *b"UUP1";

/// Chunk ID: ULO1 (simple lower mapping BMP)
const CHUNK_ID_ULO1: [u8; 4] = *b"ULO1";

/// Chunk ID: PRP1 (свойства символов BMP)
const CHUNK_ID_PRP1: [u8; 4] = *b"PRP1";

/// Ожидаемый размер таблицы BMP: 65536 элементов * 2 байта = 128 KB
const BMP_TABLE_SIZE: usize = 65536 * 2;

/// Ожидаемое количество элементов в BMP таблице
const BMP_ELEM_COUNT: u32 = 65536;

/// Ожидаемый размер элемента (u16)
const BMP_ELEM_SIZE: u32 = 2;

// =============================================================================
// NTSTATUS коды ошибок NLS
// =============================================================================

/// NLS данные отсутствуют в LoaderBlock
pub const STATUS_NLS_DATA_MISSING: NTSTATUS = 0xC0000000u32 as i32 | 0x0400;

/// Ошибка парсинга XNLS (неверный magic/version)
pub const STATUS_NLS_PARSE_ERROR: NTSTATUS = 0xC0000000u32 as i32 | 0x0401;

/// Отсутствует обязательный чанк
pub const STATUS_NLS_CHUNK_MISSING: NTSTATUS = 0xC0000000u32 as i32 | 0x0402;

/// Некорректный размер/формат чанка
pub const STATUS_NLS_CHUNK_INVALID: NTSTATUS = 0xC0000000u32 as i32 | 0x0403;

// =============================================================================
// Вспомогательные функции чтения
// =============================================================================

#[inline]
fn read_u16_le(data: &[u8], offset: usize) -> u16 {
    if offset + 2 > data.len() {
        return 0;
    }
    u16::from_le_bytes([data[offset], data[offset + 1]])
}

#[inline]
fn read_u32_le(data: &[u8], offset: usize) -> u32 {
    if offset + 4 > data.len() {
        return 0;
    }
    u32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

#[inline]
fn read_u64_le(data: &[u8], offset: usize) -> u64 {
    if offset + 8 > data.len() {
        return 0;
    }
    u64::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
        data[offset + 4],
        data[offset + 5],
        data[offset + 6],
        data[offset + 7],
    ])
}

// =============================================================================
// Структуры XNLS
// =============================================================================

/// Заголовок XNLS v1 (48 байт)
#[derive(Debug)]
struct XnlsHeader {
    /// File size в байтах
    file_size: u64,
    /// Смещение до TOC
    toc_offset: u64,
    /// Количество чанков
    chunk_count: u32,
    /// Размер записи TOC
    toc_entry_size: u32,
}

/// Запись TOC (32 байта)
#[derive(Debug)]
struct XnlsTocEntry {
    /// ID чанка (4 ASCII байта)
    id: [u8; 4],
    /// Смещение чанка от начала файла
    offset: u64,
    /// Размер чанка в байтах
    length: u64,
    /// Флаги (зависят от чанка)
    #[allow(dead_code)]
    flags: u32,
    /// Размер элемента в байтах
    elem_size: u32,
    /// Количество элементов
    elem_count: u32,
}

/// Результат парсинга XNLS - указатели на таблицы
struct XnlsTables {
    /// Указатель на таблицу UUP1 (upper case)
    upcase: *const u16,
    /// Указатель на таблицу ULO1 (lower case)
    lowcase: *const u16,
    /// Указатель на таблицу PRP1 (свойства)
    props: *const u16,
}

// =============================================================================
// Парсинг XNLS
// =============================================================================

/// Парсит заголовок XNLS
fn parse_header(data: &[u8]) -> Result<XnlsHeader, NTSTATUS> {
    if data.len() < XNLS_HEADER_SIZE {
        dbg_print("[NLS] ERROR: File too small for header\n");
        return Err(STATUS_NLS_PARSE_ERROR);
    }

    // Проверяем magic
    if data[0..4] != XNLS_MAGIC {
        dbg_print("[NLS] ERROR: Invalid magic (expected XNLS)\n");
        return Err(STATUS_NLS_PARSE_ERROR);
    }

    // Проверяем версию
    let version = read_u16_le(data, 0x04);
    if version != XNLS_VERSION {
        dbg_print("[NLS] ERROR: Unsupported version\n");
        return Err(STATUS_NLS_PARSE_ERROR);
    }

    // Проверяем endian
    let endian = data[0x06];
    if endian != 1 {
        dbg_print("[NLS] ERROR: Invalid endian (expected little-endian)\n");
        return Err(STATUS_NLS_PARSE_ERROR);
    }

    // Проверяем header_size
    let header_size = data[0x07] as usize;
    if header_size != XNLS_HEADER_SIZE {
        dbg_print("[NLS] ERROR: Invalid header size\n");
        return Err(STATUS_NLS_PARSE_ERROR);
    }

    let file_size = read_u64_le(data, 0x08);
    let toc_offset = read_u64_le(data, 0x10);
    let chunk_count = read_u32_le(data, 0x18);
    let toc_entry_size = read_u32_le(data, 0x1C);

    // Валидация
    if file_size as usize > data.len() {
        dbg_print("[NLS] ERROR: file_size exceeds buffer\n");
        return Err(STATUS_NLS_PARSE_ERROR);
    }

    if toc_entry_size as usize != XNLS_TOC_ENTRY_SIZE {
        dbg_print("[NLS] ERROR: Invalid TOC entry size\n");
        return Err(STATUS_NLS_PARSE_ERROR);
    }

    Ok(XnlsHeader {
        file_size,
        toc_offset,
        chunk_count,
        toc_entry_size,
    })
}

/// Парсит одну запись TOC
fn parse_toc_entry(data: &[u8], offset: usize) -> Result<XnlsTocEntry, NTSTATUS> {
    if offset + XNLS_TOC_ENTRY_SIZE > data.len() {
        return Err(STATUS_NLS_PARSE_ERROR);
    }

    let mut id = [0u8; 4];
    id.copy_from_slice(&data[offset..offset + 4]);

    Ok(XnlsTocEntry {
        id,
        offset: read_u64_le(data, offset + 0x04),
        length: read_u64_le(data, offset + 0x0C),
        flags: read_u32_le(data, offset + 0x14),
        elem_size: read_u32_le(data, offset + 0x18),
        elem_count: read_u32_le(data, offset + 0x1C),
    })
}

/// Валидирует BMP чанк (UUP1/ULO1/PRP1)
fn validate_bmp_chunk(
    entry: &XnlsTocEntry,
    data: &[u8],
    name: &str,
) -> Result<*const u16, NTSTATUS> {
    // Проверяем elem_size
    if entry.elem_size != BMP_ELEM_SIZE {
        dbg_print("[NLS] ERROR: ");
        dbg_print(name);
        dbg_print(" invalid elem_size\n");
        return Err(STATUS_NLS_CHUNK_INVALID);
    }

    // Проверяем elem_count
    if entry.elem_count != BMP_ELEM_COUNT {
        dbg_print("[NLS] ERROR: ");
        dbg_print(name);
        dbg_print(" invalid elem_count\n");
        return Err(STATUS_NLS_CHUNK_INVALID);
    }

    // Проверяем length
    if entry.length as usize != BMP_TABLE_SIZE {
        dbg_print("[NLS] ERROR: ");
        dbg_print(name);
        dbg_print(" invalid length\n");
        return Err(STATUS_NLS_CHUNK_INVALID);
    }

    // Проверяем что чанк не выходит за границы файла
    let chunk_end = entry.offset as usize + entry.length as usize;
    if chunk_end > data.len() {
        dbg_print("[NLS] ERROR: ");
        dbg_print(name);
        dbg_print(" extends beyond file\n");
        return Err(STATUS_NLS_CHUNK_INVALID);
    }

    // Возвращаем указатель на данные чанка
    let ptr = data.as_ptr().wrapping_add(entry.offset as usize) as *const u16;
    Ok(ptr)
}

/// Парсит XNLS файл и возвращает указатели на таблицы
fn parse_xnls(data: &[u8]) -> Result<XnlsTables, NTSTATUS> {
    // Парсим заголовок
    let header = parse_header(data)?;

    // Ищем нужные чанки
    let mut upcase_ptr: Option<*const u16> = None;
    let mut lowcase_ptr: Option<*const u16> = None;
    let mut props_ptr: Option<*const u16> = None;

    let toc_start = header.toc_offset as usize;

    for i in 0..header.chunk_count {
        let entry_offset = toc_start + (i as usize) * XNLS_TOC_ENTRY_SIZE;
        let entry = parse_toc_entry(data, entry_offset)?;

        if entry.id == CHUNK_ID_UUP1 {
            upcase_ptr = Some(validate_bmp_chunk(&entry, data, "UUP1")?);
        } else if entry.id == CHUNK_ID_ULO1 {
            lowcase_ptr = Some(validate_bmp_chunk(&entry, data, "ULO1")?);
        } else if entry.id == CHUNK_ID_PRP1 {
            props_ptr = Some(validate_bmp_chunk(&entry, data, "PRP1")?);
        }
        // META и другие чанки игнорируем
    }

    // Проверяем что все обязательные чанки найдены
    let upcase = upcase_ptr.ok_or_else(|| {
        dbg_print("[NLS] ERROR: UUP1 chunk not found\n");
        STATUS_NLS_CHUNK_MISSING
    })?;

    let lowcase = lowcase_ptr.ok_or_else(|| {
        dbg_print("[NLS] ERROR: ULO1 chunk not found\n");
        STATUS_NLS_CHUNK_MISSING
    })?;

    let props = props_ptr.ok_or_else(|| {
        dbg_print("[NLS] ERROR: PRP1 chunk not found\n");
        STATUS_NLS_CHUNK_MISSING
    })?;

    Ok(XnlsTables {
        upcase,
        lowcase,
        props,
    })
}

// =============================================================================
// Публичный API
// =============================================================================

/// RtlInitializeNlsFromLoaderBlock
///
/// Инициализирует NLS подсистему из данных LoaderBlock.
/// Должна вызываться на раннем этапе загрузки до OB/case-insensitive операций.
///
/// # Safety
/// LoaderBlock должен содержать валидные указатели UnicodeNlsBase/UnicodeNlsSize.
/// Вызывается только один раз при старте ядра.
pub unsafe fn rtl_initialize_nls_from_loader_block(
    lpb: &LOADER_PARAMETER_BLOCK,
) -> Result<(), NTSTATUS> {
    unsafe {
        // Проверяем наличие данных
        if lpb.UnicodeNlsBase.is_null() || lpb.UnicodeNlsSize == 0 {
            dbg_print("[NLS] ERROR: UnicodeNlsBase is NULL or Size is 0\n");
            return Err(STATUS_NLS_DATA_MISSING);
        }

        let size = lpb.UnicodeNlsSize as usize;
        dbg_print("\n[NLS]   NLS data: base=0x");
        crate::kd::dbg_print_hex(lpb.UnicodeNlsBase as u64);
        dbg_print(", size=");
        crate::kd::dbg_print_num(size as u64);
        dbg_print(" bytes\n");

        // Создаём slice из данных
        let data = core::slice::from_raw_parts(lpb.UnicodeNlsBase, size);

        // Парсим XNLS
        let tables = parse_xnls(data)?;

        // Устанавливаем таблицы в unicode.rs
        crate::rtl::unicode::nls_set_tables(tables.upcase, tables.lowcase, tables.props);

        dbg_print("[NLS]   UUP1 (upcase):  0x");
        crate::kd::dbg_print_hex(tables.upcase as u64);
        dbg_print("\n");
        dbg_print("[NLS]   ULO1 (lowcase): 0x");
        crate::kd::dbg_print_hex(tables.lowcase as u64);
        dbg_print("\n");
        dbg_print("[NLS]   PRP1 (props):   0x");
        crate::kd::dbg_print_hex(tables.props as u64);
        dbg_print("\n");

        Ok(())
    }
}
