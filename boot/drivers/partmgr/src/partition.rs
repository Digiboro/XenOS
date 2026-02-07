//! Partition Table Parsing for PartMgr
//!
//! Reads MBR/GPT partition tables from disk using synchronous IRP_MJ_READ.
//! As a filter driver, we read through the lower device stack using standard I/O.

use crate::types::*;
use crate::imports::ntoskrnl::*;
use crate::{PartitionInfo, PARTMGR_POOL_TAG, pm_print, pm_print_dec, pm_print_hex};
use core::ptr;

pub const MAX_PARTITIONS: usize = 16;

/// Partition list result
pub struct PartitionList {
    pub partitions: [PartitionInfo; MAX_PARTITIONS],
    pub count: usize,
}

impl PartitionList {
    pub const fn new() -> Self {
        Self {
            partitions: [PartitionInfo::new(); MAX_PARTITIONS],
            count: 0,
        }
    }
}

// =============================================================================
// Synchronous I/O Context
// =============================================================================

/// Context for synchronous I/O completion
#[repr(C)]
struct SyncIoContext {
    event: KEVENT,
    io_status: IO_STATUS_BLOCK,
}

/// Completion routine for synchronous I/O
/// Returns STATUS_MORE_PROCESSING_REQUIRED to stop IoCompleteRequest from freeing IRP
unsafe extern "win64" fn sync_io_completion(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
    context: PVOID,
) -> NTSTATUS {
    let ctx = context as *mut SyncIoContext;
    
    if !ctx.is_null() {
        // Copy status to context
        (*ctx).io_status.status = (*irp).io_status.status;
        (*ctx).io_status.information = (*irp).io_status.information;
        
        // Signal event
        KeSetEvent(&mut (*ctx).event, 0, 0);
    }
    
    // Return this to prevent IoCompleteRequest from freeing the IRP
    STATUS_MORE_PROCESSING_REQUIRED
}

// =============================================================================
// Disk Geometry
// =============================================================================

/// Read disk geometry via IOCTL_DISK_GET_DRIVE_GEOMETRY
pub unsafe fn read_disk_geometry(lower_device: PDEVICE_OBJECT) -> (u64, u32) {
    pm_print("[PARTMGR/GEOM] Reading disk geometry via IOCTL\n");
    
    // Allocate buffer for DISK_GEOMETRY
    let geom_size = 24; // sizeof(DISK_GEOMETRY) = 24 bytes
    let buffer = ExAllocatePoolWithTag(NON_PAGED_POOL, geom_size, PARTMGR_POOL_TAG);
    if buffer.is_null() {
        pm_print("[PARTMGR/GEOM] ERROR: Cannot allocate buffer\n");
        return (0, 0);
    }
    
    ptr::write_bytes(buffer as *mut u8, 0, geom_size);
    
    // Send IOCTL to lower device
    let status = send_ioctl_synchronous(
        lower_device,
        IOCTL_DISK_GET_DRIVE_GEOMETRY,
        ptr::null(),
        0,
        buffer,
        geom_size as u32,
    );
    
    if status < 0 {
        pm_print("[PARTMGR/GEOM] IOCTL failed, status=0x");
        pm_print_hex(status as u64);
        pm_print("\n");
        ExFreePoolWithTag(buffer, PARTMGR_POOL_TAG);
        return (0, 0);
    }
    
    // Parse DISK_GEOMETRY structure:
    // Offset 0: Cylinders (i64)
    // Offset 8: MediaType (u32)
    // Offset 12: TracksPerCylinder (u32)
    // Offset 16: SectorsPerTrack (u32)
    // Offset 20: BytesPerSector (u32)
    let cylinders = ptr::read_unaligned(buffer as *const i64);
    let tracks_per_cyl = ptr::read_unaligned((buffer as usize + 12) as *const u32);
    let sectors_per_track = ptr::read_unaligned((buffer as usize + 16) as *const u32);
    let bytes_per_sector = ptr::read_unaligned((buffer as usize + 20) as *const u32);
    
    ExFreePoolWithTag(buffer, PARTMGR_POOL_TAG);
    
    let total_sectors = (cylinders as u64) * (tracks_per_cyl as u64) * (sectors_per_track as u64);
    
    pm_print("[PARTMGR/GEOM] Geometry: ");
    pm_print_dec(total_sectors);
    pm_print(" sectors x ");
    pm_print_dec(bytes_per_sector as u64);
    pm_print(" bytes\n");
    
    (total_sectors, bytes_per_sector)
}

// =============================================================================
// Partition Table Reading
// =============================================================================

/// Read partition table from disk (MBR or GPT)
pub unsafe fn read_partition_table(
    lower_device: PDEVICE_OBJECT,
    sector_size: u32,
    total_sectors: u64,
) -> PartitionList {
    pm_print("[PARTMGR/PART] Reading partition table...\n");
    
    let mut result = PartitionList::new();
    let mut temp_partitions = [PartitionInfo::new(); MAX_PARTITIONS];
    let mut partition_count = 0usize;
    
    // Use default sector size if not provided
    let sector_size = if sector_size == 0 { 512 } else { sector_size };
    
    // Try to read MBR first
    let mut is_gpt = false;
    
    match read_mbr(lower_device, sector_size, &mut temp_partitions) {
        Ok(count) => {
            pm_print("[PARTMGR/PART] MBR found, ");
            pm_print_dec(count as u64);
            pm_print(" partition(s)\n");
            
            if count > 0 {
                // Check for protective GPT (type 0xEE)
                if count == 1 && temp_partitions[0].partition_type == 0xEE {
                    pm_print("[PARTMGR/PART] Detected GPT protective MBR\n");
                    is_gpt = true;
                } else {
                    // Use MBR partitions
                    for i in 0..count {
                        if partition_count < MAX_PARTITIONS - 1 {
                            let mut part = temp_partitions[i];
                            part.partition_number = (partition_count + 1) as u32;
                            result.partitions[partition_count + 1] = part;
                            partition_count += 1;
                        }
                    }
                }
            }
        }
        Err(status) => {
            pm_print("[PARTMGR/PART] MBR read failed, status=0x");
            pm_print_hex(status as u64);
            pm_print(", trying GPT\n");
            is_gpt = true;
        }
    }
    
    // Read GPT if protective MBR detected
    if is_gpt {
        for part in temp_partitions.iter_mut() {
            *part = PartitionInfo::new();
        }
        
        match read_gpt(lower_device, sector_size, &mut temp_partitions) {
            Ok(count) => {
                pm_print("[PARTMGR/PART] GPT found, ");
                pm_print_dec(count as u64);
                pm_print(" partition(s)\n");
                
                partition_count = 0; // Reset MBR partitions
                for i in 0..count {
                    if partition_count < MAX_PARTITIONS - 1 {
                        let mut part = temp_partitions[i];
                        part.partition_number = (partition_count + 1) as u32;
                        result.partitions[partition_count + 1] = part;
                        partition_count += 1;
                    }
                }
            }
            Err(status) => {
                pm_print("[PARTMGR/PART] GPT read failed, status=0x");
                pm_print_hex(status as u64);
                pm_print("\n");
            }
        }
    }
    
    // Always create Partition0 (whole disk)
    result.partitions[0] = PartitionInfo {
        partition_number: 0,
        partition_type: 0,
        bootable: false,
        starting_lba: 0,
        sector_count: total_sectors,
    };
    
    result.count = partition_count + 1;
    
    pm_print("[PARTMGR/PART] Total: ");
    pm_print_dec(result.count as u64);
    pm_print(" partition(s)\n");
    
    result
}

// =============================================================================
// MBR Parsing
// =============================================================================

const MBR_SIGNATURE: u16 = 0xAA55;

unsafe fn read_mbr(
    lower_device: PDEVICE_OBJECT,
    sector_size: u32,
    partitions: &mut [PartitionInfo; MAX_PARTITIONS],
) -> Result<usize, NTSTATUS> {
    pm_print("[PARTMGR/MBR] Reading MBR from LBA 0\n");
    
    let buffer = ExAllocatePoolWithTag(NON_PAGED_POOL, sector_size as usize, PARTMGR_POOL_TAG);
    if buffer.is_null() {
        return Err(STATUS_INSUFFICIENT_RESOURCES);
    }
    
    ptr::write_bytes(buffer as *mut u8, 0, sector_size as usize);
    
    // Read sector 0 using synchronous IRP_MJ_READ
    let status = read_bytes_synchronous(lower_device, 0, sector_size, buffer);
    if status < 0 {
        pm_print("[PARTMGR/MBR] Read failed, status=0x");
        pm_print_hex(status as u64);
        pm_print("\n");
        ExFreePoolWithTag(buffer, PARTMGR_POOL_TAG);
        return Err(status);
    }
    
    // Check signature at offset 510
    let sig_ptr = (buffer as usize + 510) as *const u16;
    let signature = ptr::read_unaligned(sig_ptr);
    
    pm_print("[PARTMGR/MBR] Signature: 0x");
    pm_print_hex(signature as u64);
    pm_print("\n");
    
    if signature != MBR_SIGNATURE {
        pm_print("[PARTMGR/MBR] Invalid signature\n");
        ExFreePoolWithTag(buffer, PARTMGR_POOL_TAG);
        return Err(STATUS_UNSUCCESSFUL);
    }
    
    // Parse partition table at offset 446
    let table_ptr = (buffer as usize + 446) as *const u8;
    let mut count = 0usize;
    
    for i in 0..4 {
        let entry_ptr = table_ptr.add(i * 16);
        
        let boot_indicator = ptr::read(entry_ptr);
        let partition_type = ptr::read(entry_ptr.add(4));
        let starting_lba = ptr::read_unaligned(entry_ptr.add(8) as *const u32);
        let sector_count = ptr::read_unaligned(entry_ptr.add(12) as *const u32);
        
        pm_print("[PARTMGR/MBR] Entry ");
        pm_print_dec(i as u64);
        pm_print(": type=0x");
        pm_print_hex(partition_type as u64);
        pm_print(" LBA=");
        pm_print_dec(starting_lba as u64);
        pm_print("\n");
        
        if partition_type == 0 {
            continue;
        }
        
        // Skip extended partitions for now
        if partition_type == 0x05 || partition_type == 0x0F {
            continue;
        }
        
        if count < MAX_PARTITIONS {
            partitions[count] = PartitionInfo {
                partition_number: (count + 1) as u32,
                partition_type,
                bootable: boot_indicator == 0x80,
                starting_lba: starting_lba as u64,
                sector_count: sector_count as u64,
            };
            count += 1;
        }
    }
    
    ExFreePoolWithTag(buffer, PARTMGR_POOL_TAG);
    Ok(count)
}

// =============================================================================
// GPT Parsing
// =============================================================================

const GPT_SIGNATURE: u64 = 0x5452415020494645; // "EFI PART"

unsafe fn read_gpt(
    lower_device: PDEVICE_OBJECT,
    sector_size: u32,
    partitions: &mut [PartitionInfo; MAX_PARTITIONS],
) -> Result<usize, NTSTATUS> {
    pm_print("[PARTMGR/GPT] Reading GPT header from LBA 1\n");
    
    let buffer = ExAllocatePoolWithTag(NON_PAGED_POOL, sector_size as usize, PARTMGR_POOL_TAG);
    if buffer.is_null() {
        return Err(STATUS_INSUFFICIENT_RESOURCES);
    }
    
    ptr::write_bytes(buffer as *mut u8, 0, sector_size as usize);
    
    // Read GPT header (LBA 1)
    let status = read_bytes_synchronous(lower_device, sector_size as u64, sector_size, buffer);
    if status < 0 {
        pm_print("[PARTMGR/GPT] Read failed\n");
        ExFreePoolWithTag(buffer, PARTMGR_POOL_TAG);
        return Err(status);
    }
    
    // Check signature
    let sig = ptr::read_unaligned(buffer as *const u64);
    pm_print("[PARTMGR/GPT] Signature: 0x");
    pm_print_hex(sig);
    pm_print("\n");
    
    if sig != GPT_SIGNATURE {
        pm_print("[PARTMGR/GPT] Invalid signature\n");
        ExFreePoolWithTag(buffer, PARTMGR_POOL_TAG);
        return Err(STATUS_UNSUCCESSFUL);
    }
    
    // Read GPT parameters
    let partition_entry_lba = ptr::read_unaligned((buffer as usize + 72) as *const u64);
    let number_of_entries = ptr::read_unaligned((buffer as usize + 80) as *const u32);
    let size_of_entry = ptr::read_unaligned((buffer as usize + 84) as *const u32);
    
    pm_print("[PARTMGR/GPT] entries_lba=");
    pm_print_dec(partition_entry_lba);
    pm_print(" count=");
    pm_print_dec(number_of_entries as u64);
    pm_print("\n");
    
    ExFreePoolWithTag(buffer, PARTMGR_POOL_TAG);
    
    if partition_entry_lba < 2 || number_of_entries == 0 || size_of_entry < 128 {
        return Err(STATUS_UNSUCCESSFUL);
    }
    
    // Read partition entries
    let max_entries = core::cmp::min(number_of_entries as usize, 32);
    let entries_size = max_entries * size_of_entry as usize;
    
    let entries_buffer = ExAllocatePoolWithTag(NON_PAGED_POOL, entries_size, PARTMGR_POOL_TAG);
    if entries_buffer.is_null() {
        return Err(STATUS_INSUFFICIENT_RESOURCES);
    }
    
    ptr::write_bytes(entries_buffer as *mut u8, 0, entries_size);
    
    // Read entry sectors
    let entries_offset = partition_entry_lba * sector_size as u64;
    let status = read_bytes_synchronous(lower_device, entries_offset, entries_size as u32, entries_buffer);
    if status < 0 {
        ExFreePoolWithTag(entries_buffer, PARTMGR_POOL_TAG);
        return Err(status);
    }
    
    // Parse entries
    let mut count = 0usize;
    
    for i in 0..max_entries {
        if count >= MAX_PARTITIONS {
            break;
        }
        
        let entry_offset = i * size_of_entry as usize;
        let entry_ptr = (entries_buffer as usize + entry_offset) as *const u8;
        
        // Check if type GUID is empty
        let mut type_guid_empty = true;
        for j in 0..16 {
            if ptr::read(entry_ptr.add(j)) != 0 {
                type_guid_empty = false;
                break;
            }
        }
        
        if type_guid_empty {
            continue;
        }
        
        // Read LBAs
        let starting_lba = ptr::read_unaligned((entry_ptr as usize + 32) as *const u64);
        let ending_lba = ptr::read_unaligned((entry_ptr as usize + 40) as *const u64);
        
        let sector_count = if ending_lba >= starting_lba {
            ending_lba - starting_lba + 1
        } else {
            0
        };
        
        // Map GUID to MBR type
        let mut type_guid = [0u8; 16];
        for j in 0..16 {
            type_guid[j] = ptr::read(entry_ptr.add(j));
        }
        let mbr_type = guid_to_mbr_type(&type_guid);
        
        pm_print("[PARTMGR/GPT] Entry ");
        pm_print_dec(i as u64);
        pm_print(": LBA=");
        pm_print_dec(starting_lba);
        pm_print("-");
        pm_print_dec(ending_lba);
        pm_print(" type=0x");
        pm_print_hex(mbr_type as u64);
        pm_print("\n");
        
        partitions[count] = PartitionInfo {
            partition_number: (count + 1) as u32,
            partition_type: mbr_type,
            bootable: false,
            starting_lba,
            sector_count,
        };
        count += 1;
    }
    
    ExFreePoolWithTag(entries_buffer, PARTMGR_POOL_TAG);
    Ok(count)
}

/// Map GPT GUID to MBR type
fn guid_to_mbr_type(guid: &[u8; 16]) -> u8 {
    // EFI System Partition
    const EFI_SYSTEM: [u8; 16] = [
        0x28, 0x73, 0x2A, 0xC1, 0x1F, 0xF8, 0xD2, 0x11,
        0xBA, 0x4B, 0x00, 0xA0, 0xC9, 0x3E, 0xC9, 0x3B
    ];
    
    // Microsoft Basic Data
    const BASIC_DATA: [u8; 16] = [
        0xA2, 0xA0, 0xD0, 0xEB, 0xE5, 0xB9, 0x33, 0x44,
        0x87, 0xC0, 0x68, 0xB6, 0xB7, 0x26, 0x99, 0xC7
    ];
    
    if *guid == EFI_SYSTEM {
        0xEF
    } else if *guid == BASIC_DATA {
        0x07
    } else {
        0x07 // Default to Basic Data
    }
}

// =============================================================================
// Synchronous I/O using KEVENT
// =============================================================================

/// Read bytes from disk using synchronous IRP_MJ_READ with KEVENT
unsafe fn read_bytes_synchronous(
    lower_device: PDEVICE_OBJECT,
    byte_offset: u64,
    length: u32,
    buffer: PVOID,
) -> NTSTATUS {
    pm_print("[PARTMGR/IO] Reading ");
    pm_print_dec(length as u64);
    pm_print(" bytes from offset ");
    pm_print_dec(byte_offset);
    pm_print("\n");
    
    // Create sync context with event
    let ctx_size = core::mem::size_of::<SyncIoContext>();
    let ctx = ExAllocatePoolWithTag(NON_PAGED_POOL, ctx_size, PARTMGR_POOL_TAG) as *mut SyncIoContext;
    if ctx.is_null() {
        return STATUS_INSUFFICIENT_RESOURCES;
    }
    
    ptr::write_bytes(ctx as *mut u8, 0, ctx_size);
    
    // Initialize event (NotificationEvent, non-signaled)
    KeInitializeEvent(&mut (*ctx).event, NOTIFICATION_EVENT, 0);
    (*ctx).io_status.status = STATUS_UNSUCCESSFUL;
    (*ctx).io_status.information = 0;
    
    // Get stack size from lower device
    let stack_size = (*lower_device).stack_size;
    
    // Allocate IRP
    let irp = IoAllocateIrp(stack_size + 1, 0);
    if irp.is_null() {
        ExFreePoolWithTag(ctx as PVOID, PARTMGR_POOL_TAG);
        return STATUS_INSUFFICIENT_RESOURCES;
    }
    
    // Setup IRP for read
    let stack = IoGetNextIrpStackLocation(irp);
    (*stack).major_function = IRP_MJ_READ;
    (*stack).minor_function = 0;
    (*stack).flags = 0;
    (*stack).device_object = lower_device;
    
    // Set read parameters in union
    // Parameters.Read: Length at +0, Key at +4, ByteOffset at +8
    let params_ptr = &mut (*stack).parameters as *mut _ as *mut u8;
    ptr::write(params_ptr as *mut u32, length);  // Length
    ptr::write(params_ptr.add(8) as *mut i64, byte_offset as i64);  // ByteOffset
    
    // Set buffer - for DO_DIRECT_IO we use AssociatedIrp.SystemBuffer  
    (*irp).associated_irp.system_buffer = buffer;
    (*irp).user_buffer = buffer;
    
    // Initialize IoStatus
    (*irp).io_status.status = STATUS_UNSUCCESSFUL;
    (*irp).io_status.information = 0;
    
    // Set completion routine
    IoSetCompletionRoutine(
        irp,
        Some(sync_io_completion),
        ctx as PVOID,
        true,  // InvokeOnSuccess
        true,  // InvokeOnError
        true,  // InvokeOnCancel
    );
    
    pm_print("[PARTMGR/IO] Sending IRP to device\n");
    
    // Send IRP
    let status = IoCallDriver(lower_device, irp);
    
    pm_print("[PARTMGR/IO] IoCallDriver returned 0x");
    pm_print_hex(status as u64);
    pm_print("\n");
    
    // If pending, wait for completion
    if status == STATUS_PENDING {
        pm_print("[PARTMGR/IO] Waiting for completion...\n");
        KeWaitForSingleObject(
            &mut (*ctx).event as *mut KEVENT as PVOID,
            EXECUTIVE,
            KERNEL_MODE,
            0,  // not alertable
            ptr::null(),  // infinite timeout
        );
        pm_print("[PARTMGR/IO] Wait completed\n");
    }
    
    // Get final status from context (set by completion routine)
    let final_status = (*ctx).io_status.status;
    let bytes_read = (*ctx).io_status.information;
    
    pm_print("[PARTMGR/IO] Final status: 0x");
    pm_print_hex(final_status as u64);
    pm_print(", bytes: ");
    pm_print_dec(bytes_read as u64);
    pm_print("\n");
    
    // Free IRP (we own it because completion returned STATUS_MORE_PROCESSING_REQUIRED)
    IoFreeIrp(irp);
    
    // Free context
    ExFreePoolWithTag(ctx as PVOID, PARTMGR_POOL_TAG);
    
    final_status
}

/// Send IOCTL to lower device synchronously
unsafe fn send_ioctl_synchronous(
    lower_device: PDEVICE_OBJECT,
    ioctl_code: u32,
    _input_buffer: *const u8,
    _input_size: u32,
    output_buffer: PVOID,
    output_size: u32,
) -> NTSTATUS {
    pm_print("[PARTMGR/IO] Sending IOCTL 0x");
    pm_print_hex(ioctl_code as u64);
    pm_print("\n");
    
    // Create sync context with event
    let ctx_size = core::mem::size_of::<SyncIoContext>();
    let ctx = ExAllocatePoolWithTag(NON_PAGED_POOL, ctx_size, PARTMGR_POOL_TAG) as *mut SyncIoContext;
    if ctx.is_null() {
        return STATUS_INSUFFICIENT_RESOURCES;
    }
    
    ptr::write_bytes(ctx as *mut u8, 0, ctx_size);
    
    // Initialize event
    KeInitializeEvent(&mut (*ctx).event, NOTIFICATION_EVENT, 0);
    (*ctx).io_status.status = STATUS_UNSUCCESSFUL;
    (*ctx).io_status.information = 0;
    
    // Get stack size
    let stack_size = (*lower_device).stack_size;
    
    // Allocate IRP
    let irp = IoAllocateIrp(stack_size + 1, 0);
    if irp.is_null() {
        ExFreePoolWithTag(ctx as PVOID, PARTMGR_POOL_TAG);
        return STATUS_INSUFFICIENT_RESOURCES;
    }
    
    // Setup IRP for device control
    let stack = IoGetNextIrpStackLocation(irp);
    (*stack).major_function = IRP_MJ_DEVICE_CONTROL;
    (*stack).minor_function = 0;
    (*stack).flags = 0;
    (*stack).device_object = lower_device;
    
    // Set IOCTL parameters
    // Parameters.DeviceIoControl: OutputBufferLength at +0, InputBufferLength at +4, IoControlCode at +8
    let params_ptr = &mut (*stack).parameters as *mut _ as *mut u8;
    ptr::write(params_ptr as *mut u32, output_size);  // OutputBufferLength
    ptr::write(params_ptr.add(8) as *mut u32, ioctl_code);  // IoControlCode
    
    // Set buffer
    (*irp).associated_irp.system_buffer = output_buffer;
    (*irp).user_buffer = output_buffer;
    (*irp).io_status.status = STATUS_UNSUCCESSFUL;
    (*irp).io_status.information = 0;
    
    // Set completion routine
    IoSetCompletionRoutine(
        irp,
        Some(sync_io_completion),
        ctx as PVOID,
        true, true, true,
    );
    
    // Send IRP
    let status = IoCallDriver(lower_device, irp);
    
    // If pending, wait
    if status == STATUS_PENDING {
        KeWaitForSingleObject(
            &mut (*ctx).event as *mut KEVENT as PVOID,
            EXECUTIVE,
            KERNEL_MODE,
            0,
            ptr::null(),
        );
    }
    
    let final_status = (*ctx).io_status.status;
    
    // Free IRP and context
    IoFreeIrp(irp);
    ExFreePoolWithTag(ctx as PVOID, PARTMGR_POOL_TAG);
    
    final_status
}

// STATUS constant
const STATUS_PENDING: NTSTATUS = 0x00000103;
