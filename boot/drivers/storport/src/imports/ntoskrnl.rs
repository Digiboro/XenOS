//! Импорты функций из ntoskrnl.exe

use crate::types::*;

#[link(name = "ntoskrnl")]
unsafe extern "win64" {
    // I/O Manager
    pub fn IoCreateDevice(
        driver_object: PDRIVER_OBJECT,
        device_extension_size: ULONG,
        device_name: *const UNICODE_STRING,
        device_type: ULONG,
        device_characteristics: ULONG,
        exclusive: BOOLEAN,
        device_object: *mut PDEVICE_OBJECT,
    ) -> NTSTATUS;

    pub fn IoDeleteDevice(device_object: PDEVICE_OBJECT);

    pub fn IoAttachDeviceToDeviceStack(
        source_device: PDEVICE_OBJECT,
        target_device: PDEVICE_OBJECT,
    ) -> PDEVICE_OBJECT;

    pub fn IoDetachDevice(target_device: PDEVICE_OBJECT);

    pub fn IoAllocateDriverObjectExtension(
        driver_object: PDRIVER_OBJECT,
        client_identification_address: PVOID,
        driver_object_extension_size: ULONG,
        driver_object_extension: *mut PVOID,
    ) -> NTSTATUS;

    pub fn IoGetDriverObjectExtension(
        driver_object: PDRIVER_OBJECT,
        client_identification_address: PVOID,
    ) -> PVOID;

    pub fn IoGetCurrentIrpStackLocation(irp: PIRP) -> PIO_STACK_LOCATION;

    pub fn IoGetNextIrpStackLocation(irp: PIRP) -> PIO_STACK_LOCATION;

    pub fn IoSkipCurrentIrpStackLocation(irp: PIRP);

    pub fn IoCallDriver(device_object: PDEVICE_OBJECT, irp: PIRP) -> NTSTATUS;

    pub fn IoCompleteRequest(irp: PIRP, priority_boost: CCHAR);

    pub fn IofCompleteRequest(irp: PIRP, priority_boost: CCHAR);

    // Memory Manager
    pub fn ExAllocatePoolWithTag(pool_type: ULONG, number_of_bytes: usize, tag: ULONG) -> PVOID;

    pub fn ExFreePoolWithTag(ptr: PVOID, tag: ULONG);

    // Object Manager
    pub fn ObReferenceObject(object: PVOID);

    pub fn ObDereferenceObject(object: PVOID) -> ULONG;

    // Debug
    pub fn DbgPrint(format: *const u8) -> ULONG;

    // PCI Configuration
    pub fn HalGetBusDataByOffset(
        bus_data_type: ULONG,
        bus_number: ULONG,
        slot_number: ULONG,
        buffer: PVOID,
        offset: ULONG,
        length: ULONG,
    ) -> ULONG;

    pub fn HalSetBusDataByOffset(
        bus_data_type: ULONG,
        bus_number: ULONG,
        slot_number: ULONG,
        buffer: PVOID,
        offset: ULONG,
        length: ULONG,
    ) -> ULONG;

    // I/O Space
    pub fn MmMapIoSpace(physical_address: ULONGLONG, number_of_bytes: usize, cache_type: ULONG) -> PVOID;
    pub fn MmGetPhysicalAddress(base_address: PVOID) -> ULONGLONG;
    pub fn MmUnmapIoSpace(base_address: PVOID, number_of_bytes: usize);
    
    // Contiguous/Uncached Memory
    pub fn MmAllocateContiguousMemorySpecifyCache(
        number_of_bytes: usize,
        lowest_acceptable_address: ULONGLONG,
        highest_acceptable_address: ULONGLONG,
        boundary_address_multiple: ULONGLONG,
        cache_type: ULONG,
    ) -> PVOID;
    pub fn MmFreeContiguousMemory(base_address: PVOID);
    
    // Virtual/Physical address conversion
    pub fn MmGetVirtualForPhysical(physical_address: ULONGLONG) -> PVOID;
    
    // Execution Control
    pub fn KeStallExecutionProcessor(microseconds: ULONG);
    
    // DPC (Deferred Procedure Call)
    pub fn KeInitializeDpc(dpc: *mut KDPC, deferred_routine: PKDEFERRED_ROUTINE, deferred_context: PVOID);
    pub fn KeInsertQueueDpc(dpc: *mut KDPC, system_argument1: PVOID, system_argument2: PVOID) -> BOOLEAN;
    pub fn KeRemoveQueueDpc(dpc: *mut KDPC) -> BOOLEAN;
    pub fn KeSetImportanceDpc(dpc: *mut KDPC, importance: ULONG);
    pub fn KeSetTargetProcessorDpc(dpc: *mut KDPC, number: CCHAR);
    
    // Timer
    pub fn KeInitializeTimer(timer: *mut KTIMER);
    pub fn KeInitializeTimerEx(timer: *mut KTIMER, timer_type: ULONG);
    pub fn KeSetTimer(timer: *mut KTIMER, due_time: LARGE_INTEGER, dpc: *mut KDPC) -> BOOLEAN;
    pub fn KeSetTimerEx(timer: *mut KTIMER, due_time: LARGE_INTEGER, period: LONG, dpc: *mut KDPC) -> BOOLEAN;
    pub fn KeCancelTimer(timer: *mut KTIMER) -> BOOLEAN;
    
    // Spinlock
    pub fn KeAcquireSpinLockAtDpcLevel(spin_lock: *mut ULONG_PTR);
    pub fn KeReleaseSpinLockFromDpcLevel(spin_lock: *mut ULONG_PTR);
    
    // Events
    pub fn KeInitializeEvent(event: *mut KEVENT, event_type: ULONG, initial_state: BOOLEAN);
    pub fn KeSetEvent(event: *mut KEVENT, increment: LONG, wait: BOOLEAN) -> LONG;
    pub fn KeClearEvent(event: *mut KEVENT);
    pub fn KeWaitForSingleObject(
        object: PVOID,
        wait_reason: ULONG,      // KWAIT_REASON enum
        wait_mode: CCHAR,        // KPROCESSOR_MODE
        alertable: BOOLEAN,
        timeout: *const LARGE_INTEGER,
    ) -> NTSTATUS;
    
    // IRP Stack Manipulation
    pub fn IoCopyCurrentIrpStackLocationToNext(irp: PIRP);
    
    // Note: IoSetCompletionRoutine is typically a macro in WDK, 
    // we implement it manually by setting IRP fields directly
    
    // Power Management
    pub fn PoCallDriver(device_object: PDEVICE_OBJECT, irp: PIRP) -> NTSTATUS;
    pub fn PoStartNextPowerIrp(irp: PIRP);
    
    // Interrupt Management
    pub fn IoConnectInterrupt(
        interrupt_object: *mut PKINTERRUPT,
        service_routine: PKSERVICE_ROUTINE,
        service_context: PVOID,
        spin_lock: PVOID,
        vector: ULONG,
        irql: UCHAR,
        synchronize_irql: UCHAR,
        interrupt_mode: ULONG,
        share_vector: BOOLEAN,
        processor_number: ULONG,
        floating_save: BOOLEAN,
    ) -> NTSTATUS;
    
    pub fn IoDisconnectInterrupt(interrupt_object: PKINTERRUPT);
}

/// KINTERRUPT pointer type
pub type PKINTERRUPT = *mut core::ffi::c_void;

/// Service routine type for IoConnectInterrupt
pub type PKSERVICE_ROUTINE = unsafe extern "win64" fn(
    interrupt: PKINTERRUPT,
    service_context: PVOID,
) -> BOOLEAN;

// =============================================================================
// DPC Structure (KDPC)
// =============================================================================

/// DPC Routine callback type
pub type PKDEFERRED_ROUTINE = unsafe extern "win64" fn(
    dpc: *mut KDPC,
    deferred_context: PVOID,
    system_argument1: PVOID,
    system_argument2: PVOID,
);

/// KDPC - Deferred Procedure Call object
#[repr(C)]
pub struct KDPC {
    pub r#type: UCHAR,
    pub importance: UCHAR,
    pub number: u16,
    pub dpc_list_entry: LIST_ENTRY,
    pub deferred_routine: PVOID, // PKDEFERRED_ROUTINE
    pub deferred_context: PVOID,
    pub system_argument1: PVOID,
    pub system_argument2: PVOID,
    pub dpc_data: PVOID,
}

impl KDPC {
    pub const fn zeroed() -> Self {
        Self {
            r#type: 0,
            importance: 0,
            number: 0,
            dpc_list_entry: LIST_ENTRY::zeroed(),
            deferred_routine: core::ptr::null_mut(),
            deferred_context: core::ptr::null_mut(),
            system_argument1: core::ptr::null_mut(),
            system_argument2: core::ptr::null_mut(),
            dpc_data: core::ptr::null_mut(),
        }
    }
}

/// LIST_ENTRY for DPC list
#[repr(C)]
pub struct LIST_ENTRY {
    pub flink: *mut LIST_ENTRY,
    pub blink: *mut LIST_ENTRY,
}

impl LIST_ENTRY {
    pub const fn zeroed() -> Self {
        Self {
            flink: core::ptr::null_mut(),
            blink: core::ptr::null_mut(),
        }
    }
}

// =============================================================================
// Timer Structure (KTIMER)
// =============================================================================

/// KTIMER - Kernel Timer object
#[repr(C)]
pub struct KTIMER {
    pub header: DISPATCHER_HEADER,
    pub due_time: ULARGE_INTEGER,
    pub timer_list_entry: LIST_ENTRY,
    pub dpc: *mut KDPC,
    pub processor: ULONG,
    pub period: ULONG,
}

impl KTIMER {
    pub const fn zeroed() -> Self {
        Self {
            header: DISPATCHER_HEADER::zeroed(),
            due_time: ULARGE_INTEGER { quad_part: 0 },
            timer_list_entry: LIST_ENTRY::zeroed(),
            dpc: core::ptr::null_mut(),
            processor: 0,
            period: 0,
        }
    }
}

/// Dispatcher header for synchronization objects
#[repr(C)]
pub struct DISPATCHER_HEADER {
    pub r#type: UCHAR,
    pub absolute: UCHAR,
    pub size: UCHAR,
    pub inserted: UCHAR,
    pub signal_state: LONG,
    pub wait_list_head: LIST_ENTRY,
}

impl DISPATCHER_HEADER {
    pub const fn zeroed() -> Self {
        Self {
            r#type: 0,
            absolute: 0,
            size: 0,
            inserted: 0,
            signal_state: 0,
            wait_list_head: LIST_ENTRY::zeroed(),
        }
    }
}

/// ULARGE_INTEGER union
#[repr(C)]
pub union ULARGE_INTEGER {
    pub quad_part: u64,
    pub parts: ULargeParts,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ULargeParts {
    pub low_part: ULONG,
    pub high_part: ULONG,
}

/// LARGE_INTEGER (signed 64-bit)
pub type LARGE_INTEGER = i64;

/// Timer type enum
pub const NOTIFICATION_TIMER: ULONG = 0;
pub const SYNCHRONIZATION_TIMER: ULONG = 1;

/// Event type enum
pub const NOTIFICATION_EVENT: ULONG = 0;
pub const SYNCHRONIZATION_EVENT: ULONG = 1;

// =============================================================================
// KEVENT - Kernel Event Object
// =============================================================================

/// KEVENT - Kernel event object (uses DISPATCHER_HEADER defined above)
#[repr(C)]
pub struct KEVENT {
    pub header: DISPATCHER_HEADER,
}

impl KEVENT {
    /// Create zeroed KEVENT (must be initialized with KeInitializeEvent)
    pub const fn zeroed() -> Self {
        Self {
            header: DISPATCHER_HEADER::zeroed(),
        }
    }
}

// =============================================================================
// IO_COMPLETION_ROUTINE type
// =============================================================================

/// IO completion routine callback type
pub type PIO_COMPLETION_ROUTINE = unsafe extern "win64" fn(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
    context: PVOID,
) -> NTSTATUS;

// =============================================================================
// Memory Caching Types (MEMORY_CACHING_TYPE enum)
// =============================================================================

pub const MM_NON_CACHED: ULONG = 0;           // MmNonCached
pub const MM_CACHED: ULONG = 1;               // MmCached  
pub const MM_WRITE_COMBINED: ULONG = 2;       // MmWriteCombined
pub const MM_HARDWARE_COHERENT_CACHED: ULONG = 3;  // MmHardwareCoherentCached
pub const MM_NON_CACHED_UNORDERED: ULONG = 4; // MmNonCachedUnordered
pub const MM_USDS_MAPPED: ULONG = 5;          // MmUSWCCached

