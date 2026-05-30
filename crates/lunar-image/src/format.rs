//! Lunar Image Format (LIF) binary layout constants.
//!
//! the .li file format consists of a 16-byte header followed by one or more
//! compressed chunks. each chunk has a 16-byte header and compressed payload.

/// magic bytes identifying a .li file: `b"LIF\0"`.
pub const MAGIC: [u8; 4] = *b"LIF\0";

/// current format version. only version 1 is supported.
pub const VERSION: u16 = 1;

/// size of the file header in bytes.
pub const HEADER_SIZE: usize = 16;

/// size of a chunk header in bytes.
pub const CHUNK_HEADER_SIZE: usize = 16;

/// chunk type identifiers.
///
/// each chunk stores its type as a single byte in the chunk header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ChunkType {
    /// raw pixel data (zstd-compressed RGBA).
    PixelData = 0x00,
    /// optional metadata string (zstd-compressed UTF-8).
    Metadata = 0x01,
    /// optional ICC color profile (zstd-compressed).
    IccProfile = 0x02,
}

impl ChunkType {
    /// convert a raw byte value into a chunk type variant.
    /// returns `None` if the value doesn't match any known chunk type.
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x00 => Some(Self::PixelData),
            0x01 => Some(Self::Metadata),
            0x02 => Some(Self::IccProfile),
            _ => None,
        }
    }
}

/// flag bit: image contains alpha channel data.
pub const FLAG_HAS_ALPHA: u16 = 1 << 0;
/// flag bit: file contains a metadata chunk.
pub const FLAG_HAS_METADATA: u16 = 1 << 1;
/// flag bit: file contains an ICC color profile chunk.
pub const FLAG_HAS_ICC: u16 = 1 << 2;
/// flag bit: pixel data uses premultiplied alpha.
pub const FLAG_PREMULTIPLIED: u16 = 1 << 3;
/// flag bit: pixel data is stored as planar channels (RRRR...GGGG...BBBB...AAAA...)
/// rather than interleaved RGBA. planar layout compresses significantly better.
pub const FLAG_PLANAR: u16 = 1 << 4;
/// flag bit: each plane row is delta-filtered (png-style predictor) before zstd.
/// implies [`FLAG_PLANAR`]. set only when filtering compresses smaller than not,
/// so it is always safe for the decoder to honour. see `filter.rs`.
pub const FLAG_FILTERED: u16 = 1 << 5;
/// mask of all known flag bits.
pub const FLAG_ALL_KNOWN: u16 = FLAG_HAS_ALPHA
    | FLAG_HAS_METADATA
    | FLAG_HAS_ICC
    | FLAG_PREMULTIPLIED
    | FLAG_PLANAR
    | FLAG_FILTERED;

/// parsed header from a .li file.
///
/// contains the format version, feature flags, and image dimensions.
#[derive(Debug)]
#[allow(dead_code)]
pub struct Header {
    /// format version number (currently 1).
    pub version: u16,
    /// feature flags (alpha, metadata, ICC, premultiplied).
    pub flags: u16,
    /// image width in pixels.
    pub width: u32,
    /// image height in pixels.
    pub height: u32,
}

impl Header {
    /// parse a header from raw file bytes.
    /// validates the magic bytes, version, and flag bits.
    /// returns an error if the data is too short or invalid.
    pub fn parse(data: &[u8]) -> Result<Self, crate::error::DecodeError> {
        use crate::error::DecodeError;

        if data.len() < HEADER_SIZE {
            return Err(DecodeError::TruncatedHeader);
        }

        let magic = [data[0], data[1], data[2], data[3]];
        if magic != MAGIC {
            return Err(DecodeError::InvalidMagic(magic));
        }

        let version = u16::from_le_bytes([data[4], data[5]]);
        if version != VERSION {
            return Err(DecodeError::UnsupportedVersion(version));
        }

        let flags = u16::from_le_bytes([data[6], data[7]]);
        if flags & !FLAG_ALL_KNOWN != 0 {
            return Err(DecodeError::UnknownFlags(flags));
        }

        let width = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
        let height = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);

        Ok(Self {
            version,
            flags,
            width,
            height,
        })
    }

    /// calculate the expected number of bytes for pixel data.
    ///
    /// returns `width * height * 4` (RGBA8).
    pub const fn expected_pixel_bytes(&self) -> usize {
        (self.width as usize) * (self.height as usize) * 4
    }
}

/// write a header into a buffer.
///
/// appends exactly [`HEADER_SIZE`] bytes to the output buffer.
pub fn write_header(buf: &mut Vec<u8>, width: u32, height: u32, flags: u16) {
    buf.extend_from_slice(&MAGIC);
    buf.extend_from_slice(&VERSION.to_le_bytes());
    buf.extend_from_slice(&flags.to_le_bytes());
    buf.extend_from_slice(&width.to_le_bytes());
    buf.extend_from_slice(&height.to_le_bytes());
}

/// parsed chunk header.
///
/// each chunk header identifies the chunk type and stores both
/// compressed and uncompressed sizes for validation.
#[derive(Debug)]
#[allow(dead_code)]
pub struct ChunkHeader {
    /// the type of data in this chunk.
    pub chunk_type: ChunkType,
    /// size of the data after decompression.
    pub uncompressed_size: u32,
    /// size of the compressed payload in the file.
    pub compressed_size: u32,
    /// zstd dictionary ID (0 = no dictionary).
    pub dict_id: u32,
}

impl ChunkHeader {
    /// parse a chunk header from raw bytes.
    ///
    /// expects at least [`CHUNK_HEADER_SIZE`] bytes of data.
    pub fn parse(data: &[u8]) -> Result<Self, crate::error::DecodeError> {
        use crate::error::DecodeError;

        if data.len() < CHUNK_HEADER_SIZE {
            return Err(DecodeError::TruncatedChunk);
        }

        let chunk_type = ChunkType::from_u8(data[0]).ok_or(DecodeError::TruncatedChunk)?;
        let uncompressed_size = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        let compressed_size = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
        let dict_id = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);

        Ok(Self {
            chunk_type,
            uncompressed_size,
            compressed_size,
            dict_id,
        })
    }

    /// write a chunk header into a buffer.
    ///
    /// appends exactly [`CHUNK_HEADER_SIZE`] bytes to the output buffer.
    pub fn write(
        buf: &mut Vec<u8>,
        chunk_type: ChunkType,
        uncompressed_size: u32,
        compressed_size: u32,
        dict_id: u32,
    ) {
        buf.push(chunk_type as u8);
        buf.extend_from_slice(&[0, 0, 0]); // reserved
        buf.extend_from_slice(&uncompressed_size.to_le_bytes());
        buf.extend_from_slice(&compressed_size.to_le_bytes());
        buf.extend_from_slice(&dict_id.to_le_bytes());
    }
}
