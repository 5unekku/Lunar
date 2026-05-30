use thiserror::Error;

/// errors that can occur when decoding a .li image.
///
/// covers header validation, chunk parsing, and decompression failures.
#[derive(Debug, Error)]
pub enum DecodeError {
    #[error("file too short for header")]
    TruncatedHeader,

    #[error("invalid magic bytes: expected LIF, got {0:?}")]
    InvalidMagic([u8; 4]),

    #[error("unsupported format version: {0}")]
    UnsupportedVersion(u16),

    #[error("unknown flag bits set: {0:#06x}")]
    UnknownFlags(u16),

    #[error("missing required pixel data chunk")]
    MissingPixelData,

    #[error("multiple pixel data chunks")]
    MultiplePixelData,

    #[error("pixel data size mismatch: expected {expected}, got {actual}")]
    SizeMismatch { expected: usize, actual: usize },

    #[error("zstd decompression failed: {0}")]
    ZstdError(#[from] std::io::Error),

    #[error("unexpected end of file")]
    TruncatedChunk,
}

/// errors that can occur when encoding a .li image.
///
/// covers dimension validation, pixel buffer validation, and compression failures.
#[derive(Debug, Error)]
pub enum EncodeError {
    #[error("image dimensions too large: {width}x{height}")]
    DimensionsTooLarge { width: u32, height: u32 },

    #[error("pixel buffer size mismatch: expected {expected}, got {actual}")]
    BufferSizeMismatch { expected: usize, actual: usize },

    #[error("zstd compression failed: {0}")]
    ZstdError(#[from] std::io::Error),
}
