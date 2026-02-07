//! Инициализация CPU
//!
//! Источники:
//! - NT5: ke/amd64/initkr.c, ke/amd64/cpu.c
//! - ReactOS: ke/amd64/kiinit.c, ke/amd64/cpu.c

#![allow(dead_code)]

use super::msr;
use super::pcr::KPCR;

/// CR0 биты
pub mod cr0 {
    pub const PE: u64 = 1 << 0; // Protected Mode Enable
    pub const MP: u64 = 1 << 1; // Monitor Coprocessor
    pub const EM: u64 = 1 << 2; // Emulation
    pub const TS: u64 = 1 << 3; // Task Switched
    pub const ET: u64 = 1 << 4; // Extension Type
    pub const NE: u64 = 1 << 5; // Numeric Error
    pub const WP: u64 = 1 << 16; // Write Protect
    pub const AM: u64 = 1 << 18; // Alignment Mask
    pub const NW: u64 = 1 << 29; // Not Write-Through
    pub const CD: u64 = 1 << 30; // Cache Disable
    pub const PG: u64 = 1 << 31; // Paging
}

/// CR4 биты
pub mod cr4 {
    pub const VME: u64 = 1 << 0; // Virtual-8086 Mode Extensions
    pub const PVI: u64 = 1 << 1; // Protected-Mode Virtual Interrupts
    pub const TSD: u64 = 1 << 2; // Time Stamp Disable
    pub const DE: u64 = 1 << 3; // Debugging Extensions
    pub const PSE: u64 = 1 << 4; // Page Size Extensions
    pub const PAE: u64 = 1 << 5; // Physical Address Extension
    pub const MCE: u64 = 1 << 6; // Machine-Check Enable
    pub const PGE: u64 = 1 << 7; // Page Global Enable
    pub const PCE: u64 = 1 << 8; // Performance-Monitoring Counter Enable
    pub const OSFXSR: u64 = 1 << 9; // OS Support for FXSAVE/FXRSTOR
    pub const OSXMMEXCPT: u64 = 1 << 10; // OS Support for Unmasked SIMD FP Exceptions
    pub const UMIP: u64 = 1 << 11; // User-Mode Instruction Prevention
    pub const VMXE: u64 = 1 << 13; // VMX Enable
    pub const SMXE: u64 = 1 << 14; // SMX Enable
    pub const FSGSBASE: u64 = 1 << 16; // FS/GS BASE Instructions
    pub const PCIDE: u64 = 1 << 17; // PCID Enable
    pub const OSXSAVE: u64 = 1 << 18; // XSAVE and Processor Extended States Enable
    pub const SMEP: u64 = 1 << 20; // Supervisor Mode Execution Prevention
    pub const SMAP: u64 = 1 << 21; // Supervisor Mode Access Prevention
    pub const PKE: u64 = 1 << 22; // Protection Keys Enable
}

/// RFLAGS биты
pub mod rflags {
    pub const CF: u64 = 1 << 0; // Carry Flag
    pub const PF: u64 = 1 << 2; // Parity Flag
    pub const AF: u64 = 1 << 4; // Auxiliary Carry Flag
    pub const ZF: u64 = 1 << 6; // Zero Flag
    pub const SF: u64 = 1 << 7; // Sign Flag
    pub const TF: u64 = 1 << 8; // Trap Flag
    pub const IF: u64 = 1 << 9; // Interrupt Enable Flag
    pub const DF: u64 = 1 << 10; // Direction Flag
    pub const OF: u64 = 1 << 11; // Overflow Flag
    pub const IOPL: u64 = 3 << 12; // I/O Privilege Level
    pub const NT: u64 = 1 << 14; // Nested Task
    pub const RF: u64 = 1 << 16; // Resume Flag
    pub const VM: u64 = 1 << 17; // Virtual-8086 Mode
    pub const AC: u64 = 1 << 18; // Alignment Check
    pub const VIF: u64 = 1 << 19; // Virtual Interrupt Flag
    pub const VIP: u64 = 1 << 20; // Virtual Interrupt Pending
    pub const ID: u64 = 1 << 21; // ID Flag
}

/// Читает CR0
#[inline]
pub fn read_cr0() -> u64 {
    let value: u64;
    unsafe {
        core::arch::asm!("mov {}, cr0", out(reg) value, options(nomem, nostack, preserves_flags));
    }
    value
}

/// Записывает CR0
///
/// # Safety
/// Неправильные значения могут привести к краху системы
#[inline]
pub unsafe fn write_cr0(value: u64) {
    unsafe {
        core::arch::asm!("mov cr0, {}", in(reg) value, options(nomem, nostack, preserves_flags));
    }
}

/// Читает CR2 (адрес page fault)
#[inline]
pub fn read_cr2() -> u64 {
    let value: u64;
    unsafe {
        core::arch::asm!("mov {}, cr2", out(reg) value, options(nomem, nostack, preserves_flags));
    }
    value
}

/// Читает CR3 (page table base)
#[inline]
pub fn read_cr3() -> u64 {
    let value: u64;
    unsafe {
        core::arch::asm!("mov {}, cr3", out(reg) value, options(nomem, nostack, preserves_flags));
    }
    value
}

/// Записывает CR3
///
/// # Safety
/// Неправильные значения приведут к краху
#[inline]
pub unsafe fn write_cr3(value: u64) {
    unsafe {
        core::arch::asm!("mov cr3, {}", in(reg) value, options(nomem, nostack, preserves_flags));
    }
}

/// Читает CR4
#[inline]
pub fn read_cr4() -> u64 {
    let value: u64;
    unsafe {
        core::arch::asm!("mov {}, cr4", out(reg) value, options(nomem, nostack, preserves_flags));
    }
    value
}

/// Записывает CR4
///
/// # Safety
/// Неправильные значения могут привести к краху системы
#[inline]
pub unsafe fn write_cr4(value: u64) {
    unsafe {
        core::arch::asm!("mov cr4, {}", in(reg) value, options(nomem, nostack, preserves_flags));
    }
}

/// Читает CR8 (Task Priority Register / IRQL на AMD64)
///
/// На AMD64 CR8 используется для hardware IRQL:
/// - CR8[3:0] = IRQL (0-15)
/// - Прерывания с приоритетом <= CR8 блокируются
///
/// Источники:
/// - Intel SDM Vol. 3A, Section 2.5
/// - AMD64 APM Vol. 2, Section 3.1.9
#[inline]
pub fn read_cr8() -> u64 {
    let value: u64;
    unsafe {
        core::arch::asm!("mov {}, cr8", out(reg) value, options(nomem, nostack, preserves_flags));
    }
    value
}

/// Записывает CR8 (Task Priority Register / IRQL на AMD64)
///
/// Устанавливает hardware IRQL. Прерывания с приоритетом <= value
/// будут заблокированы до понижения CR8.
///
/// # Safety
/// Должен вызываться только из kernel mode.
/// Неправильные значения могут привести к пропуску прерываний.
#[inline]
pub unsafe fn write_cr8(value: u64) {
    unsafe {
        core::arch::asm!("mov cr8, {}", in(reg) value, options(nomem, nostack, preserves_flags));
    }
}

/// Читает RFLAGS
#[inline]
pub fn read_rflags() -> u64 {
    let value: u64;
    unsafe {
        core::arch::asm!("pushfq; pop {}", out(reg) value, options(nomem, preserves_flags));
    }
    value
}

/// Инвалидирует TLB
#[inline]
pub fn flush_tlb() {
    unsafe {
        let cr3 = read_cr3();
        write_cr3(cr3);
    }
}

/// Инвалидирует одну страницу TLB
#[inline]
pub fn invlpg(addr: u64) {
    unsafe {
        core::arch::asm!("invlpg [{}]", in(reg) addr, options(nostack, preserves_flags));
    }
}

/// Включает прерывания
#[inline]
pub fn enable_interrupts() {
    unsafe {
        core::arch::asm!("sti", options(nomem, nostack));
    }
}

/// Выключает прерывания
#[inline]
pub fn disable_interrupts() {
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack));
    }
}

/// Проверяет включены ли прерывания
#[inline]
pub fn interrupts_enabled() -> bool {
    read_rflags() & rflags::IF != 0
}

/// Halt - останавливает CPU до прерывания
#[inline]
pub fn halt() {
    unsafe {
        core::arch::asm!("hlt", options(nomem, nostack, preserves_flags));
    }
}

/// YieldProcessor
///
/// Уступает процессорное время в spin-wait циклах.
/// На x86-64 выполняет инструкцию PAUSE (F3 90), которая:
/// - Сигнализирует процессору о spin-wait loop
/// - Предотвращает memory order violations в spin-loops
/// - Снижает энергопотребление в цикле ожидания
/// - Улучшает производительность на Hyper-Threading процессорах
///
/// Источники:
/// - Intel SDM Vol. 2B: PAUSE instruction
#[inline]
pub fn yield_processor() {
    core::hint::spin_loop();
}

/// CPUID результат
#[derive(Debug, Clone, Copy, Default)]
pub struct CpuIdResult {
    pub eax: u32,
    pub ebx: u32,
    pub ecx: u32,
    pub edx: u32,
}

/// Выполняет CPUID
#[inline]
pub fn cpuid(leaf: u32, sub_leaf: u32) -> CpuIdResult {
    let mut result = CpuIdResult::default();
    unsafe {
        // rbx зарезервирован LLVM, поэтому сохраняем через другой регистр
        core::arch::asm!(
            "push rbx",
            "cpuid",
            "mov {ebx_out:e}, ebx",
            "pop rbx",
            inout("eax") leaf => result.eax,
            inout("ecx") sub_leaf => result.ecx,
            ebx_out = out(reg) result.ebx,
            lateout("edx") result.edx,
            options(nomem, preserves_flags)
        );
    }
    result
}

/// CPUID Feature bits (EDX from leaf 1)
pub mod cpuid_features_edx {
    pub const FPU: u32 = 1 << 0;
    pub const VME: u32 = 1 << 1;
    pub const DE: u32 = 1 << 2;
    pub const PSE: u32 = 1 << 3;
    pub const TSC: u32 = 1 << 4;
    pub const MSR: u32 = 1 << 5;
    pub const PAE: u32 = 1 << 6;
    pub const MCE: u32 = 1 << 7;
    pub const CX8: u32 = 1 << 8;
    pub const APIC: u32 = 1 << 9;
    pub const SEP: u32 = 1 << 11;
    pub const MTRR: u32 = 1 << 12;
    pub const PGE: u32 = 1 << 13;
    pub const MCA: u32 = 1 << 14;
    pub const CMOV: u32 = 1 << 15;
    pub const PAT: u32 = 1 << 16;
    pub const PSE36: u32 = 1 << 17;
    pub const PSN: u32 = 1 << 18;
    pub const CLFSH: u32 = 1 << 19;
    pub const DS: u32 = 1 << 21;
    pub const ACPI: u32 = 1 << 22;
    pub const MMX: u32 = 1 << 23;
    pub const FXSR: u32 = 1 << 24;
    pub const SSE: u32 = 1 << 25;
    pub const SSE2: u32 = 1 << 26;
    pub const SS: u32 = 1 << 27;
    pub const HTT: u32 = 1 << 28;
    pub const TM: u32 = 1 << 29;
    pub const PBE: u32 = 1 << 31;
}

/// Информация о процессоре
#[derive(Debug, Default)]
pub struct CpuInfo {
    pub vendor: [u8; 12],
    pub family: u8,
    pub model: u8,
    pub stepping: u8,
    pub features_edx: u32,
    pub features_ecx: u32,
    pub max_cpuid: u32,
    pub max_cpuid_ext: u32,
}

impl CpuInfo {
    /// Получает информацию о CPU через CPUID
    pub fn detect() -> Self {
        let mut info = Self::default();

        // Leaf 0: Vendor ID
        let result = cpuid(0, 0);
        info.max_cpuid = result.eax;

        // Vendor string: EBX + EDX + ECX
        info.vendor[0..4].copy_from_slice(&result.ebx.to_le_bytes());
        info.vendor[4..8].copy_from_slice(&result.edx.to_le_bytes());
        info.vendor[8..12].copy_from_slice(&result.ecx.to_le_bytes());

        // Leaf 1: Version and features
        if info.max_cpuid >= 1 {
            let result = cpuid(1, 0);

            info.stepping = (result.eax & 0xF) as u8;
            info.model = ((result.eax >> 4) & 0xF) as u8;
            info.family = ((result.eax >> 8) & 0xF) as u8;

            // Extended model/family for family >= 0xF
            if info.family == 0xF {
                info.family += ((result.eax >> 20) & 0xFF) as u8;
            }
            if info.family >= 6 {
                info.model += (((result.eax >> 16) & 0xF) << 4) as u8;
            }

            info.features_edx = result.edx;
            info.features_ecx = result.ecx;
        }

        // Extended CPUID
        let result = cpuid(0x80000000, 0);
        info.max_cpuid_ext = result.eax;

        info
    }

    /// Проверяет поддержку SSE
    pub fn has_sse(&self) -> bool {
        self.features_edx & cpuid_features_edx::SSE != 0
    }

    /// Проверяет поддержку SSE2
    pub fn has_sse2(&self) -> bool {
        self.features_edx & cpuid_features_edx::SSE2 != 0
    }

    /// Проверяет поддержку FXSAVE/FXRSTOR
    pub fn has_fxsr(&self) -> bool {
        self.features_edx & cpuid_features_edx::FXSR != 0
    }

    /// Возвращает vendor string
    pub fn vendor_string(&self) -> &str {
        core::str::from_utf8(&self.vendor).unwrap_or("Unknown")
    }
}

/// Настраивает CPU control registers для SSE/SSE2
///
/// Биты CR0:
/// - MP (Monitor coprocessor) = 1 - для корректной работы с FPU
/// - EM (Emulation) = 0 - отключаем эмуляцию FPU
/// - TS (Task Switched) = 0 - разрешаем SSE без #NM
/// - NE (Numeric Error) = 1 - используем native FPU exceptions
/// - WP (Write Protect) = 1 - защита страниц в kernel mode
///
/// Биты CR4:
/// - OSFXSR = 1 - разрешаем FXSAVE/FXRSTOR
/// - OSXMMEXCPT = 1 - разрешаем SSE exceptions (#XM)
/// - OSXSAVE = 1 - разрешаем XSAVE (если поддерживается)
///
/// # Safety
/// Должен вызываться только при инициализации
pub unsafe fn setup_control_registers() {
    unsafe {
        // CR0: Настройка для SSE/FPU
        let mut cr0 = read_cr0();
        cr0 |= cr0::MP | cr0::WP | cr0::NE;
        cr0 &= !(cr0::EM | cr0::TS | cr0::CD | cr0::NW); // Сброс: эмуляция, task switched, cache disable
        write_cr0(cr0);

        // CR4: Включаем поддержку SSE и XSAVE
        let mut cr4 = read_cr4();
        cr4 |= cr4::OSFXSR | cr4::OSXMMEXCPT | cr4::MCE | cr4::PGE | cr4::DE;

        // Проверяем поддержку XSAVE через CPUID
        let cpuid_result = cpuid(1, 0);
        if cpuid_result.ecx & (1 << 26) != 0 {
            // XSAVE поддерживается
            cr4 |= cr4::OSXSAVE;
        }

        write_cr4(cr4);
    }
}

/// Инициализирует GS base для PCR
///
/// # Safety
/// PCR должен быть выделен и валиден
pub unsafe fn setup_gs_base(pcr: &mut KPCR) {
    let pcr_addr = pcr as *mut KPCR as u64;
    unsafe {
        // Kernel GS base (для swapgs)
        msr::write_kernel_gs_base(pcr_addr);
        // GS base (текущий)
        msr::write_gs_base(pcr_addr);
    }
}
