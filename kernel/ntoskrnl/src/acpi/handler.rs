//! ACPI Handler для XenOS
//!
//! Реализация trait Handler для интеграции ACPI с ядром XenOS.
//! HHDM покрывает всю физическую память, включая ACPI регионы.

use core::ptr::NonNull;

use pci_types::PciAddress;

use super::Handle;
use super::Handler;
use super::PhysicalMapping;
use super::aml::AmlError;
use crate::hal::portio::read_port_uchar;
use crate::hal::portio::read_port_ulong;
use crate::hal::portio::read_port_ushort;
use crate::hal::portio::write_port_uchar;
use crate::hal::portio::write_port_ulong;
use crate::hal::portio::write_port_ushort;

// =============================================================================
// XenAcpiHandler - Реализация Handler для XenOS
// =============================================================================

/// HHDM offset для преобразования физических адресов в виртуальные.
static mut HHDM_OFFSET: usize = 0;

/// Инициализация HHDM offset для ACPI handler.
///
/// # Safety
/// Должна вызываться один раз при инициализации ядра с корректным значением HHDM offset.
pub unsafe fn init_hhdm_offset(offset: usize) {
    unsafe {
        HHDM_OFFSET = offset;
    }
}

/// Получить текущий HHDM offset.
pub fn get_hhdm_offset() -> usize {
    unsafe { HHDM_OFFSET }
}

/// XenOS ACPI Handler.
///
/// Реализует trait Handler для работы с ACPI подсистемой.
/// Использует HHDM (Higher Half Direct Map) для доступа к физической памяти.
#[derive(Clone, Copy)]
pub struct XenAcpiHandler;

impl XenAcpiHandler {
    /// Создать новый ACPI handler.
    pub const fn new() -> Self {
        Self
    }

    /// Преобразовать физический адрес в виртуальный через HHDM.
    #[inline]
    fn phys_to_virt(&self, phys: usize) -> usize {
        phys + get_hhdm_offset()
    }
}

impl Handler for XenAcpiHandler {
    /// Отображает регион физической памяти через HHDM.
    ///
    /// HHDM покрывает всю физическую память, включая ACPI регионы.
    unsafe fn map_physical_region<T>(
        &self,
        physical_address: usize,
        size: usize,
    ) -> PhysicalMapping<Self, T> {
        unsafe {
            let virtual_address = self.phys_to_virt(physical_address);

            PhysicalMapping {
                physical_start: physical_address,
                virtual_start: NonNull::new_unchecked(virtual_address as *mut T),
                region_length: size,
                mapped_length: size,
                handler: *self,
            }
        }
    }

    /// Снимает отображение - в HHDM ничего делать не нужно.
    fn unmap_physical_region<T>(_region: &PhysicalMapping<Self, T>) {
        // HHDM mapping не требует unmap
    }

    // =========================================================================
    // Чтение из физической памяти (через HHDM)
    // =========================================================================

    fn read_u8(&self, address: usize) -> u8 {
        unsafe { core::ptr::read_volatile(self.phys_to_virt(address) as *const u8) }
    }

    fn read_u16(&self, address: usize) -> u16 {
        unsafe { core::ptr::read_volatile(self.phys_to_virt(address) as *const u16) }
    }

    fn read_u32(&self, address: usize) -> u32 {
        unsafe { core::ptr::read_volatile(self.phys_to_virt(address) as *const u32) }
    }

    fn read_u64(&self, address: usize) -> u64 {
        unsafe { core::ptr::read_volatile(self.phys_to_virt(address) as *const u64) }
    }

    // =========================================================================
    // Запись в физическую память (через HHDM)
    // =========================================================================

    fn write_u8(&self, address: usize, value: u8) {
        unsafe { core::ptr::write_volatile(self.phys_to_virt(address) as *mut u8, value) }
    }

    fn write_u16(&self, address: usize, value: u16) {
        unsafe { core::ptr::write_volatile(self.phys_to_virt(address) as *mut u16, value) }
    }

    fn write_u32(&self, address: usize, value: u32) {
        unsafe { core::ptr::write_volatile(self.phys_to_virt(address) as *mut u32, value) }
    }

    fn write_u64(&self, address: usize, value: u64) {
        unsafe { core::ptr::write_volatile(self.phys_to_virt(address) as *mut u64, value) }
    }

    // =========================================================================
    // I/O порты
    // =========================================================================

    fn read_io_u8(&self, port: u16) -> u8 {
        read_port_uchar(port)
    }

    fn read_io_u16(&self, port: u16) -> u16 {
        read_port_ushort(port)
    }

    fn read_io_u32(&self, port: u16) -> u32 {
        read_port_ulong(port)
    }

    fn write_io_u8(&self, port: u16, value: u8) {
        write_port_uchar(port, value)
    }

    fn write_io_u16(&self, port: u16, value: u16) {
        write_port_ushort(port, value)
    }

    fn write_io_u32(&self, port: u16, value: u32) {
        write_port_ulong(port, value)
    }

    // =========================================================================
    // PCI Config Space
    // =========================================================================

    fn read_pci_u8(&self, address: PciAddress, offset: u16) -> u8 {
        pci_config_read_u8(address, offset)
    }

    fn read_pci_u16(&self, address: PciAddress, offset: u16) -> u16 {
        pci_config_read_u16(address, offset)
    }

    fn read_pci_u32(&self, address: PciAddress, offset: u16) -> u32 {
        pci_config_read_u32(address, offset)
    }

    fn write_pci_u8(&self, address: PciAddress, offset: u16, value: u8) {
        pci_config_write_u8(address, offset, value)
    }

    fn write_pci_u16(&self, address: PciAddress, offset: u16, value: u16) {
        pci_config_write_u16(address, offset, value)
    }

    fn write_pci_u32(&self, address: PciAddress, offset: u16, value: u32) {
        pci_config_write_u32(address, offset, value)
    }

    // =========================================================================
    // Timing
    // =========================================================================

    fn nanos_since_boot(&self) -> u64 {
        // Используем ke_query_interrupt_time() которая возвращает 100ns единицы
        let interrupt_time_100ns = crate::ke::time::ke_query_interrupt_time();
        // Конвертируем 100ns -> ns: умножаем на 100
        interrupt_time_100ns * 100
    }

    fn stall(&self, microseconds: u64) {
        // Используем HAL функцию ke_stall_execution_processor
        // которая делает точный busy-wait через PIT
        crate::hal::ke_stall_execution_processor(microseconds as u32);
    }

    fn sleep(&self, milliseconds: u64) {
        // Используем KeDelayExecutionThread которая:
        // 1. Устанавливает таймер
        // 2. Переводит поток в Waiting
        // 3. Отдает CPU планировщику
        // 4. Просыпается когда таймер истекает

        // Конвертируем миллисекунды в 100ns единицы с отрицательным знаком
        // (отрицательное = относительное время в NT)
        let interval_100ns = -(milliseconds as i64 * 10_000);

        // TODO(sched): KernelMode = 0, alertable = false
        // crate::ke::sched::ke_delay_execution_thread(0, false, interval_100ns);

        // Временная spin-delay реализация
        let start = crate::ke::time::ke_query_interrupt_time();
        let target = start + interval_100ns.unsigned_abs();
        while crate::ke::time::ke_query_interrupt_time() < target {
            crate::arch::x86_64::cpu::yield_processor();
        }
    }

    // =========================================================================
    // AML Mutex support
    // =========================================================================

    fn create_mutex(&self) -> Handle {
        // TODO: Создать реальный мьютекс через ke::mutex
        // Пока возвращаем фиктивный handle
        static MUTEX_COUNTER: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(1);
        Handle(MUTEX_COUNTER.fetch_add(1, core::sync::atomic::Ordering::Relaxed))
    }

    fn acquire(&self, _mutex: Handle, _timeout: u16) -> Result<(), AmlError> {
        // TODO: Реализовать реальный захват мьютекса
        Ok(())
    }

    fn release(&self, _mutex: Handle) {
        // TODO: Реализовать реальное освобождение мьютекса
    }

    fn breakpoint(&self) {
        // Вызов отладочной точки останова
        unsafe {
            core::arch::asm!("int3");
        }
    }

    fn handle_debug(&self, object: &super::aml::object::Object) {
        // TODO: Вывод в debug console
        let _ = object;
    }

    fn handle_fatal_error(&self, fatal_type: u8, fatal_code: u32, fatal_arg: u64) {
        // TODO: Вызов KeBugCheckEx
        panic!(
            "ACPI Fatal Error: type={}, code={}, arg={}",
            fatal_type, fatal_code, fatal_arg
        );
    }
}

// =============================================================================
// PCI Config Space Access (через порты 0xCF8/0xCFC)
// =============================================================================

const PCI_CONFIG_ADDRESS: u16 = 0xCF8;
const PCI_CONFIG_DATA: u16 = 0xCFC;

/// Формирует адрес для PCI Config Space.
#[inline]
fn pci_config_address(address: PciAddress, offset: u16) -> u32 {
    let bus = address.bus() as u32;
    let device = address.device() as u32;
    let function = address.function() as u32;

    0x8000_0000 | (bus << 16) | (device << 11) | (function << 8) | ((offset as u32) & 0xFC)
}

fn pci_config_read_u8(address: PciAddress, offset: u16) -> u8 {
    let addr = pci_config_address(address, offset);
    write_port_ulong(PCI_CONFIG_ADDRESS, addr);
    let value = read_port_ulong(PCI_CONFIG_DATA);
    ((value >> ((offset & 3) * 8)) & 0xFF) as u8
}

fn pci_config_read_u16(address: PciAddress, offset: u16) -> u16 {
    let addr = pci_config_address(address, offset);
    write_port_ulong(PCI_CONFIG_ADDRESS, addr);
    let value = read_port_ulong(PCI_CONFIG_DATA);
    ((value >> ((offset & 2) * 8)) & 0xFFFF) as u16
}

fn pci_config_read_u32(address: PciAddress, offset: u16) -> u32 {
    let addr = pci_config_address(address, offset);
    write_port_ulong(PCI_CONFIG_ADDRESS, addr);
    read_port_ulong(PCI_CONFIG_DATA)
}

fn pci_config_write_u8(address: PciAddress, offset: u16, value: u8) {
    let addr = pci_config_address(address, offset);
    write_port_ulong(PCI_CONFIG_ADDRESS, addr);
    let current = read_port_ulong(PCI_CONFIG_DATA);
    let shift = (offset & 3) * 8;
    let mask = !(0xFF << shift);
    let new_value = (current & mask) | ((value as u32) << shift);
    write_port_ulong(PCI_CONFIG_DATA, new_value);
}

fn pci_config_write_u16(address: PciAddress, offset: u16, value: u16) {
    let addr = pci_config_address(address, offset);
    write_port_ulong(PCI_CONFIG_ADDRESS, addr);
    let current = read_port_ulong(PCI_CONFIG_DATA);
    let shift = (offset & 2) * 8;
    let mask = !(0xFFFF << shift);
    let new_value = (current & mask) | ((value as u32) << shift);
    write_port_ulong(PCI_CONFIG_DATA, new_value);
}

fn pci_config_write_u32(address: PciAddress, offset: u16, value: u32) {
    let addr = pci_config_address(address, offset);
    write_port_ulong(PCI_CONFIG_ADDRESS, addr);
    write_port_ulong(PCI_CONFIG_DATA, value);
}
