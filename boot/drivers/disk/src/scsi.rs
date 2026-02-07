//! SCSI operations for disk.sys
//!
//! Конвертирует IRP_MJ_READ/WRITE в SCSI Request Blocks (SRB)
//! и отправляет их в нижний драйвер (StorPort).

use crate::types::*;
use crate::{disk_print, disk_print_hex, disk_print_dec};
use core::ptr;

// =============================================================================
// SCSI Commands
// =============================================================================

/// SCSI READ CAPACITY(10) command
const SCSIOP_READ_CAPACITY: u8 = 0x25;
/// SCSI READ CAPACITY(16) command  
const SCSIOP_READ_CAPACITY16: u8 = 0x9E;
/// SCSI READ(10) command - 10-byte CDB
const SCSIOP_READ: u8 = 0x28;
/// SCSI WRITE(10) command - 10-byte CDB
const SCSIOP_WRITE: u8 = 0x2A;
/// SCSI READ(16) command - 16-byte CDB для больших LBA
const SCSIOP_READ16: u8 = 0x88;
/// SCSI WRITE(16) command - 16-byte CDB
const SCSIOP_WRITE16: u8 = 0x8A;

// =============================================================================
// SRB Constants (из Windows DDK)
// =============================================================================

/// Execute SCSI command
const SRB_FUNCTION_EXECUTE_SCSI: u8 = 0x00;

/// SRB Status codes
const SRB_STATUS_PENDING: u8 = 0x00;
const SRB_STATUS_SUCCESS: u8 = 0x01;

/// SRB Flags
const SRB_FLAGS_DATA_IN: u32 = 0x00000040;
const SRB_FLAGS_DATA_OUT: u32 = 0x00000080;
const SRB_FLAGS_DISABLE_SYNCH_TRANSFER: u32 = 0x00000008;

// =============================================================================
// =============================================================================
// SCSI_REQUEST_BLOCK
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
    pub Function: u8,                       // offset 0x02
    pub SrbStatus: u8,                      // offset 0x03
    pub ScsiStatus: u8,                     // offset 0x04
    pub PathId: u8,                         // offset 0x05
    pub TargetId: u8,                       // offset 0x06
    pub Lun: u8,                            // offset 0x07
    pub QueueTag: u8,                       // offset 0x08
    pub QueueAction: u8,                    // offset 0x09
    pub CdbLength: u8,                      // offset 0x0A
    pub SenseInfoBufferLength: u8,          // offset 0x0B
    pub SrbFlags: u32,                      // offset 0x0C
    pub DataTransferLength: u32,            // offset 0x10
    pub TimeOutValue: u32,                  // offset 0x14
    pub DataBuffer: PVOID,                  // offset 0x18
    pub SenseInfoBuffer: PVOID,             // offset 0x20 (x64)
    pub NextSrb: *mut SCSI_REQUEST_BLOCK,   // offset 0x28 (x64)
    pub OriginalRequest: PVOID,             // offset 0x30 (x64)
    pub SrbExtension: PVOID,                // offset 0x38 (x64)
    pub InternalStatus: u32,                // offset 0x40 (x64) - union with QueueSortKey, LinkTimeoutValue
    pub Reserved: u32,                      // offset 0x44 (x64) - WIN64 only, for PVOID alignment
    pub Cdb: [u8; 16],                      // offset 0x48 (x64)
}

pub type PSCSI_REQUEST_BLOCK = *mut SCSI_REQUEST_BLOCK;

// =============================================================================
// Public API
// =============================================================================

/// Выполняет SCSI READ операцию через SRB
///
/// # Arguments
/// * `lower_device` - нижнее устройство (StorPort FDO)
/// * `byte_offset` - смещение в байтах от начала диска
/// * `length` - количество байт для чтения
/// * `buffer` - буфер для данных
/// * `sector_size` - размер сектора (обычно 512)
///
/// # Returns
/// NTSTATUS и количество прочитанных байт
pub unsafe fn scsi_read(
    lower_device: PDEVICE_OBJECT,
    byte_offset: u64,
    length: u32,
    buffer: PVOID,
    sector_size: u32,
) -> (NTSTATUS, u32) {
    scsi_read_write_internal(lower_device, byte_offset, length, buffer, sector_size, true)
}

/// Выполняет SCSI WRITE операцию через SRB
pub unsafe fn scsi_write(
    lower_device: PDEVICE_OBJECT,
    byte_offset: u64,
    length: u32,
    buffer: PVOID,
    sector_size: u32,
) -> (NTSTATUS, u32) {
    scsi_read_write_internal(lower_device, byte_offset, length, buffer, sector_size, false)
}

/// Общая функция для выполнения READ/WRITE через SRB
pub unsafe fn disk_scsi_execute(
    lower_device: PDEVICE_OBJECT,
    byte_offset: u64,
    length: u32,
    buffer: PVOID,
    sector_size: u32,
    is_read: bool,
) -> (NTSTATUS, u32) {
    scsi_read_write_internal(lower_device, byte_offset, length, buffer, sector_size, is_read)
}

/// Получает размер диска через SCSI READ CAPACITY команду
///
/// # Returns
/// (sector_count, sector_size) или (0, 0) при ошибке
pub unsafe fn scsi_read_capacity(lower_device: PDEVICE_OBJECT) -> (u64, u32) {
    if lower_device.is_null() {
        return (0, 0);
    }

    // Буфер для READ CAPACITY(10) response (8 bytes)
    let mut capacity_data = [0u8; 32]; // 32 bytes для поддержки READ CAPACITY(16)
    
    // Сначала пробуем READ CAPACITY(10)
    let srb_size = core::mem::size_of::<SCSI_REQUEST_BLOCK>();
    let srb = ExAllocatePoolWithTag(NON_PAGED_POOL, srb_size, DISK_POOL_TAG) as PSCSI_REQUEST_BLOCK;
    if srb.is_null() {
        return (0, 0);
    }
    
    ptr::write_bytes(srb, 0, 1);

    // Заполняем SRB для READ CAPACITY(10)
    (*srb).Length = srb_size as u16;
    (*srb).Function = SRB_FUNCTION_EXECUTE_SCSI;
    (*srb).SrbStatus = SRB_STATUS_PENDING;
    (*srb).PathId = 0;
    (*srb).TargetId = 0;
    (*srb).Lun = 0;
    (*srb).CdbLength = 10;
    (*srb).DataTransferLength = 8; // READ CAPACITY(10) возвращает 8 bytes
    (*srb).TimeOutValue = 5;
    (*srb).DataBuffer = capacity_data.as_mut_ptr() as PVOID;
    (*srb).SrbFlags = SRB_FLAGS_DATA_IN | SRB_FLAGS_DISABLE_SYNCH_TRANSFER;

    // CDB для READ CAPACITY(10)
    (*srb).Cdb[0] = SCSIOP_READ_CAPACITY;
    // Остальные байты уже 0

    // Создаём IRP
    let stack_size = (*lower_device).stack_size;
    let irp = IoAllocateIrp(stack_size, 0);
    if irp.is_null() {
        ExFreePoolWithTag(srb as PVOID, DISK_POOL_TAG);
        return (0, 0);
    }

    (*irp).io_status.status = STATUS_NOT_SUPPORTED;
    (*irp).io_status.information = 0;
    (*irp).user_buffer = capacity_data.as_mut_ptr() as PVOID;
    (*irp).flags = IRP_SYNCHRONOUS_API;

    let stack = IoGetNextIrpStackLocation(irp);
    if stack.is_null() {
        IoFreeIrp(irp);
        ExFreePoolWithTag(srb as PVOID, DISK_POOL_TAG);
        return (0, 0);
    }

    (*stack).major_function = IRP_MJ_SCSI;
    (*stack).minor_function = 0;
    (*stack).device_object = lower_device;
    (*stack).file_object = ptr::null_mut();
    
    let params_ptr = &mut (*stack).parameters as *mut _ as *mut u8;
    ptr::write_bytes(params_ptr, 0, 32);
    ptr::write(params_ptr as *mut PSCSI_REQUEST_BLOCK, srb);
    
    (*srb).OriginalRequest = irp as PVOID;

    // Вызываем нижний драйвер
    let status = IoCallDriver(lower_device, irp);
    
    let mut sector_count: u64 = 0;
    let mut sector_size: u32 = 0;
    
    if status >= 0 && (*irp).io_status.status >= 0 {
        // Парсим ответ READ CAPACITY(10)
        // Bytes 0-3: Last LBA (big-endian)
        // Bytes 4-7: Block size (big-endian)
        let last_lba = ((capacity_data[0] as u32) << 24)
            | ((capacity_data[1] as u32) << 16)
            | ((capacity_data[2] as u32) << 8)
            | (capacity_data[3] as u32);
        
        sector_size = ((capacity_data[4] as u32) << 24)
            | ((capacity_data[5] as u32) << 16)
            | ((capacity_data[6] as u32) << 8)
            | (capacity_data[7] as u32);
        
        // Если last_lba == 0xFFFFFFFF, нужен READ CAPACITY(16)
        if last_lba == 0xFFFFFFFF {
            disk_print("[DISK/SCSI] Large disk detected, using READ CAPACITY(16)\n");
            
            // Переиспользуем SRB для READ CAPACITY(16)
            ptr::write_bytes(srb, 0, 1);
            (*srb).Length = srb_size as u16;
            (*srb).Function = SRB_FUNCTION_EXECUTE_SCSI;
            (*srb).SrbStatus = SRB_STATUS_PENDING;
            (*srb).PathId = 0;
            (*srb).TargetId = 0;
            (*srb).Lun = 0;
            (*srb).CdbLength = 16;
            (*srb).DataTransferLength = 32;
            (*srb).TimeOutValue = 5;
            (*srb).DataBuffer = capacity_data.as_mut_ptr() as PVOID;
            (*srb).SrbFlags = SRB_FLAGS_DATA_IN | SRB_FLAGS_DISABLE_SYNCH_TRANSFER;
            
            // CDB для READ CAPACITY(16)
            (*srb).Cdb[0] = SCSIOP_READ_CAPACITY16;
            (*srb).Cdb[1] = 0x10; // Service Action = 0x10
            // Bytes 10-13: Allocation length (32 bytes)
            (*srb).Cdb[13] = 32;
            
            // Переиспользуем IRP
            (*irp).io_status.status = STATUS_NOT_SUPPORTED;
            (*irp).io_status.information = 0;
            
            let stack2 = IoGetNextIrpStackLocation(irp);
            if !stack2.is_null() {
                (*stack2).major_function = IRP_MJ_SCSI;
                (*stack2).minor_function = 0;
                (*stack2).device_object = lower_device;
                (*stack2).file_object = ptr::null_mut();
                
                let params_ptr2 = &mut (*stack2).parameters as *mut _ as *mut u8;
                ptr::write_bytes(params_ptr2, 0, 32);
                ptr::write(params_ptr2 as *mut PSCSI_REQUEST_BLOCK, srb);
                
                let status2 = IoCallDriver(lower_device, irp);
                
                if status2 >= 0 && (*irp).io_status.status >= 0 {
                    // Парсим READ CAPACITY(16) response
                    // Bytes 0-7: Last LBA (big-endian 64-bit)
                    sector_count = ((capacity_data[0] as u64) << 56)
                        | ((capacity_data[1] as u64) << 48)
                        | ((capacity_data[2] as u64) << 40)
                        | ((capacity_data[3] as u64) << 32)
                        | ((capacity_data[4] as u64) << 24)
                        | ((capacity_data[5] as u64) << 16)
                        | ((capacity_data[6] as u64) << 8)
                        | (capacity_data[7] as u64);
                    
                    // Bytes 8-11: Block size (big-endian)
                    sector_size = ((capacity_data[8] as u32) << 24)
                        | ((capacity_data[9] as u32) << 16)
                        | ((capacity_data[10] as u32) << 8)
                        | (capacity_data[11] as u32);
                    
                    sector_count += 1; // Last LBA -> total sectors
                }
            }
        } else {
            sector_count = (last_lba as u64) + 1;
        }
    }

    IoFreeIrp(irp);
    ExFreePoolWithTag(srb as PVOID, DISK_POOL_TAG);

    disk_print("[DISK/SCSI] READ CAPACITY: sector_count=");
    disk_print_dec(sector_count);
    disk_print(" sector_size=");
    disk_print_dec(sector_size as u64);
    disk_print("\n");

    (sector_count, sector_size)
}

// =============================================================================
// Internal Implementation
// =============================================================================

unsafe fn scsi_read_write_internal(
    lower_device: PDEVICE_OBJECT,
    byte_offset: u64,
    length: u32,
    buffer: PVOID,
    sector_size: u32,
    is_read: bool,
) -> (NTSTATUS, u32) {
    if lower_device.is_null() || buffer.is_null() || length == 0 || sector_size == 0 {
        return (STATUS_INVALID_PARAMETER, 0);
    }

    // Вычисляем LBA и количество секторов
    let start_lba = byte_offset / sector_size as u64;
    let sector_count = (length + sector_size - 1) / sector_size;

    // Выделяем SRB
    let srb_size = core::mem::size_of::<SCSI_REQUEST_BLOCK>();
    let srb = ExAllocatePoolWithTag(NON_PAGED_POOL, srb_size, DISK_POOL_TAG) as PSCSI_REQUEST_BLOCK;
    if srb.is_null() {
        return (STATUS_INSUFFICIENT_RESOURCES, 0);
    }
    
    ptr::write_bytes(srb, 0, 1);

    // Заполняем SRB
    (*srb).Length = srb_size as u16;
    (*srb).Function = SRB_FUNCTION_EXECUTE_SCSI;
    (*srb).SrbStatus = SRB_STATUS_PENDING;
    (*srb).PathId = 0;
    (*srb).TargetId = 0;
    (*srb).Lun = 0;
    (*srb).DataTransferLength = length;
    (*srb).TimeOutValue = 10; // 10 секунд
    (*srb).DataBuffer = buffer;
    (*srb).SrbFlags = if is_read { SRB_FLAGS_DATA_IN } else { SRB_FLAGS_DATA_OUT };
    (*srb).SrbFlags |= SRB_FLAGS_DISABLE_SYNCH_TRANSFER;

    // Формируем CDB (Command Descriptor Block)
    if start_lba <= 0xFFFFFFFF && sector_count <= 0xFFFF {
        // Используем READ/WRITE(10) для небольших LBA
        (*srb).CdbLength = 10;
        
        if is_read {
            (*srb).Cdb[0] = SCSIOP_READ;
        } else {
            (*srb).Cdb[0] = SCSIOP_WRITE;
        }
        
        // LBA (big-endian, bytes 2-5)
        (*srb).Cdb[2] = ((start_lba >> 24) & 0xFF) as u8;
        (*srb).Cdb[3] = ((start_lba >> 16) & 0xFF) as u8;
        (*srb).Cdb[4] = ((start_lba >> 8) & 0xFF) as u8;
        (*srb).Cdb[5] = (start_lba & 0xFF) as u8;
        
        // Transfer length (big-endian, bytes 7-8)
        (*srb).Cdb[7] = ((sector_count >> 8) & 0xFF) as u8;
        (*srb).Cdb[8] = (sector_count & 0xFF) as u8;
    } else {
        // Используем READ/WRITE(16) для больших LBA
        (*srb).CdbLength = 16;
        
        if is_read {
            (*srb).Cdb[0] = SCSIOP_READ16;
        } else {
            (*srb).Cdb[0] = SCSIOP_WRITE16;
        }
        
        // LBA (big-endian, bytes 2-9)
        (*srb).Cdb[2] = ((start_lba >> 56) & 0xFF) as u8;
        (*srb).Cdb[3] = ((start_lba >> 48) & 0xFF) as u8;
        (*srb).Cdb[4] = ((start_lba >> 40) & 0xFF) as u8;
        (*srb).Cdb[5] = ((start_lba >> 32) & 0xFF) as u8;
        (*srb).Cdb[6] = ((start_lba >> 24) & 0xFF) as u8;
        (*srb).Cdb[7] = ((start_lba >> 16) & 0xFF) as u8;
        (*srb).Cdb[8] = ((start_lba >> 8) & 0xFF) as u8;
        (*srb).Cdb[9] = (start_lba & 0xFF) as u8;
        
        // Transfer length (big-endian, bytes 10-13)
        (*srb).Cdb[10] = ((sector_count >> 24) & 0xFF) as u8;
        (*srb).Cdb[11] = ((sector_count >> 16) & 0xFF) as u8;
        (*srb).Cdb[12] = ((sector_count >> 8) & 0xFF) as u8;
        (*srb).Cdb[13] = (sector_count & 0xFF) as u8;
    }

    // Создаём IRP для SCSI запроса
    let stack_size = (*lower_device).stack_size;
    let irp = IoAllocateIrp(stack_size, 0);
    if irp.is_null() {
        ExFreePoolWithTag(srb as PVOID, DISK_POOL_TAG);
        return (STATUS_INSUFFICIENT_RESOURCES, 0);
    }

    // Инициализируем IRP
    (*irp).io_status.status = STATUS_NOT_SUPPORTED;
    (*irp).io_status.information = 0;
    (*irp).user_buffer = buffer;
    (*irp).flags = IRP_SYNCHRONOUS_API;

    // Настраиваем stack location для IRP_MJ_SCSI
    let stack = IoGetNextIrpStackLocation(irp);
    if stack.is_null() {
        IoFreeIrp(irp);
        ExFreePoolWithTag(srb as PVOID, DISK_POOL_TAG);
        return (STATUS_INSUFFICIENT_RESOURCES, 0);
    }

    (*stack).major_function = IRP_MJ_SCSI;
    (*stack).minor_function = 0;
    (*stack).device_object = lower_device;
    (*stack).file_object = ptr::null_mut();
    
    // Parameters.Scsi.Srb - SRB указатель в первом поле параметров
    // Сначала очистим parameters
    let params_ptr = &mut (*stack).parameters as *mut _ as *mut u8;
    ptr::write_bytes(params_ptr, 0, 32);
    
    // Записываем SRB как первый указатель (offset 0)
    ptr::write(params_ptr as *mut PSCSI_REQUEST_BLOCK, srb);
    
    // DEBUG: выведем что записали
    crate::disk_print("[DISK/SCSI] stack=0x");
    crate::disk_print_hex(stack as u64);
    crate::disk_print(" SRB=0x");
    crate::disk_print_hex(srb as u64);
    crate::disk_print("\n");

    // Сохраняем IRP в SRB для completion
    (*srb).OriginalRequest = irp as PVOID;

    // Вызываем нижний драйвер
    let status = IoCallDriver(lower_device, irp);
    
    // Получаем результат
    let result_status = if status >= 0 {
        (*irp).io_status.status
    } else {
        status
    };
    
    let bytes_transferred = if result_status >= 0 {
        (*irp).io_status.information as u32
    } else {
        0
    };

    // Освобождаем ресурсы
    IoFreeIrp(irp);
    ExFreePoolWithTag(srb as PVOID, DISK_POOL_TAG);

    (result_status, bytes_transferred)
}

// =============================================================================
// Imports
// =============================================================================

const IRP_SYNCHRONOUS_API: u32 = 0x00000004;
const IRP_MJ_SCSI: u8 = 0x0F; // IRP_MJ_INTERNAL_DEVICE_CONTROL используется для SCSI

unsafe extern "C" {
    fn IoAllocateIrp(stack_size: i8, charge_quota: u8) -> PIRP;
    fn IoFreeIrp(irp: PIRP);
    fn IoGetNextIrpStackLocation(irp: PIRP) -> PIO_STACK_LOCATION;
}

