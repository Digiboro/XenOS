//! I/O Manager Internal Data (Iop*)
//!
//! Внутренние глобальные структуры и функции I/O Manager.
//! Модуль содержит глобальные локи, списки объектов, счётчики и функции
//! управления временем жизни объектов (reference counting).
//!
//! # Архитектура
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                        I/O Manager Internal                         │
//! ├─────────────────────────────────────────────────────────────────────┤
//! │                                                                     │
//! │  ┌──────────────────┐     ┌──────────────────┐                     │
//! │  │ IOP_DATABASE_LOCK│     │IOP_DEVICE_TREE_  │                     │
//! │  │   (KSPIN_LOCK)   │     │      LOCK        │                     │
//! │  └────────┬─────────┘     └────────┬─────────┘                     │
//! │           │                        │                               │
//! │           ▼                        ▼                               │
//! │  ┌──────────────────┐     ┌──────────────────┐                     │
//! │  │IOP_DRIVER_LIST_  │     │  PnP Device Tree │                     │
//! │  │     HEAD         │     │   (будущее)      │                     │
//! │  └──────────────────┘     └──────────────────┘                     │
//! │                                                                     │
//! │  ┌─────────────────────────────────────────────────────────────┐   │
//! │  │                  Reference Counting                          │   │
//! │  ├─────────────────────────────────────────────────────────────┤   │
//! │  │  DEVICE_OBJECT     DRIVER_OBJECT     FILE_OBJECT            │   │
//! │  │  ├─ Reference()    ├─ Reference()    ├─ Reference()         │   │
//! │  │  └─ Dereference()  └─ Dereference()  └─ Dereference()       │   │
//! │  └─────────────────────────────────────────────────────────────┘   │
//! │                                                                     │
//! │  ┌─────────────────────────────────────────────────────────────┐   │
//! │  │                  Статистика (Atomic)                         │   │
//! │  ├─────────────────────────────────────────────────────────────┤   │
//! │  │  IOP_DRIVER_COUNT  IOP_DEVICE_COUNT  IOP_FILE_COUNT         │   │
//! │  │  IOP_IRP_COUNT                                               │   │
//! │  └─────────────────────────────────────────────────────────────┘   │
//! └─────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Reference Counting
//!
//! Модель управления временем жизни объектов I/O:
//!
//! - **DEVICE_OBJECT**: refcount инкрементируется при создании IRP,
//!   открытии FILE_OBJECT, присоединении к стеку устройств
//! - **DRIVER_OBJECT**: refcount через DRIVER_EXTENSION.count,
//!   инкрементируется при создании DEVICE_OBJECT
//! - **FILE_OBJECT**: управляется через OB (ObReferenceObject/ObDereferenceObject)
//!
//! # Отступления и упрощения
//!
//! - FILE_OBJECT refcount — пока заглушка, в полной реализации использует OB
//! - IOP_DRIVER_LIST_HEAD — список драйверов для отладки, не полная реализация NT
//! - Статистика (счётчики) — для диагностики, в NT используется другой механизм
//!
//! Источники:
//! - ReactOS: ntoskrnl/io/iomgr/iomgr.c, ntoskrnl/io/iomgr/iofunc.c
//! - NT5: io/iomgr/internal.c

#![allow(dead_code)]

use core::sync::atomic::AtomicU32;
use core::sync::atomic::Ordering;

use super::types::*;
use crate::ke::spinlock::KSPIN_LOCK;
use crate::nt::LIST_ENTRY;

// =============================================================================
// Глобальные локи I/O Manager
// =============================================================================

/// IopDatabaseLock — основной лок для структур I/O Manager
///
/// Используется для синхронизации доступа к:
/// - Списку драйверов
/// - Списку устройств
/// - Изменениям device stacks
pub static mut IOP_DATABASE_LOCK: KSPIN_LOCK = KSPIN_LOCK::new();

/// IopDeviceTreeLock — лок для дерева устройств PnP
pub static mut IOP_DEVICE_TREE_LOCK: KSPIN_LOCK = KSPIN_LOCK::new();

/// IopFileObjectLock — лок для операций с FILE_OBJECT
pub static mut IOP_FILE_OBJECT_LOCK: KSPIN_LOCK = KSPIN_LOCK::new();

/// IoCancelSpinLock — глобальный лок для отмены IRP
///
/// Используется в IoCancelIrp и cancel routines.
/// Драйвер должен захватить этот лок перед изменением cancel routine.
pub static mut IO_CANCEL_SPIN_LOCK: KSPIN_LOCK = KSPIN_LOCK::new();

// =============================================================================
// Глобальные списки
// =============================================================================

/// Список всех загруженных драйверов
///
/// Каждый DRIVER_OBJECT содержит driver_section для связи в этом списке.
pub static mut IOP_DRIVER_LIST_HEAD: LIST_ENTRY = LIST_ENTRY::new();

/// Счётчик загруженных драйверов (для отладки)
pub static IOP_DRIVER_COUNT: AtomicU32 = AtomicU32::new(0);

/// Счётчик созданных устройств (для отладки)
pub static IOP_DEVICE_COUNT: AtomicU32 = AtomicU32::new(0);

/// Счётчик открытых файлов (для отладки)
pub static IOP_FILE_COUNT: AtomicU32 = AtomicU32::new(0);

/// Счётчик выделенных IRP (для отладки утечек)
pub static IOP_IRP_COUNT: AtomicU32 = AtomicU32::new(0);

// =============================================================================
// Инициализация внутренних структур
// =============================================================================

/// Инициализирует внутренние структуры I/O Manager
///
/// Вызывается из io_init_phase0()
pub unsafe fn iop_init_internal() {
    unsafe {
        // Инициализируем списки
        LIST_ENTRY::init_head(core::ptr::addr_of_mut!(IOP_DRIVER_LIST_HEAD));

        // Сбрасываем счётчики
        IOP_DRIVER_COUNT.store(0, Ordering::Release);
        IOP_DEVICE_COUNT.store(0, Ordering::Release);
        IOP_FILE_COUNT.store(0, Ordering::Release);
        IOP_IRP_COUNT.store(0, Ordering::Release);
    }
}

// =============================================================================
// Reference Counting - DEVICE_OBJECT
// =============================================================================

/// IopReferenceDeviceObject — увеличивает счётчик ссылок на устройство
///
/// Вызывается при:
/// - Создании IRP для устройства
/// - Открытии FILE_OBJECT на устройстве
/// - IoAttachDeviceToDeviceStack
///
/// # Safety
/// device_object должен быть валидным указателем
#[inline]
pub unsafe fn iop_reference_device_object(device_object: PDEVICE_OBJECT) {
    unsafe {
        if !device_object.is_null() {
            let old = core::sync::atomic::AtomicI32::from_ptr(
                &mut (*device_object).reference_count as *mut i32,
            )
            .fetch_add(1, Ordering::AcqRel);

            // Отладка: проверяем переполнение
            debug_assert!(old >= 0, "DEVICE_OBJECT refcount underflow detected");
        }
    }
}

/// IopDereferenceDeviceObject — уменьшает счётчик ссылок на устройство
///
/// Если счётчик становится 0, устройство может быть удалено.
///
/// # Returns
/// Новое значение счётчика ссылок
#[inline]
pub unsafe fn iop_dereference_device_object(device_object: PDEVICE_OBJECT) -> i32 {
    unsafe {
        if device_object.is_null() {
            return 0;
        }

        let new = core::sync::atomic::AtomicI32::from_ptr(
            &mut (*device_object).reference_count as *mut i32,
        )
        .fetch_sub(1, Ordering::AcqRel)
            - 1;

        debug_assert!(new >= 0, "DEVICE_OBJECT refcount went negative");

        new
    }
}

/// Получает текущий счётчик ссылок устройства
#[inline]
pub unsafe fn iop_get_device_reference_count(device_object: PDEVICE_OBJECT) -> i32 {
    unsafe {
        if device_object.is_null() {
            return 0;
        }

        core::sync::atomic::AtomicI32::from_ptr(&raw mut (*device_object).reference_count)
            .load(Ordering::Acquire)
    }
}

// =============================================================================
// Reference Counting - DRIVER_OBJECT
// =============================================================================

/// IopReferenceDriverObject — увеличивает счётчик ссылок на драйвер
///
/// Вызывается при создании DEVICE_OBJECT для драйвера.
#[inline]
pub unsafe fn iop_reference_driver_object(driver_object: PDRIVER_OBJECT) {
    unsafe {
        if driver_object.is_null() {
            return;
        }

        // driver_object.device_object_count используется как refcount
        // (в NT это отслеживается через количество устройств)
        let count_ptr = &mut (*(*driver_object).driver_extension).count as *mut u32;
        core::sync::atomic::AtomicU32::from_ptr(count_ptr).fetch_add(1, Ordering::AcqRel);
    }
}

/// IopDereferenceDriverObject — уменьшает счётчик ссылок на драйвер
///
/// # Returns
/// Новое значение счётчика
#[inline]
pub unsafe fn iop_dereference_driver_object(driver_object: PDRIVER_OBJECT) -> u32 {
    unsafe {
        if driver_object.is_null() || (*driver_object).driver_extension.is_null() {
            return 0;
        }

        let count_ptr = &mut (*(*driver_object).driver_extension).count as *mut u32;
        let new = core::sync::atomic::AtomicU32::from_ptr(count_ptr)
            .fetch_sub(1, Ordering::AcqRel)
            .saturating_sub(1);

        new
    }
}

// =============================================================================
// Reference Counting - FILE_OBJECT
// =============================================================================

/// ObReferenceFileObject — увеличивает счётчик ссылок на файл
///
/// FILE_OBJECT использует OB refcount механизм.
/// Эта функция — wrapper для удобства.
#[inline]
pub unsafe fn iop_reference_file_object(file_object: PFILE_OBJECT) {
    if file_object.is_null() {
        return;
    }

    // FILE_OBJECT управляется через OB, но мы можем добавить internal refcount
    // для отслеживания IRP, если понадобится

    // Пока просто инкрементируем счётчик в file_object если он есть
    // В полной реализации используется ObReferenceObject
}

/// ObDereferenceFileObject — уменьшает счётчик ссылок на файл
#[inline]
pub unsafe fn iop_dereference_file_object(file_object: PFILE_OBJECT) {
    if file_object.is_null() {
        return;
    }

    // В полной реализации используется ObDereferenceObject
}

// =============================================================================
// IRP Tracking
// =============================================================================

/// Отслеживает создание IRP (для отладки)
#[inline]
pub fn iop_track_irp_allocate() {
    IOP_IRP_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Отслеживает освобождение IRP (для отладки)
#[inline]
pub fn iop_track_irp_free() {
    IOP_IRP_COUNT.fetch_sub(1, Ordering::Relaxed);
}

/// Возвращает количество активных IRP
#[inline]
pub fn iop_get_active_irp_count() -> u32 {
    IOP_IRP_COUNT.load(Ordering::Relaxed)
}

// =============================================================================
// Device/Driver Tracking
// =============================================================================

/// Отслеживает создание устройства
#[inline]
pub fn iop_track_device_create() {
    IOP_DEVICE_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Отслеживает удаление устройства
#[inline]
pub fn iop_track_device_delete() {
    IOP_DEVICE_COUNT.fetch_sub(1, Ordering::Relaxed);
}

/// Возвращает количество активных устройств
#[inline]
pub fn iop_get_device_count() -> u32 {
    IOP_DEVICE_COUNT.load(Ordering::Relaxed)
}

/// Отслеживает загрузку драйвера
#[inline]
pub fn iop_track_driver_load() {
    IOP_DRIVER_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Отслеживает выгрузку драйвера
#[inline]
pub fn iop_track_driver_unload() {
    IOP_DRIVER_COUNT.fetch_sub(1, Ordering::Relaxed);
}

/// Возвращает количество загруженных драйверов
#[inline]
pub fn iop_get_driver_count() -> u32 {
    IOP_DRIVER_COUNT.load(Ordering::Relaxed)
}

// =============================================================================
// Диагностика
// =============================================================================

/// Выводит статистику I/O Manager (для отладки)
pub fn iop_dump_statistics() {
    use crate::kd::dbg_print;
    use crate::kd::dbg_print_num;

    dbg_print("[IOP] Statistics:\n");
    dbg_print("  Drivers:  ");
    dbg_print_num(IOP_DRIVER_COUNT.load(Ordering::Relaxed) as u64);
    dbg_print("\n");
    dbg_print("  Devices:  ");
    dbg_print_num(IOP_DEVICE_COUNT.load(Ordering::Relaxed) as u64);
    dbg_print("\n");
    dbg_print("  Files:    ");
    dbg_print_num(IOP_FILE_COUNT.load(Ordering::Relaxed) as u64);
    dbg_print("\n");
    dbg_print("  IRPs:     ");
    dbg_print_num(IOP_IRP_COUNT.load(Ordering::Relaxed) as u64);
    dbg_print("\n");
}

// =============================================================================
// Вспомогательные функции
// =============================================================================

/// Проверяет что устройство в валидном состоянии для операций
#[inline]
pub unsafe fn iop_is_device_valid(device_object: PDEVICE_OBJECT) -> bool {
    unsafe {
        if device_object.is_null() {
            return false;
        }

        // Проверяем тип
        if (*device_object).r#type != super::types::IO_TYPE_DEVICE as i16 {
            return false;
        }

        // Проверяем что драйвер существует
        if (*device_object).driver_object.is_null() {
            return false;
        }

        true
    }
}

/// Проверяет что IRP в валидном состоянии
#[inline]
pub unsafe fn iop_is_irp_valid(irp: PIRP) -> bool {
    unsafe {
        if irp.is_null() {
            return false;
        }

        // Проверяем тип
        if (*irp).r#type != super::types::IO_TYPE_IRP as i16 {
            return false;
        }

        // Проверяем stack location
        if (*irp).stack_count <= 0 {
            return false;
        }

        true
    }
}
