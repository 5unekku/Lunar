//! Lunar Image Format (LIF / `.li`) — a fast, lossless, zstd-compressed
//! internal image format for the Lunar engine.
//!
//! designed for fast decode and direct GPU upload. not a general-purpose
//! image format — source assets are PNG/WebP/BMP, compiled bundles use `.li`.
//!
//! # quick start
//!
//! ```
//! use lunar_image::{encode, decode};
//!
//! let pixels: Vec<u8> = (0..16 * 16 * 4).map(|i| i as u8).collect();
//! let bytes = encode(16, 16, &pixels).unwrap();
//! let image = decode(&bytes).unwrap();
//! assert_eq!(image.width, 16);
//! assert_eq!(image.pixels, pixels);
//! ```
//!
//! # binary format
//!
//! ## file layout
//!
//! ```text
//! +------------------+
//! |     header       |  16 bytes fixed
//! +------------------+
//! |    chunk 0       |  pixel data (required, zstd compressed)
//! +------------------+
//! |    chunk 1       |  metadata (optional)
//! +------------------+
//! |    chunk 2       |  icc profile (optional)
//! +------------------+
//! ```
//!
//! ## header (16 bytes, all multi-byte fields little-endian)
//!
//! ```text
//! offset  size  field    type    description
//! 0       4     magic    [u8;4]  b"LIF\0" = 0x4C 0x49 0x46 0x00
//! 4       2     version  u16     format version (currently 1)
//! 6       2     flags    u16     bit flags (see below)
//! 8       4     width    u32     image width in pixels
//! 12      4     height   u32     image height in pixels
//! ```
//!
//! **flags** (u16 LE):
//! ```text
//! bit 0  has_alpha      image contains alpha channel
//! bit 1  has_metadata   metadata chunk present
//! bit 2  has_icc        icc profile chunk present
//! bit 3  premultiplied  alpha is premultiplied
//! bit 4  planar         pixel data stored as channel planes (RRRR…GGGG…BBBB…AAAA…)
//! bit 5  filtered       each plane row is delta-filtered before zstd (implies planar)
//! 6-15   reserved       must be zero
//! ```
//!
//! ## chunk layout (each chunk has a 16-byte header followed by compressed data)
//!
//! ```text
//! offset  size  field              type    description
//! 0       1     chunk type         u8      0x00=pixel data, 0x01=metadata, 0x02=icc
//! 1       3     reserved           [u8;3]  zero padding
//! 4       4     uncompressed size  u32     size after decompression
//! 8       4     compressed size    u32     size of compressed data in bytes
//! 12      4     zstd dict id       u32     dictionary id (0 = no dictionary)
//! 16      N     compressed data    [u8;N]  zstd frame (N = compressed size)
//! ```
//!
//! pixel data chunk: uncompressed size is exactly `width * height * 4`, row-major
//! RGBA, no stride padding. decoded directly into a `Vec<u8>` ready for GPU upload.
//!
//! ## delta filtering
//!
//! when the `filtered` flag (bit 5) is set, each row of each channel plane is
//! replaced before zstd with the difference from a per-row predictor (PNG-style:
//! None/Sub/Up/Average/Paeth). the filter type is stored as one byte prefixed to
//! each row, so the filtered chunk is `4 * height` bytes larger before compression.
//!
//! filtering turns smooth gradients into long runs of near-zero bytes, stacking
//! on the per-channel coherence from planar layout. measured gains: smooth gradients
//! compress ~90% smaller, photographic content ~35% smaller than planar-only.
//! flat/sparse sprites can come out slightly larger, so the encoder compresses both
//! ways and keeps the smaller — the flag is only set when filtering wins, making it
//! a guaranteed non-regression. decode cost is one extra linear pass, paid only on
//! files that used it.
//!
//! ## memory model
//!
//! decode peak: `compressed_file_size + width * height * 4` (roughly 2× raw pixels).
//! the zstd buffer is freed immediately after decompression; the pixel vec is returned.

mod decode;
mod encode;
mod error;
mod filter;
mod format;
mod simd;

pub use decode::{Image, decode};
pub use encode::{EncodeOptions, encode, encode_with_opts};
pub use error::{DecodeError, EncodeError};
pub use simd::{
	deinterleave_rgba, premultiply_alpha, reinterleave_rgba, rgba_to_bgra, srgb_to_linear,
};
