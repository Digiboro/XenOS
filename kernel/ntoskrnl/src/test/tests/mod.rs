//! Kernel Tests
//!
//! Все тесты ядра организованы по модулям.
//! Тесты запускаются в режиме test-kernel после минимальной инициализации.

extern crate alloc;

use super::harness::KernelTest;

// Модули тестов
pub mod mm;
pub mod nls;
pub mod ob;
pub mod syscall;

// =============================================================================
// Базовые тесты (sanity checks)
// =============================================================================

/// Тест: базовая проверка что тестовый фреймворк работает
fn test_sanity_check() {
    // Этот тест всегда проходит - проверяем что фреймворк работает
}

/// Тест: проверка работы аллокатора
fn test_allocator_basic() {
    use alloc::vec::Vec;

    // Аллоцируем вектор
    let mut v: Vec<u64> = Vec::with_capacity(16);
    for i in 0..16 {
        v.push(i);
    }

    // Проверяем содержимое
    for i in 0..16 {
        assert_eq!(v[i], i as u64);
    }
}

/// Тест: проверка работы Box
fn test_box_allocation() {
    use alloc::boxed::Box;

    let boxed = Box::new(0xDEADBEEFu64);
    assert_eq!(*boxed, 0xDEADBEEF);
}

// =============================================================================
// Базовые тесты
// =============================================================================

static BASIC_TESTS: &[KernelTest] = &[
    KernelTest {
        name: "sanity_check",
        module: "basic",
        test_fn: test_sanity_check,
    },
    KernelTest {
        name: "allocator_basic",
        module: "basic",
        test_fn: test_allocator_basic,
    },
    KernelTest {
        name: "box_allocation",
        module: "basic",
        test_fn: test_box_allocation,
    },
];

// =============================================================================
// Реестр всех тестов
// =============================================================================

/// Все тестовые suites
static ALL_TEST_SUITES: &[&[KernelTest]] = &[
    BASIC_TESTS,
    mm::MM_TESTS,
    nls::NLS_TESTS,
    ob::OB_TESTS,
    syscall::SYSCALL_TESTS,
];

/// Возвращает все тесты
pub fn get_all_tests() -> &'static [&'static [KernelTest]] {
    ALL_TEST_SUITES
}

/// Общее количество тестов
pub fn total_test_count() -> usize {
    ALL_TEST_SUITES.iter().map(|s| s.len()).sum()
}
