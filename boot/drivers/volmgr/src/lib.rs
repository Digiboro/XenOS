//! Volume Manager - Basic Disk Volumes
//!
//! Creates volume device objects for partitions and manages volume GUID paths.
//! In NT 6.1, volmgr is a filter driver that sits above partition PDOs.

#![no_std]
#![allow(non_snake_case)]
#![allow(static_mut_refs)]

mod types;
mod imports;

use types::*;
use imports::ntoskrnl::*;
use imports::ntoskrnl::{
    KEVENT, IoSetCompletionRoutine, IoGetNextIrpStackLocation,
    IoAllocateIrp, IoFreeIrp, KeInitializeEvent, KeSetEvent,
    KeWaitForSingleObject, NOTIFICATION_EVENT, EXECUTIVE, KERNEL_MODE,
};
use core::ptr;

// =============================================================================
// Debug Output
// =============================================================================

macro_rules! vol_print {
    ($s:expr) => {
        #[cfg(feature = "storage-trace")]
        {
            let msg = concat!($s, "\0");
            unsafe { DbgPrint(msg.as_ptr()); }
        }
    };
}

#[allow(unused)]
fn vol_print_dec(n: u64) {
    #[cfg(feature = "storage-trace")]
    {
        let mut buf = [0u8; 21];
        let mut idx = 0;
        let mut num = n;
        
        if num == 0 {
            buf[0] = b'0';
            idx = 1;
        } else {
            let mut temp = [0u8; 20];
            let mut temp_idx = 0;
            while num > 0 {
                temp[temp_idx] = b'0' + (num % 10) as u8;
                num /= 10;
                temp_idx += 1;
            }
            for i in 0..temp_idx {
                buf[idx] = temp[temp_idx - 1 - i];
                idx += 1;
            }
        }
        buf[idx] = 0;
        unsafe { DbgPrint(buf.as_ptr()); }
    }
}

#[allow(unused)]
fn vol_print_hex(n: u64) {
    #[cfg(feature = "storage-trace")]
    {
        const HEX: &[u8] = b"0123456789ABCDEF";
        let mut buf = [0u8; 17];
        for i in 0..16 {
            buf[15 - i] = HEX[((n >> (i * 4)) & 0xF) as usize];
        }
        buf[16] = 0;
        unsafe { DbgPrint(buf.as_ptr()); }
    }
}

// =============================================================================
// Constants
// =============================================================================

const VOLMGR_FDO_SIGNATURE: ULONG = 0x464D4C56; // 'VLMF'
const VOLMGR_MAX_VOLUMES: usize = 32;

// Global volume counter for unique volume numbers
static mut VOLUME_COUNTER: u32 = 0;

// =============================================================================
// Device Extensions
// =============================================================================

/// Volume FDO extension - represents a volume device
#[repr(C)]
pub struct VOLMGR_EXTENSION {
    pub signature: ULONG,
    pub device_object: PDEVICE_OBJECT,
    pub lower_device: PDEVICE_OBJECT,   // Partition PDO
    pub physical_device: PDEVICE_OBJECT, // PDO we attached to
    
    // Volume information
    pub volume_number: u32,
    pub disk_number: u32,
    pub partition_offset: u64,    // Offset in bytes from start of disk
    pub partition_length: u64,    // Length in bytes
    
    // Volume state
    pub started: u8,
    pub online: u8,
    
    // Volume GUID
    pub volume_guid: GUID,
    
    // Device interface symbolic link
    pub interface_name: UNICODE_STRING,
    pub interface_name_buffer: [u16; 128],
}

// =============================================================================
// Driver Entry
// =============================================================================

#[unsafe(no_mangle)]
pub extern "win64" fn DriverEntry(
    driver_object: PDRIVER_OBJECT,
    _reg_path: *const UNICODE_STRING,
) -> NTSTATUS {
    unsafe {
        vol_print!("[VOLMGR] DriverEntry\n");
        
        (*driver_object).major_function[IRP_MJ_PNP as usize] = Some(VolMgrDispatchPnp);
        (*driver_object).major_function[IRP_MJ_POWER as usize] = Some(VolMgrDispatchPower);
        (*driver_object).major_function[IRP_MJ_CREATE as usize] = Some(VolMgrDispatchCreate);
        (*driver_object).major_function[IRP_MJ_CLOSE as usize] = Some(VolMgrDispatchClose);
        (*driver_object).major_function[IRP_MJ_READ as usize] = Some(VolMgrDispatchReadWrite);
        (*driver_object).major_function[IRP_MJ_WRITE as usize] = Some(VolMgrDispatchReadWrite);
        (*driver_object).major_function[IRP_MJ_DEVICE_CONTROL as usize] = Some(VolMgrDispatchDeviceControl);

        let driver_ext = (*driver_object).driver_extension;
        if !driver_ext.is_null() {
            (*driver_ext).add_device = Some(VolMgrAddDevice);
        }

        vol_print!("[VOLMGR] Driver loaded\n");
        STATUS_SUCCESS
    }
}

// =============================================================================
// AddDevice
// =============================================================================

unsafe extern "win64" fn VolMgrAddDevice(
    driver_object: PDRIVER_OBJECT,
    physical_device_object: PDEVICE_OBJECT,
) -> NTSTATUS {
    vol_print!("[VOLMGR] AddDevice called\n");
    
    let mut volume_do: PDEVICE_OBJECT = ptr::null_mut();
    let ext_size = core::mem::size_of::<VOLMGR_EXTENSION>() as ULONG;

    // Create volume device object
    let status = IoCreateDevice(
        driver_object,
        ext_size,
        ptr::null(),  // No name - we use device interface
        FILE_DEVICE_DISK,
        0,
        0,
        &mut volume_do,
    );

    if status < 0 {
        vol_print!("[VOLMGR] ERROR: IoCreateDevice failed\n");
        return status;
    }

    // Initialize extension
    let ext = (*volume_do).device_extension as *mut VOLMGR_EXTENSION;
    ptr::write_bytes(ext, 0, 1);
    
    (*ext).signature = VOLMGR_FDO_SIGNATURE;
    (*ext).device_object = volume_do;
    (*ext).physical_device = physical_device_object;
    
    // Assign volume number
    (*ext).volume_number = VOLUME_COUNTER;
    VOLUME_COUNTER += 1;
    
    // Generate a volume GUID (simplified - use volume number as part of GUID)
    (*ext).volume_guid = generate_volume_guid((*ext).volume_number);

    // Attach to device stack
    let lower = IoAttachDeviceToDeviceStack(volume_do, physical_device_object);
    if lower.is_null() {
        vol_print!("[VOLMGR] ERROR: IoAttachDeviceToDeviceStack failed\n");
        IoDeleteDevice(volume_do);
        return STATUS_NO_SUCH_DEVICE;
    }

    (*ext).lower_device = lower;
    (*ext).online = 1;  // Default online

    // Copy flags from lower device
    (*volume_do).flags |= (*lower).flags & (DO_DIRECT_IO | DO_BUFFERED_IO);
    (*volume_do).flags |= DO_POWER_PAGABLE;
    (*volume_do).flags &= !DO_DEVICE_INITIALIZING;

    vol_print!("[VOLMGR] Volume ");
    vol_print_dec((*ext).volume_number as u64);
    vol_print!(" created\n");
    
    STATUS_SUCCESS
}

/// Generate a volume GUID based on volume number
fn generate_volume_guid(volume_number: u32) -> GUID {
    // Generate a deterministic GUID for the volume
    // Format: {XXXXXXXX-0000-0000-0000-YYYYYYYYYYYY}
    // where X = volume-specific, Y = fixed
    GUID {
        data1: 0x12340000 + volume_number,
        data2: 0x5678,
        data3: 0x9ABC,
        data4: [0xDE, 0xF0, 0x00, 0x00, 0x00, 0x00, 0x00, volume_number as u8],
    }
}

// =============================================================================
// PnP Dispatch
// =============================================================================

unsafe extern "win64" fn VolMgrDispatchPnp(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    let ext = (*device_object).device_extension as *mut VOLMGR_EXTENSION;
    
    if (*ext).signature != VOLMGR_FDO_SIGNATURE {
        (*irp).io_status.status = STATUS_INVALID_DEVICE_REQUEST;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_INVALID_DEVICE_REQUEST;
    }
    
    let stack = IoGetCurrentIrpStackLocation(irp);
    let minor = (*stack).minor_function;
    
    match minor {
        IRP_MN_START_DEVICE => volmgr_start_device(device_object, irp, ext),
        IRP_MN_REMOVE_DEVICE => volmgr_remove_device(device_object, irp, ext),
        _ => {
            // Pass down other PnP IRPs
            IoSkipCurrentIrpStackLocation(irp);
            IoCallDriver((*ext).lower_device, irp)
        }
    }
}

unsafe fn volmgr_start_device(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
    ext: *mut VOLMGR_EXTENSION,
) -> NTSTATUS {
    vol_print!("[VOLMGR] START_DEVICE for volume ");
    vol_print_dec((*ext).volume_number as u64);
    vol_print!("\n");
    
    // Pass START_DEVICE down first
    IoSkipCurrentIrpStackLocation(irp);
    let status = IoCallDriver((*ext).lower_device, irp);
    
    if status >= 0 {
        (*ext).started = 1;
        
        // Query partition info from lower device
        query_partition_info(ext);
        
        // Register device interface for volume
        register_volume_interface(ext);
        
        vol_print!("[VOLMGR] Volume ");
        vol_print_dec((*ext).volume_number as u64);
        vol_print!(" started: offset=");
        vol_print_dec((*ext).partition_offset);
        vol_print!(" length=");
        vol_print_dec((*ext).partition_length);
        vol_print!("\n");
    }
    
    status
}

unsafe fn volmgr_remove_device(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
    ext: *mut VOLMGR_EXTENSION,
) -> NTSTATUS {
    vol_print!("[VOLMGR] REMOVE_DEVICE for volume ");
    vol_print_dec((*ext).volume_number as u64);
    vol_print!("\n");
    
    // Disable device interface
    if (*ext).interface_name.buffer != ptr::null_mut() {
        IoSetDeviceInterfaceState(&(*ext).interface_name, 0);
    }
    
    // Pass down
    IoSkipCurrentIrpStackLocation(irp);
    let status = IoCallDriver((*ext).lower_device, irp);
    
    // Detach and delete
    IoDetachDevice((*ext).lower_device);
    IoDeleteDevice(device_object);
    
    status
}

// =============================================================================
// Synchronous I/O Support
// =============================================================================

/// Context for synchronous I/O operations
#[repr(C)]
struct SyncIoContext {
    event: imports::ntoskrnl::KEVENT,
    io_status: IO_STATUS_BLOCK,
}

/// Completion routine for synchronous I/O
unsafe extern "win64" fn sync_io_completion(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
    context: PVOID,
) -> NTSTATUS {
    let ctx = context as *mut SyncIoContext;
    
    // Copy status from IRP to context
    (*ctx).io_status.status = (*irp).io_status.status;
    (*ctx).io_status.information = (*irp).io_status.information;
    
    // Signal the event
    KeSetEvent(&mut (*ctx).event, 0, 0);
    
    // Return STATUS_MORE_PROCESSING_REQUIRED to prevent IoCompleteRequest
    // from freeing the IRP - we want to free it ourselves
    STATUS_MORE_PROCESSING_REQUIRED
}

/// Query partition information from lower device via IOCTL
unsafe fn query_partition_info(ext: *mut VOLMGR_EXTENSION) {
    let lower_device = (*ext).lower_device;
    if lower_device.is_null() {
        vol_print!("[VOLMGR] ERROR: No lower device for partition query\n");
        return;
    }
    
    // Allocate context for synchronous I/O
    let ctx_size = core::mem::size_of::<SyncIoContext>();
    let ctx = ExAllocatePoolWithTag(NON_PAGED_POOL, ctx_size, VOLMGR_POOL_TAG) as *mut SyncIoContext;
    if ctx.is_null() {
        vol_print!("[VOLMGR] ERROR: Failed to allocate sync context\n");
        return;
    }
    ptr::write_bytes(ctx as *mut u8, 0, ctx_size);
    
    // Initialize event
    KeInitializeEvent(&mut (*ctx).event, NOTIFICATION_EVENT, 0);
    (*ctx).io_status.status = STATUS_UNSUCCESSFUL;
    (*ctx).io_status.information = 0;
    
    // Allocate buffer for PARTITION_INFORMATION
    let part_info_size = core::mem::size_of::<PARTITION_INFORMATION>();
    let part_info = ExAllocatePoolWithTag(NON_PAGED_POOL, part_info_size, VOLMGR_POOL_TAG) as *mut PARTITION_INFORMATION;
    if part_info.is_null() {
        ExFreePoolWithTag(ctx as PVOID, VOLMGR_POOL_TAG);
        vol_print!("[VOLMGR] ERROR: Failed to allocate partition info buffer\n");
        return;
    }
    ptr::write_bytes(part_info as *mut u8, 0, part_info_size);
    
    // Get stack size from lower device
    let stack_size = (*lower_device).stack_size;
    
    // Allocate IRP
    let irp = IoAllocateIrp(stack_size + 1, 0);
    if irp.is_null() {
        ExFreePoolWithTag(part_info as PVOID, VOLMGR_POOL_TAG);
        ExFreePoolWithTag(ctx as PVOID, VOLMGR_POOL_TAG);
        vol_print!("[VOLMGR] ERROR: Failed to allocate IRP\n");
        return;
    }
    
    // Setup IRP for IOCTL_DISK_GET_PARTITION_INFO
    let stack = IoGetNextIrpStackLocation(irp);
    (*stack).major_function = IRP_MJ_DEVICE_CONTROL;
    (*stack).minor_function = 0;
    (*stack).flags = 0;
    (*stack).device_object = lower_device;
    
    // Set IOCTL parameters
    // Parameters.DeviceIoControl: OutputBufferLength at +0, InputBufferLength at +4, IoControlCode at +8
    let params_ptr = &mut (*stack).parameters as *mut _ as *mut u8;
    ptr::write(params_ptr as *mut u32, part_info_size as u32);  // OutputBufferLength
    ptr::write(params_ptr.add(8) as *mut u32, IOCTL_DISK_GET_PARTITION_INFO);  // IoControlCode
    
    // Set buffer
    (*irp).associated_irp.system_buffer = part_info as PVOID;
    (*irp).user_buffer = part_info as PVOID;
    (*irp).io_status.status = STATUS_UNSUCCESSFUL;
    (*irp).io_status.information = 0;
    
    // Set completion routine
    IoSetCompletionRoutine(
        irp,
        Some(sync_io_completion),
        ctx as PVOID,
        true, true, true,
    );
    
    vol_print!("[VOLMGR] Querying partition info...\n");
    
    // Send IRP
    let status = IoCallDriver(lower_device, irp);
    
    // If pending, wait for completion
    if status == STATUS_PENDING {
        KeWaitForSingleObject(
            &mut (*ctx).event as *mut imports::ntoskrnl::KEVENT as PVOID,
            EXECUTIVE,
            KERNEL_MODE,
            0,
            ptr::null(),
        );
    }
    
    // Check final status
    let final_status = (*ctx).io_status.status;
    
    if final_status >= 0 {
        // Success - copy partition info
        (*ext).partition_offset = (*part_info).starting_offset as u64;
        (*ext).partition_length = (*part_info).partition_length as u64;
        (*ext).disk_number = (*part_info).partition_number; // Use partition number as disk number for now
        
        vol_print!("[VOLMGR] Partition info: offset=");
        vol_print_dec((*ext).partition_offset);
        vol_print!(" length=");
        vol_print_dec((*ext).partition_length);
        vol_print!("\n");
    } else {
        vol_print!("[VOLMGR] WARNING: Failed to query partition info, status=0x");
        vol_print_hex(final_status as u64);
        vol_print!("\n");
        
        // Set defaults
        (*ext).partition_offset = 0;
        (*ext).partition_length = 0;
        (*ext).disk_number = (*ext).volume_number;
    }
    
    // Cleanup
    IoFreeIrp(irp);
    ExFreePoolWithTag(part_info as PVOID, VOLMGR_POOL_TAG);
    ExFreePoolWithTag(ctx as PVOID, VOLMGR_POOL_TAG);
}

/// Register GUID_DEVINTERFACE_VOLUME for this volume
unsafe fn register_volume_interface(ext: *mut VOLMGR_EXTENSION) {
    // Initialize interface name buffer
    (*ext).interface_name.buffer = (*ext).interface_name_buffer.as_mut_ptr();
    (*ext).interface_name.length = 0;
    (*ext).interface_name.maximum_length = 256;
    
    let status = IoRegisterDeviceInterface(
        (*ext).physical_device,
        &GUID_DEVINTERFACE_VOLUME,
        ptr::null(),
        &mut (*ext).interface_name,
    );
    
    if status >= 0 {
        // Enable the interface
        let enable_status = IoSetDeviceInterfaceState(&(*ext).interface_name, 1);
        if enable_status >= 0 {
            vol_print!("[VOLMGR] Device interface registered\n");
        } else {
            vol_print!("[VOLMGR] WARNING: Failed to enable device interface\n");
        }
    } else {
        vol_print!("[VOLMGR] WARNING: Failed to register device interface\n");
    }
}

// =============================================================================
// Power Dispatch
// =============================================================================

unsafe extern "win64" fn VolMgrDispatchPower(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    let ext = (*device_object).device_extension as *mut VOLMGR_EXTENSION;
    IoSkipCurrentIrpStackLocation(irp);
    IoCallDriver((*ext).lower_device, irp)
}

// =============================================================================
// Create/Close Dispatch
// =============================================================================

unsafe extern "win64" fn VolMgrDispatchCreate(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    let ext = (*device_object).device_extension as *mut VOLMGR_EXTENSION;
    
    // Allow open if online
    if (*ext).online != 0 {
        (*irp).io_status.status = STATUS_SUCCESS;
        (*irp).io_status.information = 0;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        STATUS_SUCCESS
    } else {
        (*irp).io_status.status = STATUS_NO_SUCH_DEVICE;
        (*irp).io_status.information = 0;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        STATUS_NO_SUCH_DEVICE
    }
}

unsafe extern "win64" fn VolMgrDispatchClose(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    (*irp).io_status.status = STATUS_SUCCESS;
    (*irp).io_status.information = 0;
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    STATUS_SUCCESS
}

// =============================================================================
// Read/Write Dispatch
// =============================================================================

unsafe extern "win64" fn VolMgrDispatchReadWrite(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    let ext = (*device_object).device_extension as *mut VOLMGR_EXTENSION;
    
    // Check if online
    if (*ext).online == 0 {
        (*irp).io_status.status = STATUS_NO_SUCH_DEVICE;
        (*irp).io_status.information = 0;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_NO_SUCH_DEVICE;
    }
    
    // Pass through to partition
    IoSkipCurrentIrpStackLocation(irp);
    IoCallDriver((*ext).lower_device, irp)
}

// =============================================================================
// Device Control Dispatch
// =============================================================================

unsafe extern "win64" fn VolMgrDispatchDeviceControl(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    let ext = (*device_object).device_extension as *mut VOLMGR_EXTENSION;
    let stack = IoGetCurrentIrpStackLocation(irp);
    
    // Get IOCTL code from parameters
    let params_ptr = &(*stack).parameters as *const _ as *const u8;
    let io_control_code = core::ptr::read(params_ptr.add(8) as *const u32);
    
    match io_control_code {
        IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS => {
            volmgr_get_disk_extents(irp, ext)
        }
        IOCTL_VOLUME_ONLINE => {
            (*ext).online = 1;
            vol_print!("[VOLMGR] Volume online\n");
            (*irp).io_status.status = STATUS_SUCCESS;
            (*irp).io_status.information = 0;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            STATUS_SUCCESS
        }
        IOCTL_VOLUME_OFFLINE => {
            (*ext).online = 0;
            vol_print!("[VOLMGR] Volume offline\n");
            (*irp).io_status.status = STATUS_SUCCESS;
            (*irp).io_status.information = 0;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            STATUS_SUCCESS
        }
        IOCTL_VOLUME_IS_OFFLINE => {
            (*irp).io_status.status = if (*ext).online == 0 { 
                STATUS_SUCCESS 
            } else { 
                STATUS_INVALID_DEVICE_REQUEST 
            };
            (*irp).io_status.information = 0;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            (*irp).io_status.status
        }
        IOCTL_VOLUME_SUPPORTS_ONLINE_OFFLINE => {
            (*irp).io_status.status = STATUS_SUCCESS;
            (*irp).io_status.information = 0;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            STATUS_SUCCESS
        }
        // Pass through disk IOCTLs to partition
        IOCTL_DISK_GET_LENGTH_INFO |
        IOCTL_DISK_GET_DRIVE_GEOMETRY |
        IOCTL_DISK_GET_PARTITION_INFO => {
            IoSkipCurrentIrpStackLocation(irp);
            IoCallDriver((*ext).lower_device, irp)
        }
        _ => {
            // Unknown IOCTL - pass down
            IoSkipCurrentIrpStackLocation(irp);
            IoCallDriver((*ext).lower_device, irp)
        }
    }
}

/// Handle IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS
unsafe fn volmgr_get_disk_extents(
    irp: PIRP,
    ext: *mut VOLMGR_EXTENSION,
) -> NTSTATUS {
    let stack = IoGetCurrentIrpStackLocation(irp);
    let params_ptr = &(*stack).parameters as *const _ as *const u8;
    let output_buffer_length = core::ptr::read(params_ptr as *const u32);
    
    let required_size = core::mem::size_of::<VOLUME_DISK_EXTENTS>();
    
    if (output_buffer_length as usize) < required_size {
        (*irp).io_status.status = STATUS_BUFFER_TOO_SMALL;
        (*irp).io_status.information = required_size;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_BUFFER_TOO_SMALL;
    }
    
    let buffer = (*irp).associated_irp.system_buffer;
    if buffer.is_null() {
        (*irp).io_status.status = STATUS_INVALID_PARAMETER;
        (*irp).io_status.information = 0;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_INVALID_PARAMETER;
    }
    
    // Fill in disk extents
    let extents = buffer as *mut VOLUME_DISK_EXTENTS;
    (*extents).number_of_disk_extents = 1;
    (*extents).extents[0].disk_number = (*ext).disk_number;
    (*extents).extents[0].starting_offset = (*ext).partition_offset as i64;
    (*extents).extents[0].extent_length = (*ext).partition_length as i64;
    
    vol_print!("[VOLMGR] GET_VOLUME_DISK_EXTENTS: disk=");
    vol_print_dec((*ext).disk_number as u64);
    vol_print!("\n");
    
    (*irp).io_status.status = STATUS_SUCCESS;
    (*irp).io_status.information = required_size;
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    STATUS_SUCCESS
}

// =============================================================================
// Panic Handler
// =============================================================================

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

#[unsafe(no_mangle)]
pub static _fltused: i32 = 0;
