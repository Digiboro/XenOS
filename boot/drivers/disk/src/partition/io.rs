//! I/O операции для Partition Manager
//!
//! Синхронное чтение секторов с нижнего устройства через SCSI SRB.

use crate::types::*;
use crate::scsi;

/// Синхронно читает данные с нижнего устройства через SCSI
///
/// Конвертирует запрос в SCSI READ SRB и отправляет в StorPort.
///
/// # Safety
/// - `lower_device` должен быть валидным указателем на DEVICE_OBJECT (StorPort FDO)
/// - `buffer` должен быть валидным указателем с достаточным размером для `length` байт
pub unsafe fn read_from_device(
    lower_device: PDEVICE_OBJECT,
    byte_offset: u64,
    length: u32,
    buffer: PVOID,
) -> NTSTATUS {
    // Используем стандартный размер сектора 512 байт
    // В реальной реализации нужно получать из FDO extension
    const SECTOR_SIZE: u32 = 512;
    
    let (status, _bytes_read) = scsi::scsi_read(
        lower_device,
        byte_offset,
        length,
        buffer,
        SECTOR_SIZE,
    );
    
    status
}
