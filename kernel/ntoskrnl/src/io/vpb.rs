//! Volume Parameter Block (VPB)
//!
//! VPB содержит информацию о смонтированном томе и связывает device с file system.
//!
//! # Архитектура
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────┐
//! │                    VPB Architecture                          │
//! ├──────────────────────────────────────────────────────────────┤
//! │                                                              │
//! │  Storage Device (Partition PDO)                              │
//! │       ↓                                                      │
//! │  ┌──────────────┐                                            │
//! │  │ DEVICE_OBJECT│──► vpb ──────┐                            │
//! │  │ (Storage)    │               │                            │
//! │  └──────────────┘               ▼                            │
//! │                          ┌──────────────┐                    │
//! │                          │     VPB      │                    │
//! │                          ├──────────────┤                    │
//! │                          │ RealDevice ──┼──► Storage Device  │
//! │                          │ DeviceObject─┼──► FS Device       │
//! │                          │ Flags        │                    │
//! │                          │ VolumeLabel  │                    │
//! │                          └──────┬───────┘                    │
//! │                                 │                            │
//! │  ┌──────────────┐              │                            │
//! │  │ DEVICE_OBJECT│◄─────────────┘                            │
//! │  │ (File System)│                                            │
//! │  │              │                                            │
//! │  │ DriverObject ┼──► FAT32/NTFS Driver                      │
//! │  └──────────────┘                                            │
//! │                                                              │
//! │  I/O Request → Storage Device                                │
//! │    ├─ Если VPB->DeviceObject != NULL → перенаправить к FS   │
//! │    └─ Иначе → прямой доступ к Storage                       │
//! │                                                              │
//! └──────────────────────────────────────────────────────────────┘
//! ```
//!
//! # VPB Lifecycle
//!
//! 1. **IoAllocateVpb** - создание VPB для storage device
//! 2. **IoRegisterFileSystem** - регистрация FS driver
//! 3. **IRP_MN_MOUNT_VOLUME** - FS driver монтирует том
//!    - FS создаёт device object
//!    - Устанавливает VPB->DeviceObject = fs_device
//!    - Устанавливает VPB->Flags |= VPB_MOUNTED
//! 4. **I/O Operations** - перенаправляются к FS device через VPB
//! 5. **IRP_MN_VERIFY_VOLUME** - проверка что media не изменилась
//! 6. **Dismount** - FS освобождает resources, VPB->DeviceObject = NULL
//!
//! Источники:
//! - ReactOS: sdk/include/xdk/iotypes.h, ntoskrnl/io/iomgr/volume.c
//! - NT6.1: WinDDK, ntos/io/iomgr/internal.h

use crate::nt::*;
use super::types::*;
use core::ptr;

// =============================================================================
// VPB Constants
// =============================================================================

/// Максимальная длина volume label в байтах (включая null terminator)
pub const MAXIMUM_VOLUME_LABEL_LENGTH: usize = 32;

/// VPB Type value (IO_TYPE_VPB)
pub const IO_TYPE_VPB: u16 = 10;

// VPB Flags
pub const VPB_MOUNTED: u16 = 0x0001;
pub const VPB_LOCKED: u16 = 0x0002;
pub const VPB_PERSISTENT: u16 = 0x0004;
pub const VPB_REMOVE_PENDING: u16 = 0x0008;
pub const VPB_RAW_MOUNT: u16 = 0x0010;
pub const VPB_DIRECT_WRITES_ALLOWED: u16 = 0x0020;

// =============================================================================
// VPB Type (структура определена в device.rs)
// =============================================================================

// Используем VPB из device module
pub use super::device::VPB;
pub type PVPB = *mut VPB;

// =============================================================================
// VPB Allocation
// =============================================================================

/// IoAllocateVpb - выделяет и инициализирует VPB
///
/// # Arguments
/// * `device_object` - storage device для которого создаётся VPB
///
/// # Returns
/// Указатель на VPB или NULL при ошибке
pub unsafe fn io_allocate_vpb(device_object: PDEVICE_OBJECT) -> PVPB {
    use crate::ex::pool::POOL_TYPE;
    use crate::ex::pool::ex_allocate_pool_with_tag;
    
    if device_object.is_null() {
        return ptr::null_mut();
    }
    
    // Выделяем VPB из NonPagedPool
    let vpb = ex_allocate_pool_with_tag(
        POOL_TYPE::NonPagedPool,
        core::mem::size_of::<VPB>(),
        u32::from_le_bytes(*b"Vpb "),
    );
    
    if vpb.is_null() {
        return ptr::null_mut();
    }
    
    let vpb = vpb as PVPB;
    
    // Инициализируем VPB
    ptr::write_bytes(vpb as *mut u8, 0, core::mem::size_of::<VPB>());
    
    (*vpb).r#type = IO_TYPE_VPB as CSHORT;
    (*vpb).size = core::mem::size_of::<VPB>() as CSHORT;
    
    // Устанавливаем RealDevice
    (*vpb).real_device = device_object;
    (*vpb).reference_count = 1;
    
    vpb
}

/// IoFreeVpb - освобождает VPB
///
/// # Arguments
/// * `vpb` - VPB для освобождения
pub unsafe fn io_free_vpb(vpb: PVPB) {
    use crate::ex::pool::ex_free_pool_with_tag;
    
    if vpb.is_null() {
        return;
    }
    
    // Проверяем reference count
    if (*vpb).reference_count != 0 {
        crate::dbg_print!(
            "[IO] WARNING: Freeing VPB with refcount={}\n",
            (*vpb).reference_count
        );
    }
    
    ex_free_pool_with_tag(vpb as PVOID, u32::from_le_bytes(*b"Vpb "));
}

// =============================================================================
// VPB Reference Counting
// =============================================================================

/// Увеличивает reference count VPB
#[inline]
pub unsafe fn iop_reference_vpb(vpb: PVPB) {
    if !vpb.is_null() {
        (*vpb).reference_count += 1;
    }
}

/// Уменьшает reference count VPB
///
/// Если счётчик достигает 0, VPB **НЕ** освобождается автоматически
/// (в отличие от объектов OB). Вызывающий должен вызвать IoFreeVpb явно.
#[inline]
pub unsafe fn iop_dereference_vpb(vpb: PVPB) {
    if !vpb.is_null() && (*vpb).reference_count > 0 {
        (*vpb).reference_count -= 1;
    }
}

// =============================================================================
// File System Registration
// =============================================================================

use crate::ke::spinlock::KSPIN_LOCK;
use crate::nt::LIST_ENTRY;

/// Список зарегистрированных file systems
static mut IOP_FILE_SYSTEM_QUEUE_HEAD: LIST_ENTRY = LIST_ENTRY::new();

/// Spinlock для защиты списка file systems
static mut IOP_FILE_SYSTEM_QUEUE_LOCK: KSPIN_LOCK = KSPIN_LOCK::new();

/// Инициализирует VPB subsystem
pub unsafe fn iop_init_vpb() {
    LIST_ENTRY::init_head(&raw mut IOP_FILE_SYSTEM_QUEUE_HEAD);
}

/// IoRegisterFileSystem - регистрирует file system driver
///
/// File system drivers вызывают эту функцию в своём DriverEntry.
/// Устройство добавляется в глобальный список file systems.
///
/// # Arguments
/// * `device_object` - file system device object
pub unsafe fn io_register_file_system(device_object: PDEVICE_OBJECT) {
    use crate::ke::spinlock::{ke_acquire_spin_lock, ke_release_spin_lock};
    
    if device_object.is_null() {
        return;
    }
    
    // Захватываем lock
    let lock_ref = &*(&raw mut IOP_FILE_SYSTEM_QUEUE_LOCK);
    let old_irql = ke_acquire_spin_lock(lock_ref);
    
    // Вставляем в конец списка
    // DEVICE_OBJECT::queue это KDEVICE_QUEUE, который начинается с LIST_ENTRY
    let device_list_entry = &mut (*device_object).queue as *mut _ as *mut LIST_ENTRY;
    LIST_ENTRY::insert_tail(
        &raw mut IOP_FILE_SYSTEM_QUEUE_HEAD,
        device_list_entry,
    );
    
    ke_release_spin_lock(lock_ref, old_irql);
    
    crate::dbg_print!("[IO] Registered file system: device=0x{:016X}\n", device_object as usize);
}

/// IoUnregisterFileSystem - отменяет регистрацию file system driver
///
/// # Arguments
/// * `device_object` - file system device object
pub unsafe fn io_unregister_file_system(device_object: PDEVICE_OBJECT) {
    use crate::ke::spinlock::{ke_acquire_spin_lock, ke_release_spin_lock};
    
    if device_object.is_null() {
        return;
    }
    
    // Захватываем lock
    let lock_ref = &*(&raw mut IOP_FILE_SYSTEM_QUEUE_LOCK);
    let old_irql = ke_acquire_spin_lock(lock_ref);
    
    // Удаляем из списка
    let device_list_entry = &mut (*device_object).queue as *mut _ as *mut LIST_ENTRY;
    LIST_ENTRY::remove_entry(device_list_entry);
    
    ke_release_spin_lock(lock_ref, old_irql);
}

// =============================================================================
// Mount Operations
// =============================================================================

/// IopMountVolume - монтирует том на устройстве
///
/// Вызывается когда нужно смонтировать file system на storage device.
/// Перебирает зарегистрированные FS драйверы и отправляет IRP_MN_MOUNT_VOLUME.
///
/// # Arguments
/// * `device_object` - storage device (partition)
/// * `allow_raw_mount` - разрешить RAW mount если FS не распознана
///
/// # Returns
/// STATUS_SUCCESS если том смонтирован, код ошибки иначе
pub unsafe fn iop_mount_volume(
    device_object: PDEVICE_OBJECT,
    allow_raw_mount: bool,
) -> NTSTATUS {
    use crate::ke::spinlock::{ke_acquire_spin_lock, ke_release_spin_lock};
    
    if device_object.is_null() {
        return STATUS_INVALID_PARAMETER;
    }
    
    // Проверяем что устройство имеет VPB
    let vpb = (*device_object).vpb;
    if vpb.is_null() {
        return STATUS_INVALID_PARAMETER;
    }
    
    // Проверяем что VPB ещё не смонтирован
    if ((*vpb).flags & VPB_MOUNTED) != 0 {
        return STATUS_SUCCESS; // Уже смонтирован
    }
    
    // Захватываем список file systems
    let lock_ref = &*(&raw mut IOP_FILE_SYSTEM_QUEUE_LOCK);
    let old_irql = ke_acquire_spin_lock(lock_ref);
    
    let mut entry = (&raw mut IOP_FILE_SYSTEM_QUEUE_HEAD).cast::<LIST_ENTRY>();
    let entry_val = (*entry).flink;
    let mut entry = entry_val;
    let head = &raw mut IOP_FILE_SYSTEM_QUEUE_HEAD as *const LIST_ENTRY;
    
    let mut mount_status: NTSTATUS = 0xC000_0014u32 as i32; // STATUS_UNRECOGNIZED_VOLUME
    
    while entry as *const LIST_ENTRY != head {
        // Получаем DEVICE_OBJECT из LIST_ENTRY
        // KDEVICE_QUEUE начинается с LIST_ENTRY, который является частью DEVICE_OBJECT::queue
        // Нужно вычесть offset queue от начала DEVICE_OBJECT
        use super::device::DEVICE_OBJECT;
        let queue_offset = core::mem::offset_of!(DEVICE_OBJECT, queue);
        let fs_device = ((entry as usize) - queue_offset) as PDEVICE_OBJECT;
        
        // Освобождаем lock на время mount attempt
        ke_release_spin_lock(lock_ref, old_irql);
        
        // Отправляем IRP_MN_MOUNT_VOLUME к FS driver
        let status = iop_send_mount_volume_irp(fs_device, device_object);
        
        // Захватываем обратно
        let old_irql = ke_acquire_spin_lock(lock_ref);
        
        if status == STATUS_SUCCESS {
            mount_status = STATUS_SUCCESS;
            break;
        }
        
        entry = (*entry).flink;
    }
    
    ke_release_spin_lock(lock_ref, old_irql);
    
    // Если ни один FS не распознал том и разрешён RAW mount
    if mount_status != STATUS_SUCCESS && allow_raw_mount {
        mount_status = iop_raw_mount_volume(device_object);
    }
    
    mount_status
}

/// Отправляет IRP_MN_MOUNT_VOLUME к file system driver
unsafe fn iop_send_mount_volume_irp(
    fs_device: PDEVICE_OBJECT,
    storage_device: PDEVICE_OBJECT,
) -> NTSTATUS {
    use super::irp::{io_allocate_irp, io_free_irp, io_call_driver};
    use super::irp::{io_get_next_irp_stack_location, io_set_next_irp_stack_location};
    use crate::ke::event::{ke_initialize_event, ke_set_event, KEVENT, EVENT_TYPE};
    use crate::ke::wait::ke_wait_for_single_object;
    
    if fs_device.is_null() || storage_device.is_null() {
        return STATUS_INVALID_PARAMETER;
    }
    
    let driver = (*fs_device).driver_object;
    if driver.is_null() {
        return STATUS_INVALID_DEVICE_REQUEST;
    }
    
    // Выделяем IRP
    let stack_size = (*fs_device).stack_size as i8;
    let irp = io_allocate_irp(stack_size, false);
    if irp.is_null() {
        return STATUS_INSUFFICIENT_RESOURCES;
    }
    
    // Создаём event для синхронного wait
    let mut event = KEVENT::new();
    ke_initialize_event(&mut event, EVENT_TYPE::SynchronizationEvent, false);
    
    // Настраиваем IRP
    (*irp).user_event = &mut event as *mut KEVENT;
    (*irp).flags = IRP_SYNCHRONOUS_API | IRP_MOUNT_COMPLETION;
    
    // Заполняем stack location
    let stack = io_get_next_irp_stack_location(irp);
    (*stack).major_function = IRP_MJ_FILE_SYSTEM_CONTROL;
    (*stack).minor_function = IRP_MN_MOUNT_VOLUME;
    (*stack).device_object = storage_device;
    (*stack).parameters.mount_volume.vpb = (*storage_device).vpb;
    (*stack).parameters.mount_volume.device_object = storage_device;
    
    // Отправляем IRP
    let status = io_call_driver(fs_device, irp);
    
    // Ждём completion если PENDING
    if status == STATUS_PENDING {
        ke_wait_for_single_object(
            &mut event as *mut _ as PVOID,
            0, // Executive
            0, // KernelMode
            false,
            None,
        );
    }
    
    let final_status = (*irp).io_status.status;
    io_free_irp(irp);
    
    final_status
}

/// RAW mount - создаёт minimal FS device для прямого доступа
unsafe fn iop_raw_mount_volume(device_object: PDEVICE_OBJECT) -> NTSTATUS {
    // RAW FS - просто устанавливаем флаг, без создания FS device
    let vpb = (*device_object).vpb;
    if vpb.is_null() {
        return STATUS_INSUFFICIENT_RESOURCES;
    }
    
    (*vpb).flags |= VPB_MOUNTED | VPB_RAW_MOUNT;
    
    crate::dbg_print!("[IO] RAW mounted volume on device 0x{:016X}\n", device_object as usize);
    
    STATUS_SUCCESS
}

// =============================================================================
// IoVerifyVolume
// =============================================================================

/// IoVerifyVolume - проверяет что volume всё ещё валиден
///
/// Отправляет IRP_MN_VERIFY_VOLUME к file system driver.
/// Используется после media change или errors.
///
/// # Arguments
/// * `device_object` - storage или file system device
/// * `allow_raw_mount` - разрешить mount если verify failed
///
/// # Returns
/// STATUS_SUCCESS если volume валиден
pub unsafe fn io_verify_volume(
    device_object: PDEVICE_OBJECT,
    allow_raw_mount: bool,
) -> NTSTATUS {
    if device_object.is_null() {
        return STATUS_INVALID_PARAMETER;
    }
    
    // Получаем VPB
    let vpb = (*device_object).vpb;
    if vpb.is_null() {
        return STATUS_INVALID_PARAMETER;
    }
    
    // Если том не смонтирован, пробуем смонтировать
    if ((*vpb).flags & VPB_MOUNTED) == 0 {
        return iop_mount_volume(device_object, allow_raw_mount);
    }
    
    // Получаем FS device
    let fs_device = (*vpb).device_object;
    if fs_device.is_null() {
        // Смонтирован как RAW
        return STATUS_SUCCESS;
    }
    
    // Отправляем IRP_MN_VERIFY_VOLUME
    let status = iop_send_verify_volume_irp(fs_device, vpb);
    
    // Если verify failed, пробуем remount
    if status != STATUS_SUCCESS && allow_raw_mount {
        // Dismount старый FS
        (*vpb).device_object = ptr::null_mut();
        (*vpb).flags &= !VPB_MOUNTED;
        
        // Пробуем remount
        return iop_mount_volume(device_object, allow_raw_mount);
    }
    
    status
}

/// Отправляет IRP_MN_VERIFY_VOLUME к file system
unsafe fn iop_send_verify_volume_irp(
    fs_device: PDEVICE_OBJECT,
    vpb: PVPB,
) -> NTSTATUS {
    use super::irp::{io_allocate_irp, io_free_irp, io_call_driver};
    use super::irp::{io_get_next_irp_stack_location};
    use crate::ke::event::{ke_initialize_event, KEVENT, EVENT_TYPE};
    use crate::ke::wait::ke_wait_for_single_object;
    
    // Выделяем IRP
    let stack_size = (*fs_device).stack_size as i8;
    let irp = io_allocate_irp(stack_size, false);
    if irp.is_null() {
        return STATUS_INSUFFICIENT_RESOURCES;
    }
    
    // Event для wait
    let mut event = KEVENT::new();
    ke_initialize_event(&mut event, EVENT_TYPE::SynchronizationEvent, false);
    
    // Настраиваем IRP
    (*irp).user_event = &mut event as *mut KEVENT;
    (*irp).flags = IRP_SYNCHRONOUS_API;
    
    // Заполняем stack location
    let stack = io_get_next_irp_stack_location(irp);
    (*stack).major_function = IRP_MJ_FILE_SYSTEM_CONTROL;
    (*stack).minor_function = IRP_MN_VERIFY_VOLUME;
    (*stack).device_object = fs_device;
    (*stack).parameters.verify_volume.vpb = vpb;
    (*stack).parameters.verify_volume.device_object = (*vpb).real_device;
    
    // Отправляем
    let status = io_call_driver(fs_device, irp);
    
    if status == STATUS_PENDING {
        ke_wait_for_single_object(
            &mut event as *mut _ as PVOID,
            0,
            0,
            false,
            None,
        );
    }
    
    let final_status = (*irp).io_status.status;
    io_free_irp(irp);
    
    final_status
}

// =============================================================================
// Dismount Operations
// =============================================================================

/// IopDismountVolume - размонтирует том
///
/// Используется при eject, remove device, или explicit dismount request.
///
/// # Arguments
/// * `vpb` - VPB для размонтирования
///
/// # Returns
/// STATUS_SUCCESS при успехе
pub unsafe fn iop_dismount_volume(vpb: PVPB) -> NTSTATUS {
    if vpb.is_null() {
        return STATUS_INVALID_PARAMETER;
    }
    
    let fs_device = (*vpb).device_object;
    
    // Если есть FS device, отправляем dismount request
    if !fs_device.is_null() {
        // В NT: отправляется специальный IRP для dismount
        // Для упрощённой реализации просто очищаем VPB
        
        (*vpb).device_object = ptr::null_mut();
        (*vpb).flags &= !VPB_MOUNTED;
        (*vpb).volume_label_length = 0;
        
        crate::dbg_print!("[IO] Dismounted volume from VPB 0x{:016X}\n", vpb as usize);
    }
    
    STATUS_SUCCESS
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Проверяет смонтирован ли том
#[inline]
pub unsafe fn iop_is_volume_mounted(vpb: PVPB) -> bool {
    !vpb.is_null() && ((*vpb).flags & VPB_MOUNTED) != 0
}

/// Получает FS device из VPB
#[inline]
pub unsafe fn iop_get_mounted_device(vpb: PVPB) -> PDEVICE_OBJECT {
    if vpb.is_null() {
        ptr::null_mut()
    } else {
        (*vpb).device_object
    }
}

/// Устанавливает volume label
pub unsafe fn iop_set_volume_label(vpb: PVPB, label: &[u16]) {
    if vpb.is_null() {
        return;
    }
    
    let max_wchars = MAXIMUM_VOLUME_LABEL_LENGTH / core::mem::size_of::<u16>();
    let copy_len = core::cmp::min(label.len(), max_wchars);
    
    // Копируем label
    for i in 0..copy_len {
        (*vpb).volume_label[i] = label[i];
    }
    
    // Остальное заполняем нулями
    for i in copy_len..max_wchars {
        (*vpb).volume_label[i] = 0;
    }
    
    (*vpb).volume_label_length = (copy_len * 2) as USHORT;
}

