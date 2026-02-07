//! QEMU Exit через ISA Debug Exit Device
//!
//! Используется для завершения QEMU с кодом успеха/неудачи.

/// Коды выхода для QEMU
///
/// QEMU ISA debug exit device возвращает: (value << 1) | 1
/// - Success (0x10) -> exit code 0x21 (33)
/// - Failure (0x11) -> exit code 0x23 (35)
#[derive(Debug, Clone, Copy)]
#[repr(u32)]
pub enum QemuExitCode {
    Success = 0x10,
    Failure = 0x11,
}

/// Порт ISA Debug Exit Device
const ISA_DEBUG_EXIT_PORT: u16 = 0xf4;

/// Завершить QEMU с указанным кодом
///
/// Эта функция никогда не возвращается.
pub fn exit_qemu(exit_code: QemuExitCode) -> ! {
    unsafe {
        // Записываем в ISA debug exit port - QEMU завершится с кодом (value << 1) | 1
        core::arch::asm!(
            "out dx, al",
            in("dx") ISA_DEBUG_EXIT_PORT,
            in("al") exit_code as u8,
            options(nomem, nostack)
        );
    }

    // Если устройство не настроено - бесконечный цикл
    loop {
        unsafe {
            core::arch::asm!("hlt", options(nomem, nostack));
        }
    }
}
