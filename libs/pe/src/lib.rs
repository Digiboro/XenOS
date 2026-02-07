//! PE (Portable Executable) format library
//!
//! This crate provides types and functions for parsing and manipulating
//! PE32 and PE32+ (64-bit) executable files.
//!
//! # Features
//!
//! - `alloc` - Enable features requiring heap allocation (Vec for sections, etc.)
//!
//! # Usage
//!
//! ```no_run
//! use pe::{PeImage, PeError};
//!
//! fn parse_pe(data: &[u8]) -> Result<(), PeError> {
//!     let pe = PeImage::parse(data)?;
//!     
//!     println!("Entry point: 0x{:X}", pe.entry_point()?);
//!     println!("Image base: 0x{:X}", pe.image_base()?);
//!     println!("Size of image: 0x{:X}", pe.size_of_image()?);
//!     
//!     for section in pe.sections()? {
//!         println!("Section: {}", section.name_str());
//!     }
//!     
//!     Ok(())
//! }
//! ```

#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(feature = "alloc")]
extern crate alloc;

pub mod constants;
pub mod error;
pub mod headers;
pub mod parser;

// Re-export main types at crate root
pub use constants::*;
pub use error::{PeError, PeResult};
pub use headers::{
    IMAGE_BASE_RELOCATION, IMAGE_DATA_DIRECTORY, IMAGE_DOS_HEADER, IMAGE_EXPORT_DIRECTORY,
    IMAGE_FILE_HEADER, IMAGE_IMPORT_BY_NAME, IMAGE_IMPORT_DESCRIPTOR, IMAGE_OPTIONAL_HEADER32,
    IMAGE_OPTIONAL_HEADER64, IMAGE_SECTION_HEADER,
};
pub use parser::{
    apply_relocations, read_cstring, read_u16, read_u32, read_u64, resolve_export_by_name,
    resolve_export_by_ordinal, validate_pe, write_u64, PeImage, PeValidation, SectionIter,
};

