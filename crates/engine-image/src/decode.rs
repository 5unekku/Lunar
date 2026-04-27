use crate::error::DecodeError;
use crate::format::{self, ChunkType, Header};

/// A decoded image in RGBA format.
#[derive(Debug, Clone)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Image {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; (width as usize) * (height as usize) * 4],
        }
    }

    pub fn len(&self) -> usize {
        (self.width as usize) * (self.height as usize)
    }

    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    pub fn byte_len(&self) -> usize {
        self.pixels.len()
    }

    pub fn contains(&self, x: u32, y: u32) -> bool {
        x < self.width && y < self.height
    }

    pub fn get_pixel(&self, x: u32, y: u32) -> [u8; 4] {
        assert!(
            self.contains(x, y),
            "pixel coordinate ({}, {}) out of bounds",
            x,
            y
        );
        let idx = ((y * self.width + x) as usize) * 4;
        [
            self.pixels[idx],
            self.pixels[idx + 1],
            self.pixels[idx + 2],
            self.pixels[idx + 3],
        ]
    }

    pub fn set_pixel(&mut self, x: u32, y: u32, rgba: [u8; 4]) {
        assert!(
            self.contains(x, y),
            "pixel coordinate ({}, {}) out of bounds",
            x,
            y
        );
        let idx = ((y * self.width + x) as usize) * 4;
        self.pixels[idx] = rgba[0];
        self.pixels[idx + 1] = rgba[1];
        self.pixels[idx + 2] = rgba[2];
        self.pixels[idx + 3] = rgba[3];
    }
}

/// Decode .mi format bytes to an RGBA image.
pub fn decode(data: &[u8]) -> Result<Image, DecodeError> {
    // Parse header
    let header = Header::parse(data)?;
    let expected_bytes = header.expected_pixel_bytes();

    // Walk chunks starting after header
    let mut offset = format::HEADER_SIZE;
    let mut pixels: Option<Vec<u8>> = None;
    let mut _metadata: Option<String> = None;
    let mut _icc: Option<Vec<u8>> = None;

    while offset < data.len() {
        let chunk_header = format::ChunkHeader::parse(&data[offset..])?;
        offset += format::CHUNK_HEADER_SIZE;

        let chunk_data_end = offset + (chunk_header.compressed_size as usize);
        if chunk_data_end > data.len() {
            return Err(DecodeError::TruncatedChunk);
        }
        let compressed = &data[offset..chunk_data_end];
        offset = chunk_data_end;

        match chunk_header.chunk_type {
            ChunkType::PixelData => {
                if pixels.is_some() {
                    return Err(DecodeError::MultiplePixelData);
                }
                let mut decompressed = vec![0u8; chunk_header.uncompressed_size as usize];
                zstd::decode_all(std::io::Cursor::new(compressed))
                    .map_err(DecodeError::ZstdError)
                    .map(|src| {
                        decompressed.copy_from_slice(&src);
                    })?;
                if decompressed.len() != expected_bytes {
                    return Err(DecodeError::SizeMismatch {
                        expected: expected_bytes,
                        actual: decompressed.len(),
                    });
                }
                pixels = Some(decompressed);
            }
            ChunkType::Metadata => {
                let decompressed = zstd::decode_all(std::io::Cursor::new(compressed))
                    .map_err(DecodeError::ZstdError)?;
                _metadata = Some(String::from_utf8_lossy(&decompressed).into_owned());
            }
            ChunkType::IccProfile => {
                let decompressed = zstd::decode_all(std::io::Cursor::new(compressed))
                    .map_err(DecodeError::ZstdError)?;
                _icc = Some(decompressed);
            }
        }
    }

    let pixels = pixels.ok_or(DecodeError::MissingPixelData)?;

    Ok(Image {
        width: header.width,
        height: header.height,
        pixels,
    })
}
