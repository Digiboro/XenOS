//! Microsoft AHCI Storage Miniport Driver
//!
//! AHCI (Advanced Host Controller Interface) miniport для работы через StorPort.
//! Реализует только HW-specific логику, PnP/Power управляется StorPort.

#![no_std]
#![allow(non_snake_case)]
#![allow(static_mut_refs)]

mod types;
mod imports;

use types::*;
use imports::storport::*;
use imports::storport::{SCSI_ADAPTER_CONTROL_TYPE, SCSI_ADAPTER_CONTROL_STATUS, SCSI_ADAPTER_CONTROL_SUCCESS};
use core::ptr;

const MSAHCI_POOL_TAG: ULONG = 0x49484153; // 'SAHI'
const PCI_TYPE0_ADDRESSES: ULONG = 6;

#[unsafe(no_mangle)]
pub extern "win64" fn DriverEntry(
    driver_object: PDRIVER_OBJECT,
    registry_path: *const UNICODE_STRING,
) -> NTSTATUS {
    unsafe {
        // Initialize HW_INITIALIZATION_DATA per WinDDK 7600.16385.1/inc/ddk/storport.h
        // Field order is CRITICAL for ABI compatibility!
        let mut hw_init_data = HW_INITIALIZATION_DATA {
            HwInitializationDataSize: core::mem::size_of::<HW_INITIALIZATION_DATA>() as ULONG,
            AdapterInterfaceType: 5, // PCIBus
            // Miniport driver routines - ORDER IS CRITICAL per WinDDK!
            HwInitialize: Some(MsAhciInitialize),
            HwStartIo: Some(MsAhciStartIo),
            HwInterrupt: Some(MsAhciInterrupt),
            HwFindAdapter: Some(MsAhciFindAdapter),
            HwResetBus: Some(MsAhciResetBus),
            HwDmaStarted: None,
            HwAdapterState: None,
            // Miniport driver resources
            DeviceExtensionSize: core::mem::size_of::<MSAHCI_DEVICE_EXTENSION>() as ULONG,
            SpecificLuExtensionSize: 0,
            SrbExtensionSize: 0,
            NumberOfAccessRanges: 1,
            Reserved: ptr::null_mut(),
            // Flags
            MapBuffers: 1,
            NeedPhysicalAddresses: 0,
            TaggedQueuing: 0,
            AutoRequestSense: 1,
            MultipleRequestPerLu: 0,
            ReceiveEvent: 0,
            // Vendor/Device identification
            VendorIdLength: 0,
            VendorId: ptr::null_mut(),
            PortVersionFlags: 0,
            DeviceIdLength: 0,
            DeviceId: ptr::null_mut(),
            // Additional callbacks
            HwAdapterControl: Some(MsAhciAdapterControl),
            HwBuildIo: None,
        };

        StorPortInitialize(driver_object, registry_path, &mut hw_init_data)
    }
}

unsafe extern "win64" fn MsAhciFindAdapter(
    device_extension: PVOID,
    _hw_context: PVOID,
    _bus_information: PVOID,
    _argument_string: PVOID,
    config_info: *mut PORT_CONFIGURATION_INFORMATION,
    _again: *mut BOOLEAN,
) -> ULONG {
    unsafe {
        let ext = device_extension as *mut MSAHCI_DEVICE_EXTENSION;
        ptr::write(ext, MSAHCI_DEVICE_EXTENSION::new());

        // Получаем ABAR из access_ranges или читаем из PCI config
        let mut abar_phys: PHYSICAL_ADDRESS = 0;
        let mut abar_size: usize = 0;
        
        // Проверяем access_ranges
        if config_info.is_null() {
            return 3;
        }
        
        if (*config_info).NumberOfAccessRanges == 0 || (*config_info).AccessRanges.is_null() {
            msahci_print("[MSAHCI] ERROR: No access ranges\n");
            return 3;
        }
        
        // Ищем MEMORY access range (AHCI ABAR - BAR5)
        // ABAR - это Memory Mapped I/O, а не IO Port
        let num_ranges = (*config_info).NumberOfAccessRanges as usize;
        let mut found = false;
        
        msahci_print("[MSAHCI] Searching for MEMORY access range among ");
        msahci_print_hex(num_ranges as u64);
        msahci_print(" ranges\n");
        
        for i in 0..num_ranges {
            let access_range = (*config_info).AccessRanges.add(i);
            let is_memory = (*access_range).RangeInMemory;
            let start = (*access_range).RangeStart;
            let len = (*access_range).RangeLength;
            
            msahci_print("[MSAHCI]   Range ");
            msahci_print_hex(i as u64);
            msahci_print(": is_mem=");
            msahci_print_hex(is_memory as u64);
            msahci_print(" start=0x");
            msahci_print_hex(start);
            msahci_print(" len=0x");
            msahci_print_hex(len as u64);
            msahci_print("\n");
            
            // AHCI ABAR должен быть Memory-mapped, размер обычно 4KB или больше
            if is_memory != 0 && len >= 0x400 {
                abar_phys = start;
                abar_size = len as usize;
                found = true;
                msahci_print("[MSAHCI]   -> Selected as ABAR\n");
                break;
            }
        }
        
        if !found {
            msahci_print("[MSAHCI] ERROR: No suitable MEMORY range found for ABAR\n");
            return 3;
        }

        msahci_print("[MSAHCI] FindAdapter: ABAR phys=0x");
        msahci_print_hex(abar_phys);
        msahci_print(" size=0x");
        msahci_print_hex(abar_size as u64);
        msahci_print("\n");

        (*ext).abar_phys = abar_phys;

        // Используем StorPortGetDeviceBase для маппинга MMIO регистров
        (*ext).abar = StorPortGetDeviceBase(
            device_extension,
            0,  // bus_type - Internal
            0,  // system_io_bus_number
            abar_phys,
            abar_size as ULONG,
            0,  // not IO space, MMIO
        ) as *mut AHCI_HBA_MEM;

        msahci_print("[MSAHCI] FindAdapter: ABAR mapped to 0x");
        msahci_print_hex((*ext).abar as u64);
        msahci_print("\n");

        if (*ext).abar.is_null() {
            msahci_print("[MSAHCI] ERROR: Failed to map ABAR\n");
            return 3;
        }

        let abar = (*ext).abar;
        (*ext).cap = StorPortReadRegisterUlong(&mut (*abar).cap as *mut _);
        (*ext).cap2 = StorPortReadRegisterUlong(&mut (*abar).cap2 as *mut _);
        (*ext).version = StorPortReadRegisterUlong(&mut (*abar).vs as *mut _);
        (*ext).ports_implemented = StorPortReadRegisterUlong(&mut (*abar).pi as *mut _);

        let cap = (*ext).cap;
        (*ext).num_cmd_slots = ((cap & CAP_NCS_MASK) >> CAP_NCS_SHIFT) + 1;

        let ghc = StorPortReadRegisterUlong(&mut (*abar).ghc as *mut _);
        if (ghc & GHC_AE) == 0 {
            StorPortWriteRegisterUlong(&mut (*abar).ghc as *mut _, ghc | GHC_AE);
        }

        let pi = (*ext).ports_implemented;
        (*ext).num_ports = pi.count_ones();

        for port in 0..32u8 {
            if (pi & (1 << port)) != 0 {
                (*ext).ports[port as usize].implemented = 1;
                (*ext).ports[port as usize].port_number = port;
            }
        }

        (*config_info).NumberOfBuses = 1;
        (*config_info).InitiatorBusId[0] = (*ext).num_ports as i8;
        (*config_info).MaximumTransferLength = 0x1000 * 128;
        (*config_info).NumberOfPhysicalBreaks = 8;
        (*config_info).ScatterGather = 1;
        (*config_info).Master = 1;
        (*config_info).CachesData = 1;

        0
    }
}

unsafe extern "win64" fn MsAhciInitialize(device_extension: PVOID) -> BOOLEAN {
    unsafe {
        msahci_print("[MSAHCI] MsAhciInitialize called\n");
        
        let ext = device_extension as *mut MSAHCI_DEVICE_EXTENSION;
        let abar = (*ext).abar;

        msahci_print("[MSAHCI] abar=0x");
        msahci_print_hex(abar as u64);
        msahci_print("\n");

        if abar.is_null() {
            msahci_print("[MSAHCI] ERROR: abar is null\n");
            return 0;
        }

        let pi = (*ext).ports_implemented;
        msahci_print("[MSAHCI] ports_implemented=0x");
        msahci_print_hex(pi as u64);
        msahci_print("\n");
        
        for port in 0..32u8 {
            if (pi & (1 << port)) == 0 {
                continue;
            }

            let port_regs = &mut (*abar).ports[port as usize];
            let port_info = &mut (*ext).ports[port as usize];

            let ssts = StorPortReadRegisterUlong(&mut port_regs.ssts as *mut _);
            let det = ssts & PORT_SSTS_DET_MASK;
            
            msahci_print("[MSAHCI] Port ");
            msahci_print_hex(port as u64);
            msahci_print(": ssts=0x");
            msahci_print_hex(ssts as u64);
            msahci_print(" det=0x");
            msahci_print_hex(det as u64);
            msahci_print("\n");

            if det != PORT_SSTS_DET_PHY {
                msahci_print("[MSAHCI]   -> No device (det != PHY)\n");
                port_info.device_present = 0;
                continue;
            }

            msahci_print("[MSAHCI]   -> Device PRESENT!\n");
            port_info.device_present = 1;

            let cmd = StorPortReadRegisterUlong(&mut port_regs.cmd as *mut _);
            StorPortWriteRegisterUlong(&mut port_regs.cmd as *mut _, cmd & !(PORT_CMD_ST | PORT_CMD_FRE));

            for _ in 0..500 {
                let cmd_check = StorPortReadRegisterUlong(&mut port_regs.cmd as *mut _);
                if (cmd_check & (PORT_CMD_CR | PORT_CMD_FR)) == 0 {
                    break;
                }
                StorPortStallExecution(1000);
            }

            let cmd_list_size = core::mem::size_of::<AHCI_CMD_LIST>();
            let fis_recv_size = core::mem::size_of::<AHCI_FIS_RECV>();
            let cmd_table_size = core::mem::size_of::<AHCI_CMD_TABLE>();

            let total_size = cmd_list_size + fis_recv_size + (cmd_table_size * 32);

            let buffer = StorPortGetUncachedExtension(
                device_extension,
                ptr::null_mut(),
                total_size as ULONG,
            );

            if buffer.is_null() {
                return 0;
            }

            ptr::write_bytes(buffer, 0, total_size);

            let mut current = buffer as usize;

            port_info.cmd_list = current as *mut AHCI_CMD_LIST;
            port_info.cmd_list_phys = StorPortGetPhysicalAddress(
                device_extension,
                ptr::null_mut(),
                port_info.cmd_list as PVOID,
                ptr::null_mut(),
            );
            current += cmd_list_size;

            port_info.fis_recv = current as *mut AHCI_FIS_RECV;
            port_info.fis_recv_phys = StorPortGetPhysicalAddress(
                device_extension,
                ptr::null_mut(),
                port_info.fis_recv as PVOID,
                ptr::null_mut(),
            );
            current += fis_recv_size;

            for i in 0..32 {
                port_info.cmd_tables[i] = current as *mut AHCI_CMD_TABLE;
                port_info.cmd_tables_phys[i] = StorPortGetPhysicalAddress(
                    device_extension,
                    ptr::null_mut(),
                    port_info.cmd_tables[i] as PVOID,
                    ptr::null_mut(),
                );
                current += cmd_table_size;
            }

            let cmd_list = port_info.cmd_list;
            for i in 0..32 {
                let header = &mut (*cmd_list).entries[i];
                let ctba = port_info.cmd_tables_phys[i];
                header.ctba = ctba as u32;
                header.ctbau = (ctba >> 32) as u32;
            }

            StorPortWriteRegisterUlong(&mut port_regs.clb as *mut _, port_info.cmd_list_phys as u32);
            StorPortWriteRegisterUlong(&mut port_regs.clbu as *mut _, (port_info.cmd_list_phys >> 32) as u32);
            StorPortWriteRegisterUlong(&mut port_regs.fb as *mut _, port_info.fis_recv_phys as u32);
            StorPortWriteRegisterUlong(&mut port_regs.fbu as *mut _, (port_info.fis_recv_phys >> 32) as u32);

            StorPortWriteRegisterUlong(&mut port_regs.serr as *mut _, 0xFFFFFFFF);
            StorPortWriteRegisterUlong(&mut port_regs.is as *mut _, 0xFFFFFFFF);

            let cmd_new = (cmd & !PORT_CMD_ST) | PORT_CMD_FRE;
            StorPortWriteRegisterUlong(&mut port_regs.cmd as *mut _, cmd_new);

            for _ in 0..500 {
                let cmd_check = StorPortReadRegisterUlong(&mut port_regs.cmd as *mut _);
                if (cmd_check & PORT_CMD_FR) != 0 {
                    break;
                }
                StorPortStallExecution(1000);
            }

            StorPortWriteRegisterUlong(&mut port_regs.cmd as *mut _, cmd_new | PORT_CMD_ST);

            // Phase 3.1: Enable port interrupts for async I/O
            // Enable all interrupt types we care about
            let port_ie = PORT_IE_DHRE   // Device to Host Register FIS
                        | PORT_IE_PSE    // PIO Setup FIS
                        | PORT_IE_DSE    // DMA Setup FIS
                        | PORT_IE_SDBE   // Set Device Bits FIS
                        | PORT_IE_DPE;   // Descriptor Processed
            StorPortWriteRegisterUlong(&mut port_regs.ie as *mut _, port_ie);

            let sig = StorPortReadRegisterUlong(&mut port_regs.sig as *mut _);
            port_info.device_type = match sig {
                SATA_SIG_ATA => 0,
                SATA_SIG_ATAPI => 1,
                _ => 0xFF,
            };

            if port_info.device_type == 0 {
                ahci_identify_device(ext, port);
            }
        }

        // Enable global interrupts
        let ghc = StorPortReadRegisterUlong(&mut (*abar).ghc as *mut _);
        StorPortWriteRegisterUlong(&mut (*abar).ghc as *mut _, ghc | GHC_IE);
        
        // === DIAGNOSTIC: Verify interrupt setup ===
        let ghc_verify = StorPortReadRegisterUlong(&mut (*abar).ghc as *mut _);
        msahci_print("[MSAHCI] DIAG: GHC after enable = 0x");
        msahci_print_hex(ghc_verify as u64);
        msahci_print(" (IE bit = ");
        msahci_print_hex(((ghc_verify >> 1) & 1) as u64);
        msahci_print(")\n");
        
        // Check each port's IE register
        for port in 0..32u8 {
            if ((*ext).ports_implemented & (1 << port)) == 0 {
                continue;
            }
            let port_regs = &mut (*abar).ports[port as usize];
            let ie = StorPortReadRegisterUlong(&mut port_regs.ie as *mut _);
            let cmd = StorPortReadRegisterUlong(&mut port_regs.cmd as *mut _);
            msahci_print("[MSAHCI] DIAG: Port ");
            msahci_print_hex(port as u64);
            msahci_print(" IE=0x");
            msahci_print_hex(ie as u64);
            msahci_print(" CMD=0x");
            msahci_print_hex(cmd as u64);
            msahci_print("\n");
        }

        1
    }
}

unsafe fn ahci_identify_device(ext: *mut MSAHCI_DEVICE_EXTENSION, port: u8) {
    let port_info = &mut (*ext).ports[port as usize];
    let cmd_list = port_info.cmd_list;
    let cmd_table = port_info.cmd_tables[0];

    let header = &mut (*cmd_list).entries[0];
    header.flags = (5 << 0) | (1 << 10);
    header.prdtl = 1;

    let mut identify_buffer = [0u16; 256];
    let buffer_phys = StorPortGetPhysicalAddress(
        ext as PVOID,
        ptr::null_mut(),
        identify_buffer.as_mut_ptr() as PVOID,
        ptr::null_mut(),
    );

    let prdt = &mut (*cmd_table).prdt[0];
    prdt.dba = buffer_phys as u32;
    prdt.dbau = (buffer_phys >> 32) as u32;
    prdt.dbc = 511;

    let fis = &mut (*cmd_table).cfis as *mut _ as *mut FIS_REG_H2D;
    ptr::write_bytes(fis, 0, 1);
    (*fis).fis_type = FIS_TYPE_REG_H2D;
    (*fis).pmport_c = 0x80;
    (*fis).command = ATA_CMD_IDENTIFY;

    let abar = (*ext).abar;
    let port_regs = &mut (*abar).ports[port as usize];

    StorPortWriteRegisterUlong(&mut port_regs.ci as *mut _, 1);

    for _ in 0..1000 {
        let ci = StorPortReadRegisterUlong(&mut port_regs.ci as *mut _);
        if ci == 0 {
            break;
        }
        StorPortStallExecution(1000);
    }

    let lba48_supported = identify_buffer[83] & (1 << 10);
    if lba48_supported != 0 {
        port_info.sector_count = ((identify_buffer[103] as u64) << 48)
            | ((identify_buffer[102] as u64) << 32)
            | ((identify_buffer[101] as u64) << 16)
            | (identify_buffer[100] as u64);
    } else {
        port_info.sector_count = ((identify_buffer[61] as u64) << 16) | (identify_buffer[60] as u64);
    }

    port_info.sector_size = 512;
    
    // Extract Serial Number (words 10-19, 20 chars, byte-swapped)
    // ATA stores ASCII strings with bytes swapped within each word
    for i in 0..10 {
        let word = identify_buffer[10 + i];
        port_info.serial_number[i * 2] = (word >> 8) as u8;
        port_info.serial_number[i * 2 + 1] = (word & 0xFF) as u8;
    }
    
    // Extract Firmware Revision (words 23-26, 8 chars, byte-swapped)
    for i in 0..4 {
        let word = identify_buffer[23 + i];
        port_info.firmware_revision[i * 2] = (word >> 8) as u8;
        port_info.firmware_revision[i * 2 + 1] = (word & 0xFF) as u8;
    }
    
    // Extract Model Number (words 27-46, 40 chars, byte-swapped)
    for i in 0..20 {
        let word = identify_buffer[27 + i];
        port_info.model_number[i * 2] = (word >> 8) as u8;
        port_info.model_number[i * 2 + 1] = (word & 0xFF) as u8;
    }
    
    // Trim trailing spaces from strings
    trim_trailing_spaces(&mut port_info.serial_number);
    trim_trailing_spaces(&mut port_info.firmware_revision);
    trim_trailing_spaces(&mut port_info.model_number);
}

/// Trim trailing spaces from ATA string (replaces trailing 0x20 with 0x20, keeps padding)
fn trim_trailing_spaces(s: &mut [u8]) {
    // ATA strings are space-padded, we keep them that way for SCSI INQUIRY
    // Just ensure any NUL bytes become spaces
    for b in s.iter_mut() {
        if *b == 0 {
            *b = 0x20; // Replace NUL with space
        }
    }
}

unsafe extern "win64" fn MsAhciStartIo(
    device_extension: PVOID,
    srb: *mut SCSI_REQUEST_BLOCK,
) -> BOOLEAN {
    unsafe {
        msahci_print("[MSAHCI] MsAhciStartIo ENTRY\n");
        
        let ext = device_extension as *mut MSAHCI_DEVICE_EXTENSION;
        let target_id = (*srb).TargetId;
        let lun = (*srb).Lun;

        msahci_print("[MSAHCI] target_id=");
        msahci_print_hex(target_id as u64);
        msahci_print(" lun=");
        msahci_print_hex(lun as u64);
        msahci_print("\n");
        
        if lun != 0 || target_id >= 32 {
            msahci_print("[MSAHCI] ERROR: Invalid lun/target_id\n");
            (*srb).SrbStatus = SRB_STATUS_NO_DEVICE;
            StorPortNotification(REQUEST_COMPLETE, device_extension, srb);
            StorPortNotification(NEXT_REQUEST, device_extension);
            return 1;
        }

        let port_info = &mut (*ext).ports[target_id as usize];
        msahci_print("[MSAHCI] device_present=");
        msahci_print_hex(port_info.device_present as u64);
        msahci_print("\n");
        
        if port_info.device_present == 0 {
            msahci_print("[MSAHCI] ERROR: Device not present\n");
            (*srb).SrbStatus = SRB_STATUS_NO_DEVICE;
            StorPortNotification(REQUEST_COMPLETE, device_extension, srb);
            StorPortNotification(NEXT_REQUEST, device_extension);
            return 1;
        }

        let cdb = &(*srb).Cdb;
        
        msahci_print("[MSAHCI] StartIo: CDB[0]=0x");
        msahci_print_hex(cdb[0] as u64);
        msahci_print(" target=");
        msahci_print_hex(target_id as u64);
        msahci_print(" buffer=0x");
        msahci_print_hex((*srb).DataBuffer as u64);
        msahci_print(" length=");
        msahci_print_hex((*srb).DataTransferLength as u64);
        msahci_print("\n");
        
        match cdb[0] {
            SCSIOP_INQUIRY => {
                return handle_inquiry(ext, srb, target_id);
            }
            SCSIOP_TEST_UNIT_READY => {
                return handle_test_unit_ready(ext, srb, target_id);
            }
            SCSIOP_READ_CAPACITY => {
                return handle_read_capacity(ext, srb, target_id);
            }
            SCSIOP_SYNCHRONIZE_CACHE => {
                return handle_synchronize_cache(ext, srb, target_id);
            }
            SCSIOP_START_STOP_UNIT => {
                return handle_start_stop_unit(ext, srb, target_id);
            }
            _ => {}
        }
        
        let is_read = cdb[0] == SCSIOP_READ10;
        let is_write = cdb[0] == SCSIOP_WRITE10;

        if !is_read && !is_write {
            msahci_print("[MSAHCI] ERROR: Unknown CDB opcode 0x");
            msahci_print_hex(cdb[0] as u64);
            msahci_print("\n");
            (*srb).SrbStatus = SRB_STATUS_INVALID_REQUEST;
            StorPortNotification(REQUEST_COMPLETE, device_extension, srb);
            StorPortNotification(NEXT_REQUEST, device_extension);
            return 1;
        }

        let lba = ((cdb[2] as u64) << 24) | ((cdb[3] as u64) << 16) | ((cdb[4] as u64) << 8) | (cdb[5] as u64);
        let count = ((cdb[7] as u16) << 8) | (cdb[8] as u16);
        
        #[cfg(feature = "storage-trace")]
        {
            msahci_print("[MSAHCI] LBA=");
            msahci_print_hex(lba);
            msahci_print(" sectors=");
            msahci_print_hex(count as u64);
            msahci_print("\n");
        }

        // Phase 3.1: Async I/O - allocate slot and issue command
        let slot = match port_info.allocate_slot() {
            Some(s) => s,
            None => {
                // No free slots - queue is full, return busy
                msahci_print("[MSAHCI] No free command slots, returning BUSY\n");
                (*srb).SrbStatus = SRB_STATUS_BUSY;
                StorPortNotification(REQUEST_COMPLETE, device_extension, srb);
                StorPortNotification(NEXT_REQUEST, device_extension);
                return 1;
            }
        };
        
        let status = ahci_issue_command(
            ext, 
            target_id, 
            slot,
            srb,
            lba, 
            count, 
            (*srb).DataBuffer, 
            is_write
        );
        
        if status != 0 {
            // Command issue failed - free slot and report error
            port_info.free_slot(slot);
            (*srb).SrbStatus = SRB_STATUS_ERROR;
            StorPortNotification(REQUEST_COMPLETE, device_extension, srb);
            StorPortNotification(NEXT_REQUEST, device_extension);
            return 1;
        }

        // Command issued successfully
        // In synchronous polling mode, the command has already completed
        // Set SrbStatus and complete the request
        port_info.free_slot(slot);
        (*srb).SrbStatus = SRB_STATUS_SUCCESS;
        StorPortNotification(REQUEST_COMPLETE, device_extension, srb);
        StorPortNotification(NEXT_REQUEST, device_extension);
        1
    }
}

// =============================================================================
// SCSI Command Handlers
// =============================================================================

// =============================================================================
// INQUIRYDATA
// =============================================================================
//
// Reference: WinDDK 7600.16385.1/inc/ddk/scsi.h (lines 2193-2234)
// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/scsi/ns-scsi-_inquirydata
//

/// INQUIRYDATA structure (36 bytes minimum)
///
/// Binary compatible with Windows NT 6.1 (Win7) layout.
#[repr(C, packed)]
#[allow(non_camel_case_types)]
struct INQUIRYDATA {
    /// Byte 0: DeviceType (bits 0-4), DeviceTypeQualifier (bits 5-7)
    DeviceType: u8,
    /// Byte 1: DeviceTypeModifier (bits 0-6), RemovableMedia (bit 7)
    DeviceTypeModifier: u8,
    /// Byte 2: Versions
    Versions: u8,
    /// Byte 3: ResponseDataFormat (bits 0-3), flags (bits 4-7)
    ResponseDataFormat: u8,
    /// Byte 4: AdditionalLength (n-4)
    AdditionalLength: u8,
    /// Byte 5: Reserved
    Reserved: u8,
    /// Byte 6: Reserved2 / capability flags
    Reserved2: u8,
    /// Byte 7: SoftReset, CommandQueue, etc.
    CommandQueue: u8,
    /// Bytes 8-15: VendorId (8 bytes, space padded ASCII)
    VendorId: [u8; 8],
    /// Bytes 16-31: ProductId (16 bytes, space padded ASCII)
    ProductId: [u8; 16],
    /// Bytes 32-35: ProductRevisionLevel (4 bytes ASCII)
    ProductRevisionLevel: [u8; 4],
}

/// Handle SCSI INQUIRY command
unsafe fn handle_inquiry(
    ext: *mut MSAHCI_DEVICE_EXTENSION,
    srb: *mut SCSI_REQUEST_BLOCK,
    target_id: u8,
) -> BOOLEAN {
    unsafe {
        msahci_print("[MSAHCI] INQUIRY for target ");
        msahci_print_hex(target_id as u64);
        msahci_print("\n");
        
        let port_info = &(*ext).ports[target_id as usize];
        
        // Check if device is present
        if port_info.device_present == 0 {
            msahci_print("[MSAHCI] INQUIRY: No device at target ");
            msahci_print_hex(target_id as u64);
            msahci_print("\n");
            (*srb).SrbStatus = SRB_STATUS_NO_DEVICE;
            StorPortNotification(REQUEST_COMPLETE, ext as PVOID, srb);
            StorPortNotification(NEXT_REQUEST, ext as PVOID);
            return 1;
        }
        
        // Check buffer size
        if (*srb).DataBuffer.is_null() || (*srb).DataTransferLength < 36 {
            msahci_print("[MSAHCI] INQUIRY: Invalid buffer\n");
            (*srb).SrbStatus = SRB_STATUS_INVALID_REQUEST;
            StorPortNotification(REQUEST_COMPLETE, ext as PVOID, srb);
            StorPortNotification(NEXT_REQUEST, ext as PVOID);
            return 1;
        }
        
        // Build INQUIRY response
        let buffer = (*srb).DataBuffer as *mut u8;
        ptr::write_bytes(buffer, 0, (*srb).DataTransferLength as usize);
        
        let inquiry = buffer as *mut INQUIRYDATA;
        
        // Device type: 0x00 = Direct access device (disk)
        // Peripheral qualifier: 0x00 = Device is connected
        (*inquiry).DeviceType = 0x00;  // Direct access device, connected
        
        // Device type modifier: 0x00 = not removable
        (*inquiry).DeviceTypeModifier = 0x00;
        
        // Versions: 0x05 = SPC-3 (ANSI version 5)
        (*inquiry).Versions = 0x05;
        
        // Response data format: 2 = Response format as defined in SPC
        (*inquiry).ResponseDataFormat = 0x02;
        
        // Additional length: 31 (36 - 5)
        (*inquiry).AdditionalLength = 31;
        
        // Reserved fields
        (*inquiry).Reserved = 0;
        (*inquiry).Reserved2 = 0;
        
        // Command queue: 0 = no tagged command queuing
        (*inquiry).CommandQueue = 0;
        
        // VendorId (8 bytes) - Extract from model number or use default
        // Many drives have vendor name as first part of model (e.g., "QEMU HARDDISK")
        // Try to extract it, otherwise use "ATA     "
        let model = &port_info.model_number;
        let mut vendor_end = 0usize;
        for i in 0..8.min(model.len()) {
            if model[i] == 0x20 {
                vendor_end = i;
                break;
            }
            vendor_end = i + 1;
        }
        
        // Copy vendor (up to 8 chars)
        for i in 0..8 {
            if i < vendor_end {
                (*inquiry).VendorId[i] = model[i];
            } else {
                (*inquiry).VendorId[i] = 0x20; // Pad with spaces
            }
        }
        
        // ProductId (16 bytes) - Use model number (skip vendor part if extracted)
        let model_start = if vendor_end > 0 && vendor_end < model.len() && model[vendor_end] == 0x20 {
            vendor_end + 1
        } else {
            0
        };
        
        for i in 0..16 {
            let src_idx = model_start + i;
            if src_idx < model.len() && model[src_idx] != 0 {
                (*inquiry).ProductId[i] = model[src_idx];
            } else {
                (*inquiry).ProductId[i] = 0x20; // Pad with spaces
            }
        }
        
        // ProductRevisionLevel (4 bytes) - Use firmware revision
        for i in 0..4 {
            if i < port_info.firmware_revision.len() && port_info.firmware_revision[i] != 0 {
                (*inquiry).ProductRevisionLevel[i] = port_info.firmware_revision[i];
            } else {
                (*inquiry).ProductRevisionLevel[i] = 0x20; // Pad with spaces
            }
        }
        
        msahci_print("[MSAHCI] INQUIRY: Success\n");
        
        (*srb).SrbStatus = SRB_STATUS_SUCCESS;
        (*srb).DataTransferLength = 36;
        StorPortNotification(REQUEST_COMPLETE, ext as PVOID, srb);
        StorPortNotification(NEXT_REQUEST, ext as PVOID);
        1
    }
}

/// Handle SCSI TEST UNIT READY command
unsafe fn handle_test_unit_ready(
    ext: *mut MSAHCI_DEVICE_EXTENSION,
    srb: *mut SCSI_REQUEST_BLOCK,
    target_id: u8,
) -> BOOLEAN {
    unsafe {
        msahci_print("[MSAHCI] TEST_UNIT_READY for target ");
        msahci_print_hex(target_id as u64);
        msahci_print("\n");
        
        let port_info = &(*ext).ports[target_id as usize];
        
        if port_info.device_present == 0 {
            (*srb).SrbStatus = SRB_STATUS_NO_DEVICE;
        } else {
            (*srb).SrbStatus = SRB_STATUS_SUCCESS;
        }
        
        StorPortNotification(REQUEST_COMPLETE, ext as PVOID, srb);
        StorPortNotification(NEXT_REQUEST, ext as PVOID);
        1
    }
}

// =============================================================================
// READ_CAPACITY_DATA
// =============================================================================
//
// Reference: WinDDK 7600.16385.1/inc/ddk/scsi.h (lines 2760-2763)
// MSDN: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/scsi/ns-scsi-_read_capacity_data
//
// Note: Data is returned in Big Endian format

/// READ_CAPACITY_DATA structure (8 bytes)
#[repr(C, packed)]
#[allow(non_camel_case_types)]
struct READ_CAPACITY_DATA {
    /// LogicalBlockAddress - Last LBA (big-endian)
    LogicalBlockAddress: [u8; 4],
    /// BytesPerBlock - Block size in bytes (big-endian)
    BytesPerBlock: [u8; 4],
}

/// Handle SCSI READ CAPACITY command
unsafe fn handle_read_capacity(
    ext: *mut MSAHCI_DEVICE_EXTENSION,
    srb: *mut SCSI_REQUEST_BLOCK,
    target_id: u8,
) -> BOOLEAN {
    unsafe {
        msahci_print("[MSAHCI] READ_CAPACITY for target ");
        msahci_print_hex(target_id as u64);
        msahci_print("\n");
        
        let port_info = &(*ext).ports[target_id as usize];
        
        if port_info.device_present == 0 {
            (*srb).SrbStatus = SRB_STATUS_NO_DEVICE;
            StorPortNotification(REQUEST_COMPLETE, ext as PVOID, srb);
            StorPortNotification(NEXT_REQUEST, ext as PVOID);
            return 1;
        }
        
        if (*srb).DataBuffer.is_null() || (*srb).DataTransferLength < 8 {
            (*srb).SrbStatus = SRB_STATUS_INVALID_REQUEST;
            StorPortNotification(REQUEST_COMPLETE, ext as PVOID, srb);
            StorPortNotification(NEXT_REQUEST, ext as PVOID);
            return 1;
        }
        
        let buffer = (*srb).DataBuffer as *mut u8;
        ptr::write_bytes(buffer, 0, 8);
        
        let capacity = buffer as *mut READ_CAPACITY_DATA;
        
        // Get sector count from port info
        let sector_count = port_info.sector_count;
        let last_lba = if sector_count > 0 { 
            (sector_count - 1).min(0xFFFFFFFF) as u32 
        } else { 
            0 
        };
        let block_size: u32 = 512;
        
        msahci_print("[MSAHCI] READ_CAPACITY: last_lba=0x");
        msahci_print_hex(last_lba as u64);
        msahci_print(" block_size=");
        msahci_print_hex(block_size as u64);
        msahci_print("\n");
        
        // Write in big-endian format
        (*capacity).LogicalBlockAddress = [
            (last_lba >> 24) as u8,
            (last_lba >> 16) as u8,
            (last_lba >> 8) as u8,
            last_lba as u8,
        ];
        (*capacity).BytesPerBlock = [
            (block_size >> 24) as u8,
            (block_size >> 16) as u8,
            (block_size >> 8) as u8,
            block_size as u8,
        ];
        
        (*srb).SrbStatus = SRB_STATUS_SUCCESS;
        (*srb).DataTransferLength = 8;
        StorPortNotification(REQUEST_COMPLETE, ext as PVOID, srb);
        StorPortNotification(NEXT_REQUEST, ext as PVOID);
        1
    }
}

/// Handle SCSIOP_SYNCHRONIZE_CACHE - Flush cache to disk
/// 
/// Issues ATA FLUSH CACHE EXT command to ensure all data is written to media.
fn handle_synchronize_cache(
    ext: *mut MSAHCI_DEVICE_EXTENSION,
    srb: *mut SCSI_REQUEST_BLOCK,
    target_id: u8,
) -> BOOLEAN {
    unsafe {
        msahci_print("[MSAHCI] SYNCHRONIZE_CACHE for target ");
        msahci_print_hex(target_id as u64);
        msahci_print("\n");
        
        let port_info = &mut (*ext).ports[target_id as usize];
        
        if port_info.device_present == 0 {
            (*srb).SrbStatus = SRB_STATUS_NO_DEVICE;
            StorPortNotification(REQUEST_COMPLETE, ext as PVOID, srb);
            StorPortNotification(NEXT_REQUEST, ext as PVOID);
            return 1;
        }
        
        // Issue FLUSH CACHE EXT command
        let result = ahci_flush_cache(ext, target_id);
        
        if result == 0 {
            (*srb).SrbStatus = SRB_STATUS_SUCCESS;
        } else {
            msahci_print("[MSAHCI] FLUSH_CACHE failed\n");
            (*srb).SrbStatus = SRB_STATUS_ERROR;
        }
        
        StorPortNotification(REQUEST_COMPLETE, ext as PVOID, srb);
        StorPortNotification(NEXT_REQUEST, ext as PVOID);
        1
    }
}

/// Issue ATA FLUSH CACHE EXT command
/// 
/// Flushes write cache to media. This is a non-data command.
unsafe fn ahci_flush_cache(ext: *mut MSAHCI_DEVICE_EXTENSION, port: u8) -> i32 {
    let port_info = &mut (*ext).ports[port as usize];
    let cmd_list = port_info.cmd_list;
    let cmd_table = port_info.cmd_tables[0];
    
    if cmd_list.is_null() || cmd_table.is_null() {
        return -1;
    }
    
    // Prepare command header (non-data command)
    let header = &mut (*cmd_list).entries[0];
    header.flags = (5 << 0);  // CFL = 5 DWORDs, no data transfer
    header.prdtl = 0;         // No PRDT entries for non-data command
    header.prdbc = 0;
    
    // Clear command table
    ptr::write_bytes(cmd_table, 0, 1);
    
    // Build FIS for FLUSH CACHE EXT
    let fis = &mut (*cmd_table).cfis as *mut _ as *mut FIS_REG_H2D;
    ptr::write_bytes(fis, 0, 1);
    (*fis).fis_type = FIS_TYPE_REG_H2D;
    (*fis).pmport_c = 0x80;  // Command bit set
    (*fis).command = ATA_CMD_FLUSH_CACHE_EXT;
    (*fis).device = 0;
    
    let abar = (*ext).abar;
    let port_regs = &mut (*abar).ports[port as usize];
    
    // Issue command
    StorPortWriteRegisterUlong(&mut port_regs.ci as *mut _, 1);
    
    // Poll for completion (FLUSH can take a long time - up to 30 seconds)
    for _ in 0..30000 {  // 30 seconds max
        StorPortStallExecution(1000); // 1ms per iteration
        let ci = StorPortReadRegisterUlong(&mut port_regs.ci as *mut _);
        if ci == 0 {
            // Command complete - check for errors
            let tfd = StorPortReadRegisterUlong(&mut port_regs.tfd as *mut _);
            if (tfd & TFD_ERR) != 0 {
                msahci_print("[MSAHCI] FLUSH_CACHE completed with TFD error\n");
                return -2;
            }
            // Clear port interrupt status
            let port_is = StorPortReadRegisterUlong(&mut port_regs.is as *mut _);
            StorPortWriteRegisterUlong(&mut port_regs.is as *mut _, port_is);
            return 0;  // Success
        }
    }
    
    msahci_print("[MSAHCI] FLUSH_CACHE timeout\n");
    -3  // Timeout
}

/// Handle SCSIOP_START_STOP_UNIT - Start or stop the device
/// 
/// For AHCI, we handle:
/// - Start: Enable port (already done)
/// - Stop: Optionally issue STANDBY IMMEDIATE
/// - Eject: Not supported for hard drives
fn handle_start_stop_unit(
    ext: *mut MSAHCI_DEVICE_EXTENSION,
    srb: *mut SCSI_REQUEST_BLOCK,
    target_id: u8,
) -> BOOLEAN {
    unsafe {
        let cdb = &(*srb).Cdb;
        let start = (cdb[4] & 0x01) != 0;
        let loej = (cdb[4] & 0x02) != 0;  // Load/Eject
        
        msahci_print("[MSAHCI] START_STOP_UNIT: target=");
        msahci_print_hex(target_id as u64);
        msahci_print(" start=");
        msahci_print_hex(start as u64);
        msahci_print(" loej=");
        msahci_print_hex(loej as u64);
        msahci_print("\n");
        
        let port_info = &(*ext).ports[target_id as usize];
        
        if port_info.device_present == 0 {
            (*srb).SrbStatus = SRB_STATUS_NO_DEVICE;
            StorPortNotification(REQUEST_COMPLETE, ext as PVOID, srb);
            StorPortNotification(NEXT_REQUEST, ext as PVOID);
            return 1;
        }
        
        // For hard drives:
        // - Start with no eject: device already running, success
        // - Stop without eject: could issue STANDBY, but we'll just succeed
        // - Any eject operation: not supported for HDD
        
        if loej && !start {
            // Eject requested - not supported for HDD
            (*srb).SrbStatus = SRB_STATUS_INVALID_REQUEST;
        } else {
            (*srb).SrbStatus = SRB_STATUS_SUCCESS;
        }
        
        StorPortNotification(REQUEST_COMPLETE, ext as PVOID, srb);
        StorPortNotification(NEXT_REQUEST, ext as PVOID);
        1
    }
}

/// Максимальный размер одного PRDT entry (4MB - 2 bytes, выровнено вниз до 512)
const PRDT_MAX_BYTES: u32 = (4 * 1024 * 1024) - 512;
/// Максимум PRDT entries в одной команде
const MAX_PRDT_ENTRIES: usize = 8;

// =============================================================================
// Async I/O Support (Phase 3.1)
// =============================================================================

/// Issue async I/O command - returns immediately after programming HBA
/// 
/// # Arguments
/// * `ext` - Device extension
/// * `port` - Port number
/// * `slot` - Command slot to use (must be pre-allocated)
/// * `srb` - SRB for this command (stored for completion)
/// * `lba` - Starting LBA
/// * `count` - Sector count
/// * `buffer` - Data buffer
/// * `is_write` - true for write, false for read
/// 
/// # Returns
/// 0 on success (command issued), negative on error
unsafe fn ahci_issue_command(
    ext: *mut MSAHCI_DEVICE_EXTENSION,
    port: u8,
    slot: u8,
    srb: *mut SCSI_REQUEST_BLOCK,
    lba: u64,
    count: u16,
    buffer: PVOID,
    is_write: bool,
) -> i32 {
    #[cfg(feature = "storage-trace")]
    {
        msahci_print("[MSAHCI] ahci_issue_command: port=");
        msahci_print_hex(port as u64);
        msahci_print(" slot=");
        msahci_print_hex(slot as u64);
        msahci_print(" lba=");
        msahci_print_hex(lba);
        msahci_print(" count=");
        msahci_print_hex(count as u64);
        msahci_print("\n");
    }
    
    let port_info = &mut (*ext).ports[port as usize];
    let cmd_list = port_info.cmd_list;
    let cmd_table = port_info.cmd_tables[slot as usize];
    
    if cmd_list.is_null() || cmd_table.is_null() {
        msahci_print("[MSAHCI] ERROR: cmd_list or cmd_table is NULL\n");
        return -1;
    }

    let total_bytes = count as u32 * 512;
    let mut prdt_count: u16 = 0;
    
    // Try to use Scatter-Gather list from StorPort if SRB is available
    let sg_list = if !srb.is_null() {
        StorPortGetScatterGatherList(ext as PVOID, srb)
    } else {
        ptr::null_mut()
    };
    
    if !sg_list.is_null() {
        // === Build PRDT from Scatter-Gather list ===
        #[cfg(feature = "storage-trace")]
        {
            msahci_print("[MSAHCI] Building PRDT from SG list, elements=");
            msahci_print_hex((*sg_list).NumberOfElements as u64);
            msahci_print("\n");
        }
        
        let num_elements = (*sg_list).NumberOfElements as usize;
        let mut bytes_remaining = total_bytes;
        
        for i in 0..num_elements {
            if bytes_remaining == 0 || (prdt_count as usize) >= MAX_PRDT_ENTRIES {
                break;
            }
            
            let sg_element = (*sg_list).element(i);
            let mut sg_offset: u64 = 0;
            let mut sg_length = sg_element.Length as u32;
            
            // One SG element may span multiple PRDT entries if > PRDT_MAX_BYTES
            while sg_length > 0 && bytes_remaining > 0 && (prdt_count as usize) < MAX_PRDT_ENTRIES {
                let chunk_size = sg_length
                    .min(PRDT_MAX_BYTES)
                    .min(bytes_remaining);
                
                let prdt = &mut (*cmd_table).prdt[prdt_count as usize];
                let phys_addr = sg_element.PhysicalAddress + sg_offset;
                prdt.dba = phys_addr as u32;
                prdt.dbau = (phys_addr >> 32) as u32;
                prdt.reserved = 0;
                // DBC = byte count - 1, bit 0 for interrupt on completion (set on last entry)
                prdt.dbc = (chunk_size - 1) | 1;
                
                #[cfg(feature = "storage-trace")]
                {
                    msahci_print("[MSAHCI] PRDT[");
                    msahci_print_hex(prdt_count as u64);
                    msahci_print("]: phys=0x");
                    msahci_print_hex(phys_addr);
                    msahci_print(" len=");
                    msahci_print_hex(chunk_size as u64);
                    msahci_print("\n");
                }
                
                sg_offset += chunk_size as u64;
                sg_length -= chunk_size;
                bytes_remaining -= chunk_size;
                prdt_count += 1;
            }
        }
        
        // Free the SG list
        let is_write_bool = if is_write { 1u8 } else { 0u8 };
        StorPortPutScatterGatherList(ext as PVOID, sg_list, is_write_bool);
        
        if bytes_remaining > 0 {
            msahci_print("[MSAHCI] ERROR: SG list too short for transfer\n");
            return -1;
        }
    } else {
        // === Fallback: build PRDT using StorPortGetPhysicalAddress ===
        #[cfg(feature = "storage-trace")]
        msahci_print("[MSAHCI] Building PRDT via StorPortGetPhysicalAddress\n");
        
        let mut remaining_bytes = total_bytes;
        let mut current_offset: usize = 0;
        
        while remaining_bytes > 0 && (prdt_count as usize) < MAX_PRDT_ENTRIES {
            let current_va = (buffer as usize + current_offset) as PVOID;
            
            let mut length: ULONG = 0;
            let phys_addr = StorPortGetPhysicalAddress(
                ext as PVOID,
                ptr::null_mut(),
                current_va,
                &mut length,
            );
            
            if phys_addr == 0 {
                msahci_print("[MSAHCI] ERROR: Failed to get physical address\n");
                return -1;
            }
            
            let chunk_size = remaining_bytes
                .min(PRDT_MAX_BYTES)
                .min(length);
            
            let prdt = &mut (*cmd_table).prdt[prdt_count as usize];
            prdt.dba = phys_addr as u32;
            prdt.dbau = (phys_addr >> 32) as u32;
            prdt.reserved = 0;
            // DBC = byte count - 1, bit 0 set for interrupt on completion
            prdt.dbc = (chunk_size - 1) | 1;
            
            current_offset += chunk_size as usize;
            remaining_bytes -= chunk_size;
            prdt_count += 1;
        }
        
        if remaining_bytes > 0 {
            msahci_print("[MSAHCI] ERROR: Buffer too fragmented\n");
            return -1;
        }
    }

    let header = &mut (*cmd_list).entries[slot as usize];
    // Flags: CFL (command FIS length in DWORDs) = 5, Write bit if is_write
    header.flags = (5 << 0) | if is_write { 1 << 6 } else { 0 };
    header.prdtl = prdt_count;
    header.prdbc = 0;

    // Подготовка Command FIS
    let fis = &mut (*cmd_table).cfis as *mut _ as *mut FIS_REG_H2D;
    ptr::write_bytes(fis, 0, 1);
    (*fis).fis_type = FIS_TYPE_REG_H2D;
    (*fis).pmport_c = 0x80; // C bit = 1 (command register update)
    (*fis).command = if is_write { ATA_CMD_WRITE_DMA_EXT } else { ATA_CMD_READ_DMA_EXT };
    (*fis).lba0 = lba as u8;
    (*fis).lba1 = (lba >> 8) as u8;
    (*fis).lba2 = (lba >> 16) as u8;
    (*fis).lba3 = (lba >> 24) as u8;
    (*fis).lba4 = (lba >> 32) as u8;
    (*fis).lba5 = (lba >> 40) as u8;
    (*fis).device = 1 << 6; // LBA mode
    (*fis).countl = count as u8;
    (*fis).counth = (count >> 8) as u8;

    // Store SRB for completion handler
    port_info.set_srb(slot, srb as PVOID);
    port_info.last_issued_slot = 1u32 << slot;
    
    let abar = (*ext).abar;
    let port_regs = &mut (*abar).ports[port as usize];

    // === DIAGNOSTIC: Check port state before issuing ===
    let tfd_before = StorPortReadRegisterUlong(&mut port_regs.tfd as *mut _);
    let ci_before = StorPortReadRegisterUlong(&mut port_regs.ci as *mut _);
    let is_before = StorPortReadRegisterUlong(&mut port_regs.is as *mut _);
    msahci_print("[MSAHCI] DIAG PRE-ISSUE: port=");
    msahci_print_hex(port as u64);
    msahci_print(" TFD=0x");
    msahci_print_hex(tfd_before as u64);
    msahci_print(" CI=0x");
    msahci_print_hex(ci_before as u64);
    msahci_print(" IS=0x");
    msahci_print_hex(is_before as u64);
    msahci_print("\n");

    // Issue command
    StorPortWriteRegisterUlong(&mut port_regs.ci as *mut _, 1u32 << slot);
    
    msahci_print("[MSAHCI] Command issued to slot ");
    msahci_print_hex(slot as u64);
    msahci_print("\n");

    // Poll for completion (synchronous mode)
    // TODO: In async mode, return immediately and handle completion in HwInterrupt
    let mut completed = false;
    for i in 0..100u32 {
        StorPortStallExecution(1000); // 1ms
        let ci_now = StorPortReadRegisterUlong(&mut port_regs.ci as *mut _);
        let tfd_now = StorPortReadRegisterUlong(&mut port_regs.tfd as *mut _);
        
        #[cfg(feature = "storage-trace")]
        if i == 0 || i == 50 || i == 99 || ci_now == 0 {
            let is_now = StorPortReadRegisterUlong(&mut port_regs.is as *mut _);
            msahci_print("[MSAHCI] DIAG POLL[");
            msahci_print_hex(i as u64);
            msahci_print("]: CI=0x");
            msahci_print_hex(ci_now as u64);
            msahci_print(" IS=0x");
            msahci_print_hex(is_now as u64);
            msahci_print(" TFD=0x");
            msahci_print_hex(tfd_now as u64);
            msahci_print("\n");
        }
        
        if ci_now == 0 {
            // Check for errors in TFD
            if (tfd_now & 0x01) != 0 { // ERR bit
                msahci_print("[MSAHCI] ERROR: Command completed with TFD error\n");
                // Clear SRB reference
                port_info.set_srb(slot, ptr::null_mut());
                return -2; // Error completion
            }
            completed = true;
            break;
        }
    }
    
    // Clear SRB reference (we're handling completion synchronously)
    port_info.set_srb(slot, ptr::null_mut());
    
    if !completed {
        msahci_print("[MSAHCI] ERROR: Command timeout\n");
        return -3; // Timeout
    }
    
    // Clear port interrupt status
    let port_is = StorPortReadRegisterUlong(&mut port_regs.is as *mut _);
    StorPortWriteRegisterUlong(&mut port_regs.is as *mut _, port_is);
    
    0 // Success
}

/// Legacy synchronous I/O (for commands that need immediate completion)
/// Used for INQUIRY, READ_CAPACITY, etc.
unsafe fn ahci_do_io_sync(
    ext: *mut MSAHCI_DEVICE_EXTENSION,
    port: u8,
    lba: u64,
    count: u16,
    buffer: PVOID,
    is_write: bool,
) -> i32 {
    let port_info = &mut (*ext).ports[port as usize];
    let cmd_list = port_info.cmd_list;
    let cmd_table = port_info.cmd_tables[0];
    
    if cmd_list.is_null() || cmd_table.is_null() {
        return -1;
    }

    let total_bytes = count as u32 * 512;
    let mut remaining_bytes = total_bytes;
    let mut current_offset: usize = 0;
    let mut prdt_count: u16 = 0;
    
    while remaining_bytes > 0 && (prdt_count as usize) < MAX_PRDT_ENTRIES {
        let current_va = (buffer as usize + current_offset) as PVOID;
        
        let mut length: ULONG = 0;
        let phys_addr = StorPortGetPhysicalAddress(
            ext as PVOID,
            ptr::null_mut(),
            current_va,
            &mut length,
        );
        
        if phys_addr == 0 {
            return -1;
        }
        
        let chunk_size = remaining_bytes
            .min(PRDT_MAX_BYTES)
            .min(length);
        
        let prdt = &mut (*cmd_table).prdt[prdt_count as usize];
        prdt.dba = phys_addr as u32;
        prdt.dbau = (phys_addr >> 32) as u32;
        prdt.reserved = 0;
        prdt.dbc = (chunk_size - 1) | 1;
        
        current_offset += chunk_size as usize;
        remaining_bytes -= chunk_size;
        prdt_count += 1;
    }
    
    if remaining_bytes > 0 {
        return -1;
    }

    let header = &mut (*cmd_list).entries[0];
    header.flags = (5 << 0) | if is_write { 1 << 6 } else { 0 };
    header.prdtl = prdt_count;
    header.prdbc = 0;

    let fis = &mut (*cmd_table).cfis as *mut _ as *mut FIS_REG_H2D;
    ptr::write_bytes(fis, 0, 1);
    (*fis).fis_type = FIS_TYPE_REG_H2D;
    (*fis).pmport_c = 0x80;
    (*fis).command = if is_write { ATA_CMD_WRITE_DMA_EXT } else { ATA_CMD_READ_DMA_EXT };
    (*fis).lba0 = lba as u8;
    (*fis).lba1 = (lba >> 8) as u8;
    (*fis).lba2 = (lba >> 16) as u8;
    (*fis).lba3 = (lba >> 24) as u8;
    (*fis).lba4 = (lba >> 32) as u8;
    (*fis).lba5 = (lba >> 40) as u8;
    (*fis).device = 1 << 6;
    (*fis).countl = count as u8;
    (*fis).counth = (count >> 8) as u8;

    let abar = (*ext).abar;
    let port_regs = &mut (*abar).ports[port as usize];

    StorPortWriteRegisterUlong(&mut port_regs.ci as *mut _, 1);

    // Poll for completion (sync mode)
    for _ in 0..5000 {
        let ci = StorPortReadRegisterUlong(&mut port_regs.ci as *mut _);
        if ci == 0 {
            let tfd = StorPortReadRegisterUlong(&mut port_regs.tfd as *mut _);
            if (tfd & 0x01) != 0 {
                return -1;
            }
            return 0;
        }
        StorPortStallExecution(1000);
    }

    -1 // Timeout
}

/// HwInterrupt - Phase 3.1: Process command completions
/// 
/// Called by StorPort when an interrupt occurs. Checks which commands
/// have completed and calls StorPortNotification(REQUEST_COMPLETE) for each.
unsafe extern "win64" fn MsAhciInterrupt(device_extension: PVOID) -> BOOLEAN {
    unsafe {
        // === DIAGNOSTIC: Always print when interrupt handler is called ===
        msahci_print("[MSAHCI] >>> HwInterrupt CALLED <<<\n");
        
        let ext = device_extension as *mut MSAHCI_DEVICE_EXTENSION;
        let abar = (*ext).abar;

        // Read global interrupt status
        let is = StorPortReadRegisterUlong(&mut (*abar).is as *mut _);
        
        msahci_print("[MSAHCI] HwInterrupt: Global IS=0x");
        msahci_print_hex(is as u64);
        msahci_print("\n");
        
        if is == 0 {
            msahci_print("[MSAHCI] HwInterrupt: IS=0, not our interrupt\n");
            return 0; // Not our interrupt
        }

        // Clear global interrupt status
        StorPortWriteRegisterUlong(&mut (*abar).is as *mut _, is);

        // Process each port with pending interrupt
        for port in 0..32usize {
            if (is & (1 << port)) == 0 {
                continue;
            }
            
            let port_info = &mut (*ext).ports[port];
            if port_info.device_present == 0 {
                continue;
            }
            
            let port_regs = &mut (*abar).ports[port];
            
            // Read and clear port interrupt status
            let port_is = StorPortReadRegisterUlong(&mut port_regs.is as *mut _);
            StorPortWriteRegisterUlong(&mut port_regs.is as *mut _, port_is);
            
            // Check which commands have completed
            // CI bit is 0 when command completes
            let ci = StorPortReadRegisterUlong(&mut port_regs.ci as *mut _);
            let tfd = StorPortReadRegisterUlong(&mut port_regs.tfd as *mut _);
            
            // Find completed commands (bits that were set but now cleared)
            let completed = port_info.slot_bitmap & !ci;
            
            if completed != 0 {
                #[cfg(feature = "storage-trace")]
                {
                    msahci_print("[MSAHCI] Port ");
                    msahci_print_hex(port as u64);
                    msahci_print(" completed=0x");
                    msahci_print_hex(completed as u64);
                    msahci_print(" tfd=0x");
                    msahci_print_hex(tfd as u64);
                    msahci_print("\n");
                }
                
                // Process each completed slot
                for slot in 0..32u8 {
                    let mask = 1u32 << slot;
                    if (completed & mask) == 0 {
                        continue;
                    }
                    
                    let srb = port_info.get_srb(slot) as *mut SCSI_REQUEST_BLOCK;
                    if srb.is_null() {
                        // No SRB for this slot - shouldn't happen
                        port_info.free_slot(slot);
                        continue;
                    }
                    
                    // Check for errors (TFD ERR bit or port errors)
                    let has_error = (tfd & TFD_ERR) != 0 || (port_is & PORT_IS_ERROR_MASK) != 0;
                    
                    if has_error {
                        // Read SError for detailed error info
                        let serr = StorPortReadRegisterUlong(&mut port_regs.serr as *mut _);
                        
                        #[cfg(feature = "storage-trace")]
                        {
                            msahci_print("[MSAHCI] Slot ");
                            msahci_print_hex(slot as u64);
                            msahci_print(" ERROR: TFD=0x");
                            msahci_print_hex(tfd as u64);
                            msahci_print(" IS=0x");
                            msahci_print_hex(port_is as u64);
                            msahci_print(" SError=0x");
                            msahci_print_hex(serr as u64);
                            msahci_print("\n");
                        }
                        
                        // Map AHCI error to SRB status
                        (*srb).SrbStatus = map_ahci_error_to_srb_status(port_is, tfd, serr);
                        
                        // Clear SError register
                        StorPortWriteRegisterUlong(&mut port_regs.serr as *mut _, serr);
                    } else {
                        (*srb).SrbStatus = SRB_STATUS_SUCCESS;
                    }
                    
                    // Free the slot
                    port_info.free_slot(slot);
                    
                    // Notify StorPort of completion
                    StorPortNotification(REQUEST_COMPLETE, device_extension, srb);
                }
            }
            
            // Check for fatal port errors that require reset
            // Fatal errors: HBFS (Host Bus Fatal), IFS (Interface Fatal)
            let fatal_error = (port_is & (PORT_IS_HBFS | PORT_IS_IFS)) != 0;
            
            if fatal_error {
                msahci_print("[MSAHCI] FATAL ERROR on port ");
                msahci_print_hex(port as u64);
                msahci_print(" - initiating port reset\n");
                
                // Reset the port to recover from fatal error
                ahci_reset_port(ext, port as u8);
            }
        }

        1 // We handled the interrupt
    }
}

// Port interrupt status error bits
const PORT_IS_ERROR_MASK: u32 = 
    (1 << 30) |  // TFES - Task File Error Status
    (1 << 29) |  // HBFS - Host Bus Fatal Error Status
    (1 << 28) |  // HBDS - Host Bus Data Error Status
    (1 << 27) |  // IFS  - Interface Fatal Error Status
    (1 << 26) |  // INFS - Interface Non-fatal Error Status
    (1 << 24) |  // OFS  - Overflow Status
    (1 << 23);   // IPMS - Incorrect Port Multiplier Status

// Port interrupt status bits for classification
const PORT_IS_TFES: u32 = 1 << 30;  // Task File Error Status
const PORT_IS_HBFS: u32 = 1 << 29;  // Host Bus Fatal Error
const PORT_IS_HBDS: u32 = 1 << 28;  // Host Bus Data Error
const PORT_IS_IFS: u32 = 1 << 27;   // Interface Fatal Error

// TFD (Task File Data) register bits
const TFD_ERR: u32 = 1 << 0;        // Error bit
const TFD_DRQ: u32 = 1 << 3;        // Data request
const TFD_BSY: u32 = 1 << 7;        // Busy

/// Map AHCI error status to SRB status code
/// 
/// Per Windows DDK, translates AHCI-specific errors to appropriate SRB_STATUS_* values.
fn map_ahci_error_to_srb_status(port_is: u32, tfd: u32, serr: u32) -> u8 {
    // Fatal interface errors -> internal error
    if (port_is & PORT_IS_IFS) != 0 {
        return SRB_STATUS_INTERNAL_ERROR;
    }
    
    // Host bus errors -> parity error (closest match)
    if (port_is & (PORT_IS_HBFS | PORT_IS_HBDS)) != 0 {
        return SRB_STATUS_PARITY_ERROR;
    }
    
    // Task file error - check specific error register bits
    if (port_is & PORT_IS_TFES) != 0 || (tfd & TFD_ERR) != 0 {
        // Error register is in bits 15:8 of TFD
        let error_reg = ((tfd >> 8) & 0xFF) as u8;
        
        // Map ATA error register bits to SRB status
        // Bit 2 (ABRT) = Aborted command
        if (error_reg & 0x04) != 0 {
            return SRB_STATUS_ABORTED;
        }
        // Bit 4 (IDNF) = ID Not Found / sector not found
        if (error_reg & 0x10) != 0 {
            return SRB_STATUS_ERROR;
        }
        // Bit 6 (UNC) = Uncorrectable data error
        if (error_reg & 0x40) != 0 {
            return SRB_STATUS_PARITY_ERROR;
        }
        // Bit 7 (ICRC) = Interface CRC error
        if (error_reg & 0x80) != 0 {
            return SRB_STATUS_PARITY_ERROR;
        }
        
        // Generic error
        return SRB_STATUS_ERROR;
    }
    
    // Check SError for SATA link errors
    if serr != 0 {
        // Bit 0 (ERR.I) = Recovered data integrity error
        // Bit 1 (ERR.M) = Recovered communications error
        // Bit 8 (DIAG.N) = PHY Internal error
        // Bit 16 (DIAG.X) = Exchange
        return SRB_STATUS_PARITY_ERROR;
    }
    
    SRB_STATUS_ERROR
}

/// Reset an AHCI port after error condition
/// 
/// Performs COMRESET sequence per AHCI 1.3.1 spec section 10.4.2
unsafe fn ahci_reset_port(ext: *mut MSAHCI_DEVICE_EXTENSION, port: u8) -> i32 {
    msahci_print("[MSAHCI] Resetting port ");
    msahci_print_hex(port as u64);
    msahci_print("\n");
    
    let abar = (*ext).abar;
    if abar.is_null() {
        msahci_print("[MSAHCI] ERROR: ABAR is NULL in reset_port\n");
        return -1;
    }
    
    let port_regs = &mut (*abar).ports[port as usize];
    let port_info = &mut (*ext).ports[port as usize];
    
    // 1. Stop the port (clear ST and FRE)
    let cmd = StorPortReadRegisterUlong(&mut port_regs.cmd as *mut _);
    StorPortWriteRegisterUlong(&mut port_regs.cmd as *mut _, cmd & !(PORT_CMD_ST | PORT_CMD_FRE));
    
    // 2. Wait for port to stop (CR and FR should clear)
    for _ in 0..500 {
        let cmd_check = StorPortReadRegisterUlong(&mut port_regs.cmd as *mut _);
        if (cmd_check & (PORT_CMD_CR | PORT_CMD_FR)) == 0 {
            break;
        }
        StorPortStallExecution(1000);
    }
    
    // 3. Clear error registers
    StorPortWriteRegisterUlong(&mut port_regs.serr as *mut _, 0xFFFFFFFF);
    StorPortWriteRegisterUlong(&mut port_regs.is as *mut _, 0xFFFFFFFF);
    
    // 4. Issue COMRESET by setting DET to 1 in SCTL
    let sctl = StorPortReadRegisterUlong(&mut port_regs.sctl as *mut _);
    StorPortWriteRegisterUlong(&mut port_regs.sctl as *mut _, (sctl & !0xF) | 1);
    
    // 5. Wait at least 1ms
    StorPortStallExecution(2000);
    
    // 6. Clear DET to 0 to allow link negotiation
    StorPortWriteRegisterUlong(&mut port_regs.sctl as *mut _, sctl & !0xF);
    
    // 7. Wait for link re-establishment
    let mut link_up = false;
    for _ in 0..100 {
        StorPortStallExecution(10000); // 10ms per iteration
        let ssts = StorPortReadRegisterUlong(&mut port_regs.ssts as *mut _);
        let det = ssts & PORT_SSTS_DET_MASK;
        if det == PORT_SSTS_DET_PHY {
            link_up = true;
            break;
        }
    }
    
    if !link_up {
        msahci_print("[MSAHCI] WARNING: Link did not re-establish after reset\n");
        port_info.device_present = 0;
        return -2;
    }
    
    // 8. Clear any errors that occurred during reset
    StorPortWriteRegisterUlong(&mut port_regs.serr as *mut _, 0xFFFFFFFF);
    
    // 9. Wait for device ready (BSY and DRQ clear)
    for _ in 0..100 {
        let tfd = StorPortReadRegisterUlong(&mut port_regs.tfd as *mut _);
        if (tfd & (TFD_BSY | TFD_DRQ)) == 0 {
            break;
        }
        StorPortStallExecution(10000);
    }
    
    // 10. Re-enable FRE and ST
    let cmd_new = StorPortReadRegisterUlong(&mut port_regs.cmd as *mut _) | PORT_CMD_FRE;
    StorPortWriteRegisterUlong(&mut port_regs.cmd as *mut _, cmd_new);
    StorPortStallExecution(500);
    StorPortWriteRegisterUlong(&mut port_regs.cmd as *mut _, cmd_new | PORT_CMD_ST);
    
    // 11. Cancel all pending requests on this port
    for slot in 0..32u8 {
        let srb = port_info.get_srb(slot) as *mut SCSI_REQUEST_BLOCK;
        if !srb.is_null() {
            (*srb).SrbStatus = SRB_STATUS_BUS_RESET;
            port_info.free_slot(slot);
            StorPortNotification(REQUEST_COMPLETE, ext as PVOID, srb);
        }
    }
    port_info.slot_bitmap = 0;
    
    msahci_print("[MSAHCI] Port reset complete\n");
    0
}

unsafe extern "win64" fn MsAhciResetBus(device_extension: PVOID, path_id: ULONG) -> BOOLEAN {
    msahci_print("[MSAHCI] MsAhciResetBus called, path_id=");
    msahci_print_hex(path_id as u64);
    msahci_print("\n");
    
    let ext = device_extension as *mut MSAHCI_DEVICE_EXTENSION;
    if ext.is_null() {
        return 0;
    }
    
    // path_id maps to AHCI port in our implementation
    // If path_id is 0xFF, reset all ports; otherwise reset specific port
    if path_id == 0xFF {
        // Reset all implemented ports
        let pi = (*ext).ports_implemented;
        for port in 0..32u8 {
            if (pi & (1u32 << port)) != 0 && (*ext).ports[port as usize].device_present != 0 {
                ahci_reset_port(ext, port);
            }
        }
    } else {
        let port = path_id as u8;
        if (port as usize) < 32 && ((*ext).ports_implemented & (1u32 << port)) != 0 {
            ahci_reset_port(ext, port);
        }
    }
    
    1
}

unsafe extern "win64" fn MsAhciAdapterControl(
    _device_extension: PVOID,
    _control_type: SCSI_ADAPTER_CONTROL_TYPE,
    _parameters: PVOID,
) -> SCSI_ADAPTER_CONTROL_STATUS {
    SCSI_ADAPTER_CONTROL_SUCCESS
}

// =============================================================================
// Debug Helpers (только при storage-trace feature)
// =============================================================================

#[cfg(feature = "storage-trace")]
unsafe fn msahci_print(s: &str) {
    use imports::storport::DbgPrint;
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
unsafe fn msahci_print(_s: &str) {}

#[cfg(feature = "storage-trace")]
unsafe fn msahci_print_hex(value: u64) {
    use imports::storport::DbgPrint;
    const HEX_CHARS: &[u8] = b"0123456789ABCDEF";
    let mut buf = [0u8; 17];
    for i in 0..16 {
        let nibble = ((value >> (60 - i * 4)) & 0xF) as usize;
        buf[i] = HEX_CHARS[nibble];
    }
    buf[16] = 0;
    // Пропускаем ведущие нули
    let mut start = 0;
    while start < 15 && buf[start] == b'0' {
        start += 1;
    }
    DbgPrint(buf[start..].as_ptr());
}

#[cfg(not(feature = "storage-trace"))]
#[inline(always)]
unsafe fn msahci_print_hex(_value: u64) {}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

#[unsafe(no_mangle)]
pub static _fltused: i32 = 0;

#[unsafe(no_mangle)]
pub extern "C" fn __chkstk() {}

#[unsafe(no_mangle)]
pub extern "C" fn _chkstk() {}

