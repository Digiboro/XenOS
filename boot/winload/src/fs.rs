//! Чтение файлов с ESP через UEFI Simple File System Protocol

use alloc::vec;

use uefi::boot;
use uefi::proto::media::file::File;
use uefi::proto::media::file::FileAttribute;
use uefi::proto::media::file::FileInfo;
use uefi::proto::media::file::FileMode;

use crate::types::LoadedFile;

/// Читает файл целиком в память.
/// Возвращает None если файл не найден или не удалось прочитать.
pub fn read_file(path: &'static str) -> Option<LoadedFile> {
    let image_handle = boot::image_handle();

    // Получаем файловую систему
    let mut fs = boot::get_image_file_system(image_handle).ok()?;

    // Конвертируем путь в CStr16
    // Путь должен использовать backslash как разделитель
    let mut path_buf = [0u16; 256];
    let mut i = 0;
    for c in path.chars() {
        if i >= 255 {
            break;
        }
        path_buf[i] = c as u16;
        i += 1;
    }
    path_buf[i] = 0; // null terminate

    let path_cstr = unsafe { uefi::CStr16::from_u16_with_nul_unchecked(&path_buf[..=i]) };

    // Открываем root directory
    let mut root = fs.open_volume().ok()?;

    // Открываем файл
    let file_handle = root
        .open(path_cstr, FileMode::Read, FileAttribute::empty())
        .ok()?;

    // Получаем размер файла через FileInfo
    let mut file = match file_handle.into_regular_file() {
        Some(f) => f,
        None => return None,
    };

    // Читаем FileInfo для получения размера
    let mut info_buf = [0u8; 256];
    let info = file.get_info::<FileInfo>(&mut info_buf).ok()?;
    let file_size = info.file_size() as usize;

    if file_size == 0 {
        return Some(LoadedFile {
            data: alloc::vec::Vec::new(),
            path,
        });
    }

    // Выделяем буфер и читаем файл
    let mut data = vec![0u8; file_size];
    let bytes_read = file.read(&mut data).ok()?;
    data.truncate(bytes_read);

    Some(LoadedFile { data, path })
}
