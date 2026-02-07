//! HAL Processor Functions
//!
//! Per-CPU инициализация и управление процессорами.
//!
//! Источники:
//! - ReactOS: hal/halx86/generic/halinit.c

use core::sync::atomic::AtomicU32;
use core::sync::atomic::AtomicU64;
use core::sync::atomic::Ordering;

// =============================================================================
// Processor State
// =============================================================================

/// Количество активных процессоров
static HALP_ACTIVE_PROCESSORS: AtomicU64 = AtomicU64::new(0);

/// Affinity mask для прерываний по умолчанию
static HALP_DEFAULT_INTERRUPT_AFFINITY: AtomicU64 = AtomicU64::new(0);

/// Номер текущего процессора при инициализации
static HALP_PROCESSOR_COUNT: AtomicU32 = AtomicU32::new(0);

// =============================================================================
// Processor Initialization
// =============================================================================

/// HalInitializeProcessor - инициализация процессора
///
/// Вызывается для каждого CPU при его старте.
pub fn hal_initialize_processor(processor_number: u32) {
    // Устанавливаем бит в маске активных процессоров
    let mask = 1u64 << processor_number;
    HALP_ACTIVE_PROCESSORS.fetch_or(mask, Ordering::SeqCst);
    HALP_DEFAULT_INTERRUPT_AFFINITY.fetch_or(mask, Ordering::SeqCst);

    // Обновляем счетчик процессоров
    let count = HALP_PROCESSOR_COUNT.load(Ordering::Acquire);
    if processor_number >= count {
        HALP_PROCESSOR_COUNT.store(processor_number + 1, Ordering::Release);
    }

    // NB: IRQL и состояние IF (sti/cli) должны устанавливаться кодом ядра (Ki*),
    // а не HAL. HAL не должен "самовольно" менять IRQL в этой точке.

    // Для BSP (processor 0) выполняем дополнительную инициализацию
    if processor_number == 0 {
        halp_init_bsp();
    } else {
        halp_init_ap(processor_number);
    }
}

/// Инициализация Bootstrap Processor
fn halp_init_bsp() {
    // TODO: BSP-specific инициализация
    // PIC и PIT инициализируются позже в HalInitSystem
}

/// Инициализация Application Processor
fn halp_init_ap(_processor_number: u32) {
    // TODO: AP-specific инициализация
    // В SMP системе здесь инициализируется Local APIC
}

// =============================================================================
// Processor Query Functions
// =============================================================================

/// Возвращает количество активных процессоров
#[inline]
pub fn hal_get_processor_count() -> u32 {
    HALP_PROCESSOR_COUNT.load(Ordering::Acquire)
}

/// Возвращает маску активных процессоров
#[inline]
pub fn hal_get_active_processors() -> u64 {
    HALP_ACTIVE_PROCESSORS.load(Ordering::Acquire)
}

/// Проверяет активен ли указанный процессор
#[inline]
pub fn hal_is_processor_active(processor_number: u32) -> bool {
    let mask = 1u64 << processor_number;
    (HALP_ACTIVE_PROCESSORS.load(Ordering::Acquire) & mask) != 0
}

// =============================================================================
// System Control
// =============================================================================

/// HalReturnToFirmware - возврат к firmware (reboot/shutdown)
///
/// # Arguments
/// * `action` - действие (reboot, shutdown, etc.)
pub fn hal_return_to_firmware(action: HalFirmwareAction) {
    // Отключаем прерывания
    unsafe {
        super::irql::disable_interrupts();
    }

    match action {
        HalFirmwareAction::HalRebootRoutine => {
            // Сброс через keyboard controller (традиционный метод)
            keyboard_reset();
        },
        HalFirmwareAction::HalHaltRoutine => {
            // Остановка процессора
            loop {
                unsafe {
                    core::arch::asm!("hlt", options(nomem, nostack));
                }
            }
        },
        HalFirmwareAction::HalPowerDownRoutine => {
            // TODO: Выключение через ACPI (не реализовано)
            // Пока просто halt
            loop {
                unsafe {
                    core::arch::asm!("hlt", options(nomem, nostack));
                }
            }
        },
    }
}

/// Типы действий для HalReturnToFirmware
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HalFirmwareAction {
    /// Перезагрузка
    HalRebootRoutine,
    /// Остановка (halt)
    HalHaltRoutine,
    /// Выключение питания
    HalPowerDownRoutine,
}

/// Сброс через keyboard controller
fn keyboard_reset() {
    use super::portio::read_port_uchar;
    use super::portio::write_port_uchar;

    const KBD_STATUS_PORT: u16 = 0x64;
    const KBD_DATA_PORT: u16 = 0x60;
    const KBD_RESET_CMD: u8 = 0xFE;

    // Ждем пока контроллер готов
    for _ in 0..10000 {
        if (read_port_uchar(KBD_STATUS_PORT) & 0x02) == 0 {
            break;
        }
        super::portio::io_delay();
    }

    // Отправляем команду сброса
    write_port_uchar(KBD_STATUS_PORT, KBD_RESET_CMD);

    // Если не сработало - halt
    loop {
        unsafe {
            core::arch::asm!("hlt", options(nomem, nostack));
        }
    }
}

// =============================================================================
// CPU Control
// =============================================================================

/// Halt процессора до следующего прерывания
#[inline]
pub fn hal_halt() {
    unsafe {
        core::arch::asm!("hlt", options(nomem, nostack, preserves_flags));
    }
}

/// Пауза для гипер-треда (CPU yield)
#[inline]
pub fn hal_pause() {
    unsafe {
        core::arch::asm!("pause", options(nomem, nostack, preserves_flags));
    }
}

/// Полный барьер памяти
#[inline]
pub fn hal_memory_barrier() {
    unsafe {
        core::arch::asm!("mfence", options(nomem, nostack, preserves_flags));
    }
}

/// Барьер записи
#[inline]
pub fn hal_store_barrier() {
    unsafe {
        core::arch::asm!("sfence", options(nomem, nostack, preserves_flags));
    }
}

/// Барьер чтения
#[inline]
pub fn hal_load_barrier() {
    unsafe {
        core::arch::asm!("lfence", options(nomem, nostack, preserves_flags));
    }
}
