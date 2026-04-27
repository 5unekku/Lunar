use crate::error::EncodeError;
use crate::format::{self, ChunkType};

/// Encode options
#[derive(Debug, Clone)]
pub struct EncodeOptions {
    pub compression_level: i32,
    pub has_alpha: bool,
    pub premultiplied: bool,
    pub metadata: Option<String>,
}

impl Default for EncodeOptions {
    fn default() -> Self {
        Self {
            compression_level: 3,
            has_alpha: true,
            premultiplied: false,
            metadata: None,
        }
    }
}

/// Encode RGBA pixels to .mi format bytes.
pub fn encode(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, EncodeError> {
    encode_with_opts(width, height, rgba, EncodeOptions::default())
}

/// Encode with options.
pub fn encode_with_opts(
    width: u32,
    height: u32,
    rgba: &[u8],
    opts: EncodeOptions,
) -> Result<Vec<u8>, EncodeError> {
    let expected_bytes = (width as usize) * (height as usize) * 4;
    if rgba.len() != expected_bytes {
        return Err(EncodeError::BufferSizeMismatch {
            expected: expected_bytes,
            actual: rgba.len(),
        });
    }

    let mut out = Vec::with_capacity(expected_bytes / 2);

    // Build flags
    let mut flags = 0u16;
    if opts.has_alpha {
        flags |= format::FLAG_HAS_ALPHA;
    }
    if opts.premultiplied {
        flags |= format::FLAG_PREMULTIPLIED;
    }
    let has_metadata = opts.metadata.is_some();
    if has_metadata {
        flags |= format::FLAG_HAS_METADATA;
    }

    // Write header
    format::write_header(&mut out, width, height, flags);

    // Compress pixel data
    let compressed =
        zstd::encode_all(rgba, opts.compression_level).map_err(EncodeError::ZstdError)?;

    // Write pixel data chunk
    format::ChunkHeader::write(
        &mut out,
        ChunkType::PixelData,
        expected_bytes as u32,
        compressed.len() as u32,
        0, // no dictionary
    );
    out.extend_from_slice(&compressed);

    // Write metadata chunk if present
    if let Some(ref meta) = opts.metadata {
        let meta_bytes = meta.as_bytes();
        let compressed_meta =
            zstd::encode_all(meta_bytes, opts.compression_level).map_err(EncodeError::ZstdError)?;
        format::ChunkHeader::write(
            &mut out,
            ChunkType::Metadata,
            meta_bytes.len() as u32,
            compressed_meta.len() as u32,
            0,
        );
        out.extend_from_slice(&compressed_meta);
    }

    Ok(out)
}
