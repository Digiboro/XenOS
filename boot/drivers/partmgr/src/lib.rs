//! Partition Manager Filter Driver (partmgr.sys)
//!
//! NT 6.1 compliant partition manager that:
//! - Filters on disk.sys FDO
//! - Reads MBR/GPT partition tables  
//! - Creates partition PDOs
//! - Handles BusRelations to expose partitions
//! - Implements offset translation for partition I/O

#![no_std]
#![allow(non_snake_case)]
#![allow(static_mut_refs)]
#![allow(unsafe_op_in_unsafe_fn)]

mod types;
mod imports;
mod partition;

use types::*;
use imports::ntoskrnl::*;
use core::ptr;

const PARTMGR_POOL_TAG: ULONG = 0x544D5250; // 'PRMT'
const MAX_PARTITIONS: usize = 16;

// =============================================================================
// Debug Output
// =============================================================================

#[cfg(feature = "storage-trace")]
unsafe fn pm_print(s: &str) {
    let mut buf = [0u8; 256];
    let len = s.len().min(255);
    for (i, &b) in s.as_bytes().iter().take(len).enumerate() {
        buf[i] = b;
    }
    buf[len] = 0;
    DbgPrint(buf.as_ptr());
}

#[cfg(not(feature = "storage-trace"))]
#[inline(always)]
unsafe fn pm_print(_s: &str) {}

#[cfg(feature = "storage-trace")]
unsafe fn pm_print_dec(value: u64) {
    let mut buf = [0u8; 32];
    let mut n = value;
    let mut i = 0;
    
    if n == 0 {
        buf[0] = b'0';
        i = 1;
    } else {
        while n > 0 {
            buf[i] = b'0' + (n % 10) as u8;
            n /= 10;
            i += 1;
        }
        buf[..i].reverse();
    }
    buf[i] = 0;
    DbgPrint(buf.as_ptr());
}

#[cfg(not(feature = "storage-trace"))]
#[inline(always)]
unsafe fn pm_print_dec(_value: u64) {}

#[cfg(feature = "storage-trace")]
unsafe fn pm_print_hex(value: u64) {
    const HEX_CHARS: &[u8] = b"0123456789ABCDEF";
    let mut buf = [0u8; 17];
    
    if value == 0 {
        buf[0] = b'0';
        buf[1] = 0;
        DbgPrint(buf.as_ptr());
        return;
    }
    
    let mut start = 0;
    for i in 0..16 {
        let nibble = ((value >> (60 - i * 4)) & 0xF) as usize;
        if nibble != 0 || start > 0 {
            buf[start] = HEX_CHARS[nibble];
            start += 1;
        }
    }
    buf[start] = 0;
    DbgPrint(buf.as_ptr());
}

#[cfg(not(feature = "storage-trace"))]
#[inline(always)]
unsafe fn pm_print_hex(_value: u64) {}

// =============================================================================
// Partition Info
// =============================================================================

/// Partition information
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PartitionInfo {
    pub partition_number: u32,
    pub partition_type: u8,
    pub bootable: bool,
    pub starting_lba: u64,
    pub sector_count: u64,
}

impl PartitionInfo {
    pub const fn new() -> Self {
        Self {
            partition_number: 0,
            partition_type: 0,
            bootable: false,
            starting_lba: 0,
            sector_count: 0,
        }
    }
}

// =============================================================================
// Device Extensions
// =============================================================================

const PARTMGR_FDO_SIGNATURE: ULONG = 0x52544D50; // 'PMTR'
const PARTMGR_PDO_SIGNATURE: ULONG = 0x4F445050; // 'PPDO'

// Global volume number counter for \Device\HarddiskVolumeX names
static mut HARDDISK_VOLUME_COUNTER: u32 = 0;

/// Filter DO extension (attaches to disk FDO)
#[repr(C)]
pub struct PARTMGR_FILTER_EXTENSION {
    pub signature: ULONG,
    pub device_object: PDEVICE_OBJECT,
    pub lower_device: PDEVICE_OBJECT,   // disk FDO
    pub physical_device: PDEVICE_OBJECT, // SCSI\Disk PDO
    
    // Disk geometry (from lower device)
    pub sector_count: u64,
    pub sector_size: u32,
    
    // Partition information
    pub partitions: [PartitionInfo; MAX_PARTITIONS],
    pub partition_count: usize,
    pub partition_pdos: [PDEVICE_OBJECT; MAX_PARTITIONS],
    
    // Device state
    pub started: u8,
}

/// Partition PDO extension
#[repr(C)]
pub struct PARTMGR_PARTITION_EXTENSION {
    pub signature: ULONG,
    pub device_object: PDEVICE_OBJECT,
    pub parent_filter: PDEVICE_OBJECT,  // Our filter DO
    
    // Partition info
    pub partition_number: u32,
    pub partition_type: u8,
    pub bootable: u8,
    pub starting_lba: u64,
    pub sector_count: u64,
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
        pm_print("[PARTMGR] DriverEntry\n");
        
        (*driver_object).major_function[IRP_MJ_PNP as usize] = Some(PartMgrDispatchPnp);
        (*driver_object).major_function[IRP_MJ_POWER as usize] = Some(PartMgrDispatchPower);
        (*driver_object).major_function[IRP_MJ_CREATE as usize] = Some(PartMgrDispatchCreate);
        (*driver_object).major_function[IRP_MJ_CLOSE as usize] = Some(PartMgrDispatchClose);
        (*driver_object).major_function[IRP_MJ_READ as usize] = Some(PartMgrDispatchReadWrite);
        (*driver_object).major_function[IRP_MJ_WRITE as usize] = Some(PartMgrDispatchReadWrite);
        (*driver_object).major_function[IRP_MJ_DEVICE_CONTROL as usize] = Some(PartMgrDispatchDeviceControl);

        let driver_ext = (*driver_object).driver_extension;
        if !driver_ext.is_null() {
            (*driver_ext).add_device = Some(PartMgrAddDevice);
        }

        pm_print("[PARTMGR] Driver loaded\n");
        STATUS_SUCCESS
    }
}

// =============================================================================
// AddDevice
// =============================================================================

/// AddDevice - creates filter DO and attaches to disk device stack
unsafe extern "win64" fn PartMgrAddDevice(
    driver_object: PDRIVER_OBJECT,
    physical_device_object: PDEVICE_OBJECT,
) -> NTSTATUS {
    pm_print("[PARTMGR] AddDevice called\n");
    
    // Create filter device object
    let mut filter_do: PDEVICE_OBJECT = ptr::null_mut();
    let ext_size = core::mem::size_of::<PARTMGR_FILTER_EXTENSION>() as ULONG;

    let status = IoCreateDevice(
        driver_object,
        ext_size,
        ptr::null(),
        FILE_DEVICE_DISK,
        0,
        0,
        &mut filter_do,
    );

    if status < 0 {
        pm_print("[PARTMGR] ERROR: IoCreateDevice failed\n");
        return status;
    }

    // Initialize extension
    let ext = (*filter_do).device_extension as *mut PARTMGR_FILTER_EXTENSION;
    ptr::write_bytes(ext, 0, 1);
    
    (*ext).signature = PARTMGR_FDO_SIGNATURE;
    (*ext).device_object = filter_do;
    (*ext).physical_device = physical_device_object;

    // Attach to device stack (above disk FDO)
    let lower = IoAttachDeviceToDeviceStack(filter_do, physical_device_object);
    if lower.is_null() {
        pm_print("[PARTMGR] ERROR: IoAttachDeviceToDeviceStack failed\n");
        IoDeleteDevice(filter_do);
        return STATUS_NO_SUCH_DEVICE;
    }

    (*ext).lower_device = lower;

    // Copy flags from lower device
    (*filter_do).flags |= (*lower).flags & (DO_DIRECT_IO | DO_BUFFERED_IO);
    (*filter_do).flags |= DO_POWER_PAGABLE;
    (*filter_do).flags &= !DO_DEVICE_INITIALIZING;

    pm_print("[PARTMGR] Filter attached to disk stack\n");
    STATUS_SUCCESS
}

// =============================================================================
// PnP Dispatch
// =============================================================================

unsafe extern "win64" fn PartMgrDispatchPnp(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    let ext = (*device_object).device_extension as *mut PARTMGR_FILTER_EXTENSION;

    // Check if this is our filter DO or a partition PDO
    if ext.is_null() {
        (*irp).io_status.status = STATUS_INVALID_DEVICE_REQUEST;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_INVALID_DEVICE_REQUEST;
    }
    
    let signature = (*ext).signature;
    
    if signature == PARTMGR_PDO_SIGNATURE {
        // This is a partition PDO
        return partmgr_pdo_pnp_dispatch(device_object, irp);
    }
    
    if signature != PARTMGR_FDO_SIGNATURE {
        (*irp).io_status.status = STATUS_INVALID_DEVICE_REQUEST;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_INVALID_DEVICE_REQUEST;
    }

    // This is our filter DO
    let stack = IoGetCurrentIrpStackLocation(irp);
    let minor = (*stack).minor_function;

    match minor {
        IRP_MN_START_DEVICE => partmgr_filter_start_device(device_object, irp, ext),
        IRP_MN_QUERY_DEVICE_RELATIONS => partmgr_filter_query_relations(device_object, irp, ext),
        IRP_MN_REMOVE_DEVICE => {
            // Pass down, then cleanup
            IoSkipCurrentIrpStackLocation(irp);
            let status = IoCallDriver((*ext).lower_device, irp);
            
            // Cleanup partition PDOs
            for i in 0..(*ext).partition_count {
                let pdo = (*ext).partition_pdos[i];
                if !pdo.is_null() {
                    IoDeleteDevice(pdo);
                }
            }
            
            IoDetachDevice((*ext).lower_device);
            IoDeleteDevice(device_object);
            status
        }
        _ => {
            // Pass down
            IoSkipCurrentIrpStackLocation(irp);
            IoCallDriver((*ext).lower_device, irp)
        }
    }
}

/// Handle START_DEVICE for filter
unsafe fn partmgr_filter_start_device(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
    ext: *mut PARTMGR_FILTER_EXTENSION,
) -> NTSTATUS {
    pm_print("[PARTMGR] START_DEVICE\n");
    
    // First pass IRP down to lower device
    IoSkipCurrentIrpStackLocation(irp);
    let status = IoCallDriver((*ext).lower_device, irp);
    
    if status < 0 {
        return status;
    }
    
    (*ext).started = 1;
    
    // Read disk geometry from lower device via IOCTL
    let (sector_count, sector_size) = partition::read_disk_geometry((*ext).lower_device);
    
    if sector_count == 0 {
        pm_print("[PARTMGR] WARNING: Could not read disk geometry\n");
        (*ext).sector_size = 512;
        (*ext).sector_count = 0;
    } else {
        (*ext).sector_count = sector_count;
        (*ext).sector_size = sector_size;
        
        pm_print("[PARTMGR] Disk: ");
        pm_print_dec(sector_count);
        pm_print(" sectors x ");
        pm_print_dec(sector_size as u64);
        pm_print(" bytes\n");
    }
    
    // Read partition table
    pm_print("[PARTMGR] Reading partition table...\n");
    let partition_list = partition::read_partition_table(
        (*ext).lower_device,
        (*ext).sector_size,
        (*ext).sector_count,
    );
    
    // Store partition info
    (*ext).partition_count = partition_list.count;
    for i in 0..partition_list.count {
        (*ext).partitions[i] = partition_list.partitions[i];
    }
    
    pm_print("[PARTMGR] Found ");
    pm_print_dec(partition_list.count as u64);
    pm_print(" partition(s)\n");
    
    // Create partition PDOs
    partmgr_create_partition_pdos(ext);
    
    status
}

/// Handle QUERY_DEVICE_RELATIONS for filter
unsafe fn partmgr_filter_query_relations(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
    ext: *mut PARTMGR_FILTER_EXTENSION,
) -> NTSTATUS {
    let stack = IoGetCurrentIrpStackLocation(irp);
    
    // Get relation type from parameters
    let params_ptr = &(*stack).parameters as *const _ as *const u8;
    let relation_type = core::ptr::read(params_ptr as *const ULONG);
    
    if relation_type != BUS_RELATIONS {
        // Not BusRelations - pass down
        IoSkipCurrentIrpStackLocation(irp);
        return IoCallDriver((*ext).lower_device, irp);
    }
    
    pm_print("[PARTMGR] QUERY_DEVICE_RELATIONS (BusRelations)\n");
    
    // Return our partition PDOs
    let pdo_count = (*ext).partition_count;
    
    if pdo_count == 0 {
        (*irp).io_status.status = STATUS_SUCCESS;
        (*irp).io_status.information = 0;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_SUCCESS;
    }
    
    // Allocate DEVICE_RELATIONS
    let relations_size = core::mem::size_of::<DEVICE_RELATIONS>()
        + (pdo_count.saturating_sub(1)) * core::mem::size_of::<PDEVICE_OBJECT>();
    
    let relations = ExAllocatePoolWithTag(NON_PAGED_POOL, relations_size, PARTMGR_POOL_TAG)
        as *mut DEVICE_RELATIONS;
    
    if relations.is_null() {
        (*irp).io_status.status = STATUS_INSUFFICIENT_RESOURCES;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_INSUFFICIENT_RESOURCES;
    }
    
    // Fill in partition PDOs
    let mut actual_count = 0;
    for i in 0..pdo_count {
        let pdo = (*ext).partition_pdos[i];
        if pdo.is_null() {
            continue;
        }
        *(*relations).objects.as_mut_ptr().add(actual_count) = pdo;
        ObReferenceObject(pdo as PVOID);
        actual_count += 1;
    }
    
    (*relations).count = actual_count as ULONG;
    
    pm_print("[PARTMGR] Returning ");
    pm_print_dec(actual_count as u64);
    pm_print(" partition PDO(s)\n");
    
    (*irp).io_status.status = STATUS_SUCCESS;
    (*irp).io_status.information = relations as ULONG_PTR;
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    STATUS_SUCCESS
}

/// Create PDOs for each partition
unsafe fn partmgr_create_partition_pdos(ext: *mut PARTMGR_FILTER_EXTENSION) {
    let driver = (*(*ext).device_object).driver_object;
    
    for i in 0..(*ext).partition_count {
        let part_info = &(*ext).partitions[i];
        
        // Create PDO for this partition with a name
        let mut pdo: PDEVICE_OBJECT = ptr::null_mut();
        let pdo_ext_size = core::mem::size_of::<PARTMGR_PARTITION_EXTENSION>() as ULONG;
        
        // Build device name: \Device\HarddiskVolumeN
        let volume_number = HARDDISK_VOLUME_COUNTER;
        HARDDISK_VOLUME_COUNTER += 1;
        
        let (device_name_buf, char_count) = build_harddisk_volume_name(volume_number);
        // Create UNICODE_STRING pointing to the buffer AFTER it's been placed on stack
        let device_name = UNICODE_STRING {
            length: (char_count * 2) as USHORT,
            maximum_length: (char_count * 2 + 2) as USHORT,
            buffer: device_name_buf.as_ptr() as *mut u16,
        };
        
        let status = IoCreateDevice(
            driver,
            pdo_ext_size,
            &device_name,
            FILE_DEVICE_DISK,
            0,
            0,
            &mut pdo,
        );
        
        if status < 0 || pdo.is_null() {
            pm_print("[PARTMGR] ERROR: Failed to create partition PDO\n");
            continue;
        }
        
        // Initialize PDO extension
        let pdo_ext = (*pdo).device_extension as *mut PARTMGR_PARTITION_EXTENSION;
        ptr::write_bytes(pdo_ext, 0, 1);
        
        (*pdo_ext).signature = PARTMGR_PDO_SIGNATURE;
        (*pdo_ext).device_object = pdo;
        (*pdo_ext).parent_filter = (*ext).device_object;
        (*pdo_ext).partition_number = part_info.partition_number;
        (*pdo_ext).partition_type = part_info.partition_type;
        (*pdo_ext).bootable = if part_info.bootable { 1 } else { 0 };
        (*pdo_ext).starting_lba = part_info.starting_lba;
        (*pdo_ext).sector_count = part_info.sector_count;
        
        (*pdo).flags |= DO_DIRECT_IO;
        (*pdo).flags &= !DO_DEVICE_INITIALIZING;
        
        // Allocate VPB for file system mounting
        let vpb = IoAllocateVpb(pdo);
        if vpb.is_null() {
            // Always output this error
            let msg = "[PARTMGR] ERROR: Failed to allocate VPB\n\0";
            DbgPrint(msg.as_ptr());
        } else {
            (*pdo).vpb = vpb;
            // Always output success for debugging
            let msg = "[PARTMGR] VPB allocated successfully\n\0";
            DbgPrint(msg.as_ptr());
        }
        
        (*ext).partition_pdos[i] = pdo;
        
        pm_print("[PARTMGR] Created PDO \\Device\\HarddiskVolume");
        pm_print_dec(volume_number as u64);
        pm_print(" for Partition");
        pm_print_dec(part_info.partition_number as u64);
        pm_print(": LBA ");
        pm_print_dec(part_info.starting_lba);
        pm_print(", ");
        pm_print_dec(part_info.sector_count);
        pm_print(" sectors\n");
    }
}

/// Build device name \Device\HarddiskVolumeN
/// Returns (buffer, char_count) - caller must create UNICODE_STRING from returned buffer
fn build_harddisk_volume_name(volume_number: u32) -> ([u16; 32], usize) {
    // \Device\HarddiskVolume + number
    // Max: \Device\HarddiskVolume4294967295 = 31 chars + null
    let mut buf = [0u16; 32];
    
    // "\Device\HarddiskVolume" prefix
    let prefix: &[u16] = &[
        0x5C, 0x44, 0x65, 0x76, 0x69, 0x63, 0x65, 0x5C,  // \Device\
        0x48, 0x61, 0x72, 0x64, 0x64, 0x69, 0x73, 0x6B,  // Harddisk
        0x56, 0x6F, 0x6C, 0x75, 0x6D, 0x65,              // Volume
    ];
    
    let mut pos = 0;
    for &c in prefix {
        buf[pos] = c;
        pos += 1;
    }
    
    // Append volume number
    if volume_number == 0 {
        buf[pos] = 0x30; // '0'
        pos += 1;
    } else {
        let mut num = volume_number;
        let start = pos;
        while num > 0 {
            buf[pos] = 0x30 + (num % 10) as u16;
            num /= 10;
            pos += 1;
        }
        // Reverse the digits
        let end = pos;
        let mut i = start;
        let mut j = end - 1;
        while i < j {
            buf.swap(i, j);
            i += 1;
            j -= 1;
        }
    }
    
    (buf, pos)
}

// =============================================================================
// Partition PDO PnP Dispatch
// =============================================================================

unsafe fn partmgr_pdo_pnp_dispatch(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    let ext = (*device_object).device_extension as *mut PARTMGR_PARTITION_EXTENSION;
    let stack = IoGetCurrentIrpStackLocation(irp);
    let minor = (*stack).minor_function;
    
    match minor {
        IRP_MN_START_DEVICE => {
            (*irp).io_status.status = STATUS_SUCCESS;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            STATUS_SUCCESS
        }
        IRP_MN_QUERY_ID => partmgr_pdo_query_id(device_object, irp, ext),
        IRP_MN_QUERY_DEVICE_RELATIONS => partmgr_pdo_query_relations(device_object, irp, ext),
        IRP_MN_QUERY_CAPABILITIES => partmgr_pdo_query_capabilities(device_object, irp, ext),
        IRP_MN_REMOVE_DEVICE => {
            (*irp).io_status.status = STATUS_SUCCESS;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            STATUS_SUCCESS
        }
        _ => {
            let status = (*irp).io_status.status;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            status
        }
    }
}

/// IRP_MN_QUERY_ID for partition PDO
unsafe fn partmgr_pdo_query_id(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
    ext: *mut PARTMGR_PARTITION_EXTENSION,
) -> NTSTATUS {
    let stack = IoGetCurrentIrpStackLocation(irp);
    
    let params_ptr = &(*stack).parameters as *const _ as *const u8;
    let id_type = core::ptr::read(params_ptr as *const ULONG);
    
    const BUS_QUERY_DEVICE_ID: ULONG = 0;
    const BUS_QUERY_HARDWARE_IDS: ULONG = 1;
    const BUS_QUERY_INSTANCE_ID: ULONG = 3;
    
    match id_type {
        BUS_QUERY_DEVICE_ID => {
            let device_id = b"STORAGE\\Partition\0";
            let buffer = ExAllocatePoolWithTag(
                NON_PAGED_POOL,
                device_id.len() * 2,
                PARTMGR_POOL_TAG
            ) as *mut u16;
            
            if buffer.is_null() {
                (*irp).io_status.status = STATUS_INSUFFICIENT_RESOURCES;
                IoCompleteRequest(irp, IO_NO_INCREMENT);
                return STATUS_INSUFFICIENT_RESOURCES;
            }
            
            for (i, &b) in device_id.iter().enumerate() {
                *buffer.add(i) = b as u16;
            }
            
            (*irp).io_status.status = STATUS_SUCCESS;
            (*irp).io_status.information = buffer as ULONG_PTR;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            STATUS_SUCCESS
        }
        BUS_QUERY_HARDWARE_IDS => {
            let hw_id = b"STORAGE\\Partition\0\0";
            let buffer = ExAllocatePoolWithTag(
                NON_PAGED_POOL,
                hw_id.len() * 2,
                PARTMGR_POOL_TAG
            ) as *mut u16;
            
            if buffer.is_null() {
                (*irp).io_status.status = STATUS_INSUFFICIENT_RESOURCES;
                IoCompleteRequest(irp, IO_NO_INCREMENT);
                return STATUS_INSUFFICIENT_RESOURCES;
            }
            
            for (i, &b) in hw_id.iter().enumerate() {
                *buffer.add(i) = b as u16;
            }
            
            (*irp).io_status.status = STATUS_SUCCESS;
            (*irp).io_status.information = buffer as ULONG_PTR;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            STATUS_SUCCESS
        }
        BUS_QUERY_INSTANCE_ID => {
            let partition_num = (*ext).partition_number;
            let mut instance_id = [0u8; 32];
            let mut idx = 0;
            
            for &b in b"Partition".iter() {
                instance_id[idx] = b;
                idx += 1;
            }
            
            // Convert number to string
            let num_str = format_u32(partition_num);
            for &b in num_str.iter() {
                if b == 0 { break; }
                instance_id[idx] = b;
                idx += 1;
            }
            instance_id[idx] = 0;
            idx += 1;
            
            let buffer = ExAllocatePoolWithTag(
                NON_PAGED_POOL,
                idx * 2,
                PARTMGR_POOL_TAG
            ) as *mut u16;
            
            if buffer.is_null() {
                (*irp).io_status.status = STATUS_INSUFFICIENT_RESOURCES;
                IoCompleteRequest(irp, IO_NO_INCREMENT);
                return STATUS_INSUFFICIENT_RESOURCES;
            }
            
            for (i, &b) in instance_id[..idx].iter().enumerate() {
                *buffer.add(i) = b as u16;
            }
            
            (*irp).io_status.status = STATUS_SUCCESS;
            (*irp).io_status.information = buffer as ULONG_PTR;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            STATUS_SUCCESS
        }
        _ => {
            (*irp).io_status.status = STATUS_NOT_SUPPORTED;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            STATUS_NOT_SUPPORTED
        }
    }
}

/// IRP_MN_QUERY_DEVICE_RELATIONS for partition PDO
unsafe fn partmgr_pdo_query_relations(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
    _ext: *mut PARTMGR_PARTITION_EXTENSION,
) -> NTSTATUS {
    let stack = IoGetCurrentIrpStackLocation(irp);
    
    let params_ptr = &(*stack).parameters as *const _ as *const u8;
    let relation_type = core::ptr::read(params_ptr as *const ULONG);
    
    const TARGET_DEVICE_RELATION: ULONG = 4;
    
    if relation_type != TARGET_DEVICE_RELATION {
        let status = (*irp).io_status.status;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return status;
    }
    
    // TargetDeviceRelation - return self
    let relations = ExAllocatePoolWithTag(
        NON_PAGED_POOL,
        core::mem::size_of::<DEVICE_RELATIONS>(),
        PARTMGR_POOL_TAG
    ) as *mut DEVICE_RELATIONS;
    
    if relations.is_null() {
        (*irp).io_status.status = STATUS_INSUFFICIENT_RESOURCES;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_INSUFFICIENT_RESOURCES;
    }
    
    (*relations).count = 1;
    *(*relations).objects.as_mut_ptr() = device_object;
    ObReferenceObject(device_object as PVOID);
    
    (*irp).io_status.status = STATUS_SUCCESS;
    (*irp).io_status.information = relations as ULONG_PTR;
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    STATUS_SUCCESS
}

/// IRP_MN_QUERY_CAPABILITIES for partition PDO
unsafe fn partmgr_pdo_query_capabilities(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
    _ext: *mut PARTMGR_PARTITION_EXTENSION,
) -> NTSTATUS {
    let stack = IoGetCurrentIrpStackLocation(irp);
    
    let params_ptr = &(*stack).parameters as *const _ as *const u8;
    let capabilities = core::ptr::read(params_ptr as *const PVOID) as *mut DEVICE_CAPABILITIES;
    
    if capabilities.is_null() {
        (*irp).io_status.status = STATUS_INVALID_PARAMETER;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_INVALID_PARAMETER;
    }
    
    (*capabilities).removable = 0;
    (*capabilities).eject_supported = 0;
    (*capabilities).unique_id = 1;
    (*capabilities).silent_install = 1;
    (*capabilities).raw_device_ok = 1;
    (*capabilities).surprise_removal_ok = 0;
    
    (*irp).io_status.status = STATUS_SUCCESS;
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    STATUS_SUCCESS
}

// =============================================================================
// Other Dispatch Routines
// =============================================================================

unsafe extern "win64" fn PartMgrDispatchPower(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    let ext = (*device_object).device_extension as *mut PARTMGR_FILTER_EXTENSION;
    
    if (*ext).signature == PARTMGR_PDO_SIGNATURE {
        // Partition PDO - complete
        (*irp).io_status.status = STATUS_SUCCESS;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_SUCCESS;
    }
    
    IoSkipCurrentIrpStackLocation(irp);
    IoCallDriver((*ext).lower_device, irp)
}

unsafe extern "win64" fn PartMgrDispatchCreate(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    let ext = (*device_object).device_extension as *mut PARTMGR_FILTER_EXTENSION;
    
    if (*ext).signature == PARTMGR_PDO_SIGNATURE {
        (*irp).io_status.status = STATUS_SUCCESS;
        (*irp).io_status.information = 0;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_SUCCESS;
    }
    
    IoSkipCurrentIrpStackLocation(irp);
    IoCallDriver((*ext).lower_device, irp)
}

unsafe extern "win64" fn PartMgrDispatchClose(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    let ext = (*device_object).device_extension as *mut PARTMGR_FILTER_EXTENSION;
    
    if (*ext).signature == PARTMGR_PDO_SIGNATURE {
        (*irp).io_status.status = STATUS_SUCCESS;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_SUCCESS;
    }
    
    IoSkipCurrentIrpStackLocation(irp);
    IoCallDriver((*ext).lower_device, irp)
}

unsafe extern "win64" fn PartMgrDispatchReadWrite(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    let ext = (*device_object).device_extension as *mut PARTMGR_FILTER_EXTENSION;
    
    if (*ext).signature == PARTMGR_PDO_SIGNATURE {
        // Partition PDO - do offset translation
        return partmgr_pdo_read_write(device_object, irp);
    }
    
    // Filter - pass down
    IoSkipCurrentIrpStackLocation(irp);
    IoCallDriver((*ext).lower_device, irp)
}

/// Handle READ/WRITE for partition PDO with offset translation
unsafe fn partmgr_pdo_read_write(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    pm_print("[PARTMGR] pdo_read_write entry\n");
    
    let ext = (*device_object).device_extension as *mut PARTMGR_PARTITION_EXTENSION;
    let parent_filter = (*ext).parent_filter;
    
    if parent_filter.is_null() {
        pm_print("[PARTMGR] ERROR: parent_filter is NULL\n");
        (*irp).io_status.status = STATUS_NO_SUCH_DEVICE;
        (*irp).io_status.information = 0;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_NO_SUCH_DEVICE;
    }
    
    let parent_ext = (*parent_filter).device_extension as *const PARTMGR_FILTER_EXTENSION;
    let sector_size = (*parent_ext).sector_size as u64;
    let stack = IoGetCurrentIrpStackLocation(irp);
    
    // Get parameters
    let params_ptr = &(*stack).parameters as *const _ as *const u8;
    let length = core::ptr::read(params_ptr as *const u32);
    let irp_offset = core::ptr::read(params_ptr.add(8) as *const i64);
    
    pm_print("[PARTMGR] pdo_rw: len=");
    pm_print_dec(length as u64);
    pm_print(" offset=");
    pm_print_dec(irp_offset as u64);
    pm_print(" sector_size=");
    pm_print_dec(sector_size);
    pm_print("\n");
    
    if irp_offset < 0 {
        pm_print("[PARTMGR] ERROR: negative offset\n");
        (*irp).io_status.status = STATUS_INVALID_PARAMETER;
        (*irp).io_status.information = 0;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_INVALID_PARAMETER;
    }
    
    // Bounds checking
    let partition_size_bytes = (*ext).sector_count * sector_size;
    let request_end = irp_offset as u64 + length as u64;
    
    pm_print("[PARTMGR] pdo_rw: part_size=");
    pm_print_dec(partition_size_bytes);
    pm_print(" req_end=");
    pm_print_dec(request_end);
    pm_print("\n");
    
    if request_end > partition_size_bytes {
        pm_print("[PARTMGR] ERROR: request exceeds partition size\n");
        (*irp).io_status.status = STATUS_INVALID_PARAMETER;
        (*irp).io_status.information = 0;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_INVALID_PARAMETER;
    }
    
    // Offset translation
    let partition_start_bytes = (*ext).starting_lba * sector_size;
    let actual_offset = partition_start_bytes + irp_offset as u64;
    
    pm_print("[PARTMGR] pdo_rw: actual_offset=");
    pm_print_dec(actual_offset);
    pm_print(" lower_device=0x");
    pm_print_hex((*parent_ext).lower_device as u64);
    pm_print("\n");
    
    // Modify IRP parameters
    let params_ptr_mut = &mut (*stack).parameters as *mut _ as *mut u8;
    core::ptr::write(params_ptr_mut.add(8) as *mut i64, actual_offset as i64);
    
    pm_print("[PARTMGR] pdo_rw: forwarding to lower_device...\n");
    
    // Forward to parent filter's lower device
    IoSkipCurrentIrpStackLocation(irp);
    let status = IoCallDriver((*parent_ext).lower_device, irp);
    
    pm_print("[PARTMGR] pdo_rw: lower_device returned 0x");
    pm_print_hex(status as u64);
    pm_print("\n");
    
    status
}

unsafe extern "win64" fn PartMgrDispatchDeviceControl(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    let ext = (*device_object).device_extension as *mut PARTMGR_FILTER_EXTENSION;
    
    if (*ext).signature == PARTMGR_PDO_SIGNATURE {
        // Partition PDO - handle partition IOCTLs
        return partmgr_pdo_device_control(device_object, irp);
    }
    
    // Filter - pass down
    IoSkipCurrentIrpStackLocation(irp);
    IoCallDriver((*ext).lower_device, irp)
}

/// Handle DEVICE_CONTROL for partition PDO
unsafe fn partmgr_pdo_device_control(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    let ext = (*device_object).device_extension as *mut PARTMGR_PARTITION_EXTENSION;
    let stack = IoGetCurrentIrpStackLocation(irp);
    
    let params_ptr = &(*stack).parameters as *const _ as *const u8;
    let io_control_code = core::ptr::read(params_ptr.add(8) as *const u32);
    
    match io_control_code {
        IOCTL_DISK_GET_PARTITION_INFO => {
            let parent_filter = (*ext).parent_filter;
            let parent_ext = (*parent_filter).device_extension as *const PARTMGR_FILTER_EXTENSION;
            let sector_size = (*parent_ext).sector_size as u64;
            
            let buffer = (*irp).associated_irp.system_buffer;
            if buffer.is_null() {
                (*irp).io_status.status = STATUS_INVALID_PARAMETER;
                (*irp).io_status.information = 0;
                IoCompleteRequest(irp, IO_NO_INCREMENT);
                return STATUS_INVALID_PARAMETER;
            }
            
            let part_info = buffer as *mut PARTITION_INFORMATION;
            (*part_info).starting_offset = ((*ext).starting_lba * sector_size) as i64;
            (*part_info).partition_length = ((*ext).sector_count * sector_size) as i64;
            (*part_info).hidden_sectors = 0;
            (*part_info).partition_number = (*ext).partition_number;
            (*part_info).partition_type = (*ext).partition_type;
            (*part_info).bootable = (*ext).bootable;
            (*part_info).recognized_partition = 1;
            (*part_info).rewrite_partition = 0;
            
            (*irp).io_status.status = STATUS_SUCCESS;
            (*irp).io_status.information = core::mem::size_of::<PARTITION_INFORMATION>();
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            STATUS_SUCCESS
        }
        IOCTL_DISK_GET_LENGTH_INFO => {
            let parent_filter = (*ext).parent_filter;
            let parent_ext = (*parent_filter).device_extension as *const PARTMGR_FILTER_EXTENSION;
            let sector_size = (*parent_ext).sector_size as u64;
            
            let buffer = (*irp).associated_irp.system_buffer;
            if buffer.is_null() {
                (*irp).io_status.status = STATUS_INVALID_PARAMETER;
                (*irp).io_status.information = 0;
                IoCompleteRequest(irp, IO_NO_INCREMENT);
                return STATUS_INVALID_PARAMETER;
            }
            
            let length_info = buffer as *mut GET_LENGTH_INFORMATION;
            (*length_info).length = ((*ext).sector_count * sector_size) as i64;
            
            (*irp).io_status.status = STATUS_SUCCESS;
            (*irp).io_status.information = core::mem::size_of::<GET_LENGTH_INFORMATION>();
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            STATUS_SUCCESS
        }
        IOCTL_DISK_GET_DRIVE_GEOMETRY => {
            let parent_filter = (*ext).parent_filter;
            let parent_ext = (*parent_filter).device_extension as *const PARTMGR_FILTER_EXTENSION;
            
            let buffer = (*irp).associated_irp.system_buffer;
            if buffer.is_null() {
                (*irp).io_status.status = STATUS_INVALID_PARAMETER;
                (*irp).io_status.information = 0;
                IoCompleteRequest(irp, IO_NO_INCREMENT);
                return STATUS_INVALID_PARAMETER;
            }
            
            // Return geometry based on partition size
            let sector_size = (*parent_ext).sector_size;
            let partition_sectors = (*ext).sector_count;
            
            // DISK_GEOMETRY structure:
            // Offset 0: Cylinders (i64)
            // Offset 8: MediaType (u32)
            // Offset 12: TracksPerCylinder (u32)
            // Offset 16: SectorsPerTrack (u32)
            // Offset 20: BytesPerSector (u32)
            let geom = buffer as *mut u8;
            
            // Calculate CHS values (simplified)
            let sectors_per_track: u32 = 63;
            let tracks_per_cyl: u32 = 255;
            let sectors_per_cyl = (sectors_per_track as u64) * (tracks_per_cyl as u64);
            let cylinders = partition_sectors / sectors_per_cyl;
            
            core::ptr::write(geom as *mut i64, cylinders as i64);
            core::ptr::write(geom.add(8) as *mut u32, 12); // FixedMedia
            core::ptr::write(geom.add(12) as *mut u32, tracks_per_cyl);
            core::ptr::write(geom.add(16) as *mut u32, sectors_per_track);
            core::ptr::write(geom.add(20) as *mut u32, sector_size);
            
            (*irp).io_status.status = STATUS_SUCCESS;
            (*irp).io_status.information = 24; // sizeof(DISK_GEOMETRY)
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            STATUS_SUCCESS
        }
        _ => {
            (*irp).io_status.status = STATUS_INVALID_DEVICE_REQUEST;
            (*irp).io_status.information = 0;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            STATUS_INVALID_DEVICE_REQUEST
        }
    }
}

// =============================================================================
// Helper Functions
// =============================================================================

fn format_u32(mut n: u32) -> [u8; 10] {
    let mut buf = [0u8; 10];
    let mut idx = 0;
    
    if n == 0 {
        buf[0] = b'0';
        return buf;
    }
    
    let mut temp = [0u8; 10];
    let mut temp_idx = 0;
    while n > 0 {
        temp[temp_idx] = b'0' + (n % 10) as u8;
        n /= 10;
        temp_idx += 1;
    }
    
    for i in 0..temp_idx {
        buf[idx] = temp[temp_idx - 1 - i];
        idx += 1;
    }
    
    buf
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
