//! Context Switch - переключение контекста потоков
//!
//! Структуры и код для переключения контекста между потоками.
//! Реализация основана на Windows NT 6.1 (Windows 7) и ReactOS.
//!
//! # Архитектура
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────────┐
//! │                        ПЕРЕКЛЮЧЕНИЕ КОНТЕКСТА                               │
//! ├─────────────────────────────────────────────────────────────────────────────┤
//! │                                                                             │
//! │  Планировщик (DISPATCH_LEVEL)                                               │
//! │       │                                                                     │
//! │       ▼                                                                     │
//! │  ┌─────────────────┐                                                        │
//! │  │ ki_swap_context │  Rust обертка                                          │
//! │  │   - SwapBusy    │  - Устанавливает swap_busy старого потока              │
//! │  │   - Spin wait   │  - Ожидает освобождения нового потока                  │
//! │  └────────┬────────┘                                                        │
//! │           │                                                                 │
//! │           ▼                                                                 │
//! │  ┌─────────────────────────┐                                                │
//! │  │ ki_swap_context_internal│  ASM функция                                   │
//! │  │   - Save XMM6-XMM15     │  Сохранение non-volatile регистров             │
//! │  │   - Save RBX,RDI,RSI    │  Windows x64 ABI                               │
//! │  │   - Save R12-R15,RBP    │                                                │
//! │  │   - Switch RSP          │  ◄── Точка переключения стеков                 │
//! │  └────────┬────────────────┘                                                │
//! │           │                                                                 │
//! │           ▼                                                                 │
//! │  ┌─────────────────────────┐                                                │
//! │  │ ki_swap_context_resume  │  Rust функция (на новом стеке)                 │
//! │  │   - Update CurrentThread│  PRCB.CurrentThread = NewThread                │
//! │  │   - Update TSS.RSP0     │  Для возврата из user mode                     │
//! │  │   - Save/Restore XState │  FPU/SSE/AVX состояние                         │
//! │  │   - Switch CR3          │  Если процессы разные                          │
//! │  │   - Update GS base      │  MSR_GS_SWAP для TEB                           │
//! │  │   - Clear swap_busy     │  Освобождаем старый поток                      │
//! │  │   - Check APC pending   │                                                │
//! │  └────────┬────────────────┘                                                │
//! │           │                                                                 │
//! │           ▼                                                                 │
//! │  ┌─────────────────────────┐                                                │
//! │  │ Restore registers (ASM) │  Восстановление регистров нового потока        │
//! │  │   - Restore XMM6-XMM15  │                                                │
//! │  │   - Restore GPRs        │                                                │
//! │  │   - RET                 │  Возврат в точку, где поток был прерван        │
//! │  └─────────────────────────┘                                                │
//! │                                                                             │
//! └─────────────────────────────────────────────────────────────────────────────┘
//!
//! ┌─────────────────────────────────────────────────────────────────────────────┐
//! │                        ЗАПУСК НОВОГО ПОТОКА                                 │
//! ├─────────────────────────────────────────────────────────────────────────────┤
//! │                                                                             │
//! │  Стек после ki_initialize_context_thread:                                   │
//! │                                                                             │
//! │  InitialStack ─────────────────────────┐                                    │
//! │                    [XSAVE_AREA]         │ FPU/SSE/AVX состояние              │
//! │                ────────────────────────                                     │
//! │                    KSWITCH_FRAME        │ 256 байт                          │
//! │                      - XMM6-XMM15       │                                   │
//! │                      - RBX,RDI,RSI      │                                   │
//! │                      - R12-R15,RBP      │                                   │
//! │                      - return_address ──┼─► ki_thread_startup               │
//! │                ────────────────────────                                     │
//! │                    KSTART_FRAME         │ 48 байт                           │
//! │                      - P1Home ──────────┼─► StartRoutine                    │
//! │                      - P2Home ──────────┼─► StartContext                    │
//! │                      - P4Home ──────────┼─► SystemRoutine                   │
//! │                      - return_address ──┼─► ki_invalid_system_thread_exit   │
//! │  KernelStack ──────────────────────────┘                                    │
//! │                                                                             │
//! │  При первом переключении на поток:                                          │
//! │  1. ki_swap_context_internal восстанавливает регистры из KSWITCH_FRAME      │
//! │  2. RET переходит на ki_thread_startup                                      │
//! │  3. ki_thread_startup вызывает SystemRoutine(StartRoutine, StartContext)    │
//! │  4. Если SystemRoutine вернулась - RET на ki_invalid_system_thread_exit     │
//! │                                                                             │
//! └─────────────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Структуры фреймов
//!
//! - `KSWITCH_FRAME` - фрейм переключения контекста (256 байт, align 16)
//! - `KSTART_FRAME` - параметры запуска потока (48 байт)
//! - `KKINIT_FRAME` - полный фрейм для kernel потока (304 байта)
//! - `KUINIT_FRAME` - полный фрейм для user mode потока (с KTRAP_FRAME)
//!
//! # Отступления и упрощения
//!
//! - XMM6-XMM15 сохраняются в KSWITCH_FRAME для всех потоков (включая kernel).
//!   В NT они сохраняются только в KEXCEPTION_FRAME для user mode.
//!   Это упрощает код, но добавляет 160 байт к каждому context switch.
//!
//! - SwapBusy реализован как простой spin-wait без backoff.
//!   В NT используется более сложная схема с exponential backoff.
//!
//! # ABI контракт (смещения для ASM)
//!
//! ASM код находится в `arch/x86_64/asm/ctxswitch.S`.
//! Смещения структур зафиксированы как ABI контракт и проверяются
//! compile-time assertions. При изменении структур:
//! 1. Сборка упадёт с ошибкой если смещение изменилось
//! 2. Нужно обновить константы в ctxswitch.S
//! 3. Обновить assertions если изменение намеренное
//!
//! Источники:
//! - ReactOS: ke/amd64/ctxswitch.S, ke/amd64/thrdini.c
//! - NT5: ke/amd64/ctxswap.asm, ke/amd64/thredini.c
//! - MSDN: Windows Internals, Thread Scheduling

#![allow(dead_code)]
#![allow(non_camel_case_types)]

use crate::hal::irql::DISPATCH_LEVEL;
use crate::hal::irql::ke_get_current_irql;
use crate::ke::bugcheck::bugcheck_codes;
use crate::ke::bugcheck::ke_bug_check_ex;
use crate::ke::thread::KTHREAD;
use crate::ke::thread::KTHREAD_STATE;
use crate::nt::PVOID;
use crate::ps::process::KPROCESS;

// =============================================================================
// Константы
// =============================================================================

/// Начальное значение MXCSR (маскирование всех FP исключений)
pub const INITIAL_MXCSR: u32 = 0x1F80;

/// Начальное значение FPU Control Word
pub const INITIAL_FPCSR: u16 = 0x027F;

/// APC_LEVEL для IRQL
pub const APC_LEVEL: u8 = 1;

/// MSR для GS base swap (user mode GS)
pub const MSR_GS_SWAP: u32 = 0xC0000102;

// =============================================================================
// Static Assertions - проверка смещений в структурах
// =============================================================================
//
// Эти проверки гарантируют, что ASM константы соответствуют реальным смещениям.
// При изменении структур компиляция завершится ошибкой если смещения не совпадут.

/// Смещения в KTHREAD (для ASM)
pub mod kthread_offsets {
    use core::mem::offset_of;

    use super::*;

    pub const KERNEL_STACK: usize = offset_of!(KTHREAD, kernel_stack);
    pub const INITIAL_STACK: usize = offset_of!(KTHREAD, initial_stack);
    pub const STATE: usize = offset_of!(KTHREAD, state);
    pub const APC_STATE: usize = offset_of!(KTHREAD, apc_state);
    pub const PROCESS: usize = offset_of!(KTHREAD, process);
    pub const CONTEXT_SWITCHES: usize = offset_of!(KTHREAD, context_switches);
    pub const TEB: usize = offset_of!(KTHREAD, teb);
    pub const SWAP_BUSY: usize = offset_of!(KTHREAD, swap_busy);
    pub const NPX_STATE: usize = offset_of!(KTHREAD, npx_state);
    pub const STATE_SAVE_AREA: usize = offset_of!(KTHREAD, state_save_area);
    pub const PREVIOUS_MODE: usize = offset_of!(KTHREAD, previous_mode);
    pub const SPECIAL_APC_DISABLE: usize = offset_of!(KTHREAD, special_apc_disable);

    // Проверяем критичные смещения (используются в ASM)
    const _: () = assert!(KERNEL_STACK == 0x38, "KTHREAD.kernel_stack offset changed!");
    const _: () = assert!(
        INITIAL_STACK == 0x28,
        "KTHREAD.initial_stack offset changed!"
    );
    const _: () = assert!(STATE == 0x78, "KTHREAD.state offset changed!");
}

/// Смещения в KPCR (для GS-relative доступа)
pub mod kpcr_offsets {
    use core::mem::offset_of;

    use super::super::pcr::KPCR;

    pub const IRQL: usize = offset_of!(KPCR, irql);
    pub const PRCB: usize = offset_of!(KPCR, prcb);
    pub const TSS_BASE: usize = offset_of!(KPCR, tss_base);

    // Проверяем смещение IRQL (используется в ASM для gs:[PcrIrql])
    const _: () = assert!(IRQL == 0x70, "KPCR.irql offset changed!");
}

/// Смещения в KPRCB
pub mod kprcb_offsets {
    use core::mem::offset_of;

    use super::super::pcr::KPRCB;

    pub const CURRENT_THREAD: usize = offset_of!(KPRCB, current_thread);
    pub const NEXT_THREAD: usize = offset_of!(KPRCB, next_thread);
    pub const IDLE_THREAD: usize = offset_of!(KPRCB, idle_thread);
    pub const CONTEXT_SWITCHES: usize = offset_of!(KPRCB, context_switches);
    pub const DPC_ROUTINE_ACTIVE: usize = offset_of!(KPRCB, dpc_routine_active);
    pub const RSP_BASE: usize = offset_of!(KPRCB, rsp_base);
}

/// Смещения в KPROCESS
pub mod kprocess_offsets {
    use core::mem::offset_of;

    use super::*;

    pub const DIRECTORY_TABLE_BASE: usize = offset_of!(KPROCESS, directory_table_base);
}

/// Смещения в KAPC_STATE
pub mod kapc_state_offsets {
    use core::mem::offset_of;

    use crate::ke::thread::KAPC_STATE;

    pub const PROCESS: usize = offset_of!(KAPC_STATE, process);
    pub const KERNEL_APC_PENDING: usize = offset_of!(KAPC_STATE, kernel_apc_pending);
}

// =============================================================================
// KSWITCH_FRAME - фрейм переключения контекста
// =============================================================================

/// KSWITCH_FRAME - фрейм на стеке при переключении контекста
///
/// Этот фрейм создается на стеке при вызове KiSwapContextInternal.
/// Содержит все non-volatile регистры по Windows x64 ABI.
///
/// # Layout на стеке (от RSP к высоким адресам)
///
/// ```text
/// Offset  Size  Содержимое
/// ------  ----  ----------
/// 0x00    16    XMM6
/// 0x10    16    XMM7
/// 0x20    16    XMM8
/// 0x30    16    XMM9
/// 0x40    16    XMM10
/// 0x50    16    XMM11
/// 0x60    16    XMM12
/// 0x70    16    XMM13
/// 0x80    16    XMM14
/// 0x90    16    XMM15
/// 0xA0    4     MXCSR
/// 0xA4    4     MxcsrPad (выравнивание)
/// 0xA8    1     ApcBypass
/// 0xA9    1     NpxSave
/// 0xAA    6     Fill (выравнивание)
/// 0xB0    8     RBX
/// 0xB8    8     RDI
/// 0xC0    8     RSI
/// 0xC8    8     R12
/// 0xD0    8     R13
/// 0xD8    8     R14
/// 0xE0    8     R15
/// 0xE8    8     Padding
/// 0xF0    8     RBP
/// 0xF8    8     Return Address
/// ------  ----
/// Total: 256 байт (0x100)
/// ```
///
/// Размер 256 байт выбран для выравнивания на 16 байт (требование movaps).
#[repr(C, align(16))]
#[derive(Clone, Copy)]
pub struct KSWITCH_FRAME {
    // XMM non-volatile регистры (Windows x64 ABI: XMM6-XMM15)
    pub xmm6: [u8; 16],  // 0x00
    pub xmm7: [u8; 16],  // 0x10
    pub xmm8: [u8; 16],  // 0x20
    pub xmm9: [u8; 16],  // 0x30
    pub xmm10: [u8; 16], // 0x40
    pub xmm11: [u8; 16], // 0x50
    pub xmm12: [u8; 16], // 0x60
    pub xmm13: [u8; 16], // 0x70
    pub xmm14: [u8; 16], // 0x80
    pub xmm15: [u8; 16], // 0x90

    /// MXCSR регистр (SSE control/status)
    pub mxcsr: u32, // 0xA0
    /// Padding для выравнивания
    pub mxcsr_pad: u32, // 0xA4

    /// ApcBypass - IRQL при котором поток был приостановлен
    /// Используется для определения нужна ли доставка APC при resume
    pub apc_bypass: u8, // 0xA8
    /// NpxSave - нужно ли сохранять FPU state
    pub npx_save: u8, // 0xA9
    /// Fill для выравнивания на 8
    pub fill1: [u8; 6], // 0xAA

    // Integer non-volatile регистры (Windows x64 ABI)
    pub rbx: u64, // 0xB0
    pub rdi: u64, // 0xB8
    pub rsi: u64, // 0xC0
    pub r12: u64, // 0xC8
    pub r13: u64, // 0xD0
    pub r14: u64, // 0xD8
    pub r15: u64, // 0xE0

    /// Padding для правильного размещения RBP/Return
    pub padding: u64, // 0xE8

    /// Сохраненный RBP (non-volatile)
    pub rbp: u64, // 0xF0

    /// Адрес возврата (куда вернется поток после resume)
    pub return_address: u64, // 0xF8
}

impl KSWITCH_FRAME {
    pub const SIZE: usize = core::mem::size_of::<Self>();

    // Смещения для ASM кода
    pub const XMM6_OFFSET: usize = 0x00;
    pub const XMM15_OFFSET: usize = 0x90;
    pub const MXCSR_OFFSET: usize = 0xA0;
    pub const APC_BYPASS_OFFSET: usize = 0xA8;
    pub const RBX_OFFSET: usize = 0xB0;
    pub const RBP_OFFSET: usize = 0xF0;
    pub const RETURN_OFFSET: usize = 0xF8;
}

// Проверяем размер структуры (256 байт из-за align(16))
const _: () = assert!(
    KSWITCH_FRAME::SIZE == 256,
    "KSWITCH_FRAME size must be 256 bytes"
);
const _: () = assert!(
    KSWITCH_FRAME::RETURN_OFFSET == 0xF8,
    "KSWITCH_FRAME return offset wrong"
);

// =============================================================================
// KSTART_FRAME - начальный фрейм потока
// =============================================================================

/// KSTART_FRAME - начальный фрейм для новых потоков
///
/// Содержит параметры для KiThreadStartup. Располагается на стеке
/// сразу после KSWITCH_FRAME.
///
/// # Layout
///
/// ```text
/// Offset  Size  Содержимое
/// ------  ----  ----------
/// 0x00    8     P1Home - StartRoutine (для kernel threads)
/// 0x08    8     P2Home - StartContext
/// 0x10    8     P3Home - не используется
/// 0x18    8     P4Home - SystemRoutine
/// 0x20    8     Reserved
/// 0x28    8     Return Address (ki_invalid_system_thread_startup_exit)
/// ------  ----
/// Total: 48 байт (0x30)
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct KSTART_FRAME {
    /// StartRoutine - пользовательская функция потока
    pub p1_home: u64, // 0x00
    /// StartContext - контекст для StartRoutine
    pub p2_home: u64, // 0x08
    /// Не используется
    pub p3_home: u64, // 0x10
    /// SystemRoutine - системная обертка (psp_system_thread_startup)
    pub p4_home: u64, // 0x18
    /// Reserved
    pub reserved: u64, // 0x20
    /// Адрес возврата (если SystemRoutine вернулась - это баг)
    pub return_address: u64, // 0x28
}

impl KSTART_FRAME {
    pub const SIZE: usize = core::mem::size_of::<Self>();

    pub const P1_HOME_OFFSET: usize = 0x00;
    pub const P2_HOME_OFFSET: usize = 0x08;
    pub const P4_HOME_OFFSET: usize = 0x18;
    pub const RETURN_OFFSET: usize = 0x28;
}

const _: () = assert!(
    KSTART_FRAME::SIZE == 48,
    "KSTART_FRAME size must be 48 bytes"
);

// =============================================================================
// KKINIT_FRAME - полный начальный фрейм для kernel потока
// =============================================================================

/// KKINIT_FRAME - полный начальный фрейм для kernel потока
///
/// Объединяет KSWITCH_FRAME и KSTART_FRAME.
/// Размер: 256 + 48 = 304 байта
#[repr(C)]
#[derive(Clone, Copy)]
pub struct KKINIT_FRAME {
    /// Фрейм переключения контекста
    pub ctx_switch_frame: KSWITCH_FRAME,
    /// Начальный фрейм с параметрами
    pub start_frame: KSTART_FRAME,
}

impl KKINIT_FRAME {
    pub const SIZE: usize = core::mem::size_of::<Self>();
}

const _: () = assert!(
    KKINIT_FRAME::SIZE == 304,
    "KKINIT_FRAME size must be 304 bytes"
);

// =============================================================================
// KUINIT_FRAME - полный начальный фрейм для user mode потока
// =============================================================================

/// KUINIT_FRAME - полный начальный фрейм для user mode потока
///
/// Содержит все необходимые фреймы для инициализации контекста
/// и последующего возврата в user mode через IRET.
///
/// # Layout стека (от вершины к основанию)
///
/// ```text
/// [KSWITCH_FRAME]     256 байт - для context switch
/// [KSTART_FRAME]       48 байт - параметры startup
/// [KEXCEPTION_FRAME]  320 байт - non-volatile регистры
/// [KTRAP_FRAME]       400 байт - полный контекст для IRET
/// ```
#[repr(C)]
#[derive(Clone, Copy)]
pub struct KUINIT_FRAME {
    /// Фрейм переключения контекста
    pub ctx_switch_frame: KSWITCH_FRAME,
    /// Начальный фрейм
    pub start_frame: KSTART_FRAME,
    /// Exception frame (non-volatile регистры)
    pub exception_frame: crate::ke::trap::KEXCEPTION_FRAME,
    /// Trap frame (полный контекст для IRET)
    pub trap_frame: crate::ke::trap::KTRAP_FRAME,
}

impl KUINIT_FRAME {
    pub const SIZE: usize = core::mem::size_of::<Self>();

    pub const CTX_SWITCH_FRAME_OFFSET: usize = 0;
    pub const START_FRAME_OFFSET: usize = KSWITCH_FRAME::SIZE;
    pub const EXCEPTION_FRAME_OFFSET: usize = KSWITCH_FRAME::SIZE + KSTART_FRAME::SIZE;
    pub const TRAP_FRAME_OFFSET: usize =
        KSWITCH_FRAME::SIZE + KSTART_FRAME::SIZE + crate::ke::trap::KEXCEPTION_FRAME_LENGTH;
}

// =============================================================================
// Типы функций (Windows x64 ABI)
// =============================================================================

/// Тип системной рутины для потока
///
/// Windows x64 ABI: параметры через RCX, RDX
pub type PKSYSTEM_ROUTINE =
    unsafe extern "win64" fn(start_routine: Option<PKSTART_ROUTINE>, start_context: PVOID);

/// Тип стартовой рутины потока
pub type PKSTART_ROUTINE = unsafe extern "win64" fn(context: PVOID);

// =============================================================================
// External ASM functions
// =============================================================================

unsafe extern "win64" {
    /// KiSwapContextInternal - низкоуровневое переключение контекста
    ///
    /// # Arguments (Windows x64 ABI)
    /// * `wait_irql` (CL) - IRQL при котором поток приостановлен
    /// * `old_thread` (RDX) - указатель на текущий KTHREAD
    /// * `new_thread` (R8) - указатель на новый KTHREAD
    ///
    /// # Returns
    /// AL = 1 если есть отложенный kernel APC, иначе 0
    fn ki_swap_context_internal(
        wait_irql: u8,
        old_thread: *mut KTHREAD,
        new_thread: *mut KTHREAD,
    ) -> u8;

    /// KiThreadStartup - точка входа для новых потоков
    pub fn ki_thread_startup();

    /// KiInvalidSystemThreadStartupExit - если system thread вернулся (баг!)
    pub fn ki_invalid_system_thread_startup_exit();

    /// KiUserThreadStartupExit - переход в user mode после startup
    pub fn ki_user_thread_startup_exit();
}

// =============================================================================
// KiSwapContext - обертка для переключения контекста
// =============================================================================

/// KiSwapContext - переключение контекста на новый поток
///
/// Вызывается из планировщика для переключения на поток из `PRCB.NextThread`.
///
/// # Arguments
/// * `wait_irql` - IRQL при котором текущий поток приостанавливается
/// * `old_thread` - указатель на текущий поток
///
/// # Returns
/// `true` если есть pending kernel APC для нового потока
///
/// # Safety
/// - Должна вызываться с IRQL >= DISPATCH_LEVEL
/// - `old_thread` должен быть валидным
/// - `PRCB.NextThread` должен быть установлен планировщиком
#[inline]
pub unsafe fn ki_swap_context(wait_irql: u8, old_thread: *mut KTHREAD) -> bool {
    unsafe {
        use core::sync::atomic::Ordering;
        use core::sync::atomic::fence;

        // Получаем следующий поток из PRCB
        let prcb = super::pcr::get_prcb();
        let new_thread = (*prcb).next_thread as *mut KTHREAD;

        crate::ke::debug::debug_raw("[SWAP] KiSwapContext: old=");
        crate::ke::debug::debug_hex(old_thread as u64);
        crate::ke::debug::debug_raw(" new=");
        crate::ke::debug::debug_hex(new_thread as u64);
        crate::ke::debug::debug_raw("\n");

        if new_thread.is_null() {
            crate::ke::debug::debug_raw("[SWAP] No new thread!\n");
            return false;
        }

        // Проверки состояний потоков
        let current_irql = ke_get_current_irql();
        debug_assert!(
            current_irql == DISPATCH_LEVEL,
            "ki_swap_context: IRQL must be DISPATCH_LEVEL"
        );

        debug_assert!(
            !old_thread.is_null(),
            "ki_swap_context: old_thread is NULL"
        );

        if !old_thread.is_null() {
            let from_state = (*old_thread).get_state();
            debug_assert!(
                from_state == KTHREAD_STATE::Running 
                    || from_state == KTHREAD_STATE::Standby
                    || from_state == KTHREAD_STATE::Ready
                    || from_state == KTHREAD_STATE::Waiting,
                "ki_swap_context: old_thread in unexpected state"
            );
        }

        let to_state = (*new_thread).get_state();
        debug_assert!(
            to_state == KTHREAD_STATE::Ready || to_state == KTHREAD_STATE::Standby,
            "ki_swap_context: new_thread not Ready/Standby"
        );

        // Снимаем NextThread перед переключением (NT-паттерн)
        (*prcb).next_thread = core::ptr::null_mut();

        // === SwapBusy протокол ===
        // Защита от одновременного переключения с одним потоком на SMP

        // 1. Устанавливаем swap_busy старого потока
        if !old_thread.is_null() {
            (*old_thread).swap_busy = 1;
            fence(Ordering::SeqCst);
        }

        // 2. Ожидаем освобождения нового потока
        if !new_thread.is_null() {
            while (*new_thread).swap_busy != 0 {
                crate::arch::x86_64::cpu::yield_processor();
            }
        }

        // Вызываем ASM функцию переключения
        let apc_pending = ki_swap_context_internal(wait_irql, old_thread, new_thread);

        apc_pending != 0
    }
}

// =============================================================================
// KiSwapContextResume - завершение переключения контекста
// =============================================================================

/// KiSwapContextResume - завершение переключения контекста (вызывается из ASM)
///
/// Выполняется на стеке нового потока после переключения RSP.
///
/// # Действия
/// 1. Обновление `PRCB.CurrentThread`
/// 2. Обновление состояний потоков (Running/Ready)
/// 3. Обновление `TSS.RSP0` для возврата из user mode
/// 4. Сохранение/восстановление XSAVE состояния (FPU/SSE/AVX)
/// 5. Переключение CR3 при смене процесса
/// 6. Обновление GS base для TEB
/// 7. Инкремент счетчиков context switches
/// 8. Проверка DPC routine active (bugcheck если да)
/// 9. Сброс `swap_busy` старого потока
/// 10. Проверка kernel APC pending
///
/// # Safety
/// Вызывается только из ASM кода ki_swap_context_internal
#[unsafe(no_mangle)]
pub unsafe extern "win64" fn ki_swap_context_resume(
    apc_bypass: u8,
    old_thread: *mut KTHREAD,
    new_thread: *mut KTHREAD,
) -> u8 {
    unsafe {
        use super::pcr::get_pcr;
        use super::xstate::ki_restore_x_state;
        use super::xstate::ki_save_x_state;
        use crate::arch::x86_64::msr;
        use crate::ke::thread::KTHREAD_STATE;


        let pcr = get_pcr();

        // 1. Обновляем CurrentThread
        // ВАЖНО: делаем это ПОСЛЕ смены стека для атомарности
        (*pcr).prcb.current_thread = new_thread as PVOID;

        // 2. Обновляем состояния потоков
        (*new_thread).set_state(KTHREAD_STATE::Running);

        if !old_thread.is_null() && (*old_thread).get_state() == KTHREAD_STATE::Running {
            // Если старый поток все еще Running - переводим в Ready
            // (Waiting/Terminated устанавливаются до вызова KiSwapContext)
            (*old_thread).set_state(KTHREAD_STATE::Ready);
        }

        // 3. Обновляем TSS.RSP0 для нового потока
        // При переходе из user mode в kernel mode CPU использует RSP0 из TSS
        if !(*pcr).tss_base.is_null() {
            (*(*pcr).tss_base).rsp0 = (*new_thread).initial_stack as u64;
            (*pcr).prcb.rsp_base = (*(*pcr).tss_base).rsp0;
        }

        // 4. Сохраняем XSAVE состояние старого потока
        if !old_thread.is_null()
            && (*old_thread).npx_state != 0
            && !(*old_thread).state_save_area.is_null()
        {
            ki_save_x_state((*old_thread).state_save_area, (*old_thread).npx_state);
        }

        // 5. Восстанавливаем XSAVE состояние нового потока
        if (*new_thread).npx_state != 0 && !(*new_thread).state_save_area.is_null() {
            ki_restore_x_state((*new_thread).state_save_area, (*new_thread).npx_state);
        }

        // 6. Переключаем адресное пространство если процессы разные
        if !old_thread.is_null() {
            let old_process = (*old_thread).apc_state.process as *mut KPROCESS;
            let new_process = (*new_thread).apc_state.process as *mut KPROCESS;

            if !old_process.is_null() && !new_process.is_null() && old_process != new_process {
                let new_cr3 = (*new_process).directory_table_base;
                if new_cr3 != 0 {
                    core::arch::asm!(
                        "mov cr3, {}",
                        in(reg) new_cr3,
                        options(nostack)
                    );
                }
            }
        }

        // 7. Обновляем TEB pointer и GS base для user mode потоков
        (*pcr).nt_tib.self_ptr = (*new_thread).teb as *mut super::pcr::NT_TIB;
        if !(*new_thread).teb.is_null() {
            // MSR_GS_SWAP содержит user mode GS base
            // При SWAPGS процессор обменивает GS base с этим MSR
            msr::wrmsr(MSR_GS_SWAP, (*new_thread).teb as u64);
        }

        // 8. Восстанавливаем сегментные регистры для user mode потоков
        if (*new_thread).previous_mode != 0 {
            const USER_DS: u16 = 0x2B; // User data segment selector (RPL=3)
            core::arch::asm!(
                "mov ds, {0:x}",
                "mov es, {0:x}",
                in(reg) USER_DS,
                options(nostack, nomem)
            );
        }

        // 9. Инкрементируем счетчики переключений контекста
        (*pcr).prcb.context_switches = (*pcr).prcb.context_switches.wrapping_add(1);
        (*new_thread).context_switches = (*new_thread).context_switches.wrapping_add(1);

        // 10. Проверяем что не переключаемся из DPC (это баг!)
        if (*pcr).prcb.dpc_routine_active != 0 {
            ke_bug_check_ex(
                bugcheck_codes::ATTEMPTED_SWITCH_FROM_DPC,
                old_thread as usize,
                new_thread as usize,
                if old_thread.is_null() {
                    0
                } else {
                    (*old_thread).initial_stack as usize
                },
                0,
            );
        }

        // 11. Сбрасываем SwapBusy старого потока
        if !old_thread.is_null() {
            (*old_thread).swap_busy = 0;
        }

        // 12. Проверяем kernel APC pending
        if (*new_thread).apc_state.kernel_apc_pending != 0 {
            if (*new_thread).special_apc_disable == 0 && apc_bypass == 0 {
                // Возвращаем TRUE - нужна доставка APC
                return 1;
            }
            // Запрашиваем APC interrupt для доставки позже
            crate::hal::swint::hal_request_software_interrupt(crate::hal::irql::APC_LEVEL);
        }

        0
    }
}

// =============================================================================
// KiInitializeContextThread - инициализация контекста kernel потока
// =============================================================================

/// KiInitializeContextThread - инициализирует стек и контекст нового kernel потока
///
/// Настраивает стек потока так, чтобы при первом переключении контекста
/// управление перешло к `ki_thread_startup`, которая вызовет `SystemRoutine`.
///
/// # Layout стека после инициализации
///
/// ```text
/// InitialStack ─────────────────────────┐
///                   [XSAVE_AREA]         │ (ke_xstate_length() байт, aligned 64)
///               ────────────────────────
///                   KSWITCH_FRAME        │
///                     [0xF8] return ─────┼─► ki_thread_startup
///               ────────────────────────
///                   KSTART_FRAME         │
///                     [0x00] P1Home ─────┼─► StartRoutine
///                     [0x08] P2Home ─────┼─► StartContext
///                     [0x18] P4Home ─────┼─► SystemRoutine
///                     [0x28] return ─────┼─► ki_invalid_system_thread_startup_exit
/// KernelStack ──────────────────────────┘
/// ```
///
/// # Arguments
/// * `thread` - указатель на KTHREAD
/// * `system_routine` - системная обертка (psp_system_thread_startup)
/// * `start_routine` - пользовательская функция потока
/// * `start_context` - контекст для start_routine
///
/// # Safety
/// - `thread` должен иметь выделенный стек (`initial_stack`)
/// - Функция изменяет `kernel_stack` и `initial_stack` потока
pub unsafe fn ki_initialize_context_thread(
    thread: *mut KTHREAD,
    system_routine: PKSYSTEM_ROUTINE,
    start_routine: Option<PKSTART_ROUTINE>,
    start_context: PVOID,
) {
    unsafe {
        use super::xstate::XSAVE_AREA;
        use super::xstate::ke_xstate_length;
        use super::xstate::ki_initialize_x_state;

        if thread.is_null() {
            return;
        }

        let initial_stack = (*thread).initial_stack as usize;

        // Выравниваем стек на 64 байта (требование XSAVE)
        let aligned_stack = initial_stack & !0x3F;

        // Выделяем XSAVE area на стеке
        let xstate_length = ke_xstate_length();
        let xsave_area = (aligned_stack - xstate_length) & !0x3F;

        // Инициализируем XSAVE area
        let save_area_ptr = xsave_area as *mut XSAVE_AREA;
        ki_initialize_x_state(save_area_ptr);
        (*thread).state_save_area = save_area_ptr as PVOID;

        // Обновляем InitialStack (указывает после XSAVE area)
        (*thread).initial_stack = xsave_area as PVOID;

        // Выделяем KKINIT_FRAME на стеке
        let init_frame = (xsave_area - KKINIT_FRAME::SIZE) as *mut KKINIT_FRAME;

        // Обнуляем весь фрейм
        core::ptr::write_bytes(init_frame, 0, 1);

        // Заполняем KSWITCH_FRAME
        (*init_frame).ctx_switch_frame.return_address = ki_thread_startup as *const () as u64;
        (*init_frame).ctx_switch_frame.apc_bypass = 1; // Bypass APC при первом запуске
        (*init_frame).ctx_switch_frame.mxcsr = INITIAL_MXCSR;

        // Заполняем KSTART_FRAME
        (*init_frame).start_frame.p1_home = start_routine.map(|f| f as u64).unwrap_or(0);
        (*init_frame).start_frame.p2_home = start_context as u64;
        (*init_frame).start_frame.p4_home = system_routine as u64;
        (*init_frame).start_frame.return_address =
            ki_invalid_system_thread_startup_exit as *const () as u64;

        // Устанавливаем KernelStack - указывает на начало фрейма
        (*thread).kernel_stack = init_frame as PVOID;

        // Kernel mode поток
        (*thread).previous_mode = 0; // KernelMode
        (*thread).npx_state = 0; // Не сохраняем FPU при переключении для kernel потоков
    }
}

// =============================================================================
// KiInitializeUserContextThread - инициализация контекста user mode потока
// =============================================================================

/// KiInitializeUserContextThread - инициализирует контекст для user mode потока
///
/// Создает KUINIT_FRAME с полным KTRAP_FRAME для возврата в user mode через IRET.
///
/// # Arguments
/// * `thread` - указатель на KTHREAD
/// * `user_rip` - точка входа в user mode
/// * `user_rsp` - стек в user mode
/// * `user_rflags` - начальные RFLAGS
/// * `teb` - указатель на Thread Environment Block
///
/// # Safety
/// - `thread` должен иметь выделенный стек
pub unsafe fn ki_initialize_user_context_thread(
    thread: *mut KTHREAD,
    user_rip: u64,
    user_rsp: u64,
    user_rflags: u64,
    teb: PVOID,
) {
    unsafe {
        use super::xstate::XSAVE_AREA;
        use super::xstate::ke_xstate_length;
        use super::xstate::ki_initialize_x_state;

        if thread.is_null() {
            return;
        }

        let initial_stack = (*thread).initial_stack as usize;
        let aligned_stack = initial_stack & !0x3F;

        // Выделяем XSAVE area
        let xstate_length = ke_xstate_length();
        let xsave_area = (aligned_stack - xstate_length) & !0x3F;

        let save_area_ptr = xsave_area as *mut XSAVE_AREA;
        ki_initialize_x_state(save_area_ptr);
        (*thread).state_save_area = save_area_ptr as PVOID;
        (*thread).initial_stack = xsave_area as PVOID;

        // Выделяем KUINIT_FRAME
        let init_frame = (xsave_area - KUINIT_FRAME::SIZE) as *mut KUINIT_FRAME;
        core::ptr::write_bytes(init_frame, 0, 1);

        // KSWITCH_FRAME
        (*init_frame).ctx_switch_frame.return_address = ki_thread_startup as *const () as u64;
        (*init_frame).ctx_switch_frame.apc_bypass = 1;
        (*init_frame).ctx_switch_frame.mxcsr = INITIAL_MXCSR;

        // KSTART_FRAME - для user mode SystemRoutine не используется
        (*init_frame).start_frame.return_address = ki_user_thread_startup_exit as *const () as u64;

        // KTRAP_FRAME - контекст для IRET в user mode
        let trap_frame = &mut (*init_frame).trap_frame;
        trap_frame.rip = user_rip;
        trap_frame.rsp = user_rsp;
        trap_frame.eflags = user_rflags as u32;

        // User mode сегментные селекторы (Ring 3)
        trap_frame.seg_cs = 0x33; // User code segment
        trap_frame.seg_ss = 0x2B; // User data segment
        trap_frame.seg_ds = 0x2B;
        trap_frame.seg_es = 0x2B;
        trap_frame.gs_base = teb as u64;
        trap_frame.previous_mode = 1; // UserMode
        trap_frame.mxcsr = INITIAL_MXCSR;

        (*thread).kernel_stack = init_frame as PVOID;
        (*thread).previous_mode = 1; // UserMode
        (*thread).teb = teb;
        (*thread).npx_state = 0x3; // XSTATE_MASK_LEGACY (X87 + SSE)
    }
}

// =============================================================================
// PspSystemThreadStartup - системная рутина для kernel потоков
// =============================================================================

/// PspSystemThreadStartup - системная рутина для kernel потоков
///
/// Вызывается из ki_thread_startup. Понижает IRQL до PASSIVE_LEVEL
/// и запускает пользовательскую функцию потока.
/// Если функция вернулась - корректно завершает поток.
///
/// В NT эта функция (PspSystemThreadStartup) отвечает за:
/// 1. Понижение IRQL с APC_LEVEL до PASSIVE_LEVEL
/// 2. Вызов start_routine
/// 3. Завершение потока если start_routine вернулась
///
/// Источники:
/// - NT6.1: ps/psctx.c (PspSystemThreadStartup)
/// - ReactOS: ntoskrnl/ps/psctx.c:30-80
pub unsafe extern "win64" fn psp_system_thread_startup(
    start_routine: Option<PKSTART_ROUTINE>,
    start_context: PVOID,
) {
    unsafe {
        // Понижаем IRQL до PASSIVE_LEVEL (NT way)
        // ki_thread_startup устанавливает APC_LEVEL, здесь опускаем до PASSIVE_LEVEL
        crate::hal::irql::kf_lower_irql(crate::hal::irql::PASSIVE_LEVEL);

        if let Some(routine) = start_routine {
            routine(start_context);
        }

        // Если стартовая рутина вернулась - завершаем поток
        crate::ps::PsTerminateSystemThread(crate::nt::STATUS_SUCCESS);
    }
}

// =============================================================================
// Вспомогательные функции для получения смещений (для отладки)
// =============================================================================

/// Возвращает смещения полей в KTHREAD
pub fn get_kthread_offsets() -> KthreadOffsets {
    KthreadOffsets {
        kernel_stack: kthread_offsets::KERNEL_STACK,
        initial_stack: kthread_offsets::INITIAL_STACK,
        state: kthread_offsets::STATE,
        apc_state: kthread_offsets::APC_STATE,
        process: kthread_offsets::PROCESS,
        context_switches: kthread_offsets::CONTEXT_SWITCHES,
        teb: kthread_offsets::TEB,
    }
}

/// Возвращает смещения полей в KPRCB
pub fn get_kprcb_offsets() -> KprcbOffsets {
    KprcbOffsets {
        current_thread: kprcb_offsets::CURRENT_THREAD,
        next_thread: kprcb_offsets::NEXT_THREAD,
        idle_thread: kprcb_offsets::IDLE_THREAD,
        context_switches: kprcb_offsets::CONTEXT_SWITCHES,
        dpc_routine_active: kprcb_offsets::DPC_ROUTINE_ACTIVE,
    }
}

/// Возвращает смещения полей в KPROCESS
pub fn get_kprocess_offsets() -> KprocessOffsets {
    KprocessOffsets {
        directory_table_base: kprocess_offsets::DIRECTORY_TABLE_BASE,
    }
}

#[derive(Debug, Clone, Copy)]
pub struct KthreadOffsets {
    pub kernel_stack: usize,
    pub initial_stack: usize,
    pub state: usize,
    pub apc_state: usize,
    pub process: usize,
    pub context_switches: usize,
    pub teb: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct KprcbOffsets {
    pub current_thread: usize,
    pub next_thread: usize,
    pub idle_thread: usize,
    pub context_switches: usize,
    pub dpc_routine_active: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct KprocessOffsets {
    pub directory_table_base: usize,
}

// =============================================================================
// ABI Contract Assertions - проверки соответствия смещений для ctxswitch.S
// =============================================================================
//
// Эти проверки гарантируют что константы в ctxswitch.S соответствуют
// реальным смещениям в Rust структурах. При изменении layout сборка упадёт.

/// Проверки ABI для KSWITCH_FRAME (используется в ctxswitch.S)
mod kswitch_frame_abi {
    use core::mem::align_of;
    use core::mem::offset_of;
    use core::mem::size_of;

    use super::KSWITCH_FRAME;

    // Размер и выравнивание
    const _: () = assert!(
        size_of::<KSWITCH_FRAME>() == 0x100,
        "KSWITCH_FRAME size != 256"
    );
    const _: () = assert!(
        align_of::<KSWITCH_FRAME>() == 16,
        "KSWITCH_FRAME align != 16"
    );

    // XMM регистры (SwXmm6..SwXmm15)
    const _: () = assert!(offset_of!(KSWITCH_FRAME, xmm6) == 0x00, "SwXmm6 offset");
    const _: () = assert!(offset_of!(KSWITCH_FRAME, xmm7) == 0x10, "SwXmm7 offset");
    const _: () = assert!(offset_of!(KSWITCH_FRAME, xmm8) == 0x20, "SwXmm8 offset");
    const _: () = assert!(offset_of!(KSWITCH_FRAME, xmm9) == 0x30, "SwXmm9 offset");
    const _: () = assert!(offset_of!(KSWITCH_FRAME, xmm10) == 0x40, "SwXmm10 offset");
    const _: () = assert!(offset_of!(KSWITCH_FRAME, xmm11) == 0x50, "SwXmm11 offset");
    const _: () = assert!(offset_of!(KSWITCH_FRAME, xmm12) == 0x60, "SwXmm12 offset");
    const _: () = assert!(offset_of!(KSWITCH_FRAME, xmm13) == 0x70, "SwXmm13 offset");
    const _: () = assert!(offset_of!(KSWITCH_FRAME, xmm14) == 0x80, "SwXmm14 offset");
    const _: () = assert!(offset_of!(KSWITCH_FRAME, xmm15) == 0x90, "SwXmm15 offset");

    // MXCSR и служебные поля
    const _: () = assert!(offset_of!(KSWITCH_FRAME, mxcsr) == 0xA0, "SwMxCsr offset");
    const _: () = assert!(
        offset_of!(KSWITCH_FRAME, apc_bypass) == 0xA8,
        "SwApcBypass offset"
    );

    // Integer non-volatile регистры
    const _: () = assert!(offset_of!(KSWITCH_FRAME, rbx) == 0xB0, "SwRbx offset");
    const _: () = assert!(offset_of!(KSWITCH_FRAME, rdi) == 0xB8, "SwRdi offset");
    const _: () = assert!(offset_of!(KSWITCH_FRAME, rsi) == 0xC0, "SwRsi offset");
    const _: () = assert!(offset_of!(KSWITCH_FRAME, r12) == 0xC8, "SwR12 offset");
    const _: () = assert!(offset_of!(KSWITCH_FRAME, r13) == 0xD0, "SwR13 offset");
    const _: () = assert!(offset_of!(KSWITCH_FRAME, r14) == 0xD8, "SwR14 offset");
    const _: () = assert!(offset_of!(KSWITCH_FRAME, r15) == 0xE0, "SwR15 offset");

    // RBP и return address
    const _: () = assert!(offset_of!(KSWITCH_FRAME, rbp) == 0xF0, "SwRbp offset");
    const _: () = assert!(
        offset_of!(KSWITCH_FRAME, return_address) == 0xF8,
        "SwReturn offset"
    );
}

/// Проверки ABI для KSTART_FRAME (используется в ctxswitch.S)
mod kstart_frame_abi {
    use core::mem::offset_of;
    use core::mem::size_of;

    use super::KSTART_FRAME;

    // Размер
    const _: () = assert!(size_of::<KSTART_FRAME>() == 0x30, "KSTART_FRAME size != 48");

    // Смещения полей
    const _: () = assert!(offset_of!(KSTART_FRAME, p1_home) == 0x00, "SfP1Home offset");
    const _: () = assert!(offset_of!(KSTART_FRAME, p2_home) == 0x08, "SfP2Home offset");
    const _: () = assert!(offset_of!(KSTART_FRAME, p4_home) == 0x18, "SfP4Home offset");
    const _: () = assert!(
        offset_of!(KSTART_FRAME, return_address) == 0x28,
        "SfReturn offset"
    );
}
