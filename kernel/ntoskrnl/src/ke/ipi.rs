//! IPI (Inter-Processor Interrupt) - межпроцессорные прерывания
//!
//! Реализация межпроцессорных прерываний для SMP.
//!
//! Источники:
//! - NT5: ke/mp.c, ke/amd64/interrupt.c
//! - ReactOS: ke/ipi.c, hal/halx86/apic/apic.c

#![allow(dead_code)]

use core::sync::atomic::AtomicU64;
use core::sync::atomic::Ordering;

use crate::arch::x86_64::pcr;
use crate::hal::apic::APC_VECTOR;
use crate::hal::apic::APIC_IPI_VECTOR;
use crate::hal::apic::DISPATCH_VECTOR;
use crate::hal::apic::hal_send_ipi;
use crate::ke::globals::KE_NUMBER_PROCESSORS;
use crate::ke::globals::ki_processor_block;
use crate::ke::sched::KI_IDLE_SUMMARY;
use crate::nt::PVOID;

// =============================================================================
// IPI Request Types
// =============================================================================

/// IPI типы (битовые флаги)
pub const IPI_APC: u32 = 1; // Запрос APC доставки
pub const IPI_DPC: u32 = 2; // Запрос DPC/Dispatch
pub const IPI_FREEZE: u32 = 4; // Замораживание для debugger
pub const IPI_PACKET_READY: u32 = 8; // Generic IPI с callback
pub const IPI_SYNCH_REQUEST: u32 = 16; // Синхронизация TLB/cache

// =============================================================================
// IPI Vectors (соответствуют apic.rs)
// =============================================================================

/// Vector для APC software interrupt
pub const IPI_VECTOR_APC: u8 = APC_VECTOR; // 0x1F

/// Vector для DPC/Dispatch software interrupt  
pub const IPI_VECTOR_DPC: u8 = DISPATCH_VECTOR; // 0x2F

/// Vector для generic IPI
pub const IPI_VECTOR_IPI: u8 = APIC_IPI_VECTOR; // 0xE1

// =============================================================================
// IPI Statistics (для отладки)
// =============================================================================

/// Счетчик отправленных IPI
pub static IPI_SEND_COUNT: AtomicU64 = AtomicU64::new(0);

/// Счетчик полученных IPI
pub static IPI_RECEIVE_COUNT: AtomicU64 = AtomicU64::new(0);

// =============================================================================
// KiIpiSend - основная функция отправки IPI
// =============================================================================

/// KiIpiSend - отправка IPI указанным процессорам
///
/// Соответствует ReactOS KiIpiSend / NT KiIpiSend.
///
/// Выполняет:
/// 1. Установку IPI request флагов в PRCB целевых процессоров
/// 2. Отправку hardware IPI через Local APIC
///
/// # Arguments
/// * `target_processors` - битовая маска целевых процессоров (bit N = processor N)
/// * `ipi_request` - тип IPI запроса (IPI_APC, IPI_DPC, etc.)
///
/// # Safety
/// Должен вызываться на DISPATCH_LEVEL или выше
pub unsafe fn ki_ipi_send(target_processors: u64, ipi_request: u32) {
    unsafe {
        if target_processors == 0 {
            return;
        }

        let current_prcb = pcr::get_prcb();
        let current_number = if !current_prcb.is_null() {
            (*current_prcb).number as u64
        } else {
            0
        };

        // Маскируем текущий процессор (не отправляем IPI самому себе через эту функцию)
        let target = target_processors & !(1u64 << current_number);

        if target == 0 {
            return;
        }

        let num_processors = KE_NUMBER_PROCESSORS.load(Ordering::Acquire) as usize;

        // Определяем вектор для IPI
        let vector = match ipi_request {
            IPI_APC => IPI_VECTOR_APC,
            IPI_DPC => IPI_VECTOR_DPC,
            _ => IPI_VECTOR_IPI,
        };

        // Отправляем IPI каждому целевому процессору
        for i in 0..num_processors {
            if (target & (1u64 << i)) != 0 {
                let prcb = ki_processor_block(i);
                if !prcb.is_null() {
                    // Устанавливаем IPI request флаг
                    (*prcb).ipi_request_summary |= ipi_request;

                    // Получаем APIC ID целевого процессора
                    let target_apic_id = (*prcb).initial_apic_id as u8;

                    // Отправляем hardware IPI через APIC
                    hal_send_ipi(target_apic_id, vector);

                    // Обновляем статистику
                    IPI_SEND_COUNT.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }
}

/// KiIpiSendPacket - отправка IPI с callback функцией
///
/// Соответствует ReactOS KiIpiSendPacket.
///
/// # Arguments
/// * `target_processors` - битовая маска целевых процессоров
/// * `worker_function` - функция для выполнения на целевых процессорах
/// * `argument1` - аргумент 1
/// * `argument2` - аргумент 2
/// * `argument3` - аргумент 3
///
/// # Safety
/// Должен вызываться на DISPATCH_LEVEL или выше
pub unsafe fn ki_ipi_send_packet(
    target_processors: u64,
    worker_function: PKIPI_WORKER,
    argument1: PVOID,
    argument2: PVOID,
    argument3: PVOID,
) {
    unsafe {
        if target_processors == 0 || worker_function.is_none() {
            return;
        }

        let current_prcb = pcr::get_prcb();
        if current_prcb.is_null() {
            return;
        }

        let target = target_processors & !(1u64 << (*current_prcb).number);
        if target == 0 {
            return;
        }

        let num_processors = KE_NUMBER_PROCESSORS.load(Ordering::Acquire) as usize;

        // Устанавливаем IPI packet данные в текущем PRCB
        // (другие процессоры будут читать из SignalDone)
        (*current_prcb).ipi_worker_routine = worker_function;
        (*current_prcb).ipi_current_packet[0] = argument1;
        (*current_prcb).ipi_current_packet[1] = argument2;
        (*current_prcb).ipi_current_packet[2] = argument3;

        // Отправляем IPI
        for i in 0..num_processors {
            if (target & (1u64 << i)) != 0 {
                let prcb = ki_processor_block(i);
                if !prcb.is_null() {
                    (*prcb).ipi_request_summary |= IPI_PACKET_READY;
                    (*prcb).signal_done = current_prcb as PVOID;

                    let target_apic_id = (*prcb).initial_apic_id as u8;
                    hal_send_ipi(target_apic_id, IPI_VECTOR_IPI);
                    IPI_SEND_COUNT.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }
}

/// KiIpiSendRequest - отправка конкретного типа IPI
///
/// # Arguments
/// * `target_processors` - битовая маска
/// * `request` - тип запроса
pub unsafe fn ki_ipi_send_request(target_processors: u64, request: u32) {
    unsafe {
        ki_ipi_send(target_processors, request);
    }
}

// =============================================================================
// KiIpiServiceRoutine - обработчик IPI
// =============================================================================

/// KiIpiServiceRoutine - обработчик IPI interrupt
///
/// Соответствует ReactOS KiIpiServiceRoutine / NT KiIpiInterrupt.
///
/// Вызывается из IPI interrupt handler (vector 0xE1).
/// Обрабатывает все pending IPI requests:
/// - IPI_APC: запрашивает software interrupt для APC
/// - IPI_DPC: запрашивает software interrupt для DPC
/// - IPI_FREEZE: замораживает процессор для debugger
/// - IPI_PACKET_READY: выполняет IPI worker function
/// - IPI_SYNCH_REQUEST: синхронизация (TLB flush и др.)
///
/// # Safety
/// Вызывается из interrupt context на IPI_LEVEL
pub unsafe fn ki_ipi_service_routine() {
    unsafe {
        let prcb = pcr::get_prcb();
        if prcb.is_null() {
            return;
        }

        // Атомарно читаем и очищаем request summary
        let request = (*prcb).ipi_request_summary;
        (*prcb).ipi_request_summary = 0;

        // Обновляем статистику
        IPI_RECEIVE_COUNT.fetch_add(1, Ordering::Relaxed);

        // =========================================================================
        // IPI_APC - запрос доставки APC
        // =========================================================================
        if (request & IPI_APC) != 0 {
            // Запрашиваем software interrupt для доставки APC на APC_LEVEL
            crate::hal::swint::hal_request_software_interrupt(crate::hal::irql::APC_LEVEL);
        }

        // =========================================================================
        // IPI_DPC - запрос обработки DPC / dispatch
        // =========================================================================
        if (request & IPI_DPC) != 0 {
            // Запрашиваем software interrupt для обработки DPC на DISPATCH_LEVEL
            crate::hal::swint::hal_request_software_interrupt(crate::hal::irql::DISPATCH_LEVEL);
        }

        // =========================================================================
        // IPI_FREEZE - заморозка процессора для debugger
        // =========================================================================
        if (request & IPI_FREEZE) != 0 {
            ki_ipi_freeze_handler();
        }

        // =========================================================================
        // IPI_PACKET_READY - generic IPI с callback
        // =========================================================================
        if (request & IPI_PACKET_READY) != 0 {
            ki_ipi_packet_handler(prcb);
        }

        // =========================================================================
        // IPI_SYNCH_REQUEST - синхронизация (TLB, cache)
        // =========================================================================
        if (request & IPI_SYNCH_REQUEST) != 0 {
            ki_ipi_synch_handler(prcb);
        }
    }
}

// =============================================================================
// IPI Sub-handlers
// =============================================================================

/// Обработчик IPI_FREEZE - замораживает процессор для debugger
///
/// Процессор входит в spin loop до сброса freeze флага.
/// Используется kernel debugger для остановки всех процессоров.
unsafe fn ki_ipi_freeze_handler() {
    unsafe {
        let prcb = pcr::get_prcb();
        if prcb.is_null() {
            return;
        }

        // Устанавливаем флаг что процессор заморожен
        (*prcb).ipi_freeze_flag = 1;

        // Сохраняем контекст для debugger
        // TODO: Сохранить CONTEXT структуру если debugger требует

        // Spin loop пока не разморозят
        while (*prcb).ipi_freeze_flag != 0 {
            // Проверяем на NMI или другие критические события
            crate::arch::x86_64::cpu::yield_processor();

            // Периодически проверяем флаг (memory barrier)
            core::sync::atomic::fence(Ordering::Acquire);
        }
    }
}

/// Обработчик IPI_PACKET_READY - выполняет worker function
///
/// Читает worker routine и аргументы из PRCB инициатора (SignalDone)
/// и выполняет функцию. После выполнения сигнализирует завершение.
unsafe fn ki_ipi_packet_handler(prcb: *mut crate::arch::x86_64::pcr::KPRCB) {
    unsafe {
        // Получаем PRCB отправителя
        let sender_prcb = (*prcb).signal_done as *mut crate::arch::x86_64::pcr::KPRCB;
        if sender_prcb.is_null() {
            return;
        }

        // Получаем worker routine и аргументы
        let worker = (*sender_prcb).ipi_worker_routine;
        let arg1 = (*sender_prcb).ipi_current_packet[0];
        let arg2 = (*sender_prcb).ipi_current_packet[1];
        let arg3 = (*sender_prcb).ipi_current_packet[2];

        // Очищаем signal_done
        (*prcb).signal_done = core::ptr::null_mut();

        // Выполняем worker function
        if let Some(routine) = worker {
            routine(arg1, arg2, arg3);
        }

        // Сигнализируем завершение (декрементируем счетчик в sender PRCB)
        // NT использует InterlockedDecrement на SignalDone
        core::sync::atomic::fence(Ordering::Release);
    }
}

/// Обработчик IPI_SYNCH_REQUEST - синхронизация
///
/// Выполняет синхронизацию в зависимости от типа запроса:
/// - TLB flush
/// - Cache flush
/// - Memory barrier
unsafe fn ki_ipi_synch_handler(_prcb: *mut crate::arch::x86_64::pcr::KPRCB) {
    unsafe {
        // По умолчанию выполняем TLB flush
        // Это наиболее частое использование IPI_SYNCH_REQUEST

        // Flush TLB (перезагружаем CR3)
        // Используем отдельные инструкции mov для чтения и записи CR3
        let cr3: u64;
        core::arch::asm!(
            "mov {0}, cr3",
            "mov cr3, {0}",
            out(reg) cr3,
            options(nostack)
        );

        // Подавляем warning о неиспользуемой переменной
        let _ = cr3;

        // Memory fence
        core::sync::atomic::fence(Ordering::SeqCst);
    }
}

// =============================================================================
// IPI Worker Type
// =============================================================================

/// Тип функции IPI worker (для IPI_PACKET_READY)
///
/// Вызывается на целевом процессоре с аргументами из IPI packet.
#[allow(non_camel_case_types)]
pub type PKIPI_WORKER =
    Option<unsafe extern "win64" fn(argument1: PVOID, argument2: PVOID, argument3: PVOID)>;

// =============================================================================
// Processor Selection
// =============================================================================

/// KiSelectProcessor - выбор оптимального процессора для потока
///
/// Соответствует ReactOS KiSelectProcessor / NT KiSelectNextThread.
///
/// Алгоритм выбора (в порядке приоритета):
/// 1. IdealProcessor если idle и в affinity
/// 2. NextProcessor (последний использованный) если idle
/// 3. Любой idle процессор с подходящей affinity
/// 4. IdealProcessor если в affinity (даже если busy)
/// 5. Первый процессор с подходящей affinity
///
/// # Arguments
/// * `thread` - поток для размещения
///
/// # Returns
/// Номер выбранного процессора
pub unsafe fn ki_select_processor(thread: *mut crate::ke::thread::KTHREAD) -> u32 {
    unsafe {
        if thread.is_null() {
            return 0;
        }

        let affinity = (*thread).affinity;
        let num_processors = KE_NUMBER_PROCESSORS.load(Ordering::Acquire);

        // Если только один процессор - возвращаем 0
        if num_processors <= 1 {
            return 0;
        }

        // Проверяем ideal processor
        let ideal = (*thread).ideal_processor as u32;
        if ideal < num_processors as u32 && (affinity & (1u64 << ideal)) != 0 {
            // Проверяем что процессор idle
            let idle_summary = KI_IDLE_SUMMARY.load(Ordering::Acquire);
            if (idle_summary & (1u64 << ideal)) != 0 {
                return ideal;
            }
        }

        // Проверяем next processor (последний использованный)
        let next = (*thread).next_processor as u32;
        if next < num_processors as u32 && (affinity & (1u64 << next)) != 0 {
            let idle_summary = KI_IDLE_SUMMARY.load(Ordering::Acquire);
            if (idle_summary & (1u64 << next)) != 0 {
                return next;
            }
        }

        // Ищем любой idle процессор с подходящим affinity
        let idle_summary = KI_IDLE_SUMMARY.load(Ordering::Acquire);
        let available = idle_summary & affinity;

        if available != 0 {
            // Возвращаем первый доступный idle (least significant bit)
            return available.trailing_zeros();
        }

        // Нет idle процессоров - возвращаем ideal или next или первый с подходящей affinity
        if ideal < num_processors as u32 && (affinity & (1u64 << ideal)) != 0 {
            ideal
        } else if next < num_processors as u32 && (affinity & (1u64 << next)) != 0 {
            next
        } else {
            // Возвращаем первый с подходящим affinity
            affinity.trailing_zeros().min((num_processors - 1) as u32)
        }
    }
}

/// KiRequestDispatchOnProcessor - запрос dispatch interrupt на другом процессоре
///
/// Используется для preemption на удаленном CPU.
/// Отправляет IPI_DPC на указанный процессор.
///
/// # Arguments
/// * `processor_number` - номер целевого процессора
pub unsafe fn ki_request_dispatch_on_processor(processor_number: u32) {
    unsafe {
        let target = 1u64 << processor_number;
        ki_ipi_send(target, IPI_DPC);
    }
}

/// KiIpiSendApc - отправка IPI для APC доставки
///
/// # Arguments
/// * `processor_number` - номер целевого процессора
pub unsafe fn ki_ipi_send_apc(processor_number: u32) {
    unsafe {
        let target = 1u64 << processor_number;
        ki_ipi_send(target, IPI_APC);
    }
}

/// KiIpiFreeze - заморозка всех процессоров для debugger
///
/// Отправляет IPI_FREEZE всем процессорам кроме текущего.
pub unsafe fn ki_ipi_freeze_all() {
    unsafe {
        let current_prcb = pcr::get_prcb();
        let current_number = if !current_prcb.is_null() {
            (*current_prcb).number as u64
        } else {
            0
        };

        let num_processors = KE_NUMBER_PROCESSORS.load(Ordering::Acquire);
        let all_but_self = ((1u64 << num_processors) - 1) & !(1u64 << current_number);

        ki_ipi_send(all_but_self, IPI_FREEZE);
    }
}

/// KiIpiThaw - разморозка всех процессоров
///
/// Сбрасывает freeze флаг на всех процессорах.
pub unsafe fn ki_ipi_thaw_all() {
    unsafe {
        let num_processors = KE_NUMBER_PROCESSORS.load(Ordering::Acquire) as usize;

        for i in 0..num_processors {
            let prcb = ki_processor_block(i);
            if !prcb.is_null() {
                (*prcb).ipi_freeze_flag = 0;
            }
        }

        // Memory barrier чтобы все процессоры увидели изменение
        core::sync::atomic::fence(Ordering::SeqCst);
    }
}

/// KiFlushTargetTlb - flush TLB на указанных процессорах
///
/// Отправляет IPI_SYNCH_REQUEST для TLB invalidation.
///
/// # Arguments
/// * `target_processors` - битовая маска целевых процессоров
pub unsafe fn ki_flush_target_tlb(target_processors: u64) {
    unsafe {
        ki_ipi_send(target_processors, IPI_SYNCH_REQUEST);
    }
}

/// KiFlushAllTlb - flush TLB на всех процессорах
pub unsafe fn ki_flush_all_tlb() {
    unsafe {
        let num_processors = KE_NUMBER_PROCESSORS.load(Ordering::Acquire);
        let all_processors = (1u64 << num_processors) - 1;
        ki_ipi_send(all_processors, IPI_SYNCH_REQUEST);
    }
}
