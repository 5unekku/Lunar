//! ray-sampling PVS (potentially-visible set) computation.
//!
//! for each pair of leaves, `pvs_samples` random ray pairs are cast between
//! random points in each leaf's AABB. if any ray passes through unblocked, the
//! leaves mark each other as mutually visible.
//!
//! this is a sampled estimate, not a conservative bound: a sightline narrower than
//! the sample spacing can be missed (a false negative, i.e. possible pop-in), and
//! `skip_distance_sq` marks far pairs hidden by design. raise `pvs_samples` for
//! levels with thin openings. rays are tested against a triangle bvh, and rayon
//! parallelises across leaf rows.

use lunar_math::Vec3;
use rayon::prelude::*;

/// the full PVS bitset for all leaves.
pub struct PvsResult {
	/// flat pvs[leaf * stride + word] bitset. bit j of row i = leaf i sees leaf j.
	pub data: Vec<u64>,
	/// words per leaf row (= ceil(leaf_count / 64)).
	pub stride: u32,
}

/// build the PVS for `leaf_count` leaves.
///
/// `leaf_aabbs`: world-space (min, max) per leaf.
/// `triangles`: all level triangles (flat list of `[Vec3;3]`), used for occlusion.
/// `samples`: number of random ray pairs to test per leaf pair (default 64).
/// `skip_distance_sq`: if both leaf centroids are farther apart than this, skip the
/// pair and mark as not-visible. set to 0.0 to always test all pairs.
pub fn compute_pvs(
	leaf_aabbs: &[([f32; 3], [f32; 3])],
	triangles: &[[Vec3; 3]],
	samples: usize,
	skip_distance_sq: f32,
) -> PvsResult {
	let leaf_count = leaf_aabbs.len();
	if leaf_count == 0 {
		return PvsResult {
			data: vec![],
			stride: 0,
		};
	}

	let stride = leaf_count.div_ceil(64);
	let total_words = leaf_count * stride;
	let bvh = TriBvh::build(triangles);

	// compute pvs in parallel: each row (camera leaf) as an independent unit
	let rows: Vec<Vec<u64>> = (0..leaf_count)
		.into_par_iter()
		.map(|leaf_a| {
			let mut row = vec![0u64; stride];
			// always mark self-visible
			let self_word = leaf_a / 64;
			let self_bit = leaf_a % 64;
			row[self_word] |= 1u64 << self_bit;

			let center_a = leaf_centroid(leaf_aabbs[leaf_a]);

			for leaf_b in 0..leaf_count {
				if leaf_b == leaf_a {
					continue;
				}
				let word = leaf_b / 64;
				if row[word] & (1u64 << (leaf_b % 64)) != 0 {
					continue;
				} // already visible

				let center_b = leaf_centroid(leaf_aabbs[leaf_b]);

				if skip_distance_sq > 0.0 {
					let d = center_a - center_b;
					if d.dot(d) > skip_distance_sq {
						continue;
					}
				}

				if leaves_see_each_other(
					leaf_aabbs[leaf_a],
					leaf_aabbs[leaf_b],
					&bvh,
					samples,
					leaf_a as u64,
				) {
					row[leaf_b / 64] |= 1u64 << (leaf_b % 64);
				}
			}
			row
		})
		.collect();

	let mut data = vec![0u64; total_words];
	for (leaf_a, row) in rows.iter().enumerate() {
		for (word, &bits) in row.iter().enumerate() {
			let idx = leaf_a * stride + word;
			data[idx] = bits;
			// symmetry: if a sees b, b sees a
			if bits != 0 {
				for bit in 0..64usize {
					if bits & (1u64 << bit) != 0 {
						let leaf_b = word * 64 + bit;
						if leaf_b < leaf_count {
							data[leaf_b * stride + leaf_a / 64] |= 1u64 << (leaf_a % 64);
						}
					}
				}
			}
		}
	}

	PvsResult {
		data,
		stride: stride as u32,
	}
}

fn leaf_centroid(aabb: ([f32; 3], [f32; 3])) -> Vec3 {
	Vec3::new(
		(aabb.0[0] + aabb.1[0]) * 0.5,
		(aabb.0[1] + aabb.1[1]) * 0.5,
		(aabb.0[2] + aabb.1[2]) * 0.5,
	)
}

fn leaves_see_each_other(
	aabb_a: ([f32; 3], [f32; 3]),
	aabb_b: ([f32; 3], [f32; 3]),
	bvh: &TriBvh<'_>,
	samples: usize,
	seed: u64,
) -> bool {
	let mut rng = Lcg::new(seed ^ 0xdeadbeef_cafef00d);
	for _ in 0..samples {
		let origin = random_point_in_aabb(&mut rng, aabb_a);
		let target = random_point_in_aabb(&mut rng, aabb_b);
		let dir = target - origin;
		let dist = dir.length();
		if dist < 1e-6 {
			continue;
		}
		let dir_norm = dir / dist;
		if !bvh.any_hit(origin, dir_norm, dist) {
			return true;
		}
	}
	false
}

fn random_point_in_aabb(rng: &mut Lcg, aabb: ([f32; 3], [f32; 3])) -> Vec3 {
	Vec3::new(
		aabb.0[0] + rng.next_f32() * (aabb.1[0] - aabb.0[0]),
		aabb.0[1] + rng.next_f32() * (aabb.1[1] - aabb.0[1]),
		aabb.0[2] + rng.next_f32() * (aabb.1[2] - aabb.0[2]),
	)
}

/// linear-scan any-hit; the reference the bvh is tested against.
#[cfg(test)]
fn ray_hits_any(origin: Vec3, dir: Vec3, max_dist: f32, triangles: &[[Vec3; 3]]) -> bool {
	triangles.iter().any(|tri| segment_hits(origin, dir, max_dist, tri))
}

/// whether the segment `origin + dir * t`, `t < max_dist`, hits `tri`.
fn segment_hits(origin: Vec3, dir: Vec3, max_dist: f32, tri: &[Vec3; 3]) -> bool {
	ray_triangle(origin, dir, tri[0], tri[1], tri[2]).is_some_and(|t| t < max_dist - 1e-4)
}

/// triangles per bvh leaf
const BVH_LEAF_SIZE: usize = 4;

struct BvhNode {
	min: Vec3,
	max: Vec3,
	/// leaf: first index into `order`. internal: index of the right child (the left
	/// child is always the next node)
	start_or_right: u32,
	/// leaf: triangle count (> 0). internal: 0
	count: u32,
}

/// bounding volume hierarchy over the level triangles, for any-hit segment queries.
/// median split on the longest centroid axis; a flat node array in depth-first order.
struct TriBvh<'a> {
	triangles: &'a [[Vec3; 3]],
	order: Vec<u32>,
	nodes: Vec<BvhNode>,
}

impl<'a> TriBvh<'a> {
	fn build(triangles: &'a [[Vec3; 3]]) -> Self {
		let mut bvh = Self {
			triangles,
			order: (0..triangles.len() as u32).collect(),
			nodes: Vec::with_capacity(2 * triangles.len().div_ceil(BVH_LEAF_SIZE)),
		};
		if !triangles.is_empty() {
			bvh.build_node(0, triangles.len());
		}
		bvh
	}

	fn build_node(&mut self, start: usize, end: usize) -> usize {
		let (mut min, mut max) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
		let (mut cmin, mut cmax) = (min, max);
		for &i in &self.order[start..end] {
			let tri = &self.triangles[i as usize];
			for v in tri {
				min = min.min(*v);
				max = max.max(*v);
			}
			let c = (tri[0] + tri[1] + tri[2]) / 3.0;
			cmin = cmin.min(c);
			cmax = cmax.max(c);
		}
		let node = self.nodes.len();
		self.nodes.push(BvhNode {
			min,
			max,
			start_or_right: start as u32,
			count: (end - start) as u32,
		});
		if end - start <= BVH_LEAF_SIZE {
			return node;
		}

		let extent = cmax - cmin;
		let axis = if extent.x >= extent.y && extent.x >= extent.z {
			0
		} else if extent.y >= extent.z {
			1
		} else {
			2
		};
		let mid = start + (end - start) / 2;
		let triangles = self.triangles;
		let key = |i: &u32| {
			let tri = &triangles[*i as usize];
			(tri[0][axis] + tri[1][axis] + tri[2][axis]) / 3.0
		};
		self.order[start..end].select_nth_unstable_by(mid - start, |a, b| key(a).total_cmp(&key(b)));

		self.build_node(start, mid);
		let right = self.build_node(mid, end);
		self.nodes[node].start_or_right = right as u32;
		self.nodes[node].count = 0;
		node
	}

	/// whether the segment `origin + dir * t`, `t < max_dist`, hits any triangle.
	fn any_hit(&self, origin: Vec3, dir: Vec3, max_dist: f32) -> bool {
		if self.nodes.is_empty() {
			return false;
		}
		// depth-first; depth is ~log2(n / BVH_LEAF_SIZE), far below the stack size
		let mut stack = [0u32; 64];
		let mut top = 1;
		while top > 0 {
			top -= 1;
			let index = stack[top] as usize;
			let node = &self.nodes[index];
			if !segment_overlaps_box(origin, dir, max_dist, node.min, node.max) {
				continue;
			}
			if node.count > 0 {
				let start = node.start_or_right as usize;
				let tris = &self.order[start..start + node.count as usize];
				if tris
					.iter()
					.any(|&i| segment_hits(origin, dir, max_dist, &self.triangles[i as usize]))
				{
					return true;
				}
			} else {
				stack[top] = node.start_or_right;
				stack[top + 1] = (index + 1) as u32;
				top += 2;
			}
		}
		false
	}
}

/// slab test: whether the segment `origin + dir * t`, `t` in `[0, max_dist]`, touches
/// the box. boxes are padded slightly so hits on a box face are never pruned.
fn segment_overlaps_box(origin: Vec3, dir: Vec3, max_dist: f32, min: Vec3, max: Vec3) -> bool {
	const PAD: f32 = 1e-3;
	let (mut t0, mut t1) = (0.0f32, max_dist);
	for axis in 0..3 {
		let (o, d) = (origin[axis], dir[axis]);
		let (lo, hi) = (min[axis] - PAD, max[axis] + PAD);
		if d.abs() < 1e-12 {
			if o < lo || o > hi {
				return false;
			}
			continue;
		}
		let inv = 1.0 / d;
		let (mut near, mut far) = ((lo - o) * inv, (hi - o) * inv);
		if near > far {
			std::mem::swap(&mut near, &mut far);
		}
		t0 = t0.max(near);
		t1 = t1.min(far);
		if t0 > t1 {
			return false;
		}
	}
	true
}

/// Möller-Trumbore ray-triangle intersection. returns distance along ray or None.
fn ray_triangle(origin: Vec3, dir: Vec3, v0: Vec3, v1: Vec3, v2: Vec3) -> Option<f32> {
	let e1 = v1 - v0;
	let e2 = v2 - v0;
	let h = dir.cross(e2);
	let a = e1.dot(h);
	if a.abs() < 1e-7 {
		return None;
	}
	let f = 1.0 / a;
	let s = origin - v0;
	let u = f * s.dot(h);
	if !(0.0..=1.0).contains(&u) {
		return None;
	}
	let q = s.cross(e1);
	let v = f * dir.dot(q);
	if v < 0.0 || u + v > 1.0 {
		return None;
	}
	let t = f * e2.dot(q);
	if t > 1e-6 { Some(t) } else { None }
}

/// minimal LCG PRNG; avoids pulling in the `rand` crate for a build tool.
struct Lcg(u64);

impl Lcg {
	fn new(seed: u64) -> Self {
		Self(seed ^ 0x6c62272e07bb0142)
	}

	fn next_u64(&mut self) -> u64 {
		self.0 = self
			.0
			.wrapping_mul(6364136223846793005)
			.wrapping_add(1442695040888963407);
		self.0
	}

	/// uniform in [0, 1): the top 24 bits over 2^24 (an f32's mantissa). the old
	/// `(x >> 33) / u32::MAX` topped out at 0.5, so every sample landed in the lower
	/// half of each leaf box.
	fn next_f32(&mut self) -> f32 {
		(self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn lcg_samples_span_the_unit_interval() {
		let mut rng = Lcg::new(42);
		let (mut lo, mut hi) = (1.0f32, 0.0f32);
		for _ in 0..10_000 {
			let v = rng.next_f32();
			assert!((0.0..=1.0).contains(&v));
			lo = lo.min(v);
			hi = hi.max(v);
		}
		assert!(lo < 0.01 && hi > 0.99, "samples must cover [0, 1], got [{lo}, {hi}]");
	}

	/// random triangle soup + random segments: the bvh any-hit must agree with a
	/// linear scan on every query.
	#[test]
	fn bvh_any_hit_matches_linear_scan() {
		let mut rng = Lcg::new(7);
		let mut v = || Vec3::new(rng.next_f32(), rng.next_f32(), rng.next_f32()) * 100.0;
		let triangles: Vec<[Vec3; 3]> = (0..500)
			.map(|_| {
				let base = v();
				[base, base + v() * 0.1, base + v() * 0.1]
			})
			.collect();
		let bvh = TriBvh::build(&triangles);
		for _ in 0..2000 {
			let origin = v();
			let target = v();
			let dir = target - origin;
			let dist = dir.length();
			if dist < 1e-3 {
				continue;
			}
			let dir = dir / dist;
			assert_eq!(
				bvh.any_hit(origin, dir, dist),
				ray_hits_any(origin, dir, dist, &triangles),
				"bvh and linear scan disagree for {origin:?} -> {target:?}"
			);
		}
	}

	/// two unit-cube leaves side by side on x with a quad between them: sealed, they
	/// cannot see each other; with a hole cut in the wall's upper half they can.
	#[test]
	fn wall_blocks_and_gap_reveals() {
		let leaves = [([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]), ([2.0, 0.0, 0.0], [3.0, 1.0, 1.0])];
		let quad = |y0: f32, y1: f32| {
			let (a, b, c, d) = (
				Vec3::new(1.5, y0, -1.0),
				Vec3::new(1.5, y1, -1.0),
				Vec3::new(1.5, y1, 2.0),
				Vec3::new(1.5, y0, 2.0),
			);
			[[a, b, c], [a, c, d]]
		};
		let sealed: Vec<[Vec3; 3]> = quad(-1.0, 2.0).to_vec();
		let pvs = compute_pvs(&leaves, &sealed, 64, 0.0);
		assert_eq!(pvs.data[0] & 0b10, 0, "a full wall must hide leaf 1 from leaf 0");

		// wall only covers the lower half: rays through the upper half get through
		let low_wall: Vec<[Vec3; 3]> = quad(-1.0, 0.5).to_vec();
		let pvs = compute_pvs(&leaves, &low_wall, 64, 0.0);
		assert_ne!(pvs.data[0] & 0b10, 0, "leaf 1 is visible over a half-height wall");
	}
}
