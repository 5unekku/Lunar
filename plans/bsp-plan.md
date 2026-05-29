  as a game developer, i want to compile my level geometry into a BSP+PVS blob at build time so that the runtime never pays to cull geometry that can't be seen.

  ---
  acceptance criteria

  - compile_bsp(meshes: &[BspInputMesh]) -> Result<Vec<u8>, String> takes a list of static mesh triangles (position + material id) and returns a binary blob readable by lunar-bsp::BspLevel::from_binary
  - compile_bsp_file(path: &str) -> Result<Vec<u8>, String> convenience wrapper that loads a GLTF/GLB file and calls the above
  - the blob encodes: BSP node tree, leaf→entity list, per-leaf PVS bitset, area/portal graph
  - BspLevel::from_binary (in lunar-bsp) deserializes the blob and exposes it as a resource the PortalPlugin and BvhPlugin can use instead of building from ECS components at runtime
  - a game's build.rs can compile a level in under 30 seconds for a scene with ≤10k triangles
  - the compiler emits cargo:rerun-if-changed directives so incremental builds work correctly

  ---
  technical requirements

  input
  - BspInputMesh { vertices: &[Vec3], indices: &[u32], area_id: Option<u32> } — caller provides world-space triangles; area_id is the designer-assigned area tag (matches Area(u32) component), None =
  always-visible geometry
  - optional: BspPortalHint { plane: Vec4, area_a: u32, area_b: u32 } — designer-placed portal planes; if absent the compiler attempts auto-detection from convex leaf boundaries

  BSP construction
  - axis-aligned BSP (k-d tree style, not polygon-splitting); simpler, fast to build, sufficient for culling
  - split heuristic: surface area heuristic (SAH) — minimise cost(left) * area(left) + cost(right) * area(right)
  - max leaf size: 16 triangles or configurable
  - output: flat Vec<BspNode> (cache-friendly), each node stores AABB + left/right child indices; leaves store triangle index ranges

  PVS computation
  - per-leaf potentially-visible set stored as a bitset (Vec<u64>, one bit per leaf)
  - algorithm: for each pair of leaves, check if a line of sight exists by sampling N random ray pairs between them and testing against the triangle list; if any ray is unblocked, mark visible
  - sample count: configurable, default 64 rays per pair
  - this is an approximation (not exact Quake-style flow-of-visibility) but conservative — false positives (mark visible when occluded) are safe; false negatives would cause pop-in so we err on the side of
  visible
  - for small scenes (≤256 leaves) do all pairs; for larger scenes use spatial coherence to skip clearly-separated pairs

  portal extraction
  - if BspPortalHints are provided: use them directly
  - auto-detection: for each pair of adjacent BSP leaves sharing a boundary AABB face, emit a portal connecting them; assign area_ids from the area_id field on the input meshes
  - portals stored as (area_a, area_b, center: Vec3, half_extents: Vec3) matching lunar-bsp::Portal

  output binary format
  - use bincode (consistent with the rest of the engine)
  - schema:
  BspBlob {
      nodes: Vec<BspNode>,
      leaf_triangles: Vec<u32>,       // triangle indices, leaf ranges index into this
      pvs: Vec<u64>,                  // flat bitsets, pvs[leaf * pvs_stride + word]
      pvs_stride: u32,                // words per leaf
      leaf_count: u32,
      portals: Vec<PortalData>,
      area_map: Vec<(u32, u32)>,      // (leaf_index, area_id)
  }

  integration with lunar-bsp runtime
  - BspLevel::from_binary (add to lunar-bsp) deserializes the blob
  - BspLevel resource: replaces the dynamic Bvh build — renderer queries it instead of building from ECS AABBs each frame
  - BspLevel::camera_leaf(pos: Vec3) -> usize — walk tree to find which leaf the camera is in
  - BspLevel::visible_leaves(camera_leaf: usize) -> impl Iterator<Item = usize> — iterate PVS bitset

  crate structure
  crates/lunar-bsp-build/
    Cargo.toml     -- deps: lunar-bsp (for shared types), gltf, bincode, rayon
    src/
      lib.rs       -- public API: compile_bsp, compile_bsp_file
      partition.rs -- SAH k-d tree construction
      pvs.rs       -- ray-sampling PVS computation
      portal.rs    -- portal extraction (hints + auto-detect)
      gltf.rs      -- GLTF loader → BspInputMesh

  out of scope for this story
  - exact flow-of-visibility PVS (Quake-style beam trees) — the ray-sampling approximation is good enough and 100× simpler to implement
  - streaming / partial BSP loading — full blob loaded at startup
  - dynamic geometry in the BSP — only static level mesh; dynamic entities continue using Bvh
