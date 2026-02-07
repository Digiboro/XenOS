//! GDT (Global Descriptor Table) для x86_64
//!
//! Источники:
//! - NT5: ke/amd64/initkr.c
//! - ReactOS: ke/amd64/kiinit.c

#![allow(dead_code)]

use core::mem::size_of;

/// Селекторы GDT (из NT)
pub mod selector {
    pub const KGDT64_NULL: u16 = 0x00;
    pub const KGDT64_R0_CODE: u16 = 0x10;
    pub const KGDT64_R0_DATA: u16 = 0x18;
    pub const KGDT64_R3_CMCODE: u16 = 0x20; // Compatibility mode code (32-bit)
    pub const KGDT64_R3_DATA: u16 = 0x28;
    pub const KGDT64_R3_CODE: u16 = 0x30;
    pub const KGDT64_SYS_TSS: u16 = 0x40;
    pub const KGDT64_R3_CMTEB: u16 = 0x50; // Compatibility mode TEB

    /// Ring 0 privilege level
    pub const RPL_RING0: u16 = 0;
    /// Ring 3 privilege level
    pub const RPL_RING3: u16 = 3;
}

/// Флаги доступа дескриптора
mod access {
    /// Present bit
    pub const PRESENT: u8 = 1 << 7;
    /// DPL Ring 0
    pub const DPL_RING0: u8 = 0 << 5;
    /// DPL Ring 3
    pub const DPL_RING3: u8 = 3 << 5;
    /// Descriptor type (1 = code/data)
    pub const DESCRIPTOR_TYPE: u8 = 1 << 4;
    /// Executable
    pub const EXECUTABLE: u8 = 1 << 3;
    /// Conforming/Direction
    pub const CONFORMING: u8 = 1 << 2;
    /// Readable (code) / Writable (data)
    pub const RW: u8 = 1 << 1;
    /// Accessed
    pub const ACCESSED: u8 = 1 << 0;

    // System segment types
    pub const TSS_AVAILABLE: u8 = 0x9;
    pub const TSS_BUSY: u8 = 0xB;
}

/// Флаги дескриптора (верхние 4 бита)
mod flags {
    /// Granularity (1 = 4KB pages)
    pub const GRANULARITY: u8 = 1 << 3;
    /// Size (1 = 32-bit, 0 = 16-bit) - должен быть 0 для 64-bit code
    pub const SIZE_32: u8 = 1 << 2;
    /// Long mode (1 = 64-bit code)
    pub const LONG_MODE: u8 = 1 << 1;
}

/// Дескриптор сегмента GDT (8 байт)
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct GdtEntry {
    limit_low: u16,
    base_low: u16,
    base_middle: u8,
    access: u8,
    limit_high_flags: u8, // bits 0-3 = limit high, bits 4-7 = flags
    base_high: u8,
}

impl GdtEntry {
    /// Создает null дескриптор
    pub const fn null() -> Self {
        Self {
            limit_low: 0,
            base_low: 0,
            base_middle: 0,
            access: 0,
            limit_high_flags: 0,
            base_high: 0,
        }
    }

    /// Создает дескриптор кода/данных
    pub const fn new(base: u32, limit: u32, access: u8, flags: u8) -> Self {
        Self {
            limit_low: limit as u16,
            base_low: base as u16,
            base_middle: (base >> 16) as u8,
            access,
            limit_high_flags: ((limit >> 16) as u8 & 0x0F) | ((flags & 0x0F) << 4),
            base_high: (base >> 24) as u8,
        }
    }

    /// Создает 64-bit kernel code дескриптор
    pub const fn kernel_code_64() -> Self {
        Self::new(
            0,
            0xFFFFF,
            access::PRESENT
                | access::DPL_RING0
                | access::DESCRIPTOR_TYPE
                | access::EXECUTABLE
                | access::RW,
            flags::GRANULARITY | flags::LONG_MODE,
        )
    }

    /// Создает kernel data дескриптор
    pub const fn kernel_data() -> Self {
        Self::new(
            0,
            0xFFFFF,
            access::PRESENT | access::DPL_RING0 | access::DESCRIPTOR_TYPE | access::RW,
            flags::GRANULARITY | flags::SIZE_32,
        )
    }

    /// Создает 32-bit user code дескриптор (compatibility mode)
    pub const fn user_code_32() -> Self {
        Self::new(
            0,
            0xFFFFF,
            access::PRESENT
                | access::DPL_RING3
                | access::DESCRIPTOR_TYPE
                | access::EXECUTABLE
                | access::RW,
            flags::GRANULARITY | flags::SIZE_32,
        )
    }

    /// Создает user data дескриптор
    pub const fn user_data() -> Self {
        Self::new(
            0,
            0xFFFFF,
            access::PRESENT | access::DPL_RING3 | access::DESCRIPTOR_TYPE | access::RW,
            flags::GRANULARITY | flags::SIZE_32,
        )
    }

    /// Создает 64-bit user code дескриптор
    pub const fn user_code_64() -> Self {
        Self::new(
            0,
            0xFFFFF,
            access::PRESENT
                | access::DPL_RING3
                | access::DESCRIPTOR_TYPE
                | access::EXECUTABLE
                | access::RW,
            flags::GRANULARITY | flags::LONG_MODE,
        )
    }
}

/// Дескриптор TSS (16 байт для 64-bit)
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct TssDescriptor {
    limit_low: u16,
    base_low: u16,
    base_middle: u8,
    access: u8,
    limit_high_flags: u8,
    base_high: u8,
    base_upper: u32,
    reserved: u32,
}

impl TssDescriptor {
    pub const fn null() -> Self {
        Self {
            limit_low: 0,
            base_low: 0,
            base_middle: 0,
            access: 0,
            limit_high_flags: 0,
            base_high: 0,
            base_upper: 0,
            reserved: 0,
        }
    }

    /// Создает TSS дескриптор
    pub fn new(base: u64, limit: u32) -> Self {
        Self {
            limit_low: limit as u16,
            base_low: base as u16,
            base_middle: (base >> 16) as u8,
            access: access::PRESENT | access::TSS_AVAILABLE,
            limit_high_flags: ((limit >> 16) as u8) & 0x0F,
            base_high: (base >> 24) as u8,
            base_upper: (base >> 32) as u32,
            reserved: 0,
        }
    }
}

/// GDT для одного процессора
/// Offsets соответствуют NT селекторам
#[repr(C, align(16))]
pub struct Gdt {
    pub null: GdtEntry,         // 0x00
    pub reserved: GdtEntry,     // 0x08 (reserved для совместимости с NT layout)
    pub kernel_code: GdtEntry,  // 0x10
    pub kernel_data: GdtEntry,  // 0x18
    pub user_code_32: GdtEntry, // 0x20 (compatibility mode)
    pub user_data: GdtEntry,    // 0x28
    pub user_code_64: GdtEntry, // 0x30
    pub reserved2: GdtEntry,    // 0x38 (padding before TSS)
    pub tss: TssDescriptor,     // 0x40 (16 bytes)
    pub user_teb: GdtEntry,     // 0x50 (для compatibility mode TEB)
}

impl Gdt {
    /// Создает новую GDT
    pub const fn new() -> Self {
        Self {
            null: GdtEntry::null(),
            reserved: GdtEntry::null(),
            kernel_code: GdtEntry::kernel_code_64(),
            kernel_data: GdtEntry::kernel_data(),
            user_code_32: GdtEntry::user_code_32(),
            user_data: GdtEntry::user_data(),
            user_code_64: GdtEntry::user_code_64(),
            reserved2: GdtEntry::null(),
            tss: TssDescriptor::null(),
            user_teb: GdtEntry::null(),
        }
    }

    /// Устанавливает TSS дескриптор
    pub fn set_tss(&mut self, tss_addr: u64) {
        self.tss = TssDescriptor::new(tss_addr, (size_of::<super::tss::Tss64>() - 1) as u32);
    }

    /// Возвращает указатель на GDT для GDTR
    pub fn pointer(&self) -> GdtPointer {
        GdtPointer {
            limit: (size_of::<Self>() - 1) as u16,
            base: self as *const _ as u64,
        }
    }
}

impl Default for Gdt {
    fn default() -> Self {
        Self::new()
    }
}

/// GDTR структура
#[repr(C, packed)]
pub struct GdtPointer {
    pub limit: u16,
    pub base: u64,
}

/// Загружает GDT
///
/// # Safety
/// GDT должна быть корректно инициализирована и оставаться валидной
pub unsafe fn load_gdt(gdt_ptr: &GdtPointer) {
    unsafe {
        core::arch::asm!(
            "lgdt [{}]",
            in(reg) gdt_ptr,
            options(nostack, preserves_flags)
        );
    }
}

/// Перезагружает сегментные регистры после загрузки GDT
///
/// # Safety
/// GDT должна быть загружена и содержать корректные дескрипторы
pub unsafe fn reload_segments(code_selector: u16, data_selector: u16) {
    unsafe {
        // Загружаем CS через far return
        core::arch::asm!(
            "push {sel}",
            "lea {tmp}, [rip + 2f]",
            "push {tmp}",
            "retfq",
            "2:",
            sel = in(reg) code_selector as u64,
            tmp = lateout(reg) _,
            options(preserves_flags)
        );

        // Загружаем остальные сегментные регистры
        core::arch::asm!(
            "mov ds, {sel:x}",
            "mov es, {sel:x}",
            "mov ss, {sel:x}",
            sel = in(reg) data_selector as u32,
            options(nostack, preserves_flags)
        );

        // FS и GS обнуляем (будут установлены через MSR)
        core::arch::asm!(
            "mov fs, {zero:x}",
            "mov gs, {zero:x}",
            zero = in(reg) 0u32,
            options(nostack, preserves_flags)
        );
    }
}

/// Загружает TR (Task Register) для TSS
///
/// # Safety
/// TSS дескриптор должен быть корректно установлен в GDT
pub unsafe fn load_tss(tss_selector: u16) {
    unsafe {
        core::arch::asm!(
            "ltr {sel:x}",
            sel = in(reg) tss_selector as u32,
            options(nostack, preserves_flags)
        );
    }
}
