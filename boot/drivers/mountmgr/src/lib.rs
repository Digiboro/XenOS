//! Mount Manager - Drive Letter and Mount Point Management
//!
//! Implements the Windows NT Mount Point Manager (mountmgr.sys).
//! Manages drive letters (C:, D:, ...), mount points, and volume GUIDs.
//!
//! Device: \Device\MountPointManager
//! DOS: \\.\MountPointManager

#![no_std]
#![allow(non_snake_case)]
#![allow(static_mut_refs)]

mod types;
mod imports;

use types::*;
use imports::ntoskrnl::*;
use core::ptr;

// =============================================================================
// Debug Output
// =============================================================================

macro_rules! mnt_print {
    ($s:expr) => {
        #[cfg(feature = "storage-trace")]
        {
            let msg = concat!($s, "\0");
            unsafe { DbgPrint(msg.as_ptr()); }
        }
    };
}

#[allow(unused)]
fn mnt_print_hex(n: u64) {
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

#[allow(unused)]
fn mnt_print_dec(n: u64) {
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

// =============================================================================
// Constants
// =============================================================================

const MOUNTMGR_SIGNATURE: ULONG = 0x4D746D67; // 'MtMg'
const MAX_MOUNT_POINTS: usize = 256;
const MAX_UNIQUE_ID_SIZE: usize = 512;
const MAX_DEVICE_NAME_SIZE: usize = 256;
const MAX_SYMBOLIC_LINK_SIZE: usize = 256;

// Reserved drive letters
const FIRST_DRIVE_LETTER: u8 = b'C';
const LAST_DRIVE_LETTER: u8 = b'Z';

// =============================================================================
// Mount Point Entry
// =============================================================================

/// Internal representation of a mount point
#[repr(C)]
struct MountPointEntry {
    /// Is this entry in use?
    in_use: bool,
    /// Keep links when device is offline
    keep_links_when_offline: bool,
    /// Device name (e.g., \Device\HarddiskVolume1)
    device_name: [u16; MAX_DEVICE_NAME_SIZE],
    device_name_length: USHORT,
    /// Symbolic link name (e.g., \DosDevices\C: or \??\Volume{guid})
    symbolic_link_name: [u16; MAX_SYMBOLIC_LINK_SIZE],
    symbolic_link_name_length: USHORT,
    /// Unique ID (volume signature)
    unique_id: [u8; MAX_UNIQUE_ID_SIZE],
    unique_id_length: USHORT,
}

impl MountPointEntry {
    const fn empty() -> Self {
        Self {
            in_use: false,
            keep_links_when_offline: false,
            device_name: [0; MAX_DEVICE_NAME_SIZE],
            device_name_length: 0,
            symbolic_link_name: [0; MAX_SYMBOLIC_LINK_SIZE],
            symbolic_link_name_length: 0,
            unique_id: [0; MAX_UNIQUE_ID_SIZE],
            unique_id_length: 0,
        }
    }
}

// =============================================================================
// MountMgr Device Extension
// =============================================================================

/// Control device extension
#[repr(C)]
struct MOUNTMGR_EXTENSION {
    signature: ULONG,
    device_object: PDEVICE_OBJECT,
    
    /// Auto-mount state
    auto_mount_enabled: bool,
    
    /// Epic number for change notifications (incremented on each change)
    epic_number: ULONG,
    
    /// Drive letters in use (bitmap for A-Z)
    drive_letters_in_use: u32,
    
    /// Mount points database
    mount_points: [MountPointEntry; MAX_MOUNT_POINTS],
    mount_point_count: usize,
}

// =============================================================================
// Global State
// =============================================================================

static mut MOUNTMGR_DEVICE: PDEVICE_OBJECT = ptr::null_mut();

// =============================================================================
// Driver Entry
// =============================================================================

#[unsafe(no_mangle)]
pub extern "win64" fn DriverEntry(
    driver_object: PDRIVER_OBJECT,
    _reg_path: *const UNICODE_STRING,
) -> NTSTATUS {
    unsafe {
        mnt_print!("[MOUNTMGR] DriverEntry\n");
        
        // Setup dispatch routines
        (*driver_object).major_function[IRP_MJ_CREATE as usize] = Some(MountMgrDispatchCreate);
        (*driver_object).major_function[IRP_MJ_CLOSE as usize] = Some(MountMgrDispatchClose);
        (*driver_object).major_function[IRP_MJ_CLEANUP as usize] = Some(MountMgrDispatchCleanup);
        (*driver_object).major_function[IRP_MJ_DEVICE_CONTROL as usize] = Some(MountMgrDispatchDeviceControl);
        (*driver_object).driver_unload = Some(MountMgrUnload);

        // Create control device \Device\MountPointManager
        let mut device_name = UNICODE_STRING::empty();
        RtlInitUnicodeString(&mut device_name, MOUNTMGR_DEVICE_NAME.as_ptr());

        let mut device: PDEVICE_OBJECT = ptr::null_mut();
        let ext_size = core::mem::size_of::<MOUNTMGR_EXTENSION>() as ULONG;
        
        let status = IoCreateDevice(
            driver_object,
            ext_size,
            &device_name,
            FILE_DEVICE_NETWORK, // MountMgr uses FILE_DEVICE_NETWORK
            0,
            0, // Not exclusive
            &mut device,
        );

        if status < 0 {
            mnt_print!("[MOUNTMGR] ERROR: IoCreateDevice failed\n");
            return status;
        }

        // Initialize extension
        let ext = (*device).device_extension as *mut MOUNTMGR_EXTENSION;
        ptr::write_bytes(ext, 0, 1);
        
        (*ext).signature = MOUNTMGR_SIGNATURE;
        (*ext).device_object = device;
        (*ext).auto_mount_enabled = true;
        (*ext).epic_number = 1;
        (*ext).drive_letters_in_use = 0;
        (*ext).mount_point_count = 0;
        
        // Initialize mount points array
        for i in 0..MAX_MOUNT_POINTS {
            (*ext).mount_points[i] = MountPointEntry::empty();
        }
        
        // Reserve A: and B: for floppy drives
        (*ext).drive_letters_in_use |= (1 << 0) | (1 << 1); // A and B

        // Create DOS symbolic link \??\MountPointManager
        let mut dos_name = UNICODE_STRING::empty();
        RtlInitUnicodeString(&mut dos_name, MOUNTMGR_DOS_DEVICE_NAME.as_ptr());
        
        let status = IoCreateSymbolicLink(&dos_name, &device_name);
        if status < 0 {
            mnt_print!("[MOUNTMGR] WARNING: IoCreateSymbolicLink failed\n");
            // Continue anyway - not critical
        }

        (*device).flags |= DO_BUFFERED_IO;
        (*device).flags &= !DO_DEVICE_INITIALIZING;

        MOUNTMGR_DEVICE = device;
        
        mnt_print!("[MOUNTMGR] Driver loaded, device created\n");
        STATUS_SUCCESS
    }
}

// =============================================================================
// Unload
// =============================================================================

unsafe extern "win64" fn MountMgrUnload(driver_object: PDRIVER_OBJECT) {
    mnt_print!("[MOUNTMGR] Unload\n");
    
    // Delete DOS symbolic link
    let mut dos_name = UNICODE_STRING::empty();
    RtlInitUnicodeString(&mut dos_name, MOUNTMGR_DOS_DEVICE_NAME.as_ptr());
    IoDeleteSymbolicLink(&dos_name);
    
    // Delete device
    if !MOUNTMGR_DEVICE.is_null() {
        IoDeleteDevice(MOUNTMGR_DEVICE);
        MOUNTMGR_DEVICE = ptr::null_mut();
    }
    
    let _ = driver_object;
}

// =============================================================================
// Create/Close/Cleanup Dispatch
// =============================================================================

unsafe extern "win64" fn MountMgrDispatchCreate(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    mnt_print!("[MOUNTMGR] Create\n");
    (*irp).io_status.status = STATUS_SUCCESS;
    (*irp).io_status.information = 0;
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    STATUS_SUCCESS
}

unsafe extern "win64" fn MountMgrDispatchClose(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    mnt_print!("[MOUNTMGR] Close\n");
    (*irp).io_status.status = STATUS_SUCCESS;
    (*irp).io_status.information = 0;
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    STATUS_SUCCESS
}

unsafe extern "win64" fn MountMgrDispatchCleanup(
    _device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    mnt_print!("[MOUNTMGR] Cleanup\n");
    (*irp).io_status.status = STATUS_SUCCESS;
    (*irp).io_status.information = 0;
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    STATUS_SUCCESS
}

// =============================================================================
// Device Control Dispatch
// =============================================================================

unsafe extern "win64" fn MountMgrDispatchDeviceControl(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    let ext = (*device_object).device_extension as *mut MOUNTMGR_EXTENSION;
    
    if (*ext).signature != MOUNTMGR_SIGNATURE {
        (*irp).io_status.status = STATUS_INVALID_DEVICE_REQUEST;
        (*irp).io_status.information = 0;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_INVALID_DEVICE_REQUEST;
    }
    
    let stack = IoGetCurrentIrpStackLocation(irp);
    
    // Get IOCTL parameters: DeviceIoControl structure
    // Offset 0: OutputBufferLength (ULONG)
    // Offset 4: InputBufferLength (ULONG) - but at different position
    // Offset 8: IoControlCode (ULONG)
    let params_ptr = &(*stack).parameters as *const _ as *const u8;
    let output_buffer_length = ptr::read(params_ptr as *const u32);
    let input_buffer_length = ptr::read(params_ptr.add(8) as *const u32);
    let io_control_code = ptr::read(params_ptr.add(12) as *const u32);
    
    let system_buffer = (*irp).associated_irp.system_buffer;
    
    mnt_print!("[MOUNTMGR] IOCTL: 0x");
    mnt_print_hex(io_control_code as u64);
    mnt_print!("\n");
    
    let status = match io_control_code {
        IOCTL_MOUNTMGR_QUERY_POINTS => {
            mountmgr_query_points(ext, irp, system_buffer, input_buffer_length, output_buffer_length)
        }
        IOCTL_MOUNTMGR_CREATE_POINT => {
            mountmgr_create_point(ext, irp, system_buffer, input_buffer_length)
        }
        IOCTL_MOUNTMGR_DELETE_POINTS => {
            mountmgr_delete_points(ext, irp, system_buffer, input_buffer_length, output_buffer_length, false)
        }
        IOCTL_MOUNTMGR_DELETE_POINTS_DBONLY => {
            mountmgr_delete_points(ext, irp, system_buffer, input_buffer_length, output_buffer_length, true)
        }
        IOCTL_MOUNTMGR_NEXT_DRIVE_LETTER => {
            mountmgr_next_drive_letter(ext, irp, system_buffer, input_buffer_length, output_buffer_length)
        }
        IOCTL_MOUNTMGR_VOLUME_ARRIVAL_NOTIFICATION => {
            mountmgr_volume_arrival(ext, irp, system_buffer, input_buffer_length)
        }
        IOCTL_MOUNTMGR_QUERY_DOS_VOLUME_PATH => {
            mountmgr_query_dos_volume_path(ext, irp, system_buffer, input_buffer_length, output_buffer_length, false)
        }
        IOCTL_MOUNTMGR_QUERY_DOS_VOLUME_PATHS => {
            mountmgr_query_dos_volume_path(ext, irp, system_buffer, input_buffer_length, output_buffer_length, true)
        }
        IOCTL_MOUNTMGR_CHANGE_NOTIFY => {
            mountmgr_change_notify(ext, irp, system_buffer, output_buffer_length)
        }
        IOCTL_MOUNTMGR_QUERY_AUTO_MOUNT => {
            mountmgr_query_auto_mount(ext, irp, system_buffer, output_buffer_length)
        }
        IOCTL_MOUNTMGR_SET_AUTO_MOUNT => {
            mountmgr_set_auto_mount(ext, irp, system_buffer, input_buffer_length)
        }
        IOCTL_MOUNTMGR_KEEP_LINKS_WHEN_OFFLINE => {
            mountmgr_keep_links_offline(ext, irp, system_buffer, input_buffer_length)
        }
        IOCTL_MOUNTMGR_CHECK_UNPROCESSED_VOLUMES => {
            // No-op for now - just succeed
            (*irp).io_status.status = STATUS_SUCCESS;
            (*irp).io_status.information = 0;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            STATUS_SUCCESS
        }
        IOCTL_MOUNTMGR_AUTO_DL_ASSIGNMENTS => {
            // Trigger automatic drive letter assignments
            // For now just succeed
            (*irp).io_status.status = STATUS_SUCCESS;
            (*irp).io_status.information = 0;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            STATUS_SUCCESS
        }
        IOCTL_MOUNTMGR_VOLUME_MOUNT_POINT_CREATED |
        IOCTL_MOUNTMGR_VOLUME_MOUNT_POINT_DELETED => {
            // These are notifications from the file system
            // Just acknowledge for now
            (*irp).io_status.status = STATUS_SUCCESS;
            (*irp).io_status.information = 0;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            STATUS_SUCCESS
        }
        IOCTL_MOUNTMGR_SCRUB_REGISTRY => {
            // Clean up stale entries - no-op for now
            (*irp).io_status.status = STATUS_SUCCESS;
            (*irp).io_status.information = 0;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            STATUS_SUCCESS
        }
        _ => {
            mnt_print!("[MOUNTMGR] Unknown IOCTL\n");
            (*irp).io_status.status = STATUS_INVALID_DEVICE_REQUEST;
            (*irp).io_status.information = 0;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            STATUS_INVALID_DEVICE_REQUEST
        }
    };
    
    status
}

// =============================================================================
// IOCTL Handlers
// =============================================================================

/// IOCTL_MOUNTMGR_QUERY_POINTS
/// Query mount points matching the given criteria
unsafe fn mountmgr_query_points(
    ext: *mut MOUNTMGR_EXTENSION,
    irp: PIRP,
    buffer: PVOID,
    input_len: u32,
    output_len: u32,
) -> NTSTATUS {
    mnt_print!("[MOUNTMGR] QUERY_POINTS\n");
    
    if buffer.is_null() {
        (*irp).io_status.status = STATUS_INVALID_PARAMETER;
        (*irp).io_status.information = 0;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_INVALID_PARAMETER;
    }
    
    // Parse input filter
    let filter = if input_len >= core::mem::size_of::<MOUNTMGR_MOUNT_POINT>() as u32 {
        Some(&*(buffer as *const MOUNTMGR_MOUNT_POINT))
    } else {
        None
    };
    
    // Count matching mount points and calculate required size
    let mut match_count = 0usize;
    let mut data_size = 0usize;
    
    for i in 0..(*ext).mount_point_count {
        let entry = &(*ext).mount_points[i];
        if !entry.in_use {
            continue;
        }
        
        if matches_filter(entry, filter, buffer as *const u8) {
            match_count += 1;
            data_size += entry.symbolic_link_name_length as usize;
            data_size += entry.unique_id_length as usize;
            data_size += entry.device_name_length as usize;
        }
    }
    
    // Calculate total required size
    let header_size = core::mem::size_of::<MOUNTMGR_MOUNT_POINTS>();
    let entries_size = match_count.saturating_sub(1) * core::mem::size_of::<MOUNTMGR_MOUNT_POINT>();
    let total_size = header_size + entries_size + data_size;
    
    // Check output buffer size
    if (output_len as usize) < core::mem::size_of::<MOUNTMGR_MOUNT_POINTS>() {
        (*irp).io_status.status = STATUS_BUFFER_TOO_SMALL;
        (*irp).io_status.information = total_size;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_BUFFER_TOO_SMALL;
    }
    
    // Fill output
    let output = buffer as *mut MOUNTMGR_MOUNT_POINTS;
    (*output).size = total_size as ULONG;
    (*output).number_of_mount_points = match_count as ULONG;
    
    if (output_len as usize) < total_size {
        // Return partial info with correct size
        (*irp).io_status.status = STATUS_BUFFER_OVERFLOW;
        (*irp).io_status.information = core::mem::size_of::<MOUNTMGR_MOUNT_POINTS>();
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_BUFFER_OVERFLOW;
    }
    
    // Fill in mount point entries
    let mut entry_idx = 0usize;
    let mut data_offset = header_size + entries_size;
    let output_base = buffer as *mut u8;
    
    for i in 0..(*ext).mount_point_count {
        let entry = &(*ext).mount_points[i];
        if !entry.in_use || !matches_filter(entry, filter, buffer as *const u8) {
            continue;
        }
        
        let mp = &mut (*output).mount_points.as_mut_ptr().add(entry_idx).as_mut().unwrap();
        
        // Symbolic link name
        mp.symbolic_link_name_offset = data_offset as ULONG;
        mp.symbolic_link_name_length = entry.symbolic_link_name_length;
        ptr::copy_nonoverlapping(
            entry.symbolic_link_name.as_ptr(),
            output_base.add(data_offset) as *mut u16,
            (entry.symbolic_link_name_length as usize) / 2,
        );
        data_offset += entry.symbolic_link_name_length as usize;
        
        // Unique ID
        mp.unique_id_offset = data_offset as ULONG;
        mp.unique_id_length = entry.unique_id_length;
        ptr::copy_nonoverlapping(
            entry.unique_id.as_ptr(),
            output_base.add(data_offset),
            entry.unique_id_length as usize,
        );
        data_offset += entry.unique_id_length as usize;
        
        // Device name
        mp.device_name_offset = data_offset as ULONG;
        mp.device_name_length = entry.device_name_length;
        ptr::copy_nonoverlapping(
            entry.device_name.as_ptr(),
            output_base.add(data_offset) as *mut u16,
            (entry.device_name_length as usize) / 2,
        );
        data_offset += entry.device_name_length as usize;
        
        entry_idx += 1;
    }
    
    (*irp).io_status.status = STATUS_SUCCESS;
    (*irp).io_status.information = total_size;
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    STATUS_SUCCESS
}

/// Check if mount point entry matches the filter
unsafe fn matches_filter(
    entry: &MountPointEntry,
    filter: Option<&MOUNTMGR_MOUNT_POINT>,
    buffer_base: *const u8,
) -> bool {
    let filter = match filter {
        Some(f) => f,
        None => return true, // No filter = match all
    };
    
    // Check symbolic link name filter
    if filter.symbolic_link_name_length > 0 {
        let filter_name = buffer_base.add(filter.symbolic_link_name_offset as usize) as *const u16;
        let filter_len = filter.symbolic_link_name_length as usize;
        
        if entry.symbolic_link_name_length as usize != filter_len {
            return false;
        }
        
        if RtlCompareMemory(
            entry.symbolic_link_name.as_ptr() as *const _,
            filter_name as *const _,
            filter_len,
        ) != filter_len {
            return false;
        }
    }
    
    // Check unique ID filter
    if filter.unique_id_length > 0 {
        let filter_id = buffer_base.add(filter.unique_id_offset as usize);
        let filter_len = filter.unique_id_length as usize;
        
        if entry.unique_id_length as usize != filter_len {
            return false;
        }
        
        if RtlCompareMemory(
            entry.unique_id.as_ptr() as *const _,
            filter_id as *const _,
            filter_len,
        ) != filter_len {
            return false;
        }
    }
    
    // Check device name filter
    if filter.device_name_length > 0 {
        let filter_name = buffer_base.add(filter.device_name_offset as usize) as *const u16;
        let filter_len = filter.device_name_length as usize;
        
        if entry.device_name_length as usize != filter_len {
            return false;
        }
        
        if RtlCompareMemory(
            entry.device_name.as_ptr() as *const _,
            filter_name as *const _,
            filter_len,
        ) != filter_len {
            return false;
        }
    }
    
    true
}

/// IOCTL_MOUNTMGR_CREATE_POINT
/// Create a new mount point
unsafe fn mountmgr_create_point(
    ext: *mut MOUNTMGR_EXTENSION,
    irp: PIRP,
    buffer: PVOID,
    input_len: u32,
) -> NTSTATUS {
    mnt_print!("[MOUNTMGR] CREATE_POINT\n");
    
    if buffer.is_null() || input_len < core::mem::size_of::<MOUNTMGR_CREATE_POINT_INPUT>() as u32 {
        (*irp).io_status.status = STATUS_INVALID_PARAMETER;
        (*irp).io_status.information = 0;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_INVALID_PARAMETER;
    }
    
    let input = &*(buffer as *const MOUNTMGR_CREATE_POINT_INPUT);
    let buffer_base = buffer as *const u8;
    
    // Validate offsets and lengths
    let sym_link_end = input.symbolic_link_name_offset as usize + input.symbolic_link_name_length as usize;
    let dev_name_end = input.device_name_offset as usize + input.device_name_length as usize;
    
    if sym_link_end > input_len as usize || dev_name_end > input_len as usize {
        (*irp).io_status.status = STATUS_INVALID_PARAMETER;
        (*irp).io_status.information = 0;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_INVALID_PARAMETER;
    }
    
    // Check for existing mount point with same symbolic link
    let sym_link_ptr = buffer_base.add(input.symbolic_link_name_offset as usize) as *const u16;
    let sym_link_len = input.symbolic_link_name_length as usize;
    
    for i in 0..(*ext).mount_point_count {
        let entry = &(*ext).mount_points[i];
        if !entry.in_use {
            continue;
        }
        
        if entry.symbolic_link_name_length as usize == sym_link_len {
            if RtlCompareMemory(
                entry.symbolic_link_name.as_ptr() as *const _,
                sym_link_ptr as *const _,
                sym_link_len,
            ) == sym_link_len {
                (*irp).io_status.status = STATUS_OBJECT_NAME_COLLISION;
                (*irp).io_status.information = 0;
                IoCompleteRequest(irp, IO_NO_INCREMENT);
                return STATUS_OBJECT_NAME_COLLISION;
            }
        }
    }
    
    // Find free slot
    let slot = find_free_mount_point_slot(ext);
    if slot.is_none() {
        (*irp).io_status.status = STATUS_INSUFFICIENT_RESOURCES;
        (*irp).io_status.information = 0;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_INSUFFICIENT_RESOURCES;
    }
    
    let slot = slot.unwrap();
    let entry = &mut (*ext).mount_points[slot];
    
    // Copy symbolic link name
    if sym_link_len <= MAX_SYMBOLIC_LINK_SIZE * 2 {
        ptr::copy_nonoverlapping(
            sym_link_ptr,
            entry.symbolic_link_name.as_mut_ptr(),
            sym_link_len / 2,
        );
        entry.symbolic_link_name_length = sym_link_len as USHORT;
    }
    
    // Copy device name
    let dev_name_ptr = buffer_base.add(input.device_name_offset as usize) as *const u16;
    let dev_name_len = input.device_name_length as usize;
    
    if dev_name_len <= MAX_DEVICE_NAME_SIZE * 2 {
        ptr::copy_nonoverlapping(
            dev_name_ptr,
            entry.device_name.as_mut_ptr(),
            dev_name_len / 2,
        );
        entry.device_name_length = dev_name_len as USHORT;
    }
    
    entry.in_use = true;
    
    if slot >= (*ext).mount_point_count {
        (*ext).mount_point_count = slot + 1;
    }
    
    // Increment epic number for change notifications
    (*ext).epic_number += 1;
    
    // Check if this is a drive letter assignment
    if is_drive_letter_link(sym_link_ptr, sym_link_len) {
        let letter = get_drive_letter_from_link(sym_link_ptr, sym_link_len);
        if letter >= b'A' && letter <= b'Z' {
            let bit = (letter - b'A') as u32;
            (*ext).drive_letters_in_use |= 1 << bit;
        }
    }
    
    mnt_print!("[MOUNTMGR] Mount point created\n");
    
    (*irp).io_status.status = STATUS_SUCCESS;
    (*irp).io_status.information = 0;
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    STATUS_SUCCESS
}

/// IOCTL_MOUNTMGR_DELETE_POINTS / IOCTL_MOUNTMGR_DELETE_POINTS_DBONLY
unsafe fn mountmgr_delete_points(
    ext: *mut MOUNTMGR_EXTENSION,
    irp: PIRP,
    buffer: PVOID,
    input_len: u32,
    output_len: u32,
    db_only: bool,
) -> NTSTATUS {
    mnt_print!("[MOUNTMGR] DELETE_POINTS");
    if db_only {
        mnt_print!("_DBONLY");
    }
    mnt_print!("\n");
    
    if buffer.is_null() {
        (*irp).io_status.status = STATUS_INVALID_PARAMETER;
        (*irp).io_status.information = 0;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_INVALID_PARAMETER;
    }
    
    // Parse filter
    let filter = if input_len >= core::mem::size_of::<MOUNTMGR_MOUNT_POINT>() as u32 {
        Some(&*(buffer as *const MOUNTMGR_MOUNT_POINT))
    } else {
        None
    };
    
    // Find and delete matching entries
    let mut deleted_count = 0usize;
    let buffer_base = buffer as *const u8;
    
    for i in 0..(*ext).mount_point_count {
        let entry = &mut (*ext).mount_points[i];
        if !entry.in_use {
            continue;
        }
        
        if matches_filter(entry, filter, buffer_base) {
            // Free drive letter if applicable
            if is_drive_letter_link(entry.symbolic_link_name.as_ptr(), entry.symbolic_link_name_length as usize) {
                let letter = get_drive_letter_from_link(
                    entry.symbolic_link_name.as_ptr(),
                    entry.symbolic_link_name_length as usize,
                );
                if letter >= b'A' && letter <= b'Z' {
                    let bit = (letter - b'A') as u32;
                    (*ext).drive_letters_in_use &= !(1 << bit);
                }
            }
            
            entry.in_use = false;
            deleted_count += 1;
        }
    }
    
    // Increment epic number
    if deleted_count > 0 {
        (*ext).epic_number += 1;
    }
    
    // Return deleted mount points info if output buffer is large enough
    if output_len >= core::mem::size_of::<MOUNTMGR_MOUNT_POINTS>() as u32 {
        let output = buffer as *mut MOUNTMGR_MOUNT_POINTS;
        (*output).size = core::mem::size_of::<MOUNTMGR_MOUNT_POINTS>() as ULONG;
        (*output).number_of_mount_points = deleted_count as ULONG;
        
        (*irp).io_status.information = core::mem::size_of::<MOUNTMGR_MOUNT_POINTS>();
    } else {
        (*irp).io_status.information = 0;
    }
    
    (*irp).io_status.status = STATUS_SUCCESS;
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    STATUS_SUCCESS
}

/// IOCTL_MOUNTMGR_NEXT_DRIVE_LETTER
/// Assign next available drive letter to a device
unsafe fn mountmgr_next_drive_letter(
    ext: *mut MOUNTMGR_EXTENSION,
    irp: PIRP,
    buffer: PVOID,
    input_len: u32,
    output_len: u32,
) -> NTSTATUS {
    mnt_print!("[MOUNTMGR] NEXT_DRIVE_LETTER\n");
    
    if buffer.is_null() || input_len < core::mem::size_of::<MOUNTMGR_DRIVE_LETTER_TARGET>() as u32 {
        (*irp).io_status.status = STATUS_INVALID_PARAMETER;
        (*irp).io_status.information = 0;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_INVALID_PARAMETER;
    }
    
    if output_len < core::mem::size_of::<MOUNTMGR_DRIVE_LETTER_INFORMATION>() as u32 {
        (*irp).io_status.status = STATUS_BUFFER_TOO_SMALL;
        (*irp).io_status.information = core::mem::size_of::<MOUNTMGR_DRIVE_LETTER_INFORMATION>();
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_BUFFER_TOO_SMALL;
    }
    
    let input = &*(buffer as *const MOUNTMGR_DRIVE_LETTER_TARGET);
    
    // Find next available drive letter (C-Z)
    let mut assigned_letter: Option<u8> = None;
    
    for letter in FIRST_DRIVE_LETTER..=LAST_DRIVE_LETTER {
        let bit = (letter - b'A') as u32;
        if ((*ext).drive_letters_in_use & (1 << bit)) == 0 {
            assigned_letter = Some(letter);
            (*ext).drive_letters_in_use |= 1 << bit;
            break;
        }
    }
    
    // Prepare output
    let output = buffer as *mut MOUNTMGR_DRIVE_LETTER_INFORMATION;
    
    if let Some(letter) = assigned_letter {
        // Create mount point entry
        let slot = find_free_mount_point_slot(ext);
        if let Some(slot) = slot {
            let entry = &mut (*ext).mount_points[slot];
            
            // Build symbolic link name: \DosDevices\X:
            let dos_prefix: &[u16] = &[
                b'\\' as u16, b'D' as u16, b'o' as u16, b's' as u16, b'D' as u16,
                b'e' as u16, b'v' as u16, b'i' as u16, b'c' as u16, b'e' as u16,
                b's' as u16, b'\\' as u16, letter as u16, b':' as u16,
            ];
            
            ptr::copy_nonoverlapping(
                dos_prefix.as_ptr(),
                entry.symbolic_link_name.as_mut_ptr(),
                dos_prefix.len(),
            );
            entry.symbolic_link_name_length = (dos_prefix.len() * 2) as USHORT;
            
            // Copy device name
            let dev_name_len = input.device_name_length as usize;
            if dev_name_len <= MAX_DEVICE_NAME_SIZE * 2 {
                ptr::copy_nonoverlapping(
                    input.device_name.as_ptr(),
                    entry.device_name.as_mut_ptr(),
                    dev_name_len / 2,
                );
                entry.device_name_length = dev_name_len as USHORT;
            }
            
            entry.in_use = true;
            
            if slot >= (*ext).mount_point_count {
                (*ext).mount_point_count = slot + 1;
            }
        }
        
        (*output).drive_letter_was_assigned = 1;
        (*output).current_drive_letter = letter;
        (*ext).epic_number += 1;
        
        mnt_print!("[MOUNTMGR] Assigned drive letter: ");
        let buf = [letter, b'\n', 0];
        DbgPrint(buf.as_ptr());
    } else {
        (*output).drive_letter_was_assigned = 0;
        (*output).current_drive_letter = 0;
        mnt_print!("[MOUNTMGR] No drive letter available\n");
    }
    
    (*irp).io_status.status = STATUS_SUCCESS;
    (*irp).io_status.information = core::mem::size_of::<MOUNTMGR_DRIVE_LETTER_INFORMATION>();
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    STATUS_SUCCESS
}

/// IOCTL_MOUNTMGR_VOLUME_ARRIVAL_NOTIFICATION
/// Notify mount manager about new volume
unsafe fn mountmgr_volume_arrival(
    ext: *mut MOUNTMGR_EXTENSION,
    irp: PIRP,
    buffer: PVOID,
    input_len: u32,
) -> NTSTATUS {
    mnt_print!("[MOUNTMGR] VOLUME_ARRIVAL_NOTIFICATION\n");
    
    if buffer.is_null() || input_len < core::mem::size_of::<MOUNTMGR_TARGET_NAME>() as u32 {
        (*irp).io_status.status = STATUS_INVALID_PARAMETER;
        (*irp).io_status.information = 0;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_INVALID_PARAMETER;
    }
    
    let _input = &*(buffer as *const MOUNTMGR_TARGET_NAME);
    
    // If auto-mount is enabled, assign a drive letter
    if (*ext).auto_mount_enabled {
        // In a real implementation, we would:
        // 1. Query the device for its unique ID
        // 2. Check if we have a persistent mapping for this ID
        // 3. If yes, restore the old drive letter
        // 4. If no, assign a new one
        
        // For now, just acknowledge
        mnt_print!("[MOUNTMGR] Volume arrival acknowledged\n");
    }
    
    (*irp).io_status.status = STATUS_SUCCESS;
    (*irp).io_status.information = 0;
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    STATUS_SUCCESS
}

/// IOCTL_MOUNTMGR_QUERY_DOS_VOLUME_PATH / IOCTL_MOUNTMGR_QUERY_DOS_VOLUME_PATHS
unsafe fn mountmgr_query_dos_volume_path(
    ext: *mut MOUNTMGR_EXTENSION,
    irp: PIRP,
    buffer: PVOID,
    input_len: u32,
    output_len: u32,
    all_paths: bool,
) -> NTSTATUS {
    mnt_print!("[MOUNTMGR] QUERY_DOS_VOLUME_PATH");
    if all_paths {
        mnt_print!("S");
    }
    mnt_print!("\n");
    
    if buffer.is_null() || input_len < core::mem::size_of::<MOUNTMGR_TARGET_NAME>() as u32 {
        (*irp).io_status.status = STATUS_INVALID_PARAMETER;
        (*irp).io_status.information = 0;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_INVALID_PARAMETER;
    }
    
    let input = &*(buffer as *const MOUNTMGR_TARGET_NAME);
    let dev_name_ptr = input.device_name.as_ptr();
    let dev_name_len = input.device_name_length as usize;
    
    // Find mount points for this device
    let mut found_path: Option<(&[u16], usize)> = None;
    
    for i in 0..(*ext).mount_point_count {
        let entry = &(*ext).mount_points[i];
        if !entry.in_use {
            continue;
        }
        
        // Match device name
        if entry.device_name_length as usize != dev_name_len {
            continue;
        }
        
        if RtlCompareMemory(
            entry.device_name.as_ptr() as *const _,
            dev_name_ptr as *const _,
            dev_name_len,
        ) != dev_name_len {
            continue;
        }
        
        // Found a match - check if it's a drive letter
        if is_drive_letter_link(entry.symbolic_link_name.as_ptr(), entry.symbolic_link_name_length as usize) {
            // Convert \DosDevices\X: to X:\
            let letter = get_drive_letter_from_link(
                entry.symbolic_link_name.as_ptr(),
                entry.symbolic_link_name_length as usize,
            );
            
            // We'll return just the drive letter path
            found_path = Some((entry.symbolic_link_name.as_ref(), letter as usize));
            
            if !all_paths {
                break;
            }
        }
    }
    
    // Calculate required output size
    // MOUNTMGR_VOLUME_PATHS header + path string + null terminator
    let path_len = if found_path.is_some() { 4 } else { 0 }; // "X:\" + null
    let required_size = core::mem::size_of::<MOUNTMGR_VOLUME_PATHS>() + path_len * 2;
    
    if output_len < core::mem::size_of::<MOUNTMGR_VOLUME_PATHS>() as u32 {
        (*irp).io_status.status = STATUS_BUFFER_TOO_SMALL;
        (*irp).io_status.information = required_size;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_BUFFER_TOO_SMALL;
    }
    
    let output = buffer as *mut MOUNTMGR_VOLUME_PATHS;
    
    if let Some((_, letter_val)) = found_path {
        let letter = letter_val as u8;
        
        if (output_len as usize) < required_size {
            (*output).multi_sz_length = (path_len * 2) as ULONG;
            (*irp).io_status.status = STATUS_BUFFER_OVERFLOW;
            (*irp).io_status.information = core::mem::size_of::<MOUNTMGR_VOLUME_PATHS>();
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            return STATUS_BUFFER_OVERFLOW;
        }
        
        // Write path "X:\" as multi-sz
        let path_buf = (*output).multi_sz.as_mut_ptr();
        *path_buf = letter as u16;
        *path_buf.add(1) = b':' as u16;
        *path_buf.add(2) = b'\\' as u16;
        *path_buf.add(3) = 0; // String null
        // Multi-sz requires double null at end (already 0 from buffer)
        
        (*output).multi_sz_length = 8; // 4 chars * 2 bytes
        (*irp).io_status.information = required_size;
    } else {
        // No path found - return empty multi-sz
        (*output).multi_sz_length = 2; // Just the double-null terminator
        if output_len as usize >= core::mem::size_of::<MOUNTMGR_VOLUME_PATHS>() + 2 {
            *(*output).multi_sz.as_mut_ptr() = 0;
        }
        (*irp).io_status.information = core::mem::size_of::<MOUNTMGR_VOLUME_PATHS>();
    }
    
    (*irp).io_status.status = STATUS_SUCCESS;
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    STATUS_SUCCESS
}

/// IOCTL_MOUNTMGR_CHANGE_NOTIFY
/// Wait for changes in mount point database
unsafe fn mountmgr_change_notify(
    ext: *mut MOUNTMGR_EXTENSION,
    irp: PIRP,
    buffer: PVOID,
    output_len: u32,
) -> NTSTATUS {
    mnt_print!("[MOUNTMGR] CHANGE_NOTIFY\n");
    
    if output_len < core::mem::size_of::<MOUNTMGR_CHANGE_NOTIFY_INFO>() as u32 {
        (*irp).io_status.status = STATUS_BUFFER_TOO_SMALL;
        (*irp).io_status.information = core::mem::size_of::<MOUNTMGR_CHANGE_NOTIFY_INFO>();
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_BUFFER_TOO_SMALL;
    }
    
    // In a real implementation, this would be async - we'd pend the IRP
    // and complete it when a change occurs. For now, return current epic.
    let output = buffer as *mut MOUNTMGR_CHANGE_NOTIFY_INFO;
    (*output).epic_number = (*ext).epic_number;
    
    (*irp).io_status.status = STATUS_SUCCESS;
    (*irp).io_status.information = core::mem::size_of::<MOUNTMGR_CHANGE_NOTIFY_INFO>();
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    STATUS_SUCCESS
}

/// IOCTL_MOUNTMGR_QUERY_AUTO_MOUNT
unsafe fn mountmgr_query_auto_mount(
    ext: *mut MOUNTMGR_EXTENSION,
    irp: PIRP,
    buffer: PVOID,
    output_len: u32,
) -> NTSTATUS {
    mnt_print!("[MOUNTMGR] QUERY_AUTO_MOUNT\n");
    
    if output_len < core::mem::size_of::<MOUNTMGR_QUERY_AUTO_MOUNT>() as u32 {
        (*irp).io_status.status = STATUS_BUFFER_TOO_SMALL;
        (*irp).io_status.information = core::mem::size_of::<MOUNTMGR_QUERY_AUTO_MOUNT>();
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_BUFFER_TOO_SMALL;
    }
    
    let output = buffer as *mut MOUNTMGR_QUERY_AUTO_MOUNT;
    (*output).current_state = if (*ext).auto_mount_enabled {
        MOUNTMGR_AUTO_MOUNT_STATE::Enabled
    } else {
        MOUNTMGR_AUTO_MOUNT_STATE::Disabled
    };
    
    (*irp).io_status.status = STATUS_SUCCESS;
    (*irp).io_status.information = core::mem::size_of::<MOUNTMGR_QUERY_AUTO_MOUNT>();
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    STATUS_SUCCESS
}

/// IOCTL_MOUNTMGR_SET_AUTO_MOUNT
unsafe fn mountmgr_set_auto_mount(
    ext: *mut MOUNTMGR_EXTENSION,
    irp: PIRP,
    buffer: PVOID,
    input_len: u32,
) -> NTSTATUS {
    mnt_print!("[MOUNTMGR] SET_AUTO_MOUNT\n");
    
    if input_len < core::mem::size_of::<MOUNTMGR_SET_AUTO_MOUNT>() as u32 {
        (*irp).io_status.status = STATUS_INVALID_PARAMETER;
        (*irp).io_status.information = 0;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_INVALID_PARAMETER;
    }
    
    let input = &*(buffer as *const MOUNTMGR_SET_AUTO_MOUNT);
    (*ext).auto_mount_enabled = input.new_state == MOUNTMGR_AUTO_MOUNT_STATE::Enabled;
    
    mnt_print!("[MOUNTMGR] Auto-mount: ");
    if (*ext).auto_mount_enabled {
        mnt_print!("enabled\n");
    } else {
        mnt_print!("disabled\n");
    }
    
    (*irp).io_status.status = STATUS_SUCCESS;
    (*irp).io_status.information = 0;
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    STATUS_SUCCESS
}

/// IOCTL_MOUNTMGR_KEEP_LINKS_WHEN_OFFLINE
unsafe fn mountmgr_keep_links_offline(
    ext: *mut MOUNTMGR_EXTENSION,
    irp: PIRP,
    buffer: PVOID,
    input_len: u32,
) -> NTSTATUS {
    mnt_print!("[MOUNTMGR] KEEP_LINKS_WHEN_OFFLINE\n");
    
    if buffer.is_null() || input_len < core::mem::size_of::<MOUNTMGR_TARGET_NAME>() as u32 {
        (*irp).io_status.status = STATUS_INVALID_PARAMETER;
        (*irp).io_status.information = 0;
        IoCompleteRequest(irp, IO_NO_INCREMENT);
        return STATUS_INVALID_PARAMETER;
    }
    
    let input = &*(buffer as *const MOUNTMGR_TARGET_NAME);
    let dev_name_ptr = input.device_name.as_ptr();
    let dev_name_len = input.device_name_length as usize;
    
    // Find mount points for this device and mark them
    for i in 0..(*ext).mount_point_count {
        let entry = &mut (*ext).mount_points[i];
        if !entry.in_use {
            continue;
        }
        
        if entry.device_name_length as usize == dev_name_len {
            if RtlCompareMemory(
                entry.device_name.as_ptr() as *const _,
                dev_name_ptr as *const _,
                dev_name_len,
            ) == dev_name_len {
                entry.keep_links_when_offline = true;
            }
        }
    }
    
    (*irp).io_status.status = STATUS_SUCCESS;
    (*irp).io_status.information = 0;
    IoCompleteRequest(irp, IO_NO_INCREMENT);
    STATUS_SUCCESS
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Find a free slot in mount points array
unsafe fn find_free_mount_point_slot(ext: *mut MOUNTMGR_EXTENSION) -> Option<usize> {
    // First try to find an unused slot
    for i in 0..(*ext).mount_point_count {
        if !(*ext).mount_points[i].in_use {
            return Some(i);
        }
    }
    
    // Otherwise use next slot if available
    if (*ext).mount_point_count < MAX_MOUNT_POINTS {
        return Some((*ext).mount_point_count);
    }
    
    None
}

/// Check if symbolic link is a drive letter (e.g., \DosDevices\C:)
fn is_drive_letter_link(link: *const u16, len: usize) -> bool {
    // \DosDevices\X: = 14 chars * 2 = 28 bytes
    if len != 28 {
        return false;
    }
    
    unsafe {
        // Check prefix \DosDevices\
        let expected: &[u16] = &[
            b'\\' as u16, b'D' as u16, b'o' as u16, b's' as u16, b'D' as u16,
            b'e' as u16, b'v' as u16, b'i' as u16, b'c' as u16, b'e' as u16,
            b's' as u16, b'\\' as u16,
        ];
        
        for i in 0..12 {
            if *link.add(i) != expected[i] {
                return false;
            }
        }
        
        // Check drive letter (A-Z)
        let letter = *link.add(12);
        if letter < b'A' as u16 || letter > b'Z' as u16 {
            return false;
        }
        
        // Check colon
        if *link.add(13) != b':' as u16 {
            return false;
        }
        
        true
    }
}

/// Get drive letter from symbolic link
fn get_drive_letter_from_link(link: *const u16, len: usize) -> u8 {
    if len < 28 {
        return 0;
    }
    
    unsafe {
        let letter = *link.add(12);
        if letter >= b'A' as u16 && letter <= b'Z' as u16 {
            letter as u8
        } else {
            0
        }
    }
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
