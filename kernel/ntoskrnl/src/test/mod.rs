//! Kernel Test Framework
//!
//! Фреймворк для запуска тестов ядра в QEMU.
//! Активируется через feature `test-kernel`.

mod exit;
mod harness;
pub mod tests;

pub use exit::QemuExitCode;
pub use exit::exit_qemu;
pub use harness::KernelTest;
pub use harness::TestResult;
pub use harness::run_all_test_suites;
// Re-export тестов
pub use tests::get_all_tests;
