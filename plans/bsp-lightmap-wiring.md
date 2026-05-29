# bsp and lightmap wiring

these are the two missing wires between existing systems and the renderer.
both are self-contained changes to `lunar-render-3d`. no new crates needed.

---

## task 1 — wire bsp pvs into the renderer gather pass

### what it does

when `BspLevel` is loaded, the gather pass should filter ECS entities to only
those whose `Area` id is reachable from the camera leaf's PVS, replacing the
ECS BFS portal traversal for level geometry.

### acceptance criteria

- if `BspLevel::is_loaded()` returns false, the gather pass is unchanged (falls
  through to existing `VisibleAreas` + `BvhVisible` filtering)
- if loaded, the gather pass calls `camera_leaf(cam_pos)` and
  `visible_leaves(camera_leaf)` to build a `HashSet<u32>` of visible area ids
  for this frame
- entities with an `Area` component are only drawn if their area id is in that set
- entities without an `Area` component are always drawn (same as current behaviour)
- the `VisibleAreas` resource is updated to reflect the PVS result so portal-aware
  game code (e.g. AI line-of-sight queries) still reads a correct value

### where to change

**`crates/lunar-render-3d/src/lib.rs`**

in the `render()` call, after reading `cam_pos` and before the gather loop:

```rust
// read BspLevel from world; build area visibility set for this frame
let bsp_visible_areas: Option<std::collections::HashSet<u32>> = world
    .get_resource::<lunar_bsp::BspLevel>()
    .filter(|level| level.is_loaded())
    .map(|level| {
        let leaf = level.camera_leaf(cam_pos);
        let visible = level.visible_leaves(leaf);
        let area_map = level.area_map();
        let mut areas = std::collections::HashSet::new();
        for leaf_idx in visible {
            // binary search — area_map must be sorted by leaf_index at load time
            if let Ok(pos) = area_map.binary_search_by_key(&(leaf_idx as u32), |&(li, _)| li) {
                areas.insert(area_map[pos].1);
            }
        }
        areas
    });
```

note: `BspBlob::area_map` must be sorted by leaf_index when the blob is written.
add `area_map.sort_unstable_by_key(|(li, _)| *li)` in `compile_bsp` before
serialization. this makes per-frame lookup O(n log n) instead of O(n²).

in the gather filter closure:

```rust
.filter(|(entity, _, _, _, vis, aabb, _, _)| {
    if !vis.0 { return false; }
    if let Some(ref visible_areas) = bsp_visible_areas {
        if let Some(area) = entity_area {
            if !visible_areas.contains(&area.0) { return false; }
        }
    }
    aabb.is_none() || self.frustum_visible.contains(entity)
})
```

add `Option<&Area>` to the gather query. write result back to `VisibleAreas`.

### expected impact

indoor scenes: 80–95% reduction in draw calls submitted to the GPU. everything
downstream (shadow cascade cost, lighting, overdraft) scales down with it.

---

## task 2 — wire lightmaps into the pbr shader

### what it does

entities with a `Lightmap` component get their diffuse lighting replaced by a
precomputed baked texture, skipping runtime directional light evaluation for
those fragments. dynamic geometry (characters, projectiles) continues using
full PBR.

### acceptance criteria

- a mesh with `Lightmap { texture, intensity }` samples `uv_lightmap` and uses
  the result instead of the directional shadow + diffuse term
- ambient and point lights still contribute
- meshes without a `Lightmap` component behave exactly as before
- the renderer does not crash when some entities have lightmaps and others do not

### where to change

#### shader — `crates/lunar-render-3d/src/shader.wgsl`

add lightmap texture and sampler at group 4:

```wgsl
@group(4) @binding(0) var lightmap_tex: texture_2d<f32>;
@group(4) @binding(1) var lightmap_sampler: sampler;
```

add `has_lightmap: u32` to `MaterialUniforms`:

```wgsl
struct MaterialUniforms {
    base_color:   vec4<f32>,
    metallic:     f32,
    roughness:    f32,
    flags:        u32,
    has_lightmap: u32,
};
```

in the fragment shader:

```wgsl
if (material.has_lightmap != 0u) {
    let lm = textureSample(lightmap_tex, lightmap_sampler, in.uv_lightmap).rgb;
    out_color = vec4(lm * albedo.rgb + point_contribution + ambient_term, albedo.a);
} else {
    out_color = vec4(existing_pbr_result, albedo.a);
}
```

#### bind group layout

add `lightmap_bgl` (group 4: texture + sampler). create a 1×1 white fallback
bind group for entities without a lightmap. cache per-texture lightmap bind
groups in a `HashMap<u64, wgpu::BindGroup>` keyed by asset id.

update pipeline layout to 5 bind group layouts.

### expected impact

static geometry stops paying for directional light evaluation per fragment.
on a level where 80% of visible geometry is static, lighting pass cost drops
by ~80% for lit fragments.

---

## implementation order

BSP wiring first — pure gather-pass filter, no shader changes.
lightmap wiring second — changes shader and pipeline layout.

---

## caveats — not yet competitive after these two tasks alone

### developer discipline required

the BSP PVS filters at entity/area granularity — ECS entities with `Area`
components. if a game puts the entire level in one `Mesh3d` entity with no
area tags, none of it gets culled. quake 3 culled individual BSP faces; we
cull entities. the game developer must split level geometry by room/corridor
and tag it. this is a workflow requirement, not an engine bug, but it means
the engine cannot save a poorly-structured game.

### everything above is theoretical without profiling

no frametime numbers exist yet. there could be a wgpu overhead floor, a hash
map resize, or a GPU pipeline bubble that offsets the culling gains. the only
way to verify is to profile a real level under real conditions.

---

## full competitive analysis — quake 1, quake 3, doom 3, halo ce/3

after BSP wiring and lightmap wiring, the engine has the same structural
properties as those classic engines. but structural parity is not performance
parity. here is every remaining gap, organized by what it costs.

---

### cpu-side waste

#### A. no gpu instancing (incorrectly claimed as implemented)

`plans/accessibility-gap.md` states "GPU instancing: 500 identical objects = 1
draw call" as something we have. we do not. the draw loop issues one
`draw_indexed` per entity regardless of how many share the same mesh and
material.

for a scene with 200 identical tree trunks, that is 200 separate draw calls, 200
separate `set_bind_group` calls, 200 separate uniform buffer writes. quake 3 did
not need instancing (each BSP face is unique), but an ECS engine with entity-level
geometry absolutely does.

**fix**: after sorting `draw_scratch` by (mesh_id, mat_id), detect consecutive runs
with the same key. for runs longer than 1, upload all transform matrices into a
single `StorageBuffer` and issue one `draw_indexed` with `instance_count =
run.len()`. the vertex shader reads `@builtin(instance_index)` to select its
transform. requires adding a storage buffer binding to group 2 (per-mesh bind
group) for the transform array.

**impact**: reduces N draw calls to 1 for repeated geometry. outdoor scenes with
repeated assets (rocks, trees, props) go from hundreds of draw calls to tens.

#### B. gpu resource creation inside the draw loop

the current code checks `self.mesh_gpu.contains_key(&mesh_id)` inside the render
pass recording loop and calls `device.create_buffer` + `queue.write_buffer` inline.
buffer creation mid-frame is a major source of frametime variance — the first frame
any new mesh becomes visible causes a GPU allocation stall of variable duration.

**fix**: a separate **prepare phase** before any `RenderPass` opens. walk the
expected draw list, identify all missing mesh_gpu and texture_gpu entries, and
upload them all. the render pass recording phase must never create GPU resources.
after the prepare phase, render pass recording is purely state-setting and draw
calls — no allocations.

**impact**: eliminates frametime spikes when new entities enter the frustum (e.g.
rounding a corner that reveals a new room). frametime variance on steady-state
frames drops significantly.

#### C. per-frame vec allocs in scratch buffers

`raw_scratch`, `draw_scratch`, and `impostor_scratch` are cleared and refilled
every frame. if any frame has more entities than previous peak, Vec reallocates.
on the reallocation frame, the allocator is called mid-render, adding unpredictable
latency.

**fix**: after the first non-trivial frame, measure peak sizes and call
`reserve_exact(peak * 2)` once. never call `shrink_to_fit`. the vecs will plateau
at a fixed allocation and never reallocate again on steady-state scenes.

**impact**: eliminates allocator calls on steady-state frames. removes a category
of frametime variance that is currently invisible but triggered by level changes.

#### D. O(n²) area_map lookup in bsp wiring

the plan above already uses `binary_search_by_key` (O(log n) per leaf). ensure
`area_map` is sorted by leaf_index at blob write time in `compile_bsp`.

#### E. transparent depth sort algorithm

`draw_scratch.sort_unstable_by_key` is a full O(n log n) sort every frame. for
the opaque sublist this is fine since (mesh_id, mat_id) sort keys rarely change
between frames. for the transparent sublist, depth order can change every frame.

**fix**: track whether the transparent sublist changed since last frame (any entity
entered/exited, or camera moved significantly). if unchanged: skip the sort. if
changed: insertion sort (O(n) on nearly-sorted data) instead of full unstable sort.

#### F. shadow cascade rebuild spikes

when `shadow_cascade_dirty` fires for all 3 cascades simultaneously (sun moves,
many dynamic objects enter), the frame rebuilds 3 depth passes in sequence. that
frame is noticeably longer than its neighbours.

**fix**: rebuild at most 1 dirty cascade per frame, prioritizing cascade 0 (nearest,
highest detail, most visually impactful). cascades 1 and 2 wait for subsequent
frames. the 1-2 frame stale shadow on cascade 2 (far cascade, low detail) is
imperceptible.

**impact**: converts a triple-cascade spike into 3 consecutive single-cascade frames.
frametime variance becomes bounded and predictable.

---

### gpu-side waste

#### G. no texture compression (bc3/bc5/bc7)

we upload textures as RGBA8 (4 bytes/texel). BC3 (DXT5) stores the same data in
1 byte/texel for a 4:1 ratio. BC7 gives higher quality at the same ratio. BC5
stores two-channel normal maps in 1 byte/texel (vs 4 bytes uncompressed).

on a 2015 mid-range GPU (GTX 950, R9 380), texture cache is 512KB–2MB. a single
1024² RGBA8 diffuse texture is 4MB — well outside the cache. the same texture as
BC3 is 512KB, which fits comfortably. texture cache pressure is the primary
bottleneck for fragment shading on mid-range hardware.

**fix**: add BC3/BC5/BC7 support to `lunar-assets` and `lunar-render`. decode at
asset import time (offline, not at runtime) using `image` crate with DXT feature
or a dedicated compressor. store the compressed bytes in `Texture::data` and set a
`TextureFormat` field. in `lunar-render`, create the GPU texture with the
corresponding `wgpu::TextureFormat::Bc3RgbaUnorm` etc. and upload the compressed
bytes directly.

**impact**: 4× reduction in texture VRAM and bandwidth. fragment shading throughput
improves proportionally with texture fetch rate. this is arguably the single highest
bandwidth-per-effort improvement available.

#### H. hdr framebuffer format

the HDR intermediate render target is `RGBA16Float` — 8 bytes/texel. at 1920×1080,
that is ~16MB read and written by every post-processing pass. every bloom mip level,
composite pass, and SSR sample touches this buffer.

**fix**: use `R11G11B10Float` for the HDR target — 4 bytes/texel, same HDR range for
typical scene values (no negative components, values under ~65000 nits). half the
bandwidth. on `QualityPreset::Minimum`, skip the HDR target entirely and write
directly to the swapchain `RGBA8` surface with inline tonemap in a single pass.

**impact**: 2× reduction in post-processing bandwidth. on Minimum quality, the
intermediate target is eliminated entirely.

#### I. msaa bandwidth on mid-tier

4× MSAA doubles the memory bandwidth for color and depth targets and adds a
resolve pass. quake 3 and doom 3 had no MSAA. ensure `QualityPreset::Low` and
`QualityPreset::Minimum` set `msaa_samples = 1`. verify the render targets are
actually recreated at sample count 1 when quality changes (they may be created
once at engine init and never updated).

#### J. no gpu-driven indirect rendering

we read GPU cull results back to the CPU (1-frame pipeline) and then resubmit
draw calls from the CPU. this means the CPU still pays per-draw overhead for
every surviving entity.

**fix**: the GPU cull compute shader (`cull.wgsl`) writes `visible_flags[i]`.
extend it to also write a `DrawIndexedIndirect` buffer: for each visible entity,
write `{vertex_count, instance_count=1, first_index, base_vertex, first_instance}`
into an indirect buffer, and a running count of how many draws to issue. then
replace the CPU draw loop with a single `multi_draw_indexed_indirect` call (or
N individual `draw_indexed_indirect` calls if multi_draw is unavailable on the
target platform).

this eliminates the 1-frame lag entirely (GPU culls and GPU draws are in the same
frame) and reduces CPU-side draw submission to near-zero.

**blocks on**: wgpu feature `INDIRECT_FIRST_INSTANCE` (required for per-instance
data). available on dx12/metal/vulkan; not on all GL/GLES targets. add a capability
check and fall back to current CPU path on unsupported hardware.

**impact**: CPU draw submission cost drops from O(visible entities) to O(1).

#### K. no renderbundle for static level geometry

wgpu `RenderBundle` records a sequence of state-setting and draw commands once and
replays it with near-zero CPU overhead. static level geometry (walls, floors,
ceilings — anything with `BspLevel` area tags and no `LocalTransform3d` animation)
never changes between frames.

**fix**: at level load, after BSP wiring is set up, record all static entity draws
into a `RenderBundle`. each frame, replay it with one `execute_bundles()` call
instead of N individual draw calls. requires detecting which entities are static
(no `AnimationPlayer`, no physics body, `LocalTransform3d` not changing).

classic engines had display lists for exactly this reason. this is the modern
equivalent.

**impact**: static mesh CPU submission cost drops to near-zero. for an indoor level
where 90% of geometry is static, this eliminates most of the draw loop.

---

### frametime variance

#### L. no cpu-gpu overlap between frames

current frame loop: input → game logic → render → submit → wait for present.
the CPU idles while the GPU renders. at 60fps this idle is ~8ms (CPU finishes in
~8ms, GPU finishes in ~16ms if GPU-bound). that CPU time is wasted.

**fix**: classic double-frame pipelining. CPU begins frame N+1 game logic
immediately after submitting frame N to the GPU — no wait. requires double-buffering
all per-frame GPU buffers (globals, lights, per-mesh uniforms). wgpu's `StagingBelt`
already double-buffers uploads; the remaining work is ensuring per-frame uniform
buffers are ring-allocated with 2 slots.

**impact**: the CPU idle between frames is eliminated. at 60fps target, this
recovers ~8ms of CPU time per frame budget that can be used for more game logic or
richer physics.

#### M. input polling timing

input events are polled at frame start. game logic runs. frame is submitted and
presented. total input-to-display latency = 1 full frame + GPU render time.

**fix**: poll SDL input as late as possible — after the previous frame's GPU work
is submitted but before the current frame's physics step. on a well-pipelined
engine this means input captured at t = 0 is visible at t = 1 frame, not t = 2.

additionally: sample SDL input immediately before the camera transform is
computed, not at the top of the main loop. this shaves another ~4ms off
input-to-render latency by capturing the most recent joystick/mouse state.

**impact**: cuts input-to-display latency by up to 1 full frame. at 60fps that is
16.67ms — the difference between a game that feels responsive and one that feels
"floaty".

---

### correctness gaps that cause visual variance

#### N. no csm texel snapping (shadow shimmer)

csm shadows shimmer when the camera moves slightly because the shadow map texel
grid shifts in world space. this is a well-known artifact.

**fix**: for each cascade, round the projection matrix translation to the nearest
shadow map texel in world space. specifically: after computing the ortho projection
center, quantize it to `texel_size = frustum_extent / shadow_map_resolution`. this
is a 3-line fix to the cascade setup.

**impact**: eliminates shadow shimmer at no performance cost. zero-cost correctness
fix.

#### O. lightmap uv seam bleeding

the lightmap baker (`lunar-lightmap`) does not pad texels at UV island boundaries.
at mip level 1 and beyond, texels from adjacent UV islands bleed into each other,
producing visible seams.

**fix**: add a dilation pass after baking — for each background (unwritten) texel,
fill it with the nearest written texel's value. 2-pixel dilation eliminates seams
at up to mip level 1 (which halves linear size twice). implement as a simple flood-
fill or distance-transform pass after `BakeResult` is produced.

**impact**: eliminates lightmap seam artifacts on mip 1+. required for production-
quality lightmaps.

---

### structural gaps vs specific engines

#### vs quake 1 / quake 3

quake's BSP operated at face level — only individual BSP faces in visible leaves
were submitted for rendering. our BSP operates at entity level. a room's walls are
one entity; we cannot cull individual faces within it.

this gap only matters if individual entities are very large (e.g. a single mesh
covering an entire floor). the mitigation is to advise game developers to split
large surfaces by area rather than building one giant mesh. for well-structured
levels the entity-level granularity is fine.

quake 3 also sorted geometry by shader (material) for GPU state coherency and used
hardware T&L vertex strips, maximizing post-transform vertex cache reuse. we sort
by (mesh_id, mat_id) which achieves state coherency. vertex cache optimization (see
below) addresses the cache reuse gap.

#### vs doom 3

doom 3 used stencil shadow volumes — expensive, but bounded and predictable.
our CSM + dirty-flag approach is cheaper for typical scenes. we have an advantage
here once lightmaps are wired (static geometry pays no runtime shadow cost at all).

doom 3's area/portal system was more fine-grained than ours — individual rooms
were separate portal areas with explicit portal geometry. ours uses the same
concept but relies on the game developer to tag entities correctly. structurally
equivalent given correct usage.

#### vs halo ce

halo CE had separate render paths for BSP geometry (face-level, rendered as a
whole) and dynamic entities (object-level, rendered via a draw list). we treat
everything as draw-list entities. the performance difference matters only for
large static meshes — see quake 1 note above.

halo CE baked per-vertex irradiance for outdoor dynamic objects (characters in
outdoor areas pick up baked sky/sun irradiance from the nearest BSP cluster).
our dynamic objects use a single ambient term. this is a quality gap, not a
performance gap.

#### vs halo 3

halo 3 ran on Xbox 360 with a GPU-driven job system that was effectively GPU-
driven indirect rendering with SPU pre-processing. our GPU-driven indirect plan
(item J) achieves the same architecture on PC. the difference is that halo 3's
job system had near-zero CPU overhead for draw submission; ours still has wgpu
API call overhead per draw until indirect rendering is implemented.

halo 3's LOD system used alpha-dither blending between LOD levels (no pop). ours
has hard LOD transitions. this is a visual quality gap, not a performance gap, but
visible LOD pop signals low-quality to players.

---

### vertex throughput

#### P. no vertex cache optimization

mesh data is uploaded with arbitrary index ordering from the source GLTF file. the
GPU post-transform vertex cache (16–32 entries on modern hardware) is maximally
effective when adjacent triangles share vertices recently processed. arbitrary
ordering defeats this.

**fix**: run the Forsyth algorithm (or meshoptimizer's `meshopt_optimizeVertexCache`)
over each mesh's index buffer at upload time (once, in the prepare phase). this
reorders indices so the most recently processed vertices are reused as often as
possible.

**impact**: 20–40% reduction in vertex shader invocations for dense meshes. free
after the one-time reorder, which takes microseconds per mesh.

---

### adaptation

#### Q. no adaptive quality / frame budget enforcement

if a frame exceeds 16.67ms, nothing changes for the next frame. the engine has no
awareness of whether it is on budget. a game that ships and runs at 45fps on
minimum-spec hardware has no recourse other than the developer manually testing and
lowering settings.

**fix**: measure `frame_time_ms` each frame using a rolling average over 30 frames.
if the average exceeds 18ms (10% over 60fps budget) for 3 consecutive seconds,
step `QualityPreset` down by one. if the average is below 14ms (15% headroom) for
10 consecutive seconds, step up. expose `AutoQuality { enabled: bool, min:
QualityPreset, max: QualityPreset }` as a resource that games can configure.

**impact**: guarantees 60fps experience on minimum-spec hardware with graceful
degradation. halo 3 and later games shipped with similar auto-quality systems.

---

## verdict: when are we competitive?

given proper level authoring (area-tagged geometry, BSP compiled), after all
items above are implemented:

| engine | verdict |
|---|---|
| quake 1 | **competitive or better** — we have hardware the quake engine authors dreamed of; GPU culling, parallel recording, and instancing cover what BSP face-level rendering provided. |
| quake 3 | **competitive** — our architecture matches it structurally. the gap closes when instancing (A), vertex cache opt (P), and texture compression (G) are done. |
| doom 3 | **competitive or better** — lightmaps give us a structural advantage over doom 3's all-realtime shadow approach. the remaining gaps are overhead, not architecture. |
| halo ce | **competitive** — equivalent architecture after BSP wiring. halo CE's per-vertex outdoor irradiance is a quality gap we accept. |
| halo 3 | **competitive on indoor scenes** — GPU-driven indirect rendering (J) and RenderBundles (K) close the CPU submission gap. halo 3's SPU job system had lower overhead per-draw; we can match it with indirect rendering on modern wgpu. |

the engine becomes **unconditionally competitive** (any well-authored game, any
scene type) when items A, G, J, and K are done on top of BSP + lightmap wiring.
items B, C, H, L, M, N, P are the polish layer that puts us ahead.

none of this is achievable without profiling. build a reference indoor level,
instrument frametime, and verify each item's actual contribution. the analysis
above is architectural reasoning, not measured data.
