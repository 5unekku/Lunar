//! runtime BSP level loaded from a precompiled blob.
//!
//! use [`BspLevel::from_binary`] to load a blob compiled by `lunar-bsp-build`,
//! then call [`BspLevel::camera_leaf`] and [`BspLevel::visible_leaves`] each frame
//! to drive portal and BVH culling.
//!
//! the blob format is produced by `compile_bsp` in the `lunar-bsp-build` crate.
//! store the resulting bytes in your game's `assets/` and load with
//! `include_bytes!` or the asset server.

use bevy_ecs::prelude::Resource;
use lunar_math::Vec3;
use serde::{Deserialize, Serialize};

/// single node in the precomputed BSP tree.
///
/// internal nodes: `left_or_start >= 0` (left child node index),
/// `right_or_end >= 0` (right child node index).
///
/// leaf nodes: `left_or_start < 0`.
/// `start = -(left_or_start + 1)`, `end = -(right_or_end + 1)` (exclusive)
/// give the range into `BspBlob::leaf_triangles`.
#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct BspNode {
	/// world-space AABB min for this node's subtree.
	pub min: [f32; 3],
	/// world-space AABB max for this node's subtree.
	pub max: [f32; 3],
	/// internal: left child index. leaf: -(start_in_leaf_triangles + 1)
	pub left_or_start: i32,
	/// internal: right child index. leaf: -(end_exclusive_in_leaf_triangles + 1)
	pub right_or_end: i32,
	/// split axis (0=x, 1=y, 2=z). internal nodes only.
	pub split_axis: u8,
	/// world-space split position along `split_axis`. internal nodes only.
	pub split_value: f32,
	/// sequential leaf index (0..leaf_count) used to look up the PVS row.
	/// only valid when `left_or_start < 0`. set to `u32::MAX` for internal nodes.
	pub leaf_index: u32,
}

/// portal extracted from level geometry or provided by a designer hint.
#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct PortalData {
	pub area_a: u32,
	pub area_b: u32,
	/// world-space center of the portal opening.
	pub center: [f32; 3],
	/// world-space half-extents of the portal opening.
	pub half_extents: [f32; 3],
}

/// the full precomputed BSP blob. serialized/deserialized with bincode.
#[derive(Serialize, Deserialize)]
pub struct BspBlob {
	/// flat BSP node array. node 0 is always the root.
	pub nodes: Vec<BspNode>,
	/// triangle indices packed in leaf order. leaves index ranges into this vec.
	pub leaf_triangles: Vec<u32>,
	/// flat PVS bitsets. layout: `pvs[leaf * pvs_stride + word]`, bit `j % 64`.
	/// if bit `j` of leaf `i`'s row is set, leaf `i` can see leaf `j`.
	pub pvs: Vec<u64>,
	/// number of u64 words per leaf in `pvs` (= ceil(leaf_count / 64)).
	pub pvs_stride: u32,
	/// total number of leaves in the BSP tree.
	pub leaf_count: u32,
	/// portals between areas (usable as `Portal` components at runtime).
	pub portals: Vec<PortalData>,
	/// maps leaf index → area id. only entries for leaves that have an area assigned.
	pub area_map: Vec<(u32, u32)>,
}

impl BspBlob {
	/// check the invariants the runtime walks rely on. bincode only checks the byte
	/// layout, so a stale or mismatched blob (compiled by an older tool, partially
	/// regenerated) used to deserialize fine and then panic on an out-of-range index
	/// in `camera_leaf` every frame.
	///
	/// - internal node children point strictly forward (`parent < child < len`); the
	///   compiler emits nodes parent-first, and this also makes every walk terminate
	/// - leaf triangle ranges lie inside `leaf_triangles`, leaf indices below `leaf_count`
	/// - the pvs holds `leaf_count * pvs_stride` words; area-map leaves exist
	pub fn validate(&self) -> Result<(), String> {
		let len = self.nodes.len();
		for (i, node) in self.nodes.iter().enumerate() {
			if node.left_or_start < 0 {
				let start = -(i64::from(node.left_or_start) + 1);
				let end = -(i64::from(node.right_or_end) + 1);
				if start < 0 || end < start || end as usize > self.leaf_triangles.len() {
					return Err(format!("bsp: leaf node {i} has triangle range {start}..{end}"));
				}
				if node.leaf_index >= self.leaf_count {
					return Err(format!(
						"bsp: leaf node {i} has leaf_index {} >= leaf_count {}",
						node.leaf_index, self.leaf_count
					));
				}
			} else {
				for child in [node.left_or_start, node.right_or_end] {
					let child = child as usize;
					if child <= i || child >= len {
						return Err(format!("bsp: node {i} has child {child} (nodes: {len})"));
					}
				}
			}
		}
		let expected_pvs = self.leaf_count as usize * self.pvs_stride as usize;
		if self.pvs_stride > 0 && self.pvs.len() != expected_pvs {
			return Err(format!("bsp: pvs has {} words, expected {expected_pvs}", self.pvs.len()));
		}
		if let Some(&(leaf, _)) = self.area_map.iter().find(|(leaf, _)| *leaf >= self.leaf_count) {
			return Err(format!("bsp: area map names leaf {leaf} >= leaf_count {}", self.leaf_count));
		}
		Ok(())
	}
}

/// resource: a loaded, precompiled BSP level.
///
/// insert this resource to enable BSP-based PVS culling. when absent, the engine
/// falls back to the dynamic BVH and ECS portal system.
///
/// # example
///
/// ```ignore
/// let bytes = include_bytes!("../assets/level1.bsp");
/// let level = BspLevel::from_binary(bytes).expect("failed to load level bsp");
/// app.insert_resource(level);
/// ```
#[derive(Resource, Default)]
pub struct BspLevel {
	blob: Option<BspBlob>,
}

impl BspLevel {
	/// load a BSP level from a binary blob produced by `lunar-bsp-build::compile_bsp`.
	///
	/// # Errors
	///
	/// returns an error string if deserialization fails (corrupt or wrong-version blob).
	pub fn from_binary(bytes: &[u8]) -> Result<Self, String> {
		let blob: BspBlob = bincode::deserialize(bytes)
			.map_err(|error| format!("bsp deserialize error: {error}"))?;
		blob.validate()?;
		Ok(Self { blob: Some(blob) })
	}

	/// returns true if a BSP blob has been loaded.
	pub fn is_loaded(&self) -> bool {
		self.blob.is_some()
	}

	/// walk the BSP tree to find which leaf `pos` is in.
	///
	/// returns leaf index 0 if no blob is loaded or the tree is empty.
	/// the returned value is the `leaf_index` field of the leaf node, suitable
	/// for passing directly to [`BspLevel::visible_leaves`].
	pub fn camera_leaf(&self, pos: Vec3) -> usize {
		let blob = match &self.blob {
			Some(b) => b,
			None => return 0,
		};
		if blob.nodes.is_empty() {
			return 0;
		}
		// from_binary validates the tree, but the fields are public: stay in bounds and
		// bounded regardless (a malformed tree yields leaf 0, i.e. "see everything")
		let mut node_idx = 0usize;
		for _ in 0..blob.nodes.len() {
			let Some(node) = blob.nodes.get(node_idx) else {
				return 0;
			};
			if node.left_or_start < 0 {
				return node.leaf_index as usize;
			}
			let coord = match node.split_axis {
				0 => pos.x,
				1 => pos.y,
				_ => pos.z,
			};
			node_idx = if coord >= node.split_value {
				node.right_or_end as usize
			} else {
				node.left_or_start as usize
			};
		}
		0
	}

	/// call `callback` with each leaf index visible from `camera_leaf` per the PVS.
	///
	/// calls back with every leaf (0..leaf_count) if pvs_stride is 0 or the leaf
	/// is out of range, so downstream code always gets a valid visible set. the
	/// renderer uses this each frame: no allocation, and the bitset scan skips
	/// empty words via trailing_zeros instead of probing every bit.
	pub fn for_each_visible_leaf(&self, camera_leaf: usize, mut callback: impl FnMut(usize)) {
		let Some(blob) = &self.blob else { return };
		let leaf_count = blob.leaf_count as usize;
		if blob.pvs_stride == 0 || camera_leaf >= leaf_count {
			for leaf in 0..leaf_count {
				callback(leaf);
			}
			return;
		}
		let stride = blob.pvs_stride as usize;
		let base = camera_leaf * stride;
		for word_idx in 0..stride {
			let Some(&word) = blob.pvs.get(base + word_idx) else {
				break;
			};
			let mut bits = word;
			while bits != 0 {
				let leaf = word_idx * 64 + bits.trailing_zeros() as usize;
				if leaf < leaf_count {
					callback(leaf);
				}
				bits &= bits - 1;
			}
		}
	}

	/// return all leaf indices visible from `camera_leaf` according to the PVS.
	///
	/// returns all leaves (0..leaf_count) if no blob is loaded or pvs_stride is 0,
	/// so downstream code always gets a valid visible set. allocates: prefer
	/// [`BspLevel::for_each_visible_leaf`] in per-frame code.
	pub fn visible_leaves(&self, camera_leaf: usize) -> Vec<usize> {
		let mut out = Vec::new();
		self.for_each_visible_leaf(camera_leaf, |leaf| out.push(leaf));
		out
	}

	/// portals stored in the blob.
	///
	/// game code can spawn `Portal` entities from these at level load if the
	/// ECS portal system is also in use alongside BSP culling.
	pub fn portals(&self) -> &[PortalData] {
		self.blob.as_ref().map_or(&[], |b| b.portals.as_slice())
	}

	/// area map: `(leaf_index, area_id)` pairs for leaves with an assigned area.
	pub fn area_map(&self) -> &[(u32, u32)] {
		self.blob.as_ref().map_or(&[], |b| b.area_map.as_slice())
	}
}

#[cfg(test)]
mod validation_tests {
	use super::*;

	fn node(left_or_start: i32, right_or_end: i32, leaf_index: u32) -> BspNode {
		BspNode {
			min: [-1.0; 3],
			max: [1.0; 3],
			left_or_start,
			right_or_end,
			split_axis: 0,
			split_value: 0.0,
			leaf_index,
		}
	}

	/// root splits on x into two leaves holding triangles [0, 1) and [1, 2)
	fn two_leaf_blob() -> BspBlob {
		BspBlob {
			nodes: vec![node(1, 2, u32::MAX), node(-1, -2, 0), node(-2, -3, 1)],
			leaf_triangles: vec![0, 1],
			pvs: vec![0b11, 0b11],
			pvs_stride: 1,
			leaf_count: 2,
			portals: Vec::new(),
			area_map: vec![(0, 0), (1, 1)],
		}
	}

	fn encode(blob: &BspBlob) -> Vec<u8> {
		bincode::serialize(blob).unwrap()
	}

	#[test]
	fn a_consistent_blob_loads_and_walks() {
		let level = BspLevel::from_binary(&encode(&two_leaf_blob())).unwrap();
		assert_eq!(level.camera_leaf(Vec3::new(-0.5, 0.0, 0.0)), 0);
		assert_eq!(level.camera_leaf(Vec3::new(0.5, 0.0, 0.0)), 1);
	}

	/// corr-13: child indices were followed unchecked, so a stale or mismatched blob
	/// panicked in camera_leaf on the first frame and every frame after.
	#[test]
	fn out_of_range_or_cyclic_children_are_rejected() {
		let mut blob = two_leaf_blob();
		blob.nodes[0].right_or_end = 9;
		assert!(BspLevel::from_binary(&encode(&blob)).is_err());

		let mut blob = two_leaf_blob();
		blob.nodes[0].left_or_start = 0; // points at itself: the walk never ends
		assert!(BspLevel::from_binary(&encode(&blob)).is_err());
	}

	#[test]
	fn bad_leaf_ranges_and_tables_are_rejected() {
		let mut blob = two_leaf_blob();
		blob.nodes[2].right_or_end = -9; // triangle range past leaf_triangles
		assert!(BspLevel::from_binary(&encode(&blob)).is_err());

		let mut blob = two_leaf_blob();
		blob.nodes[1].leaf_index = 5;
		assert!(BspLevel::from_binary(&encode(&blob)).is_err());

		let mut blob = two_leaf_blob();
		blob.pvs.pop();
		assert!(BspLevel::from_binary(&encode(&blob)).is_err());

		let mut blob = two_leaf_blob();
		blob.area_map.push((7, 0));
		assert!(BspLevel::from_binary(&encode(&blob)).is_err());
	}
}
