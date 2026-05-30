//! bake-pvs: fill the PVS bitsets in a BspBlob via area-flood BFS.
//!
//! reads a bincode-serialized `BspBlob`, runs the flood algorithm through
//! the portal area adjacency graph, writes the blob back with `pvs` and
//! `pvs_stride` filled in. the runtime renderer automatically uses the PVS
//! once `pvs_stride > 0`.
//!
//! # usage
//!
//! ```
//! bake-pvs level1.bsp
//! bake-pvs level1.bsp --out level1_pvs.bsp
//! ```
//!
//! # algorithm
//!
//! 1. build area adjacency from PortalData (area_a ↔ area_b edges)
//! 2. build area→leaves map from area_map
//! 3. for each leaf: BFS from its area through area adjacency, collect all
//!    reachable areas, mark every leaf in those areas as visible
//! 4. encode into flat pvs bitset with pvs_stride = ceil(leaf_count / 64)
//!
//! for levels with no portals (pvs_stride would stay 0), the tool exits early
//! and does not modify the blob — the runtime correctly falls back to full
//! BVH culling in that case.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

// local copies of the BspBlob structures so this tool has no engine dependency
#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
struct BspNode {
    pub min:            [f32; 3],
    pub max:            [f32; 3],
    pub left_or_start:  i32,
    pub right_or_end:   i32,
    pub split_axis:     u8,
    pub split_value:    f32,
    pub leaf_index:     u32,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
struct PortalData {
    pub area_a:        u32,
    pub area_b:        u32,
    pub center:        [f32; 3],
    pub half_extents:  [f32; 3],
}

#[derive(Serialize, Deserialize)]
struct BspBlob {
    pub nodes:          Vec<BspNode>,
    pub leaf_triangles: Vec<u32>,
    pub pvs:            Vec<u64>,
    pub pvs_stride:     u32,
    pub leaf_count:     u32,
    pub portals:        Vec<PortalData>,
    pub area_map:       Vec<(u32, u32)>,
}

/// build area adjacency: area_id → set of directly adjacent area_ids
fn build_area_adjacency(portals: &[PortalData]) -> HashMap<u32, HashSet<u32>> {
    let mut adj: HashMap<u32, HashSet<u32>> = HashMap::new();
    for portal in portals {
        adj.entry(portal.area_a).or_default().insert(portal.area_b);
        adj.entry(portal.area_b).or_default().insert(portal.area_a);
    }
    adj
}

/// build area→leaves: area_id → list of leaf indices
fn build_area_leaves(area_map: &[(u32, u32)]) -> HashMap<u32, Vec<u32>> {
    let mut map: HashMap<u32, Vec<u32>> = HashMap::new();
    for &(leaf_idx, area_id) in area_map {
        map.entry(area_id).or_default().push(leaf_idx);
    }
    map
}

/// BFS through area adjacency starting from `start_area`, returns all reachable area_ids
fn flood_from_area(start: u32, adj: &HashMap<u32, HashSet<u32>>) -> HashSet<u32> {
    let mut visited: HashSet<u32> = HashSet::new();
    let mut queue: VecDeque<u32> = VecDeque::new();
    visited.insert(start);
    queue.push_back(start);
    while let Some(area) = queue.pop_front() {
        if let Some(neighbors) = adj.get(&area) {
            for &neighbor in neighbors {
                if visited.insert(neighbor) {
                    queue.push_back(neighbor);
                }
            }
        }
    }
    visited
}

fn bake(blob: &mut BspBlob) {
    let leaf_count = blob.leaf_count as usize;

    if blob.portals.is_empty() || blob.area_map.is_empty() {
        println!("no portals or area_map — level has no indoor PVS to bake. skipping.");
        return;
    }

    let adj = build_area_adjacency(&blob.portals);
    let area_leaves = build_area_leaves(&blob.area_map);

    // leaf_index → area_id
    let leaf_area: HashMap<u32, u32> = blob.area_map.iter().map(|&(leaf, area)| (leaf, area)).collect();

    let pvs_stride = (leaf_count + 63) / 64;
    let mut pvs = vec![0u64; leaf_count * pvs_stride];

    let mut leaves_without_area = 0u32;

    for leaf in 0..leaf_count as u32 {
        let row_base = leaf as usize * pvs_stride;

        // always mark self as visible
        pvs[row_base + leaf as usize / 64] |= 1u64 << (leaf % 64);

        let Some(&area) = leaf_area.get(&leaf) else {
            // leaf not in any area — only sees itself
            leaves_without_area += 1;
            continue;
        };

        let reachable_areas = flood_from_area(area, &adj);

        for visible_area in &reachable_areas {
            if let Some(area_leaf_list) = area_leaves.get(visible_area) {
                for &visible_leaf in area_leaf_list {
                    if visible_leaf < leaf_count as u32 {
                        pvs[row_base + visible_leaf as usize / 64] |= 1u64 << (visible_leaf % 64);
                    }
                }
            }
        }
    }

    blob.pvs        = pvs;
    blob.pvs_stride = pvs_stride as u32;

    println!("baked pvs: {} leaves, {} words/leaf, {} leaves without area", leaf_count, pvs_stride, leaves_without_area);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.is_empty() {
        eprintln!("usage: bake-pvs <input.bsp> [--out output.bsp]");
        std::process::exit(1);
    }

    let input_path = PathBuf::from(&args[0]);
    let output_path = if let Some(pos) = args.iter().position(|a| a == "--out") {
        PathBuf::from(&args[pos + 1])
    } else {
        input_path.clone()
    };

    let bytes = std::fs::read(&input_path).unwrap_or_else(|e| panic!("read {}: {e}", input_path.display()));
    let mut blob: BspBlob = bincode::deserialize(&bytes)
        .unwrap_or_else(|e| panic!("deserialize {}: {e}", input_path.display()));

    println!("{}: {} nodes, {} leaves, {} portals", input_path.display(), blob.nodes.len(), blob.leaf_count, blob.portals.len());

    if blob.pvs_stride > 0 {
        println!("pvs already baked (pvs_stride={}), re-baking anyway", blob.pvs_stride);
    }

    bake(&mut blob);

    let out_bytes = bincode::serialize(&blob).expect("serialize failed");
    std::fs::write(&output_path, &out_bytes).unwrap_or_else(|e| panic!("write {}: {e}", output_path.display()));
    println!("wrote {} ({} bytes)", output_path.display(), out_bytes.len());
}
