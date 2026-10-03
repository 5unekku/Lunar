//! SIMD frustum-vs-AABB culling over `CullSoa`. the kernel lives in
//! `lunar_math::simd_cull` so only that small crate needs the llvm backend in dev
//! builds (its avx2 intrinsics are unsupported by cranelift); lunar-3d re-exports it
//! and keeps the test that ties it to `Frustum::intersects_aabb`.

pub use lunar_math::simd_cull::cull_aabbs_soa;

#[cfg(test)]
mod tests {
	use super::cull_aabbs_soa;
	use crate::visibility::Frustum;
	use lunar_math::{Vec3A, glam::camera::rh};

	/// tiny deterministic LCG so tests don't pull a rng dependency.
	struct Lcg(u64);
	impl Lcg {
		fn next_f32(&mut self, lo: f32, hi: f32) -> f32 {
			self.0 = self
				.0
				.wrapping_mul(6364136223846793005)
				.wrapping_add(1442695040888963407);
			let unit = ((self.0 >> 40) as f32) / ((1u64 << 24) as f32);
			lo + unit * (hi - lo)
		}
	}

	fn test_frustum() -> Frustum {
		// a realistic perspective × look-at view, so the planes are non-trivial.
		let proj = rh::proj::directx::perspective(60_f32.to_radians(), 16.0 / 9.0, 0.1, 500.0);
		let view = rh::view::look_at_mat4(
			lunar_math::Vec3::new(3.0, 4.0, 10.0),
			lunar_math::Vec3::ZERO,
			lunar_math::Vec3::Y,
		);
		Frustum::from_view_proj(proj * view)
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
				assert_eq!(
					flags[i], 1,
					"box {i} kept by intersects_aabb but culled by SIMD"
				);
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
