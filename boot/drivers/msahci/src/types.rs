//! AHCI Miniport Types

pub type ULONG = u32;
pub type USHORT = u16;
pub type UCHAR = u8;
pub type BOOLEAN = u8;
pub type PVOID = *mut core::ffi::c_void;
pub type PHYSICAL_ADDRESS = u64;

// =============================================================================
// AHCI HBA Registers
// =============================================================================

#[repr(C)]
pub struct AHCI_HBA_MEM {
    pub cap: u32,
    pub ghc: u32,
    pub is: u32,
    pub pi: u32,
    pub vs: u32,
    pub ccc_ctl: u32,
    pub ccc_ports: u32,
    pub em_loc: u32,
    pub em_ctl: u32,
    pub cap2: u32,
    pub bohc: u32,
    pub reserved: [u8; 0xA0 - 0x2C],
    pub vendor: [u8; 0x100 - 0xA0],
    pub ports: [AHCI_HBA_PORT; 32],
}

#[repr(C)]
pub struct AHCI_HBA_PORT {
    pub clb: u32,
    pub clbu: u32,
    pub fb: u32,
    pub fbu: u32,
    pub is: u32,
    pub ie: u32,
    pub cmd: u32,
    pub reserved0: u32,
    pub tfd: u32,
    pub sig: u32,
    pub ssts: u32,
    pub sctl: u32,
    pub serr: u32,
    pub sact: u32,
    pub ci: u32,
    pub sntf: u32,
    pub fbs: u32,
    pub reserved1: [u32; 11],
    pub vendor: [u32; 4],
}

// GHC bits
pub const GHC_AE: u32 = 1 << 31;
pub const GHC_IE: u32 = 1 << 1;
pub const GHC_HR: u32 = 1 << 0;

// CAP bits
pub const CAP_NCS_MASK: u32 = 0x1F << 8;
pub const CAP_NCS_SHIFT: u32 = 8;
pub const CAP_S64A: u32 = 1 << 31;
pub const CAP_SNCQ: u32 = 1 << 30;

// PORT CMD bits
pub const PORT_CMD_ST: u32 = 1 << 0;
pub const PORT_CMD_FRE: u32 = 1 << 4;
pub const PORT_CMD_FR: u32 = 1 << 14;
pub const PORT_CMD_CR: u32 = 1 << 15;

// PORT IE (Interrupt Enable) bits
pub const PORT_IE_DHRE: u32 = 1 << 0;   // Device to Host Register FIS Interrupt
pub const PORT_IE_PSE: u32 = 1 << 1;    // PIO Setup FIS Interrupt
pub const PORT_IE_DSE: u32 = 1 << 2;    // DMA Setup FIS Interrupt
pub const PORT_IE_SDBE: u32 = 1 << 3;   // Set Device Bits FIS Interrupt
pub const PORT_IE_UFE: u32 = 1 << 4;    // Unknown FIS Interrupt
pub const PORT_IE_DPE: u32 = 1 << 5;    // Descriptor Processed Interrupt
pub const PORT_IE_PCE: u32 = 1 << 6;    // Port Connect Change Interrupt
pub const PORT_IE_DMPE: u32 = 1 << 7;   // Device Mechanical Presence
pub const PORT_IE_PRCE: u32 = 1 << 22;  // PhyRdy Change Interrupt
pub const PORT_IE_IPME: u32 = 1 << 23;  // Incorrect Port Multiplier
pub const PORT_IE_OFE: u32 = 1 << 24;   // Overflow Interrupt
pub const PORT_IE_INFE: u32 = 1 << 26;  // Interface Non-fatal Error
pub const PORT_IE_IFE: u32 = 1 << 27;   // Interface Fatal Error
pub const PORT_IE_HBDE: u32 = 1 << 28;  // Host Bus Data Error
pub const PORT_IE_HBFE: u32 = 1 << 29;  // Host Bus Fatal Error
pub const PORT_IE_TFEE: u32 = 1 << 30;  // Task File Error

// PORT SSTS bits
pub const PORT_SSTS_DET_MASK: u32 = 0xF;
pub const PORT_SSTS_DET_PHY: u32 = 0x3;

// SATA Signatures
pub const SATA_SIG_ATA: u32 = 0x00000101;
pub const SATA_SIG_ATAPI: u32 = 0xEB140101;
pub const SATA_SIG_SEMB: u32 = 0xC33C0101;
pub const SATA_SIG_PM: u32 = 0x96690101;

// =============================================================================
// AHCI Command Structures
// =============================================================================

#[repr(C, align(1024))]
pub struct AHCI_CMD_LIST {
    pub entries: [AHCI_CMD_HEADER; 32],
}

#[repr(C)]
pub struct AHCI_CMD_HEADER {
    pub flags: u16,
    pub prdtl: u16,
    pub prdbc: u32,
    pub ctba: u32,
    pub ctbau: u32,
    pub reserved: [u32; 4],
}

#[repr(C, align(256))]
pub struct AHCI_FIS_RECV {
    pub dsfis: [u8; 0x20],
    pub psfis: [u8; 0x20],
    pub rfis: [u8; 0x18],
    pub sdbfis: [u8; 0x08],
    pub ufis: [u8; 0x40],
    pub reserved: [u8; 0x60],
}

#[repr(C, align(128))]
pub struct AHCI_CMD_TABLE {
    pub cfis: [u8; 64],
    pub acmd: [u8; 16],
    pub reserved: [u8; 48],
    pub prdt: [AHCI_PRDT_ENTRY; 8],
}

#[repr(C)]
pub struct AHCI_PRDT_ENTRY {
    pub dba: u32,
    pub dbau: u32,
    pub reserved: u32,
    pub dbc: u32,
}

// =============================================================================
// FIS Types
// =============================================================================

pub const FIS_TYPE_REG_H2D: u8 = 0x27;
pub const FIS_TYPE_REG_D2H: u8 = 0x34;

#[repr(C)]
pub struct FIS_REG_H2D {
    pub fis_type: u8,
    pub pmport_c: u8,
    pub command: u8,
    pub featurel: u8,
    pub lba0: u8,
    pub lba1: u8,
    pub lba2: u8,
    pub device: u8,
    pub lba3: u8,
    pub lba4: u8,
    pub lba5: u8,
    pub featureh: u8,
    pub countl: u8,
    pub counth: u8,
    pub icc: u8,
    pub control: u8,
    pub reserved: [u8; 4],
}

// ATA Commands (ATA8-ACS)
pub const ATA_CMD_READ_DMA_EXT: u8 = 0x25;
pub const ATA_CMD_WRITE_DMA_EXT: u8 = 0x35;
pub const ATA_CMD_FLUSH_CACHE: u8 = 0xE7;
pub const ATA_CMD_FLUSH_CACHE_EXT: u8 = 0xEA;
pub const ATA_CMD_IDENTIFY: u8 = 0xEC;
pub const ATA_CMD_SET_FEATURES: u8 = 0xEF;
pub const ATA_CMD_STANDBY_IMMEDIATE: u8 = 0xE0;
pub const ATA_CMD_IDLE_IMMEDIATE: u8 = 0xE1;

// =============================================================================
// Miniport Device Extension
// =============================================================================

pub struct MSAHCI_DEVICE_EXTENSION {
    pub abar: *mut AHCI_HBA_MEM,
    pub abar_phys: PHYSICAL_ADDRESS,
    pub cap: u32,
    pub cap2: u32,
    pub version: u32,
    pub ports_implemented: u32,
    pub num_cmd_slots: u32,
    pub num_ports: u32,
    pub ports: [MSAHCI_PORT_INFO; 32],
}

// =============================================================================
// ATA IDENTIFY DEVICE Data (T13/1699-D ATA8-ACS)
// =============================================================================
//
// Reference: ATA/ATAPI Command Set - 3 (ACS-3), T13/2161-D
// Word offsets in 512-byte IDENTIFY DEVICE data:
//   Words 10-19: Serial number (20 ASCII chars)
//   Words 23-26: Firmware revision (8 ASCII chars)
//   Words 27-46: Model number (40 ASCII chars)
//

/// SCSI_REQUEST_BLOCK pointer type for outstanding commands
pub type PSCSI_REQUEST_BLOCK = *mut core::ffi::c_void;

#[derive(Clone, Copy)]
pub struct MSAHCI_PORT_INFO {
    pub port_number: u8,
    pub implemented: u8,
    pub device_present: u8,
    pub device_type: u8,
    pub cmd_list: *mut AHCI_CMD_LIST,
    pub cmd_list_phys: PHYSICAL_ADDRESS,
    pub fis_recv: *mut AHCI_FIS_RECV,
    pub fis_recv_phys: PHYSICAL_ADDRESS,
    pub cmd_tables: [*mut AHCI_CMD_TABLE; 32],
    pub cmd_tables_phys: [PHYSICAL_ADDRESS; 32],
    pub sector_count: u64,
    pub sector_size: u32,
    /// Model number from ATA IDENTIFY (40 chars, byte-swapped ASCII)
    pub model_number: [u8; 40],
    /// Serial number from ATA IDENTIFY (20 chars, byte-swapped ASCII)
    pub serial_number: [u8; 20],
    /// Firmware revision from ATA IDENTIFY (8 chars, byte-swapped ASCII)
    pub firmware_revision: [u8; 8],
    
    // --- Async I/O support (Phase 3.1) ---
    /// Outstanding SRBs - mapping slot index -> SRB pointer
    pub outstanding_srbs: [PSCSI_REQUEST_BLOCK; 32],
    /// Bitmap of occupied command slots (bit N = slot N is in use)
    pub slot_bitmap: u32,
    /// Last issued command slot (for CI tracking)
    pub last_issued_slot: u32,
}

impl MSAHCI_PORT_INFO {
    pub const fn new() -> Self {
        Self {
            port_number: 0,
            implemented: 0,
            device_present: 0,
            device_type: 0xFF,
            cmd_list: core::ptr::null_mut(),
            cmd_list_phys: 0,
            fis_recv: core::ptr::null_mut(),
            fis_recv_phys: 0,
            cmd_tables: [core::ptr::null_mut(); 32],
            cmd_tables_phys: [0; 32],
            sector_count: 0,
            sector_size: 512,
            model_number: [0x20; 40],      // Space-padded
            serial_number: [0x20; 20],     // Space-padded
            firmware_revision: [0x20; 8],  // Space-padded
            // Async I/O
            outstanding_srbs: [core::ptr::null_mut(); 32],
            slot_bitmap: 0,
            last_issued_slot: 0,
        }
    }
    
    /// Allocate a free command slot
    /// Returns slot index or None if all slots are busy
    #[inline]
    pub fn allocate_slot(&mut self) -> Option<u8> {
        // Find first zero bit in slot_bitmap
        for slot in 0..32u8 {
            let mask = 1u32 << slot;
            if (self.slot_bitmap & mask) == 0 {
                self.slot_bitmap |= mask;
                return Some(slot);
            }
        }
        None
    }
    
    /// Free a command slot
    #[inline]
    pub fn free_slot(&mut self, slot: u8) {
        if slot < 32 {
            let mask = 1u32 << slot;
            self.slot_bitmap &= !mask;
            self.outstanding_srbs[slot as usize] = core::ptr::null_mut();
        }
    }
    
    /// Get outstanding SRB for slot
    #[inline]
    pub fn get_srb(&self, slot: u8) -> PSCSI_REQUEST_BLOCK {
        if slot < 32 {
            self.outstanding_srbs[slot as usize]
        } else {
            core::ptr::null_mut()
        }
    }
    
    /// Set outstanding SRB for slot
    #[inline]
    pub fn set_srb(&mut self, slot: u8, srb: PSCSI_REQUEST_BLOCK) {
        if slot < 32 {
            self.outstanding_srbs[slot as usize] = srb;
        }
    }
}

impl MSAHCI_DEVICE_EXTENSION {
    pub const fn new() -> Self {
        Self {
            abar: core::ptr::null_mut(),
            abar_phys: 0,
            cap: 0,
            cap2: 0,
            version: 0,
            ports_implemented: 0,
            num_cmd_slots: 0,
            num_ports: 0,
            ports: [MSAHCI_PORT_INFO::new(); 32],
        }
    }
}

// =============================================================================
// Scatter-Gather List Types (for StorPortGetScatterGatherList)
// =============================================================================

/// Physical address type for scatter-gather operations
pub type STOR_PHYSICAL_ADDRESS = u64;
pub type ULONG_PTR = usize;

/// Single element in a scatter-gather list
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct STOR_SCATTER_GATHER_ELEMENT {
    /// Physical address of the memory region
    pub PhysicalAddress: STOR_PHYSICAL_ADDRESS,
    /// Length of the memory region in bytes
    pub Length: ULONG,
    /// Reserved, must be zero
    pub Reserved: ULONG_PTR,
}

/// Scatter/gather list for DMA transfers
#[repr(C)]
pub struct STOR_SCATTER_GATHER_LIST {
    /// Number of elements in the scatter/gather list
    pub NumberOfElements: ULONG,
    /// Reserved
    pub Reserved: ULONG_PTR,
    /// First element (array extends beyond this)
    pub List: [STOR_SCATTER_GATHER_ELEMENT; 1],
}

pub type PSTOR_SCATTER_GATHER_LIST = *mut STOR_SCATTER_GATHER_LIST;

impl STOR_SCATTER_GATHER_LIST {
    /// Get element at index (unsafe: no bounds checking beyond first)
    #[inline]
    pub unsafe fn element(&self, index: usize) -> &STOR_SCATTER_GATHER_ELEMENT {
        unsafe { &*((self.List.as_ptr()).add(index)) }
    }
}

