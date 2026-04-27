/// Magic bytes: b'MRIF'
pub const MAGIC: [u8; 4] = [0x4D, 0x52, 0x49, 0x46];

/// Current format version
pub const VERSION: u16 = 1;

/// Header size in bytes
pub const HEADER_SIZE: usize = 16;

/// Chunk header size in bytes
pub const CHUNK_HEADER_SIZE: usize = 16;

/// Chunk type identifiers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ChunkType {
    PixelData = 0x00,
    Metadata = 0x01,
    IccProfile = 0x02,
}

impl ChunkType {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x00 => Some(ChunkType::PixelData),
            0x01 => Some(ChunkType::Metadata),
            0x02 => Some(ChunkType::IccProfile),
            _ => None,
        }
    }
}

/// Flag bits
pub const FLAG_HAS_ALPHA: u16 = 1 << 0;
pub const FLAG_HAS_METADATA: u16 = 1 << 1;
pub const FLAG_HAS_ICC: u16 = 1 << 2;
pub const FLAG_PREMULTIPLIED: u16 = 1 << 3;
pub const FLAG_ALL_KNOWN: u16 =
    FLAG_HAS_ALPHA | FLAG_HAS_METADATA | FLAG_HAS_ICC | FLAG_PREMULTIPLIED;

/// Parsed header from a .mi file
#[derive(Debug)]
#[allow(dead_code)]
pub struct Header {
    pub version: u16,
    pub flags: u16,
    pub width: u32,
    pub height: u32,
}

impl Header {
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

        Ok(Header {
            version,
            flags,
            width,
            height,
        })
    }

    pub fn expected_pixel_bytes(&self) -> usize {
        (self.width as usize) * (self.height as usize) * 4
    }
}

/// Write a header into a buffer (assumes buffer has at least HEADER_SIZE bytes)
pub fn write_header(buf: &mut Vec<u8>, width: u32, height: u32, flags: u16) {
    buf.extend_from_slice(&MAGIC);
    buf.extend_from_slice(&VERSION.to_le_bytes());
    buf.extend_from_slice(&flags.to_le_bytes());
    buf.extend_from_slice(&width.to_le_bytes());
    buf.extend_from_slice(&height.to_le_bytes());
}

/// Parsed chunk header
#[derive(Debug)]
#[allow(dead_code)]
pub struct ChunkHeader {
    pub chunk_type: ChunkType,
    pub uncompressed_size: u32,
    pub compressed_size: u32,
    pub dict_id: u32,
}

impl ChunkHeader {
    pub fn parse(data: &[u8]) -> Result<Self, crate::error::DecodeError> {
        use crate::error::DecodeError;

        if data.len() < CHUNK_HEADER_SIZE {
            return Err(DecodeError::TruncatedChunk);
        }

        let chunk_type = ChunkType::from_u8(data[0]).ok_or(DecodeError::TruncatedChunk)?;
        let uncompressed_size = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        let compressed_size = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
        let dict_id = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);

        Ok(ChunkHeader {
            chunk_type,
            uncompressed_size,
            compressed_size,
            dict_id,
        })
    }

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
