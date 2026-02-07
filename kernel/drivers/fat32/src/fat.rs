//! FAT Chain Reading
//!
//! Модуль для чтения FAT table и следования cluster chains.

use crate::types::*;
use crate::bpb::*;
use crate::vcb::FAT32_VCB;
use crate::fcb::FAT32_FCB;

/// Читает следующий cluster в цепочке из FAT
///
/// # Arguments
/// * `vcb` - Volume Control Block
/// * `cluster` - текущий cluster number
///
/// # Returns
/// * Следующий cluster number или EOC/BAD marker
pub unsafe fn fat_read_next_cluster(
    vcb: *mut FAT32_VCB,
    cluster: u32,
) -> Result<u32, NTSTATUS> {
    unsafe {
        if cluster < 2 || cluster >= 0x0FFFFFF7 {
            return Err(STATUS_INVALID_PARAMETER);
        }
        
        let vcb_ref = &*vcb;
        
        // Вычисляем offset в FAT
        // FAT32: 4 bytes per entry
        let fat_offset = cluster * 4;
        
        // Вычисляем sector и offset within sector
        let fat_start_lba = vcb_ref.fat_start_sector;
        let sector_size = vcb_ref.bytes_per_sector;
        
        let fat_sector = fat_start_lba + (fat_offset / sector_size);
        let offset_in_sector = fat_offset % sector_size;
        
        // Читаем сектор из FAT
        let mut sector_buffer = [0u8; 512]; // Предполагаем 512 bytes per sector
        
        let read_status = read_disk_sector(
            vcb_ref.target_device,
            fat_sector as u64,
            &mut sector_buffer,
        );
        
        if read_status < 0 {
            return Err(read_status);
        }
        
        // Читаем FAT entry (4 bytes, little-endian)
        let entry_ptr = sector_buffer.as_ptr().add(offset_in_sector as usize) as *const u32;
        let fat_entry = core::ptr::read_unaligned(entry_ptr);
        
        // Маскируем верхние 4 бита (только 28 бит используются в FAT32)
        let next_cluster = fat_entry & 0x0FFFFFFF;
        
        Ok(next_cluster)
    }
}

/// Проверяет является ли FAT entry End-Of-Chain
#[inline]
pub fn is_eoc(entry: u32) -> bool {
    let masked = entry & 0x0FFFFFFF;
    masked >= FAT32_EOC_MIN
}

/// Проверяет является ли cluster валидным
#[inline]
pub fn is_valid_cluster(entry: u32) -> bool {
    let masked = entry & 0x0FFFFFFF;
    masked >= 2 && masked < FAT32_BAD
}

/// Читает цепочку кластеров в buffer
///
/// # Arguments
/// * `vcb` - Volume Control Block
/// * `first_cluster` - начальный cluster
/// * `file_offset` - offset в файле (в байтах)
/// * `buffer` - буфер для чтения данных
/// * `length` - количество байт для чтения
///
/// # Returns
/// * Количество прочитанных байт
pub unsafe fn fat_read_cluster_chain(
    vcb: *mut FAT32_VCB,
    first_cluster: u32,
    file_offset: u64,
    buffer: *mut u8,
    length: u32,
) -> Result<u32, NTSTATUS> {
    unsafe {
        let vcb_ref = &*vcb;
        let cluster_size = vcb_ref.bytes_per_cluster;
        
        if first_cluster < 2 {
            return Ok(0); // Пустой файл
        }
        
        // Вычисляем начальный cluster для чтения
        let start_cluster_index = (file_offset / cluster_size as u64) as u32;
        let offset_in_first_cluster = (file_offset % cluster_size as u64) as u32;
        
        // Следуем по chain до нужного cluster
        let mut current_cluster = first_cluster;
        for _ in 0..start_cluster_index {
            let next = fat_read_next_cluster(vcb, current_cluster)?;
            if is_eoc(next) {
                // Достигли конца файла раньше чем ожидалось
                return Ok(0);
            }
            current_cluster = next;
        }
        
        // Теперь читаем данные из clusters
        let mut bytes_read = 0u32;
        let mut remaining = length;
        let mut buf_offset = 0;
        let mut cluster_offset = offset_in_first_cluster;
        
        loop {
            if remaining == 0 {
                break;
            }
            
            // Читаем из текущего cluster
            let bytes_to_read = remaining.min(cluster_size - cluster_offset);
            
            let read = read_cluster_partial(
                vcb,
                current_cluster,
                cluster_offset,
                buffer.add(buf_offset as usize),
                bytes_to_read,
            )?;
            
            bytes_read += read;
            remaining -= read;
            buf_offset += read;
            
            if read < bytes_to_read {
                // Достигли конца данных
                break;
            }
            
            // Переходим к следующему cluster
            cluster_offset = 0; // Следующий cluster читаем с начала
            
            let next = fat_read_next_cluster(vcb, current_cluster)?;
            if is_eoc(next) || !is_valid_cluster(next) {
                break; // Конец цепочки
            }
            
            current_cluster = next;
        }
        
        Ok(bytes_read)
    }
}

/// Читает часть cluster в buffer
unsafe fn read_cluster_partial(
    vcb: *mut FAT32_VCB,
    cluster: u32,
    offset_in_cluster: u32,
    buffer: *mut u8,
    length: u32,
) -> Result<u32, NTSTATUS> {
    unsafe {
        let vcb_ref = &*vcb;
        
        // Конвертируем cluster в LBA
        let lba = vcb_ref.bpb.cluster_to_lba(cluster);
        let bytes_per_sector = vcb_ref.bytes_per_sector;
        
        // Вычисляем offset в байтах от начала partition
        let cluster_byte_offset = lba * bytes_per_sector as u64;
        let absolute_offset = cluster_byte_offset + offset_in_cluster as u64;
        
        // Читаем данные через IRP к storage device
        let status = read_disk_bytes(
            vcb_ref.target_device,
            absolute_offset,
            buffer,
            length,
        );
        
        if status < 0 {
            Err(status)
        } else {
            Ok(length) // Возвращаем запрошенную длину при успехе
        }
    }
}

/// Читает сектор с диска
unsafe fn read_disk_sector(
    device: PDEVICE_OBJECT,
    lba: u64,
    buffer: &mut [u8; 512],
) -> NTSTATUS {
    unsafe {
        let byte_offset = lba * 512;
        read_disk_bytes(device, byte_offset, buffer.as_mut_ptr(), 512)
    }
}

/// Выделяет свободный cluster из FAT
///
/// # Returns
/// * Ok(cluster_number) если найден свободный кластер
/// * Err(STATUS_DISK_FULL) если нет свободных кластеров
pub unsafe fn fat_allocate_cluster(
    vcb: *mut FAT32_VCB,
) -> Result<u32, NTSTATUS> {
    unsafe {
        let vcb_ref = &*vcb;
        
        // Начинаем поиск с кластера 2 (первые 2 reserved)
        let start_cluster = 2u32;
        let max_cluster = 0x0FFFFFF0u32; // Максимальный валидный cluster
        
        // Простая линейная проверка
        // В полной реализации можно использовать FSInfo sector для hint
        for cluster in start_cluster..max_cluster.min(100000) {
            // Читаем FAT entry
            let fat_entry = fat_read_fat_entry(vcb, cluster)?;
            
            if fat_entry == FAT32_FREE {
                // Нашли свободный! Помечаем как EOC
                fat_write_fat_entry(vcb, cluster, FAT32_EOC_MIN)?;
                return Ok(cluster);
            }
        }
        
        Err(STATUS_DISK_FULL)
    }
}

/// Расширяет cluster chain добавлением нового кластера
pub unsafe fn fat_extend_chain(
    vcb: *mut FAT32_VCB,
    last_cluster: u32,
) -> Result<u32, NTSTATUS> {
    unsafe {
        // Выделяем новый cluster
        let new_cluster = fat_allocate_cluster(vcb)?;
        
        // Связываем с предыдущим
        fat_write_fat_entry(vcb, last_cluster, new_cluster)?;
        
        // Помечаем новый как EOC
        fat_write_fat_entry(vcb, new_cluster, FAT32_EOC_MIN)?;
        
        Ok(new_cluster)
    }
}

/// Освобождает cluster chain
pub unsafe fn fat_free_chain(
    vcb: *mut FAT32_VCB,
    first_cluster: u32,
) -> Result<(), NTSTATUS> {
    unsafe {
        let mut current = first_cluster;
        let mut iterations = 0u32;
        
        loop {
            iterations += 1;
            if iterations > 10000 {
                return Err(STATUS_FILE_CORRUPT_ERROR); // Защита от циклов
            }
            
            if current < 2 || !is_valid_cluster(current) {
                break;
            }
            
            // Читаем следующий cluster перед освобождением
            let next = fat_read_fat_entry(vcb, current)?;
            
            // Помечаем текущий как свободный
            fat_write_fat_entry(vcb, current, FAT32_FREE)?;
            
            // Переходим к следующему
            if is_eoc(next) || !is_valid_cluster(next) {
                break;
            }
            
            current = next;
        }
        
        Ok(())
    }
}

/// Читает FAT entry для кластера
unsafe fn fat_read_fat_entry(
    vcb: *mut FAT32_VCB,
    cluster: u32,
) -> Result<u32, NTSTATUS> {
    // Аналогично fat_read_next_cluster, но возвращает raw value
    fat_read_next_cluster(vcb, cluster)
}

/// Записывает FAT entry (публичная для использования в других модулях)
pub unsafe fn fat_write_fat_entry(
    vcb: *mut FAT32_VCB,
    cluster: u32,
    value: u32,
) -> Result<(), NTSTATUS> {
    unsafe {
        if cluster < 2 {
            return Err(STATUS_INVALID_PARAMETER);
        }
        
        let vcb_ref = &*vcb;
        
        // Вычисляем offset в FAT
        let fat_offset = cluster * 4;
        let fat_start_lba = vcb_ref.fat_start_sector;
        let sector_size = vcb_ref.bytes_per_sector;
        
        let fat_sector = fat_start_lba + (fat_offset / sector_size);
        let offset_in_sector = fat_offset % sector_size;
        
        // Читаем сектор FAT
        let mut sector_buffer = [0u8; 512];
        let read_status = read_disk_sector(
            vcb_ref.target_device,
            fat_sector as u64,
            &mut sector_buffer,
        );
        
        if read_status < 0 {
            return Err(read_status);
        }
        
        // Модифицируем entry (маскируем верхние 4 бита)
        let entry_ptr = sector_buffer.as_mut_ptr().add(offset_in_sector as usize) as *mut u32;
        let old_value = core::ptr::read_unaligned(entry_ptr);
        let new_value = (old_value & 0xF0000000) | (value & 0x0FFFFFFF);
        core::ptr::write_unaligned(entry_ptr, new_value);
        
        // Записываем сектор обратно
        let write_status = write_disk_sector(
            vcb_ref.target_device,
            fat_sector as u64,
            &sector_buffer,
        );
        
        if write_status < 0 {
            return Err(write_status);
        }
        
        Ok(())
    }
}

/// Записывает сектор на диск
unsafe fn write_disk_sector(
    device: PDEVICE_OBJECT,
    lba: u64,
    buffer: &[u8; 512],
) -> NTSTATUS {
    unsafe {
        let byte_offset = lba * 512;
        write_disk_bytes(device, byte_offset, buffer.as_ptr(), 512)
    }
}

/// Записывает байты на диск через synchronous IRP
unsafe fn write_disk_bytes(
    device: PDEVICE_OBJECT,
    byte_offset: u64,
    buffer: *const u8,
    length: u32,
) -> NTSTATUS {
    unsafe {
        let mut event: KEVENT = core::mem::zeroed();
        let mut iosb: IO_STATUS_BLOCK = core::mem::zeroed();
        iosb.status_or_pointer = IoStatusBlockUnion { status: STATUS_UNSUCCESSFUL };
        iosb.information = 0;
        
        let irp = IoBuildSynchronousFsdRequest(
            IRP_MJ_WRITE as ULONG,
            device,
            buffer as *mut u8 as PVOID, // Write needs mut
            length,
            byte_offset as i64,
            &mut event as *mut KEVENT as PVOID,
            &mut iosb as *mut IO_STATUS_BLOCK as PVOID,
        );
        
        if irp.is_null() {
            return STATUS_INSUFFICIENT_RESOURCES;
        }
        
        let status = IoCallDriver(device, irp);
        
        if status == STATUS_PENDING {
            KeWaitForSingleObject(
                &mut event as *mut KEVENT as PVOID,
                0, // Executive
                0, // KernelMode
                0, // Not alertable
                core::ptr::null_mut(), // No timeout
            );
            return iosb.status_or_pointer.status;
        }
        
        status
    }
}

// Новые константы
const STATUS_DISK_FULL: NTSTATUS = 0xC000007Fu32 as i32;
const STATUS_FILE_CORRUPT_ERROR: NTSTATUS = 0xC0000102u32 as i32;

/// Записывает данные в файл через cluster chain
pub unsafe fn fat_write_cluster_chain(
    vcb: *mut FAT32_VCB,
    first_cluster: u32,
    file_offset: u64,
    buffer: *const u8,
    length: u32,
) -> Result<u32, NTSTATUS> {
    unsafe {
        let vcb_ref = &*vcb;
        let cluster_size = vcb_ref.bytes_per_cluster;
        
        if first_cluster < 2 {
            return Ok(0); // Пустой файл
        }
        
        // Вычисляем начальный cluster
        let start_cluster_index = (file_offset / cluster_size as u64) as u32;
        let offset_in_first_cluster = (file_offset % cluster_size as u64) as u32;
        
        // Следуем по chain до нужного cluster
        let mut current_cluster = first_cluster;
        for _ in 0..start_cluster_index {
            let next = fat_read_next_cluster(vcb, current_cluster)?;
            if is_eoc(next) {
                return Err(STATUS_END_OF_FILE);
            }
            current_cluster = next;
        }
        
        // Записываем данные
        let mut bytes_written = 0u32;
        let mut remaining = length;
        let mut buf_offset = 0;
        let mut cluster_offset = offset_in_first_cluster;
        
        loop {
            if remaining == 0 {
                break;
            }
            
            let bytes_to_write = remaining.min(cluster_size - cluster_offset);
            
            let written = write_cluster_partial(
                vcb,
                current_cluster,
                cluster_offset,
                buffer.add(buf_offset as usize),
                bytes_to_write,
            )?;
            
            bytes_written += written;
            remaining -= written;
            buf_offset += written;
            
            if remaining == 0 {
                break;
            }
            
            // Переходим к следующему cluster
            cluster_offset = 0;
            
            let next = fat_read_next_cluster(vcb, current_cluster)?;
            if is_eoc(next) {
                break; // Достигли конца цепочки
            }
            
            current_cluster = next;
        }
        
        Ok(bytes_written)
    }
}

/// Записывает часть cluster
unsafe fn write_cluster_partial(
    vcb: *mut FAT32_VCB,
    cluster: u32,
    offset_in_cluster: u32,
    buffer: *const u8,
    length: u32,
) -> Result<u32, NTSTATUS> {
    unsafe {
        let vcb_ref = &*vcb;
        
        let lba = vcb_ref.bpb.cluster_to_lba(cluster);
        let bytes_per_sector = vcb_ref.bytes_per_sector;
        let cluster_byte_offset = lba * bytes_per_sector as u64;
        let absolute_offset = cluster_byte_offset + offset_in_cluster as u64;
        
        let status = write_disk_bytes(
            vcb_ref.target_device,
            absolute_offset,
            buffer,
            length,
        );
        
        if status < 0 {
            Err(status)
        } else {
            Ok(length)
        }
    }
}

/// Расширяет размер файла выделяя дополнительные clusters
pub unsafe fn fat_expand_file_size(
    vcb: *mut FAT32_VCB,
    fcb: *mut FAT32_FCB,
    new_size: u64,
) -> NTSTATUS {
    unsafe {
        let vcb_ref = &*vcb;
        let cluster_size = vcb_ref.bytes_per_cluster as u64;
        
        let old_clusters = if (*fcb).file_size > 0 {
            (((*fcb).file_size as u64 + cluster_size - 1) / cluster_size) as u32
        } else {
            0
        };
        
        let new_clusters = ((new_size + cluster_size - 1) / cluster_size) as u32;
        
        if new_clusters <= old_clusters {
            // Не нужно расширять
            (*fcb).file_size = new_size as u32;
            return STATUS_SUCCESS;
        }
        
        // Нужно выделить дополнительные clusters
        let clusters_to_add = new_clusters - old_clusters;
        
        // Находим последний cluster в chain
        let mut last_cluster = (*fcb).first_cluster;
        
        if last_cluster == 0 {
            // Файл был пустой - выделяем первый cluster
            match fat_allocate_cluster(vcb) {
                Ok(cluster) => {
                    (*fcb).first_cluster = cluster;
                    last_cluster = cluster;
                }
                Err(e) => return e,
            }
        } else {
            // Следуем до конца chain
            loop {
                match fat_read_next_cluster(vcb, last_cluster) {
                    Ok(next) if is_valid_cluster(next) && !is_eoc(next) => {
                        last_cluster = next;
                    }
                    _ => break,
                }
            }
        }
        
        // Добавляем нужное количество clusters
        for _ in 1..clusters_to_add {
            match fat_extend_chain(vcb, last_cluster) {
                Ok(new_cluster) => {
                    last_cluster = new_cluster;
                }
                Err(e) => return e,
            }
        }
        
        (*fcb).file_size = new_size as u32;
        (*fcb).flags |= 0x04; // FCB_FLAGS_MODIFIED
        
        STATUS_SUCCESS
    }
}

/// Читает байты с диска через synchronous IRP
unsafe fn read_disk_bytes(
    device: PDEVICE_OBJECT,
    byte_offset: u64,
    buffer: *mut u8,
    length: u32,
) -> NTSTATUS {
    unsafe {
        // Создаём synchronous IRP
        let mut event: KEVENT = core::mem::zeroed();
        let mut iosb: IO_STATUS_BLOCK = core::mem::zeroed();
        iosb.status_or_pointer = IoStatusBlockUnion { status: STATUS_UNSUCCESSFUL };
        iosb.information = 0;
        
        let irp = IoBuildSynchronousFsdRequest(
            IRP_MJ_READ as ULONG,
            device,
            buffer as PVOID,
            length,
            byte_offset as i64,
            &mut event as *mut KEVENT as PVOID,
            &mut iosb as *mut IO_STATUS_BLOCK as PVOID,
        );
        
        if irp.is_null() {
            return STATUS_INSUFFICIENT_RESOURCES;
        }
        
        let status = IoCallDriver(device, irp);
        
        // Для synchronous request ждём completion
        if status == STATUS_PENDING {
            // Ждём завершения I/O через event
            KeWaitForSingleObject(
                &mut event as *mut KEVENT as PVOID,
                0, // Executive
                0, // KernelMode
                0, // Not alertable
                core::ptr::null_mut(), // No timeout
            );
            // Получаем финальный статус из io_status_block
            return iosb.status_or_pointer.status;
        }
        
        status
    }
}

