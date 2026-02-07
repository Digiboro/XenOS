//! Resource Arbiters для PnP Manager
//!
//! Арбитры управляют распределением аппаратных ресурсов между устройствами:
//! - Memory ranges (физическая память для MMIO)
//! - I/O Port ranges
//! - IRQ (прерывания)
//! - DMA channels
//! - Bus numbers
//!
//! Модель WinNT 6.1:
//! - Каждый тип ресурса имеет свой арбитр
//! - Арбитры отслеживают занятые/свободные диапазоны
//! - PnP Manager запрашивает арбитры при START_DEVICE
//! - Конфликты ресурсов разрешаются через rebalance
//!
//! Источники:
//! - ReactOS: ntoskrnl/io/pnpmgr/arbiter.c
//! - Windows 7: ntos/io/pnpmgr/arbiter.c

use alloc::vec::Vec;
use crate::nt::{NTSTATUS, PVOID};
use crate::nt::ntstatus::*;
use crate::ke::spinlock::KSPIN_LOCK;

// Определяем недостающую константу
const STATUS_RESOURCE_IN_USE: NTSTATUS = 0xC0000708u32 as i32;

// =============================================================================
// Resource Range
// =============================================================================

/// Диапазон ресурсов (занятый или свободный)
#[derive(Clone, Copy, Debug)]
pub struct ResourceRange {
    /// Начальный адрес/номер
    pub start: u64,
    /// Конечный адрес/номер (включительно)
    pub end: u64,
    /// Владелец (device node или null для свободного)
    pub owner: PVOID,
    /// Флаги
    pub flags: u32,
}

impl ResourceRange {
    pub const fn new(start: u64, end: u64, owner: PVOID) -> Self {
        Self {
            start,
            end,
            owner,
            flags: 0,
        }
    }

    /// Проверяет пересечение с другим диапазоном
    pub fn overlaps(&self, other: &Self) -> bool {
        self.start <= other.end && other.start <= self.end
    }

    /// Размер диапазона
    pub fn size(&self) -> u64 {
        self.end - self.start + 1
    }
}

// =============================================================================
// Memory Arbiter
// =============================================================================

/// Memory Resource Arbiter
///
/// Управляет распределением физических адресов для MMIO.
pub struct MemoryArbiter {
    /// Список занятых диапазонов
    allocated_ranges: Vec<ResourceRange>,
    /// Spinlock для синхронизации
    lock: KSPIN_LOCK,
}

impl MemoryArbiter {
    pub const fn new() -> Self {
        Self {
            allocated_ranges: Vec::new(),
            lock: KSPIN_LOCK::new(),
        }
    }

    /// Выделяет диапазон памяти
    ///
    /// # Arguments
    /// * `start` - начальный физический адрес
    /// * `length` - размер в байтах
    /// * `owner` - владелец (device node)
    ///
    /// # Returns
    /// STATUS_SUCCESS или ошибка конфликта
    pub unsafe fn allocate_range(&mut self, start: u64, length: u64, owner: PVOID) -> NTSTATUS {
        use crate::ke::spinlock::{ke_acquire_spin_lock, ke_release_spin_lock};

        if length == 0 {
            return STATUS_INVALID_PARAMETER;
        }

        let end = start + length - 1;
        let new_range = ResourceRange::new(start, end, owner);

        let old_irql = ke_acquire_spin_lock(&self.lock);

        // Проверяем конфликты
        for range in &self.allocated_ranges {
            if new_range.overlaps(range) {
                ke_release_spin_lock(&self.lock, old_irql);
                return STATUS_RESOURCE_IN_USE;
            }
        }

        // Добавляем в список
        self.allocated_ranges.push(new_range);

        ke_release_spin_lock(&self.lock, old_irql);
        STATUS_SUCCESS
    }

    /// Освобождает диапазон памяти
    pub unsafe fn free_range(&mut self, start: u64, _length: u64, owner: PVOID) -> NTSTATUS {
        use crate::ke::spinlock::{ke_acquire_spin_lock, ke_release_spin_lock};

        let old_irql = ke_acquire_spin_lock(&self.lock);

        // Ищем и удаляем диапазон
        self.allocated_ranges.retain(|r| r.start != start || r.owner != owner);

        ke_release_spin_lock(&self.lock, old_irql);
        STATUS_SUCCESS
    }
}

// =============================================================================
// Port I/O Arbiter
// =============================================================================

/// Port I/O Resource Arbiter
///
/// Управляет распределением I/O портов.
pub struct PortArbiter {
    /// Список занятых диапазонов
    allocated_ranges: Vec<ResourceRange>,
    /// Spinlock для синхронизации
    lock: KSPIN_LOCK,
}

impl PortArbiter {
    pub const fn new() -> Self {
        Self {
            allocated_ranges: Vec::new(),
            lock: KSPIN_LOCK::new(),
        }
    }

    /// Выделяет диапазон I/O портов
    pub unsafe fn allocate_range(&mut self, start: u64, length: u64, owner: PVOID) -> NTSTATUS {
        use crate::ke::spinlock::{ke_acquire_spin_lock, ke_release_spin_lock};

        if length == 0 {
            return STATUS_INVALID_PARAMETER;
        }

        let end = start + length - 1;
        let new_range = ResourceRange::new(start, end, owner);

        let old_irql = ke_acquire_spin_lock(&self.lock);

        // Проверяем конфликты
        for range in &self.allocated_ranges {
            if new_range.overlaps(range) {
                ke_release_spin_lock(&self.lock, old_irql);
                return STATUS_RESOURCE_IN_USE;
            }
        }

        // Добавляем в список
        self.allocated_ranges.push(new_range);

        ke_release_spin_lock(&self.lock, old_irql);
        STATUS_SUCCESS
    }

    /// Освобождает диапазон портов
    pub unsafe fn free_range(&mut self, start: u64, _length: u64, owner: PVOID) -> NTSTATUS {
        use crate::ke::spinlock::{ke_acquire_spin_lock, ke_release_spin_lock};

        let old_irql = ke_acquire_spin_lock(&self.lock);

        self.allocated_ranges.retain(|r| r.start != start || r.owner != owner);

        ke_release_spin_lock(&self.lock, old_irql);
        STATUS_SUCCESS
    }
}

// =============================================================================
// IRQ Arbiter
// =============================================================================

/// IRQ Resource Arbiter
///
/// Управляет распределением прерываний (IRQ).
pub struct IrqArbiter {
    /// Список занятых IRQ
    allocated_irqs: Vec<ResourceRange>,
    /// Spinlock для синхронизации
    lock: KSPIN_LOCK,
}

impl IrqArbiter {
    pub const fn new() -> Self {
        Self {
            allocated_irqs: Vec::new(),
            lock: KSPIN_LOCK::new(),
        }
    }

    /// Выделяет IRQ
    pub unsafe fn allocate_irq(&mut self, irq: u32, owner: PVOID) -> NTSTATUS {
        use crate::ke::spinlock::{ke_acquire_spin_lock, ke_release_spin_lock};

        let range = ResourceRange::new(irq as u64, irq as u64, owner);

        let old_irql = ke_acquire_spin_lock(&self.lock);

        // Проверяем, не занят ли IRQ
        for r in &self.allocated_irqs {
            if r.start == irq as u64 {
                ke_release_spin_lock(&self.lock, old_irql);
                return STATUS_RESOURCE_IN_USE;
            }
        }

        // Добавляем в список
        self.allocated_irqs.push(range);

        ke_release_spin_lock(&self.lock, old_irql);
        STATUS_SUCCESS
    }

    /// Освобождает IRQ
    pub unsafe fn free_irq(&mut self, irq: u32, owner: PVOID) -> NTSTATUS {
        use crate::ke::spinlock::{ke_acquire_spin_lock, ke_release_spin_lock};

        let old_irql = ke_acquire_spin_lock(&self.lock);

        self.allocated_irqs.retain(|r| r.start != irq as u64 || r.owner != owner);

        ke_release_spin_lock(&self.lock, old_irql);
        STATUS_SUCCESS
    }
}

// =============================================================================
// Global Arbiters
// =============================================================================

/// Глобальный Memory Arbiter
static mut IOP_MEMORY_ARBITER: MemoryArbiter = MemoryArbiter::new();

/// Глобальный Port I/O Arbiter
static mut IOP_PORT_ARBITER: PortArbiter = PortArbiter::new();

/// Глобальный IRQ Arbiter
static mut IOP_IRQ_ARBITER: IrqArbiter = IrqArbiter::new();

// =============================================================================
// Public API
// =============================================================================

/// Инициализирует все арбитры ресурсов
pub unsafe fn iop_init_resource_arbiters() -> NTSTATUS {
    unsafe {
        // Резервируем системные Memory ranges
        let mem_arbiter = &raw mut IOP_MEMORY_ARBITER;
        
        // Низкая память (0x0-0x9FFFF) - реальный режим, BIOS data
        let _ = (*mem_arbiter).allocate_range(0x0, 0xA0000, core::ptr::null_mut());
        
        // VGA/ROM area (0xA0000-0xFFFFF)
        let _ = (*mem_arbiter).allocate_range(0xA0000, 0x60000, core::ptr::null_mut());
        
        // IOAPIC (0xFEC00000-0xFECFFFFF)
        let _ = (*mem_arbiter).allocate_range(0xFEC00000, 0x10000, core::ptr::null_mut());
        
        // Local APIC (0xFEE00000-0xFEEFFFFF)
        let _ = (*mem_arbiter).allocate_range(0xFEE00000, 0x10000, core::ptr::null_mut());
        
        // Резервируем системные I/O Ports
        let port_arbiter = &raw mut IOP_PORT_ARBITER;
        
        // DMA Controller (0x0-0x1F, 0x80-0x9F, 0xC0-0xDF)
        let _ = (*port_arbiter).allocate_range(0x0, 0x20, core::ptr::null_mut());
        let _ = (*port_arbiter).allocate_range(0x80, 0x20, core::ptr::null_mut());
        let _ = (*port_arbiter).allocate_range(0xC0, 0x20, core::ptr::null_mut());
        
        // PIC (0x20-0x21, 0xA0-0xA1)
        let _ = (*port_arbiter).allocate_range(0x20, 0x2, core::ptr::null_mut());
        let _ = (*port_arbiter).allocate_range(0xA0, 0x2, core::ptr::null_mut());
        
        // PIT (0x40-0x43)
        let _ = (*port_arbiter).allocate_range(0x40, 0x4, core::ptr::null_mut());
        
        // RTC (0x70-0x71)
        let _ = (*port_arbiter).allocate_range(0x70, 0x2, core::ptr::null_mut());
        
        // Резервируем системные IRQs
        let irq_arbiter = &raw mut IOP_IRQ_ARBITER;
        
        // System timer (IRQ 0)
        let _ = (*irq_arbiter).allocate_irq(0, core::ptr::null_mut());
        
        // Keyboard (IRQ 1)
        let _ = (*irq_arbiter).allocate_irq(1, core::ptr::null_mut());
        
        // Cascade IRQ (IRQ 2) - связь между PIC1 и PIC2
        let _ = (*irq_arbiter).allocate_irq(2, core::ptr::null_mut());
        
        // RTC (IRQ 8)
        let _ = (*irq_arbiter).allocate_irq(8, core::ptr::null_mut());
        
        // FPU Exception (IRQ 13)
        let _ = (*irq_arbiter).allocate_irq(13, core::ptr::null_mut());
    }

    STATUS_SUCCESS
}

/// Выделяет Memory range
pub unsafe fn iop_allocate_memory_range(start: u64, length: u64, owner: PVOID) -> NTSTATUS {
    unsafe {
        let arbiter = &raw mut IOP_MEMORY_ARBITER;
        (*arbiter).allocate_range(start, length, owner)
    }
}

/// Освобождает Memory range
pub unsafe fn iop_free_memory_range(start: u64, length: u64, owner: PVOID) -> NTSTATUS {
    unsafe {
        let arbiter = &raw mut IOP_MEMORY_ARBITER;
        (*arbiter).free_range(start, length, owner)
    }
}

/// Выделяет Port I/O range
pub unsafe fn iop_allocate_port_range(start: u64, length: u64, owner: PVOID) -> NTSTATUS {
    unsafe {
        let arbiter = &raw mut IOP_PORT_ARBITER;
        (*arbiter).allocate_range(start, length, owner)
    }
}

/// Освобождает Port I/O range
pub unsafe fn iop_free_port_range(start: u64, length: u64, owner: PVOID) -> NTSTATUS {
    unsafe {
        let arbiter = &raw mut IOP_PORT_ARBITER;
        (*arbiter).free_range(start, length, owner)
    }
}

/// Выделяет IRQ
pub unsafe fn iop_allocate_irq(irq: u32, owner: PVOID) -> NTSTATUS {
    unsafe {
        let arbiter = &raw mut IOP_IRQ_ARBITER;
        (*arbiter).allocate_irq(irq, owner)
    }
}

/// Освобождает IRQ
pub unsafe fn iop_free_irq(irq: u32, owner: PVOID) -> NTSTATUS {
    unsafe {
        let arbiter = &raw mut IOP_IRQ_ARBITER;
        (*arbiter).free_irq(irq, owner)
    }
}

