//! offline BSP tree and PVS compiler for Lunar levels.
//!
//! call from your crate's `build.rs` to compile a level mesh into a binary blob
//! that [`BspLevel`][lunar_bsp::level::BspLevel] can load at runtime with zero
//! parsing cost.
//!
//! # build.rs example
//!
//! ```ignore
//! fn main() {
//!     let blob = lunar_bsp_build::compile_bsp_file("assets/levels/level1.glb").unwrap();
//!     let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
//!     std::fs::write(out.join("level1.bsp"), blob).unwrap();
//!     println!("cargo:rerun-if-changed=assets/levels/level1.glb");
//! }
//! ```
//!
//! then in your game:
//!
//! ```ignore
//! let level = BspLevel::from_binary(include_bytes!(concat!(env!("OUT_DIR"), "/level1.bsp")))
//!     .expect("level bsp corrupt");
//! app.insert_resource(level);
//! ```
//!
//! # area ids
//!
//! name your GLTF meshes with an `areaN_` prefix (e.g. `area0_floor`, `area1_corridor`)
//! to automatically assign area ids. meshes without this prefix are untagged and
//! treated as always-visible geometry.
//!
//! portal hints let you override the auto-detected portals or add portals where
//! the geometry auto-detection misses connections.

pub mod gltf;
pub mod partition;
pub mod portal;
pub mod pvs;

pub use portal::BspPortalHint;

use partition::{InputTriangle, build_bsp};
use pvs::compute_pvs;
use portal::extract_portals;
use lunar_math::Vec3;
use lunar_bsp::level::BspBlob;

/// a mesh to include in the BSP compilation.
///
/// all vertices must be in world space (apply node transforms before passing in).
/// if loading from GLTF, use [`compile_bsp_file`] which handles transforms automatically.
pub struct BspInputMesh {
    /// world-space vertex positions.
    pub vertices: Vec<Vec3>,
    /// triangle index list (every 3 indices = one triangle).
    pub indices: Vec<u32>,
    /// area id for portal culling. `None` = always-visible (not portal-culled).
    pub area_id: Option<u32>,
}

/// configuration for the BSP compiler.
pub struct BspCompileConfig {
    /// maximum number of triangles per BSP leaf before forcing a split. default: 16.
    pub max_leaf_size: usize,
    /// number of random ray pairs to cast per leaf pair for PVS computation. default: 64.
    pub pvs_samples: usize,
    /// if > 0, leaf pairs whose centroids are farther apart than this distance (in
    /// world units) are assumed not visible and skipped. default: 0 (always test).
    pub pvs_skip_distance: f32,
}

impl Default for BspCompileConfig {
    fn default() -> Self {
        Self { max_leaf_size: 16, pvs_samples: 64, pvs_skip_distance: 0.0 }
    }
}

/// compile a set of meshes into a BSP blob.
///
/// `hints` provides optional designer-placed portals. if empty, portals are
/// auto-detected from adjacent leaf AABBs.
///
/// # Errors
///
/// returns an error string if no triangles are found or serialization fails.
pub fn compile_bsp(
    meshes: &[BspInputMesh],
    hints: &[BspPortalHint],
    config: &BspCompileConfig,
) -> Result<Vec<u8>, String> {
    // flatten all meshes into a single triangle list
    let mut triangles: Vec<InputTriangle> = Vec::new();
    let mut all_tris: Vec<[Vec3; 3]> = Vec::new();

    for mesh in meshes {
        let verts = &mesh.vertices;
        let mut i = 0;
        while i + 2 < mesh.indices.len() {
            let i0 = mesh.indices[i] as usize;
            let i1 = mesh.indices[i + 1] as usize;
            let i2 = mesh.indices[i + 2] as usize;
            if i0 >= verts.len() || i1 >= verts.len() || i2 >= verts.len() {
                i += 3;
                continue;
            }
            let orig_idx = triangles.len() as u32;
            triangles.push(InputTriangle {
                verts: [verts[i0], verts[i1], verts[i2]],
                area_id: mesh.area_id,
                original_index: orig_idx,
            });
            all_tris.push([verts[i0], verts[i1], verts[i2]]);
            i += 3;
        }
    }

    if triangles.is_empty() {
        return Err("compile_bsp: no valid triangles in input meshes".into());
    }

    // build BSP tree
    let partition = build_bsp(&triangles, config.max_leaf_size);

    // compute PVS
    let skip_dist_sq = if config.pvs_skip_distance > 0.0 {
        config.pvs_skip_distance * config.pvs_skip_distance
    } else {
        0.0
    };
    let pvs = compute_pvs(
        &partition.leaf_aabbs,
        &all_tris,
        config.pvs_samples,
        skip_dist_sq,
    );

    // extract portals
    let portals = extract_portals(
        &partition.leaf_aabbs,
        &partition.leaf_areas,
        hints,
    );

    // build area map: (leaf_index, area_id) for tagged leaves, sorted for binary search at runtime
    let mut area_map: Vec<(u32, u32)> = partition.leaf_areas.iter().enumerate()
        .filter_map(|(leaf, area)| area.map(|a| (leaf as u32, a)))
        .collect();
    area_map.sort_unstable_by_key(|&(li, _)| li);

    let blob = BspBlob {
        nodes: partition.nodes,
        leaf_triangles: partition.leaf_triangles,
        pvs: pvs.data,
        pvs_stride: pvs.stride,
        leaf_count: partition.leaf_count,
        portals,
        area_map,
    };

    bincode::serialize(&blob).map_err(|error| format!("bsp serialize error: {error}"))
}

/// compile a GLTF/GLB level file into a BSP blob using default settings.
///
/// equivalent to loading with [`gltf::load_gltf_meshes`] then calling [`compile_bsp`]
/// with default config and no hints.
///
/// # Errors
///
/// returns an error if the file cannot be read, parsed, or compiled.
pub fn compile_bsp_file(path: &str) -> Result<Vec<u8>, String> {
    compile_bsp_file_with_config(path, &[], &BspCompileConfig::default())
}

/// compile a GLTF/GLB level file with explicit portal hints and compile config.
///
/// # Errors
///
/// returns an error if the file cannot be read, parsed, or compiled.
pub fn compile_bsp_file_with_config(
    path: &str,
    hints: &[BspPortalHint],
    config: &BspCompileConfig,
) -> Result<Vec<u8>, String> {
    let meshes = gltf::load_gltf_meshes(path)?;
    compile_bsp(&meshes, hints, config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lunar_bsp::level::BspLevel;

    fn unit_cube_mesh(area_id: Option<u32>) -> BspInputMesh {
        // 12 triangles for a unit cube [0,1]^3
        let v = |x: f32, y: f32, z: f32| Vec3::new(x, y, z);
        let verts = vec![
            v(0.0,0.0,0.0), v(1.0,0.0,0.0), v(1.0,1.0,0.0), v(0.0,1.0,0.0), // front
            v(0.0,0.0,1.0), v(1.0,0.0,1.0), v(1.0,1.0,1.0), v(0.0,1.0,1.0), // back
        ];
        let indices = vec![
            0,1,2, 0,2,3, // front
            4,6,5, 4,7,6, // back
            0,4,5, 0,5,1, // bottom
            3,2,6, 3,6,7, // top
            0,3,7, 0,7,4, // left
            1,5,6, 1,6,2, // right
        ];
        BspInputMesh { vertices: verts, indices, area_id }
    }

    #[test]
    fn compile_and_load_round_trip() {
        let mesh = unit_cube_mesh(Some(0));
        let blob = compile_bsp(&[mesh], &[], &BspCompileConfig::default()).unwrap();
        let level = BspLevel::from_binary(&blob).unwrap();
        assert!(level.is_loaded());
        let leaf = level.camera_leaf(Vec3::new(0.5, 0.5, 0.5));
        let visible = level.visible_leaves(leaf);
        assert!(!visible.is_empty(), "camera leaf should see at least itself");
    }

    #[test]
    fn two_area_portals_detected() {
        // two cubes side-by-side, different areas — should auto-detect a portal
        let mesh_a = unit_cube_mesh(Some(0));
        let mesh_b = BspInputMesh {
            vertices: unit_cube_mesh(Some(1)).vertices.iter()
                .map(|v| Vec3::new(v.x + 1.0, v.y, v.z))
                .collect(),
            indices: unit_cube_mesh(Some(1)).indices,
            area_id: Some(1),
        };
        let blob = compile_bsp(
            &[mesh_a, mesh_b],
            &[],
            &BspCompileConfig { pvs_samples: 8, ..Default::default() },
        ).unwrap();
        let level = BspLevel::from_binary(&blob).unwrap();
        assert!(level.is_loaded());
    }

    #[test]
    fn empty_input_is_error() {
        assert!(compile_bsp(&[], &[], &BspCompileConfig::default()).is_err());
    }

    #[test]
    fn file_not_found_is_error() {
        assert!(compile_bsp_file("does_not_exist.glb").is_err());
    }
}
