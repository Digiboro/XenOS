//! Imports from ntoskrnl

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

    pub fn IoCreateSymbolicLink(
        symbolic_link_name: *const UNICODE_STRING,
        device_name: *const UNICODE_STRING,
    ) -> NTSTATUS;

    pub fn IoDeleteSymbolicLink(
        symbolic_link_name: *const UNICODE_STRING,
    ) -> NTSTATUS;

    pub fn IoAttachDeviceToDeviceStack(
        source_device: PDEVICE_OBJECT,
        target_device: PDEVICE_OBJECT,
    ) -> PDEVICE_OBJECT;

    pub fn IoDetachDevice(target_device: PDEVICE_OBJECT);

    pub fn IoSkipCurrentIrpStackLocation(irp: PIRP);

    pub fn IoCallDriver(device_object: PDEVICE_OBJECT, irp: PIRP) -> NTSTATUS;

    pub fn IoCompleteRequest(irp: PIRP, priority_boost: CCHAR);

    pub fn IoGetCurrentIrpStackLocation(irp: PIRP) -> PIO_STACK_LOCATION;

    pub fn ExAllocatePoolWithTag(pool_type: ULONG, number_of_bytes: usize, tag: ULONG) -> PVOID;

    pub fn ExFreePoolWithTag(ptr: PVOID, tag: ULONG);

    pub fn DbgPrint(format: *const u8) -> ULONG;
}

// =============================================================================
// Local Rtl implementations (since not exported from ntoskrnl)
// =============================================================================

/// Initialize UNICODE_STRING from a null-terminated wide string
#[inline]
pub unsafe fn RtlInitUnicodeString(
    destination_string: *mut UNICODE_STRING,
    source_string: *const u16,
) {
    if source_string.is_null() {
        (*destination_string).length = 0;
        (*destination_string).maximum_length = 0;
        (*destination_string).buffer = core::ptr::null_mut();
    } else {
        // Count length (find null terminator)
        let mut len = 0usize;
        let mut ptr = source_string;
        while *ptr != 0 {
            len += 1;
            ptr = ptr.add(1);
        }
        
        let byte_len = len * 2;
        (*destination_string).length = byte_len as USHORT;
        (*destination_string).maximum_length = (byte_len + 2) as USHORT;
        (*destination_string).buffer = source_string as *mut u16;
    }
}

/// Compare memory regions, returns number of bytes that match
#[inline]
pub unsafe fn RtlCompareMemory(
    source1: *const core::ffi::c_void,
    source2: *const core::ffi::c_void,
    length: usize,
) -> usize {
    let s1 = source1 as *const u8;
    let s2 = source2 as *const u8;
    
    for i in 0..length {
        if *s1.add(i) != *s2.add(i) {
            return i;
        }
    }
    
    length
}
