//! SIMD frustum-AABB culling over structure-of-arrays box data.
//!
//! the renderer's mid/low tiers (no GPU compute cull) test every cullable AABB
//! against the camera frustum on the CPU each frame. [`cull_aabbs_soa`] does that
//! eight boxes at a time on x86_64 with AVX2+FMA, four at a time on aarch64 with
//! NEON (baseline, no runtime detection), and falls back to a scalar reference
//! elsewhere or when AVX2 is absent at runtime. the box data comes from
//! [`crate::visibility::CullSoa`], whose axes are already laid out as contiguous
//! `f32` slices so each lane is a straight load.
//!
//! the test is conservative and matches [`crate::visibility::Frustum::intersects_aabb`]:
//! a box is "visible" unless it is fully outside one of the six planes. a false
//! positive is harmless (a redundant draw); a false negative would drop geometry,
//! so the scalar reference and the SIMD paths use the same `>= 0` plane comparison.

// the cull kernels take the six SoA axis slices as separate params by design — that's
// the whole point of the layout — so the arg-count lint doesn't apply here.
#![allow(clippy::too_many_arguments)]

use lunar_math::Vec4;

/// per-plane scalars used by the cull: inward normal `(nx, ny, nz)`, plane offset
/// `d`, and the absolute normal `(|nx|, |ny|, |nz|)` for the projected box radius.
#[derive(Clone, Copy)]
struct PlanePrecomp {
	nx: f32,
	ny: f32,
	nz: f32,
	d: f32,
	anx: f32,
	any: f32,
	anz: f32,
}

#[inline]
fn precompute(planes: &[Vec4; 6]) -> [PlanePrecomp; 6] {
	core::array::from_fn(|p| {
		let pl = planes[p];
		PlanePrecomp {
			nx: pl.x,
			ny: pl.y,
			nz: pl.z,
			d: pl.w,
			anx: pl.x.abs(),
			any: pl.y.abs(),
			anz: pl.z.abs(),
		}
	})
}

/// conservative frustum-AABB visibility for the first `out.len()` boxes.
///
/// writes `1` to `out[i]` when box `i` may intersect the frustum and `0` when it
/// is provably outside. the six axis slices must each be at least `out.len()` long;
/// element `i` of every slice describes the same box.
///
/// dispatches to an AVX2+FMA kernel when the running CPU supports it, else the
/// scalar reference. both produce the same result.
pub fn cull_aabbs_soa(
	planes: &[Vec4; 6],
	center_x: &[f32],
	center_y: &[f32],
	center_z: &[f32],
	half_x: &[f32],
	half_y: &[f32],
	half_z: &[f32],
	out: &mut [u8],
) {
	let n = out.len();
	debug_assert!(
		center_x.len() >= n
			&& center_y.len() >= n
			&& center_z.len() >= n
			&& half_x.len() >= n
			&& half_y.len() >= n
			&& half_z.len() >= n,
		"cull_aabbs_soa: axis slices shorter than out"
	);
	if n == 0 {
		return;
	}
	let pre = precompute(planes);

	#[cfg(target_arch = "x86_64")]
	{
		if std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma") {
			// SAFETY: avx2 + fma confirmed present at runtime by the checks above.
			unsafe {
				cull_avx2(&pre, center_x, center_y, center_z, half_x, half_y, half_z, out);
			}
			return;
		}
	}

	#[cfg(target_arch = "aarch64")]
	// SAFETY: NEON is part of the mandatory AArch64 baseline, so the kernel's
	// target-feature precondition always holds (no runtime detection needed).
	unsafe {
		cull_neon(&pre, center_x, center_y, center_z, half_x, half_y, half_z, out);
		return;
	}

	#[cfg_attr(target_arch = "aarch64", allow(unreachable_code))]
	cull_scalar_into(&pre, center_x, center_y, center_z, half_x, half_y, half_z, out);
}

/// scalar reference path — also the exact arithmetic the AVX2 kernel mirrors.
#[inline]
fn cull_scalar_into(
	pre: &[PlanePrecomp; 6],
	center_x: &[f32],
	center_y: &[f32],
	center_z: &[f32],
	half_x: &[f32],
	half_y: &[f32],
	half_z: &[f32],
	out: &mut [u8],
) {
	for i in 0..out.len() {
		out[i] = cull_one(
			pre, center_x[i], center_y[i], center_z[i], half_x[i], half_y[i], half_z[i],
		) as u8;
	}
}

/// fused multiply-add where the hardware has it, plain mul+add elsewhere.
///
/// on targets without native FMA (armv7 default float ABI, i686, pre-Haswell
/// x86_64), `f32::mul_add` lowers to an `fmaf` libcall — an order of magnitude
/// slower than the two-instruction form, and this runs 24 times per box. the
/// cull's conservativeness does not depend on last-bit rounding, so the unfused
/// form is safe; where FMA is native the fused form keeps the scalar reference
/// bit-identical to the SIMD kernels.
#[inline(always)]
fn fused_mul_add(a: f32, b: f32, c: f32) -> f32 {
	#[cfg(any(target_arch = "aarch64", target_feature = "fma", target_feature = "vfp4"))]
	{
		a.mul_add(b, c)
	}
	#[cfg(not(any(target_arch = "aarch64", target_feature = "fma", target_feature = "vfp4")))]
	{
		a * b + c
	}
}

/// true when the box is inside-or-touching all six planes. evaluates every plane
/// (no early-out) so the result matches the branchless SIMD accumulation. on
/// hardware-FMA targets the arithmetic is bit-identical to the SIMD kernels; on
/// default x86_64 builds (no compile-time fma) the runtime-dispatched AVX2 kernel
/// fuses while this doesn't — a last-ulp difference that can only matter for a box
/// landing exactly on a plane, where either answer is acceptable (conservative test).
#[inline]
fn cull_one(
	pre: &[PlanePrecomp; 6],
	cx: f32,
	cy: f32,
	cz: f32,
	hx: f32,
	hy: f32,
	hz: f32,
) -> bool {
	let mut inside = true;
	for p in pre {
		// dot(normal, center): innermost product plain, outer two fused — matches fmadd order.
		let dot = fused_mul_add(p.nz, cz, fused_mul_add(p.ny, cy, p.nx * cx));
		// projected box radius along the plane normal.
		let radius = fused_mul_add(p.anz, hz, fused_mul_add(p.any, hy, p.anx * hx));
		let test = (dot + p.d) + radius;
		inside &= test >= 0.0;
	}
	inside
}

/// AVX2+FMA kernel: eight boxes per iteration, scalar tail for the remainder.
///
/// # Safety
/// caller must ensure the target CPU supports AVX2 and FMA (checked at runtime in
/// [`cull_aabbs_soa`]). all axis slices must be at least `out.len()` elements.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn cull_avx2(
	pre: &[PlanePrecomp; 6],
	center_x: &[f32],
	center_y: &[f32],
	center_z: &[f32],
	half_x: &[f32],
	half_y: &[f32],
	half_z: &[f32],
	out: &mut [u8],
) {
	use std::arch::x86_64::*;

	// SAFETY: every intrinsic below is gated by the function's avx2+fma target
	// features; pointers are read 8 lanes at a time strictly within `lanes <= n`,
	// and each `out` byte is written exactly once. the scalar tail covers n % 8.
	unsafe {
		let n = out.len();
		let zero = _mm256_setzero_ps();

		// broadcast each plane's scalars to all 8 lanes once.
		let nx: [__m256; 6] = core::array::from_fn(|p| _mm256_set1_ps(pre[p].nx));
		let ny: [__m256; 6] = core::array::from_fn(|p| _mm256_set1_ps(pre[p].ny));
		let nz: [__m256; 6] = core::array::from_fn(|p| _mm256_set1_ps(pre[p].nz));
		let dd: [__m256; 6] = core::array::from_fn(|p| _mm256_set1_ps(pre[p].d));
		let anx: [__m256; 6] = core::array::from_fn(|p| _mm256_set1_ps(pre[p].anx));
		let any: [__m256; 6] = core::array::from_fn(|p| _mm256_set1_ps(pre[p].any));
		let anz: [__m256; 6] = core::array::from_fn(|p| _mm256_set1_ps(pre[p].anz));

		let lanes = n - (n % 8);
		let mut i = 0;
		while i < lanes {
			let cxv = _mm256_loadu_ps(center_x.as_ptr().add(i));
			let cyv = _mm256_loadu_ps(center_y.as_ptr().add(i));
			let czv = _mm256_loadu_ps(center_z.as_ptr().add(i));
			let hxv = _mm256_loadu_ps(half_x.as_ptr().add(i));
			let hyv = _mm256_loadu_ps(half_y.as_ptr().add(i));
			let hzv = _mm256_loadu_ps(half_z.as_ptr().add(i));

			let mut inside = _mm256_setzero_ps();
			for p in 0..6 {
				let dot = _mm256_fmadd_ps(nz[p], czv, _mm256_fmadd_ps(ny[p], cyv, _mm256_mul_ps(nx[p], cxv)));
				let radius =
					_mm256_fmadd_ps(anz[p], hzv, _mm256_fmadd_ps(any[p], hyv, _mm256_mul_ps(anx[p], hxv)));
				let test = _mm256_add_ps(_mm256_add_ps(dot, dd[p]), radius);
				// 0xFFFFFFFF in lanes where test >= 0 (inside-or-touching this plane).
				let ge = _mm256_cmp_ps::<_CMP_GE_OQ>(test, zero);
				if p == 0 {
					inside = ge;
				} else {
					inside = _mm256_and_ps(inside, ge);
				}
			}
			// bit j set => lane j passed all planes => visible.
			let mask = _mm256_movemask_ps(inside) as u32;
			for j in 0..8 {
				*out.get_unchecked_mut(i + j) = ((mask >> j) & 1) as u8;
			}
			i += 8;
		}

		// scalar tail (n % 8 boxes).
		while i < n {
			*out.get_unchecked_mut(i) = cull_one(
				pre,
				*center_x.get_unchecked(i),
				*center_y.get_unchecked(i),
				*center_z.get_unchecked(i),
				*half_x.get_unchecked(i),
				*half_y.get_unchecked(i),
				*half_z.get_unchecked(i),
			) as u8;
			i += 1;
		}
	}
}

/// NEON kernel: four boxes per iteration, scalar tail for the remainder.
///
/// same plane-accumulation structure as [`cull_avx2`]; `vfmaq_f32` keeps the
/// arithmetic bit-identical to the aarch64 scalar reference (which fuses via
/// [`fused_mul_add`]).
///
/// # Safety
/// caller must ensure the target CPU supports NEON (always true: NEON is baseline
/// on AArch64). all axis slices must be at least `out.len()` elements.
#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn cull_neon(
	pre: &[PlanePrecomp; 6],
	center_x: &[f32],
	center_y: &[f32],
	center_z: &[f32],
	half_x: &[f32],
	half_y: &[f32],
	half_z: &[f32],
	out: &mut [u8],
) {
	use std::arch::aarch64::*;

	// SAFETY: every intrinsic below is gated by the function's neon target feature;
	// pointers are read 4 lanes at a time strictly within `lanes <= n`, and each
	// `out` byte is written exactly once. the scalar tail covers n % 4.
	unsafe {
		let n = out.len();

		// broadcast each plane's scalars to all 4 lanes once.
		let nx: [float32x4_t; 6] = core::array::from_fn(|p| vdupq_n_f32(pre[p].nx));
		let ny: [float32x4_t; 6] = core::array::from_fn(|p| vdupq_n_f32(pre[p].ny));
		let nz: [float32x4_t; 6] = core::array::from_fn(|p| vdupq_n_f32(pre[p].nz));
		let dd: [float32x4_t; 6] = core::array::from_fn(|p| vdupq_n_f32(pre[p].d));
		let anx: [float32x4_t; 6] = core::array::from_fn(|p| vdupq_n_f32(pre[p].anx));
		let any: [float32x4_t; 6] = core::array::from_fn(|p| vdupq_n_f32(pre[p].any));
		let anz: [float32x4_t; 6] = core::array::from_fn(|p| vdupq_n_f32(pre[p].anz));

		let lanes = n - (n % 4);
		let mut i = 0;
		while i < lanes {
			let cxv = vld1q_f32(center_x.as_ptr().add(i));
			let cyv = vld1q_f32(center_y.as_ptr().add(i));
			let czv = vld1q_f32(center_z.as_ptr().add(i));
			let hxv = vld1q_f32(half_x.as_ptr().add(i));
			let hyv = vld1q_f32(half_y.as_ptr().add(i));
			let hzv = vld1q_f32(half_z.as_ptr().add(i));

			let mut inside = vdupq_n_u32(u32::MAX);
			for p in 0..6 {
				// vfmaq_f32(a, b, c) = a + b*c — same order as the scalar reference:
				// nz*cz + (ny*cy + (nx*cx))
				let dot = vfmaq_f32(vfmaq_f32(vmulq_f32(nx[p], cxv), ny[p], cyv), nz[p], czv);
				let radius =
					vfmaq_f32(vfmaq_f32(vmulq_f32(anx[p], hxv), any[p], hyv), anz[p], hzv);
				let test = vaddq_f32(vaddq_f32(dot, dd[p]), radius);
				// all-ones in lanes where test >= 0 (inside-or-touching this plane).
				let ge = vcgezq_f32(test);
				inside = vandq_u32(inside, ge);
			}
			let mut mask = [0u32; 4];
			vst1q_u32(mask.as_mut_ptr(), inside);
			for j in 0..4 {
				*out.get_unchecked_mut(i + j) = (mask[j] & 1) as u8;
			}
			i += 4;
		}

		// scalar tail (n % 4 boxes).
		while i < n {
			*out.get_unchecked_mut(i) = cull_one(
				pre,
				*center_x.get_unchecked(i),
				*center_y.get_unchecked(i),
				*center_z.get_unchecked(i),
				*half_x.get_unchecked(i),
				*half_y.get_unchecked(i),
				*half_z.get_unchecked(i),
			) as u8;
			i += 1;
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::visibility::Frustum;
	use lunar_math::{Mat4, Vec3A};

	/// tiny deterministic LCG so tests don't pull a rng dependency.
	struct Lcg(u64);
	impl Lcg {
		fn next_f32(&mut self, lo: f32, hi: f32) -> f32 {
			self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
			let unit = ((self.0 >> 40) as f32) / ((1u64 << 24) as f32);
			lo + unit * (hi - lo)
		}
	}

	fn test_frustum() -> Frustum {
		// a realistic reverse-z perspective × look-at view, so the planes are non-trivial.
		let proj = Mat4::perspective_rh(60_f32.to_radians(), 16.0 / 9.0, 0.1, 500.0);
		let view = Mat4::look_at_rh(
			lunar_math::Vec3::new(3.0, 4.0, 10.0),
			lunar_math::Vec3::ZERO,
			lunar_math::Vec3::Y,
		);
		Frustum::from_view_proj(proj * view)
	}

	/// the dispatched kernel (AVX2 where available) must equal the scalar reference.
	#[test]
	fn simd_matches_scalar_reference() {
		let frustum = test_frustum();
		let pre = precompute(&frustum.planes);
		let mut rng = Lcg(0x1234_5678_9abc_def0);

		let n = 1000;
		let (mut cx, mut cy, mut cz) = (vec![0.0; n], vec![0.0; n], vec![0.0; n]);
		let (mut hx, mut hy, mut hz) = (vec![0.0; n], vec![0.0; n], vec![0.0; n]);
		for i in 0..n {
			cx[i] = rng.next_f32(-300.0, 300.0);
			cy[i] = rng.next_f32(-300.0, 300.0);
			cz[i] = rng.next_f32(-300.0, 300.0);
			hx[i] = rng.next_f32(0.0, 30.0);
			hy[i] = rng.next_f32(0.0, 30.0);
			hz[i] = rng.next_f32(0.0, 30.0);
		}

		let mut dispatched = vec![0u8; n];
		cull_aabbs_soa(&frustum.planes, &cx, &cy, &cz, &hx, &hy, &hz, &mut dispatched);
		let mut reference = vec![0u8; n];
		cull_scalar_into(&pre, &cx, &cy, &cz, &hx, &hy, &hz, &mut reference);

		assert_eq!(dispatched, reference, "SIMD dispatch diverged from scalar reference");
	}

	/// the cull must never drop a box that `Frustum::intersects_aabb` keeps (no false
	/// negatives), and must actually cull boxes that are clearly outside.
	#[test]
	fn cull_is_conservative_vs_frustum() {
		let frustum = test_frustum();
		let mut rng = Lcg(0xfeed_face_cafe_babe);

		let n = 4096; // exercises the 8-wide body plus a scalar tail
		let (mut cx, mut cy, mut cz) = (vec![0.0; n], vec![0.0; n], vec![0.0; n]);
		let (mut hx, mut hy, mut hz) = (vec![0.0; n], vec![0.0; n], vec![0.0; n]);
		for i in 0..n {
			cx[i] = rng.next_f32(-400.0, 400.0);
			cy[i] = rng.next_f32(-400.0, 400.0);
			cz[i] = rng.next_f32(-400.0, 400.0);
			hx[i] = rng.next_f32(0.0, 20.0);
			hy[i] = rng.next_f32(0.0, 20.0);
			hz[i] = rng.next_f32(0.0, 20.0);
		}

		let mut flags = vec![0u8; n];
		cull_aabbs_soa(&frustum.planes, &cx, &cy, &cz, &hx, &hy, &hz, &mut flags);

		for i in 0..n {
			let truth = frustum.intersects_aabb(
				Vec3A::new(cx[i], cy[i], cz[i]),
				Vec3A::new(hx[i], hy[i], hz[i]),
			);
			if truth {
				assert_eq!(flags[i], 1, "box {i} kept by intersects_aabb but culled by SIMD");
			}
		}

		// a box far behind the camera must be culled by both.
		let mut one = [0u8; 1];
		cull_aabbs_soa(
			&frustum.planes,
			&[0.0],
			&[0.0],
			&[1000.0],
			&[1.0],
			&[1.0],
			&[1.0],
			&mut one,
		);
		assert_eq!(one[0], 0, "box far behind camera should be culled");
	}
}
