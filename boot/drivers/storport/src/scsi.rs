//! SCSI request обработчики

use crate::types::*;
use crate::imports::ntoskrnl::*;
use crate::miniport::*;
use crate::{storport_print, storport_print_hex};
use core::ptr;
use core::mem::MaybeUninit;

// =============================================================================
// Helper Functions
// =============================================================================

/// Преобразует SRB status в NTSTATUS
fn srb_status_to_ntstatus(srb_status: UCHAR) -> NTSTATUS {
    match srb_status {
        SRB_STATUS_SUCCESS => STATUS_SUCCESS,
        SRB_STATUS_PENDING => STATUS_PENDING,
        SRB_STATUS_ABORTED => STATUS_REQUEST_ABORTED,
        SRB_STATUS_ERROR => STATUS_IO_DEVICE_ERROR,
        SRB_STATUS_BUSY => STATUS_DEVICE_BUSY,
        SRB_STATUS_INVALID_REQUEST => STATUS_INVALID_DEVICE_REQUEST,
        SRB_STATUS_INVALID_PATH_ID => STATUS_INVALID_PARAMETER,
        SRB_STATUS_NO_DEVICE => STATUS_NO_SUCH_DEVICE,
        SRB_STATUS_TIMEOUT | SRB_STATUS_COMMAND_TIMEOUT => STATUS_IO_TIMEOUT,
        SRB_STATUS_SELECTION_TIMEOUT => STATUS_DEVICE_NOT_CONNECTED,
        SRB_STATUS_NO_HBA => STATUS_ADAPTER_HARDWARE_ERROR,
        SRB_STATUS_DATA_OVERRUN => STATUS_DATA_OVERRUN,
        SRB_STATUS_INVALID_LUN | SRB_STATUS_INVALID_TARGET_ID => STATUS_INVALID_PARAMETER,
        SRB_STATUS_NOT_POWERED => STATUS_DEVICE_POWERED_OFF,
        _ => STATUS_IO_DEVICE_ERROR,
    }
}

// =============================================================================
// PDO Extension (для поиска parent FDO)
// =============================================================================

/// SCSI PDO Extension
#[repr(C)]
struct SCSI_PDO_EXTENSION {
    signature: ULONG,
    parent_fdo: PDEVICE_OBJECT,
    path_id: UCHAR,
    target_id: UCHAR,
    lun: UCHAR,
}

const SCSI_PDO_SIGNATURE: ULONG = 0x4F445053; // 'SPDO'

// =============================================================================
// SCSI Dispatch
// =============================================================================

/// SCSI dispatch (IRP_MJ_SCSI для SRB)
///
/// Обрабатывает SCSI запросы как на FDO, так и на PDO.
/// Для PDO находит родительский FDO и обрабатывает запрос там.
pub unsafe fn storport_scsi_dispatch(
    device_object: PDEVICE_OBJECT,
    irp: PIRP,
) -> NTSTATUS {
    unsafe {
        storport_print("[STORPORT] SCSI request\n");
        
        let ext_ptr = (*device_object).device_extension;
        if ext_ptr.is_null() {
            storport_print("[STORPORT] ERROR: NULL device extension\n");
            (*irp).io_status.status = STATUS_INVALID_DEVICE_REQUEST;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            return STATUS_INVALID_DEVICE_REQUEST;
        }
        
        // Читаем signature чтобы определить тип extension
        let signature = *(ext_ptr as *const ULONG);
        
        let fdo_ext: *mut STORPORT_DEVICE_EXTENSION;
        let pdo_ext: *const SCSI_PDO_EXTENSION;
        
        if signature == STORPORT_DEVICE_EXTENSION_SIGNATURE {
            // Это FDO - используем напрямую
            storport_print("[STORPORT] SCSI on FDO\n");
            fdo_ext = ext_ptr as *mut STORPORT_DEVICE_EXTENSION;
            pdo_ext = ptr::null();
        } else if signature == SCSI_PDO_SIGNATURE {
            // Это PDO - находим родительский FDO
            storport_print("[STORPORT] SCSI on PDO, forwarding to parent FDO\n");
            pdo_ext = ext_ptr as *const SCSI_PDO_EXTENSION;
            
            storport_print("[STORPORT]   PDO ext ptr=0x");
            storport_print_hex(pdo_ext as u64);
            storport_print("\n");
            
            let parent_fdo = (*pdo_ext).parent_fdo;
            
            storport_print("[STORPORT]   parent_fdo=0x");
            storport_print_hex(parent_fdo as u64);
            storport_print("\n");
            
            if parent_fdo.is_null() {
                storport_print("[STORPORT] ERROR: PDO has no parent FDO\n");
                (*irp).io_status.status = STATUS_INVALID_DEVICE_REQUEST;
                IoCompleteRequest(irp, IO_NO_INCREMENT);
                return STATUS_INVALID_DEVICE_REQUEST;
            }
            
            let parent_ext_ptr = (*parent_fdo).device_extension;
            storport_print("[STORPORT]   parent ext ptr=0x");
            storport_print_hex(parent_ext_ptr as u64);
            storport_print("\n");
            
            if parent_ext_ptr.is_null() {
                storport_print("[STORPORT] ERROR: Parent FDO has no extension\n");
                (*irp).io_status.status = STATUS_INVALID_DEVICE_REQUEST;
                IoCompleteRequest(irp, IO_NO_INCREMENT);
                return STATUS_INVALID_DEVICE_REQUEST;
            }
            
            fdo_ext = parent_ext_ptr as *mut STORPORT_DEVICE_EXTENSION;
            
            let parent_sig = (*fdo_ext).signature;
            storport_print("[STORPORT]   parent signature=0x");
            storport_print_hex(parent_sig as u64);
            storport_print("\n");
            
            // Верифицируем что parent действительно FDO
            if parent_sig != STORPORT_DEVICE_EXTENSION_SIGNATURE {
                storport_print("[STORPORT] ERROR: Parent is not valid FDO (bad signature)\n");
                (*irp).io_status.status = STATUS_INVALID_DEVICE_REQUEST;
                IoCompleteRequest(irp, IO_NO_INCREMENT);
                return STATUS_INVALID_DEVICE_REQUEST;
            }
        } else {
            storport_print("[STORPORT] ERROR: Unknown device extension signature 0x");
            storport_print_hex(signature as u64);
            storport_print("\n");
            (*irp).io_status.status = STATUS_INVALID_DEVICE_REQUEST;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            return STATUS_INVALID_DEVICE_REQUEST;
        }
        
        storport_print("[STORPORT] Getting IRP stack location\n");
        
        let stack = IoGetCurrentIrpStackLocation(irp);
        
        storport_print("[STORPORT]   stack=0x");
        storport_print_hex(stack as u64);
        storport_print("\n");
        
        if stack.is_null() {
            storport_print("[STORPORT] ERROR: NULL stack location\n");
            (*irp).io_status.status = STATUS_INVALID_PARAMETER;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            return STATUS_INVALID_PARAMETER;
        }
        
        // Получаем SRB из IRP
        storport_print("[STORPORT] Reading SRB from parameters\n");
        let params_ptr = &(*stack).parameters as *const _ as *const u8;
        let srb = core::ptr::read(params_ptr as *const PSCSI_REQUEST_BLOCK);
        
        storport_print("[STORPORT]   SRB=0x");
        storport_print_hex(srb as u64);
        storport_print("\n");
        
        if srb.is_null() {
            storport_print("[STORPORT] ERROR: NULL SRB\n");
            (*irp).io_status.status = STATUS_INVALID_PARAMETER;
            IoCompleteRequest(irp, IO_NO_INCREMENT);
            return STATUS_INVALID_PARAMETER;
        }
        
        // Если запрос от PDO - устанавливаем path/target/lun из PDO extension
        if !pdo_ext.is_null() {
            storport_print("[STORPORT] Setting path/target/lun from PDO\n");
            (*srb).PathId = (*pdo_ext).path_id;
            (*srb).TargetId = (*pdo_ext).target_id;
            (*srb).Lun = (*pdo_ext).lun;
        }
        
        storport_print("[STORPORT] Calling handle_srb\n");
        
        // Обрабатываем SRB через miniport
        handle_srb(fdo_ext, srb, irp)
    }
}

/// Обработка SCSI Request Block
///
/// Фаза 2.2: Поддержка async completion через StorPortNotification.
/// - Устанавливает current_srb/current_irp перед вызовом HwStartIo
/// - При SrbStatus==PENDING возвращает STATUS_PENDING и ждёт StorPortNotification(RequestComplete)
/// - При синхронном завершении немедленно завершает IRP
unsafe fn handle_srb(
    ext: *mut STORPORT_DEVICE_EXTENSION,
    srb: PSCSI_REQUEST_BLOCK,
    irp: PIRP,
) -> NTSTATUS {
    unsafe {
        let srb_ref = &mut *srb;
        
        storport_print("[STORPORT] SRB function=0x");
        storport_print_hex(srb_ref.Function as u64);
        storport_print(", PathID=");
        storport_print_hex(srb_ref.PathId as u64);
        storport_print(", TargetID=");
        storport_print_hex(srb_ref.TargetId as u64);
        storport_print(", LUN=");
        storport_print_hex(srb_ref.Lun as u64);
        storport_print("\n");
        
        match srb_ref.Function {
            SRB_FUNCTION_EXECUTE_SCSI => {
                // Проверяем готовность принять запрос
                if !(*ext).can_accept_request() {
                    storport_print("[STORPORT] Adapter busy, cannot accept request\n");
                    srb_ref.SrbStatus = SRB_STATUS_BUSY;
                    (*irp).io_status.status = STATUS_DEVICE_BUSY;
                    IoCompleteRequest(irp, IO_NO_INCREMENT);
                    return STATUS_DEVICE_BUSY;
                }
                
                // Вызываем miniport HwStartIo
                if let Some(hw_start_io) = (*ext).hw_init_data.HwStartIo {
                    storport_print("[STORPORT] Calling miniport HwStartIo\n");
                    storport_print("[STORPORT]   hw_start_io=0x");
                    storport_print_hex(hw_start_io as usize as u64);
                    storport_print(" ext=0x");
                    storport_print_hex((*ext).miniport_device_extension as u64);
                    storport_print("\n");
                    
                    // PHASE 2.2: Устанавливаем текущий запрос ПЕРЕД вызовом HwStartIo
                    // Это позволяет StorPortNotification найти связанный IRP
                    (*ext).set_current_request(srb, irp);
                    
                    let result = hw_start_io((*ext).miniport_device_extension, srb);
                    
                    storport_print("[STORPORT]   HwStartIo returned ");
                    storport_print_hex(result as u64);
                    storport_print(", SrbStatus=0x");
                    storport_print_hex(srb_ref.SrbStatus as u64);
                    storport_print("\n");
                    
                    if result != 0 {
                        // Miniport принял request
                        let srb_status = srb_ref.SrbStatus & 0x3F; // Маска status-only bits
                        
                        if srb_status == SRB_STATUS_PENDING {
                            // PHASE 2.2: Асинхронная модель
                            // Miniport ещё обрабатывает запрос, IRP будет завершён
                            // через StorPortNotification(RequestComplete)
                            storport_print("[STORPORT] SRB pending - waiting for async completion\n");
                            
                            // Отмечаем IRP как pending
                            // NOTE: В реальной реализации нужен IoMarkIrpPending
                            // Для текущей boot-stage это работает синхронно через polling
                            
                            // Не завершаем IRP здесь - он будет завершён в StorPortNotification
                            // Возвращаем STATUS_PENDING
                            return STATUS_PENDING;
                        } else if srb_status == SRB_STATUS_SUCCESS {
                            // Синхронное завершение с успехом
                            storport_print("[STORPORT] SRB completed synchronously (success)\n");
                            
                            (*ext).clear_current_request();
                            
                            (*irp).io_status.status = STATUS_SUCCESS;
                            (*irp).io_status.information = srb_ref.DataTransferLength as usize;
                            IoCompleteRequest(irp, IO_NO_INCREMENT);
                            return STATUS_SUCCESS;
                        } else {
                            // Синхронное завершение с ошибкой
                            storport_print("[STORPORT] SRB completed with error: 0x");
                            storport_print_hex(srb_status as u64);
                            storport_print("\n");
                            
                            (*ext).clear_current_request();
                            
                            (*irp).io_status.status = srb_status_to_ntstatus(srb_status);
                            (*irp).io_status.information = 0;
                            IoCompleteRequest(irp, IO_NO_INCREMENT);
                            return (*irp).io_status.status;
                        }
                    } else {
                        // Miniport отверг запрос
                        storport_print("[STORPORT] Miniport rejected request\n");
                        
                        (*ext).clear_current_request();
                        
                        srb_ref.SrbStatus = SRB_STATUS_BUSY;
                        (*irp).io_status.status = STATUS_DEVICE_BUSY;
                        IoCompleteRequest(irp, IO_NO_INCREMENT);
                        return STATUS_DEVICE_BUSY;
                    }
                }
                
                storport_print("[STORPORT] ERROR: No HwStartIo callback\n");
                srb_ref.SrbStatus = SRB_STATUS_ERROR;
                (*irp).io_status.status = STATUS_NOT_IMPLEMENTED;
                IoCompleteRequest(irp, IO_NO_INCREMENT);
                STATUS_NOT_IMPLEMENTED
            }
            SRB_FUNCTION_RESET_BUS => {
                // Reset bus
                if let Some(hw_reset_bus) = (*ext).hw_init_data.HwResetBus {
                    hw_reset_bus((*ext).miniport_device_extension, srb_ref.PathId as ULONG);
                }
                
                srb_ref.SrbStatus = SRB_STATUS_SUCCESS;
                (*irp).io_status.status = STATUS_SUCCESS;
                IoCompleteRequest(irp, IO_NO_INCREMENT);
                STATUS_SUCCESS
            }
            _ => {
                storport_print("[STORPORT] Unsupported SRB function\n");
                srb_ref.SrbStatus = SRB_STATUS_INVALID_REQUEST;
                (*irp).io_status.status = STATUS_NOT_SUPPORTED;
                IoCompleteRequest(irp, IO_NO_INCREMENT);
                STATUS_NOT_SUPPORTED
            }
        }
    }
}

// =============================================================================
// Device Discovery via SCSI INQUIRY
// =============================================================================

/// Result of SCSI INQUIRY probe
#[derive(Debug, Clone, Copy)]
pub struct InquiryResult {
    /// Device is present and responded to INQUIRY
    pub present: bool,
    /// Device type from INQUIRY data (DIRECT_ACCESS_DEVICE, etc.)
    pub device_type: UCHAR,
    /// Peripheral qualifier (bits 5-7 of byte 0)
    pub peripheral_qualifier: UCHAR,
    /// Removable media bit
    pub removable: bool,
    /// Vendor ID (8 bytes, space padded ASCII)
    pub vendor_id: [UCHAR; 8],
    /// Product ID (16 bytes, space padded ASCII)
    pub product_id: [UCHAR; 16],
    /// Product revision (4 bytes)
    pub product_revision: [UCHAR; 4],
}

impl InquiryResult {
    pub const fn not_present() -> Self {
        Self {
            present: false,
            device_type: UNKNOWN_OR_NO_DEVICE,
            peripheral_qualifier: DEVICE_NOT_CAPABLE,
            removable: false,
            vendor_id: [0; 8],
            product_id: [0; 16],
            product_revision: [0; 4],
        }
    }
}

/// Probe a SCSI target using INQUIRY command
///
/// Sends a SCSI INQUIRY command to the specified target and returns
/// information about the device if present.
///
/// # Arguments
/// * `ext` - StorPort device extension (FDO)
/// * `path_id` - SCSI bus/path ID
/// * `target_id` - SCSI target ID (0-based)
/// * `lun` - Logical Unit Number (usually 0)
///
/// # Returns
/// InquiryResult containing device information or not_present if no device
pub unsafe fn probe_target_with_inquiry(
    ext: *mut STORPORT_DEVICE_EXTENSION,
    path_id: UCHAR,
    target_id: UCHAR,
    lun: UCHAR,
) -> InquiryResult {
    unsafe {
        storport_print("[STORPORT] INQUIRY probe: path=");
        storport_print_hex(path_id as u64);
        storport_print(" target=");
        storport_print_hex(target_id as u64);
        storport_print(" lun=");
        storport_print_hex(lun as u64);
        storport_print("\n");
        
        // Check miniport callback is available
        let hw_start_io = match (*ext).hw_init_data.HwStartIo {
            Some(f) => f,
            None => {
                storport_print("[STORPORT] ERROR: No HwStartIo callback\n");
                return InquiryResult::not_present();
            }
        };
        
        // Allocate INQUIRY data buffer on stack (36 bytes standard)
        let mut inquiry_data: MaybeUninit<INQUIRYDATA> = MaybeUninit::uninit();
        let inquiry_buffer = inquiry_data.as_mut_ptr() as PVOID;
        
        // Zero the buffer
        ptr::write_bytes(inquiry_buffer as *mut u8, 0, INQUIRYDATABUFFERSIZE);
        
        // Build SCSI_REQUEST_BLOCK for INQUIRY command
        let mut srb: SCSI_REQUEST_BLOCK = core::mem::zeroed();
        
        srb.Length = core::mem::size_of::<SCSI_REQUEST_BLOCK>() as u16;
        srb.Function = SRB_FUNCTION_EXECUTE_SCSI;
        srb.SrbStatus = SRB_STATUS_PENDING;
        srb.ScsiStatus = 0;
        srb.PathId = path_id;
        srb.TargetId = target_id;
        srb.Lun = lun;
        srb.QueueTag = 0;
        srb.QueueAction = 0;
        srb.CdbLength = 6;  // INQUIRY is a 6-byte CDB
        srb.SenseInfoBufferLength = 0;
        srb.SrbFlags = SRB_FLAGS_DATA_IN | SRB_FLAGS_DISABLE_SYNCH_TRANSFER;
        srb.DataTransferLength = INQUIRYDATABUFFERSIZE as ULONG;
        srb.TimeOutValue = 2;  // 2 seconds timeout
        srb.DataBuffer = inquiry_buffer;
        srb.SenseInfoBuffer = ptr::null_mut();
        srb.NextSrb = ptr::null_mut();
        srb.OriginalRequest = ptr::null_mut();
        srb.SrbExtension = ptr::null_mut();
        srb.InternalStatus = 0;
        srb.Reserved = 0;
        
        // Build INQUIRY CDB (Command Descriptor Block)
        // CDB format for INQUIRY:
        // Byte 0: Operation code (0x12 = INQUIRY)
        // Byte 1: Reserved/flags (bit 0 = EVPD, bit 1 = CMDDT)
        // Byte 2: Page code (for EVPD)
        // Byte 3: Allocation length MSB
        // Byte 4: Allocation length LSB
        // Byte 5: Control
        srb.Cdb[0] = SCSIOP_INQUIRY;
        srb.Cdb[1] = 0;  // Standard INQUIRY (no VPD)
        srb.Cdb[2] = 0;  // Page code (not used for standard INQUIRY)
        srb.Cdb[3] = 0;  // Allocation length MSB
        srb.Cdb[4] = INQUIRYDATABUFFERSIZE as UCHAR;  // Allocation length LSB
        srb.Cdb[5] = 0;  // Control
        
        storport_print("[STORPORT]   Sending INQUIRY command...\n");
        storport_print("[STORPORT]   CDB: ");
        for i in 0..6 {
            storport_print_hex(srb.Cdb[i] as u64);
            storport_print(" ");
        }
        storport_print("\n");
        
        // Call miniport's HwStartIo to execute the command
        let result = hw_start_io((*ext).miniport_device_extension, &mut srb as *mut _);
        
        storport_print("[STORPORT]   HwStartIo returned ");
        storport_print_hex(result as u64);
        storport_print(", SrbStatus=");
        storport_print_hex(srb.SrbStatus as u64);
        storport_print("\n");
        
        // Check if command completed successfully
        if result == 0 {
            storport_print("[STORPORT]   Miniport rejected INQUIRY\n");
            return InquiryResult::not_present();
        }
        
        // Check SRB status
        match srb.SrbStatus & 0x3F {  // Mask off status-only bits
            SRB_STATUS_SUCCESS => {
                // Parse INQUIRY response
                let inquiry = inquiry_data.assume_init();
                
                let device_type = inquiry.DeviceType & 0x1F;  // Bits 0-4
                let peripheral_qualifier = (inquiry.DeviceType >> 5) & 0x07;  // Bits 5-7
                let removable = (inquiry.DeviceTypeModifier & 0x80) != 0;
                
                storport_print("[STORPORT]   INQUIRY success!\n");
                storport_print("[STORPORT]   Device type: 0x");
                storport_print_hex(device_type as u64);
                storport_print(", qualifier: 0x");
                storport_print_hex(peripheral_qualifier as u64);
                storport_print("\n");
                
                // Print vendor ID and product ID together
                // Build buffer: "[STORPORT]   Vendor: XXXXXXXX  Product: XXXXXXXXXXXXXXXX\n"
                let mut info_buf = [0u8; 80];
                let mut pos = 0;
                
                // Copy prefix
                for &b in b"[STORPORT]   Vendor: ".iter() {
                    info_buf[pos] = b;
                    pos += 1;
                }
                
                // Copy vendor ID (8 chars)
                for i in 0..8 {
                    let c = inquiry.VendorId[i];
                    if c >= 0x20 && c <= 0x7E {
                        info_buf[pos] = c;
                        pos += 1;
                    }
                }
                
                // Separator
                for &b in b"  Product: ".iter() {
                    info_buf[pos] = b;
                    pos += 1;
                }
                
                // Copy product ID (16 chars)
                for i in 0..16 {
                    let c = inquiry.ProductId[i];
                    if c >= 0x20 && c <= 0x7E {
                        info_buf[pos] = c;
                        pos += 1;
                    }
                }
                
                // Newline and null terminator
                info_buf[pos] = b'\n';
                pos += 1;
                info_buf[pos] = 0;
                
                // Output in one call
                DbgPrint(info_buf.as_ptr());
                
                // Check if device is actually present
                // Peripheral qualifier:
                // 0 = Device is connected to this logical unit
                // 1 = Device is not connected but is capable of supporting the specified type
                // 3 = Target does not support this device type
                // 0x7F (device_type = 0x1F, qualifier = 3) = No device
                
                if peripheral_qualifier == DEVICE_NOT_CAPABLE || device_type == LOGICAL_UNIT_NOT_PRESENT {
                    storport_print("[STORPORT]   Device not present (qualifier/type indicates no device)\n");
                    return InquiryResult::not_present();
                }
                
                InquiryResult {
                    present: true,
                    device_type,
                    peripheral_qualifier,
                    removable,
                    vendor_id: inquiry.VendorId,
                    product_id: inquiry.ProductId,
                    product_revision: inquiry.ProductRevisionLevel,
                }
            }
            SRB_STATUS_NO_DEVICE | SRB_STATUS_SELECTION_TIMEOUT => {
                storport_print("[STORPORT]   No device at this target\n");
                InquiryResult::not_present()
            }
            SRB_STATUS_TIMEOUT | SRB_STATUS_COMMAND_TIMEOUT => {
                storport_print("[STORPORT]   INQUIRY timed out\n");
                InquiryResult::not_present()
            }
            SRB_STATUS_BUSY => {
                storport_print("[STORPORT]   Device busy\n");
                InquiryResult::not_present()
            }
            _ => {
                storport_print("[STORPORT]   INQUIRY failed with status 0x");
                storport_print_hex(srb.SrbStatus as u64);
                storport_print("\n");
                InquiryResult::not_present()
            }
        }
    }
}

