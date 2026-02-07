//! MSR (Model Specific Registers) операции
//!
//! Источники:
//! - NT5: ke/amd64/cpu.c
//! - ReactOS: ke/amd64/cpu.c

#![allow(dead_code)]

/// MSR адреса
pub mod msr_addr {
    /// APIC Base Address
    pub const IA32_APIC_BASE: u32 = 0x1B;

    /// Extended Feature Enable Register
    pub const IA32_EFER: u32 = 0xC0000080;

    /// Kernel GS Base (swapped on syscall)
    pub const IA32_KERNEL_GS_BASE: u32 = 0xC0000102;

    /// GS Base
    pub const IA32_GS_BASE: u32 = 0xC0000101;

    /// FS Base
    pub const IA32_FS_BASE: u32 = 0xC0000100;

    /// STAR - Syscall Target Address Register
    pub const IA32_STAR: u32 = 0xC0000081;

    /// LSTAR - Long Mode Syscall Target Address
    pub const IA32_LSTAR: u32 = 0xC0000082;

    /// CSTAR - Compatibility Mode Syscall Target Address
    pub const IA32_CSTAR: u32 = 0xC0000083;

    /// SFMASK - Syscall Flag Mask
    pub const IA32_FMASK: u32 = 0xC0000084;

    /// Page Attribute Table
    pub const IA32_PAT: u32 = 0x277;

    /// Time Stamp Counter
    pub const IA32_TSC: u32 = 0x10;

    /// MTRR Capabilities
    pub const IA32_MTRRCAP: u32 = 0xFE;

    /// MTRR Default Type
    pub const IA32_MTRR_DEF_TYPE: u32 = 0x2FF;

    /// Sysenter CS
    pub const IA32_SYSENTER_CS: u32 = 0x174;

    /// Sysenter ESP
    pub const IA32_SYSENTER_ESP: u32 = 0x175;

    /// Sysenter EIP
    pub const IA32_SYSENTER_EIP: u32 = 0x176;
}

/// EFER биты
pub mod efer {
    /// System Call Extensions
    pub const SCE: u64 = 1 << 0;
    /// Long Mode Enable
    pub const LME: u64 = 1 << 8;
    /// Long Mode Active
    pub const LMA: u64 = 1 << 10;
    /// No-Execute Enable
    pub const NXE: u64 = 1 << 11;
    /// Secure Virtual Machine Enable
    pub const SVME: u64 = 1 << 12;
    /// Long Mode Segment Limit Enable
    pub const LMSLE: u64 = 1 << 13;
    /// Fast FXSAVE/FXRSTOR
    pub const FFXSR: u64 = 1 << 14;
    /// Translation Cache Extension
    pub const TCE: u64 = 1 << 15;
}

/// Читает MSR
///
/// # Safety
/// Вызывающий должен гарантировать что MSR существует
#[inline]
pub unsafe fn rdmsr(msr: u32) -> u64 {
    let low: u32;
    let high: u32;
    unsafe {
        core::arch::asm!(
            "rdmsr",
            in("ecx") msr,
            out("eax") low,
            out("edx") high,
            options(nomem, nostack, preserves_flags)
        );
    }
    ((high as u64) << 32) | (low as u64)
}

/// Записывает MSR
///
/// # Safety
/// Вызывающий должен гарантировать что MSR существует и значение корректно
#[inline]
pub unsafe fn wrmsr(msr: u32, value: u64) {
    let low = value as u32;
    let high = (value >> 32) as u32;
    unsafe {
        core::arch::asm!(
            "wrmsr",
            in("ecx") msr,
            in("eax") low,
            in("edx") high,
            options(nomem, nostack, preserves_flags)
        );
    }
}

/// Устанавливает GS Base
#[inline]
pub unsafe fn write_gs_base(value: u64) {
    unsafe {
        wrmsr(msr_addr::IA32_GS_BASE, value);
    }
}

/// Читает GS Base
#[inline]
pub unsafe fn read_gs_base() -> u64 {
    unsafe { rdmsr(msr_addr::IA32_GS_BASE) }
}

/// Устанавливает Kernel GS Base (используется при syscall/swapgs)
#[inline]
pub unsafe fn write_kernel_gs_base(value: u64) {
    unsafe {
        wrmsr(msr_addr::IA32_KERNEL_GS_BASE, value);
    }
}

/// Читает Kernel GS Base
#[inline]
pub unsafe fn read_kernel_gs_base() -> u64 {
    unsafe { rdmsr(msr_addr::IA32_KERNEL_GS_BASE) }
}

/// Устанавливает FS Base
#[inline]
pub unsafe fn write_fs_base(value: u64) {
    unsafe {
        wrmsr(msr_addr::IA32_FS_BASE, value);
    }
}

/// Читает FS Base
#[inline]
pub unsafe fn read_fs_base() -> u64 {
    unsafe { rdmsr(msr_addr::IA32_FS_BASE) }
}

/// Читает EFER
#[inline]
pub unsafe fn read_efer() -> u64 {
    unsafe { rdmsr(msr_addr::IA32_EFER) }
}

/// Записывает EFER
#[inline]
pub unsafe fn write_efer(value: u64) {
    unsafe {
        wrmsr(msr_addr::IA32_EFER, value);
    }
}

/// Читает TSC (Time Stamp Counter)
#[inline]
pub fn rdtsc() -> u64 {
    let low: u32;
    let high: u32;
    unsafe {
        core::arch::asm!(
            "rdtsc",
            out("eax") low,
            out("edx") high,
            options(nomem, nostack, preserves_flags)
        );
    }
    ((high as u64) << 32) | (low as u64)
}

/// Настраивает syscall MSR
///
/// # Safety
/// Должен вызываться только при инициализации CPU
pub unsafe fn setup_syscall_msrs(syscall_handler: u64, sysret_cs: u16, syscall_cs: u16) {
    unsafe {
        // STAR: bits 63:48 = SYSRET CS, bits 47:32 = SYSCALL CS
        let star = ((sysret_cs as u64) << 48) | ((syscall_cs as u64) << 32);
        wrmsr(msr_addr::IA32_STAR, star);

        // LSTAR: syscall entry point for 64-bit mode
        wrmsr(msr_addr::IA32_LSTAR, syscall_handler);

        // CSTAR: syscall entry point for compatibility mode (32-bit)
        // Можно установить тот же или другой обработчик
        wrmsr(msr_addr::IA32_CSTAR, syscall_handler);

        // FMASK: flags to clear on syscall (typically IF and TF)
        wrmsr(msr_addr::IA32_FMASK, 0x200 | 0x100); // Clear IF and TF
    }
}
