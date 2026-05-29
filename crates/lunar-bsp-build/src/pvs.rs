//! ray-sampling PVS (potentially-visible set) computation.
//!
//! for each pair of leaves, `pvs_samples` random ray pairs are cast between
//! random points in each leaf's AABB. if any ray passes through unblocked, the
//! leaves mark each other as mutually visible. this is a conservative approximation:
//! false positives (over-visible) are safe; false negatives would cause pop-in.
//!
//! rayon parallelises across leaf pairs for performance.

use rayon::prelude::*;
use lunar_math::Vec3;

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
        return PvsResult { data: vec![], stride: 0 };
    }

    let stride = (leaf_count + 63) / 64;
    let total_words = leaf_count * stride;

    // compute pvs in parallel: each row (camera leaf) as an independent unit
    let rows: Vec<Vec<u64>> = (0..leaf_count).into_par_iter().map(|leaf_a| {
        let mut row = vec![0u64; stride];
        // always mark self-visible
        let self_word = leaf_a / 64;
        let self_bit = leaf_a % 64;
        row[self_word] |= 1u64 << self_bit;

        let center_a = leaf_centroid(leaf_aabbs[leaf_a]);

        for leaf_b in 0..leaf_count {
            if leaf_b == leaf_a { continue; }
            let word = leaf_b / 64;
            if row[word] & (1u64 << (leaf_b % 64)) != 0 { continue; } // already visible

            let center_b = leaf_centroid(leaf_aabbs[leaf_b]);

            if skip_distance_sq > 0.0 {
                let d = center_a - center_b;
                if d.dot(d) > skip_distance_sq { continue; }
            }

            if leaves_see_each_other(leaf_aabbs[leaf_a], leaf_aabbs[leaf_b], triangles, samples, leaf_a as u64) {
                row[leaf_b / 64] |= 1u64 << (leaf_b % 64);
            }
        }
        row
    }).collect();

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

    PvsResult { data, stride: stride as u32 }
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
    triangles: &[[Vec3; 3]],
    samples: usize,
    seed: u64,
) -> bool {
    let mut rng = Lcg::new(seed ^ 0xdeadbeef_cafef00d);
    for _ in 0..samples {
        let origin = random_point_in_aabb(&mut rng, aabb_a);
        let target = random_point_in_aabb(&mut rng, aabb_b);
        let dir = target - origin;
        let dist = dir.length();
        if dist < 1e-6 { continue; }
        let dir_norm = dir / dist;
        if !ray_hits_any(origin, dir_norm, dist, triangles) {
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

fn ray_hits_any(origin: Vec3, dir: Vec3, max_dist: f32, triangles: &[[Vec3; 3]]) -> bool {
    for tri in triangles {
        if let Some(t) = ray_triangle(origin, dir, tri[0], tri[1], tri[2]) {
            if t < max_dist - 1e-4 { return true; }
        }
    }
    false
}

/// Möller-Trumbore ray-triangle intersection. returns distance along ray or None.
fn ray_triangle(origin: Vec3, dir: Vec3, v0: Vec3, v1: Vec3, v2: Vec3) -> Option<f32> {
    let e1 = v1 - v0;
    let e2 = v2 - v0;
    let h = dir.cross(e2);
    let a = e1.dot(h);
    if a.abs() < 1e-7 { return None; }
    let f = 1.0 / a;
    let s = origin - v0;
    let u = f * s.dot(h);
    if !(0.0..=1.0).contains(&u) { return None; }
    let q = s.cross(e1);
    let v = f * dir.dot(q);
    if v < 0.0 || u + v > 1.0 { return None; }
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
        self.0 = self.0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    fn next_f32(&mut self) -> f32 {
        (self.next_u64() >> 33) as f32 / (u32::MAX as f32)
    }
}
