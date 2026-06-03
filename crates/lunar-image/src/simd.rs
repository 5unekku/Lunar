//! pixel processing utilities with SIMD acceleration where available.
//!
//! deinterleave/reinterleave are the hot path — they run on every encode/decode.
//! NEON uses vld4q_u8/vst4q_u8 which deinterleave 64 bytes in a single instruction.
//! x86 falls back to scalar; LLVM auto-vectorises the inner loop well on AVX2 targets.

/// deinterleave RGBA → `[R...][G...][B...][A...]` contiguous planes.
///
/// zstd compresses each channel plane far better than interleaved data because
/// each plane has coherent statistics. a solid-color sprite's alpha plane is
/// all-255, a 4-byte run becomes a ~5-byte output from zstd — not possible
/// when the 255s are scattered every 4 bytes through interleaved data.
pub fn deinterleave_rgba(rgba: &[u8]) -> Vec<u8> {
	assert_eq!(rgba.len() % 4, 0, "rgba length must be a multiple of 4");
	let n = rgba.len() / 4;
	let mut out = vec![0u8; rgba.len()];

	#[cfg(target_arch = "aarch64")]
	// SAFETY: NEON is part of the mandatory AArch64 baseline, so the
	// `target_feature(enable = "neon")` precondition always holds on this target
	// (no runtime detection needed). `out` is allocated to `rgba.len()` bytes and
	// `n == rgba.len() / 4`, satisfying `deinterleave_neon`'s buffer-size contract.
	unsafe {
		deinterleave_neon(rgba, &mut out, n);
		return out;
	}

	#[cfg_attr(target_arch = "aarch64", allow(unreachable_code))]
	{
		#[cfg(not(target_arch = "wasm32"))]
		deinterleave_parallel(rgba, &mut out, n);
		#[cfg(target_arch = "wasm32")]
		deinterleave_scalar(rgba, &mut out, n);
		out
	}
}

/// reinterleave `[R...][G...][B...][A...]` planes back to RGBA.
pub fn reinterleave_rgba(planar: &[u8], n_pixels: usize) -> Vec<u8> {
	assert_eq!(
		planar.len(),
		n_pixels * 4,
		"planar length must be n_pixels * 4"
	);
	let mut out = vec![0u8; planar.len()];

	#[cfg(target_arch = "aarch64")]
	// SAFETY: NEON is part of the mandatory AArch64 baseline, so the
	// `target_feature(enable = "neon")` precondition always holds on this target.
	// `out` and `planar` are both `n_pixels * 4` bytes (the assert above guarantees
	// `planar.len()`), satisfying `reinterleave_neon`'s buffer-size contract.
	unsafe {
		reinterleave_neon(planar, &mut out, n_pixels);
		return out;
	}

	#[cfg_attr(target_arch = "aarch64", allow(unreachable_code))]
	{
		#[cfg(not(target_arch = "wasm32"))]
		reinterleave_parallel(planar, &mut out, n_pixels);
		#[cfg(target_arch = "wasm32")]
		reinterleave_scalar(planar, &mut out, n_pixels);
		out
	}
}

#[cfg(not(target_arch = "wasm32"))]
fn deinterleave_parallel(rgba: &[u8], out: &mut [u8], n: usize) {
	use rayon::prelude::*;
	let (r, rest) = out.split_at_mut(n);
	let (g, rest) = rest.split_at_mut(n);
	let (b, a) = rest.split_at_mut(n);
	r.par_iter_mut()
		.zip(g.par_iter_mut())
		.zip(b.par_iter_mut())
		.zip(a.par_iter_mut())
		.zip(rgba.par_chunks_exact(4))
		.for_each(|((((rv, gv), bv), av), px)| {
			*rv = px[0];
			*gv = px[1];
			*bv = px[2];
			*av = px[3];
		});
}

#[cfg(not(target_arch = "wasm32"))]
fn reinterleave_parallel(planar: &[u8], out: &mut [u8], n: usize) {
	use rayon::prelude::*;
	let (r, rest) = planar.split_at(n);
	let (g, rest) = rest.split_at(n);
	let (b, a) = rest.split_at(n);
	r.par_iter()
		.zip(g.par_iter())
		.zip(b.par_iter())
		.zip(a.par_iter())
		.zip(out.par_chunks_exact_mut(4))
		.for_each(|((((rv, gv), bv), av), px)| {
			px[0] = *rv;
			px[1] = *gv;
			px[2] = *bv;
			px[3] = *av;
		});
}

#[cfg(target_arch = "wasm32")]
fn deinterleave_scalar(rgba: &[u8], out: &mut [u8], n: usize) {
	let (r, rest) = out.split_at_mut(n);
	let (g, rest) = rest.split_at_mut(n);
	let (b, a) = rest.split_at_mut(n);
	for (i, px) in rgba.chunks_exact(4).enumerate() {
		r[i] = px[0];
		g[i] = px[1];
		b[i] = px[2];
		a[i] = px[3];
	}
}

#[cfg(target_arch = "wasm32")]
fn reinterleave_scalar(planar: &[u8], out: &mut [u8], n: usize) {
	let (r, rest) = planar.split_at(n);
	let (g, rest) = rest.split_at(n);
	let (b, a) = rest.split_at(n);
	for (i, px) in out.chunks_exact_mut(4).enumerate() {
		px[0] = r[i];
		px[1] = g[i];
		px[2] = b[i];
		px[3] = a[i];
	}
}

/// neon deinterleave using vld4q_u8 — loads 64 bytes and splits into 4 x 16-byte
/// channel registers in a single instruction.
///
/// # Safety
/// caller must ensure `rgba.len() >= n * 4` and `out.len() >= n * 4`; the four
/// output planes are written at offsets `0, n, 2n, 3n`. must run on an aarch64
/// target with NEON (always true: NEON is baseline on AArch64).
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn deinterleave_neon(rgba: &[u8], out: &mut [u8], n: usize) {
	use std::arch::aarch64::*;
	let n_blocks = n / 16;
	let remainder = n % 16;
	let src = rgba.as_ptr();
	let r_ptr = out.as_mut_ptr();
	let g_ptr = r_ptr.add(n);
	let b_ptr = g_ptr.add(n);
	let a_ptr = b_ptr.add(n);
	for i in 0..n_blocks {
		let quad = vld4q_u8(src.add(i * 64));
		vst1q_u8(r_ptr.add(i * 16), quad.0);
		vst1q_u8(g_ptr.add(i * 16), quad.1);
		vst1q_u8(b_ptr.add(i * 16), quad.2);
		vst1q_u8(a_ptr.add(i * 16), quad.3);
	}
	// scalar tail
	let done = n_blocks * 16;
	for j in 0..remainder {
		let px = src.add((done + j) * 4);
		*r_ptr.add(done + j) = *px;
		*g_ptr.add(done + j) = *px.add(1);
		*b_ptr.add(done + j) = *px.add(2);
		*a_ptr.add(done + j) = *px.add(3);
	}
}

/// neon reinterleave using vst4q_u8 — interleaves 4 x 16-byte channel registers
/// into 64 bytes of RGBA in a single instruction.
///
/// # Safety
/// caller must ensure `planar.len() >= n * 4` (four input planes at offsets
/// `0, n, 2n, 3n`) and `out.len() >= n * 4`. must run on an aarch64 target with
/// NEON (always true: NEON is baseline on AArch64).
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn reinterleave_neon(planar: &[u8], out: &mut [u8], n: usize) {
	use std::arch::aarch64::*;
	let n_blocks = n / 16;
	let remainder = n % 16;
	let r_ptr = planar.as_ptr();
	let g_ptr = r_ptr.add(n);
	let b_ptr = g_ptr.add(n);
	let a_ptr = b_ptr.add(n);
	let dst = out.as_mut_ptr();
	for i in 0..n_blocks {
		let quad = uint8x16x4_t(
			vld1q_u8(r_ptr.add(i * 16)),
			vld1q_u8(g_ptr.add(i * 16)),
			vld1q_u8(b_ptr.add(i * 16)),
			vld1q_u8(a_ptr.add(i * 16)),
		);
		vst4q_u8(dst.add(i * 64), quad);
	}
	// scalar tail
	let done = n_blocks * 16;
	for j in 0..remainder {
		let base = (done + j) * 4;
		out[base] = *r_ptr.add(done + j);
		out[base + 1] = *g_ptr.add(done + j);
		out[base + 2] = *b_ptr.add(done + j);
		out[base + 3] = *a_ptr.add(done + j);
	}
}

/// premultiply RGB channels by the alpha value in-place.
///
/// # Panics
/// panics if the rgba buffer length is not a multiple of 4.
pub fn premultiply_alpha(rgba: &mut [u8]) {
	assert_eq!(
		rgba.len() % 4,
		0,
		"rgba buffer must be a multiple of 4 bytes"
	);
	for chunk in rgba.chunks_exact_mut(4) {
		let a = chunk[3] as u32;
		chunk[0] = ((chunk[0] as u32 * a + 127) / 255) as u8;
		chunk[1] = ((chunk[1] as u32 * a + 127) / 255) as u8;
		chunk[2] = ((chunk[2] as u32 * a + 127) / 255) as u8;
	}
}

/// convert RGBA to BGRA by swapping red and blue channels in-place.
///
/// # Panics
/// panics if the buffer length is not a multiple of 4.
pub fn rgba_to_bgra(buf: &mut [u8]) {
	assert_eq!(buf.len() % 4, 0, "buffer must be a multiple of 4 bytes");
	for chunk in buf.chunks_exact_mut(4) {
		chunk.swap(0, 2);
	}
}

/// convert sRGB byte values to linear f32. output must be the same length as input.
///
/// uses a 256-entry precomputed LUT; first call initialises it, subsequent calls are free.
///
/// # Panics
/// panics if output length does not equal input length.
pub fn srgb_to_linear(input: &[u8], output: &mut [f32]) {
	assert_eq!(
		input.len(),
		output.len(),
		"output must be same length as input"
	);
	let lut = srgb_lut();
	for (out, &byte) in output.iter_mut().zip(input.iter()) {
		*out = lut[byte as usize];
	}
}

fn srgb_lut() -> &'static [f32; 256] {
	use std::sync::OnceLock;
	static LUT: OnceLock<[f32; 256]> = OnceLock::new();
	LUT.get_or_init(|| {
		std::array::from_fn(|i| {
			let s = i as f32 / 255.0;
			if s <= 0.04045 {
				s / 12.92
			} else {
				((s + 0.055) / 1.055).powf(2.4)
			}
		})
	})
}
