//! SAH k-d tree construction for offline BSP compilation.
//!
//! input triangles are partitioned into a flat node array using the surface-area
//! heuristic (SAH) to minimise the expected number of nodes visited per query.
//! the tree is axis-aligned (k-d style, no polygon splitting).

use lunar_bsp::level::BspNode;
use lunar_math::Vec3;

/// a single triangle with metadata for BSP construction.
pub struct InputTriangle {
	pub verts: [Vec3; 3],
	pub area_id: Option<u32>,
	pub original_index: u32,
}

/// result of building the BSP tree over a set of triangles.
pub struct PartitionResult {
	/// flat node array. node 0 is always the root.
	pub nodes: Vec<BspNode>,
	/// triangle `original_index` values packed in leaf order.
	pub leaf_triangles: Vec<u32>,
	/// sequential count of leaves created.
	pub leaf_count: u32,
	/// world-space AABB (min, max) per leaf, in leaf-index order.
	pub leaf_aabbs: Vec<([f32; 3], [f32; 3])>,
	/// area id per leaf, in leaf-index order. `None` if unassigned.
	pub leaf_areas: Vec<Option<u32>>,
}

/// build a BSP tree over `triangles` using SAH splits.
///
/// `max_leaf_size` controls how many triangles land in a leaf before splitting stops.
pub fn build_bsp(triangles: &[InputTriangle], max_leaf_size: usize) -> PartitionResult {
	let mut result = PartitionResult {
		nodes: Vec::new(),
		leaf_triangles: Vec::new(),
		leaf_count: 0,
		leaf_aabbs: Vec::new(),
		leaf_areas: Vec::new(),
	};
	if triangles.is_empty() {
		return result;
	}
	let indices: Vec<usize> = (0..triangles.len()).collect();
	build_node(triangles, &indices, max_leaf_size, &mut result);
	result
}

fn tri_centroid(tri: &InputTriangle) -> Vec3 {
	(tri.verts[0] + tri.verts[1] + tri.verts[2]) / 3.0
}

fn tri_aabb(tri: &InputTriangle) -> ([f32; 3], [f32; 3]) {
	let mut min = [f32::INFINITY; 3];
	let mut max = [f32::NEG_INFINITY; 3];
	for v in &tri.verts {
		let coords = [v.x, v.y, v.z];
		for a in 0..3 {
			if coords[a] < min[a] {
				min[a] = coords[a];
			}
			if coords[a] > max[a] {
				max[a] = coords[a];
			}
		}
	}
	(min, max)
}

fn surface_area(min: &[f32; 3], max: &[f32; 3]) -> f32 {
	let dx = (max[0] - min[0]).max(0.0);
	let dy = (max[1] - min[1]).max(0.0);
	let dz = (max[2] - min[2]).max(0.0);
	2.0 * (dx * dy + dy * dz + dz * dx)
}

fn build_node(
	triangles: &[InputTriangle],
	indices: &[usize],
	max_leaf_size: usize,
	result: &mut PartitionResult,
) -> i32 {
	// compute AABB over all triangles in this node
	let mut min = [f32::INFINITY; 3];
	let mut max = [f32::NEG_INFINITY; 3];
	for &i in indices {
		let (tmin, tmax) = tri_aabb(&triangles[i]);
		for a in 0..3 {
			if tmin[a] < min[a] {
				min[a] = tmin[a];
			}
			if tmax[a] > max[a] {
				max[a] = tmax[a];
			}
		}
	}

	let node_idx = result.nodes.len() as i32;
	result.nodes.push(BspNode {
		min,
		max,
		left_or_start: 0,
		right_or_end: 0,
		split_axis: 0,
		split_value: 0.0,
		leaf_index: u32::MAX,
	});

	if indices.len() <= max_leaf_size {
		return make_leaf(triangles, indices, min, max, node_idx, result);
	}

	// SAH: try 8 candidate splits per axis, pick the one with lowest expected cost
	const BUCKETS: usize = 8;
	let mut best_cost = f32::INFINITY;
	let mut best_axis = 0u8;
	let mut best_split = 0.0f32;

	for axis in 0u8..3 {
		let axis_extent = max[axis as usize] - min[axis as usize];
		if axis_extent < 1e-6 {
			continue;
		}

		for bucket in 1..BUCKETS {
			let split = min[axis as usize] + axis_extent * (bucket as f32 / BUCKETS as f32);
			let mut lmin = [f32::INFINITY; 3];
			let mut lmax = [f32::NEG_INFINITY; 3];
			let mut rmin = [f32::INFINITY; 3];
			let mut rmax = [f32::NEG_INFINITY; 3];
			let mut lcount = 0usize;
			let mut rcount = 0usize;

			for &i in indices {
				let c = tri_centroid(&triangles[i]);
				let c_arr = [c.x, c.y, c.z];
				let (tmin, tmax) = tri_aabb(&triangles[i]);
				if c_arr[axis as usize] < split {
					lcount += 1;
					for a in 0..3 {
						if tmin[a] < lmin[a] {
							lmin[a] = tmin[a];
						}
						if tmax[a] > lmax[a] {
							lmax[a] = tmax[a];
						}
					}
				} else {
					rcount += 1;
					for a in 0..3 {
						if tmin[a] < rmin[a] {
							rmin[a] = tmin[a];
						}
						if tmax[a] > rmax[a] {
							rmax[a] = tmax[a];
						}
					}
				}
			}

			if lcount == 0 || rcount == 0 {
				continue;
			}

			let cost = lcount as f32 * surface_area(&lmin, &lmax)
				+ rcount as f32 * surface_area(&rmin, &rmax);
			if cost < best_cost {
				best_cost = cost;
				best_axis = axis;
				best_split = split;
			}
		}
	}

	// fallback when SAH found no valid split: median along longest axis
	if best_cost.is_infinite() {
		let extents = [max[0] - min[0], max[1] - min[1], max[2] - min[2]];
		best_axis = if extents[0] >= extents[1] && extents[0] >= extents[2] {
			0
		} else if extents[1] >= extents[2] {
			1
		} else {
			2
		};
		best_split = (min[best_axis as usize] + max[best_axis as usize]) * 0.5;
	}

	let (left_indices, right_indices): (Vec<usize>, Vec<usize>) =
		indices.iter().copied().partition(|&i| {
			let c = tri_centroid(&triangles[i]);
			let c_arr = [c.x, c.y, c.z];
			c_arr[best_axis as usize] < best_split
		});

	// all landed on one side (degenerate geometry) — force median split to avoid infinite recursion
	if left_indices.is_empty() || right_indices.is_empty() {
		let mid = indices.len() / 2;
		let left_node = build_node(triangles, &indices[..mid], max_leaf_size, result);
		let right_node = build_node(triangles, &indices[mid..], max_leaf_size, result);
		let node = &mut result.nodes[node_idx as usize];
		node.left_or_start = left_node;
		node.right_or_end = right_node;
		node.split_axis = best_axis;
		node.split_value = best_split;
		return node_idx;
	}

	let left_node = build_node(triangles, &left_indices, max_leaf_size, result);
	let right_node = build_node(triangles, &right_indices, max_leaf_size, result);
	let node = &mut result.nodes[node_idx as usize];
	node.left_or_start = left_node;
	node.right_or_end = right_node;
	node.split_axis = best_axis;
	node.split_value = best_split;
	node_idx
}

fn make_leaf(
	triangles: &[InputTriangle],
	indices: &[usize],
	min: [f32; 3],
	max: [f32; 3],
	node_idx: i32,
	result: &mut PartitionResult,
) -> i32 {
	let leaf_idx = result.leaf_count;
	let start = result.leaf_triangles.len() as i32;
	// majority area_id for this leaf (most common among its triangles)
	let area_id = dominant_area(triangles, indices);
	for &i in indices {
		result.leaf_triangles.push(triangles[i].original_index);
	}
	let end = result.leaf_triangles.len() as i32;
	result.leaf_aabbs.push((min, max));
	result.leaf_areas.push(area_id);
	result.leaf_count += 1;

	let node = &mut result.nodes[node_idx as usize];
	node.left_or_start = -(start + 1);
	node.right_or_end = -(end + 1);
	node.leaf_index = leaf_idx;
	node_idx
}

fn dominant_area(triangles: &[InputTriangle], indices: &[usize]) -> Option<u32> {
	let mut counts: Vec<(u32, usize)> = Vec::new();
	for &i in indices {
		if let Some(area) = triangles[i].area_id {
			if let Some(entry) = counts.iter_mut().find(|(id, _)| *id == area) {
				entry.1 += 1;
			} else {
				counts.push((area, 1));
			}
		}
	}
	counts
		.into_iter()
		.max_by_key(|(_, count)| *count)
		.map(|(area, _)| area)
}
