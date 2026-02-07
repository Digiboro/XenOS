//! Управление системным временем
//!
//! Источники:
//! - NT5: ke/time.c
//! - ReactOS: ke/time.c, ntoskrnl/include/internal/ke.h

#![allow(dead_code)]

use core::sync::atomic::AtomicU32;
use core::sync::atomic::AtomicU64;
use core::sync::atomic::Ordering;

use crate::arch::x86_64::pcr;

// =============================================================================
// KUSER_SHARED_DATA - структура общая для kernel и user mode
// =============================================================================

/// KUSER_SHARED_DATA - данные разделяемые между ядром и user mode
///
/// В Windows эта структура размещается по фиксированному адресу:
/// - User mode: 0x7FFE0000
/// - Kernel mode: 0xFFFFF78000000000 (x64)
///
/// Источник: ReactOS sdk/include/xdk/ketypes.h
#[repr(C)]
pub struct KUSER_SHARED_DATA {
    /// Tick count в 100ns единицах
    pub tick_count_low_deprecated: u32,
    /// Tick count multiplier
    pub tick_count_multiplier: u32,

    /// Volatile interrupt time (в 100ns)
    pub interrupt_time: KSYSTEM_TIME,
    /// Volatile system time (в 100ns от 1601)
    pub system_time: KSYSTEM_TIME,
    /// Volatile time zone bias (в 100ns)
    pub time_zone_bias: KSYSTEM_TIME,

    /// Image number range (для binaries)
    pub image_number_low: u16,
    pub image_number_high: u16,
    /// NT system root path
    pub nt_system_root: [u16; 260],

    /// Maximum stack trace depth
    pub max_stack_trace_depth: u32,

    /// Crypto exponent
    pub crypto_exponent: u32,

    /// Time zone ID
    pub time_zone_id: u32,

    /// Large page minimum
    pub large_page_minimum: u32,

    /// Reserved
    pub reserved2: [u32; 7],

    /// NT product type
    pub nt_product_type: u32,
    /// Product type is valid
    pub product_type_is_valid: u8,

    /// Reserved
    pub reserved0: [u8; 1],
    /// Native processor architecture
    pub native_processor_architecture: u16,

    /// NT major version
    pub nt_major_version: u32,
    /// NT minor version
    pub nt_minor_version: u32,

    /// Processor features
    pub processor_features: [u8; 64],

    /// Reserved
    pub reserved1: u32,
    pub reserved3: u32,

    /// Time slip
    pub time_slip: u32,

    /// Alternative architecture
    pub alternative_architecture: u32,

    /// Boot ID
    pub boot_id: u32,

    /// System expiration date
    pub system_expiration_date: i64,

    /// Suite mask
    pub suite_mask: u32,

    /// KdDebuggerEnabled
    pub kd_debugger_enabled: u8,

    /// Mitigation policies
    pub mitigation_policies: u8,

    /// Reserved
    pub reserved6: [u8; 2],

    /// Active console ID
    pub active_console_id: u32,

    /// Dismount count
    pub dismount_count: u32,

    /// ComPlus package
    pub com_plus_package: u32,

    /// Last system RIT event tick count
    pub last_system_rit_event_tick_count: u32,

    /// Number of physical pages
    pub number_of_physical_pages: u32,

    /// Safe boot mode
    pub safe_boot_mode: u8,

    /// Virtualization flags
    pub virtualization_flags: u8,

    /// Reserved
    pub reserved12: [u8; 2],

    /// Shared data flags
    pub shared_data_flags: u32,

    /// Reserved padding
    pub data_flags_pad: [u32; 1],

    /// Test return instruction (для user mode fast syscall)
    pub test_ret_instruction: u64,

    /// QPC data
    pub qpc_frequency: i64,

    /// System call
    pub system_call: u32,
    pub system_call_pad0: u32,

    /// System call return
    pub system_call_return: u64,

    /// System call pad
    pub system_call_pad: [u64; 3],

    /// Tick count (volatile KSYSTEM_TIME)
    pub tick_count: KSYSTEM_TIME,
    pub tick_count_pad: [u32; 1],

    /// Cookie (для security)
    pub cookie: u32,
    pub cookie_pad: [u32; 1],

    /// Console session foreground process ID
    pub console_session_foreground_process_id: i64,

    /// Time update lock
    pub time_update_lock: u64,

    /// Базовое системное время (для вычисления абсолютного времени)
    pub base_time: i64,

    /// Reserved (обширный padding до конца страницы)
    pub reserved_end: [u8; 0x200],
}

impl KUSER_SHARED_DATA {
    pub const fn new() -> Self {
        Self {
            tick_count_low_deprecated: 0,
            tick_count_multiplier: 0x0FA00000, // Standard multiplier

            interrupt_time: KSYSTEM_TIME::new(),
            system_time: KSYSTEM_TIME::new(),
            time_zone_bias: KSYSTEM_TIME::new(),

            image_number_low: 0x8664, // IMAGE_FILE_MACHINE_AMD64
            image_number_high: 0x8664,
            nt_system_root: [0; 260],

            max_stack_trace_depth: 0,
            crypto_exponent: 0,
            time_zone_id: 0,
            large_page_minimum: 0x200000, // 2MB

            reserved2: [0; 7],

            nt_product_type: 1, // NtProductWinNt
            product_type_is_valid: 1,

            reserved0: [0; 1],
            native_processor_architecture: 9, // PROCESSOR_ARCHITECTURE_AMD64

            nt_major_version: 5,
            nt_minor_version: 1, // XP

            processor_features: [0; 64],

            reserved1: 0,
            reserved3: 0,

            time_slip: 0,
            alternative_architecture: 0,
            boot_id: 0,
            system_expiration_date: 0,
            suite_mask: 0,

            kd_debugger_enabled: 0,
            mitigation_policies: 0,
            reserved6: [0; 2],

            active_console_id: 0,
            dismount_count: 0,
            com_plus_package: 0,
            last_system_rit_event_tick_count: 0,
            number_of_physical_pages: 0,

            safe_boot_mode: 0,
            virtualization_flags: 0,
            reserved12: [0; 2],

            shared_data_flags: 0,
            data_flags_pad: [0; 1],

            test_ret_instruction: 0,
            qpc_frequency: 0,

            system_call: 0,
            system_call_pad0: 0,
            system_call_return: 0,
            system_call_pad: [0; 3],

            tick_count: KSYSTEM_TIME::new(),
            tick_count_pad: [0; 1],

            cookie: 0,
            cookie_pad: [0; 1],

            console_session_foreground_process_id: 0,
            time_update_lock: 0,
            base_time: 0,

            reserved_end: [0; 0x200],
        }
    }
}

/// KSYSTEM_TIME - атомарное 64-битное время через два 32-битных поля
#[repr(C)]
#[derive(Clone, Copy)]
pub struct KSYSTEM_TIME {
    pub low_part: u32,
    pub high1_time: i32,
    pub high2_time: i32,
}

impl KSYSTEM_TIME {
    pub const fn new() -> Self {
        Self {
            low_part: 0,
            high1_time: 0,
            high2_time: 0,
        }
    }

    /// Читает 64-битное значение атомарно
    #[inline]
    pub fn read(&self) -> i64 {
        loop {
            let high1 = unsafe { core::ptr::read_volatile(&self.high1_time) };
            let low = unsafe { core::ptr::read_volatile(&self.low_part) };
            let high2 = unsafe { core::ptr::read_volatile(&self.high2_time) };

            if high1 == high2 {
                return ((high1 as i64) << 32) | (low as i64);
            }
            // Если high1 != high2, значит было обновление во время чтения
            crate::arch::x86_64::cpu::yield_processor();
        }
    }

    /// Записывает 64-битное значение атомарно
    #[inline]
    pub fn write(&mut self, value: i64) {
        let high = (value >> 32) as i32;
        let low = value as u32;

        // Порядок важен: сначала high2, потом low, потом high1
        unsafe {
            core::ptr::write_volatile(&mut self.high2_time, high);
            core::ptr::write_volatile(&mut self.low_part, low);
            core::ptr::write_volatile(&mut self.high1_time, high);
        }
    }
}

// =============================================================================
// Глобальные переменные времени
// =============================================================================

/// SharedUserData - глобальный экземпляр
/// TODO: Разместить по фиксированному адресу для user mode доступа
pub static mut SHARED_USER_DATA: KUSER_SHARED_DATA = KUSER_SHARED_DATA::new();

/// Interrupt time в 100ns единицах (volatile)
static INTERRUPT_TIME: AtomicU64 = AtomicU64::new(0);

/// System time в 100ns единицах от 1 Jan 1601 (volatile)
static SYSTEM_TIME: AtomicU64 = AtomicU64::new(0);

/// Tick count (количество тиков таймера)
static TICK_COUNT: AtomicU64 = AtomicU64::new(0);

/// Time increment в 100ns единицах (установленный интервал таймера)
static TIME_INCREMENT: AtomicU32 = AtomicU32::new(100_000); // 10ms default

// =============================================================================
// KeUpdateSystemTime - главная функция обновления времени
// =============================================================================

/// KeUpdateSystemTime - обновляет системное время при каждом clock interrupt
///
/// Вызывается из HalpClockInterrupt. Выполняет:
/// 1. Обновление InterruptTime
/// 2. Обновление SystemTime
/// 3. Обновление TickCount
/// 4. Проверка и обработка истекших таймеров
/// 5. Вызов KeUpdateRunTime для quantum management
///
/// # Arguments
/// * `increment` - инкремент времени в 100ns единицах
///
/// # Safety
/// Должен вызываться из interrupt context на CLOCK_LEVEL
pub unsafe fn ke_update_system_time(increment: u32) {
    unsafe {
        // 1. Обновляем interrupt time
        let new_interrupt_time =
            INTERRUPT_TIME.fetch_add(increment as u64, Ordering::SeqCst) + increment as u64;

        // 2. Обновляем system time (interrupt time + base time)
        let base_time = unsafe { (*(&raw const SHARED_USER_DATA)).base_time };
        let new_system_time = new_interrupt_time as i64 + base_time;
        SYSTEM_TIME.store(new_system_time as u64, Ordering::SeqCst);

        // 3. Обновляем tick count
        let new_tick_count = TICK_COUNT.fetch_add(1, Ordering::SeqCst) + 1;

        // 4. Обновляем SharedUserData (для user mode)
        unsafe {
            let sud = &raw mut SHARED_USER_DATA;
            (*sud).interrupt_time.write(new_interrupt_time as i64);
            (*sud).system_time.write(new_system_time);
            (*sud).tick_count.write(new_tick_count as i64);
        }

        // 5. Проверяем наличие истекших таймеров (быстрая проверка без блокировок)
        //
        // ReactOS pattern: Hand = TickCount & 511, проверяем bucket[Hand].time <= InterruptTime
        // Если есть истекшие таймеры - запрашиваем software interrupt для обработки на DISPATCH_LEVEL
        if let Some(table_index) = unsafe { super::timer::ki_check_timer_table(new_interrupt_time) }
        {
            // DEBUG: вывод через debugcon без блокировок
            super::debug::debug_raw("[TIME] Timer expired, requesting DISPATCH_LEVEL interrupt\n");

            let prcb = pcr::get_prcb();
            if !prcb.is_null() {
                unsafe {
                    if (*prcb).timer_request == 0 {
                        (*prcb).timer_hand = table_index as u32;
                        (*prcb).timer_request = 1;
                        // Запрашиваем software interrupt для обработки на DISPATCH_LEVEL
                        crate::hal::swint::hal_request_software_interrupt(
                            crate::hal::irql::DISPATCH_LEVEL,
                        );
                    }
                }
            }
        }

        // 6. Обновляем run time текущего потока (quantum)
        ke_update_run_time();
    }
}

/// KeUpdateRunTime - обновляет quantum текущего потока
///
/// Вызывается из KeUpdateSystemTime для декремента quantum
/// и возможного preemption.
unsafe fn ke_update_run_time() {
    unsafe {
        let prcb = pcr::get_prcb();
        if prcb.is_null() {
            return;
        }

        unsafe {
            let thread = (*prcb).current_thread as *mut super::thread::KTHREAD;
            if thread.is_null() {
                return;
            }

            // Инкрементируем DPC time если в DPC
            // (для отслеживания зависших DPC)

            // Проверяем quantum текущего потока
            // Это делается в ki_quantum_tick который вызывается отдельно
        }
    }
}

// =============================================================================
// Функции запроса времени
// =============================================================================

/// KeQueryInterruptTime - возвращает interrupt time в 100ns
#[inline]
pub fn ke_query_interrupt_time() -> u64 {
    INTERRUPT_TIME.load(Ordering::Acquire)
}

/// KeQuerySystemTime - возвращает system time в 100ns от 1601
#[inline]
pub fn ke_query_system_time() -> u64 {
    SYSTEM_TIME.load(Ordering::Acquire)
}

/// KeQueryTickCount - возвращает tick count
#[inline]
pub fn ke_query_tick_count() -> u64 {
    TICK_COUNT.load(Ordering::Acquire)
}

/// KeQueryTimeIncrement - возвращает time increment в 100ns
#[inline]
pub fn ke_query_time_increment() -> u32 {
    TIME_INCREMENT.load(Ordering::Acquire)
}

/// KeSetTimeIncrement - устанавливает time increment
pub fn ke_set_time_increment(increment: u32) -> u32 {
    let old = TIME_INCREMENT.swap(increment, Ordering::AcqRel);
    old
}

// =============================================================================
// Инициализация времени
// =============================================================================

/// Инициализирует подсистему времени
///
/// Читает текущее время из CMOS RTC и устанавливает базовое системное время.
///
/// # Returns
/// `Some(TIME_FIELDS)` с текущим временем если RTC прочитан успешно, иначе `None`
pub fn ke_initialize_time() -> Option<crate::hal::rtc::TIME_FIELDS> {
    use crate::hal::rtc::TIME_FIELDS;
    use crate::hal::rtc::hal_query_real_time_clock;
    use crate::hal::rtc::time_fields_to_time;

    let mut time_fields = TIME_FIELDS::default();
    let rtc_ok = unsafe { hal_query_real_time_clock(&mut time_fields) };

    let base_time = if rtc_ok {
        time_fields_to_time(&time_fields)
    } else {
        // Fallback: 1 Jan 2024 00:00:00 (примерное время)
        // Это лучше чем 0 (1601 год)
        const FALLBACK_2024: i64 = 133_479_360_000_000_000; // 1 Jan 2024
        FALLBACK_2024
    };

    unsafe {
        let sud = &raw mut SHARED_USER_DATA;
        (*sud).base_time = base_time;
        (*sud).nt_major_version = 5;
        (*sud).nt_minor_version = 1;
        // Получаем реальное количество физических страниц из MM
        (*sud).number_of_physical_pages = crate::mm::pfn::mm_get_number_of_physical_pages() as u32;
    }

    if rtc_ok { Some(time_fields) } else { None }
}

/// Устанавливает базовое системное время (из RTC)
pub fn ke_set_base_time(base_time_100ns: i64) {
    unsafe {
        let sud = &raw mut SHARED_USER_DATA;
        (*sud).base_time = base_time_100ns;
    }
}
