//! Lunar Image Format (MRIF / `.mi`) — a fast, lossless, zstd-compressed
//! internal image format for the Lunar engine.
//!
//! # Quick start
//!
//! ```
//! use engine_image::{encode, decode};
//!
//! let pixels: Vec<u8> = (0..16 * 16 * 4).map(|i| i as u8).collect();
//! let bytes = encode(16, 16, &pixels).unwrap();
//! let image = decode(&bytes).unwrap();
//! assert_eq!(image.width, 16);
//! assert_eq!(image.pixels, pixels);
//! ```

mod decode;
mod encode;
mod error;
mod format;
mod simd;

pub use decode::{Image, decode};
pub use encode::{EncodeOptions, encode, encode_with_opts};
pub use error::{DecodeError, EncodeError};
pub use simd::{SimdLevel, premultiply_alpha_simd, rgba_to_bgra_simd, srgb_to_linear_simd};
