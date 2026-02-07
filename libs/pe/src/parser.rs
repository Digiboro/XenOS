//! PE parsing functions
//!
//! Provides safe parsing of PE files from byte slices.
//! Works in no_std environment with optional alloc support.

use crate::constants::*;
use crate::error::{PeError, PeResult};
use crate::headers::{
    IMAGE_DATA_DIRECTORY, IMAGE_DOS_HEADER, IMAGE_EXPORT_DIRECTORY, IMAGE_FILE_HEADER,
    IMAGE_OPTIONAL_HEADER32, IMAGE_OPTIONAL_HEADER64, IMAGE_SECTION_HEADER,
};
use core::mem;

// =============================================================================
// Reading primitives
// =============================================================================

/// Read u16 from byte slice at offset (little-endian)
#[inline]
pub fn read_u16(data: &[u8], offset: usize) -> Option<u16> {
    if offset + 2 > data.len() {
        return None;
    }
    Some(u16::from_le_bytes([data[offset], data[offset + 1]]))
}

/// Read u32 from byte slice at offset (little-endian)
#[inline]
pub fn read_u32(data: &[u8], offset: usize) -> Option<u32> {
    if offset + 4 > data.len() {
        return None;
    }
    Some(u32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ]))
}

/// Read u64 from byte slice at offset (little-endian)
#[inline]
pub fn read_u64(data: &[u8], offset: usize) -> Option<u64> {
    if offset + 8 > data.len() {
        return None;
    }
    Some(u64::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
        data[offset + 4],
        data[offset + 5],
        data[offset + 6],
        data[offset + 7],
    ]))
}

/// Write u64 to byte slice at offset (little-endian)
#[inline]
pub fn write_u64(data: &mut [u8], offset: usize, value: u64) -> bool {
    if offset + 8 > data.len() {
        return false;
    }
    data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    true
}

/// Read null-terminated string from byte slice
pub fn read_cstring(data: &[u8], offset: usize) -> Option<&str> {
    if offset >= data.len() {
        return None;
    }

    let mut end = offset;
    while end < data.len() && data[end] != 0 {
        end += 1;
    }

    core::str::from_utf8(&data[offset..end]).ok()
}

// =============================================================================
// PE Image Reference
// =============================================================================

/// A reference to a PE image in memory
///
/// This struct provides safe access to PE headers and sections
/// without copying data.
#[derive(Clone, Copy)]
pub struct PeImage<'a> {
    data: &'a [u8],
    pe_offset: usize,
}

impl<'a> PeImage<'a> {
    /// Parse a PE image from a byte slice
    ///
    /// Validates DOS and PE signatures.
    pub fn parse(data: &'a [u8]) -> PeResult<Self> {
        // Check minimum size for DOS header
        if data.len() < DOS_HEADER_SIZE {
            return Err(PeError::BufferTooSmall);
        }

        // Validate DOS signature
        let dos_sig = read_u16(data, 0).ok_or(PeError::BufferTooSmall)?;
        if dos_sig != IMAGE_DOS_SIGNATURE {
            return Err(PeError::InvalidDosSignature);
        }

        // Get PE header offset
        let pe_offset = read_u32(data, 0x3C).ok_or(PeError::BufferTooSmall)? as usize;

        // Validate PE signature
        if pe_offset + 4 > data.len() {
            return Err(PeError::BufferTooSmall);
        }

        let pe_sig = read_u32(data, pe_offset).ok_or(PeError::BufferTooSmall)?;
        if pe_sig != IMAGE_NT_SIGNATURE {
            return Err(PeError::InvalidPeSignature);
        }

        Ok(PeImage { data, pe_offset })
    }

    /// Get raw data slice
    #[inline]
    pub fn data(&self) -> &'a [u8] {
        self.data
    }

    /// Get PE header offset
    #[inline]
    pub fn pe_offset(&self) -> usize {
        self.pe_offset
    }

    /// Get DOS header
    pub fn dos_header(&self) -> &IMAGE_DOS_HEADER {
        unsafe { &*(self.data.as_ptr() as *const IMAGE_DOS_HEADER) }
    }

    /// Get COFF file header
    pub fn file_header(&self) -> PeResult<&IMAGE_FILE_HEADER> {
        let offset = self.pe_offset + 4;
        if offset + FILE_HEADER_SIZE > self.data.len() {
            return Err(PeError::BufferTooSmall);
        }
        Ok(unsafe { &*(self.data.as_ptr().add(offset) as *const IMAGE_FILE_HEADER) })
    }

    /// Get optional header offset
    #[inline]
    fn optional_header_offset(&self) -> usize {
        self.pe_offset + 4 + FILE_HEADER_SIZE
    }

    /// Check if this is a PE32+ (64-bit) image
    pub fn is_pe32_plus(&self) -> PeResult<bool> {
        let offset = self.optional_header_offset();
        let magic = read_u16(self.data, offset).ok_or(PeError::BufferTooSmall)?;
        Ok(magic == IMAGE_NT_OPTIONAL_HDR64_MAGIC)
    }

    /// Check if this is a PE32 (32-bit) image
    pub fn is_pe32(&self) -> PeResult<bool> {
        let offset = self.optional_header_offset();
        let magic = read_u16(self.data, offset).ok_or(PeError::BufferTooSmall)?;
        Ok(magic == IMAGE_NT_OPTIONAL_HDR32_MAGIC)
    }

    /// Get optional header (PE32+)
    pub fn optional_header_64(&self) -> PeResult<&IMAGE_OPTIONAL_HEADER64> {
        let offset = self.optional_header_offset();
        if offset + mem::size_of::<IMAGE_OPTIONAL_HEADER64>() > self.data.len() {
            return Err(PeError::BufferTooSmall);
        }

        let header = unsafe { &*(self.data.as_ptr().add(offset) as *const IMAGE_OPTIONAL_HEADER64) };

        if header.magic != IMAGE_NT_OPTIONAL_HDR64_MAGIC {
            return Err(PeError::NotPe32Plus);
        }

        Ok(header)
    }

    /// Get optional header (PE32)
    pub fn optional_header_32(&self) -> PeResult<&IMAGE_OPTIONAL_HEADER32> {
        let offset = self.optional_header_offset();
        if offset + mem::size_of::<IMAGE_OPTIONAL_HEADER32>() > self.data.len() {
            return Err(PeError::BufferTooSmall);
        }

        let header = unsafe { &*(self.data.as_ptr().add(offset) as *const IMAGE_OPTIONAL_HEADER32) };

        if header.magic != IMAGE_NT_OPTIONAL_HDR32_MAGIC {
            return Err(PeError::NotPe32);
        }

        Ok(header)
    }

    /// Get data directory entry by index
    pub fn data_directory(&self, index: usize) -> PeResult<IMAGE_DATA_DIRECTORY> {
        let file_header = self.file_header()?;
        let opt_size = file_header.size_of_optional_header as usize;
        let opt_offset = self.optional_header_offset();

        // Get number of directories
        let num_dirs = if self.is_pe32_plus()? {
            let opt = self.optional_header_64()?;
            opt.number_of_rva_and_sizes as usize
        } else {
            let opt = self.optional_header_32()?;
            opt.number_of_rva_and_sizes as usize
        };

        if index >= num_dirs {
            return Err(PeError::DataDirectoryIndexOutOfBounds {
                index,
                count: num_dirs,
            });
        }

        // Data directories start after the fixed part of optional header
        let dd_offset = if self.is_pe32_plus()? {
            opt_offset + mem::size_of::<IMAGE_OPTIONAL_HEADER64>()
        } else {
            opt_offset + mem::size_of::<IMAGE_OPTIONAL_HEADER32>()
        };

        let entry_offset = dd_offset + index * mem::size_of::<IMAGE_DATA_DIRECTORY>();

        if entry_offset + mem::size_of::<IMAGE_DATA_DIRECTORY>() > opt_offset + opt_size {
            return Err(PeError::BufferTooSmall);
        }

        let rva = read_u32(self.data, entry_offset).ok_or(PeError::BufferTooSmall)?;
        let size = read_u32(self.data, entry_offset + 4).ok_or(PeError::BufferTooSmall)?;

        Ok(IMAGE_DATA_DIRECTORY {
            virtual_address: rva,
            size,
        })
    }

    /// Get section headers offset
    pub fn sections_offset(&self) -> PeResult<usize> {
        let file_header = self.file_header()?;
        Ok(self.optional_header_offset() + file_header.size_of_optional_header as usize)
    }

    /// Get number of sections
    pub fn section_count(&self) -> PeResult<usize> {
        let file_header = self.file_header()?;
        Ok(file_header.number_of_sections as usize)
    }

    /// Get section header by index
    pub fn section(&self, index: usize) -> PeResult<&IMAGE_SECTION_HEADER> {
        let count = self.section_count()?;
        if index >= count {
            return Err(PeError::SectionIndexOutOfBounds { index, count });
        }

        let offset = self.sections_offset()? + index * SECTION_HEADER_SIZE;
        if offset + SECTION_HEADER_SIZE > self.data.len() {
            return Err(PeError::BufferTooSmall);
        }

        Ok(unsafe { &*(self.data.as_ptr().add(offset) as *const IMAGE_SECTION_HEADER) })
    }

    /// Iterate over all section headers
    pub fn sections(&self) -> PeResult<SectionIter<'a>> {
        Ok(SectionIter {
            data: self.data,
            offset: self.sections_offset()?,
            remaining: self.section_count()?,
        })
    }

    /// Convert RVA to file offset
    pub fn rva_to_offset(&self, rva: u32) -> PeResult<usize> {
        // Check if RVA is in headers
        if self.is_pe32_plus()? {
            let opt = self.optional_header_64()?;
            if rva < opt.size_of_headers {
                return Ok(rva as usize);
            }
        } else {
            let opt = self.optional_header_32()?;
            if rva < opt.size_of_headers {
                return Ok(rva as usize);
            }
        }

        // Find section containing RVA
        for section in self.sections()? {
            let va_start = section.virtual_address;
            let va_end = va_start + section.virtual_size.max(section.size_of_raw_data);

            if rva >= va_start && rva < va_end {
                let section_offset = (rva - va_start) as usize;
                return Ok(section.pointer_to_raw_data as usize + section_offset);
            }
        }

        Err(PeError::RvaNotInSection { rva })
    }

    /// Get data at RVA
    pub fn data_at_rva(&self, rva: u32, size: usize) -> PeResult<&'a [u8]> {
        let offset = self.rva_to_offset(rva)?;
        if offset + size > self.data.len() {
            return Err(PeError::BufferTooSmall);
        }
        Ok(&self.data[offset..offset + size])
    }

    /// Get entry point RVA
    pub fn entry_point(&self) -> PeResult<u32> {
        if self.is_pe32_plus()? {
            Ok(self.optional_header_64()?.address_of_entry_point)
        } else {
            Ok(self.optional_header_32()?.address_of_entry_point)
        }
    }

    /// Get image base
    pub fn image_base(&self) -> PeResult<u64> {
        if self.is_pe32_plus()? {
            Ok(self.optional_header_64()?.image_base)
        } else {
            Ok(self.optional_header_32()?.image_base as u64)
        }
    }

    /// Get size of image
    pub fn size_of_image(&self) -> PeResult<u32> {
        if self.is_pe32_plus()? {
            Ok(self.optional_header_64()?.size_of_image)
        } else {
            Ok(self.optional_header_32()?.size_of_image)
        }
    }

    /// Get size of headers
    pub fn size_of_headers(&self) -> PeResult<u32> {
        if self.is_pe32_plus()? {
            Ok(self.optional_header_64()?.size_of_headers)
        } else {
            Ok(self.optional_header_32()?.size_of_headers)
        }
    }

    /// Get checksum
    pub fn checksum(&self) -> PeResult<u32> {
        if self.is_pe32_plus()? {
            Ok(self.optional_header_64()?.checksum)
        } else {
            Ok(self.optional_header_32()?.checksum)
        }
    }

    /// Get timestamp
    pub fn timestamp(&self) -> PeResult<u32> {
        Ok(self.file_header()?.time_date_stamp)
    }

    /// Get machine type
    pub fn machine(&self) -> PeResult<u16> {
        Ok(self.file_header()?.machine)
    }

    /// Check if this is AMD64
    pub fn is_amd64(&self) -> PeResult<bool> {
        Ok(self.file_header()?.machine == IMAGE_FILE_MACHINE_AMD64)
    }
}

// =============================================================================
// Section Iterator
// =============================================================================

/// Iterator over section headers
pub struct SectionIter<'a> {
    data: &'a [u8],
    offset: usize,
    remaining: usize,
}

impl<'a> Iterator for SectionIter<'a> {
    type Item = &'a IMAGE_SECTION_HEADER;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }

        if self.offset + SECTION_HEADER_SIZE > self.data.len() {
            return None;
        }

        let section =
            unsafe { &*(self.data.as_ptr().add(self.offset) as *const IMAGE_SECTION_HEADER) };

        self.offset += SECTION_HEADER_SIZE;
        self.remaining -= 1;

        Some(section)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl<'a> ExactSizeIterator for SectionIter<'a> {}

// =============================================================================
// Quick Validation
// =============================================================================

/// Quick validation result
#[derive(Debug, Clone, Copy)]
pub struct PeValidation {
    /// Whether the PE is valid
    pub valid: bool,
    /// Whether this is PE32+ (64-bit)
    pub is_pe32_plus: bool,
    /// Machine type
    pub machine: u16,
    /// Entry point RVA
    pub entry_point: u32,
    /// Preferred image base
    pub image_base: u64,
    /// Size of image in memory
    pub size_of_image: u32,
}

impl Default for PeValidation {
    fn default() -> Self {
        PeValidation {
            valid: false,
            is_pe32_plus: false,
            machine: 0,
            entry_point: 0,
            image_base: 0,
            size_of_image: 0,
        }
    }
}

/// Quick PE validation without full parsing
///
/// Returns basic information about the PE file or an invalid result
/// if validation fails.
pub fn validate_pe(data: &[u8]) -> PeValidation {
    let invalid = PeValidation::default();

    // Parse PE
    let pe = match PeImage::parse(data) {
        Ok(pe) => pe,
        Err(_) => return invalid,
    };

    // Get machine type
    let machine = match pe.machine() {
        Ok(m) => m,
        Err(_) => return invalid,
    };

    // Check if PE32+
    let is_pe32_plus = match pe.is_pe32_plus() {
        Ok(v) => v,
        Err(_) => return invalid,
    };

    // Get optional header info
    if is_pe32_plus {
        match pe.optional_header_64() {
            Ok(opt) => PeValidation {
                valid: true,
                is_pe32_plus: true,
                machine,
                entry_point: opt.address_of_entry_point,
                image_base: opt.image_base,
                size_of_image: opt.size_of_image,
            },
            Err(_) => invalid,
        }
    } else {
        match pe.optional_header_32() {
            Ok(opt) => PeValidation {
                valid: true,
                is_pe32_plus: false,
                machine,
                entry_point: opt.address_of_entry_point,
                image_base: opt.image_base as u64,
                size_of_image: opt.size_of_image,
            },
            Err(_) => invalid,
        }
    }
}

// =============================================================================
// Relocation Processing
// =============================================================================

/// Apply base relocations to an image
///
/// # Arguments
/// * `image` - Mutable slice containing the loaded image
/// * `pe` - Parsed PE image (for headers)
/// * `delta` - Relocation delta (new_base - old_base)
///
/// # Returns
/// Ok(()) on success, or error
pub fn apply_relocations(image: &mut [u8], pe: &PeImage, delta: i64) -> PeResult<()> {
    if delta == 0 {
        return Ok(());
    }

    // Get relocation directory
    let reloc_dir = pe.data_directory(IMAGE_DIRECTORY_ENTRY_BASERELOC)?;
    if !reloc_dir.is_present() {
        return Ok(()); // No relocations
    }

    let reloc_rva = reloc_dir.virtual_address;
    let reloc_size = reloc_dir.size;

    // For loaded image, RVA == offset
    let mut offset = reloc_rva as usize;
    let end_offset = offset + reloc_size as usize;

    while offset + 8 <= end_offset && offset + 8 <= image.len() {
        let block_rva = read_u32(image, offset).ok_or(PeError::BufferTooSmall)?;
        let block_size = read_u32(image, offset + 4).ok_or(PeError::BufferTooSmall)?;

        if block_size < 8 {
            break;
        }

        let num_entries = (block_size as usize - 8) / 2;

        for i in 0..num_entries {
            let entry_offset = offset + 8 + i * 2;
            if entry_offset + 2 > image.len() {
                break;
            }

            let entry = read_u16(image, entry_offset).ok_or(PeError::BufferTooSmall)?;
            let reloc_type = (entry >> 12) as u16;
            let reloc_offset = (entry & 0x0FFF) as u32;

            let target_rva = block_rva + reloc_offset;
            let target_offset = target_rva as usize;

            match reloc_type {
                IMAGE_REL_BASED_ABSOLUTE => {
                    // Padding, skip
                },
                IMAGE_REL_BASED_DIR64 => {
                    // 64-bit relocation
                    if target_offset + 8 <= image.len() {
                        let value = read_u64(image, target_offset).ok_or(PeError::BufferTooSmall)?;
                        let new_value = (value as i64 + delta) as u64;
                        write_u64(image, target_offset, new_value);
                    }
                },
                IMAGE_REL_BASED_HIGHLOW => {
                    // 32-bit relocation
                    if target_offset + 4 <= image.len() {
                        let value = read_u32(image, target_offset).ok_or(PeError::BufferTooSmall)?;
                        let new_value = (value as i64 + delta) as u32;
                        image[target_offset..target_offset + 4]
                            .copy_from_slice(&new_value.to_le_bytes());
                    }
                },
                _ => {
                    // Unsupported type - skip for now
                    // Could return error: Err(PeError::UnsupportedRelocationType { reloc_type })
                },
            }
        }

        offset += block_size as usize;
    }

    Ok(())
}

// =============================================================================
// Export Resolution
// =============================================================================

/// Resolve export by name
///
/// # Arguments
/// * `pe` - Parsed PE image
/// * `name` - Export name to find
///
/// # Returns
/// RVA of the exported function or error
pub fn resolve_export_by_name(pe: &PeImage, name: &str) -> PeResult<u32> {
    let export_dir = pe.data_directory(IMAGE_DIRECTORY_ENTRY_EXPORT)?;
    if !export_dir.is_present() {
        return Err(PeError::ExportNotFound);
    }

    let dir_offset = pe.rva_to_offset(export_dir.virtual_address)?;
    if dir_offset + mem::size_of::<IMAGE_EXPORT_DIRECTORY>() > pe.data.len() {
        return Err(PeError::BufferTooSmall);
    }

    // Read export directory fields
    let number_of_names = read_u32(pe.data, dir_offset + 0x18).ok_or(PeError::BufferTooSmall)?;
    let address_of_functions =
        read_u32(pe.data, dir_offset + 0x1C).ok_or(PeError::BufferTooSmall)?;
    let address_of_names = read_u32(pe.data, dir_offset + 0x20).ok_or(PeError::BufferTooSmall)?;
    let address_of_name_ordinals =
        read_u32(pe.data, dir_offset + 0x24).ok_or(PeError::BufferTooSmall)?;

    let names_offset = pe.rva_to_offset(address_of_names)?;
    let ordinals_offset = pe.rva_to_offset(address_of_name_ordinals)?;
    let functions_offset = pe.rva_to_offset(address_of_functions)?;

    // Binary search in name table
    let mut low = 0usize;
    let mut high = number_of_names as usize;

    while low < high {
        let mid = (low + high) / 2;

        // Read name RVA
        let name_rva = read_u32(pe.data, names_offset + mid * 4).ok_or(PeError::BufferTooSmall)?;
        let name_offset = pe.rva_to_offset(name_rva)?;
        let export_name = read_cstring(pe.data, name_offset).ok_or(PeError::InvalidString)?;

        match export_name.cmp(name) {
            core::cmp::Ordering::Equal => {
                // Found! Get ordinal
                let ordinal =
                    read_u16(pe.data, ordinals_offset + mid * 2).ok_or(PeError::BufferTooSmall)?;

                // Get function RVA from EAT
                let func_rva = read_u32(pe.data, functions_offset + ordinal as usize * 4)
                    .ok_or(PeError::BufferTooSmall)?;

                // Check for forwarder (RVA within export directory)
                if func_rva >= export_dir.virtual_address
                    && func_rva < export_dir.virtual_address + export_dir.size
                {
                    // Forwarder - not supported yet
                    return Err(PeError::ExportNotFound);
                }

                return Ok(func_rva);
            },
            core::cmp::Ordering::Less => {
                low = mid + 1;
            },
            core::cmp::Ordering::Greater => {
                high = mid;
            },
        }
    }

    Err(PeError::ExportNotFound)
}

/// Resolve export by ordinal
///
/// # Arguments
/// * `pe` - Parsed PE image
/// * `ordinal` - Export ordinal
///
/// # Returns
/// RVA of the exported function or error
pub fn resolve_export_by_ordinal(pe: &PeImage, ordinal: u32) -> PeResult<u32> {
    let export_dir = pe.data_directory(IMAGE_DIRECTORY_ENTRY_EXPORT)?;
    if !export_dir.is_present() {
        return Err(PeError::ExportNotFound);
    }

    let dir_offset = pe.rva_to_offset(export_dir.virtual_address)?;

    // Read export directory fields
    let base = read_u32(pe.data, dir_offset + 0x10).ok_or(PeError::BufferTooSmall)?;
    let number_of_functions =
        read_u32(pe.data, dir_offset + 0x14).ok_or(PeError::BufferTooSmall)?;
    let address_of_functions =
        read_u32(pe.data, dir_offset + 0x1C).ok_or(PeError::BufferTooSmall)?;

    // Calculate index
    if ordinal < base {
        return Err(PeError::ExportNotFound);
    }
    let index = (ordinal - base) as usize;

    if index >= number_of_functions as usize {
        return Err(PeError::ExportNotFound);
    }

    let functions_offset = pe.rva_to_offset(address_of_functions)?;
    let func_rva =
        read_u32(pe.data, functions_offset + index * 4).ok_or(PeError::BufferTooSmall)?;

    // Check for forwarder
    if func_rva >= export_dir.virtual_address
        && func_rva < export_dir.virtual_address + export_dir.size
    {
        return Err(PeError::ExportNotFound);
    }

    Ok(func_rva)
}

