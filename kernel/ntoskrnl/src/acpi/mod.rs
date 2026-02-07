//! ACPI модуль для XenOS
//!
//! Реализация поддержки Advanced Configuration and Power Interface (ACPI).
//! Основан на rust-osdev/acpi crate (https://github.com/rust-osdev/acpi)
//!
//! Модуль предоставляет:
//! - Парсинг ACPI таблиц (RSDP, RSDT/XSDT, MADT, FADT, HPET, MCFG и др.)
//! - AML интерпретатор для выполнения DSDT/SSDT
//! - Высокоуровневый API для работы с платформой

#![allow(dead_code)]

extern crate alloc;

pub mod address;
pub mod aml;
pub mod handler;
pub mod mgr;
pub mod platform;
pub mod registers;
pub mod rsdp;
pub mod sdt;

pub use mgr::*;

use core::fmt;
use core::mem;
use core::ops::Deref;
use core::ops::DerefMut;
use core::pin::Pin;
use core::ptr::NonNull;

pub use handler::XenAcpiHandler;
use log::warn;
pub use pci_types::PciAddress;
use rsdp::Rsdp;
pub use sdt::fadt::PowerProfile;
pub use sdt::hpet::HpetInfo;
pub use sdt::madt::MadtError;

use crate::acpi::sdt::SdtHeader;
use crate::acpi::sdt::Signature;

// =============================================================================
// AcpiTables - Основная структура для работы с ACPI таблицами
// =============================================================================

/// `AcpiTables` конструируется после нахождения RSDP или RSDT/XSDT и позволяет
/// перечислять ACPI таблицы системы.
pub struct AcpiTables<H: Handler> {
    rsdt_mapping: PhysicalMapping<H, SdtHeader>,
    pub rsdp_revision: u8,
    handler: H,
}

unsafe impl<H> Send for AcpiTables<H> where H: Handler + Send {}
unsafe impl<H> Sync for AcpiTables<H> where H: Handler + Send {}

impl<H> AcpiTables<H>
where
    H: Handler,
{
    /// Конструирует `AcpiTables` из **физического** адреса RSDP.
    ///
    /// # Safety
    /// Адрес RSDP должен быть валидным.
    pub unsafe fn from_rsdp(handler: H, rsdp_address: usize) -> Result<AcpiTables<H>, AcpiError> {
        let rsdp_mapping =
            unsafe { handler.map_physical_region::<Rsdp>(rsdp_address, mem::size_of::<Rsdp>()) };

        // Проверяем сигнатуру RSDP
        match rsdp_mapping.validate() {
            Ok(()) => (),
            Err(AcpiError::RsdpIncorrectSignature) => {
                return Err(AcpiError::RsdpIncorrectSignature);
            },
            Err(AcpiError::RsdpInvalidOemId) | Err(AcpiError::RsdpInvalidChecksum) => {
                warn!("RSDP has invalid checksum or OEM ID. Continuing.");
            },
            Err(_) => (),
        }

        let rsdp_revision = rsdp_mapping.revision();
        let rsdt_address = if rsdp_revision == 0 {
            // ACPI Version 1.0 - используем 32-битный RSDT адрес
            rsdp_mapping.rsdt_address() as usize
        } else {
            // ACPI Version 2.0+ - используем 64-битный XSDT адрес
            rsdp_mapping.xsdt_address() as usize
        };

        unsafe { Self::from_rsdt(handler, rsdp_revision, rsdt_address) }
    }

    /// Конструирует `AcpiTables` из **физического** адреса RSDT/XSDT.
    ///
    /// # Safety
    /// Адрес RSDT должен быть валидным.
    pub unsafe fn from_rsdt(
        handler: H,
        rsdp_revision: u8,
        rsdt_address: usize,
    ) -> Result<AcpiTables<H>, AcpiError> {
        let rsdt_mapping = unsafe {
            handler.map_physical_region::<SdtHeader>(rsdt_address, mem::size_of::<SdtHeader>())
        };
        let rsdt_length = rsdt_mapping.length;
        let rsdt_mapping =
            unsafe { handler.map_physical_region::<SdtHeader>(rsdt_address, rsdt_length as usize) };
        Ok(Self {
            rsdt_mapping,
            rsdp_revision,
            handler,
        })
    }

    /// Итератор по **физическим** адресам SDT таблиц.
    pub fn table_entries(&self) -> impl Iterator<Item = usize> {
        let entry_size = if self.rsdp_revision == 0 { 4 } else { 8 };
        let mut table_entries_ptr = unsafe {
            self.rsdt_mapping
                .virtual_start
                .as_ptr()
                .byte_add(mem::size_of::<SdtHeader>())
        }
        .cast::<u8>();
        let mut num_entries =
            (self.rsdt_mapping.region_length - mem::size_of::<SdtHeader>()) / entry_size;

        core::iter::from_fn(move || {
            if num_entries > 0 {
                unsafe {
                    let entry = if entry_size == 4 {
                        *table_entries_ptr.cast::<u32>() as usize
                    } else {
                        *table_entries_ptr.cast::<u64>() as usize
                    };
                    table_entries_ptr = table_entries_ptr.byte_add(entry_size);
                    num_entries -= 1;

                    Some(entry)
                }
            } else {
                None
            }
        })
    }

    /// Итератор по заголовкам SDT вместе с их **физическими** адресами.
    pub fn table_headers(&self) -> impl Iterator<Item = (usize, SdtHeader)> {
        self.table_entries().map(|table_phys_address| {
            let mapping = unsafe {
                self.handler.map_physical_region::<SdtHeader>(
                    table_phys_address,
                    mem::size_of::<SdtHeader>(),
                )
            };
            (table_phys_address, *mapping)
        })
    }

    /// Найти все таблицы с сигнатурой `T::SIGNATURE`.
    pub fn find_tables<T>(&self) -> impl Iterator<Item = PhysicalMapping<H, T>>
    where
        T: AcpiTable,
    {
        self.table_entries().filter_map(|table_phys_address| {
            let header_mapping = unsafe {
                self.handler.map_physical_region::<SdtHeader>(
                    table_phys_address,
                    mem::size_of::<SdtHeader>(),
                )
            };
            if header_mapping.signature == T::SIGNATURE {
                let length = header_mapping.length;
                drop(header_mapping);
                Some(unsafe {
                    self.handler
                        .map_physical_region::<T>(table_phys_address, length as usize)
                })
            } else {
                None
            }
        })
    }

    /// Найти первую таблицу с сигнатурой `T::SIGNATURE`.
    pub fn find_table<T>(&self) -> Option<PhysicalMapping<H, T>>
    where
        T: AcpiTable,
    {
        self.find_tables().next()
    }

    /// Получить DSDT таблицу.
    pub fn dsdt(&self) -> Result<AmlTable, AcpiError> {
        let Some(fadt) = self.find_table::<sdt::fadt::Fadt>() else {
            Err(AcpiError::TableNotFound(Signature::FADT))?
        };
        let phys_address = fadt.dsdt_address()?;
        let header = unsafe {
            self.handler
                .map_physical_region::<SdtHeader>(phys_address, mem::size_of::<SdtHeader>())
        };
        Ok(AmlTable {
            phys_address,
            length: header.length,
            revision: header.revision,
        })
    }

    /// Итератор по SSDT таблицам.
    pub fn ssdts(&self) -> impl Iterator<Item = AmlTable> {
        self.table_headers().filter_map(|(phys_address, header)| {
            if header.signature == Signature::SSDT {
                Some(AmlTable {
                    phys_address,
                    length: header.length,
                    revision: header.revision,
                })
            } else {
                None
            }
        })
    }
}

// =============================================================================
// AmlTable - Информация о таблице с AML кодом
// =============================================================================

#[derive(Clone, Copy, Debug)]
pub struct AmlTable {
    /// Физический адрес начала таблицы. Добавьте `mem::size_of::<SdtHeader>()`
    /// чтобы получить адрес начала AML потока.
    pub phys_address: usize,
    /// Длина таблицы, включая заголовок.
    pub length: u32,
    pub revision: u8,
}

// =============================================================================
// AcpiTable trait
// =============================================================================

/// Все типы, представляющие ACPI таблицы, должны реализовать этот trait.
///
/// ### Safety
/// Память таблицы интерпретируется напрямую, поэтому тип должен корректно
/// представлять структуру таблицы.
pub unsafe trait AcpiTable {
    const SIGNATURE: Signature;

    fn header(&self) -> &SdtHeader;

    fn validate(&self) -> Result<(), AcpiError> {
        unsafe { self.header().validate(Self::SIGNATURE) }
    }
}

// =============================================================================
// AcpiError - Ошибки ACPI
// =============================================================================

#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum AcpiError {
    NoValidRsdp,
    RsdpIncorrectSignature,
    RsdpInvalidOemId,
    RsdpInvalidChecksum,

    SdtInvalidSignature(Signature),
    SdtInvalidOemId(Signature),
    SdtInvalidTableId(Signature),
    SdtInvalidChecksum(Signature),
    SdtInvalidCreatorId(Signature),

    TableNotFound(Signature),
    InvalidFacsAddress,
    InvalidDsdtAddress,
    InvalidMadt(MadtError),
    InvalidGenericAddress,

    Timeout,

    Aml(aml::AmlError),

    /// Библиотека не поддерживает запрошенное поведение.
    LibUnimplemented,

    /// Хост не реализовал требуемое поведение.
    HostUnimplemented,
}

// =============================================================================
// PhysicalMapping - Отображение физической памяти
// =============================================================================

/// Описывает отображение физической памяти, созданное [`Handler::map_physical_region`].
pub struct PhysicalMapping<H, T>
where
    H: Handler,
{
    /// Физический адрес отображенной структуры.
    pub physical_start: usize,
    /// Виртуальный адрес отображенной структуры.
    pub virtual_start: NonNull<T>,
    /// Размер запрошенного региона в байтах.
    pub region_length: usize,
    /// Общий размер отображения.
    pub mapped_length: usize,
    /// Handler, использованный для создания отображения.
    pub handler: H,
}

impl<H, T> PhysicalMapping<H, T>
where
    H: Handler,
{
    /// Получить pinned ссылку на внутренний `T`.
    pub fn get(&self) -> Pin<&T> {
        unsafe { Pin::new_unchecked(self.virtual_start.as_ref()) }
    }
}

impl<H, T> fmt::Debug for PhysicalMapping<H, T>
where
    H: Handler,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PhysicalMapping")
            .field("physical_start", &self.physical_start)
            .field("virtual_start", &self.virtual_start)
            .field("region_length", &self.region_length)
            .field("mapped_length", &self.mapped_length)
            .field("handler", &())
            .finish()
    }
}

unsafe impl<H: Handler + Send, T: Send> Send for PhysicalMapping<H, T> {}

impl<H, T> Deref for PhysicalMapping<H, T>
where
    T: Unpin,
    H: Handler,
{
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { self.virtual_start.as_ref() }
    }
}

impl<H, T> DerefMut for PhysicalMapping<H, T>
where
    T: Unpin,
    H: Handler,
{
    fn deref_mut(&mut self) -> &mut T {
        unsafe { self.virtual_start.as_mut() }
    }
}

impl<H, T> Drop for PhysicalMapping<H, T>
where
    H: Handler,
{
    fn drop(&mut self) {
        H::unmap_physical_region(self)
    }
}

// =============================================================================
// Handle - Непрозрачная ссылка на объект хоста
// =============================================================================

/// `Handle` - непрозрачная ссылка на объект, управляемый хостом.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct Handle(pub u32);

// =============================================================================
// Handler trait - Интерфейс взаимодействия с хостом
// =============================================================================

/// Trait, который должен быть реализован для взаимодействия ACPI с железом.
///
/// Handler должен быть дешево клонируемым (например, ссылка, Arc, маркерная структура).
pub trait Handler: Clone {
    /// Отображает регион физической памяти.
    ///
    /// ## Safety
    ///
    /// - `physical_address` должен указывать на валидный `T` в физической памяти.
    /// - `size` должен быть >= `size_of::<T>()`.
    unsafe fn map_physical_region<T>(
        &self,
        physical_address: usize,
        size: usize,
    ) -> PhysicalMapping<Self, T>;

    /// Снимает отображение физической памяти.
    fn unmap_physical_region<T>(region: &PhysicalMapping<Self, T>);

    // Чтение из памяти
    fn read_u8(&self, address: usize) -> u8;
    fn read_u16(&self, address: usize) -> u16;
    fn read_u32(&self, address: usize) -> u32;
    fn read_u64(&self, address: usize) -> u64;

    // Запись в память
    fn write_u8(&self, address: usize, value: u8);
    fn write_u16(&self, address: usize, value: u16);
    fn write_u32(&self, address: usize, value: u32);
    fn write_u64(&self, address: usize, value: u64);

    // Чтение I/O портов
    fn read_io_u8(&self, port: u16) -> u8;
    fn read_io_u16(&self, port: u16) -> u16;
    fn read_io_u32(&self, port: u16) -> u32;

    // Запись в I/O порты
    fn write_io_u8(&self, port: u16, value: u8);
    fn write_io_u16(&self, port: u16, value: u16);
    fn write_io_u32(&self, port: u16, value: u32);

    // PCI Config Space
    fn read_pci_u8(&self, address: PciAddress, offset: u16) -> u8;
    fn read_pci_u16(&self, address: PciAddress, offset: u16) -> u16;
    fn read_pci_u32(&self, address: PciAddress, offset: u16) -> u32;

    fn write_pci_u8(&self, address: PciAddress, offset: u16, value: u8);
    fn write_pci_u16(&self, address: PciAddress, offset: u16, value: u16);
    fn write_pci_u32(&self, address: PciAddress, offset: u16, value: u32);

    /// Возвращает монотонно возрастающее значение наносекунд.
    fn nanos_since_boot(&self) -> u64;

    /// Задержка на указанное количество **микросекунд** без отдачи процессора.
    fn stall(&self, microseconds: u64);

    /// Сон на указанное количество **миллисекунд** с отдачей процессора.
    fn sleep(&self, milliseconds: u64);

    /// Создать мьютекс для AML.
    fn create_mutex(&self) -> Handle;

    /// Захватить мьютекс.
    /// - `0` - неблокирующая попытка
    /// - `1-0xfffe` - ожидание в мс
    /// - `0xffff` - бесконечное ожидание
    fn acquire(&self, mutex: Handle, timeout: u16) -> Result<(), aml::AmlError>;

    /// Освободить мьютекс.
    fn release(&self, mutex: Handle);

    /// Точка останова отладчика.
    fn breakpoint(&self) {}

    /// Обработка Debug объекта.
    fn handle_debug(&self, _object: &aml::object::Object) {}

    /// Обработка фатальной ошибки AML.
    fn handle_fatal_error(&self, fatal_type: u8, fatal_code: u32, fatal_arg: u64) {
        panic!(
            "Fatal error while executing AML (encountered DefFatalOp). fatal_type = {}, fatal_code = {}, fatal_arg = {}",
            fatal_type, fatal_code, fatal_arg
        );
    }
}
