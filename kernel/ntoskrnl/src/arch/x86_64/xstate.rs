//! Расширенное состояние процессора (XSAVE/XRSTOR)
//!
//! Управление расширенным состоянием процессора (FPU/SSE/AVX).
//!
//! Источники:
//! - ReactOS: ke/amd64/xstate.c, include/internal/amd64/intrin_i.h
//! - NT5: ke/amd64/cpu.c

#![allow(dead_code)]

use crate::nt::PVOID;

// =============================================================================
// Индексы компонентов XSTATE
// =============================================================================

/// Устаревшее состояние x87 FPU
pub const XSTATE_LEGACY_FLOATING_POINT: u32 = 0;

/// Устаревшее состояние SSE (регистры XMM)
pub const XSTATE_LEGACY_SSE: u32 = 1;

/// Состояние AVX (регистры YMM)
pub const XSTATE_GSSE: u32 = 2;
pub const XSTATE_AVX: u32 = XSTATE_GSSE;

/// Регистры границ MPX
pub const XSTATE_MPX_BNDREGS: u32 = 3;

/// Конфигурация границ MPX
pub const XSTATE_MPX_BNDCSR: u32 = 4;

/// Регистры масок AVX-512
pub const XSTATE_AVX512_KMASK: u32 = 5;

/// AVX-512 ZMM_Hi256
pub const XSTATE_AVX512_ZMM_H: u32 = 6;

/// AVX-512 Hi16_ZMM
pub const XSTATE_AVX512_ZMM: u32 = 7;

/// Трассировка процессора
pub const XSTATE_IPT: u32 = 8;

/// Ключи защиты для пользовательских страниц
pub const XSTATE_PKRU: u32 = 9;

/// Состояние PASID
pub const XSTATE_PASID: u32 = 10;

/// Состояние CET для пользовательского режима
pub const XSTATE_CET_U: u32 = 11;

/// Состояние CET для режима супервизора
pub const XSTATE_CET_S: u32 = 12;

/// Аппаратное управление энергопотреблением
pub const XSTATE_HDC: u32 = 13;

/// Пользовательские прерывания
pub const XSTATE_UINTR: u32 = 14;

/// Запись последней ветви
pub const XSTATE_LBR: u32 = 15;

/// Максимальное количество функций XSTATE
pub const MAXIMUM_XSTATE_FEATURES: usize = 64;

// =============================================================================
// Маски XSTATE
// =============================================================================

/// Маска для устаревшего состояния FPU
pub const XSTATE_MASK_LEGACY_FLOATING_POINT: u64 = 1 << XSTATE_LEGACY_FLOATING_POINT;

/// Маска для устаревшего состояния SSE
pub const XSTATE_MASK_LEGACY_SSE: u64 = 1 << XSTATE_LEGACY_SSE;

/// Маска для устаревшего состояния (FPU + SSE)
pub const XSTATE_MASK_LEGACY: u64 = XSTATE_MASK_LEGACY_FLOATING_POINT | XSTATE_MASK_LEGACY_SSE;

/// Маска для состояния AVX
pub const XSTATE_MASK_GSSE: u64 = 1 << XSTATE_GSSE;
pub const XSTATE_MASK_AVX: u64 = XSTATE_MASK_GSSE;

/// Маска для состояния MPX
pub const XSTATE_MASK_MPX: u64 = (1 << XSTATE_MPX_BNDREGS) | (1 << XSTATE_MPX_BNDCSR);

/// Маска для состояния AVX-512
pub const XSTATE_MASK_AVX512: u64 =
    (1 << XSTATE_AVX512_KMASK) | (1 << XSTATE_AVX512_ZMM_H) | (1 << XSTATE_AVX512_ZMM);

/// Маска для состояния IPT
pub const XSTATE_MASK_IPT: u64 = 1 << XSTATE_IPT;

/// Маска для состояния PKRU
pub const XSTATE_MASK_PKRU: u64 = 1 << XSTATE_PKRU;

/// Маска для состояния PASID
pub const XSTATE_MASK_PASID: u64 = 1 << XSTATE_PASID;

/// Маска для состояния CET пользовательского режима
pub const XSTATE_MASK_CET_U: u64 = 1 << XSTATE_CET_U;

/// Маска для состояния CET супервизора
pub const XSTATE_MASK_CET_S: u64 = 1 << XSTATE_CET_S;

/// Маска для состояния HDC
pub const XSTATE_MASK_HDC: u64 = 1 << XSTATE_HDC;

/// Маска для состояния UINTR
pub const XSTATE_MASK_UINTR: u64 = 1 << XSTATE_UINTR;

/// Маска для состояния LBR
pub const XSTATE_MASK_LBR: u64 = 1 << XSTATE_LBR;

// =============================================================================
// Биты возможностей ядра (для определения поддержки XSAVE)
// =============================================================================

/// Поддерживается функция XSTATE
pub const KF_XSTATE: u64 = 0x00800000;

/// Поддерживается инструкция XSAVEOPT
pub const KF_XSAVEOPT: u64 = 0x00008000;

/// Поддерживаются инструкции XSAVES/XRSTORS
pub const KF_XSAVES: u64 = 0x0000004000000000;

/// Поддерживаются инструкции FXSAVE/FXRSTOR
pub const KF_FXSR: u64 = 0x00000800;

// =============================================================================
// M128A - 128-битный регистр SSE
// =============================================================================

/// M128A - 128-битное значение для регистров XMM
#[repr(C, align(16))]
#[derive(Debug, Clone, Copy, Default)]
pub struct M128A {
    pub low: u64,
    pub high: i64,
}

impl M128A {
    /// Создает нулевой M128A
    pub const fn new() -> Self {
        Self { low: 0, high: 0 }
    }
}

// =============================================================================
// XSAVE_FORMAT - Устаревшее состояние FPU/SSE (512 байт)
// =============================================================================

/// XSAVE_FORMAT - формат состояния FPU/SSE (формат FXSAVE)
///
/// Это устаревшая 512-байтная область, используемая FXSAVE/FXRSTOR.
/// Также является первой частью области XSAVE.
#[repr(C, align(16))]
#[derive(Clone, Copy)]
pub struct XSAVE_FORMAT {
    /// Управляющее слово FPU
    pub control_word: u16,
    /// Слово состояния FPU
    pub status_word: u16,
    /// Слово тегов FPU (сжатое)
    pub tag_word: u8,
    /// Зарезервировано
    pub reserved1: u8,
    /// Код операции FPU
    pub error_opcode: u16,
    /// Смещение указателя инструкции FPU
    pub error_offset: u32,
    /// Селектор указателя инструкции FPU
    pub error_selector: u16,
    /// Зарезервировано
    pub reserved2: u16,
    /// Смещение указателя данных FPU
    pub data_offset: u32,
    /// Селектор указателя данных FPU
    pub data_selector: u16,
    /// Зарезервировано
    pub reserved3: u16,
    /// Регистр MXCSR
    pub mxcsr: u32,
    /// Маска MXCSR
    pub mxcsr_mask: u32,
    /// Регистры FPU/MMX (ST0-ST7 / MM0-MM7)
    pub float_registers: [M128A; 8],
    /// Регистры XMM (XMM0-XMM15 на x64)
    pub xmm_registers: [M128A; 16],
    /// Зарезервировано
    pub reserved4: [u8; 96],
}

impl XSAVE_FORMAT {
    pub const SIZE: usize = core::mem::size_of::<Self>();
}

impl Default for XSAVE_FORMAT {
    fn default() -> Self {
        Self {
            control_word: INITIAL_FPCSR,
            status_word: 0,
            tag_word: 0,
            reserved1: 0,
            error_opcode: 0,
            error_offset: 0,
            error_selector: 0,
            reserved2: 0,
            data_offset: 0,
            data_selector: 0,
            reserved3: 0,
            mxcsr: INITIAL_MXCSR,
            mxcsr_mask: 0xFFFF,
            float_registers: [M128A::default(); 8],
            xmm_registers: [M128A::default(); 16],
            reserved4: [0; 96],
        }
    }
}

// Проверка что размер 512 байт
const _: () = assert!(XSAVE_FORMAT::SIZE == 512);

// =============================================================================
// XSAVE_AREA_HEADER - Заголовок XSAVE (64 байта)
// =============================================================================

/// XSAVE_AREA_HEADER - заголовок для расширенного состояния
#[repr(C, align(8))]
#[derive(Debug, Clone, Copy, Default)]
pub struct XSAVE_AREA_HEADER {
    /// Битовая маска присутствующих компонентов состояния
    pub mask: u64,
    /// Маска уплотнения (бит 63 = уплотненный формат)
    pub compaction_mask: u64,
    /// Зарезервировано
    pub reserved2: [u64; 6],
}

impl XSAVE_AREA_HEADER {
    pub const SIZE: usize = core::mem::size_of::<Self>();
}

// Проверка что размер 64 байта
const _: () = assert!(XSAVE_AREA_HEADER::SIZE == 64);

// =============================================================================
// XSAVE_AREA - Полная область XSAVE (576+ байт)
// =============================================================================

/// XSAVE_AREA - базовая область XSAVE (устаревшее состояние + заголовок)
///
/// Это минимальная область XSAVE, содержащая устаревшее состояние FPU/SSE
/// и заголовок XSAVE. Расширенные компоненты (AVX и т.д.) следуют
/// после этой структуры.
#[repr(C, align(64))]
#[derive(Clone, Copy)]
pub struct XSAVE_AREA {
    /// Устаревшее состояние FPU/SSE
    pub legacy_state: XSAVE_FORMAT,
    /// Заголовок XSAVE
    pub header: XSAVE_AREA_HEADER,
}

impl XSAVE_AREA {
    pub const SIZE: usize = core::mem::size_of::<Self>();
}

impl Default for XSAVE_AREA {
    fn default() -> Self {
        Self {
            legacy_state: XSAVE_FORMAT::default(),
            header: XSAVE_AREA_HEADER::default(),
        }
    }
}

// Проверка что размер 576 байт
const _: () = assert!(XSAVE_AREA::SIZE == 576);

// =============================================================================
// Начальные значения состояния FPU
// =============================================================================

/// Начальное значение MXCSR (все исключения замаскированы)
pub const INITIAL_MXCSR: u32 = 0x1F80;

/// Начальное управляющее слово FPU (двойная точность, все исключения замаскированы)
pub const INITIAL_FPCSR: u16 = 0x027F;

// =============================================================================
// Глобальные переменные состояния
// =============================================================================

use core::sync::atomic::AtomicU64;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;

/// Биты возможностей ядра (флаги поддержки XSTATE)
pub static KE_FEATURE_BITS: AtomicU64 = AtomicU64::new(0);

/// Размер области XSAVE для всех включенных компонентов
pub static KE_XSTATE_LENGTH: AtomicUsize = AtomicUsize::new(XSAVE_AREA::SIZE);

/// Получить текущие биты возможностей
#[inline]
pub fn ke_feature_bits() -> u64 {
    KE_FEATURE_BITS.load(Ordering::Relaxed)
}

/// Получить текущую длину XSTATE
#[inline]
pub fn ke_xstate_length() -> usize {
    KE_XSTATE_LENGTH.load(Ordering::Relaxed)
}

// =============================================================================
// Функции XSAVE/XRSTOR
// =============================================================================

/// KiSaveXState - сохранение расширенного состояния процессора
///
/// Сохраняет расширенное состояние процессора в указанный буфер,
/// используя наиболее эффективную доступную инструкцию
/// (XSAVES > XSAVEOPT > XSAVE > FXSAVE).
///
/// # Аргументы
/// * `buffer` - указатель на область сохранения с выравниванием 64 байта
/// * `component_mask` - битовая маска компонентов состояния для сохранения
///
/// # Безопасность
/// - Буфер должен быть правильно выровнен (64 байта для XSAVE, 16 для FXSAVE)
/// - Буфер должен быть достаточно большим для запрошенных компонентов
#[inline]
pub unsafe fn ki_save_x_state(buffer: PVOID, component_mask: u64) {
    unsafe {
        let features = ke_feature_bits();

        // Состояние SSE всегда сохраняется как часть устаревшего состояния
        let mask = component_mask & !XSTATE_MASK_LEGACY_SSE;

        if features & KF_XSAVES != 0 {
            // Использовать XSAVES (поддержка состояния супервизора, уплотненный формат)
            xsaves64(buffer, mask);
        } else if features & KF_XSAVEOPT != 0 {
            // Использовать XSAVEOPT (оптимизированный, пропускает неизмененные компоненты)
            xsaveopt64(buffer, mask);
        } else if features & KF_XSTATE != 0 {
            // Использовать XSAVE (базовый)
            xsave64(buffer, mask);
        } else if features & KF_FXSR != 0 {
            // Откат к FXSAVE (только устаревшее FPU/SSE)
            fxsave64(buffer);
        }
        // Если нет возможности сохранения FPU, ничего не делать
    }
}

/// KiRestoreXState - восстановление расширенного состояния процессора
///
/// Восстанавливает расширенное состояние процессора из указанного буфера,
/// используя наиболее эффективную доступную инструкцию
/// (XRSTORS > XRSTOR > FXRSTOR).
///
/// # Аргументы
/// * `buffer` - указатель на область сохранения с ранее сохраненным состоянием
/// * `component_mask` - битовая маска компонентов состояния для восстановления
///
/// # Безопасность
/// - Буфер должен быть правильно выровнен
/// - Буфер должен содержать валидное состояние для запрошенных компонентов
#[inline]
pub unsafe fn ki_restore_x_state(buffer: PVOID, component_mask: u64) {
    unsafe {
        let features = ke_feature_bits();

        // Состояние SSE всегда восстанавливается как часть устаревшего состояния
        let mask = component_mask & !XSTATE_MASK_LEGACY_SSE;

        if features & KF_XSAVES != 0 {
            // Использовать XRSTORS (поддержка состояния супервизора, уплотненный формат)
            xrstors64(buffer, mask);
        } else if features & KF_XSTATE != 0 {
            // Использовать XRSTOR (базовый)
            xrstor64(buffer, mask);
        } else if features & KF_FXSR != 0 {
            // Откат к FXRSTOR (только устаревшее FPU/SSE)
            fxrstor64(buffer);
        }
        // Если нет возможности восстановления FPU, ничего не делать
    }
}

/// Инициализация области XSAVE начальным состоянием FPU
///
/// Настраивает область сохранения с начальными управляющими значениями,
/// чтобы поток начинал с чистым состоянием FPU.
pub unsafe fn ki_initialize_x_state(save_area: *mut XSAVE_AREA) {
    unsafe {
        if save_area.is_null() {
            return;
        }

        // Обнуляем всю область
        let length = ke_xstate_length();
        core::ptr::write_bytes(save_area as *mut u8, 0, length);

        // Устанавливаем начальные управляющие значения
        (*save_area).legacy_state.control_word = INITIAL_FPCSR;
        (*save_area).legacy_state.mxcsr = INITIAL_MXCSR;

        // Устанавливаем маску для указания что устаревшее состояние FP присутствует
        (*save_area).header.mask = XSTATE_MASK_LEGACY_FLOATING_POINT;

        // Если поддерживается XSAVES, помечаем как уплотненный формат
        if ke_feature_bits() & KF_XSAVES != 0 {
            (*save_area).header.compaction_mask =
                0x8000000000000000 | XSTATE_MASK_LEGACY_FLOATING_POINT;
        }
    }
}

// =============================================================================
// Низкоуровневые инструкции XSAVE/XRSTOR
// =============================================================================

/// Инструкция XSAVE64
#[inline]
unsafe fn xsave64(buffer: PVOID, mask: u64) {
    unsafe {
        let mask_lo = mask as u32;
        let mask_hi = (mask >> 32) as u32;
        core::arch::asm!(
            "xsave64 [{buf}]",
            buf = in(reg) buffer,
            in("eax") mask_lo,
            in("edx") mask_hi,
            options(nostack)
        );
    }
}

/// Инструкция XSAVEOPT64 (оптимизированная, пропускает неизмененные)
#[inline]
unsafe fn xsaveopt64(buffer: PVOID, mask: u64) {
    unsafe {
        let mask_lo = mask as u32;
        let mask_hi = (mask >> 32) as u32;
        core::arch::asm!(
            "xsaveopt64 [{buf}]",
            buf = in(reg) buffer,
            in("eax") mask_lo,
            in("edx") mask_hi,
            options(nostack)
        );
    }
}

/// Инструкция XSAVES64 (поддержка состояния супервизора)
#[inline]
unsafe fn xsaves64(buffer: PVOID, mask: u64) {
    unsafe {
        let mask_lo = mask as u32;
        let mask_hi = (mask >> 32) as u32;
        core::arch::asm!(
            "xsaves64 [{buf}]",
            buf = in(reg) buffer,
            in("eax") mask_lo,
            in("edx") mask_hi,
            options(nostack)
        );
    }
}

/// Инструкция XRSTOR64
#[inline]
unsafe fn xrstor64(buffer: PVOID, mask: u64) {
    unsafe {
        let mask_lo = mask as u32;
        let mask_hi = (mask >> 32) as u32;
        core::arch::asm!(
            "xrstor64 [{buf}]",
            buf = in(reg) buffer,
            in("eax") mask_lo,
            in("edx") mask_hi,
            options(nostack)
        );
    }
}

/// Инструкция XRSTORS64 (поддержка состояния супервизора)
#[inline]
unsafe fn xrstors64(buffer: PVOID, mask: u64) {
    unsafe {
        let mask_lo = mask as u32;
        let mask_hi = (mask >> 32) as u32;
        core::arch::asm!(
            "xrstors64 [{buf}]",
            buf = in(reg) buffer,
            in("eax") mask_lo,
            in("edx") mask_hi,
            options(nostack)
        );
    }
}

/// Инструкция FXSAVE64 (устаревшая, 512 байт)
#[inline]
unsafe fn fxsave64(buffer: PVOID) {
    unsafe {
        core::arch::asm!(
            "fxsave64 [{buf}]",
            buf = in(reg) buffer,
            options(nostack)
        );
    }
}

/// Инструкция FXRSTOR64 (устаревшая, 512 байт)
#[inline]
unsafe fn fxrstor64(buffer: PVOID) {
    unsafe {
        core::arch::asm!(
            "fxrstor64 [{buf}]",
            buf = in(reg) buffer,
            options(nostack)
        );
    }
}

// =============================================================================
// Определение возможностей через CPUID
// =============================================================================

/// Инициализация конфигурации XSTATE при загрузке
///
/// Определяет возможности XSAVE и включает поддерживаемые компоненты.
///
/// # Безопасность
/// Должна вызываться один раз при инициализации ядра.
pub unsafe fn ki_initialize_x_state_configuration() {
    unsafe {
        // Проверка доступности CPUID (всегда true на x86_64)

        // CPUID.1:ECX бит 26 = поддержка XSAVE
        let (_, _, ecx, _) = cpuid(1);

        if ecx & (1 << 26) == 0 {
            // Нет поддержки XSAVE, использовать только FXSAVE
            let mut features = ke_feature_bits();
            features |= KF_FXSR; // Предполагаем что FXSR доступен на x86_64
            KE_FEATURE_BITS.store(features, Ordering::Relaxed);
            KE_XSTATE_LENGTH.store(XSAVE_FORMAT::SIZE, Ordering::Relaxed);
            return;
        }

        // Включаем XSAVE в CR4
        let cr4 = read_cr4();
        write_cr4(cr4 | (1 << 18)); // CR4.OSXSAVE

        // CPUID.0xD:0 - получаем поддерживаемые биты XCR0 и размеры
        let (eax, ebx, ecx, edx) = cpuid_subleaf(0x0D, 0);
        let supported_mask = ((edx as u64) << 32) | (eax as u64);
        let _max_size = ebx as usize;
        let _current_size = ecx as usize;

        // CPUID.0xD:1 - получаем флаги возможностей XSAVE
        let (eax_sub, _ebx_sub, _ecx_sub, _edx_sub) = cpuid_subleaf(0x0D, 1);

        let mut features = ke_feature_bits();
        features |= KF_FXSR;
        features |= KF_XSTATE;

        // Проверяем XSAVEOPT (бит 0)
        if eax_sub & 1 != 0 {
            features |= KF_XSAVEOPT;
        }

        // Проверяем XSAVES/XRSTORS (бит 3)
        if eax_sub & (1 << 3) != 0 {
            features |= KF_XSAVES;
        }

        KE_FEATURE_BITS.store(features, Ordering::Relaxed);

        // Включаем устаревшее FPU + SSE в XCR0
        // Можно включить больше (AVX и т.д.), но начинаем с базовых
        let xcr0_mask = supported_mask & XSTATE_MASK_LEGACY;
        xsetbv(0, xcr0_mask);

        // Вычисляем требуемый размер для включенных компонентов
        let (_, ebx_final, _, _) = cpuid_subleaf(0x0D, 0);
        let xstate_length = ebx_final as usize;

        // Используем минимум размер XSAVE_AREA
        let final_length = xstate_length.max(XSAVE_AREA::SIZE);
        KE_XSTATE_LENGTH.store(final_length, Ordering::Relaxed);
    }
}

// =============================================================================
// Вспомогательные функции
// =============================================================================

#[inline]
unsafe fn cpuid(leaf: u32) -> (u32, u32, u32, u32) {
    unsafe {
        let eax: u32;
        let ebx: u32;
        let ecx: u32;
        let edx: u32;
        // rbx зарезервирован LLVM, поэтому сохраняем/восстанавливаем вручную
        core::arch::asm!(
            "push rbx",
            "cpuid",
            "mov {ebx:e}, ebx",
            "pop rbx",
            inout("eax") leaf => eax,
            ebx = out(reg) ebx,
            out("ecx") ecx,
            out("edx") edx,
            options(preserves_flags)
        );
        (eax, ebx, ecx, edx)
    }
}

#[inline]
unsafe fn cpuid_subleaf(leaf: u32, subleaf: u32) -> (u32, u32, u32, u32) {
    unsafe {
        let eax: u32;
        let ebx: u32;
        let ecx: u32;
        let edx: u32;
        // rbx зарезервирован LLVM, поэтому сохраняем/восстанавливаем вручную
        core::arch::asm!(
            "push rbx",
            "cpuid",
            "mov {ebx:e}, ebx",
            "pop rbx",
            inout("eax") leaf => eax,
            ebx = out(reg) ebx,
            inout("ecx") subleaf => ecx,
            out("edx") edx,
            options(preserves_flags)
        );
        (eax, ebx, ecx, edx)
    }
}

#[inline]
unsafe fn read_cr4() -> u64 {
    unsafe {
        let cr4: u64;
        core::arch::asm!("mov {}, cr4", out(reg) cr4, options(nostack, nomem));
        cr4
    }
}

#[inline]
unsafe fn write_cr4(val: u64) {
    unsafe {
        core::arch::asm!("mov cr4, {}", in(reg) val, options(nostack, nomem));
    }
}

#[inline]
unsafe fn xsetbv(xcr: u32, value: u64) {
    unsafe {
        let lo = value as u32;
        let hi = (value >> 32) as u32;
        core::arch::asm!(
            "xsetbv",
            in("ecx") xcr,
            in("eax") lo,
            in("edx") hi,
            options(nostack, nomem)
        );
    }
}
