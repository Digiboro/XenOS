//! Импорты из `ntoskrnl.exe` (PE/IAT).
//!
//! Эти символы должны существовать в export table `ntoskrnl.exe` (и быть в `.def`,
//! если драйвер линкуется через import library).

use crate::{
    BOOLEAN, CCHAR, NTSTATUS, PDEVICE_OBJECT, PDRIVER_OBJECT, PIRP, ULONG, UNICODE_STRING,
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

    pub fn IoCompleteRequest(irp: PIRP, priority_boost: CCHAR);
}


