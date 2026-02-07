//! 8254 PIT - Programmable Interval Timer
//!
//! PIT используется только для:
//! - Калибровки APIC Timer и TSC
//! - Busy-wait задержек (ke_stall_execution_processor)
//!
//! Источники:
//! - ReactOS: hal/halx86/generic/timer.c

use core::sync::atomic::AtomicU32;
use core::sync::atomic::Ordering;

use super::portio::read_port_uchar;
use super::portio::write_port_uchar;

// =============================================================================
// PIT Constants
// =============================================================================

/// PIT base frequency (Hz) - 1.193182 MHz
pub const PIT_FREQUENCY: u32 = 1_193_182;

/// Channel 0 data port (system timer)
pub const PIT_CHANNEL0_DATA: u16 = 0x40;

/// Channel 2 data port (speaker)
pub const PIT_CHANNEL2_DATA: u16 = 0x42;

/// Mode/Command register
pub const PIT_COMMAND: u16 = 0x43;

// =============================================================================
// PIT Command Byte
// =============================================================================

/// Channel select
pub const PIT_CHANNEL_0: u8 = 0b00_000000;

/// Access mode
pub const PIT_ACCESS_LATCH: u8 = 0b00_00_0000;
pub const PIT_ACCESS_LOHI: u8 = 0b00_11_0000;

/// Operating mode
pub const PIT_MODE_2: u8 = 0b0000_100; // Rate generator

/// BCD/Binary mode
pub const PIT_BINARY: u8 = 0b0000_0000;

// =============================================================================
// State
// =============================================================================

/// Текущее значение rollover (reload value)
static CURRENT_ROLLOVER: AtomicU32 = AtomicU32::new(0);

// =============================================================================
// Busy-Wait Delay
// =============================================================================

/// KeStallExecutionProcessor - точная busy-wait задержка
///
/// Используется для:
/// - Калибровки APIC Timer
/// - Калибровки TSC
/// - Коротких задержек при инициализации hardware
///
/// # Arguments
/// * `microseconds` - время задержки в микросекундах
pub fn ke_stall_execution_processor(microseconds: u32) {
    if microseconds == 0 {
        return;
    }

    if CURRENT_ROLLOVER.load(Ordering::Acquire) == 0 {
        halp_set_timer_rollover(0xFFFF);
    }

    let ticks_to_wait = (microseconds as u64 * PIT_FREQUENCY as u64) / 1_000_000;

    let mut remaining = ticks_to_wait;
    let mut last_count = halp_read_pit_counter() as u64;

    while remaining > 0 {
        let current_count = halp_read_pit_counter() as u64;

        let elapsed = if current_count <= last_count {
            last_count - current_count
        } else {
            let rollover = CURRENT_ROLLOVER.load(Ordering::Acquire) as u64;
            last_count + (rollover - current_count)
        };

        if elapsed >= remaining {
            break;
        }
        remaining -= elapsed;
        last_count = current_count;

        crate::arch::x86_64::cpu::yield_processor();
    }
}

/// Короткая задержка в наносекундах (округляется вверх до микросекунд)
#[inline]
pub fn ke_stall_execution_nanoseconds(nanoseconds: u32) {
    ke_stall_execution_processor((nanoseconds + 999) / 1000);
}

// =============================================================================
// PIT Counter Access
// =============================================================================

/// Читает текущее значение счетчика PIT Channel 0
pub fn halp_read_pit_counter() -> u16 {
    write_port_uchar(PIT_COMMAND, PIT_CHANNEL_0 | PIT_ACCESS_LATCH);

    let low = read_port_uchar(PIT_CHANNEL0_DATA) as u16;
    let high = read_port_uchar(PIT_CHANNEL0_DATA) as u16;

    (high << 8) | low
}

/// Устанавливает rollover value для PIT Channel 0
fn halp_set_timer_rollover(rollover: u16) {
    CURRENT_ROLLOVER.store(rollover as u32, Ordering::Release);

    let command = PIT_CHANNEL_0 | PIT_ACCESS_LOHI | PIT_MODE_2 | PIT_BINARY;
    write_port_uchar(PIT_COMMAND, command);

    write_port_uchar(PIT_CHANNEL0_DATA, (rollover & 0xFF) as u8);
    write_port_uchar(PIT_CHANNEL0_DATA, (rollover >> 8) as u8);
}
