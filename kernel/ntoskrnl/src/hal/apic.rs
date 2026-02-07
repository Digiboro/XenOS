//! Local APIC - Advanced Programmable Interrupt Controller
//!
//! Реализация Local APIC для системного таймера и управления прерываниями.
//!
//! Источники:
//! - ReactOS: hal/halx86/apic/apic.c, apicp.h
//! - Intel SDM Vol. 3A, Chapter 10
//! - OSDev Wiki: https://wiki.osdev.org/APIC

#![allow(dead_code)]

use core::sync::atomic::AtomicU32;
use core::sync::atomic::AtomicU64;
use core::sync::atomic::Ordering;

// =============================================================================
// APIC Addresses
// =============================================================================

/// MSR для APIC Base Address
pub const MSR_APIC_BASE: u32 = 0x1B;

/// Базовый виртуальный адрес Local APIC (64-bit)
/// NT/ReactOS используют 0xFFFFFFFFFFFE0000
pub const LOCAL_APIC_BASE: u64 = 0xFFFFFFFFFFFE0000;

/// Физический адрес Local APIC по умолчанию
pub const LOCAL_APIC_PHYS_DEFAULT: u64 = 0xFEE00000;

// =============================================================================
// APIC Vectors (из ReactOS apicp.h и NT5 для AMD64)
// =============================================================================
//
// Vector Layout на NT AMD64:
//   0x00-0x1E: Reserved / Exceptions
//   0x1F:      APC software interrupt
//   0x20-0x2E: Reserved
//   0x2F:      DPC/Dispatch software interrupt
//   0x30-0xCF: Device IRQs (via IOAPIC)
//   0xD0-0xDF: High priority system vectors
//   0xE0-0xFE: APIC system vectors
//   0xFF:      APIC NMI (reserved)
//

/// APC software interrupt vector
pub const APC_VECTOR: u8 = 0x1F;

/// DPC/Dispatch software interrupt vector
pub const DISPATCH_VECTOR: u8 = 0x2F;

/// Corrected Machine Check Interrupt vector
pub const CMCI_VECTOR: u8 = 0x35;

/// APIC Timer vector (CLOCK_LEVEL)
pub const APIC_TIMER_VECTOR: u8 = 0xD1;

/// Clock IPI vector (для SMP)
pub const CLOCK_IPI_VECTOR: u8 = 0xD2;

/// Reboot vector (для SMP shutdown)
pub const REBOOT_VECTOR: u8 = 0xD7;

/// Stub vector (placeholder)
pub const STUB_VECTOR: u8 = 0xD8;

/// Spurious interrupt vector
pub const APIC_SPURIOUS_VECTOR: u8 = 0xDF;

/// IPI vector
pub const APIC_IPI_VECTOR: u8 = 0xE1;

/// Error vector
pub const APIC_ERROR_VECTOR: u8 = 0xE2;

/// Power fail vector (опционально)
pub const POWERFAIL_VECTOR: u8 = 0xE3;

/// Profiling vector (для профайлинга)
pub const APIC_PROFILE_VECTOR: u8 = 0xFD;

/// Performance counter vector
pub const APIC_PERF_VECTOR: u8 = 0xFE;

/// NMI vector
pub const APIC_NMI_VECTOR: u8 = 0xFF;

// =============================================================================
// Device IRQ Range
// =============================================================================

/// Первый вектор для device IRQs
pub const DEVICE_VECTOR_MIN: u8 = 0x30;

/// Последний вектор для device IRQs
pub const DEVICE_VECTOR_MAX: u8 = 0xCF;

// =============================================================================
// APIC Registers
// =============================================================================

/// Регистры Local APIC (offsets от базового адреса)
#[repr(u32)]
#[derive(Clone, Copy, Debug)]
pub enum ApicRegister {
    /// Local APIC ID
    Id = 0x020,
    /// Local APIC Version
    Version = 0x030,
    /// Task Priority Register
    Tpr = 0x080,
    /// Arbitration Priority Register
    Apr = 0x090,
    /// Processor Priority Register
    Ppr = 0x0A0,
    /// End of Interrupt
    Eoi = 0x0B0,
    /// Remote Read Register
    Rrd = 0x0C0,
    /// Logical Destination Register
    Ldr = 0x0D0,
    /// Destination Format Register
    Dfr = 0x0E0,
    /// Spurious Interrupt Vector Register
    Sivr = 0x0F0,
    /// In-Service Register (8 x 32-bit, начиная с 0x100)
    Isr = 0x100,
    /// Trigger Mode Register (8 x 32-bit, начиная с 0x180)
    Tmr = 0x180,
    /// Interrupt Request Register (8 x 32-bit, начиная с 0x200)
    Irr = 0x200,
    /// Error Status Register
    Esr = 0x280,
    /// Interrupt Command Register (low 32 bits)
    Icr0 = 0x300,
    /// Interrupt Command Register (high 32 bits)
    Icr1 = 0x310,
    /// Timer LVT (Local Vector Table)
    TimerLvt = 0x320,
    /// Thermal Sensor LVT
    ThermalLvt = 0x330,
    /// Performance Counter LVT
    PerfLvt = 0x340,
    /// Local Interrupt 0 LVT
    Lint0 = 0x350,
    /// Local Interrupt 1 LVT
    Lint1 = 0x360,
    /// Error LVT
    ErrorLvt = 0x370,
    /// Timer Initial Count Register
    TimerIcr = 0x380,
    /// Timer Current Count Register (read-only)
    TimerCcr = 0x390,
    /// Timer Divide Configuration Register
    TimerDcr = 0x3E0,
}

/// Timer divide values для Timer DCR
#[repr(u32)]
#[derive(Clone, Copy, Debug)]
pub enum ApicTimerDivide {
    DivideBy2 = 0b0000,
    DivideBy4 = 0b0001,
    DivideBy8 = 0b0010,
    DivideBy16 = 0b0011,
    DivideBy32 = 0b1000,
    DivideBy64 = 0b1001,
    DivideBy128 = 0b1010,
    DivideBy1 = 0b1011,
}

/// Delivery Mode для LVT и ICR
#[repr(u32)]
#[derive(Clone, Copy, Debug)]
pub enum ApicDeliveryMode {
    Fixed = 0b000,
    LowestPriority = 0b001,
    Smi = 0b010,
    Nmi = 0b100,
    Init = 0b101,
    Startup = 0b110,
    ExtInt = 0b111,
}

/// Trigger Mode
#[repr(u32)]
#[derive(Clone, Copy, Debug)]
pub enum ApicTriggerMode {
    Edge = 0,
    Level = 1,
}

/// Destination Format
#[repr(u32)]
#[derive(Clone, Copy, Debug)]
pub enum ApicDestinationFormat {
    /// Cluster model
    Cluster = 0x0FFFFFFF,
    /// Flat model (до 8 CPU)
    Flat = 0xFFFFFFFF,
}

// =============================================================================
// LVT Register Bits
// =============================================================================

/// LVT Register bit masks
pub mod lvt {
    /// Vector mask (bits 0-7)
    pub const VECTOR_MASK: u32 = 0xFF;
    /// Delivery mode (bits 8-10)
    pub const DELIVERY_MODE_SHIFT: u32 = 8;
    pub const DELIVERY_MODE_MASK: u32 = 0x7 << 8;
    /// Delivery status (bit 12, read-only)
    pub const DELIVERY_STATUS: u32 = 1 << 12;
    /// Interrupt input pin polarity (bit 13)
    pub const POLARITY: u32 = 1 << 13;
    /// Remote IRR (bit 14, read-only)
    pub const REMOTE_IRR: u32 = 1 << 14;
    /// Trigger mode (bit 15)
    pub const TRIGGER_MODE: u32 = 1 << 15;
    /// Mask (bit 16): 1 = masked (disabled), 0 = unmasked (enabled)
    pub const MASK: u32 = 1 << 16;
    /// Timer mode (bits 17-18)
    pub const TIMER_MODE_SHIFT: u32 = 17;
    pub const TIMER_MODE_MASK: u32 = 0x3 << 17;
    /// Timer periodic mode
    pub const TIMER_PERIODIC: u32 = 1 << 17;
    /// Timer TSC-deadline mode
    pub const TIMER_TSC_DEADLINE: u32 = 2 << 17;
}

// =============================================================================
// APIC State
// =============================================================================

/// Частота APIC timer в Hz (после калибровки)
static APIC_TIMER_FREQUENCY: AtomicU64 = AtomicU64::new(0);

/// Текущий интервал таймера в 100ns единицах
static APIC_TIME_INCREMENT: AtomicU32 = AtomicU32::new(0);

/// Счетчик тиков с момента запуска
static APIC_TICK_COUNT: AtomicU64 = AtomicU64::new(0);

/// Performance counter (накопленные тики)
static APIC_PERF_COUNTER: AtomicU64 = AtomicU64::new(0);

/// APIC инициализирован
static APIC_INITIALIZED: AtomicU32 = AtomicU32::new(0);

// =============================================================================
// Timer Configuration
// =============================================================================

/// Стандартный интервал ~10ms (100 Hz)
pub const APIC_DEFAULT_INTERVAL_100NS: u32 = 100_000;

/// Минимальный интервал ~1ms (1000 Hz)
pub const APIC_MIN_INTERVAL_100NS: u32 = 10_000;

/// Максимальный интервал ~15ms (~67 Hz)
pub const APIC_MAX_INTERVAL_100NS: u32 = 150_000;

// =============================================================================
// APIC Register Access
// =============================================================================

/// Читает регистр Local APIC
///
/// # Arguments
/// * `reg` - регистр для чтения
///
/// # Returns
/// Значение регистра
#[inline]
pub fn apic_read(reg: ApicRegister) -> u32 {
    let base = apic_get_virtual_base();
    unsafe { core::ptr::read_volatile((base + reg as u64) as *const u32) }
}

/// Записывает в регистр Local APIC
///
/// # Arguments
/// * `reg` - регистр для записи
/// * `value` - значение для записи
#[inline]
pub fn apic_write(reg: ApicRegister, value: u32) {
    let base = apic_get_virtual_base();
    unsafe {
        core::ptr::write_volatile((base + reg as u64) as *mut u32, value);
    }
}

/// Отправляет EOI (End of Interrupt) в Local APIC
///
/// Должен вызываться в конце каждого interrupt handler для APIC прерываний.
#[inline]
pub fn apic_send_eoi() {
    apic_write(ApicRegister::Eoi, 0);
}

// =============================================================================
// APIC Detection
// =============================================================================

/// Проверяет поддержку APIC через CPUID
///
/// Использует CPUID leaf 1, EDX bit 9.
pub fn is_apic_supported() -> bool {
    use crate::arch::x86_64::cpu::cpuid;
    use crate::arch::x86_64::cpu::cpuid_features_edx;

    // CPUID leaf 1
    let result = cpuid(1, 0);
    (result.edx & cpuid_features_edx::APIC) != 0
}

/// Проверяет включен ли APIC через MSR
pub fn is_apic_enabled() -> bool {
    let msr = unsafe { crate::arch::x86_64::msr::rdmsr(MSR_APIC_BASE) };
    // Bit 11 = APIC Global Enable
    (msr & (1 << 11)) != 0
}

/// Проверяет является ли текущий процессор BSP (Bootstrap Processor)
pub fn is_bsp() -> bool {
    let msr = unsafe { crate::arch::x86_64::msr::rdmsr(MSR_APIC_BASE) };
    // Bit 8 = BSP flag
    (msr & (1 << 8)) != 0
}

/// Возвращает физический базовый адрес APIC из MSR
pub fn get_apic_base_address() -> u64 {
    let msr = unsafe { crate::arch::x86_64::msr::rdmsr(MSR_APIC_BASE) };
    // Bits 12-35 содержат базовый адрес (выровнен на 4KB)
    msr & 0xFFFFFF000
}

/// Проверяет инициализирован ли APIC
#[inline]
pub fn is_apic_initialized() -> bool {
    APIC_INITIALIZED.load(Ordering::Acquire) != 0
}

// =============================================================================
// APIC Information
// =============================================================================

/// Возвращает APIC ID текущего процессора
pub fn apic_get_id() -> u8 {
    let id_reg = apic_read(ApicRegister::Id);
    // APIC ID в битах 24-31
    (id_reg >> 24) as u8
}

/// Возвращает версию APIC
pub fn apic_get_version() -> u8 {
    let ver_reg = apic_read(ApicRegister::Version);
    (ver_reg & 0xFF) as u8
}

/// Возвращает максимальное количество LVT entries
pub fn apic_get_max_lvt() -> u8 {
    let ver_reg = apic_read(ApicRegister::Version);
    // Max LVT entry в битах 16-23
    ((ver_reg >> 16) & 0xFF) as u8 + 1
}

// =============================================================================
// Timer Query Functions
// =============================================================================

/// Возвращает частоту APIC timer в Hz
pub fn apic_get_timer_frequency() -> u64 {
    APIC_TIMER_FREQUENCY.load(Ordering::Acquire)
}

/// Возвращает количество тиков с момента запуска
#[inline]
pub fn apic_query_tick_count() -> u64 {
    APIC_TICK_COUNT.load(Ordering::Acquire)
}

/// Возвращает интервал между тиками в 100ns единицах
#[inline]
pub fn apic_query_time_increment() -> u32 {
    APIC_TIME_INCREMENT.load(Ordering::Acquire)
}

/// Обработчик clock interrupt (вызывается из ISR)
///
/// Инкрементирует счетчики тиков.
pub fn apic_clock_interrupt() {
    APIC_TICK_COUNT.fetch_add(1, Ordering::SeqCst);

    // Обновляем performance counter
    let increment = APIC_TIME_INCREMENT.load(Ordering::Acquire);
    APIC_PERF_COUNTER.fetch_add(increment as u64, Ordering::SeqCst);
}

// =============================================================================
// APIC Mapping
// =============================================================================

/// Маппит Local APIC на фиксированный NT-совместимый виртуальный адрес
///
/// Использует HAL MMIO region (mm/hal_mmio.rs) для маппинга на адрес
/// LOCAL_APIC_BASE (0xFFFFFFFFFFFE0000).
///
/// # Safety
/// Должен вызываться после mm_init_hal_mmio()
pub unsafe fn apic_map_local_apic() {
    unsafe {
        use crate::mm::hal_mmio;

        // Получаем физический адрес APIC из MSR
        let apic_phys = get_apic_base_address();

        // Маппим через HAL MMIO на фиксированный адрес
        let virt = hal_mmio::mm_map_local_apic(apic_phys);

        if virt == 0 {
            // HAL MMIO не инициализирован - используем HHDM как fallback
            // Это временное решение для ранней загрузки
            panic!("HAL MMIO not initialized before APIC mapping");
        }

        // Виртуальный адрес всегда LOCAL_APIC_BASE после успешного маппинга
        debug_assert_eq!(virt, LOCAL_APIC_BASE);
    }
}

/// Возвращает текущий виртуальный адрес APIC
///
/// После успешного маппинга всегда возвращает LOCAL_APIC_BASE (0xFFFFFFFFFFFE0000)
#[inline]
pub fn apic_get_virtual_base() -> u64 {
    // NT-совместимый фиксированный адрес
    LOCAL_APIC_BASE
}

// =============================================================================
// APIC Initialization
// =============================================================================

/// Инициализирует Local APIC для указанного CPU
///
/// # Arguments
/// * `cpu` - номер процессора (0 для BSP)
///
/// # Safety
/// Должен вызываться после apic_map_local_apic()
pub unsafe fn apic_initialize_local_apic(cpu: u32) {
    unsafe {
        use crate::arch::x86_64::msr::rdmsr;
        use crate::arch::x86_64::msr::wrmsr;

        // 1. Включаем APIC через MSR если не включен
        let mut base_msr = rdmsr(MSR_APIC_BASE);
        if (base_msr & (1 << 11)) == 0 {
            base_msr |= 1 << 11; // Global Enable
            wrmsr(MSR_APIC_BASE, base_msr);
        }

        // 2. Настраиваем Spurious Interrupt Vector Register
        // Bit 8 = Software Enable, Bits 0-7 = Spurious Vector
        let sivr = (1 << 8) | (APIC_SPURIOUS_VECTOR as u32);
        apic_write(ApicRegister::Sivr, sivr);

        // 3. Устанавливаем Destination Format Register (Flat model)
        apic_write(ApicRegister::Dfr, ApicDestinationFormat::Flat as u32);

        // 4. Устанавливаем Logical Destination Register
        // Каждый CPU получает уникальный бит (для flat model, max 8 CPUs)
        let logical_id = 1u32 << cpu;
        apic_write(ApicRegister::Ldr, logical_id << 24);

        // 5. Устанавливаем Task Priority Register в 0 (принимать все прерывания)
        apic_write(ApicRegister::Tpr, 0);

        // 6. Маскируем все LVT entries изначально
        let masked_lvt = lvt::MASK; // Только mask bit, vector = 0
        apic_write(ApicRegister::TimerLvt, masked_lvt);
        apic_write(ApicRegister::ThermalLvt, masked_lvt);
        apic_write(ApicRegister::PerfLvt, masked_lvt);
        apic_write(ApicRegister::ErrorLvt, masked_lvt);

        // 7. Настраиваем LINT0 (ExtINT от PIC) - masked
        // Delivery Mode = ExtINT (111), Masked
        let lint0 = (ApicDeliveryMode::ExtInt as u32) << lvt::DELIVERY_MODE_SHIFT | lvt::MASK;
        apic_write(ApicRegister::Lint0, lint0);

        // 8. Настраиваем LINT1 (NMI) - unmasked
        // Delivery Mode = NMI (100), Unmasked
        let lint1 = (ApicDeliveryMode::Nmi as u32) << lvt::DELIVERY_MODE_SHIFT;
        apic_write(ApicRegister::Lint1, lint1);

        // 9. Настраиваем Error LVT
        let error_lvt = (APIC_ERROR_VECTOR as u32) | lvt::MASK;
        apic_write(ApicRegister::ErrorLvt, error_lvt);

        // 10. Очищаем Error Status Register (записываем дважды по спецификации)
        apic_write(ApicRegister::Esr, 0);
        apic_write(ApicRegister::Esr, 0);
        let _ = apic_read(ApicRegister::Esr);

        // Отмечаем APIC как инициализированный
        APIC_INITIALIZED.store(1, Ordering::Release);
    }
}

// =============================================================================
// APIC Timer Calibration
// =============================================================================

/// Калибрует APIC timer используя PIT как эталон
///
/// Измеряет частоту APIC timer запуская его и ожидая
/// известное время через PIT.
///
/// # Safety
/// Должен вызываться после apic_initialize_local_apic() и при
/// доступном PIT.
pub unsafe fn apic_calibrate_timer() {
    const CALIBRATION_MS: u32 = 50; // 50ms для хорошей точности

    // 1. Устанавливаем делитель на 1 для максимальной точности
    apic_write(ApicRegister::TimerDcr, ApicTimerDivide::DivideBy1 as u32);

    // 2. Устанавливаем максимальный initial count (таймер считает вниз)
    apic_write(ApicRegister::TimerIcr, 0xFFFFFFFF);

    // 3. Ждем используя PIT (busy-wait)
    super::pit::ke_stall_execution_processor(CALIBRATION_MS * 1000);

    // 4. Читаем текущее значение счетчика
    let remaining = apic_read(ApicRegister::TimerCcr);
    let elapsed = 0xFFFFFFFFu32 - remaining;

    // 5. Вычисляем частоту
    // elapsed ticks за CALIBRATION_MS миллисекунд
    // frequency = elapsed * 1000 / CALIBRATION_MS
    let frequency = (elapsed as u64 * 1000) / CALIBRATION_MS as u64;
    APIC_TIMER_FREQUENCY.store(frequency, Ordering::Release);

    // 6. Останавливаем таймер (initial count = 0)
    apic_write(ApicRegister::TimerIcr, 0);
}

// =============================================================================
// APIC Timer Control
// =============================================================================

/// Инициализирует APIC timer в периодическом режиме
///
/// # Arguments
/// * `interval_100ns` - интервал в 100ns единицах (100_000 = 10ms)
///
/// # Safety
/// Должен вызываться после apic_calibrate_timer()
pub unsafe fn apic_initialize_clock(interval_100ns: u32) {
    let frequency = APIC_TIMER_FREQUENCY.load(Ordering::Acquire);
    if frequency == 0 {
        panic!("APIC timer not calibrated!");
    }

    // Ограничиваем интервал
    let interval = interval_100ns.clamp(APIC_MIN_INTERVAL_100NS, APIC_MAX_INTERVAL_100NS);

    // Вычисляем initial count для нужного интервала
    // interval_100ns * 100 = interval_ns
    // ticks = frequency * interval_ns / 1_000_000_000
    // Упрощаем: ticks = frequency * interval_100ns / 10_000_000
    let ticks = (frequency * interval as u64) / 10_000_000;

    // Устанавливаем делитель
    apic_write(ApicRegister::TimerDcr, ApicTimerDivide::DivideBy1 as u32);

    // Настраиваем LVT Timer Register:
    // - Vector: APIC_TIMER_VECTOR
    // - Mode: Periodic (bit 17 = 1)
    // - Mask: 0 (enabled)
    let lvt_timer = (APIC_TIMER_VECTOR as u32) | lvt::TIMER_PERIODIC;
    apic_write(ApicRegister::TimerLvt, lvt_timer);

    // Устанавливаем initial count - это запускает таймер
    apic_write(ApicRegister::TimerIcr, ticks as u32);

    // Сохраняем реальный интервал
    let actual_interval = (ticks * 10_000_000) / frequency;
    APIC_TIME_INCREMENT.store(actual_interval as u32, Ordering::Release);
}

/// Изменяет интервал таймера
///
/// # Arguments
/// * `interval_100ns` - новый интервал в 100ns единицах
///
/// # Returns
/// Реальный установленный интервал
pub fn apic_set_time_increment(interval_100ns: u32) -> u32 {
    let frequency = APIC_TIMER_FREQUENCY.load(Ordering::Acquire);
    if frequency == 0 {
        return 0;
    }

    let interval = interval_100ns.clamp(APIC_MIN_INTERVAL_100NS, APIC_MAX_INTERVAL_100NS);
    let ticks = (frequency * interval as u64) / 10_000_000;

    apic_write(ApicRegister::TimerIcr, ticks as u32);

    let actual = (ticks * 10_000_000) / frequency;
    APIC_TIME_INCREMENT.store(actual as u32, Ordering::Release);
    actual as u32
}

/// Останавливает APIC timer
pub fn apic_stop_timer() {
    // Маскируем таймер
    let lvt = apic_read(ApicRegister::TimerLvt);
    apic_write(ApicRegister::TimerLvt, lvt | lvt::MASK);
    // Устанавливаем initial count в 0
    apic_write(ApicRegister::TimerIcr, 0);
}

// =============================================================================
// Legacy PIC Disable
// =============================================================================

/// Отключает Legacy PIC для работы с APIC
///
/// Маскирует все IRQ на 8259 PIC и переключает IMCR если присутствует.
pub fn apic_disable_legacy_pic() {
    // Маскируем все IRQ на обоих PIC
    super::pic::halp_disable_legacy_pics();

    // Программируем IMCR если присутствует
    // IMCR (Interrupt Mode Configuration Register)
    // Port 0x22 = address, Port 0x23 = data
    // Write 0x70 to 0x22, then write 0x01 to 0x23 для APIC mode
    super::portio::write_port_uchar(0x22, 0x70);
    super::portio::write_port_uchar(0x23, 0x01);
}

// =============================================================================
// IPI (Inter-Processor Interrupt) Support
// =============================================================================

/// ICR Delivery Mode
#[repr(u32)]
#[derive(Clone, Copy, Debug)]
pub enum IcrDeliveryMode {
    Fixed = 0b000,
    LowestPriority = 0b001,
    Smi = 0b010,
    Nmi = 0b100,
    Init = 0b101,
    StartUp = 0b110,
}

/// ICR Destination Shorthand
#[repr(u32)]
#[derive(Clone, Copy, Debug)]
pub enum IcrDestinationShorthand {
    /// No shorthand - use destination field
    NoShorthand = 0b00,
    /// Send to self only
    SelfOnly = 0b01,
    /// Send to all including self
    AllIncludingSelf = 0b10,
    /// Send to all excluding self
    AllExcludingSelf = 0b11,
}

/// ICR Level (для INIT)
#[repr(u32)]
#[derive(Clone, Copy, Debug)]
pub enum IcrLevel {
    Deassert = 0,
    Assert = 1,
}

/// ICR Trigger Mode
#[repr(u32)]
#[derive(Clone, Copy, Debug)]
pub enum IcrTriggerMode {
    Edge = 0,
    Level = 1,
}

/// Построение ICR low значения
///
/// ICR Low (bits):
/// - 0-7:   Vector
/// - 8-10:  Delivery Mode
/// - 11:    Destination Mode (0=Physical, 1=Logical)
/// - 12:    Delivery Status (RO)
/// - 13:    Reserved
/// - 14:    Level (0=De-assert, 1=Assert)
/// - 15:    Trigger Mode (0=Edge, 1=Level)
/// - 16-17: Reserved
/// - 18-19: Destination Shorthand
#[inline]
pub fn build_icr_low(
    vector: u8,
    delivery_mode: IcrDeliveryMode,
    destination_mode_logical: bool,
    level: IcrLevel,
    trigger_mode: IcrTriggerMode,
    shorthand: IcrDestinationShorthand,
) -> u32 {
    let mut icr: u32 = vector as u32;
    icr |= (delivery_mode as u32) << 8;
    if destination_mode_logical {
        icr |= 1 << 11;
    }
    icr |= (level as u32) << 14;
    icr |= (trigger_mode as u32) << 15;
    icr |= (shorthand as u32) << 18;
    icr
}

/// Построение ICR high значения (destination APIC ID)
///
/// ICR High (bits 24-31): Destination APIC ID (для Physical mode)
#[inline]
pub fn build_icr_high(destination_apic_id: u8) -> u32 {
    (destination_apic_id as u32) << 24
}

/// Ожидание завершения доставки IPI
///
/// Проверяет Delivery Status bit (bit 12) в ICR.
/// Возвращает true если доставка завершена, false если таймаут.
#[inline]
pub fn apic_wait_for_ipi_delivery() -> bool {
    const MAX_WAIT_ITERATIONS: u32 = 100_000;

    for _ in 0..MAX_WAIT_ITERATIONS {
        let icr_low = apic_read(ApicRegister::Icr0);
        // Bit 12 = Delivery Status: 0=Idle, 1=Send Pending
        if (icr_low & (1 << 12)) == 0 {
            return true;
        }
        crate::arch::x86_64::cpu::yield_processor();
    }

    false
}

/// HalRequestIpi - отправка IPI на указанный процессор
///
/// Соответствует ReactOS HalRequestIpi / NT HalpSendIPI.
///
/// # Arguments
/// * `target_apic_id` - APIC ID целевого процессора
/// * `vector` - номер вектора прерывания
///
/// # Safety
/// Должен вызываться на DISPATCH_LEVEL или выше
pub fn hal_send_ipi(target_apic_id: u8, vector: u8) {
    // Ожидаем завершения предыдущей отправки
    if !apic_wait_for_ipi_delivery() {
        // Предыдущий IPI не доставлен - это ошибка, но продолжаем
        // В production можно добавить panic или logging
    }

    // Строим ICR значения
    let icr_high = build_icr_high(target_apic_id);
    let icr_low = build_icr_low(
        vector,
        IcrDeliveryMode::Fixed,
        false, // Physical destination mode
        IcrLevel::Assert,
        IcrTriggerMode::Edge,
        IcrDestinationShorthand::NoShorthand,
    );

    // ВАЖНО: Сначала записываем ICR High, потом ICR Low
    // Запись в ICR Low инициирует отправку IPI
    apic_write(ApicRegister::Icr1, icr_high);
    apic_write(ApicRegister::Icr0, icr_low);
}

/// HalSendSelfIpi - отправка IPI самому себе
///
/// Используется для software interrupt через APIC.
///
/// # Arguments
/// * `vector` - номер вектора прерывания
pub fn hal_send_self_ipi(vector: u8) {
    // Self-IPI можно отправить через shorthand
    let icr_low = build_icr_low(
        vector,
        IcrDeliveryMode::Fixed,
        false,
        IcrLevel::Assert,
        IcrTriggerMode::Edge,
        IcrDestinationShorthand::SelfOnly,
    );

    // Для self-IPI не нужно ждать и не нужен ICR High
    apic_write(ApicRegister::Icr0, icr_low);
}

/// HalSendBroadcastIpi - отправка IPI всем процессорам кроме себя
///
/// # Arguments
/// * `vector` - номер вектора прерывания
pub fn hal_send_broadcast_ipi(vector: u8) {
    // Ожидаем завершения предыдущей отправки
    apic_wait_for_ipi_delivery();

    let icr_low = build_icr_low(
        vector,
        IcrDeliveryMode::Fixed,
        false,
        IcrLevel::Assert,
        IcrTriggerMode::Edge,
        IcrDestinationShorthand::AllExcludingSelf,
    );

    apic_write(ApicRegister::Icr0, icr_low);
}

/// HalSendNmiToProcessor - отправка NMI на указанный процессор
///
/// # Arguments
/// * `target_apic_id` - APIC ID целевого процессора
pub fn hal_send_nmi(target_apic_id: u8) {
    apic_wait_for_ipi_delivery();

    let icr_high = build_icr_high(target_apic_id);
    let icr_low = build_icr_low(
        0, // Vector игнорируется для NMI
        IcrDeliveryMode::Nmi,
        false,
        IcrLevel::Assert,
        IcrTriggerMode::Edge,
        IcrDestinationShorthand::NoShorthand,
    );

    apic_write(ApicRegister::Icr1, icr_high);
    apic_write(ApicRegister::Icr0, icr_low);
}

/// HalSendInitIpi - отправка INIT IPI (для SMP startup)
///
/// # Arguments
/// * `target_apic_id` - APIC ID целевого процессора
pub fn hal_send_init_ipi(target_apic_id: u8) {
    apic_wait_for_ipi_delivery();

    let icr_high = build_icr_high(target_apic_id);
    let icr_low = build_icr_low(
        0,
        IcrDeliveryMode::Init,
        false,
        IcrLevel::Assert,
        IcrTriggerMode::Level,
        IcrDestinationShorthand::NoShorthand,
    );

    apic_write(ApicRegister::Icr1, icr_high);
    apic_write(ApicRegister::Icr0, icr_low);

    apic_wait_for_ipi_delivery();

    // Deassert INIT
    let icr_low_deassert = build_icr_low(
        0,
        IcrDeliveryMode::Init,
        false,
        IcrLevel::Deassert,
        IcrTriggerMode::Level,
        IcrDestinationShorthand::NoShorthand,
    );

    apic_write(ApicRegister::Icr0, icr_low_deassert);
}

/// HalSendStartupIpi - отправка SIPI (Startup IPI) для SMP
///
/// # Arguments
/// * `target_apic_id` - APIC ID целевого процессора
/// * `vector` - страница (4KB aligned) с кодом startup (vector * 0x1000)
pub fn hal_send_startup_ipi(target_apic_id: u8, vector: u8) {
    apic_wait_for_ipi_delivery();

    let icr_high = build_icr_high(target_apic_id);
    let icr_low = build_icr_low(
        vector,
        IcrDeliveryMode::StartUp,
        false,
        IcrLevel::Assert,
        IcrTriggerMode::Edge,
        IcrDestinationShorthand::NoShorthand,
    );

    apic_write(ApicRegister::Icr1, icr_high);
    apic_write(ApicRegister::Icr0, icr_low);
}
