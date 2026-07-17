//! deterministic helpers shared by the bench scenes: seeded rng, procedural
//! textures, f16 conversion for terrain heightmaps.

use lunar::lunar_assets::{AssetServer, Handle, Texture};

/// xorshift64* — tiny, deterministic, good enough for scene placement.
/// never seeded from wall time: every run must build the identical scene.
pub struct Rng(u64);

impl Rng {
	pub fn new(seed: u64) -> Self {
		Self(seed.max(1))
	}

	pub fn next_u64(&mut self) -> u64 {
		let mut x = self.0;
		x ^= x >> 12;
		x ^= x << 25;
		x ^= x >> 27;
		self.0 = x;
		x.wrapping_mul(0x2545_f491_4f6c_dd1d)
	}

	/// uniform in [0, 1)
	pub fn next_f32(&mut self) -> f32 {
		(self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
	}

	pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
		lo + self.next_f32() * (hi - lo)
	}

	pub fn index(&mut self, len: usize) -> usize {
		(self.next_u64() % len as u64) as usize
	}
}

/// ieee 754 binary32 → binary16 bits (round-to-nearest-even), for the terrain
/// heightmap's R16Float texel layout.
pub fn f32_to_f16_bits(value: f32) -> u16 {
	let bits = value.to_bits();
	let sign = ((bits >> 16) & 0x8000) as u16;
	let exponent = ((bits >> 23) & 0xff) as i32;
	let mantissa = bits & 0x007f_ffff;
	if exponent == 0xff {
		// inf/nan
		return sign | 0x7c00 | ((mantissa != 0) as u16 * 0x200);
	}
	let unbiased = exponent - 127 + 15;
	if unbiased >= 0x1f {
		return sign | 0x7c00; // overflow → inf
	}
	if unbiased <= 0 {
		// subnormal or underflow to zero
		if unbiased < -10 {
			return sign;
		}
		let mantissa = mantissa | 0x0080_0000;
		let shift = 14 - unbiased;
		let half = (mantissa >> shift) as u16;
		let round = (mantissa >> (shift - 1)) & 1;
		return sign | (half + round as u16);
	}
	let half = sign | ((unbiased as u16) << 10) | (mantissa >> 13) as u16;
	let round = (mantissa >> 12) & 1;
	half + round as u16
}

/// register raw rgba pixels as a texture that is ready the same frame.
/// wraps [`AssetServer::create_texture`] — no asset files, no decode.
pub fn rgba_texture(
	assets: &mut AssetServer,
	width: u32,
	height: u32,
	rgba: Vec<u8>,
) -> Handle<Texture> {
	assets.create_texture(width, height, rgba)
}

/// checkerboard in two colors — cheap detail that makes uv/bindless
/// sampling visible in golden frames.
pub fn checker_texture(
	assets: &mut AssetServer,
	size: u32,
	cell: u32,
	color_a: [u8; 4],
	color_b: [u8; 4],
) -> Handle<Texture> {
	let mut rgba = Vec::with_capacity((size * size * 4) as usize);
	for y in 0..size {
		for x in 0..size {
			let a_side = ((x / cell) + (y / cell)).is_multiple_of(2);
			rgba.extend_from_slice(if a_side { &color_a } else { &color_b });
		}
	}
	rgba_texture(assets, size, size, rgba)
}

/// value-noise-ish speckle around a base color, seeded — for density maps and
/// material variety without an asset pipeline.
pub fn noise_texture(
	assets: &mut AssetServer,
	size: u32,
	seed: u64,
	base: [u8; 3],
) -> Handle<Texture> {
	let mut rng = Rng::new(seed);
	let mut rgba = Vec::with_capacity((size * size * 4) as usize);
	for _ in 0..size * size {
		let jitter = rng.range(0.6, 1.0);
		rgba.extend_from_slice(&[
			(base[0] as f32 * jitter) as u8,
			(base[1] as f32 * jitter) as u8,
			(base[2] as f32 * jitter) as u8,
			255,
		]);
	}
	rgba_texture(assets, size, size, rgba)
}
