# performance squeezing

findings from the third code review pass, focused on competitive FPS performance.
cross-referenced against `plans/performance.md` (tracks what's done) and
`plans/optimize.md` (gpu-driven architecture research).

reference engines are at `/var/tmp/lunar-ref/`. license notes:
- **copy freely**: godot (MIT), ogre/ogre-next (MIT), o3de (Apache 2.0), bevy (MIT/Apache), fyrox (MIT)
- **reference only**: quake, ioq3, DOOM, doom-3/BFG (GPL)

---

## implementation status

all tiers fully implemented. tier 4 notes:
- **mesh LOD**: `MeshLod` component + select() in gather pass — done
- **render graph DAG**: `render_graph.rs` with Kahn topological sort, all passes registered,
  drives execution order in render_frame — done
- **GPU frustum cull**: `cull.wgsl` compute shader replacing CPU CullSoa on high tier — done
- **HZB occlusion cull**: `hzb.wgsl` (depth copy + min-depth mip chain + AABB screen-projection
  occlusion test), runs as two-pass system on high tier — done
- **meshlets**: blocked on wgpu mesh shader support; cannot implement until wgpu adds the feature

note: the "2D tile and sprite batching" tier 1 item turned out to be already implemented —
the 2D renderer already sorts draw commands by (layer, tex_id) and batches all same-texture
sprites into one GPU draw call per group. 300 tilemap `push()` calls → 1 GPU draw call.

---

## already done (do not re-litigate)

see `plans/performance.md` for the full done list. short version: z-prepass, 3-cascade CSM,
SoA frustum culling with Vec3A, dynamic entity UBO (single write_buffer per frame),
GPU particle simulation via compute, terrain geometry clipmap LOD, pipeline cache,
fixed-timestep accumulator, staging belt, text quad vec reuse, per-frame alloc elimination
in the 3D render hot path, triple-buffered presentation, transparent back-to-front sort,
GTAO, volumetric fog, atmospheric scattering, dynamic resolution scaling.

---

## tier 1 — engine cannot scale without these

### physics spatial partitioning (2D and 3D)

**what it is**: a spatial data structure that lets you answer "what objects are near this
AABB?" in O(log n) or O(k) time instead of scanning every object in the scene. without one,
every collision query touches every collider — O(n²) total. with one, each query only visits
the objects that could plausibly overlap.

the two main options:
- **uniform grid**: divide space into fixed-size cells, bucket objects by cell. query = check
  only the cells the AABB covers. O(1) insert/remove, O(k) query where k = objects in
  nearby cells. works well when objects are roughly uniform in size.
- **dynamic AABB tree (BVH)**: binary tree where each leaf is a collider AABB and each
  internal node is the union of its children. insert/remove = O(log n) refit. query = tree
  traversal, early-exit on miss. handles objects of wildly different sizes well. this is
  what godot, bullet, and box2d all use.

**problem**: `overlapping_at()` in physics-3d and the broad phase in physics-2d both scan
all entries in the collision world per query. O(n²) at steady state.

- 100 dynamic bodies = 10,000 pair tests per frame
- 500 bodies = 250,000 — unplayable

**reference implementations**:
- `godot/servers/physics_3d/godot_broad_phase_3d_bvh.cpp` — godot's dynamic BVH (MIT, copy freely)
- `godot/core/math/dynamic_bvh.h` — the actual BVH structure; well-commented, battle-tested
- `Fyrox/src/scene/graph/physics/mod.rs` — rapier-backed but the integration pattern is clear
- quake: clip hulls in `world.c` (BSP-based, reference only) — overkill for dynamic bodies
- doom-3: `idBoundsMap` in `Simd_Generic.cpp` (reference only)

**approach**: dynamic AABB tree (same design as godot's `DynamicBVH`). insert on spawn,
refit leaf on transform change, refit branch on any child change. broad phase = tree query,
narrow phase unchanged. target: O(n log n) updates, O(k) queries where k = contacts.

---

### GPU instancing for static and repeated geometry

**what it is**: when multiple entities share the same mesh and material, instead of issuing
one draw call per entity, collect all their transforms into a single buffer and issue one
draw call with `instance_count = N`. the GPU fans the work across its cores internally,
which is exactly what it's built for. the CPU overhead of a draw call (command encoding,
pipeline state validation, driver work) is roughly 1–10µs per call — 500 separate draws
of the same barrel wastes 0.5–5ms doing logistics before the GPU touches a single triangle.
with instancing that becomes one call: ~10µs regardless of N.

```
// current: 500 entities, 500 draw calls
for each entity:
    upload transform
    bind material
    draw mesh

// instanced: 500 entities, 1 draw call
collect all (barrel_mesh, barrel_material) → [t0, t1, ..., t499]
upload instance buffer once
draw barrel_mesh, instance_count=500
```

the shader change is one line: read the transform from the instance buffer via the
built-in instance index rather than a per-entity uniform. the existing dynamic entity UBO
already packs transforms contiguously — the upload side is already close to what's needed.

**problem**: every `Mesh3d` entity is its own draw call with its own transform upload.
a forest of 500 identical trees = 500 draw calls. Quake levels have 100–300 BSP surfaces
total — we can't beat that by being 10× worse per entity.

**reference implementations**:
- `ogre-next/OgreMain/src/OgreInstanceManager.cpp` — hardware instancing (MIT, copy freely);
  specifically `HWInstancingVTF` (vertex texture fetch) for drivers without UBO indexing
- `o3de/Gems/Atom/RPI/Code/Source/RPI.Reflect/Model/` — mesh instance system (Apache 2.0)
- `bevy/crates/bevy_pbr/src/render/mesh.rs` — `GpuMesh` and instance buffer pattern (MIT/Apache)
- `Fyrox/src/renderer/batch.rs` — surface batching by material+mesh key (MIT)

**approach**: `MeshInstance` component (handle to mesh + handle to material + transform index).
batch collection pass groups instances by (mesh_id, material_id). single
`draw_indexed_indirect` or `draw_indexed` with instance count per group. transform data
in the existing dynamic entity UBO — already packed, just need the draw call side.

---

### 2D tile and sprite batching

**what it is**: instead of issuing one draw call per tile or sprite, accumulate all quads
that share the same texture into a single vertex buffer and draw them all at once. a 20×15
tilemap using one tileset texture goes from 300 draw calls to 1. the CPU builds a list of
quad vertices (position + UV for each corner) in a buffer, uploads it once, and the GPU
draws every tile in a single pass. the only time you need a new draw call is when the
texture changes — so grouping by texture is the key.

```
// current: one call per tile
for each visible tile:
    draw_sprite_atlas_on_layer(texture, pos, uv_rect)  ← 300 calls

// batched: one call per texture per layer
for each (texture, layer) group:
    build vertex buffer of all quads in this group
    upload once, draw once                             ← 1–3 calls
```

**problem**: `render_tilemaps` calls `draw_sprite_atlas_on_layer()` once per visible tile.
20×15 map at full visibility = 300 draw commands, each a separate GPU call.
same issue affects any scene with many sprites sharing a texture.

**reference implementations**:
- `godot/servers/rendering/renderer_2d.cpp` — canvas item batching, groups by texture (MIT)
- `godot/servers/rendering/renderer_canvas_cull.cpp` — the cull + batch build pass
- `Fyrox/src/renderer/sprite_renderer.rs` — sprite batch builder (MIT, Rust, copy freely)

**approach**: per texture per layer, accumulate quad vertices into a CPU-side vertex buffer.
one `write_buffer` + one draw call per (texture, layer) pair. existing atlas UV rects slot
in naturally. replaces the current per-tile `draw_sprite_atlas_on_layer` loop entirely.

---

## tier 2 — measurable per-frame cost

### A* pathfinding: eliminate per-query HashMap allocations

**file**: `crates/lunar-pathfinding-rt/src/lib.rs:139–175`

**problem**: every `find_path()` call does `HashMap::new()` twice — g_score and parent maps.
with 20 AI agents requesting paths in the same frame that's 40 heap allocations minimum.

**reference**: ioq3 `botlib/l_precomp.c` uses flat arrays indexed by area ID (reference only).
bevy's `bevy_pathmesh` crate (MIT) uses pre-allocated Vec-based open set.

**fix**: replace both HashMaps with `Vec<f32>` (size = node_count, initialized to `f32::MAX`)
and `Vec<u32>` for parent, stored as fields on a `PathfinderScratch` resource.
clear with `fill()` between queries. O(node_count) clear cost but no allocation, no hashing.
if node_count is large, track a dirty list and only clear touched nodes.

---

### animation: three separate per-frame inefficiencies

**files**: `crates/lunar-animation/src/lib.rs`

1. **`total_duration()` recomputed every call** (line 84–86): `frames.iter().map(|f| f.duration_secs).sum()` called multiple times per frame per entity. cache as a field on `AnimationClip`, set at construction and on any frame modification.

2. **linear frame search** (line 248–257): `frame_index_at()` scans frames until accumulated duration exceeds elapsed. precompute cumulative durations `[0.0, d0, d0+d1, ...]` in `AnimationClip`. use `partition_point` for O(log n) lookup.

3. **clip lookup by String hash every frame** (lines 192, 206): `animator.clips.get(clip_name)` hashes a string twice per entity per frame. cache the active clip as `current_clip_idx: Option<usize>`; switch to index lookup on clip change.

**reference**: doom-3 `MD5Anim::GetInterpolatedFrame` uses precomputed frame offsets (reference only). Fyrox `animation/src/machine/mod.rs` caches transition indices (MIT, Rust).

---

### particle system: trig LUT + scan-compact

**file**: `crates/lunar-particles/src/lib.rs`

1. **per-particle trig** (lines 224–227): `atan2` + `sin` + `cos` on every spawn = ~200 cycles/particle. `atan2(direction.y, direction.x)` is a property of the *emitter*, not the particle — precompute `base_angle` once per emitter in the emitter tick, not inside the particle spawn loop. `noise_offset.sin()` is already scalar and fine.

2. **swap-remove compaction** (lines 200–214): cache-hostile removal pattern. replace with scan-compact:
   ```rust
   let mut w = 0;
   for r in 0..pool.particles.len() {
       if !pool.particles[r].is_dead() {
           pool.particles[w] = pool.particles[r];
           w += 1;
       }
   }
   pool.particles.truncate(w);
   ```

**note**: GPU particle simulation already exists for the 3D path (see performance.md). these fixes are for the 2D CPU particle fallback path.

---

### sRGB conversion: powf → 256-entry LUT

**file**: `crates/lunar-image/src/simd.rs:173–187`

**problem**: `((s + 0.055) / 1.055).powf(2.4)` is ~15 cycles per byte. called at image load time.
a 512×512 RGBA image = 262K pixels × ~15 cycles ≈ 4M cycles (~2ms on a 2GHz CPU).

**fix**: precompute once at startup as a `[f32; 256]` via `OnceLock`:
```rust
static SRGB_LUT: OnceLock<[f32; 256]> = OnceLock::new();

fn srgb_lut() -> &'static [f32; 256] {
    SRGB_LUT.get_or_init(|| std::array::from_fn(|i| {
        let s = i as f32 / 255.0;
        if s <= 0.04045 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
    }))
}
```
then `output[i] = srgb_lut()[byte as usize]` — 1 cycle. same output, 15× faster.

**reference**: doom-3 `idImage::UploadCompressedData` uses a hardcoded gamma LUT (reference only).
godot `servers/rendering/renderer_rd/storage_rd/texture_storage.cpp` builds a LUT at init (MIT).

---

### premultiply alpha: float → integer math

**file**: `crates/lunar-image/src/simd.rs:141–156`

**problem**: `f32::from(chunk[0]) * (f32::from(chunk[3]) / 255.0)` — 1 float div + 3 float muls + 3 casts per pixel.

**fix**: integer multiply:
```rust
let a = chunk[3] as u32;
chunk[0] = ((chunk[0] as u32 * a + 127) / 255) as u8;
chunk[1] = ((chunk[1] as u32 * a + 127) / 255) as u8;
chunk[2] = ((chunk[2] as u32 * a + 127) / 255) as u8;
```
`u32 * u32` is 1 cycle on any modern CPU. no float pipeline stalls. result is within 1 ULP of the float version.

---

### spring arm: dirty-flag the raycast

**file**: `crates/lunar-camera-3d/src/lib.rs:111–121`

**problem**: `raycast_3d()` fires every frame even when neither the camera target nor any geometry has moved.

**fix**: store `last_arm_target: Vec3` and `last_arm_len: f32` on `SpringArm3d`. skip the raycast if target translation and desired arm length haven't changed by more than a small epsilon. update cached values on raycast. cost: two Vec3 comparisons per camera per frame instead of a full BVH traversal.

---

### merge transform + visibility tree walks

**files**: `crates/lunar-3d/src/visibility.rs:231–299`, `crates/lunar-3d/src/systems.rs`

**problem**: `propagate_transforms_3d` and `propagate_visibility` are separate DFS passes over the same entity hierarchy, maintaining separate scratch buffers with the same sort-by-depth order.

**fix**: run visibility propagation inside the transform propagation pass. after writing `WorldTransform3d` for an entity, immediately propagate the parent's inherited visibility to the child. one scratch, one sort, one pass.

**reference**: godot `scene/main/node.cpp` `_propagate_visibility_changed` is called from within the transform propagation path (MIT, reference pattern).

---

### spline arc-length: cache in PathFollower

**file**: `crates/lunar-spline/src/lib.rs:111–135`

**problem**: `sample_arc()` allocates `Vec::with_capacity(steps + 1)` and resamples the entire spline every call. called per path-following entity per frame.

**fix**: add `cached_lengths: Vec<f32>` and `cached_total: f32` to `PathFollower`. build once when the follower is created or when the spline changes. `sample_arc` becomes a pure lookup + lerp — no allocation, no resampling.

---

### behavior tree: persistent entity cache

**file**: `crates/lunar-ai/src/lib.rs:127–152`

**problem**: `tick_behavior_trees()` collects all `BehaviorTree` entities into a `Vec` every frame.

**fix**: maintain a `BehaviorTreeEntities(Vec<Entity>)` resource, updated by `Added<BehaviorTree>` and `RemovedComponents<BehaviorTree>` observers. `tick_behavior_trees` takes `Res<BehaviorTreeEntities>` — zero allocation in steady state.

---

### localization: avoid String alloc on key miss

**file**: `crates/lunar-localization/src/lib.rs:127–143`

**problem**: `get()` returns `.cloned().unwrap_or_else(|| key.to_string())` — allocates a new String on every cache miss. in development, missing keys are common.

**fix**: return `Cow<'_, str>`:
```rust
pub fn get<'a>(&'a self, key: &'a str) -> Cow<'a, str> {
    self.string_tables
        .get(&self.current_language)
        .and_then(|t| t.get(key))
        .map(|s| Cow::Borrowed(s.as_str()))
        .unwrap_or(Cow::Borrowed(key))
}
```

---

## tier 3 — quality-of-life perf (smaller, consistent gains)

- **`world_manifest.rs:241`**: scene lookup by name is a linear scan. add a `HashMap<String, usize>` index built when the manifest is loaded.
- **`world_manifest.rs:257–277`**: `chunks_in_bounds` and `chunks_in_radius` collect to Vec. return an iterator or accept `&mut Vec` for caller reuse.
- **`pathfinding-pre/src/lib.rs:82–99`**: Dijkstra bake uses `f32::to_bits()` for heap ordering. wrap in a newtype with `Ord` instead of bit-casting.
- **`assets/lib.rs:553–577`**: `drain_texture_results()` etc. allocate a new Vec every frame during loading. change signature to `drain_texture_results_into(&self, out: &mut Vec<...>)`.
- **`animation:207–253`**: `advance_animations` rebuilds a scratch Vec each frame. use a `Local<Vec>` to reuse across frames.

---

## tier 4 — far-term architectural work

already listed in `plans/performance.md` under "next / far term". reproducing here for completeness:

### render graph DAG
model on bevy's extract→prepare→queue→render→cleanup design. nodes record command buffers
in parallel. resource lifetimes computed from graph edges; transient attachments aliased.
**reference**: `bevy/crates/bevy_render/src/render_graph/` (MIT/Apache, copy freely — we already use bevy_ecs).

### GPU-driven culling + indirect drawing
compute shader outputs draw commands directly, skips CPU gather loop entirely.
gated behind `DownlevelFlags::INDIRECT_EXECUTION` (unavailable on GLES/WebGL2).
**reference**: o3de `Gems/Atom/RPI/Code/Source/RPI.Reflect/Culling/` (Apache 2.0).

### HZB two-pass occlusion culling
depth buffer → hierarchical mip chain → second compute pass occludes last-frame occludees.
**reference**: `ogre-next/RenderSystems/GL3Plus/src/OgreGL3PlusHardwareOcclusionQuery.cpp` (MIT).
godot `servers/rendering/renderer_rd/renderer_scene_render_rd.cpp` implements a software rasterizer occluder system (MIT, quite readable).

### LOD for non-terrain meshes
terrain already has clipmap LOD. regular `Mesh3d` entities have none.
`MeshLod` component: `[(distance_sq, mesh_handle)]` sorted near→far. LOD selection
happens in the cull pass (distance from camera to AABB center is already computed there).
**reference**: `ogre/OgreMain/src/OgreLodStrategy.cpp` + `OgreLodConfig.cpp` (MIT).
Fyrox `src/scene/mesh/mod.rs` has a level-of-detail field directly on `Mesh` (MIT, Rust).

### meshlet / virtualized geometry
blocked on wgpu mesh shader support. watch `wgpu` issue tracker.
**reference**: o3de `Gems/Atom/Feature/Common/Code/Source/Mesh/MeshFeatureProcessor.cpp` (Apache 2.0).

---

## reference engine index (where to look for what)

| topic | best reference | path | license |
|---|---|---|---|
| dynamic BVH | godot | `core/math/dynamic_bvh.h` | MIT |
| broad phase physics | godot | `servers/physics_3d/godot_broad_phase_3d_bvh.cpp` | MIT |
| sprite batching | godot | `servers/rendering/renderer_2d.cpp` | MIT |
| hardware instancing | ogre-next | `OgreMain/src/OgreInstanceManager.cpp` | MIT |
| LOD system | ogre | `OgreMain/src/OgreLodStrategy.cpp` | MIT |
| render graph | bevy | `crates/bevy_render/src/render_graph/` | MIT |
| GPU-driven culling | o3de | `Gems/Atom/RPI/Code/Source/RPI.Reflect/Culling/` | Apache 2.0 |
| animation blend tree | fyrox | `src/animation/machine/` | MIT |
| pathfinding flat arrays | bevy | `crates/bevy_pathmesh/` | MIT |
| sRGB/gamma LUT | godot | `servers/rendering/renderer_rd/storage_rd/texture_storage.cpp` | MIT |
| scene tree propagation | godot | `scene/main/node.cpp` | MIT |
| BSP + PVS | quake | `bspfile.h`, `world.c` | GPL (reference only) |
| clip hulls | quake | `world.c` | GPL (reference only) |
| MD5 animation sampling | doom-3 | `renderer/Model_md5.cpp` | GPL (reference only) |
