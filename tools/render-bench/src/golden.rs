//! golden-frame capture and comparison: bgra readback → rgba png, per-channel
//! tolerance diff against a committed reference image.

use std::path::Path;

/// per-channel absolute tolerance absorbing driver-level float variance.
/// an intentional visual change requires user sign-off + a refreshed reference,
/// never a bump of this value.
pub const CHANNEL_TOLERANCE: u8 = 2;

/// the headless target reads back bgra; swap to rgba and force alpha opaque
/// (alpha carries no color information in the composite output).
pub fn bgra_to_rgba_in_place(bytes: &mut [u8]) {
	for px in bytes.chunks_exact_mut(4) {
		px.swap(0, 2);
		px[3] = 0xff;
	}
}

pub fn write_png(path: &Path, rgba: &[u8], width: u32, height: u32) -> Result<(), String> {
	if let Some(parent) = path.parent() {
		std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
	}
	let file = std::fs::File::create(path).map_err(|e| format!("create {}: {e}", path.display()))?;
	let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
	encoder.set_color(png::ColorType::Rgba);
	encoder.set_depth(png::BitDepth::Eight);
	let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
	writer.write_image_data(rgba).map_err(|e| e.to_string())?;
	Ok(())
}

pub fn read_png(path: &Path) -> Result<(Vec<u8>, u32, u32), String> {
	let file = std::fs::File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
	let decoder = png::Decoder::new(std::io::BufReader::new(file));
	let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
	let mut buf = vec![0u8; reader.output_buffer_size().ok_or("png too large")?];
	let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
	if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
		return Err(format!(
			"{}: expected 8-bit rgba, got {:?}/{:?}",
			path.display(),
			info.color_type,
			info.bit_depth
		));
	}
	buf.truncate(info.buffer_size());
	Ok((buf, info.width, info.height))
}

pub struct GoldenDiff {
	pub max_channel_diff: u8,
	/// pixels with any channel differing beyond [`CHANNEL_TOLERANCE`]
	pub differing_pixels: usize,
	pub total_pixels: usize,
}

impl GoldenDiff {
	pub fn passed(&self) -> bool {
		self.differing_pixels == 0
	}
}

/// compare candidate against reference (same dimensions, rgba8). alpha is
/// ignored — both sides force it opaque, it carries no signal.
pub fn compare(reference: &[u8], candidate: &[u8]) -> GoldenDiff {
	assert_eq!(reference.len(), candidate.len(), "golden image size mismatch");
	let mut max_diff = 0u8;
	let mut differing = 0usize;
	for (r, c) in reference.chunks_exact(4).zip(candidate.chunks_exact(4)) {
		let mut over = false;
		for ch in 0..3 {
			let d = r[ch].abs_diff(c[ch]);
			max_diff = max_diff.max(d);
			over |= d > CHANNEL_TOLERANCE;
		}
		differing += over as usize;
	}
	GoldenDiff { max_channel_diff: max_diff, differing_pixels: differing, total_pixels: reference.len() / 4 }
}
