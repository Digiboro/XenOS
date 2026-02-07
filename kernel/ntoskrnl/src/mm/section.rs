//! Section/Segment Manager
//!
//! Реализация NT-подобной модели mapped files/images и shared memory.
//!
//! # Архитектура
//!
//! ```text
//! SECTION (kernel object)
//!     |
//!     v
//! SEGMENT
//!     |
//!     v
//! CONTROL_AREA
//!     |
//!     +---> SUBSECTION(s) ---> Prototype PTEs
//!     |
//!     +---> FILE_OBJECT (для file-backed)
//! ```
//!
//! # Типы секций
//!
//! - **Pagefile-backed** — anonymous shared memory
//! - **File-backed (data)** — mapped data files
//! - **Image** — PE executable/DLL mapping
//!
//! # API
//!
//! - `NtCreateSection` — создание секции
//! - `NtOpenSection` — открытие существующей секции
//! - `NtMapViewOfSection` — маппинг в адресное пространство
//! - `NtUnmapViewOfSection` — размаппинг
//!
//! Источники:
//! - ReactOS: mm/ARM3/section.c, mm/ARM3/miarm.h
//! - NT6.1: mm/section.c

use core::ptr;
use core::sync::atomic::AtomicU32;
use core::sync::atomic::AtomicU64;
use core::sync::atomic::Ordering;

use super::pte::*;
use super::types::*;
use super::vad::*;
use crate::io::file::FILE_OBJECT;
use crate::ke::spinlock::KSPIN_LOCK;
use crate::nt::HANDLE;
use crate::nt::INVALID_HANDLE_VALUE;
use crate::nt::LARGE_INTEGER;
use crate::nt::NTSTATUS;
use crate::nt::NULL_HANDLE;
use crate::nt::PVOID;
use crate::nt::STATUS_INVALID_PARAMETER;
use crate::nt::STATUS_NO_MEMORY;
use crate::nt::STATUS_OBJECT_TYPE_MISMATCH;
use crate::nt::STATUS_SECTION_NOT_IMAGE;
use crate::nt::STATUS_SUCCESS;
use crate::nt::ULONG;
use crate::nt::UNICODE_STRING;
use crate::nt::ntdef::ACCESS_MASK;
use crate::ob::init::SyncUnsafeCell;
use crate::ob::life::ob_create_object;
use crate::ob::life::ob_create_object_type;
use crate::ob::life::ob_insert_object;
use crate::ob::refcount::ob_dereference_object;
use crate::ob::refcount::ob_reference_object_by_handle;
use crate::ob::types::GENERIC_MAPPING;
use crate::ob::types::OBJECT_ATTRIBUTES;
use crate::ob::types::OBJECT_TYPE;
use crate::ob::types::OBJECT_TYPE_INITIALIZER;
use crate::ob::types::STANDARD_RIGHTS_ALL;
use crate::ob::types::STANDARD_RIGHTS_EXECUTE;
use crate::ob::types::STANDARD_RIGHTS_READ;
use crate::ob::types::STANDARD_RIGHTS_WRITE;

// =============================================================================
// Section Access Rights
// =============================================================================

/// Права доступа к секциям
pub const SECTION_QUERY: u32 = 0x0001;
pub const SECTION_MAP_WRITE: u32 = 0x0002;
pub const SECTION_MAP_READ: u32 = 0x0004;
pub const SECTION_MAP_EXECUTE: u32 = 0x0008;
pub const SECTION_EXTEND_SIZE: u32 = 0x0010;
pub const SECTION_MAP_EXECUTE_EXPLICIT: u32 = 0x0020;

/// Полный доступ к секции
pub const SECTION_ALL_ACCESS: u32 = STANDARD_RIGHTS_ALL
    | SECTION_QUERY
    | SECTION_MAP_WRITE
    | SECTION_MAP_READ
    | SECTION_MAP_EXECUTE
    | SECTION_EXTEND_SIZE;

// =============================================================================
// Section Object Type
// =============================================================================

/// Тип объекта "Section"
pub static MM_SECTION_OBJECT_TYPE: SyncUnsafeCell<*mut OBJECT_TYPE> =
    SyncUnsafeCell::new(core::ptr::null_mut());

/// Generic mapping для Section
pub static MM_SECTION_GENERIC_MAPPING: GENERIC_MAPPING = GENERIC_MAPPING {
    generic_read: STANDARD_RIGHTS_READ | SECTION_QUERY | SECTION_MAP_READ,
    generic_write: STANDARD_RIGHTS_WRITE | SECTION_MAP_WRITE,
    generic_execute: STANDARD_RIGHTS_EXECUTE | SECTION_MAP_EXECUTE,
    generic_all: SECTION_ALL_ACCESS,
};

/// Проверяет, инициализирован ли Section Object Type
#[inline]
pub fn mm_is_section_type_initialized() -> bool {
    unsafe { !(*MM_SECTION_OBJECT_TYPE.get()).is_null() }
}

/// Возвращает указатель на Section Object Type
#[inline]
pub fn mm_get_section_object_type() -> *mut OBJECT_TYPE {
    unsafe { *MM_SECTION_OBJECT_TYPE.get() }
}

// =============================================================================
// Section Type Constants
// =============================================================================

/// Тип секции
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SECTION_TYPE {
    /// Pagefile-backed (anonymous shared memory)
    PagefileBacked = 0,
    /// Data file mapping
    DataFile = 1,
    /// Image file (PE) mapping
    Image = 2,
    /// Physical memory mapping
    Physical = 3,
}

// =============================================================================
// Section Flags
// =============================================================================

/// Флаги секции (SEC_*)
pub mod section_flags {
    /// Based section (адрес фиксирован)
    pub const SEC_BASED: u32 = 0x00200000;
    /// No change protection
    pub const SEC_NO_CHANGE: u32 = 0x00400000;
    /// Image section (PE)
    pub const SEC_IMAGE: u32 = 0x01000000;
    /// Protected image (code integrity)
    pub const SEC_PROTECTED_IMAGE: u32 = 0x02000000;
    /// Reserve only (no commit)
    pub const SEC_RESERVE: u32 = 0x04000000;
    /// Commit memory
    pub const SEC_COMMIT: u32 = 0x08000000;
    /// No cache
    pub const SEC_NOCACHE: u32 = 0x10000000;
    /// Write combine
    pub const SEC_WRITECOMBINE: u32 = 0x40000000;
    /// Large pages
    pub const SEC_LARGE_PAGES: u32 = 0x80000000;
}

// =============================================================================
// CONTROL_AREA
// =============================================================================

/// CONTROL_AREA — управляющая структура для секции
///
/// Содержит информацию о файле/pagefile и списки подсекций.
/// Одна CONTROL_AREA может разделяться между несколькими SEGMENT.
#[repr(C)]
pub struct CONTROL_AREA {
    /// Указатель на SEGMENT
    pub segment: *mut SEGMENT,

    /// Список SECTION объектов, ссылающихся на эту CA
    /// (для accounting и cleanup)
    pub derived_section_count: AtomicU32,

    /// Количество маппингов (views)
    pub number_of_mapped_views: AtomicU32,

    /// Количество user references
    pub number_of_user_references: AtomicU32,

    /// Флаги (SEC_*)
    pub flags: u32,

    /// Указатель на FILE_OBJECT (для file-backed)
    pub file_object: *mut FILE_OBJECT,

    /// Событие ожидания (для flush и т.п.)
    pub wait_event: PVOID,

    /// Первая subsection
    pub first_subsection: *mut SUBSECTION,

    /// Последняя subsection
    pub last_subsection: *mut SUBSECTION,

    /// Spinlock для синхронизации
    pub lock: KSPIN_LOCK,

    /// Тип секции
    pub section_type: SECTION_TYPE,

    /// Padding
    _reserved: [u8; 3],
}

impl CONTROL_AREA {
    pub const fn new() -> Self {
        Self {
            segment: ptr::null_mut(),
            derived_section_count: AtomicU32::new(0),
            number_of_mapped_views: AtomicU32::new(0),
            number_of_user_references: AtomicU32::new(0),
            flags: 0,
            file_object: ptr::null_mut(),
            wait_event: ptr::null_mut(),
            first_subsection: ptr::null_mut(),
            last_subsection: ptr::null_mut(),
            lock: KSPIN_LOCK::new(),
            section_type: SECTION_TYPE::PagefileBacked,
            _reserved: [0; 3],
        }
    }

    /// Проверяет, является ли секция image
    #[inline]
    pub fn is_image(&self) -> bool {
        self.section_type == SECTION_TYPE::Image
    }

    /// Проверяет, является ли секция file-backed
    #[inline]
    pub fn is_file_backed(&self) -> bool {
        !self.file_object.is_null()
    }
}

// =============================================================================
// SEGMENT
// =============================================================================

/// SEGMENT — описание сегмента памяти секции
///
/// Содержит информацию о размере и prototype PTEs.
#[repr(C)]
pub struct SEGMENT {
    /// Указатель на CONTROL_AREA
    pub control_area: *mut CONTROL_AREA,

    /// Общий размер сегмента (в байтах)
    pub total_number_of_ptes: u64,

    /// Размер сегмента (для accounting)
    pub segment_size: LARGE_INTEGER,

    /// Commit charge для pagefile-backed
    pub committed_pages: AtomicU64,

    /// Пиковый commit
    pub peak_committed_pages: u64,

    /// Флаги сегмента
    pub segment_flags: u32,

    /// Количество images based on this segment
    pub number_of_images: AtomicU32,

    /// System image base (для image sections)
    pub system_image_base: u64,

    /// Base address hint
    pub based_address: u64,

    /// Указатель на массив prototype PTEs
    pub prototype_pte: *mut MMPTE,

    /// Первый PTE в массиве (для быстрого доступа)
    pub first_prototype_pte: *mut MMPTE,

    /// Последний PTE + 1
    pub last_prototype_pte: *mut MMPTE,
}

impl SEGMENT {
    pub const fn new() -> Self {
        Self {
            control_area: ptr::null_mut(),
            total_number_of_ptes: 0,
            segment_size: LARGE_INTEGER { quad_part: 0 },
            committed_pages: AtomicU64::new(0),
            peak_committed_pages: 0,
            segment_flags: 0,
            number_of_images: AtomicU32::new(0),
            system_image_base: 0,
            based_address: 0,
            prototype_pte: ptr::null_mut(),
            first_prototype_pte: ptr::null_mut(),
            last_prototype_pte: ptr::null_mut(),
        }
    }

    /// Возвращает количество prototype PTEs
    #[inline]
    pub fn pte_count(&self) -> usize {
        self.total_number_of_ptes as usize
    }
}

// =============================================================================
// SUBSECTION
// =============================================================================

/// SUBSECTION — подсекция сегмента
///
/// Для image sections: одна subsection на каждую PE секцию (.text, .data, etc.)
/// Для data files: обычно одна subsection на весь файл.
#[repr(C)]
pub struct SUBSECTION {
    /// Указатель на CONTROL_AREA
    pub control_area: *mut CONTROL_AREA,

    /// Следующая subsection в списке
    pub next_subsection: *mut SUBSECTION,

    /// Первый prototype PTE для этой subsection
    pub subsection_base: *mut MMPTE,

    /// Количество PTEs в этой subsection
    pub ptes_in_subsection: u32,

    /// Флаги подсекции
    pub subsection_flags: SubsectionFlags,

    /// Смещение в файле (в секторах, 512 bytes)
    pub starting_sector: u64,

    /// Количество секторов
    pub number_of_full_sectors: u32,

    /// Padding
    _reserved: u32,
}

/// Флаги подсекции
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct SubsectionFlags {
    value: u32,
}

impl SubsectionFlags {
    pub const fn new() -> Self {
        Self { value: 0 }
    }

    /// Read-only subsection
    #[inline]
    pub fn read_only(&self) -> bool {
        (self.value & 1) != 0
    }

    pub fn set_read_only(&mut self, v: bool) {
        if v {
            self.value |= 1;
        } else {
            self.value &= !1;
        }
    }

    /// Copy-on-write
    #[inline]
    pub fn copy_on_write(&self) -> bool {
        (self.value & 2) != 0
    }

    pub fn set_copy_on_write(&mut self, v: bool) {
        if v {
            self.value |= 2;
        } else {
            self.value &= !2;
        }
    }

    /// Global (shared across processes)
    #[inline]
    pub fn global(&self) -> bool {
        (self.value & 4) != 0
    }

    pub fn set_global(&mut self, v: bool) {
        if v {
            self.value |= 4;
        } else {
            self.value &= !4;
        }
    }

    /// Protection (bits 3-7)
    #[inline]
    pub fn protection(&self) -> u32 {
        (self.value >> 3) & 0x1F
    }

    pub fn set_protection(&mut self, p: u32) {
        self.value = (self.value & !(0x1F << 3)) | ((p & 0x1F) << 3);
    }
}

impl SUBSECTION {
    pub const fn new() -> Self {
        Self {
            control_area: ptr::null_mut(),
            next_subsection: ptr::null_mut(),
            subsection_base: ptr::null_mut(),
            ptes_in_subsection: 0,
            subsection_flags: SubsectionFlags::new(),
            starting_sector: 0,
            number_of_full_sectors: 0,
            _reserved: 0,
        }
    }
}

// =============================================================================
// SECTION — Kernel Object
// =============================================================================

/// SECTION — объект секции (kernel object)
///
/// Это объект, который возвращается из NtCreateSection.
/// Ссылается на SEGMENT через CONTROL_AREA.
#[repr(C)]
pub struct SECTION {
    /// Начальный адрес (для based sections)
    pub starting_va: u64,

    /// Конечный адрес
    pub ending_va: u64,

    /// Указатель на родительскую секцию (для image)
    pub parent: *mut SECTION,

    /// Левый потомок (для дерева секций)
    pub left_child: *mut SECTION,

    /// Правый потомок
    pub right_child: *mut SECTION,

    /// Указатель на SEGMENT
    pub segment: *mut SEGMENT,

    /// Флаги секции
    pub flags: u32,

    /// Initial page protection
    pub initial_page_protection: u32,

    /// Размер секции
    pub size_of_section: LARGE_INTEGER,
}

impl SECTION {
    pub const fn new() -> Self {
        Self {
            starting_va: 0,
            ending_va: 0,
            parent: ptr::null_mut(),
            left_child: ptr::null_mut(),
            right_child: ptr::null_mut(),
            segment: ptr::null_mut(),
            flags: 0,
            initial_page_protection: PAGE_READWRITE,
            size_of_section: LARGE_INTEGER { quad_part: 0 },
        }
    }

    /// Проверяет, является ли секция image
    #[inline]
    pub fn is_image(&self) -> bool {
        (self.flags & section_flags::SEC_IMAGE) != 0
    }

    /// Возвращает CONTROL_AREA
    #[inline]
    pub unsafe fn control_area(&self) -> *mut CONTROL_AREA {
        unsafe {
            if self.segment.is_null() {
                return ptr::null_mut();
            }
            (*self.segment).control_area
        }
    }
}

// =============================================================================
// Section Object Type Initialization
// =============================================================================

/// MmCreateSectionObjectType
///
/// Создает тип объекта "Section" и регистрирует его в Object Manager.
///
/// Вызывается из MmInitSystem Phase 1 после инициализации OB.
///
/// # Returns
/// STATUS_SUCCESS или код ошибки.
pub fn mm_create_section_object_type() -> NTSTATUS {
    // Имя типа "Section"
    static SECTION_TYPE_NAME: [u16; 8] = [
        b'S' as u16,
        b'e' as u16,
        b'c' as u16,
        b't' as u16,
        b'i' as u16,
        b'o' as u16,
        b'n' as u16,
        0,
    ];

    let mut type_name = UNICODE_STRING {
        length: 14,       // 7 символов * 2 байта
        maximum_length: 16,
        buffer: SECTION_TYPE_NAME.as_ptr() as *mut u16,
    };

    // Инициализируем OBJECT_TYPE_INITIALIZER
    let mut init = OBJECT_TYPE_INITIALIZER::new();
    init.length = core::mem::size_of::<OBJECT_TYPE_INITIALIZER>() as u16;

    // Флаги
    init.case_insensitive = true; // Section names case-insensitive
    init.security_required = true; // Security descriptor required

    // Access mask
    init.generic_mapping = MM_SECTION_GENERIC_MAPPING;
    init.valid_access_mask = SECTION_ALL_ACCESS;

    // Pool charges
    init.pool_type = 0; // NonPagedPool
    init.default_non_paged_pool_charge = core::mem::size_of::<SECTION>() as u32;
    init.default_paged_pool_charge = 0;

    // Callbacks
    init.delete_procedure = Some(mi_section_delete_procedure);
    init.close_procedure = Some(mi_section_close_procedure);

    // Создаем тип объекта
    let mut section_type: *mut OBJECT_TYPE = core::ptr::null_mut();

    let status = ob_create_object_type(
        &mut type_name,
        &init,
        core::ptr::null_mut(),
        &mut section_type,
    );

    if status == STATUS_SUCCESS {
        unsafe {
            *MM_SECTION_OBJECT_TYPE.get() = section_type;
        }
    }

    status
}

/// Delete procedure для Section объектов
///
/// Вызывается когда reference count падает до 0.
/// Освобождает CONTROL_AREA, SEGMENT, SUBSECTION и prototype PTEs.
unsafe extern "win64" fn mi_section_delete_procedure(object: PVOID) {
    unsafe {
        if object.is_null() {
            return;
        }

        let section = object as *mut SECTION;

        // Получаем segment
        let segment = (*section).segment;
        if segment.is_null() {
            return;
        }

        // Получаем control area
        let ca = (*segment).control_area;

        // Уменьшаем счетчик секций в control area
        if !ca.is_null() {
            let prev_count = (*ca).derived_section_count.fetch_sub(1, Ordering::AcqRel);

            // Если это была последняя секция, освобождаем control area и связанные структуры
            if prev_count == 1 {
                // Освобождаем subsections
                let mut subsec = (*ca).first_subsection;
                while !subsec.is_null() {
                    let next = (*subsec).next_subsection;

                    // Освобождаем prototype PTEs
                    let ptes = (*subsec).subsection_base;
                    if !ptes.is_null() {
                        mi_free_prototype_ptes(ptes);
                    }

                    mi_free_subsection(subsec);
                    subsec = next;
                }

                // Освобождаем segment
                mi_free_segment(segment);

                // Освобождаем control area
                mi_free_control_area(ca);
            }
        }

        // Section deleted
    }
}

/// Close procedure для Section объектов
///
/// Вызывается при закрытии handle.
unsafe extern "win64" fn mi_section_close_procedure(
    _process: PVOID,
    _object: PVOID,
    _granted_access: u32,
    _process_handle_count: u32,
    _system_handle_count: u32,
) {
    // Для Section основная работа в delete procedure
    // Close procedure может использоваться для accounting
}

// =============================================================================
// Prototype PTE
// =============================================================================

/// Prototype PTE для секций
///
/// Prototype PTE — это PTE в SEGMENT, на который ссылаются процессные PTEs.
/// Позволяет разделять страницы между процессами.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PROTOTYPE_PTE {
    /// Значение PTE
    pub value: u64,
}

impl PROTOTYPE_PTE {
    pub const fn new() -> Self {
        Self { value: 0 }
    }

    /// Demand-zero prototype
    pub const fn demand_zero(protection: u32) -> Self {
        Self {
            value: super::pagefault::pte_state::DEMAND_ZERO | ((protection as u64 & 0x1F) << 5),
        }
    }

    /// Active (в памяти)
    pub fn active(pfn: usize, protection: u64) -> Self {
        Self {
            value: ((pfn as u64) << 12) | protection | PTE_VALID,
        }
    }

    /// Проверяет валидность (страница в памяти)
    #[inline]
    pub fn is_valid(&self) -> bool {
        (self.value & PTE_VALID) != 0
    }

    /// Возвращает PFN
    #[inline]
    pub fn pfn(&self) -> usize {
        ((self.value >> 12) & 0xFFFFFFFFFF) as usize
    }
}

// =============================================================================
// Section Allocation/Deallocation
// =============================================================================

/// Выделяет CONTROL_AREA из pool
pub unsafe fn mi_allocate_control_area() -> *mut CONTROL_AREA {
    unsafe {
        let ptr = crate::ex::pool::ex_allocate_pool_with_tag(
            crate::ex::pool::POOL_TYPE::NonPagedPool,
            core::mem::size_of::<CONTROL_AREA>(),
            u32::from_le_bytes(*b"MmCa"),
        ) as *mut CONTROL_AREA;

        if !ptr.is_null() {
            core::ptr::write(ptr, CONTROL_AREA::new());
        }
        ptr
    }
}

/// Освобождает CONTROL_AREA
pub unsafe fn mi_free_control_area(ca: *mut CONTROL_AREA) {
    if !ca.is_null() {
        crate::ex::pool::ex_free_pool_with_tag(ca as PVOID, u32::from_le_bytes(*b"MmCa"));
    }
}

/// Выделяет SEGMENT из pool
pub unsafe fn mi_allocate_segment() -> *mut SEGMENT {
    unsafe {
        let ptr = crate::ex::pool::ex_allocate_pool_with_tag(
            crate::ex::pool::POOL_TYPE::NonPagedPool,
            core::mem::size_of::<SEGMENT>(),
            u32::from_le_bytes(*b"MmSg"),
        ) as *mut SEGMENT;

        if !ptr.is_null() {
            core::ptr::write(ptr, SEGMENT::new());
        }
        ptr
    }
}

/// Освобождает SEGMENT
pub unsafe fn mi_free_segment(seg: *mut SEGMENT) {
    if !seg.is_null() {
        crate::ex::pool::ex_free_pool_with_tag(seg as PVOID, u32::from_le_bytes(*b"MmSg"));
    }
}

/// Выделяет SUBSECTION из pool
pub unsafe fn mi_allocate_subsection() -> *mut SUBSECTION {
    unsafe {
        let ptr = crate::ex::pool::ex_allocate_pool_with_tag(
            crate::ex::pool::POOL_TYPE::NonPagedPool,
            core::mem::size_of::<SUBSECTION>(),
            u32::from_le_bytes(*b"MmSs"),
        ) as *mut SUBSECTION;

        if !ptr.is_null() {
            core::ptr::write(ptr, SUBSECTION::new());
        }
        ptr
    }
}

/// Освобождает SUBSECTION
pub unsafe fn mi_free_subsection(ss: *mut SUBSECTION) {
    if !ss.is_null() {
        crate::ex::pool::ex_free_pool_with_tag(ss as PVOID, u32::from_le_bytes(*b"MmSs"));
    }
}

/// Выделяет SECTION из pool
pub unsafe fn mi_allocate_section() -> *mut SECTION {
    unsafe {
        let ptr = crate::ex::pool::ex_allocate_pool_with_tag(
            crate::ex::pool::POOL_TYPE::NonPagedPool,
            core::mem::size_of::<SECTION>(),
            u32::from_le_bytes(*b"MmSe"),
        ) as *mut SECTION;

        if !ptr.is_null() {
            core::ptr::write(ptr, SECTION::new());
        }
        ptr
    }
}

/// Освобождает SECTION
pub unsafe fn mi_free_section(section: *mut SECTION) {
    if !section.is_null() {
        crate::ex::pool::ex_free_pool_with_tag(section as PVOID, u32::from_le_bytes(*b"MmSe"));
    }
}

/// Выделяет массив prototype PTEs
pub unsafe fn mi_allocate_prototype_ptes(count: usize) -> *mut MMPTE {
    unsafe {
        if count == 0 {
            return ptr::null_mut();
        }

        let size = count * core::mem::size_of::<MMPTE>();
        let ptr = crate::ex::pool::ex_allocate_pool_with_tag(
            crate::ex::pool::POOL_TYPE::NonPagedPool,
            size,
            u32::from_le_bytes(*b"MmPp"),
        ) as *mut MMPTE;

        if !ptr.is_null() {
            // Инициализируем demand-zero
            for i in 0..count {
                let pte = ptr.add(i);
                (*pte).value = super::pagefault::pte_state::DEMAND_ZERO;
            }
        }
        ptr
    }
}

/// Освобождает массив prototype PTEs
pub unsafe fn mi_free_prototype_ptes(ptes: *mut MMPTE) {
    if !ptes.is_null() {
        crate::ex::pool::ex_free_pool_with_tag(ptes as PVOID, u32::from_le_bytes(*b"MmPp"));
    }
}

// =============================================================================
// NtCreateSection
// =============================================================================

/// NtCreateSection
///
/// Создает объект секции.
///
/// # Arguments
/// * `section_handle` - указатель для возврата хэндла
/// * `desired_access` - требуемые права доступа
/// * `object_attributes` - атрибуты объекта (может быть NULL)
/// * `maximum_size` - максимальный размер секции (для pagefile-backed)
/// * `section_page_protection` - защита страниц
/// * `allocation_attributes` - SEC_* флаги
/// * `file_handle` - хэндл файла (NULL для pagefile-backed)
///
/// # Returns
/// NTSTATUS
#[unsafe(no_mangle)]
pub extern "win64" fn NtCreateSection(
    section_handle: *mut HANDLE,
    desired_access: ACCESS_MASK,
    object_attributes: *const OBJECT_ATTRIBUTES,
    maximum_size: *const LARGE_INTEGER,
    section_page_protection: ULONG,
    allocation_attributes: ULONG,
    file_handle: HANDLE,
) -> NTSTATUS {
    if section_handle.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    unsafe {
        // Проверяем что Section Object Type инициализирован
        let section_type = mm_get_section_object_type();
        if section_type.is_null() {
            // Section Object Type должен быть инициализирован в MM Phase 1
            return STATUS_OBJECT_TYPE_MISMATCH;
        }

        // Определяем тип секции
        let is_image = (allocation_attributes & section_flags::SEC_IMAGE) != 0;
        let _is_reserve = (allocation_attributes & section_flags::SEC_RESERVE) != 0;
        let _is_commit = (allocation_attributes & section_flags::SEC_COMMIT) != 0;

        // File handle NULL -> pagefile-backed
        let is_pagefile_backed = file_handle == NULL_HANDLE || file_handle == INVALID_HANDLE_VALUE;

        if is_image && is_pagefile_backed {
            // Image section требует файл
            return STATUS_SECTION_NOT_IMAGE;
        }

        // Получаем размер
        let size = if !maximum_size.is_null() {
            (*maximum_size).quad_part as u64
        } else if is_pagefile_backed {
            // Для pagefile-backed размер обязателен
            return STATUS_INVALID_PARAMETER;
        } else {
            // Для file-backed можно определить из файла
            // TODO: получить размер файла через ObReferenceObjectByHandle
            0
        };

        if size == 0 && is_pagefile_backed {
            return STATUS_INVALID_PARAMETER;
        }

        // Создаем Section объект через Object Manager
        let mut section_obj: PVOID = core::ptr::null_mut();

        let status = ob_create_object(
            0, // KernelMode
            section_type,
            object_attributes,
            0, // KernelMode access
            core::ptr::null_mut(),
            core::mem::size_of::<SECTION>(),
            0, // default paged pool charge
            0, // default non-paged pool charge
            &mut section_obj,
        );

        if status != STATUS_SUCCESS {
            return status;
        }

        // Инициализируем SECTION объект
        let section = section_obj as *mut SECTION;
        core::ptr::write(section, SECTION::new());

        // Создаем внутренние структуры в зависимости от типа
        let status = if is_pagefile_backed {
            mi_init_pagefile_section(
                section,
                size,
                section_page_protection,
                allocation_attributes,
            )
        } else if is_image {
            mi_init_image_section(section, file_handle, section_page_protection)
        } else {
            mi_init_data_section(
                section,
                file_handle,
                size,
                section_page_protection,
                allocation_attributes,
            )
        };

        if status != STATUS_SUCCESS {
            // Освобождаем объект при ошибке
            ob_dereference_object(section_obj);
            return status;
        }

        // Вставляем объект в namespace и создаем handle
        let mut handle: PVOID = core::ptr::null_mut();
        let status = ob_insert_object(
            section_obj,
            core::ptr::null_mut(), // access state
            desired_access,
            0, // object pointer bias
            core::ptr::null_mut(), // new object
            &mut handle,
        );

        if status == STATUS_SUCCESS {
            *section_handle = handle as HANDLE;
        }

        status
    }
}

/// Инициализирует pagefile-backed секцию (OB-managed SECTION объект)
///
/// # Arguments
/// * `section` - Указатель на уже выделенный SECTION объект (через ob_create_object)
/// * `size` - Размер секции в байтах
/// * `protection` - Начальная защита страниц
/// * `flags` - Флаги секции (SEC_*)
unsafe fn mi_init_pagefile_section(
    section: *mut SECTION,
    size: u64,
    protection: ULONG,
    flags: ULONG,
) -> NTSTATUS {
    unsafe {
        // Выделяем CONTROL_AREA
        let ca = mi_allocate_control_area();
        if ca.is_null() {
            return STATUS_NO_MEMORY;
        }

        // Выделяем SEGMENT
        let segment = mi_allocate_segment();
        if segment.is_null() {
            mi_free_control_area(ca);
            return STATUS_NO_MEMORY;
        }

        // Выделяем SUBSECTION
        let subsection = mi_allocate_subsection();
        if subsection.is_null() {
            mi_free_segment(segment);
            mi_free_control_area(ca);
            return STATUS_NO_MEMORY;
        }

        // Вычисляем количество PTEs
        let page_count = (size as usize + PAGE_SIZE - 1) / PAGE_SIZE;

        // Выделяем prototype PTEs
        let ptes = mi_allocate_prototype_ptes(page_count);
        if ptes.is_null() {
            mi_free_subsection(subsection);
            mi_free_segment(segment);
            mi_free_control_area(ca);
            return STATUS_NO_MEMORY;
        }

        // Настраиваем CONTROL_AREA
        (*ca).segment = segment;
        (*ca).section_type = SECTION_TYPE::PagefileBacked;
        (*ca).flags = flags;
        (*ca).first_subsection = subsection;
        (*ca).last_subsection = subsection;
        (*ca).derived_section_count.store(1, Ordering::Release);

        // Настраиваем SEGMENT
        (*segment).control_area = ca;
        (*segment).total_number_of_ptes = page_count as u64;
        (*segment).segment_size.quad_part = size as i64;
        (*segment).prototype_pte = ptes;
        (*segment).first_prototype_pte = ptes;
        (*segment).last_prototype_pte = ptes.add(page_count);

        // Настраиваем SUBSECTION
        (*subsection).control_area = ca;
        (*subsection).subsection_base = ptes;
        (*subsection).ptes_in_subsection = page_count as u32;

        // Устанавливаем protection в subsection flags
        let mm_protect = super::virtual_mem::mi_convert_protection(protection);
        (*subsection).subsection_flags.set_protection(mm_protect);

        // Настраиваем SECTION объект
        (*section).segment = segment;
        (*section).flags = flags;
        (*section).initial_page_protection = protection;
        (*section).size_of_section.quad_part = size as i64;

        STATUS_SUCCESS
    }
}

/// Инициализирует image секцию (OB-managed SECTION объект)
unsafe fn mi_init_image_section(
    section: *mut SECTION,
    _file_handle: HANDLE,
    protection: ULONG,
) -> NTSTATUS {
    unsafe {
        // TODO: Открыть FILE_OBJECT из file_handle через ObReferenceObjectByHandle
        // Пока создаём заглушку image section

        // Выделяем CONTROL_AREA
        let ca = mi_allocate_control_area();
        if ca.is_null() {
            return STATUS_NO_MEMORY;
        }

        // Выделяем SEGMENT
        let segment = mi_allocate_segment();
        if segment.is_null() {
            mi_free_control_area(ca);
            return STATUS_NO_MEMORY;
        }

        // Настраиваем CONTROL_AREA для image
        (*ca).segment = segment;
        (*ca).section_type = SECTION_TYPE::Image;
        (*ca).flags = section_flags::SEC_IMAGE;
        (*ca).derived_section_count.store(1, Ordering::Release);

        // Настраиваем SEGMENT
        (*segment).control_area = ca;
        // Размер будет заполнен при парсинге PE header

        // Настраиваем SECTION объект
        (*section).segment = segment;
        (*section).flags = section_flags::SEC_IMAGE;
        (*section).initial_page_protection = protection;

        // TODO: Прочитать PE header и создать subsections
        // Это требует функций чтения файла через IO Manager

        STATUS_SUCCESS
    }
}

/// Инициализирует data file секцию (OB-managed SECTION объект)
unsafe fn mi_init_data_section(
    _section: *mut SECTION,
    _file_handle: HANDLE,
    _size: u64,
    _protection: ULONG,
    _flags: ULONG,
) -> NTSTATUS {
    // TODO: Реализовать после интеграции с IO Manager
    // 1. Открыть FILE_OBJECT из file_handle через ObReferenceObjectByHandle
    // 2. Проверить/установить SECTION_OBJECT_POINTERS
    // 3. Создать CONTROL_AREA и SEGMENT
    // 4. Настроить prototype PTEs

    STATUS_INVALID_PARAMETER
}
// =============================================================================
// NtMapViewOfSection
// =============================================================================

/// NtMapViewOfSection
///
/// Маппит view секции в адресное пространство процесса.
///
/// # Arguments
/// * `section_handle` - хэндл секции
/// * `process_handle` - хэндл процесса
/// * `base_address` - указатель на базовый адрес (in/out)
/// * `zero_bits` - количество нулевых бит в адресе
/// * `commit_size` - размер для commit
/// * `section_offset` - смещение в секции
/// * `view_size` - размер view (in/out)
/// * `inherit_disposition` - наследование
/// * `allocation_type` - MEM_* флаги
/// * `win32_protect` - PAGE_* защита
#[unsafe(no_mangle)]
pub extern "win64" fn NtMapViewOfSection(
    section_handle: HANDLE,
    process_handle: HANDLE,
    base_address: *mut PVOID,
    _zero_bits: usize,
    _commit_size: usize,
    section_offset: *mut LARGE_INTEGER,
    view_size: *mut usize,
    _inherit_disposition: u32, // SECTION_INHERIT
    allocation_type: ULONG,
    win32_protect: ULONG,
) -> NTSTATUS {
    if section_handle == NULL_HANDLE || base_address.is_null() || view_size.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    unsafe {
        // Проверяем что Section Object Type инициализирован
        let section_type = mm_get_section_object_type();
        if section_type.is_null() {
            return STATUS_OBJECT_TYPE_MISMATCH;
        }

        // Получаем SECTION из хэндла через Object Manager
        let mut section_obj: PVOID = core::ptr::null_mut();
        let status = ob_reference_object_by_handle(
            section_handle as PVOID,
            SECTION_MAP_READ | SECTION_MAP_WRITE,
            section_type,
            0, // KernelMode
            &mut section_obj,
            core::ptr::null_mut(),
        );

        if status != STATUS_SUCCESS {
            return status;
        }

        let section = section_obj as *mut SECTION;

        // Получаем segment
        let segment = (*section).segment;
        if segment.is_null() {
            ob_dereference_object(section_obj);
            return STATUS_INVALID_PARAMETER;
        }

        // Получаем control area
        let ca = (*segment).control_area;
        if ca.is_null() {
            ob_dereference_object(section_obj);
            return STATUS_INVALID_PARAMETER;
        }

        // Определяем размер view
        let requested_view_size = *view_size;
        let actual_view_size = if requested_view_size == 0 {
            (*segment).segment_size.quad_part as usize
        } else {
            requested_view_size
        };

        // Выравниваем до 64KB (allocation granularity)
        let aligned_size =
            (actual_view_size + MM_ALLOCATION_GRANULARITY - 1) & !(MM_ALLOCATION_GRANULARITY - 1);

        // Определяем базовый адрес
        let requested_base = *base_address as u64;
        let actual_base = if requested_base == 0 {
            // Нужно найти свободное место в VAD дереве процесса
            let found_base = mi_find_empty_address_range_for_section(
                process_handle,
                aligned_size,
                MM_ALLOCATION_GRANULARITY as u64,
                (*ca).is_image(),
            );
            if found_base == 0 {
                ob_dereference_object(section_obj);
                return STATUS_NO_MEMORY;
            }
            found_base
        } else {
            // Выравниваем запрошенный адрес
            requested_base & !(MM_ALLOCATION_GRANULARITY as u64 - 1)
        };

        // Вычисляем смещение
        let offset = if !section_offset.is_null() {
            (*section_offset).quad_part as u64
        } else {
            0
        };

        // Создаем VAD для этого view
        let status = mi_map_view_in_process(
            process_handle,
            section,
            actual_base,
            aligned_size,
            offset,
            win32_protect,
            allocation_type,
        );

        if status == STATUS_SUCCESS {
            *base_address = actual_base as PVOID;
            *view_size = aligned_size;

            // Увеличиваем счетчик mapped views
            (*ca).number_of_mapped_views.fetch_add(1, Ordering::AcqRel);
        }

        // Освобождаем reference на section (VAD держит свой reference)
        ob_dereference_object(section_obj);

        status
    }
}

/// Маппит view в адресное пространство процесса
unsafe fn mi_map_view_in_process(
    _process_handle: HANDLE,
    section: *mut SECTION,
    base: u64,
    size: usize,
    offset: u64,
    protection: ULONG,
    _allocation_type: ULONG,
) -> NTSTATUS {
    unsafe {
        // Получаем segment и control area
        let segment = (*section).segment;
        let ca = (*segment).control_area;

        // Создаем VAD для mapped view
        let vad = mi_allocate_vad();
        if vad.is_null() {
            return STATUS_NO_MEMORY;
        }

        // Настраиваем VAD
        let start_vpn = base >> PAGE_SHIFT;
        let end_vpn = (base + size as u64 - 1) >> PAGE_SHIFT;

        (*vad).short.starting_vpn = start_vpn;
        (*vad).short.ending_vpn = end_vpn;
        (*vad).short.flags.set_vad_type(VAD_TYPE::VadMapped);

        // Устанавливаем protection
        let mm_protect = super::virtual_mem::mi_convert_protection(protection);
        (*vad).short.flags.set_protection(mm_protect);

        // Устанавливаем ссылку на control area
        (*vad).control_area = ca as PVOID;

        // Вычисляем первый prototype PTE для этого view
        let pte_offset = (offset / PAGE_SIZE as u64) as usize;
        let first_pte = (*segment).first_prototype_pte.add(pte_offset);
        (*vad).first_prototype_pte = first_pte as PVOID;

        // Настраиваем процессные PTEs как prototype references
        let page_count = size / PAGE_SIZE;
        for i in 0..page_count {
            let va = base + (i * PAGE_SIZE) as u64;
            let pte_ptr = mm_get_pte_address(va) as *mut u64;

            // Prototype PTE reference format:
            // [prototype_pte_address:48][prototype=1][valid=0]
            let proto_pte = first_pte.add(i);
            let proto_value = (proto_pte as u64) | super::pagefault::pte_state::PROTOTYPE;

            core::ptr::write_volatile(pte_ptr, proto_value);
        }

        // TODO: Вставить VAD в дерево процесса
        // mi_insert_vad(&mut (*process).vad_root, &mut (*vad).short);

        STATUS_SUCCESS
    }
}

// =============================================================================
// NtUnmapViewOfSection
// =============================================================================

/// NtUnmapViewOfSection
///
/// Размаппит view секции из адресного пространства.
#[unsafe(no_mangle)]
pub extern "win64" fn NtUnmapViewOfSection(
    process_handle: HANDLE,
    base_address: PVOID,
) -> NTSTATUS {
    if base_address.is_null() {
        return STATUS_INVALID_PARAMETER;
    }

    unsafe {
        mi_unmap_view_of_section(process_handle, base_address as u64)
    }
}

/// Внутренняя реализация unmap view
unsafe fn mi_unmap_view_of_section(process_handle: HANDLE, base_address: u64) -> NTSTATUS {
    unsafe {
        // Получаем VAD table процесса
        // TODO: ObReferenceObjectByHandle для process_handle
        let vad_table = mi_get_vad_table_for_process(process_handle);
        if vad_table.is_null() {
            return STATUS_INVALID_PARAMETER;
        }

        // Находим VAD по адресу
        let vad = mi_find_vad(vad_table, base_address);
        if vad.is_null() {
            return crate::nt::STATUS_NOT_MAPPED_VIEW;
        }

        // Проверяем что это mapped view
        if (*vad).flags.vad_type() != VAD_TYPE::VadMapped {
            return crate::nt::STATUS_NOT_MAPPED_VIEW;
        }

        // Получаем control area
        let vad_full = vad as *mut MMVAD;
        let ca = (*vad_full).control_area as *mut CONTROL_AREA;

        // Вычисляем диапазон
        let start_va = (*vad).starting_vpn << PAGE_SHIFT;
        let end_va = ((*vad).ending_vpn << PAGE_SHIFT) + PAGE_SIZE as u64;
        let page_count = ((end_va - start_va) / PAGE_SIZE as u64) as usize;

        // Очищаем PTEs и освобождаем PFN
        for i in 0..page_count {
            let va = start_va + (i * PAGE_SIZE) as u64;
            let pte_ptr = mm_get_pte_address(va) as *mut u64;
            let pte_value = core::ptr::read_volatile(pte_ptr);

            // Если страница в памяти — освобождаем PFN
            if (pte_value & PTE_VALID) != 0 {
                let pfn = ((pte_value >> 12) & 0xFFFFFFFFFF) as usize;

                // Уменьшаем share count
                let _lock = crate::ke::spinlock::KSpinLockGuard::new(
                    super::pfn::MM_PFN_LOCK.get()
                );
                super::pfn::mi_decrement_share_count(pfn);
            }

            // Очищаем PTE
            core::ptr::write_volatile(pte_ptr, 0);
            crate::arch::x86_64::cpu::invlpg(va);
        }

        // Удаляем VAD из дерева
        mi_remove_vad(vad_table, vad);
        mi_free_vad(vad_full);

        // Уменьшаем счетчик mapped views
        if !ca.is_null() {
            let old_count = (*ca).number_of_mapped_views.fetch_sub(1, Ordering::AcqRel);

            // Если это был последний view — можно освободить control area
            if old_count == 1 {
                mm_dereference_section_for_control_area(ca);
            }
        }

        STATUS_SUCCESS
    }
}

/// Получает VAD table для процесса
///
/// Поддерживает NtCurrentProcess (-1) и handle-based lookup.
unsafe fn mi_get_vad_table_for_process(process_handle: HANDLE) -> *mut MM_AVL_TABLE {
    unsafe {
        use crate::ps::process::ps_get_current_process;
        use crate::ps::process::EPROCESS;

        // Для текущего процесса (-1 или NtCurrentProcess)
        let handle_value = process_handle as isize;
        if handle_value == -1 || process_handle == NULL_HANDLE {
            let process = ps_get_current_process();
            if process.is_null() {
                return ptr::null_mut();
            }
            return (*process).vad_root as *mut MM_AVL_TABLE;
        }

        // Для других процессов: пробуем ObReferenceObjectByHandle
        // TODO: Требуется PS_PROCESS_OBJECT_TYPE
        // Пока возвращаем null для не-текущих процессов
        ptr::null_mut()
    }
}

/// Уменьшает reference count control area при unmap
unsafe fn mm_dereference_section_for_control_area(ca: *mut CONTROL_AREA) {
    unsafe {
        if ca.is_null() {
            return;
        }

        let old_count = (*ca).derived_section_count.fetch_sub(1, Ordering::AcqRel);

        if old_count == 1 {
            // Последняя ссылка — освобождаем всё
            let segment = (*ca).segment;
            if !segment.is_null() {
                let ptes = (*segment).prototype_pte;
                if !ptes.is_null() {
                    mi_free_prototype_ptes(ptes);
                }
                mi_free_segment(segment);
            }

            // Освобождаем subsections
            let mut ss = (*ca).first_subsection;
            while !ss.is_null() {
                let next = (*ss).next_subsection;
                mi_free_subsection(ss);
                ss = next;
            }

            mi_free_control_area(ca);
        }
    }
}

// =============================================================================
// Address Space Search for Section Mapping
// =============================================================================

/// Ищет свободный диапазон адресов для маппинга секции
///
/// # Arguments
/// * `process_handle` - хэндл процесса
/// * `size` - требуемый размер в байтах
/// * `alignment` - выравнивание (обычно 64KB)
/// * `is_image` - true для image sections (предпочитает high addresses)
///
/// # Returns
/// Найденный базовый адрес или 0 при ошибке
unsafe fn mi_find_empty_address_range_for_section(
    process_handle: HANDLE,
    size: usize,
    alignment: u64,
    is_image: bool,
) -> u64 {
    unsafe {
        let vad_table = mi_get_vad_table_for_process(process_handle);

        // Если нет VAD table — используем фиксированные диапазоны
        if vad_table.is_null() {
            // Для image sections предпочитаем high user space
            // Для data sections — low user space
            if is_image {
                // Image base: 0x7FF6_0000_0000 и выше
                return 0x7FF6_0000_0000u64;
            } else {
                // Data/shared: 0x0000_0001_0000_0000 и выше
                return 0x0000_0001_0000_0000u64;
            }
        }

        // Полный поиск через VAD дерево
        mi_find_empty_address_range_in_vad_tree(vad_table, size, alignment, is_image)
    }
}

/// Ищет свободный диапазон в VAD дереве
unsafe fn mi_find_empty_address_range_in_vad_tree(
    vad_table: *mut MM_AVL_TABLE,
    size: usize,
    alignment: u64,
    is_image: bool,
) -> u64 {
    unsafe {
        let size_in_pages = (size + PAGE_SIZE - 1) / PAGE_SIZE;

        // Определяем диапазон поиска
        let (search_start, search_end) = if is_image {
            // Image sections: high user space
            (0x7FF0_0000_0000u64, 0x7FFF_FFFF_0000u64)
        } else {
            // Data sections: low-mid user space
            (0x0000_0001_0000_0000u64, 0x7FEF_FFFF_0000u64)
        };

        // Проходим по VAD дереву и ищем дыру
        let mut current_va = (search_start + alignment - 1) & !(alignment - 1);

        loop {
            if current_va >= search_end {
                return 0;
            }

            // Проверяем, есть ли VAD в этом диапазоне
            let vad = mi_find_vad(vad_table, current_va);

            if vad.is_null() {
                // Нет VAD — проверяем достаточно ли места
                let end_va = current_va + size as u64 - 1;

                // Ищем следующий VAD после current_va
                let next_vad = mi_find_next_vad_after(vad_table, current_va);

                if next_vad.is_null() {
                    // Нет VAD дальше — место найдено
                    if end_va < search_end {
                        return current_va;
                    }
                    return 0;
                }

                let next_vad_start = (*next_vad).starting_vpn << PAGE_SHIFT;

                if end_va < next_vad_start {
                    // Достаточно места до следующего VAD
                    return current_va;
                }

                // Недостаточно места — переходим за следующий VAD
                current_va = (((*next_vad).ending_vpn + 1) << PAGE_SHIFT);
                current_va = (current_va + alignment - 1) & !(alignment - 1);
            } else {
                // Есть VAD — переходим за него
                current_va = (((*vad).ending_vpn + 1) << PAGE_SHIFT);
                current_va = (current_va + alignment - 1) & !(alignment - 1);
            }
        }
    }
}

/// Находит следующий VAD после заданного адреса
unsafe fn mi_find_next_vad_after(vad_table: *mut MM_AVL_TABLE, va: u64) -> *mut MMVAD_SHORT {
    unsafe {
        if vad_table.is_null() || (*vad_table).root.is_null() {
            return ptr::null_mut();
        }

        let target_vpn = va >> PAGE_SHIFT;
        let mut result: *mut MMVAD_SHORT = ptr::null_mut();
        let mut current = (*vad_table).root;

        while !current.is_null() {
            if (*current).starting_vpn > target_vpn {
                // Этот VAD начинается после target — это кандидат
                result = current;
                // Ищем меньший VAD слева
                current = (*current).left_child;
            } else {
                // VAD начинается до или на target — ищем справа
                current = (*current).right_child;
            }
        }

        result
    }
}

// =============================================================================
// Section Reference Counting
// =============================================================================

/// Увеличивает reference count секции
pub unsafe fn mm_reference_section(section: *mut SECTION) {
    unsafe {
        if section.is_null() {
            return;
        }

        let segment = (*section).segment;
        if segment.is_null() {
            return;
        }

        let ca = (*segment).control_area;
        if !ca.is_null() {
            (*ca).derived_section_count.fetch_add(1, Ordering::AcqRel);
        }
    }
}

/// Уменьшает reference count секции
pub unsafe fn mm_dereference_section(section: *mut SECTION) {
    unsafe {
        if section.is_null() {
            return;
        }

        let segment = (*section).segment;
        if segment.is_null() {
            return;
        }

        let ca = (*segment).control_area;
        if ca.is_null() {
            return;
        }

        let old_count = (*ca).derived_section_count.fetch_sub(1, Ordering::AcqRel);

        if old_count == 1 {
            // Последняя ссылка — освобождаем
            mi_delete_section_internal(section);
        }
    }
}

/// Внутренняя функция удаления секции
unsafe fn mi_delete_section_internal(section: *mut SECTION) {
    unsafe {
        if section.is_null() {
            return;
        }

        let segment = (*section).segment;
        if !segment.is_null() {
            // Освобождаем prototype PTEs
            let ptes = (*segment).prototype_pte;
            if !ptes.is_null() {
                mi_free_prototype_ptes(ptes);
            }

            let ca = (*segment).control_area;
            if !ca.is_null() {
                // Освобождаем subsections
                let mut ss = (*ca).first_subsection;
                while !ss.is_null() {
                    let next = (*ss).next_subsection;
                    mi_free_subsection(ss);
                    ss = next;
                }

                mi_free_control_area(ca);
            }

            mi_free_segment(segment);
        }

        mi_free_section(section);
    }
}

// =============================================================================
// Image Section Creation from Memory
// =============================================================================

/// Создает image section из PE в памяти
///
/// Используется для загрузки PE из boot modules или RAM disk.
///
/// # Arguments
/// * `section_handle` - указатель для возврата хэндла
/// * `image_base` - базовый адрес PE образа в памяти
/// * `image_size` - размер образа
///
/// # Returns
/// NTSTATUS
pub unsafe fn mi_create_image_section_from_memory(
    section_handle: *mut HANDLE,
    image_base: *const u8,
    image_size: usize,
) -> NTSTATUS {
    unsafe {
        if section_handle.is_null() || image_base.is_null() || image_size < 64 {
            return STATUS_INVALID_PARAMETER;
        }

        // Проверяем DOS header
        let dos_header = image_base as *const pe::IMAGE_DOS_HEADER;
        if (*dos_header).e_magic != pe::IMAGE_DOS_SIGNATURE {
            return crate::nt::STATUS_INVALID_IMAGE_NOT_MZ;
        }

        // Находим PE header
        let pe_offset = (*dos_header).e_lfanew as usize;
        if pe_offset + 4 > image_size {
            return crate::nt::STATUS_INVALID_IMAGE_FORMAT;
        }

        let pe_sig = *(image_base.add(pe_offset) as *const u32);
        if pe_sig != pe::IMAGE_NT_SIGNATURE {
            return crate::nt::STATUS_INVALID_IMAGE_FORMAT;
        }

        // Получаем file header
        let file_header = image_base.add(pe_offset + 4) as *const pe::IMAGE_FILE_HEADER;

        // Проверяем архитектуру
        if (*file_header).machine != pe::IMAGE_FILE_MACHINE_AMD64 {
            return crate::nt::STATUS_INVALID_IMAGE_FORMAT;
        }

        // Получаем optional header
        let opt_header = image_base.add(pe_offset + 4 + core::mem::size_of::<pe::IMAGE_FILE_HEADER>())
            as *const pe::IMAGE_OPTIONAL_HEADER64;

        // Создаём section
        let ca = mi_allocate_control_area();
        if ca.is_null() {
            return STATUS_NO_MEMORY;
        }

        let segment = mi_allocate_segment();
        if segment.is_null() {
            mi_free_control_area(ca);
            return STATUS_NO_MEMORY;
        }

        let section = mi_allocate_section();
        if section.is_null() {
            mi_free_segment(segment);
            mi_free_control_area(ca);
            return STATUS_NO_MEMORY;
        }

        // Вычисляем общий размер image
        let size_of_image = (*opt_header).size_of_image as usize;
        let page_count = (size_of_image + PAGE_SIZE - 1) / PAGE_SIZE;

        // Выделяем prototype PTEs для всего image
        let ptes = mi_allocate_prototype_ptes(page_count);
        if ptes.is_null() {
            mi_free_section(section);
            mi_free_segment(segment);
            mi_free_control_area(ca);
            return STATUS_NO_MEMORY;
        }

        // Настраиваем CONTROL_AREA
        (*ca).segment = segment;
        (*ca).section_type = SECTION_TYPE::Image;
        (*ca).flags = section_flags::SEC_IMAGE;
        (*ca).derived_section_count.store(1, Ordering::Release);

        // Настраиваем SEGMENT
        (*segment).control_area = ca;
        (*segment).total_number_of_ptes = page_count as u64;
        (*segment).segment_size.quad_part = size_of_image as i64;
        (*segment).prototype_pte = ptes;
        (*segment).first_prototype_pte = ptes;
        (*segment).last_prototype_pte = ptes.add(page_count);
        (*segment).system_image_base = (*opt_header).image_base;
        (*segment).based_address = (*opt_header).image_base;

        // Создаём subsections для каждой PE секции
        let num_sections = (*file_header).number_of_sections as usize;
        let sections_ptr = image_base.add(
            pe_offset
                + 4
                + core::mem::size_of::<pe::IMAGE_FILE_HEADER>()
                + (*file_header).size_of_optional_header as usize,
        ) as *const pe::IMAGE_SECTION_HEADER;

        let mut prev_ss: *mut SUBSECTION = ptr::null_mut();

        for i in 0..num_sections {
            let pe_section = sections_ptr.add(i);

            let subsection = mi_allocate_subsection();
            if subsection.is_null() {
                // Cleanup on error
                mi_free_prototype_ptes(ptes);
                mi_free_section(section);
                mi_free_segment(segment);
                mi_free_control_area(ca);
                return STATUS_NO_MEMORY;
            }

            // Вычисляем параметры subsection
            let va_offset = (*pe_section).virtual_address as usize;
            let va_size = (*pe_section).virtual_size as usize;
            let start_pte_index = va_offset / PAGE_SIZE;
            let pte_count = (va_size + PAGE_SIZE - 1) / PAGE_SIZE;

            (*subsection).control_area = ca;
            (*subsection).subsection_base = ptes.add(start_pte_index);
            (*subsection).ptes_in_subsection = pte_count as u32;
            (*subsection).starting_sector = ((*pe_section).pointer_to_raw_data / 512) as u64;
            (*subsection).number_of_full_sectors = ((*pe_section).size_of_raw_data / 512) as u32;

            // Устанавливаем protection на основе section characteristics
            let chars = (*pe_section).characteristics;
            let mut prot = MM_READONLY;

            if (chars & pe::IMAGE_SCN_MEM_EXECUTE) != 0 {
                if (chars & pe::IMAGE_SCN_MEM_WRITE) != 0 {
                    prot = MM_EXECUTE_READWRITE;
                } else {
                    prot = MM_EXECUTE_READ;
                }
            } else if (chars & pe::IMAGE_SCN_MEM_WRITE) != 0 {
                prot = MM_READWRITE;
            }

            (*subsection).subsection_flags.set_protection(prot);

            if (chars & pe::IMAGE_SCN_MEM_SHARED) != 0 {
                (*subsection).subsection_flags.set_global(true);
            }

            // Связываем в список
            if prev_ss.is_null() {
                (*ca).first_subsection = subsection;
            } else {
                (*prev_ss).next_subsection = subsection;
            }
            prev_ss = subsection;
        }

        (*ca).last_subsection = prev_ss;

        // Настраиваем SECTION
        (*section).segment = segment;
        (*section).flags = section_flags::SEC_IMAGE;
        (*section).initial_page_protection = PAGE_EXECUTE_READ;
        (*section).size_of_section.quad_part = size_of_image as i64;
        (*section).starting_va = (*opt_header).image_base;
        (*section).ending_va = (*opt_header).image_base + size_of_image as u64;

        *section_handle = section as HANDLE;

        STATUS_SUCCESS
    }
}

/// Конвертирует PE section characteristics в MM protection
pub fn mi_pe_section_to_protection(characteristics: u32) -> u32 {
    use pe::{IMAGE_SCN_MEM_EXECUTE, IMAGE_SCN_MEM_READ, IMAGE_SCN_MEM_WRITE};

    let execute = (characteristics & IMAGE_SCN_MEM_EXECUTE) != 0;
    let read = (characteristics & IMAGE_SCN_MEM_READ) != 0;
    let write = (characteristics & IMAGE_SCN_MEM_WRITE) != 0;

    match (execute, read, write) {
        (true, true, true) => PAGE_EXECUTE_READWRITE,
        (true, true, false) => PAGE_EXECUTE_READ,
        (true, false, true) => PAGE_EXECUTE_READWRITE,
        (true, false, false) => PAGE_EXECUTE,
        (false, true, true) => PAGE_READWRITE,
        (false, true, false) => PAGE_READONLY,
        (false, false, true) => PAGE_READWRITE,
        (false, false, false) => PAGE_NOACCESS,
    }
}
