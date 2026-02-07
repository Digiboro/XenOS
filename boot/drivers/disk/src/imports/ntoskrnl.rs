//! Импорты из `ntoskrnl.exe` (PE/IAT) для `disk.sys`.

use crate::types::{
    BOOLEAN, CCHAR, NTSTATUS, PDEVICE_OBJECT, PDRIVER_OBJECT, PIO_STACK_LOCATION, PIRP, PVOID,
    ULONG, ULONG_PTR, UNICODE_STRING,
};

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

    pub fn IoCompleteRequest(irp: PIRP, priority_boost: CCHAR);

    pub fn IoGetCurrentIrpStackLocation(irp: PIRP) -> PIO_STACK_LOCATION;

    pub fn IoSkipCurrentIrpStackLocation(irp: PIRP);

    pub fn IoCallDriver(device_object: PDEVICE_OBJECT, irp: PIRP) -> NTSTATUS;

    pub fn IoMountVolume(device_object: PDEVICE_OBJECT, allow_raw_mount: BOOLEAN) -> NTSTATUS;

    pub fn ExAllocatePoolWithTag(pool_type: ULONG, size: ULONG_PTR, tag: ULONG) -> PVOID;

    pub fn ExFreePoolWithTag(ptr: PVOID, tag: ULONG);

    pub fn ObReferenceObject(object: PVOID);

    pub fn DbgPrint(format: *const u8) -> ULONG;

    pub fn IoCreateSymbolicLink(
        symbolic_link_name: *const UNICODE_STRING,
        device_name: *const UNICODE_STRING,
    ) -> NTSTATUS;

    pub fn IoDeleteSymbolicLink(symbolic_link_name: *const UNICODE_STRING) -> NTSTATUS;

    pub fn IoAllocateVpb(device_object: PDEVICE_OBJECT) -> PVOID;
    pub fn IoFreeVpb(vpb: PVOID);
    pub fn IoRegisterFileSystem(device_object: PDEVICE_OBJECT);
    pub fn IoUnregisterFileSystem(device_object: PDEVICE_OBJECT);
    pub fn IoVerifyVolume(device_object: PDEVICE_OBJECT, allow_raw_mount: u8) -> NTSTATUS;
}


