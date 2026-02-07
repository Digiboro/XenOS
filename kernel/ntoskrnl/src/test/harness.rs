//! Test Runner - запуск тестов и сбор результатов

use super::exit::QemuExitCode;
use super::exit::exit_qemu;
use crate::kd::BV_COLOR_LIGHT_GREEN;
use crate::kd::BV_COLOR_LIGHT_RED;
use crate::kd::dbg_print;
use crate::kd::dbg_print_colored;
use crate::kd::dbg_print_num;

/// Информация о тесте
pub struct KernelTest {
    /// Имя теста
    pub name: &'static str,
    /// Модуль где определен тест
    pub module: &'static str,
    /// Функция теста
    pub test_fn: fn(),
}

/// Результат выполнения теста
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestResult {
    Passed,
    Failed,
}

/// Запустить все тесты из вложенных массивов
///
/// Эта функция никогда не возвращается - завершает QEMU.
pub fn run_all_test_suites(suites: &[&[KernelTest]]) -> ! {
    dbg_print("\n");
    dbg_print("================================================================================\n");
    dbg_print("                         KERNEL TEST SUITE\n");
    dbg_print("================================================================================\n");
    dbg_print("\n");

    // Подсчитываем общее количество тестов
    let total: usize = suites.iter().map(|s| s.len()).sum();
    let mut passed = 0usize;
    let mut failed = 0usize;

    dbg_print("Running ");
    dbg_print_num(total as u64);
    dbg_print(" tests...\n\n");

    // Запускаем все тесты из всех suites
    for suite in suites {
        for test in *suite {
            dbg_print("test ");
            dbg_print(test.module);
            dbg_print("::");
            dbg_print(test.name);
            dbg_print(" ... ");

            // Запускаем тест
            let result = run_single_test(test);

            match result {
                TestResult::Passed => {
                    dbg_print_colored("ok", BV_COLOR_LIGHT_GREEN);
                    passed += 1;
                },
                TestResult::Failed => {
                    dbg_print_colored("FAILED", BV_COLOR_LIGHT_RED);
                    failed += 1;
                },
            }
            dbg_print("\n");
        }
    }

    // Итоги
    dbg_print("\n");
    dbg_print("--------------------------------------------------------------------------------\n");
    dbg_print("test result: ");

    if failed == 0 {
        dbg_print_colored("ok", BV_COLOR_LIGHT_GREEN);
    } else {
        dbg_print_colored("FAILED", BV_COLOR_LIGHT_RED);
    }

    dbg_print(". ");
    dbg_print_num(passed as u64);
    dbg_print(" passed; ");
    dbg_print_num(failed as u64);
    dbg_print(" failed\n");
    dbg_print("================================================================================\n");

    // Выход из QEMU
    dbg_print("\n[TEST] Exiting QEMU...\n");
    if failed == 0 {
        exit_qemu(QemuExitCode::Success);
    } else {
        exit_qemu(QemuExitCode::Failure);
    }
}

/// Запускает один тест и возвращает результат
fn run_single_test(test: &KernelTest) -> TestResult {
    // TODO: В будущем можно добавить перехват panic через custom panic handler
    // Пока просто вызываем функцию - panic приведет к остановке всех тестов
    (test.test_fn)();
    TestResult::Passed
}
