use crate::error::DecodeError;
use crate::filter;
use crate::format::{self, ChunkType, Header};
use crate::simd;

/// a decoded image in RGBA format.
///
/// contains the image dimensions and raw pixel data (RGBA8 order).
/// use [`decode`] to load a .li file into this type.
#[derive(Debug, Clone)]
pub struct Image {
    /// image width in pixels.
    pub width: u32,
    /// image height in pixels.
    pub height: u32,
    /// raw pixel data in RGBA order, `width * height * 4` bytes.
    pub pixels: Vec<u8>,
}

impl Image {
    /// create a new blank image filled with transparent black pixels.
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; (width as usize) * (height as usize) * 4],
        }
    }

    /// get the number of pixels in the image.
    #[must_use]
    pub const fn len(&self) -> usize {
        (self.width as usize) * (self.height as usize)
    }

    /// check if the image has zero width or height.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// get the total byte count of the pixel buffer.
    #[must_use]
    pub const fn byte_len(&self) -> usize {
        self.pixels.len()
    }

    /// check if a pixel coordinate is within the image bounds.
    #[must_use]
    pub const fn contains(&self, x: u32, y: u32) -> bool {
        x < self.width && y < self.height
    }

    /// get the RGBA value at a pixel coordinate.
    ///
    /// # Panics
    /// panics if the coordinate is out of bounds. use [`try_get_pixel`](Self::try_get_pixel) to
    /// handle out-of-bounds without panicking.
    #[must_use]
    pub fn get_pixel(&self, x: u32, y: u32) -> [u8; 4] {
        self.try_get_pixel(x, y).expect("pixel coordinate out of bounds")
    }

    /// get the RGBA value at a pixel coordinate, or `None` if out of bounds.
    #[must_use]
    pub fn try_get_pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if !self.contains(x, y) {
            return None;
        }
        let idx = ((y * self.width + x) as usize) * 4;
        Some([
            self.pixels[idx],
            self.pixels[idx + 1],
            self.pixels[idx + 2],
            self.pixels[idx + 3],
        ])
    }

    /// set the RGBA value at a pixel coordinate.
    ///
    /// # Panics
    /// panics if the coordinate is out of bounds. use [`try_set_pixel`](Self::try_set_pixel) to
    /// handle out-of-bounds without panicking.
    pub fn set_pixel(&mut self, x: u32, y: u32, rgba: [u8; 4]) {
        self.try_set_pixel(x, y, rgba).expect("pixel coordinate out of bounds");
    }

    /// set the RGBA value at a pixel coordinate. returns `None` if out of bounds.
    pub fn try_set_pixel(&mut self, x: u32, y: u32, rgba: [u8; 4]) -> Option<()> {
        if !self.contains(x, y) {
            return None;
        }
        let idx = ((y * self.width + x) as usize) * 4;
        self.pixels[idx] = rgba[0];
        self.pixels[idx + 1] = rgba[1];
        self.pixels[idx + 2] = rgba[2];
        self.pixels[idx + 3] = rgba[3];
        Some(())
    }
}

/// decode .mi format bytes to an RGBA image.
///
/// parses the file header and decompresses the pixel data.
/// returns an error if the file is malformed or incomplete.
///
/// # Errors
/// returns an error if the data is not a valid .mi file, if pixel data is missing,
/// or if decompression fails.
pub fn decode(data: &[u8]) -> Result<Image, DecodeError> {
    // Parse header
    let header = Header::parse(data)?;
    let expected_bytes = header.expected_pixel_bytes();

    // Walk chunks starting after header
    let mut offset = format::HEADER_SIZE;
    let mut pixels: Option<Vec<u8>> = None;

    while offset < data.len() {
        let chunk_header = format::ChunkHeader::parse(&data[offset..])?;
        offset += format::CHUNK_HEADER_SIZE;

        let chunk_data_end = offset
            .checked_add(chunk_header.compressed_size as usize)
            .filter(|&end| end <= data.len())
            .ok_or(DecodeError::TruncatedChunk)?;
        let compressed = &data[offset..chunk_data_end];
        offset = chunk_data_end;

        match chunk_header.chunk_type {
            ChunkType::PixelData => {
                if pixels.is_some() {
                    return Err(DecodeError::MultiplePixelData);
                }
                let decompressed = zstd::decode_all(std::io::Cursor::new(compressed))
                    .map_err(DecodeError::ZstdError)?;

                // undo the per-row delta filter first, if present, recovering the raw
                // planar buffer. filtered data carries one extra byte per plane row.
                let planar = if header.flags & format::FLAG_FILTERED != 0 {
                    let width = header.width as usize;
                    let height = header.height as usize;
                    let expected_filtered = expected_bytes + filter::overhead_bytes(height, 4);
                    if decompressed.len() != expected_filtered {
                        return Err(DecodeError::SizeMismatch {
                            expected: expected_filtered,
                            actual: decompressed.len(),
                        });
                    }
                    filter::unfilter_planes(&decompressed, width, height, 4).ok_or(
                        DecodeError::SizeMismatch {
                            expected: expected_bytes,
                            actual: decompressed.len(),
                        },
                    )?
                } else {
                    if decompressed.len() != expected_bytes {
                        return Err(DecodeError::SizeMismatch {
                            expected: expected_bytes,
                            actual: decompressed.len(),
                        });
                    }
                    decompressed
                };

                // reinterleave if planar flag is set (all files encoded since v1.1)
                let rgba = if header.flags & format::FLAG_PLANAR != 0 {
                    let n_pixels = header.width as usize * header.height as usize;
                    simd::reinterleave_rgba(&planar, n_pixels)
                } else {
                    planar
                };
                pixels = Some(rgba);
            }
            ChunkType::Metadata | ChunkType::IccProfile => {
                // not yet exposed through the public API; skip without decompressing
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
