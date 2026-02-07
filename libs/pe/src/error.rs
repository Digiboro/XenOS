//! PE parsing error types

use core::fmt;

/// Error type for PE parsing operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeError {
    /// File/buffer is too small to contain required data
    BufferTooSmall,

    /// Invalid DOS signature (expected "MZ")
    InvalidDosSignature,

    /// Invalid PE signature (expected "PE\0\0")
    InvalidPeSignature,

    /// Invalid optional header magic (not PE32 or PE32+)
    InvalidOptionalHeaderMagic,

    /// Expected PE32+ but found PE32
    NotPe32Plus,

    /// Expected PE32 but found PE32+
    NotPe32,

    /// Unsupported machine architecture
    UnsupportedMachine {
        /// The machine type that was found
        machine: u16,
    },

    /// Offset points outside the buffer
    InvalidOffset {
        /// Name of the field containing invalid offset
        field: &'static str,
        /// The invalid offset value
        offset: usize,
    },

    /// Section index out of bounds
    SectionIndexOutOfBounds {
        /// Requested index
        index: usize,
        /// Total number of sections
        count: usize,
    },

    /// Data directory index out of bounds
    DataDirectoryIndexOutOfBounds {
        /// Requested index
        index: usize,
        /// Total number of directories
        count: usize,
    },

    /// RVA is not within any section
    RvaNotInSection {
        /// The RVA that couldn't be resolved
        rva: u32,
    },

    /// String is not valid UTF-8
    InvalidString,

    /// Export not found
    ExportNotFound,

    /// Unsupported relocation type
    UnsupportedRelocationType {
        /// The relocation type that was found
        reloc_type: u16,
    },
}

impl fmt::Display for PeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PeError::BufferTooSmall => write!(f, "buffer too small for PE data"),
            PeError::InvalidDosSignature => write!(f, "invalid DOS signature (expected MZ)"),
            PeError::InvalidPeSignature => write!(f, "invalid PE signature (expected PE\\0\\0)"),
            PeError::InvalidOptionalHeaderMagic => {
                write!(f, "invalid optional header magic (not PE32 or PE32+)")
            },
            PeError::NotPe32Plus => write!(f, "expected PE32+ but found PE32"),
            PeError::NotPe32 => write!(f, "expected PE32 but found PE32+"),
            PeError::UnsupportedMachine { machine } => {
                write!(f, "unsupported machine type: 0x{:04X}", machine)
            },
            PeError::InvalidOffset { field, offset } => {
                write!(f, "invalid offset in {}: 0x{:X}", field, offset)
            },
            PeError::SectionIndexOutOfBounds { index, count } => {
                write!(f, "section index {} out of bounds (count: {})", index, count)
            },
            PeError::DataDirectoryIndexOutOfBounds { index, count } => {
                write!(
                    f,
                    "data directory index {} out of bounds (count: {})",
                    index, count
                )
            },
            PeError::RvaNotInSection { rva } => {
                write!(f, "RVA 0x{:X} is not within any section", rva)
            },
            PeError::InvalidString => write!(f, "string is not valid UTF-8"),
            PeError::ExportNotFound => write!(f, "export not found"),
            PeError::UnsupportedRelocationType { reloc_type } => {
                write!(f, "unsupported relocation type: {}", reloc_type)
            },
        }
    }
}

/// Result type alias for PE operations
pub type PeResult<T> = Result<T, PeError>;

