use crate::error::EncodeError;
use crate::filter;
use crate::format::{self, ChunkType};
use crate::simd;

/// options for encoding an image to .li format.
#[derive(Debug, Clone)]
pub struct EncodeOptions {
	/// zstd compression level (1-22).
	/// level 9 sits at the pareto knee — ~80% of max ratio for ~20% of max encode cost.
	/// encoding is a build-time operation so levels above 3 are always worth it.
	pub compression_level: i32,
	/// whether the image contains alpha channel data.
	pub has_alpha: bool,
	/// whether the pixel data has premultiplied alpha.
	pub premultiplied: bool,
	/// optional metadata string (e.g. json) embedded in the file.
	pub metadata: Option<String>,
}

impl Default for EncodeOptions {
	fn default() -> Self {
		Self {
			compression_level: 9,
			has_alpha: true,
			premultiplied: false,
			metadata: None,
		}
	}
}

/// encode RGBA pixels to .li format bytes using default options.
///
/// # Errors
/// returns an error if the pixel buffer size does not match `width * height * 4`.
pub fn encode(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, EncodeError> {
	encode_with_opts(width, height, rgba, &EncodeOptions::default())
}

/// encode RGBA pixels to .li format bytes with custom options.
///
/// pixel data is deinterleaved into channel planes before compression.
/// this gives zstd coherent per-channel statistics and significantly better ratios,
/// especially for sprites with solid or near-solid regions.
///
/// # Errors
/// returns an error if the pixel buffer size does not match `width * height * 4`,
/// or if zstd compression fails.
pub fn encode_with_opts(
	width: u32,
	height: u32,
	rgba: &[u8],
	opts: &EncodeOptions,
) -> Result<Vec<u8>, EncodeError> {
	let expected_bytes = (width as usize) * (height as usize) * 4;
	if rgba.len() != expected_bytes {
		return Err(EncodeError::BufferSizeMismatch {
			expected: expected_bytes,
			actual: rgba.len(),
		});
	}

	// deinterleave then compress — channels separate means zstd sees coherent data
	let planar = simd::deinterleave_rgba(rgba);
	let unfiltered = zstd::encode_all(planar.as_slice(), opts.compression_level)
		.map_err(EncodeError::ZstdError)?;

	// also try a per-row delta filter on the planes and keep whichever compresses
	// smaller, so filtering can never make a file larger. encode is build-time, so
	// the extra zstd pass costs nothing at runtime.
	let (pixel_data, pixel_uncompressed_size, filtered) = if width > 0 && height > 0 {
		let filtered_planar = filter::filter_planes(&planar, width as usize, height as usize, 4);
		let filtered_compressed =
			zstd::encode_all(filtered_planar.as_slice(), opts.compression_level)
				.map_err(EncodeError::ZstdError)?;
		if filtered_compressed.len() < unfiltered.len() {
			(filtered_compressed, filtered_planar.len(), true)
		} else {
			(unfiltered, expected_bytes, false)
		}
	} else {
		(unfiltered, expected_bytes, false)
	};

	let mut flags = format::FLAG_PLANAR;
	if opts.has_alpha {
		flags |= format::FLAG_HAS_ALPHA;
	}
	if opts.premultiplied {
		flags |= format::FLAG_PREMULTIPLIED;
	}
	if opts.metadata.is_some() {
		flags |= format::FLAG_HAS_METADATA;
	}
	if filtered {
		flags |= format::FLAG_FILTERED;
	}

	let mut out = Vec::with_capacity(expected_bytes / 2);
	format::write_header(&mut out, width, height, flags);

	format::ChunkHeader::write(
		&mut out,
		ChunkType::PixelData,
		u32::try_from(pixel_uncompressed_size).unwrap_or(u32::MAX),
		u32::try_from(pixel_data.len()).unwrap_or(u32::MAX),
		0,
	);
	out.extend_from_slice(&pixel_data);

	if let Some(meta) = &opts.metadata {
		let meta_bytes = meta.as_bytes();
		let compressed_meta =
			zstd::encode_all(meta_bytes, opts.compression_level).map_err(EncodeError::ZstdError)?;
		format::ChunkHeader::write(
			&mut out,
			ChunkType::Metadata,
			u32::try_from(meta_bytes.len()).unwrap_or(u32::MAX),
			u32::try_from(compressed_meta.len()).unwrap_or(u32::MAX),
			0,
		);
		out.extend_from_slice(&compressed_meta);
	}

	Ok(out)
}
