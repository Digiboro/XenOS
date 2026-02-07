//! KD Serial Provider
//!
//! Провайдер вывода через COM порт.
//!
//! Источники:
//! - ReactOS: kd/kdio.c (KdpSerialInit, KdpSerialPrint)
//! - ReactOS: drivers/base/kdcom/kdcom.c

use core::sync::atomic::AtomicBool;
use core::sync::atomic::AtomicU16;
use core::sync::atomic::AtomicU32;
use core::sync::atomic::Ordering;

use super::data::*;
use crate::hal::portio::read_port_uchar;
use crate::hal::portio::write_port_uchar;
use crate::ke::spinlock::HIGH_LEVEL;
use crate::ke::spinlock::KSPIN_LOCK;
use crate::ke::spinlock::ke_acquire_spin_lock_raise_to;
use crate::ke::spinlock::ke_release_spin_lock;

// =============================================================================
// Serial Port Registers (относительно базового адреса)
// =============================================================================

/// Data register (read/write)
const SERIAL_DATA: u16 = 0;
/// Interrupt Enable Register
const SERIAL_IER: u16 = 1;
/// FIFO Control Register (write) / Interrupt ID Register (read)
const SERIAL_FCR: u16 = 2;
/// Line Control Register
const SERIAL_LCR: u16 = 3;
/// Modem Control Register
const SERIAL_MCR: u16 = 4;
/// Line Status Register
const SERIAL_LSR: u16 = 5;
/// Modem Status Register
const SERIAL_MSR: u16 = 6;

// Line Status Register bits
const LSR_DATA_READY: u8 = 0x01;
const LSR_TRANSMIT_EMPTY: u8 = 0x20;

// Line Control Register bits
const LCR_DLAB: u8 = 0x80;
const LCR_8N1: u8 = 0x03; // 8 data bits, no parity, 1 stop bit

// Modem Control Register bits
const MCR_DTR: u8 = 0x01;
const MCR_RTS: u8 = 0x02;
const MCR_OUT2: u8 = 0x08;

// FIFO Control Register bits
const FCR_ENABLE: u8 = 0x01;
const FCR_CLEAR_RX: u8 = 0x02;
const FCR_CLEAR_TX: u8 = 0x04;
const FCR_TRIGGER_14: u8 = 0xC0;

// =============================================================================
// Serial Port State
// =============================================================================

/// Базовый адрес порта (atomic для безопасного доступа)
static SERIAL_PORT_ADDRESS: AtomicU16 = AtomicU16::new(0);

/// Baud rate (atomic)
static SERIAL_PORT_BAUD: AtomicU32 = AtomicU32::new(0);

/// Serial port инициализирован
static SERIAL_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Спинлок для сериализации вывода в COM порт.
///
/// NT/ReactOS: печать через KD serial защищена spinlock'ом и делается на HIGH_LEVEL,
/// чтобы исключить deadlock при реэнтерабельном выводе из IRQ на том же CPU.
static KDP_SERIAL_PRINT_LOCK: KSPIN_LOCK = KSPIN_LOCK::new();

// =============================================================================
// Serial Port Initialization
// =============================================================================

/// Инициализация COM порта
///
/// Возвращает true если порт успешно инициализирован
pub fn kd_port_initialize(port_number: u32) -> bool {
    let base = get_com_port_base(port_number);
    let baud_rate = SERIAL_BAUD_RATE.load(Ordering::Relaxed);

    // Вычисляем divisor для baud rate
    // Базовая частота UART = 115200 * 16 = 1843200 Hz
    let divisor = if baud_rate > 0 {
        115200 / baud_rate
    } else {
        1 // 115200 baud
    };

    // Отключаем прерывания
    write_port_uchar(base + SERIAL_IER, 0x00);

    // Устанавливаем DLAB для настройки baud rate
    write_port_uchar(base + SERIAL_LCR, LCR_DLAB);

    // Устанавливаем divisor (low byte, high byte)
    write_port_uchar(base + SERIAL_DATA, (divisor & 0xFF) as u8);
    write_port_uchar(base + SERIAL_IER, ((divisor >> 8) & 0xFF) as u8);

    // 8 бит, без четности, 1 стоп-бит (сбрасывает DLAB)
    write_port_uchar(base + SERIAL_LCR, LCR_8N1);

    // Включаем и очищаем FIFO, trigger level = 14 bytes
    write_port_uchar(
        base + SERIAL_FCR,
        FCR_ENABLE | FCR_CLEAR_RX | FCR_CLEAR_TX | FCR_TRIGGER_14,
    );

    // DTR + RTS + OUT2 (OUT2 нужен для прерываний, но мы их не используем)
    write_port_uchar(base + SERIAL_MCR, MCR_DTR | MCR_RTS | MCR_OUT2);

    // Проверяем, что порт работает (loopback test)
    // Включаем loopback mode
    write_port_uchar(base + SERIAL_MCR, MCR_DTR | MCR_RTS | MCR_OUT2 | 0x10);

    // Отправляем тестовый байт
    write_port_uchar(base + SERIAL_DATA, 0xAE);

    // Ждем и читаем
    for _ in 0..1000 {
        if (read_port_uchar(base + SERIAL_LSR) & LSR_DATA_READY) != 0 {
            break;
        }
    }

    let received = read_port_uchar(base + SERIAL_DATA);

    // Выключаем loopback
    write_port_uchar(base + SERIAL_MCR, MCR_DTR | MCR_RTS | MCR_OUT2);

    // Проверяем результат
    if received != 0xAE {
        // Порт не работает, но все равно пробуем использовать
        // (в эмуляторах loopback может не работать)
    }

    // Сохраняем информацию о порте (атомарно)
    SERIAL_PORT_ADDRESS.store(base, Ordering::Release);
    SERIAL_PORT_BAUD.store(baud_rate, Ordering::Release);

    true
}

/// Инициализация Serial провайдера для KD
pub fn kdp_serial_init() -> bool {
    // NB: на SMP возможна гонка двойной инициализации. Фиксируем через CAS.
    if SERIAL_INITIALIZED.load(Ordering::Acquire) {
        return true;
    }

    if SERIAL_INITIALIZED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
        .is_err()
    {
        return true;
    }

    let port_number = SERIAL_PORT_NUMBER.load(Ordering::Relaxed);

    if kd_port_initialize(port_number) {
        true
    } else {
        // Инициализация не удалась — откатываем флаг.
        SERIAL_INITIALIZED.store(false, Ordering::Release);
        false
    }
}

// =============================================================================
// Serial Port Output
// =============================================================================

/// Отправить байт в COM порт
#[inline]
pub fn kd_port_put_byte(base: u16, byte: u8) {
    if base == 0 {
        return;
    }

    // Ждем пока transmit buffer пуст
    // Timeout ~1ms при 1GHz CPU
    for _ in 0..100_000 {
        if (read_port_uchar(base + SERIAL_LSR) & LSR_TRANSMIT_EMPTY) != 0 {
            break;
        }
    }

    // Отправляем байт
    write_port_uchar(base + SERIAL_DATA, byte);
}

/// Отправить байт (используя глобальный порт)
#[inline]
pub fn kd_port_put_byte_ex(byte: u8) {
    let base = SERIAL_PORT_ADDRESS.load(Ordering::Relaxed);
    if base != 0 {
        kd_port_put_byte(base, byte);
    }
}

/// Вывод строки через Serial
///
/// Это основная функция провайдера, вызываемая из KdIoPrintString
pub fn kdp_serial_print(string: &[u8]) {
    if !SERIAL_INITIALIZED.load(Ordering::Acquire) {
        return;
    }

    let base = SERIAL_PORT_ADDRESS.load(Ordering::Relaxed);
    if base == 0 {
        return;
    }

    // Сериализуем вывод, чтобы строки не перемешивались побайтно между CPU/IRQ.
    let old_irql = ke_acquire_spin_lock_raise_to(&KDP_SERIAL_PRINT_LOCK, HIGH_LEVEL);

    for &byte in string {
        // Конвертируем \n в \r\n
        if byte == b'\n' {
            kd_port_put_byte(base, b'\r');
        }
        kd_port_put_byte(base, byte);
    }

    ke_release_spin_lock(&KDP_SERIAL_PRINT_LOCK, old_irql);
}

/// Вывод строки через Serial (string slice версия)
pub fn kdp_serial_print_str(s: &str) {
    kdp_serial_print(s.as_bytes());
}

/// Вывод строки через Serial при уже захваченном внешнем lock'е.
///
/// Не захватывает `KDP_SERIAL_PRINT_LOCK` - предполагается что вызывающий
/// код уже держит глобальный `KD_PRINT_LOCK`.
pub fn kdp_serial_print_str_locked(s: &str) {
    if !SERIAL_INITIALIZED.load(Ordering::Acquire) {
        return;
    }

    let base = SERIAL_PORT_ADDRESS.load(Ordering::Relaxed);
    if base == 0 {
        return;
    }

    for &byte in s.as_bytes() {
        // Конвертируем \n в \r\n
        if byte == b'\n' {
            kd_port_put_byte(base, b'\r');
        }
        kd_port_put_byte(base, byte);
    }
}

// =============================================================================
// Serial Port Input (для будущего использования)
// =============================================================================

/// Проверить, есть ли данные для чтения
#[inline]
pub fn kd_port_get_byte_available() -> bool {
    let base = SERIAL_PORT_ADDRESS.load(Ordering::Relaxed);
    if base == 0 {
        return false;
    }
    (read_port_uchar(base + SERIAL_LSR) & LSR_DATA_READY) != 0
}

/// Прочитать байт из COM порта (неблокирующий)
pub fn kd_port_get_byte() -> Option<u8> {
    let base = SERIAL_PORT_ADDRESS.load(Ordering::Relaxed);
    if base == 0 {
        return None;
    }

    if (read_port_uchar(base + SERIAL_LSR) & LSR_DATA_READY) != 0 {
        Some(read_port_uchar(base + SERIAL_DATA))
    } else {
        None
    }
}

/// Проверка нажатия Ctrl+C (для break-in)
pub fn kd_poll_break_in() -> bool {
    if let Some(byte) = kd_port_get_byte() {
        // Ctrl+C = 0x03
        byte == 0x03
    } else {
        false
    }
}

// =============================================================================
// High-level Serial API
// =============================================================================

/// Инициализировать serial с параметрами по умолчанию
pub fn serial_init_default() {
    SERIAL_PORT_NUMBER.store(1, Ordering::Relaxed);
    SERIAL_BAUD_RATE.store(DEFAULT_DEBUG_BAUD_RATE, Ordering::Relaxed);
    kdp_serial_init();
}

/// Инициализировать KD serial по ACPI SPCR (если доступно), иначе использовать дефолт.
///
/// На текущем этапе поддерживаем только legacy I/O порты 16550 (COM1..COM4),
/// т.к. KD транспорт выбран как "UART как консоль".
pub fn serial_init_from_spcr_or_default() {
    let rsdp_phys = kd_get_acpi_rsdp_physical() as usize;
    if rsdp_phys == 0 {
        serial_init_default();
        return;
    }

    // Пытаемся получить SPCR.
    let handler = crate::acpi::XenAcpiHandler::new();
    let Ok(tables) = (unsafe { crate::acpi::AcpiTables::from_rsdp(handler, rsdp_phys) }) else {
        serial_init_default();
        return;
    };

    let Some(spcr) = tables.find_table::<crate::acpi::sdt::spcr::Spcr>() else {
        serial_init_default();
        return;
    };

    // Нас интересует только System I/O базовый адрес.
    let Some(Ok(base)) = spcr.base_address() else {
        serial_init_default();
        return;
    };
    if base.address_space != crate::acpi::address::AddressSpace::SystemIo {
        serial_init_default();
        return;
    }
    let base_port = base.address as u16;

    // Определяем COM номер по стандартным базам.
    let port_number = match base_port {
        COM1_BASE => 1,
        COM2_BASE => 2,
        COM3_BASE => 3,
        COM4_BASE => 4,
        _ => {
            serial_init_default();
            return;
        },
    };

    // Baud rate из SPCR (если указан), иначе дефолт.
    let baud = spcr
        .baud_rate()
        .map(|b| b.get())
        .unwrap_or(DEFAULT_DEBUG_BAUD_RATE);

    SERIAL_PORT_NUMBER.store(port_number, Ordering::Relaxed);
    SERIAL_BAUD_RATE.store(baud, Ordering::Relaxed);
    kdp_serial_init();
}

/// Вывести строку в serial (удобная функция)
pub fn serial_print(s: &str) {
    kdp_serial_print_str(s);
}

/// Вывести число в serial
pub fn serial_print_num(mut n: u64) {
    if n == 0 {
        kd_port_put_byte_ex(b'0');
        return;
    }

    let mut buf = [0u8; 20];
    let mut i = 0;

    while n > 0 {
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        i += 1;
    }

    while i > 0 {
        i -= 1;
        kd_port_put_byte_ex(buf[i]);
    }
}

/// Вывести число в hex формате
pub fn serial_print_hex(mut n: u64) {
    if n == 0 {
        kd_port_put_byte_ex(b'0');
        return;
    }

    let mut buf = [0u8; 16];
    let mut i = 0;

    while n > 0 {
        let digit = (n & 0xF) as u8;
        buf[i] = if digit < 10 {
            b'0' + digit
        } else {
            b'A' + digit - 10
        };
        n >>= 4;
        i += 1;
    }

    while i > 0 {
        i -= 1;
        kd_port_put_byte_ex(buf[i]);
    }
}
