//! Тестовые потоки PS подсистемы
//!
//! Модуль содержит тестовые системные потоки для проверки работы
//! планировщика и переключения контекста.
//!
//! Активируется feature `ps-test`.

use crate::kd::dbg_print;
use crate::kd::dbg_print_hex;
use crate::kd::dbg_print_num;
use crate::nt::PVOID;

// =============================================================================
// Константы
// =============================================================================

/// Количество тестовых потоков
const PS_TEST_THREAD_COUNT: usize = 10;

/// Интервал задержки между выводами (1 секунда в 100ns единицах)
/// Отрицательное значение = относительное время
const PS_TEST_DELAY_INTERVAL: i64 = -1_000_000; // 0.1 секунды

/// Начальное смещение между потоками (100ms)
const PS_TEST_STAGGER_INTERVAL: i64 = -1_000_000; // 0.1 секунды

/// Через сколько итераций принудительно вызвать BSOD в тесте.
///
/// При текущем `PS_TEST_DELAY_INTERVAL = 0.1s` значение 20 даёт примерно 2 секунды работы.
const PS_TEST_BSOD_AFTER_ITERATIONS: u64 = 200; // Увеличено для отладки

/// Номер тестового потока (1-based), который инициирует BSOD.
const PS_TEST_BSOD_THREAD_NUM: usize = 1;

/// Приоритеты для тестовых потоков (разные для проверки планировщика)
/// Потоки 1-5: низкие приоритеты (4-8)
/// Потоки 6-8: средние приоритеты (9-11)  
/// Потоки 9-10: высокие приоритеты (12-13)
const PS_TEST_PRIORITIES: [i8; PS_TEST_THREAD_COUNT] = [
    4,  // Thread 1: низкий
    5,  // Thread 2: низкий
    6,  // Thread 3: низкий
    7,  // Thread 4: низкий
    8,  // Thread 5: нормальный (default)
    9,  // Thread 6: выше нормального
    10, // Thread 7: выше нормального
    11, // Thread 8: высокий
    12, // Thread 9: высокий
    13, // Thread 10: очень высокий
];

// =============================================================================
// Универсальная функция тестового потока
// =============================================================================

/// Универсальный тестовый системный поток
///
/// Выводит информацию о себе каждую секунду.
/// Номер потока передаётся через context.
pub unsafe extern "win64" fn ps_test_thread_worker(context: PVOID) {
    // Понижаем IRQL до PASSIVE_LEVEL для корректной работы
    crate::hal::irql::kf_lower_irql(crate::hal::irql::PASSIVE_LEVEL);

    // Номер потока из context (1-based для красивого вывода)
    let thread_num = context as usize;

    crate::ke::debug::debug_raw("[PS TEST] Thread ");
    crate::ke::debug::debug_dec(thread_num as u64);
    crate::ke::debug::debug_raw(" started, lowered to PASSIVE\n");

    // Начальная задержка для распределения выводов во времени
    // Каждый поток стартует с задержкой (thread_num - 1) * 100ms
    if thread_num > 1 {
        crate::ke::debug::debug_raw("[PS TEST] Thread ");
        crate::ke::debug::debug_dec(thread_num as u64);
        crate::ke::debug::debug_raw(" doing stagger delay\n");
        let stagger_delay = PS_TEST_STAGGER_INTERVAL * (thread_num as i64 - 1);
        crate::ke::wait::ke_delay_execution_thread(0, false, stagger_delay);
        crate::ke::debug::debug_raw("[PS TEST] Thread ");
        crate::ke::debug::debug_dec(thread_num as u64);
        crate::ke::debug::debug_raw(" stagger delay done\n");
    }

    let mut iteration: u64 = 0;

    crate::ke::debug::debug_raw("[PS TEST] Thread ");
    crate::ke::debug::debug_dec(thread_num as u64);
    crate::ke::debug::debug_raw(" entering main loop\n");

    loop {
        iteration += 1;

        crate::ke::debug::debug_raw("[PS TEST] Thread ");
        crate::ke::debug::debug_dec(thread_num as u64);
        crate::ke::debug::debug_raw(" iter=");
        crate::ke::debug::debug_dec(iteration);
        crate::ke::debug::debug_raw(" START\n");

        crate::ke::debug::debug_raw("[PS TEST] Thread ");
        crate::ke::debug::debug_dec(thread_num as u64);
        crate::ke::debug::debug_raw(" getting current thread\n");

        // Получаем информацию о текущем потоке через ETHREAD
        let thread = crate::ps::ps_get_current_thread() as *mut crate::ps::ETHREAD;
        
        crate::ke::debug::debug_raw("[PS TEST] Thread ");
        crate::ke::debug::debug_dec(thread_num as u64);
        crate::ke::debug::debug_raw(" got thread ptr=");
        crate::ke::debug::debug_hex(thread as u64);
        crate::ke::debug::debug_raw("\n");

        let kthread = &(*thread).tcb;

        let thread_id = (*thread).cid.unique_thread as u64;
        let process_id = (*thread).cid.unique_process as u64;
        let priority = kthread.priority;
        let base_priority = kthread.base_priority;
        let quantum = kthread.quantum;

        // Выводим информацию через debugcon (без блокировок!)
        crate::ke::debug::debug_raw("[PS] T");
        crate::ke::debug::debug_dec(thread_num as u64);
        crate::ke::debug::debug_raw(" iter=");
        crate::ke::debug::debug_dec(iteration);
        crate::ke::debug::debug_raw(" TID=");
        crate::ke::debug::debug_dec(thread_id);
        crate::ke::debug::debug_raw(" pri=");
        crate::ke::debug::debug_dec(priority as u64);
        crate::ke::debug::debug_raw("/");
        crate::ke::debug::debug_dec(base_priority as u64);
        crate::ke::debug::debug_raw(" q=");
        crate::ke::debug::debug_dec(quantum as u64);
        crate::ke::debug::debug_raw("\n");

        // Принудительный BSOD для тестирования аварийного пути (через пару секунд работы).
        // Делаем это только в одном потоке, чтобы не устраивать "гонку bugcheck".
        if thread_num == PS_TEST_BSOD_THREAD_NUM && iteration == PS_TEST_BSOD_AFTER_ITERATIONS {
            crate::ke::debug::debug_raw("[PS TEST] Forcing BSOD (MANUALLY_INITIATED_CRASH) from T");
            crate::ke::debug::debug_dec(thread_num as u64);
            crate::ke::debug::debug_raw("...\n");

            // KeBugCheckEx(MANUALLY_INITIATED_CRASH, p1..p4)
            // Param1..4: best-effort диагностические значения без аллокаций и сложной логики.
            crate::ke::bugcheck::ke_bug_check_ex(
                crate::ke::bugcheck::bugcheck_codes::MANUALLY_INITIATED_CRASH,
                thread_num as crate::nt::ULONG_PTR,
                iteration as crate::nt::ULONG_PTR,
                thread_id as crate::nt::ULONG_PTR,
                process_id as crate::nt::ULONG_PTR,
            );
        }

        crate::ke::debug::debug_raw("[PS TEST] Thread ");
        crate::ke::debug::debug_dec(thread_num as u64);
        crate::ke::debug::debug_raw(" before delay\n");

        // Задержка между выводами
        crate::ke::wait::ke_delay_execution_thread(
            0,     // KernelMode
            false, // not alertable
            PS_TEST_DELAY_INTERVAL,
        );

        crate::ke::debug::debug_raw("[PS TEST] Thread ");
        crate::ke::debug::debug_dec(thread_num as u64);
        crate::ke::debug::debug_raw(" after delay\n");
    }
}

// =============================================================================
// Запуск тестовых потоков
// =============================================================================

/// Запускает тестовые потоки PS
///
/// Вызывается из phase1_initialization после полной инициализации PS подсистемы.
/// Создаёт PS_TEST_THREAD_COUNT потоков с разными приоритетами.
pub unsafe fn ps_start_test_threads() {
    use crate::ke::priority::ke_set_priority_thread;
    use crate::nt::STATUS_SUCCESS;
    use crate::ps::ps_create_system_thread;

    dbg_print("[PS TEST] Starting ");
    dbg_print_num(PS_TEST_THREAD_COUNT as u64);
    dbg_print(" test threads with different priorities...\n");

    // Выводим таблицу приоритетов
    dbg_print("[PS TEST] Priority map: ");
    for i in 0..PS_TEST_THREAD_COUNT {
        if i > 0 {
            dbg_print(", ");
        }
        dbg_print("T");
        dbg_print_num((i + 1) as u64);
        dbg_print("=");
        dbg_print_num(PS_TEST_PRIORITIES[i] as u64);
    }
    dbg_print("\n");

    let mut success_count = 0u32;
    let mut fail_count = 0u32;

    // Массив для хранения указателей на потоки (для установки приоритета)
    let mut thread_handles: [*mut crate::ke::thread::KTHREAD; PS_TEST_THREAD_COUNT] =
        [core::ptr::null_mut(); PS_TEST_THREAD_COUNT];

    for i in 0..PS_TEST_THREAD_COUNT {
        let thread_num = i + 1;
        let mut client_id = crate::ps::CLIENT_ID {
            unique_process: core::ptr::null_mut(),
            unique_thread: core::ptr::null_mut(),
        };

        let status = ps_create_system_thread(
            core::ptr::null_mut(), // thread_handle
            0,                     // desired_access
            core::ptr::null_mut(), // object_attributes
            0,                     // process_handle (System Process)
            &mut client_id,        // client_id - получаем TID
            Some(ps_test_thread_worker),
            thread_num as PVOID, // start_context = номер потока
        );

        if status == STATUS_SUCCESS {
            success_count += 1;

            // Получаем указатель на поток через CID
            let tid = client_id.unique_thread as usize;
            let mut thread_ptr: *mut crate::ps::ETHREAD = core::ptr::null_mut();
            let lookup_status = crate::ps::cid::ps_lookup_thread_by_thread_id(tid, &mut thread_ptr);

            if lookup_status == STATUS_SUCCESS && !thread_ptr.is_null() {
                let kthread = &raw mut (*thread_ptr).tcb;
                thread_handles[i] = kthread;

                // Устанавливаем приоритет
                let target_priority = PS_TEST_PRIORITIES[i];
                let old_priority = ke_set_priority_thread(kthread, target_priority);

                dbg_print("[PS TEST] Thread ");
                dbg_print_num(thread_num as u64);
                dbg_print(" created (TID=");
                dbg_print_num(tid as u64);
                dbg_print("), priority ");
                dbg_print_num(old_priority as u64);
                dbg_print(" -> ");
                dbg_print_num(target_priority as u64);
                dbg_print("\n");
            }
        } else {
            fail_count += 1;
            dbg_print("[PS TEST] Thread ");
            dbg_print_num(thread_num as u64);
            dbg_print(" creation failed, status=0x");
            dbg_print_hex(status as u64);
            dbg_print("\n");
        }
    }

    dbg_print("[PS TEST] Created ");
    dbg_print_num(success_count as u64);
    dbg_print("/");
    dbg_print_num(PS_TEST_THREAD_COUNT as u64);
    dbg_print(" threads");
    if fail_count > 0 {
        dbg_print(" (");
        dbg_print_num(fail_count as u64);
        dbg_print(" failed)");
    }
    dbg_print("\n");
    dbg_print("[PS TEST] Higher priority threads should run more frequently!\n");
}
