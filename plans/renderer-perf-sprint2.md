# renderer perf sprint 2

picking up the remaining items from `plans/bsp-lightmap-wiring.md` that are
self-contained changes to existing crates. items requiring new tooling or
significant architectural work are documented and deferred.

---

## status of bsp-lightmap-wiring.md items

| item | status |
|---|---|
| A — GPU instancing | **done** — storage buffer + instance range already in place |
| B — prepare phase | **done** — mesh/lightmap upload already before RenderPass |
| C — vec pre-sizing | done last sprint |
| D — area_map binary search | done last sprint |
| E — transparent sort skip | **this sprint** |
| F — cascade amortization | done last sprint |
| G — texture compression BC3/BC5/BC7 | deferred (needs offline toolchain) |
| H — HDR format R11G11B10Float | deferred (capability check + min-quality path rewrite) |
| I — MSAA off on Minimum/Low | **this sprint** |
| J — GPU-driven indirect | deferred (large compute shader rewrite) |
| K — RenderBundle for statics | deferred (needs static entity marker + lifecycle hooks) |
| L — CPU-GPU pipelining | deferred (double-buffer all per-frame uniforms across main loop) |
| M — input polling timing | deferred (SDL main loop restructure) |
| N — CSM texel snapping | done last sprint |
| O — lightmap UV seam dilation | **this sprint** |
| P — vertex cache optimization | **this sprint** |
| Q — adaptive quality | **this sprint** |

---

## item I — MSAA from QualitySettings

### problem

`msaa_samples` in the renderer is hardcoded to derive from `RenderTier` (Low=1,
Mid/High=4). `QualitySettings::minimum()` correctly sets `msaa_samples=1` but
the renderer ignores it; the resource is created after the render targets.

### fix

create `QualitySettings` first in `from_surface`, then derive `msaa_samples`
from `quality.msaa_samples`. the `QualitySettings` resource inserted into the
app already has the correct value. this aligns the renderer's internal state
with the resource.

```rust
// before pipeline and render target creation:
let quality = QualitySettings::from_tier(render_tier);
let msaa_samples = quality.msaa_samples;
// then insert quality into Self at the end
```

### acceptance criteria

- `QualityPreset::Minimum` and `QualityPreset::Low` produce render targets with
  `sample_count = 1`
- `QualityPreset::Medium` and above use `sample_count = 4` as before

---

## item E — transparent sort skip

### problem

`transparent_scratch.sort_unstable_by` runs every frame even when no transparent
entities moved. for typical scenes with a few glass surfaces, this is a small but
unnecessary O(n log n) sort every frame.

### fix

track `transparent_depth_key: Vec<i32>` alongside `transparent_scratch` — a
sorted list of quantized depth keys from the previous frame. if the set of
transparent entities and camera position haven't changed (same count, same entities
at same quantized depths), skip the sort.

simpler approach: track the camera's forward dot product with each transparent
entity's translation. if all match the previous frame's values within a threshold,
skip the sort. if camera or any transparent entity moved, re-sort.

```rust
// stored state:
transparent_last_cam_fwd: Vec3,
transparent_last_depths: Vec<i32>, // quantized to 1mm buckets

// each frame: build depth key list, compare to last
// if identical AND same count: skip sort
// else: sort and update
```

### acceptance criteria

- scenes where no transparent entity moved and camera didn't move: zero sort calls
- scenes where camera moved or a transparent entity moved: sort runs as before

---

## item O — lightmap UV seam dilation

### problem

the lightmap baker writes pixels only to texels covered by a UV triangle. texels
at island boundaries are left as transparent black (alpha=0). at mip level 1 and
above, GPU bilinear filtering samples these unwritten texels and blends them with
adjacent written texels, producing visible dark seams.

### fix

after baking, flood-fill unwritten texels (alpha=0) with the nearest written texel
(alpha=255) value. 2 pixels of dilation covers mip 1 seams (which halves linear
dimension twice).

```
// in BakeResult::dilate() or called automatically in LightmapBaker::bake()
for 0..dilation_radius:
    for each unwritten texel:
        check 4-connected neighbours; if any is written, copy its color
```

a single-pass flood fill (scanning all unwritten texels each dilation step) is
O(n × width × height) where n is the dilation radius. for n=2 and 512² textures,
that's 2 × 262144 = ~500k operations, which is negligible.

### acceptance criteria

- baked lightmaps show no dark seam artifacts when viewed with mip filtering
- written texels are not modified by dilation
- the dilation runs by default (n=2 pixels) unless the baker is configured with
  `with_dilation(0)` to disable it

---

## item P — vertex cache optimization

### problem

mesh index buffers are uploaded in the order they came from the GLTF exporter.
the GPU post-transform vertex cache (16–32 entries) is maximally effective when
adjacent triangles share recently-processed vertices. arbitrary index order gives
~50% cache hit rate; optimized order gives ~90%.

### fix

implement the Forsyth algorithm at mesh upload time (once per mesh, in
`upload_mesh_data`). the algorithm reorders indices to maximize vertex cache reuse.
it's O(n × k) where n = number of triangles, k = indices per triangle = 3.

```
fn forsyth_optimize(indices: &[u32], vertex_count: usize) -> Vec<u32>
```

the algorithm:
1. build per-vertex adjacency lists (which triangles reference each vertex)
2. score each vertex by cache position (recently used = high score)
3. greedily pick the highest-scored triangle, emit its indices, update vertex scores

implemented in ~100 lines in the build crate; no external dependency needed.

### acceptance criteria

- mesh index buffers are Forsyth-reordered before GPU upload
- vertex cache ACMR (average cache miss rate) for test meshes improves from ~0.5
  to ~0.2 on typical geometry
- mesh data in `MeshRegistry` is not modified (only the GPU-side buffer is reordered)

---

## item Q — adaptive quality

### problem

a game running at 45fps on minimum-spec hardware has no mechanism to automatically
drop quality further. the player experience degrades silently. the engine needs a
feedback loop.

### fix

extend `tick_dynamic_resolution` to also step `QualityPreset` up or down based
on the same EMA. use separate time constants to avoid flapping:
- step **down** after 3 consecutive seconds over budget (60 frames ≥ 18ms EMA)
- step **up** after 10 consecutive seconds under budget (600 frames ≤ 14ms EMA)

add `AutoQuality { enabled: bool, min: QualityPreset, max: QualityPreset }` resource.
when quality changes, apply the new settings to the renderer (shadow resolution,
bloom, post-process passes).

```rust
#[derive(Resource)]
pub struct AutoQuality {
    pub enabled: bool,
    pub min: QualityPreset,
    pub max: QualityPreset,
}
```

the down-step fires faster than the up-step to prefer stability over visual quality
and avoid flapping.

### acceptance criteria

- with `AutoQuality { enabled: true, min: Minimum, max: High }`, a scene running
  over budget for 3 seconds steps down one quality level
- a scene running under budget for 10 seconds steps up one quality level
- quality never goes below `min` or above `max`
- `AutoQuality { enabled: false }` leaves quality untouched

---

## implementation order

1. I (MSAA) — prerequisite to nothing, trivial, ensures quality settings are correct
2. E (transparent sort skip) — pure optimization, isolated
3. O (seam dilation) — isolated change to lunar-lightmap
4. P (vertex cache) — isolated change to mesh upload in lunar-render-3d
5. Q (adaptive quality) — builds on existing EMA infrastructure

---

## deferred items with estimated scope

**G — texture compression (BC3/BC5/BC7)**
requires: `image` crate DXT feature or `texpresso` crate; new `TextureFormat` enum
variant in `lunar-assets`; `lunar-render-3d` create_texture with compressed format.
estimate: 2-3 days. the offline compression step could live in `lunar-assets` build
script or a dedicated `lunar-asset-compress` crate.

**H — HDR format R11G11B10Float**
requires: wgpu capability check (`Features::RG11B10UFLOAT_RENDERABLE`); fallback
to RGBA16Float when unsupported; for Minimum quality, skip intermediate HDR entirely
and write straight to swapchain RGBA8 with inline tonemap.
estimate: 1 day.

**J — GPU-driven indirect**
requires: `cull.wgsl` writes `DrawIndexedIndirect` buffer; CPU loop replaced by
`draw_indexed_indirect`; per-mesh `IndexedIndirectArgs` staging buffer; wgpu feature
check for `INDIRECT_FIRST_INSTANCE`.
estimate: 3-4 days.

**K — RenderBundle for static geometry**
requires: `Static` marker component; system to detect when static set changes and
re-record bundle; separate static/dynamic draw lists.
estimate: 2 days.

**L — CPU-GPU pipelining**
requires: ring buffer for all per-frame GPU buffers (globals, material, entity);
submit and immediately begin next frame's game logic; present signal replaced by
buffer fence.
estimate: 3-4 days.

**M — input polling timing**
requires: SDL3 event loop restructure; input polled between frame submit and physics
step rather than at loop start.
estimate: 1 day.
