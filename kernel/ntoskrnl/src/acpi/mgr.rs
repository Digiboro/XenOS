//! ACPI Manager — глобальный менеджер ACPI подсистемы
//!
//! Реализует единую точку доступа к AcpiTables, AcpiPlatform и Interpreter.
//! Проектируется как фасад для будущего выноса в отдельный `acpi.dll`.
//!
//! # Фазы инициализации
//!
//! ```text
//!   Uninitialized
//!        │
//!        ▼ acpi_init_early(rsdp)
//!   TablesReady ───────────────────────► KD/serial, HAL timers, MCFG
//!        │
//!        ▼ acpi_init_full()
//!   PlatformReady ─────────────────────► FADT, InterruptModel, FixedRegs
//!        │
//!        ▼ (продолжение acpi_init_full)
//!   AmlReady ──────────────────────────► PnP ACPI enumerator, namespace
//! ```
//!
//! # Синхронизация
//!
//! - Инициализация защищена `ACPI_INIT_LOCK` (spinlock).
//! - Состояние хранится в `ACPI_STATE` (AtomicU32).
//! - Указатели на объекты — `AtomicPtr`, читаются с Acquire после проверки state.

use core::sync::atomic::{AtomicPtr, AtomicU32, Ordering};

use crate::ke::spinlock::KSPIN_LOCK;
use crate::nt::NTSTATUS;
use crate::nt::ntstatus::*;

/// ACPI data is invalid (placeholder, use proper NTSTATUS in production)
const STATUS_ACPI_INVALID_DATA: NTSTATUS = 0xC0140003u32 as NTSTATUS;

use super::platform::AcpiPlatform;
use super::aml::Interpreter;
use super::{AcpiError, AcpiTables, XenAcpiHandler};

// =============================================================================
// Состояния ACPI Manager
// =============================================================================

/// Уровни готовности ACPI
#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum AcpiReadyLevel {
    /// Не инициализирован
    Uninitialized = 0,
    /// AcpiTables построены — можно искать таблицы (SPCR/MCFG/MADT/FADT/HPET)
    TablesReady = 1,
    /// AcpiPlatform построен — доступны FADT/FixedRegs/InterruptModel/PowerProfile
    PlatformReady = 2,
    /// Interpreter построен, DSDT+SSDT загружены, namespace готов
    AmlReady = 3,
}

impl From<u32> for AcpiReadyLevel {
    fn from(v: u32) -> Self {
        match v {
            1 => Self::TablesReady,
            2 => Self::PlatformReady,
            3 => Self::AmlReady,
            _ => Self::Uninitialized,
        }
    }
}

// =============================================================================
// Глобальное состояние
// =============================================================================

/// Текущее состояние ACPI Manager
static ACPI_STATE: AtomicU32 = AtomicU32::new(AcpiReadyLevel::Uninitialized as u32);

/// Spinlock для защиты инициализации
static ACPI_INIT_LOCK: KSPIN_LOCK = KSPIN_LOCK::new();

/// Указатель на глобальный XenAcpiHandler (единственный экземпляр)
static ACPI_HANDLER_PTR: AtomicPtr<XenAcpiHandler> = AtomicPtr::new(core::ptr::null_mut());

/// Указатель на AcpiTables (leak'нутый Box)
static ACPI_TABLES_PTR: AtomicPtr<AcpiTables<XenAcpiHandler>> = AtomicPtr::new(core::ptr::null_mut());

/// Указатель на AcpiPlatform (leak'нутый Box)
static ACPI_PLATFORM_PTR: AtomicPtr<AcpiPlatform<XenAcpiHandler>> = AtomicPtr::new(core::ptr::null_mut());

/// Указатель на Interpreter (leak'нутый Box)
static ACPI_INTERPRETER_PTR: AtomicPtr<Interpreter<XenAcpiHandler>> = AtomicPtr::new(core::ptr::null_mut());

/// RSDP физический адрес (сохраняем для диагностики)
static ACPI_RSDP_PHYS: AtomicPtr<()> = AtomicPtr::new(core::ptr::null_mut());

// =============================================================================
// Публичный API
// =============================================================================

/// Ранняя инициализация ACPI — только таблицы
///
/// Вызывается рано в Phase 1, после того как MM готов к маппингу.
/// После успешного вызова состояние = TablesReady.
///
/// # Arguments
/// * `rsdp_phys` - физический адрес RSDP (из LoaderBlock)
///
/// # Returns
/// * `STATUS_SUCCESS` — таблицы построены
/// * `STATUS_ACPI_INVALID_DATA` — ошибка парсинга RSDP/RSDT
/// * `STATUS_ALREADY_COMPLETE` — уже инициализировано
pub unsafe fn acpi_init_early(rsdp_phys: usize) -> NTSTATUS {
    use crate::kd::dbg_print;
    use crate::ke::spinlock::{ke_acquire_spin_lock, ke_release_spin_lock};

    // Быстрая проверка без лока
    if acpi_is_ready(AcpiReadyLevel::TablesReady) {
        return STATUS_SUCCESS; // Уже готово
    }

    // Захватываем лок для инициализации
    let old_irql = ke_acquire_spin_lock(&ACPI_INIT_LOCK);

    // Повторная проверка под локом
    if acpi_is_ready(AcpiReadyLevel::TablesReady) {
        ke_release_spin_lock(&ACPI_INIT_LOCK, old_irql);
        return STATUS_SUCCESS;
    }

    dbg_print("   [ACPI] acpi_init_early: RSDP=0x");
    crate::kd::dbg_print_hex(rsdp_phys as u64);
    dbg_print("...\n");

    // Сохраняем RSDP
    ACPI_RSDP_PHYS.store(rsdp_phys as *mut (), Ordering::Release);

    // Создаём handler
    let handler = XenAcpiHandler::new();
    let handler_box = alloc::boxed::Box::new(handler);
    let handler_ptr = alloc::boxed::Box::into_raw(handler_box);
    ACPI_HANDLER_PTR.store(handler_ptr, Ordering::Release);

    // Строим AcpiTables
    let tables_result = AcpiTables::from_rsdp(handler, rsdp_phys);

    match tables_result {
        Ok(tables) => {
            let tables_box = alloc::boxed::Box::new(tables);
            let tables_ptr = alloc::boxed::Box::into_raw(tables_box);
            ACPI_TABLES_PTR.store(tables_ptr, Ordering::Release);

            // Переводим состояние в TablesReady
            ACPI_STATE.store(AcpiReadyLevel::TablesReady as u32, Ordering::Release);

            dbg_print("   [ACPI] acpi_init_early: OK (TablesReady)\n");

            ke_release_spin_lock(&ACPI_INIT_LOCK, old_irql);
            STATUS_SUCCESS
        }
        Err(e) => {
            dbg_print("   [ACPI] acpi_init_early: FAILED - ");
            dbg_print(acpi_error_to_str(&e));
            dbg_print("\n");

            ke_release_spin_lock(&ACPI_INIT_LOCK, old_irql);
            STATUS_ACPI_INVALID_DATA
        }
    }
}

/// Полная инициализация ACPI — Platform + Interpreter
///
/// Вызывается позже в Phase 1, перед PnP enumeration.
/// Требует: TablesReady.
/// После успешного вызова состояние = AmlReady.
///
/// # Returns
/// * `STATUS_SUCCESS` — platform и interpreter построены
/// * `STATUS_ACPI_INVALID_DATA` — ошибка построения platform/interpreter
/// * `STATUS_UNSUCCESSFUL` — таблицы не готовы
pub unsafe fn acpi_init_full() -> NTSTATUS {
    use crate::kd::dbg_print;
    use crate::ke::spinlock::{ke_acquire_spin_lock, ke_release_spin_lock};

    // Проверяем что tables готовы
    if !acpi_is_ready(AcpiReadyLevel::TablesReady) {
        dbg_print("   [ACPI] acpi_init_full: TablesReady required\n");
        return STATUS_UNSUCCESSFUL;
    }

    // Быстрая проверка — может уже AmlReady
    if acpi_is_ready(AcpiReadyLevel::AmlReady) {
        return STATUS_SUCCESS;
    }

    let old_irql = ke_acquire_spin_lock(&ACPI_INIT_LOCK);

    // Повторная проверка под локом
    if acpi_is_ready(AcpiReadyLevel::AmlReady) {
        ke_release_spin_lock(&ACPI_INIT_LOCK, old_irql);
        return STATUS_SUCCESS;
    }

    dbg_print("   [ACPI] acpi_init_full: building Platform...\n");

    let tables_ptr = ACPI_TABLES_PTR.load(Ordering::Acquire);
    let handler_ptr = ACPI_HANDLER_PTR.load(Ordering::Acquire);

    if tables_ptr.is_null() || handler_ptr.is_null() {
        ke_release_spin_lock(&ACPI_INIT_LOCK, old_irql);
        return STATUS_UNSUCCESSFUL;
    }

    let tables = &*tables_ptr;
    let handler = (*handler_ptr).clone();

    // Строим AcpiPlatform
    // Примечание: AcpiPlatform::new потребляет tables, поэтому нужно клонировать
    // или передать ownership. Пока делаем через rebuild tables.
    let rsdp_phys = ACPI_RSDP_PHYS.load(Ordering::Acquire) as usize;
    
    // Пересоздаём tables для platform (platform хочет ownership)
    let tables_for_platform = match AcpiTables::from_rsdp(handler.clone(), rsdp_phys) {
        Ok(t) => t,
        Err(e) => {
            dbg_print("   [ACPI] acpi_init_full: failed to rebuild tables - ");
            dbg_print(acpi_error_to_str(&e));
            dbg_print("\n");
            ke_release_spin_lock(&ACPI_INIT_LOCK, old_irql);
            return STATUS_ACPI_INVALID_DATA;
        }
    };

    let platform_result = AcpiPlatform::new(tables_for_platform, handler.clone());

    match platform_result {
        Ok(platform) => {
            let platform_box = alloc::boxed::Box::new(platform);
            let platform_ptr = alloc::boxed::Box::into_raw(platform_box);
            ACPI_PLATFORM_PTR.store(platform_ptr, Ordering::Release);

            ACPI_STATE.store(AcpiReadyLevel::PlatformReady as u32, Ordering::Release);
            dbg_print("   [ACPI] acpi_init_full: PlatformReady\n");

            // Строим Interpreter
            dbg_print("   [ACPI] acpi_init_full: building Interpreter...\n");

            let platform_ref = &*platform_ptr;
            let interpreter_result = Interpreter::new_from_platform(platform_ref);

            match interpreter_result {
                Ok(interpreter) => {
                    let interpreter_box = alloc::boxed::Box::new(interpreter);
                    let interpreter_ptr = alloc::boxed::Box::into_raw(interpreter_box);
                    ACPI_INTERPRETER_PTR.store(interpreter_ptr, Ordering::Release);

                    ACPI_STATE.store(AcpiReadyLevel::AmlReady as u32, Ordering::Release);
                    dbg_print("   [ACPI] acpi_init_full: OK (AmlReady)\n");

                    ke_release_spin_lock(&ACPI_INIT_LOCK, old_irql);
                    STATUS_SUCCESS
                }
                Err(e) => {
                    dbg_print("   [ACPI] acpi_init_full: Interpreter failed - ");
                    dbg_print(acpi_error_to_str(&e));
                    dbg_print("\n");

                    // Platform готов, но Interpreter нет — оставляем PlatformReady
                    ke_release_spin_lock(&ACPI_INIT_LOCK, old_irql);
                    STATUS_ACPI_INVALID_DATA
                }
            }
        }
        Err(e) => {
            dbg_print("   [ACPI] acpi_init_full: Platform failed - ");
            dbg_print(acpi_error_to_str(&e));
            dbg_print("\n");

            ke_release_spin_lock(&ACPI_INIT_LOCK, old_irql);
            STATUS_ACPI_INVALID_DATA
        }
    }
}

/// Проверяет, достигнут ли указанный уровень готовности
#[inline]
pub fn acpi_is_ready(level: AcpiReadyLevel) -> bool {
    let current = ACPI_STATE.load(Ordering::Acquire);
    current >= level as u32
}

/// Возвращает текущий уровень готовности
#[inline]
pub fn acpi_ready_level() -> AcpiReadyLevel {
    AcpiReadyLevel::from(ACPI_STATE.load(Ordering::Acquire))
}

/// Возвращает ссылку на глобальные AcpiTables
///
/// Доступно после `acpi_init_early()` (TablesReady).
#[inline]
pub fn acpi_tables() -> Option<&'static AcpiTables<XenAcpiHandler>> {
    if !acpi_is_ready(AcpiReadyLevel::TablesReady) {
        return None;
    }
    let ptr = ACPI_TABLES_PTR.load(Ordering::Acquire);
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { &*ptr })
    }
}

/// Возвращает ссылку на глобальный AcpiPlatform
///
/// Доступно после `acpi_init_full()` (PlatformReady).
#[inline]
pub fn acpi_platform() -> Option<&'static AcpiPlatform<XenAcpiHandler>> {
    if !acpi_is_ready(AcpiReadyLevel::PlatformReady) {
        return None;
    }
    let ptr = ACPI_PLATFORM_PTR.load(Ordering::Acquire);
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { &*ptr })
    }
}

/// Возвращает ссылку на глобальный Interpreter
///
/// Доступно после `acpi_init_full()` (AmlReady).
#[inline]
pub fn acpi_interpreter() -> Option<&'static Interpreter<XenAcpiHandler>> {
    if !acpi_is_ready(AcpiReadyLevel::AmlReady) {
        return None;
    }
    let ptr = ACPI_INTERPRETER_PTR.load(Ordering::Acquire);
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { &*ptr })
    }
}

/// Возвращает физический адрес RSDP
#[inline]
pub fn acpi_rsdp_physical() -> usize {
    ACPI_RSDP_PHYS.load(Ordering::Acquire) as usize
}

// =============================================================================
// Вспомогательные функции
// =============================================================================

/// Конвертирует AcpiError в строку для логирования
fn acpi_error_to_str(e: &AcpiError) -> &'static str {
    match e {
        AcpiError::NoValidRsdp => "NoValidRsdp",
        AcpiError::RsdpIncorrectSignature => "RsdpIncorrectSignature",
        AcpiError::RsdpInvalidOemId => "RsdpInvalidOemId",
        AcpiError::RsdpInvalidChecksum => "RsdpInvalidChecksum",
        AcpiError::SdtInvalidSignature(_) => "SdtInvalidSignature",
        AcpiError::SdtInvalidOemId(_) => "SdtInvalidOemId",
        AcpiError::SdtInvalidTableId(_) => "SdtInvalidTableId",
        AcpiError::SdtInvalidChecksum(_) => "SdtInvalidChecksum",
        AcpiError::SdtInvalidCreatorId(_) => "SdtInvalidCreatorId",
        AcpiError::TableNotFound(_) => "TableNotFound",
        AcpiError::InvalidFacsAddress => "InvalidFacsAddress",
        AcpiError::InvalidDsdtAddress => "InvalidDsdtAddress",
        AcpiError::InvalidMadt(_) => "InvalidMadt",
        AcpiError::InvalidGenericAddress => "InvalidGenericAddress",
        AcpiError::Timeout => "Timeout",
        AcpiError::Aml(_) => "AmlError",
        AcpiError::LibUnimplemented => "LibUnimplemented",
        AcpiError::HostUnimplemented => "HostUnimplemented",
    }
}

