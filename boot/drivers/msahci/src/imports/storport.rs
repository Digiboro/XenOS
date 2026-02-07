//! Импорты из storport.sys
//!
//! Reference: WinDDK 7600.16385.1/inc/ddk/storport.h

use crate::types::*;

pub type NTSTATUS = i32;
pub type PDRIVER_OBJECT = *mut core::ffi::c_void;
pub type ULONGLONG = u64;
pub type USHORT = u16;
pub type UCHAR = u8;
pub type CCHAR = i8;

/// SCSI_ADAPTER_CONTROL_TYPE enumeration
pub type SCSI_ADAPTER_CONTROL_TYPE = ULONG;
pub const SCSI_ADAPTER_CONTROL_STOP: SCSI_ADAPTER_CONTROL_TYPE = 0;
pub const SCSI_ADAPTER_CONTROL_RESTART: SCSI_ADAPTER_CONTROL_TYPE = 1;

/// SCSI_ADAPTER_CONTROL_STATUS enumeration
pub type SCSI_ADAPTER_CONTROL_STATUS = ULONG;
pub const SCSI_ADAPTER_CONTROL_SUCCESS: SCSI_ADAPTER_CONTROL_STATUS = 0;
pub const SCSI_ADAPTER_CONTROL_UNSUCCESSFUL: SCSI_ADAPTER_CONTROL_STATUS = 1;

#[repr(C)]
pub struct UNICODE_STRING {
    pub length: u16,
    pub maximum_length: u16,
    pub buffer: *mut u16,
}

/// HW_INITIALIZATION_DATA structure
///
/// Reference: WinDDK 7600.16385.1/inc/ddk/storport.h (lines 4931-5073)
/// CRITICAL: Field order MUST match WinDDK exactly for ABI compatibility!
#[repr(C)]
pub struct HW_INITIALIZATION_DATA {
    /// Size of this structure
    pub HwInitializationDataSize: ULONG,
    /// Adapter interface type (PCIBus = 5, etc.)
    pub AdapterInterfaceType: ULONG,
    
    // Miniport driver routines - ORDER IS CRITICAL!
    pub HwInitialize: Option<PHW_INITIALIZE>,
    pub HwStartIo: Option<PHW_START_IO>,
    pub HwInterrupt: Option<PHW_INTERRUPT>,
    pub HwFindAdapter: Option<PHW_FIND_ADAPTER>,
    pub HwResetBus: Option<PHW_RESET_BUS>,
    pub HwDmaStarted: Option<PHW_DMA_STARTED>,
    pub HwAdapterState: Option<PHW_ADAPTER_STATE>,
    
    // Miniport driver resources
    pub DeviceExtensionSize: ULONG,
    pub SpecificLuExtensionSize: ULONG,
    pub SrbExtensionSize: ULONG,
    pub NumberOfAccessRanges: ULONG,
    pub Reserved: PVOID,
    
    // Flags
    pub MapBuffers: UCHAR,
    pub NeedPhysicalAddresses: BOOLEAN,
    pub TaggedQueuing: BOOLEAN,
    pub AutoRequestSense: BOOLEAN,
    pub MultipleRequestPerLu: BOOLEAN,
    pub ReceiveEvent: BOOLEAN,
    
    // Vendor/Device identification
    pub VendorIdLength: USHORT,
    pub VendorId: PVOID,
    pub PortVersionFlags: USHORT,
    pub DeviceIdLength: USHORT,
    pub DeviceId: PVOID,
    
    // Additional callbacks
    pub HwAdapterControl: Option<PHW_ADAPTER_CONTROL>,
    pub HwBuildIo: Option<PHW_BUILDIO>,
}

// Miniport callback function types - ORDER in HW_INITIALIZATION_DATA is critical!

pub type PHW_INITIALIZE = unsafe extern "win64" fn(device_extension: PVOID) -> BOOLEAN;

pub type PHW_START_IO = unsafe extern "win64" fn(device_extension: PVOID, srb: *mut SCSI_REQUEST_BLOCK) -> BOOLEAN;

pub type PHW_INTERRUPT = unsafe extern "win64" fn(device_extension: PVOID) -> BOOLEAN;

pub type PHW_FIND_ADAPTER = unsafe extern "win64" fn(
    device_extension: PVOID,
    hw_context: PVOID,
    bus_information: PVOID,
    argument_string: PVOID,
    config_info: *mut PORT_CONFIGURATION_INFORMATION,
    again: *mut BOOLEAN,
) -> ULONG;

pub type PHW_RESET_BUS = unsafe extern "win64" fn(device_extension: PVOID, path_id: ULONG) -> BOOLEAN;

pub type PHW_DMA_STARTED = unsafe extern "win64" fn(device_extension: PVOID);

pub type PHW_ADAPTER_STATE = unsafe extern "win64" fn(
    device_extension: PVOID,
    context: PVOID,
    save_state: BOOLEAN,
) -> SCSI_ADAPTER_CONTROL_STATUS;

pub type PHW_ADAPTER_CONTROL = unsafe extern "win64" fn(
    device_extension: PVOID,
    control_type: SCSI_ADAPTER_CONTROL_TYPE,
    parameters: PVOID,
) -> SCSI_ADAPTER_CONTROL_STATUS;

pub type PHW_BUILDIO = unsafe extern "win64" fn(device_extension: PVOID, srb: *mut SCSI_REQUEST_BLOCK) -> BOOLEAN;

/// Port Configuration Information
///
/// Reference: WinDDK 7600.16385.1/inc/ddk/storport.h (lines 4145-4400)
/// CRITICAL: Field order MUST match WinDDK exactly for ABI compatibility!
#[repr(C)]
pub struct PORT_CONFIGURATION_INFORMATION {
    pub Length: ULONG,
    pub SystemIoBusNumber: ULONG,
    pub AdapterInterfaceType: ULONG,
    pub BusInterruptLevel: ULONG,
    pub BusInterruptVector: ULONG,
    pub InterruptMode: ULONG,
    pub MaximumTransferLength: ULONG,
    pub NumberOfPhysicalBreaks: ULONG,
    pub DmaChannel: ULONG,
    pub DmaPort: ULONG,
    pub DmaWidth: ULONG,
    pub DmaSpeed: ULONG,
    pub AlignmentMask: ULONG,
    pub NumberOfAccessRanges: ULONG,
    pub AccessRanges: *mut ACCESS_RANGE,
    pub Reserved: PVOID,
    pub NumberOfBuses: UCHAR,
    pub InitiatorBusId: [CCHAR; 8],
    pub ScatterGather: BOOLEAN,
    pub Master: BOOLEAN,
    pub CachesData: BOOLEAN,
    pub AdapterScansDown: BOOLEAN,
    pub AtdiskPrimaryClaimed: BOOLEAN,
    pub AtdiskSecondaryClaimed: BOOLEAN,
    pub Dma32BitAddresses: BOOLEAN,
    pub DemandMode: BOOLEAN,
    pub MapBuffers: UCHAR,
    pub NeedPhysicalAddresses: BOOLEAN,
    pub TaggedQueuing: BOOLEAN,
    pub AutoRequestSense: BOOLEAN,
    pub MultipleRequestPerLu: BOOLEAN,
    pub ReceiveEvent: BOOLEAN,
    pub RealModeInitialized: BOOLEAN,
    pub BufferAccessScsiPortControlled: BOOLEAN,
    pub MaximumNumberOfTargets: UCHAR,
    pub ReservedUchars: [UCHAR; 2],
    pub SlotNumber: ULONG,
    pub BusInterruptLevel2: ULONG,
    pub BusInterruptVector2: ULONG,
    pub InterruptMode2: ULONG,
    pub DmaChannel2: ULONG,
    pub DmaPort2: ULONG,
    pub DmaWidth2: ULONG,
    pub DmaSpeed2: ULONG,
    pub DeviceExtensionSize: ULONG,
    pub SpecificLuExtensionSize: ULONG,
    pub SrbExtensionSize: ULONG,
    pub Dma64BitAddresses: UCHAR,
    pub ResetTargetSupported: BOOLEAN,
    pub MaximumNumberOfLogicalUnits: UCHAR,
    pub WmiDataProvider: BOOLEAN,
}

/// Access Range structure for MMIO/IO port resources
#[repr(C)]
pub struct ACCESS_RANGE {
    pub RangeStart: u64,
    pub RangeLength: ULONG,
    pub RangeInMemory: BOOLEAN,
}

// =============================================================================
// SCSI Request Block (SRB)
// =============================================================================
//
// Reference: WinDDK 7600.16385.1/inc/ddk/srb.h (lines 462-500)
// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/srb/ns-srb-_scsi_request_block
//

/// SCSI_REQUEST_BLOCK structure
///
/// Binary compatible with Windows NT 6.1 (Win7) x64 layout.
#[repr(C)]
pub struct SCSI_REQUEST_BLOCK {
    pub Length: u16,                        // offset 0x00
    pub Function: UCHAR,                    // offset 0x02
    pub SrbStatus: UCHAR,                   // offset 0x03
    pub ScsiStatus: UCHAR,                  // offset 0x04
    pub PathId: UCHAR,                      // offset 0x05
    pub TargetId: UCHAR,                    // offset 0x06
    pub Lun: UCHAR,                         // offset 0x07
    pub QueueTag: UCHAR,                    // offset 0x08
    pub QueueAction: UCHAR,                 // offset 0x09
    pub CdbLength: UCHAR,                   // offset 0x0A
    pub SenseInfoBufferLength: UCHAR,       // offset 0x0B
    pub SrbFlags: ULONG,                    // offset 0x0C
    pub DataTransferLength: ULONG,          // offset 0x10
    pub TimeOutValue: ULONG,                // offset 0x14
    pub DataBuffer: PVOID,                  // offset 0x18
    pub SenseInfoBuffer: PVOID,             // offset 0x20 (x64)
    pub NextSrb: *mut SCSI_REQUEST_BLOCK,   // offset 0x28 (x64)
    pub OriginalRequest: PVOID,             // offset 0x30 (x64)
    pub SrbExtension: PVOID,                // offset 0x38 (x64)
    pub InternalStatus: ULONG,              // offset 0x40 (x64) - union with QueueSortKey, LinkTimeoutValue
    pub Reserved: ULONG,                    // offset 0x44 (x64) - WIN64 only, for PVOID alignment
    pub Cdb: [UCHAR; 16],                   // offset 0x48 (x64)
}

// SRB_STATUS codes per WinDDK 7600.16385.1/inc/ddk/srb.h
pub const SRB_STATUS_PENDING: UCHAR = 0x00;
pub const SRB_STATUS_SUCCESS: UCHAR = 0x01;
pub const SRB_STATUS_ABORTED: UCHAR = 0x02;
pub const SRB_STATUS_ABORT_FAILED: UCHAR = 0x03;
pub const SRB_STATUS_ERROR: UCHAR = 0x04;
pub const SRB_STATUS_BUSY: UCHAR = 0x05;
pub const SRB_STATUS_INVALID_REQUEST: UCHAR = 0x06;
pub const SRB_STATUS_INVALID_PATH_ID: UCHAR = 0x07;
pub const SRB_STATUS_NO_DEVICE: UCHAR = 0x08;
pub const SRB_STATUS_TIMEOUT: UCHAR = 0x09;
pub const SRB_STATUS_SELECTION_TIMEOUT: UCHAR = 0x0A;
pub const SRB_STATUS_COMMAND_TIMEOUT: UCHAR = 0x0B;
pub const SRB_STATUS_MESSAGE_REJECTED: UCHAR = 0x0D;
pub const SRB_STATUS_BUS_RESET: UCHAR = 0x0E;
pub const SRB_STATUS_PARITY_ERROR: UCHAR = 0x0F;
pub const SRB_STATUS_REQUEST_SENSE_FAILED: UCHAR = 0x10;
pub const SRB_STATUS_NO_HBA: UCHAR = 0x11;
pub const SRB_STATUS_DATA_OVERRUN: UCHAR = 0x12;
pub const SRB_STATUS_UNEXPECTED_BUS_FREE: UCHAR = 0x13;
pub const SRB_STATUS_PHASE_SEQUENCE_FAILURE: UCHAR = 0x14;
pub const SRB_STATUS_BAD_SRB_BLOCK_LENGTH: UCHAR = 0x15;
pub const SRB_STATUS_REQUEST_FLUSHED: UCHAR = 0x16;
pub const SRB_STATUS_INVALID_LUN: UCHAR = 0x20;
pub const SRB_STATUS_INVALID_TARGET_ID: UCHAR = 0x21;
pub const SRB_STATUS_BAD_FUNCTION: UCHAR = 0x22;
pub const SRB_STATUS_ERROR_RECOVERY: UCHAR = 0x23;
pub const SRB_STATUS_NOT_POWERED: UCHAR = 0x24;
pub const SRB_STATUS_INTERNAL_ERROR: UCHAR = 0x30;

pub const SRB_FLAGS_DATA_IN: ULONG = 0x00000040;
pub const SRB_FLAGS_DATA_OUT: ULONG = 0x00000080;

// SCSI Operation Codes per WinDDK 7600.16385.1/inc/ddk/scsi.h
pub const SCSIOP_TEST_UNIT_READY: UCHAR = 0x00;
pub const SCSIOP_REQUEST_SENSE: UCHAR = 0x03;
pub const SCSIOP_INQUIRY: UCHAR = 0x12;
pub const SCSIOP_MODE_SENSE: UCHAR = 0x1A;
pub const SCSIOP_START_STOP_UNIT: UCHAR = 0x1B;
pub const SCSIOP_READ_CAPACITY: UCHAR = 0x25;
pub const SCSIOP_READ10: UCHAR = 0x28;
pub const SCSIOP_WRITE10: UCHAR = 0x2A;
pub const SCSIOP_SYNCHRONIZE_CACHE: UCHAR = 0x35;
pub const SCSIOP_READ16: UCHAR = 0x88;
pub const SCSIOP_WRITE16: UCHAR = 0x8A;
pub const SCSIOP_READ_CAPACITY16: UCHAR = 0x9E;
pub const SCSIOP_SYNCHRONIZE_CACHE16: UCHAR = 0x91;

pub const REQUEST_COMPLETE: ULONG = 0;
pub const NEXT_REQUEST: ULONG = 1;

#[link(name = "storport")]
unsafe extern "win64" {
    pub fn StorPortInitialize(
        driver_object: PDRIVER_OBJECT,
        registry_path: *const UNICODE_STRING,
        hw_init_data: *mut HW_INITIALIZATION_DATA,
    ) -> NTSTATUS;

    pub fn StorPortNotification(notification_type: ULONG, hw_device_extension: PVOID, ...);

    pub fn StorPortGetUncachedExtension(
        hw_device_extension: PVOID,
        config_info: *mut PORT_CONFIGURATION_INFORMATION,
        size: ULONG,
    ) -> PVOID;

    pub fn StorPortReadRegisterUlong(address: *mut ULONG) -> ULONG;
    pub fn StorPortWriteRegisterUlong(address: *mut ULONG, value: ULONG);
    pub fn StorPortReadRegisterUshort(address: *mut u16) -> u16;
    pub fn StorPortWriteRegisterUshort(address: *mut u16, value: u16);
    pub fn StorPortReadRegisterUchar(address: *mut UCHAR) -> UCHAR;
    pub fn StorPortWriteRegisterUchar(address: *mut UCHAR, value: UCHAR);

    pub fn StorPortStallExecution(delay: ULONG);

    pub fn StorPortGetPhysicalAddress(
        hw_device_extension: PVOID,
        srb: *mut SCSI_REQUEST_BLOCK,
        virtual_address: PVOID,
        length: *mut ULONG,
    ) -> u64;

    pub fn StorPortLogError(
        hw_device_extension: PVOID,
        srb: *mut SCSI_REQUEST_BLOCK,
        path_id: UCHAR,
        target_id: UCHAR,
        lun: UCHAR,
        error_code: ULONG,
        unique_id: ULONG,
    );
    
    pub fn StorPortGetDeviceBase(
        hw_device_extension: PVOID,
        bus_type: ULONG,
        system_io_bus_number: ULONG,
        io_address: ULONGLONG,
        number_of_bytes: ULONG,
        in_io_space: BOOLEAN,
    ) -> PVOID;
    
    pub fn StorPortFreeDeviceBase(
        hw_device_extension: PVOID,
        mapped_address: PVOID,
    );
    
    /// Get scatter-gather list for SRB
    /// Returns a PSTOR_SCATTER_GATHER_LIST which must be freed with StorPortPutScatterGatherList
    pub fn StorPortGetScatterGatherList(
        hw_device_extension: PVOID,
        srb: *mut SCSI_REQUEST_BLOCK,
    ) -> crate::types::PSTOR_SCATTER_GATHER_LIST;
    
    /// Free scatter-gather list allocated by StorPortGetScatterGatherList
    pub fn StorPortPutScatterGatherList(
        hw_device_extension: PVOID,
        sg_list: crate::types::PSTOR_SCATTER_GATHER_LIST,
        write_to_device: BOOLEAN,
    );
    
    // ntoskrnl exports
    pub fn DbgPrint(format: *const u8, ...);
}

