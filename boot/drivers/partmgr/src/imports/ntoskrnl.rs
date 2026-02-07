//! ntoskrnl imports for PartMgr

use crate::types::*;

#[link(name = "ntoskrnl")]
unsafe extern "win64" {
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

    pub fn IoSkipCurrentIrpStackLocation(irp: PIRP);

    pub fn IoCallDriver(device_object: PDEVICE_OBJECT, irp: PIRP) -> NTSTATUS;

    pub fn IoCompleteRequest(irp: PIRP, priority_boost: CCHAR);
    
    pub fn IoAllocateIrp(stack_size: CCHAR, charge_quota: BOOLEAN) -> PIRP;
    
    pub fn IoFreeIrp(irp: PIRP);
    
    pub fn IoGetNextIrpStackLocation(irp: PIRP) -> PIO_STACK_LOCATION;

    pub fn ExAllocatePoolWithTag(pool_type: ULONG, number_of_bytes: usize, tag: ULONG) -> PVOID;

    pub fn ExFreePoolWithTag(ptr: PVOID, tag: ULONG);

    pub fn DbgPrint(format: *const u8) -> ULONG;
    
    pub fn ObReferenceObject(object: PVOID);
    
    pub fn ObDereferenceObject(object: PVOID);
    
    pub fn IoAllocateVpb(device_object: PDEVICE_OBJECT) -> PVPB;
    
    // Event functions
    pub fn KeInitializeEvent(
        event: *mut KEVENT,
        event_type: i32,
        initial_state: BOOLEAN,
    );
    
    pub fn KeSetEvent(
        event: *mut KEVENT,
        increment: LONG,
        wait: BOOLEAN,
    ) -> LONG;
    
    pub fn KeWaitForSingleObject(
        object: PVOID,
        wait_reason: u8,
        wait_mode: u8,
        alertable: BOOLEAN,
        timeout: *const LARGE_INTEGER,
    ) -> NTSTATUS;
}

// Event types
pub const NOTIFICATION_EVENT: i32 = 0;
pub const SYNCHRONIZATION_EVENT: i32 = 1;

// Wait reasons
pub const EXECUTIVE: u8 = 0;

// Wait modes  
pub const KERNEL_MODE: u8 = 0;

/// KEVENT - kernel event object
#[repr(C)]
pub struct KEVENT {
    pub header: DISPATCHER_HEADER,
}

/// DISPATCHER_HEADER
#[repr(C)]
pub struct DISPATCHER_HEADER {
    pub r#type: u8,
    pub size: u8,
    pub flags: u8,
    pub reserved: u8,
    pub signal_state: i32,
    pub wait_list_head: LIST_ENTRY,
}

/// LIST_ENTRY
#[repr(C)]
pub struct LIST_ENTRY {
    pub flink: *mut LIST_ENTRY,
    pub blink: *mut LIST_ENTRY,
}

/// LARGE_INTEGER
#[repr(C)]
pub union LARGE_INTEGER {
    pub quad_part: i64,
    pub parts: LargeParts,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct LargeParts {
    pub low_part: u32,
    pub high_part: i32,
}

/// Get current IRP stack location
#[inline]
pub unsafe fn IoGetCurrentIrpStackLocation(irp: PIRP) -> PIO_STACK_LOCATION {
    (*irp).tail.overlay.current_stack_location
}

/// IO completion routine type
pub type PIO_COMPLETION_ROUTINE = Option<
    unsafe extern "win64" fn(
        device_object: PDEVICE_OBJECT,
        irp: PIRP,
        context: PVOID,
    ) -> NTSTATUS,
>;

// Completion routine control flags (must match ntoskrnl!)
pub const SL_INVOKE_ON_SUCCESS: u8 = 0x40;
pub const SL_INVOKE_ON_ERROR: u8 = 0x80;
pub const SL_INVOKE_ON_CANCEL: u8 = 0x20;

/// Set completion routine on next stack location
#[inline]
pub unsafe fn IoSetCompletionRoutine(
    irp: PIRP,
    completion_routine: PIO_COMPLETION_ROUTINE,
    context: PVOID,
    invoke_on_success: bool,
    invoke_on_error: bool,
    invoke_on_cancel: bool,
) {
    let next = IoGetNextIrpStackLocation(irp);
    
    // Convert Option<fn> to raw pointer
    let routine_ptr = match completion_routine {
        Some(f) => f as *const () as PVOID,
        None => core::ptr::null_mut(),
    };
    
    (*next).completion_routine = routine_ptr;
    (*next).context = context;
    
    let mut control: u8 = 0;
    if invoke_on_success {
        control |= SL_INVOKE_ON_SUCCESS;
    }
    if invoke_on_error {
        control |= SL_INVOKE_ON_ERROR;
    }
    if invoke_on_cancel {
        control |= SL_INVOKE_ON_CANCEL;
    }
    (*next).control = control;
}

// STATUS_MORE_PROCESSING_REQUIRED - tells IoCompleteRequest to stop
pub const STATUS_MORE_PROCESSING_REQUIRED: NTSTATUS = 0xC0000016u32 as i32;
