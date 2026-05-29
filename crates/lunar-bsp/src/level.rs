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
        Ok(Self { blob: Some(blob) })
    }

    /// returns true if a BSP blob has been loaded.
    pub fn is_loaded(&self) -> bool { self.blob.is_some() }

    /// walk the BSP tree to find which leaf `pos` is in.
    ///
    /// returns leaf index 0 if no blob is loaded or the tree is empty.
    /// the returned value is the `leaf_index` field of the leaf node, suitable
    /// for passing directly to [`BspLevel::visible_leaves`].
    pub fn camera_leaf(&self, pos: Vec3) -> usize {
        let blob = match &self.blob { Some(b) => b, None => return 0 };
        if blob.nodes.is_empty() { return 0; }
        let mut node_idx = 0usize;
        loop {
            let node = &blob.nodes[node_idx];
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
    }

    /// return all leaf indices visible from `camera_leaf` according to the PVS.
    ///
    /// returns all leaves (0..leaf_count) if no blob is loaded or pvs_stride is 0,
    /// so downstream code always gets a valid visible set.
    pub fn visible_leaves(&self, camera_leaf: usize) -> Vec<usize> {
        let blob = match &self.blob { Some(b) => b, None => return vec![] };
        let leaf_count = blob.leaf_count as usize;
        if blob.pvs_stride == 0 || camera_leaf >= leaf_count {
            return (0..leaf_count).collect();
        }
        let stride = blob.pvs_stride as usize;
        let base = camera_leaf * stride;
        let mut out = Vec::with_capacity(leaf_count / 4);
        for leaf in 0..leaf_count {
            let word = leaf / 64;
            let bit = leaf % 64;
            let word_idx = base + word;
            if word_idx < blob.pvs.len() && blob.pvs[word_idx] & (1u64 << bit) != 0 {
                out.push(leaf);
            }
        }
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
