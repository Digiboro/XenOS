//! Memory Manager экспорты (`Mm*`)
//!
//! Экспорты подсистемы MM: виртуальная память, MMIO, пулы и т.д.

use crate::mm;

// =============================================================================
// Type Aliases
// =============================================================================

type PVOID = *mut core::ffi::c_void;
type PHYSICAL_ADDRESS = u64;
type SIZE_T = usize;
type ULONG = u32;
type KPROCESSOR_MODE = i8;

// =============================================================================
// MMIO
// =============================================================================

/// MmMapIoSpace - маппит физический адрес в kernel space
///
/// # Arguments
/// * `physical_address` - физический адрес
/// * `number_of_bytes` - размер региона
/// * `cache_type` - тип кеширования (0=non-cached, 1=cached, 2=write-combined)
///
/// # Returns
/// Виртуальный адрес или NULL при ошибке
#[unsafe(export_name = "MmMapIoSpace")]
pub unsafe extern "win64" fn MmMapIoSpace(
    physical_address: PHYSICAL_ADDRESS,
    number_of_bytes: SIZE_T,
    cache_type: ULONG,
) -> PVOID {
    // XenCore использует HHDM (Higher-Half Direct Map) для MMIO
    // HHDM: вся физическая память замаппирована на 0xFFFF800000000000
    // Это архитектурное решение, аналогичное Linux direct mapping
    //
    // Преимущества HHDM для MMIO:
    // 1. Нет необходимости создавать page tables динамически
    // 2. Нет фрагментации VA space
    // 3. Простота и надёжность
    // 4. Одинаковый VA для одного и того же phys_addr
    //
    // NOTE: cache_type игнорируется т.к. HHDM использует default caching (UC- для MMIO range)
    // Для изменения cache attributes нужно использовать PAT (Page Attribute Table)
    
    let _ = cache_type;
    let _ = number_of_bytes;
    
    const HHDM_BASE: u64 = 0xFFFF800000000000;
    (HHDM_BASE + physical_address) as PVOID
}

/// MmUnmapIoSpace - размаппливает IO space
#[unsafe(export_name = "MmUnmapIoSpace")]
pub unsafe extern "win64" fn MmUnmapIoSpace(
    base_address: PVOID,
    number_of_bytes: SIZE_T,
) {
    // HHDM mappings постоянные - не требуют размаппинга
    // Это корректное поведение для HHDM архитектуры
    
    let _ = base_address;
    let _ = number_of_bytes;
}

// =============================================================================
// Contiguous Memory
// =============================================================================

/// MmAllocateContiguousMemory - выделяет физически непрерывную память
///
/// Используется для DMA буферов.
///
/// # Arguments
/// * `number_of_bytes` - размер в байтах
/// * `highest_acceptable_address` - максимальный физический адрес
///
/// # Returns
/// Виртуальный адрес или NULL
#[unsafe(export_name = "MmAllocateContiguousMemory")]
pub unsafe extern "win64" fn MmAllocateContiguousMemory(
    number_of_bytes: SIZE_T,
    highest_acceptable_address: PHYSICAL_ADDRESS,
) -> PVOID {
    // Используем pool allocator для выделения contiguous памяти
    // Для DMA важно чтобы память была физически непрерывной
    
    use crate::ex::pool::{ex_allocate_pool_with_tag, POOL_TYPE};
    
    // Выделяем из NonPagedPool (который гарантирует непрерывность для малых аллокаций)
    let tag = u32::from_le_bytes(*b"MmCt");
    let ptr = ex_allocate_pool_with_tag(POOL_TYPE::NonPagedPool, number_of_bytes, tag);
    
    if ptr.is_null() {
        return core::ptr::null_mut();
    }
    
    // Обнуляем память
    core::ptr::write_bytes(ptr as *mut u8, 0, number_of_bytes);
    
    ptr
}

/// MmAllocateContiguousMemorySpecifyCache - выделяет contiguous память с указанием cache type
#[unsafe(export_name = "MmAllocateContiguousMemorySpecifyCache")]
pub unsafe extern "win64" fn MmAllocateContiguousMemorySpecifyCache(
    number_of_bytes: SIZE_T,
    lowest_acceptable_address: PHYSICAL_ADDRESS,
    highest_acceptable_address: PHYSICAL_ADDRESS,
    boundary_address_multiple: PHYSICAL_ADDRESS,
    cache_type: ULONG,
) -> PVOID {
    // Для простоты используем обычный allocator
    MmAllocateContiguousMemory(number_of_bytes, highest_acceptable_address)
}

/// MmFreeContiguousMemory - освобождает contiguous память
#[unsafe(export_name = "MmFreeContiguousMemory")]
pub unsafe extern "win64" fn MmFreeContiguousMemory(base_address: PVOID) {
    if base_address.is_null() {
        return;
    }
    
    use crate::ex::pool::ex_free_pool_with_tag;
    
    let tag = u32::from_le_bytes(*b"MmCt");
    ex_free_pool_with_tag(base_address, tag);
}

/// MmGetPhysicalAddress - получает физический адрес для виртуального
#[unsafe(export_name = "MmGetPhysicalAddress")]
pub unsafe extern "win64" fn MmGetPhysicalAddress(base_address: PVOID) -> PHYSICAL_ADDRESS {
    let va = base_address as u64;
    
    // Для HHDM mapped адресов (0xFFFF8000_00000000+)
    if va >= 0xFFFF8000_00000000u64 && va < 0xFFFFFFFF_80000000u64 {
        return va - 0xFFFF8000_00000000u64;
    }
    
    // Для kernel pool (0xFFFF9000_00000000+)
    if va >= 0xFFFF9000_00000000u64 && va < 0xFFFFA000_00000000u64 {
        // Pool использует HHDM-like mapping
        return va - 0xFFFF8000_00000000u64;
    }
    
    // Для других адресов - полный page table walk
    mm::mm_virtual_to_physical(va).unwrap_or(0)
}

/// MmGetVirtualForPhysical - получает виртуальный адрес для физического (через HHDM)
#[unsafe(export_name = "MmGetVirtualForPhysical")]
pub unsafe extern "win64" fn MmGetVirtualForPhysical(physical_address: PHYSICAL_ADDRESS) -> PVOID {
    // Используем HHDM (Higher-Half Direct Map)
    let hhdm_base = 0xFFFF8000_00000000u64;
    (hhdm_base + physical_address) as PVOID
}

// =============================================================================
// MDL Functions
// =============================================================================

/// MmBuildMdlForNonPagedPool - строит MDL для non-paged pool
///
/// Для NonPagedPool память уже физически contiguous и locked,
/// поэтому MDL просто нужно заполнить физическими адресами.
#[unsafe(export_name = "MmBuildMdlForNonPagedPool")]
pub unsafe extern "win64" fn MmBuildMdlForNonPagedPool(memory_descriptor_list: PVOID) {
    if memory_descriptor_list.is_null() {
        return;
    }
    
    // NonPagedPool память уже locked - MDL можно использовать сразу
    // В полной реализации нужно заполнить PFN array в MDL
    // Пока это no-op т.к. NonPagedPool всегда accessible
}

/// MmMapLockedPages - маппит страницы описанные в MDL
///
/// Для NonPagedPool возвращает оригинальный VA (память уже замаппирована).
#[unsafe(export_name = "MmMapLockedPages")]
pub unsafe extern "win64" fn MmMapLockedPages(
    memory_descriptor_list: PVOID,
    access_mode: KPROCESSOR_MODE,
) -> PVOID {
    let _ = access_mode;
    
    if memory_descriptor_list.is_null() {
        return core::ptr::null_mut();
    }
    
    // MDL structure (упрощённо):
    // struct MDL { next, size, flags, process, mapped_system_va, start_va, ... }
    // Offset to start_va - обычно 0x18
    let mdl_ptr = memory_descriptor_list as *const u8;
    let start_va_ptr = mdl_ptr.add(0x18) as *const PVOID;
    let start_va = core::ptr::read(start_va_ptr);
    
    // Для NonPagedPool просто возвращаем исходный VA
    start_va
}

/// MmUnmapLockedPages - размаппливает страницы MDL
///
/// Для NonPagedPool - no-op (память permanent mapped).
#[unsafe(export_name = "MmUnmapLockedPages")]
pub unsafe extern "win64" fn MmUnmapLockedPages(
    base_address: PVOID,
    memory_descriptor_list: PVOID,
) {
    let _ = base_address;
    let _ = memory_descriptor_list;
    
    // NonPagedPool mappings постоянные - ничего не делаем
}

// =============================================================================
// MDL Page Locking
// =============================================================================

/// Тип операции блокировки страниц
/// 
/// MSDN: LOCK_OPERATION
pub type LOCK_OPERATION = ntoskrnl::mm::mdl::LOCK_OPERATION;

/// MmProbeAndLockPages - проверяет и блокирует страницы буфера
///
/// Выполняет page walk, заполняет PFN array в MDL и увеличивает
/// reference count для каждой страницы (pinning).
///
/// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/nf-wdm-mmprobeandlockpages
#[unsafe(export_name = "MmProbeAndLockPages")]
pub unsafe extern "win64" fn MmProbeAndLockPages(
    memory_descriptor_list: PVOID,
    access_mode: KPROCESSOR_MODE,
    operation: LOCK_OPERATION,
) {
    #[cfg(feature = "storage-trace")]
    {
        ntoskrnl::kd::dbg_print("[MDL] MmProbeAndLockPages: mdl=0x");
        ntoskrnl::kd::dbg_print_hex(memory_descriptor_list as u64);
        ntoskrnl::kd::dbg_print(" mode=");
        ntoskrnl::kd::dbg_print_hex(access_mode as u64);
        ntoskrnl::kd::dbg_print("\n");
    }
    
    let status = ntoskrnl::mm::mdl::mm_probe_and_lock_pages(
        memory_descriptor_list as *mut ntoskrnl::mm::mdl::MDL,
        access_mode as u8,
        operation,
    );
    
    #[cfg(feature = "storage-trace")]
    {
        ntoskrnl::kd::dbg_print("[MDL] MmProbeAndLockPages: status=0x");
        ntoskrnl::kd::dbg_print_hex(status as u64);
        ntoskrnl::kd::dbg_print("\n");
    }
    
    // NT raises exception on failure, but we just log for now
    if status != 0 {
        #[cfg(feature = "storage-trace")]
        ntoskrnl::kd::dbg_print("[MDL] MmProbeAndLockPages: FAILED!\n");
    }
}

/// MmUnlockPages - разблокирует страницы MDL
///
/// Уменьшает reference count для страниц и очищает флаг MDL_PAGES_LOCKED.
///
/// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/nf-wdm-mmunlockpages
#[unsafe(export_name = "MmUnlockPages")]
pub unsafe extern "win64" fn MmUnlockPages(memory_descriptor_list: PVOID) {
    #[cfg(feature = "storage-trace")]
    {
        ntoskrnl::kd::dbg_print("[MDL] MmUnlockPages: mdl=0x");
        ntoskrnl::kd::dbg_print_hex(memory_descriptor_list as u64);
        ntoskrnl::kd::dbg_print("\n");
    }
    
    ntoskrnl::mm::mdl::mm_unlock_pages(
        memory_descriptor_list as *mut ntoskrnl::mm::mdl::MDL,
    );
}
