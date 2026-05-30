# sprint 8 — rendering maturity

seven items. closes the remaining visual quality gaps vs portal 2 / halo 3
and finishes the gpu-driven architecture begun in sprint 5. heavier than
sprint 7 — some items are multi-day.

---

## item A — gpu-driven LOD selection

### what and why

auto-LOD (sprint 7) generates the mesh levels. `MeshLod::select()` picks the
right level on the CPU in the draw_scratch gather loop: one distance-squared
comparison per entity per frame. fine for 200 entities. at 2000+ entities
(outdoor halo 3 scale) this is a measurable cpu cost — iterating a world query
to compute camera distance and selecting mesh handles.

fix: a compute pass writes one LOD index per entity to a storage buffer. the
draw_scratch gather reads from that buffer rather than computing distance itself.
removes the distance math and handle lookup from the hot gather loop.

### implementation

**`lod_select.wgsl` (new compute shader):**
```wgsl
struct LodParams {
    cam_pos:   vec3<f32>,
    entity_count: u32,
    thresholds: array<f32, 5>,  // max_dist_sq per LOD level (0 = base)
    _pad: array<f32, 3>,
}
@group(0) @binding(0) var<uniform>             params:      LodParams;
@group(0) @binding(1) var<storage, read>       positions:   array<vec4<f32>>;  // entity world centres
@group(0) @binding(2) var<storage, read_write> lod_indices: array<u32>;

@compute @workgroup_size(64)
fn cs_lod_select(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if i >= params.entity_count { return; }
    let dist_sq = distance_squared(positions[i].xyz, params.cam_pos);
    var lod: u32 = 4u;
    for (var l: u32 = 0u; l < 5u; l++) {
        if dist_sq <= params.thresholds[l] { lod = l; break; }
    }
    lod_indices[i] = lod;
}
```

the `positions` buffer is the same AABB centre buffer built for GPU culling —
no extra cpu upload needed.

**draw_scratch gather (lib.rs):** replace the `MeshLod::select(dist_sq)` call
with a read from `lod_indices[cull_soa_index]`. since cull results are already
read back this frame, the LOD index is available at the same time.

**new buffers:** `lod_params_buf`, `lod_indices_buf` (u32 per entity, same
capacity as cull_flags_buf). the compute dispatches alongside cull.

### files
- `crates/lunar-render-3d/src/lod_select.wgsl` (new)
- `crates/lunar-render-3d/src/lib.rs` (dispatch, buffers, gather change)

### win
- LOD selection cost: O(entity_count) cpu → O(1) cpu (reads a buffer)
- closes the LOD pipeline: generate offline, select on GPU, draw indirect

---

## item B — screen-space contact shadows

### what and why

pcf shadow maps have a minimum penumbra width determined by texel size —
objects resting on surfaces always show a visible "floating" gap in shadow
where the shadow map resolution is too coarse to capture the contact. reference
games (hl2, portal 2, doom 3) used a cheap screen-space raymarch under
objects to fill this gap.

this is a quarter-resolution post-pass that runs before the main lighting:
march a short ray downward in screen space from each fragment, compare depth
against the depth buffer, output a binary occlusion factor. the main shader
multiplies shadow by `(1 - contact_shadow)` for the final combined result.

very cheap: ~0.1ms at 1080p quarter-res, no geometry passes.

### implementation

**`contact_shadow.wgsl` (new compute/post shader):**
- input: depth buffer, view-space normals
- for each fragment: march 8 steps in view-space, step distance 0.05-0.5m
- compare marched depth to buffer depth — if occluded, increase shadow
- output: `Rg8Unorm` texture (one channel contact shadow, one channel AO blend)

**lib.rs:** new post-pass before lighting composition. the composite pass
reads `contact_shadow_tex` and applies it to the direct light contribution.

### files
- `crates/lunar-render-3d/src/contact_shadow.wgsl` (new)
- `crates/lunar-render-3d/src/lib.rs` (pass, texture, bind group)
- `crates/lunar-render-3d/src/composite.wgsl` (read contact shadow factor)

### win
- no more floating objects on lightmapped surfaces
- complements pcf shadows at contact distances they can't reach
- essentially free at quarter-res 8-step march

---

## item C — soft shadows (PCSS + ESM)

### what and why

our current directional shadows use a fixed 5×5 PCF kernel — hard penumbrae
at all distances. reference games post-2004 have soft shadows where the
penumbra width grows with blocker distance (a wall close to the shadow caster
gets a hard edge; a wall far away gets a wide soft penumbra). two formats:

**PCSS (percentage closer soft shadows)** for directional/CSM:
- pass 1: average blocker depth in a search radius around the sample point
- pass 2: scale the PCF kernel radius by `(receiver_depth - avg_blocker) / avg_blocker`
- result: near contact = hard shadow, far separation = soft penumbra

**ESM (exponential shadow maps)** for point lights:
- instead of hard depth comparison, store `exp(c * depth)` in the shadow map
- the texture can be linearly filtered (gaussian blur on cpu once at light update)
- in shader: `exp(c * receiver_depth) * sample < threshold` → smooth shadow
- replaces the hard `dist/radius` comparison in point_shadow.wgsl

both require no geometry changes — shader changes only.

### implementation

**`shader.wgsl` — PCSS path:**
```wgsl
fn pcss_penumbra_width(cascade: u32, receiver_depth: f32, uv: vec2<f32>) -> f32 {
    // 16-sample blocker search in a fixed world-space radius
    var blocker_sum = 0.0;
    var blocker_count = 0u;
    for (var i = 0u; i < 16u; i++) {
        let offset = poisson16[i] * BLOCKER_SEARCH_RADIUS;
        let blocker_depth = textureSample(shadow_map, ..., uv + offset, cascade);
        if blocker_depth < receiver_depth - SHADOW_BIAS {
            blocker_sum += blocker_depth; blocker_count++;
        }
    }
    if blocker_count == 0u { return 0.0; }
    let avg_blocker = blocker_sum / f32(blocker_count);
    return (receiver_depth - avg_blocker) / avg_blocker * LIGHT_SIZE;
}
```

then use `penumbra_width` to scale the PCF sampling radius.

**`point_shadow.wgsl` — ESM storage:**
store `exp(ESM_C * (dist / radius))` instead of raw linear depth. `ESM_C`
≈ 80 gives good precision without overflow for f32. write this from
`fs_point_shadow`.

**`shader.wgsl` — ESM comparison:**
replace `textureSampleCompare` with a regular sample + exponential comparison:
```wgsl
let esm_val = textureSample(point_shadow_maps, ...);
let shadow_factor = clamp(esm_val * exp(-ESM_C * dist_norm), 0.0, 1.0);
```

optionally: a 3×3 gaussian blur on the point shadow faces after rendering
(one compute pass per dirty face). cheap since 512² textures.

**DevRenderProfile:** add `soft_shadows: bool` (default false on standard,
true on full). PCSS has a small perf cost (~0.2ms for 3 cascades), ESM is
effectively free.

### files
- `crates/lunar-render-3d/src/shader.wgsl` (PCSS directional, poisson disk constants)
- `crates/lunar-render-3d/src/point_shadow.wgsl` (ESM storage)
- `crates/lunar-render-3d/src/lib.rs` (DevRenderProfile flag, optional blur pass)

### win
- closes the visual quality gap on shadow realism vs all reference games
- point light shadows stop looking like hard cubes at grazing angles
- penumbra width physically correct: contact = sharp, separation = soft

---

## item D — per-volume ambient light probes

### what and why

we have one global `IrradianceSH` resource. dynamic objects anywhere in the
scene receive the same ambient — a character in a dark cave gets the same sky
ambient as one standing outside. halo 3 / hl2 divide the world into volumes,
each capturing its own SH probe. dynamic objects use the probe for their
current volume.

approach: a uniform 3D grid of SH probes at configurable spacing (default 4m).
each probe stores the same 9 L2 coefficients as `IrradianceSH`. at render time,
the renderer finds the grid cell containing the entity origin and uploads those
coefficients to the per-entity uniform. the shader already evaluates SH for
ambient — no shader change needed, just different coefficient values per entity.

### implementation

**`crates/lunar-3d/src/light.rs`** — new resource:
```rust
pub struct AmbientProbeGrid {
    /// world-space origin of the grid (corner)
    pub origin: Vec3,
    /// spacing between probes in world units
    pub cell_size: f32,
    /// grid dimensions (x, y, z)
    pub dims: [u32; 3],
    /// packed SH data: dims.x * dims.y * dims.z * 9 coefficients * 3 channels
    pub coefficients: Vec<f32>,
}

impl AmbientProbeGrid {
    /// look up the SH coefficients for a world position.
    /// clamps to grid bounds (no extrapolation).
    pub fn sample(&self, pos: Vec3) -> [[f32; 3]; 9] { ... }
}
```

**`crates/lunar-render-3d/src/lib.rs`:**
when `AmbientProbeGrid` is present as a world resource, for each entity in
draw_scratch, call `grid.sample(entity_world_pos)` and write those coefficients
into the entity's uniform slot instead of the global `IrradianceSH` value.

fall back to global `IrradianceSH` when no grid is loaded (backwards compatible).

**offline baking:** a `tools/bake-probes/` tool that places a camera at each
grid point, captures a cubemap (using the renderer or a simple raycast), and
projects it to SH coefficients. this is the most expensive part but runs
offline. alternatively, game code can populate `AmbientProbeGrid` manually
(hand-authored per-room values work well for indoor games).

### files
- `crates/lunar-3d/src/light.rs` (AmbientProbeGrid resource)
- `crates/lunar-3d/src/lib.rs` (re-export)
- `crates/lunar-render-3d/src/lib.rs` (per-entity probe lookup in draw_scratch)
- `tools/bake-probes/` (optional offline bake tool)

### win
- dynamic objects correctly lit by their environment (dark cave vs sunlit field)
- closes the halo 3 ambient accuracy gap
- backwards compatible — global IrradianceSH still works when no grid loaded

---

## item E — custom anti-aliasing (TBD)

replacing FXAA with a higher-quality AA solution. TAA was considered but
rejected — the temporal blend blurs the full frame even at low weights, and
thin geometry (fences, wires, powerlines) becomes wispy or flickers as the
Halton jitter shifts sub-pixel coverage between frames. approach TBD.

motion vectors (`PrevWorldTransform3d` from sprint 7 already provides prev
model matrices) are a likely prerequisite regardless of final approach and
may be implemented as a standalone item before E is finalized.

---

## item F — planar reflections

### what and why

our water shader uses SSR for reflection. SSR only captures geometry visible
on screen — at glancing angles where the reflected content is mostly off-screen,
the reflection disappears. HL2's water reflected the full scene using a planar
reflection: render the entire scene from a camera mirrored about the water plane,
feed the result as a texture to the water shader.

this is the most expensive item in the sprint: one full render pass of the
scene per reflection plane visible this frame (usually 1-2). justified because
it's the single most visible quality gap for outdoor water.

### implementation

**`PlanarReflector` component:**
```rust
pub struct PlanarReflector {
    /// reflection plane normal (usually Vec3::Y for water)
    pub normal: Vec3,
    /// max render distance for the reflected scene
    pub clip_dist: f32,
    /// resolution divisor: 1 = full, 2 = half-res (default 2)
    pub resolution_divisor: u32,
}
```

**reflection pass (lib.rs):**
before the main color pass, for each visible `PlanarReflector` entity:
1. compute the reflected camera: flip position and forward vector about the
   plane, keeping up-vector consistent. adjust near plane to the reflection
   plane (oblique clip)
2. render the scene from the reflected camera into a `reflection_tex` (HDR,
   half resolution)
3. skip rendering the reflection entity itself in this pass (avoid recursion)
4. bind `reflection_tex` in the water shader (group 2 binding)

**`water.wgsl`:** add `reflection_tex` sampler binding. blend planar reflection
with the current SSR result: at glancing angles (low dot(V, N)) use planar
reflection; at steep angles (looking straight down) use SSR or refraction.
fresnel factor drives the blend naturally.

**clip plane:** the reflected camera must use an oblique projection matrix to
clip geometry below the water plane (avoids underwater geometry appearing in the
reflection). standard oblique near-plane technique.

**limit:** at most 2 reflection planes rendered per frame (configurable). if
more planes are visible, only the two largest by screen area are reflected.

### files
- `crates/lunar-3d/src/mesh.rs` (PlanarReflector component)
- `crates/lunar-render-3d/src/lib.rs` (reflection pass, reflection texture,
  oblique clip matrix)
- `crates/lunar-render-3d/src/water.wgsl` (reflection_tex binding + blend)

### win
- water reflection quality: SSR-only → full planar reflection with SSR fallback
- closes the hl2 water quality gap
- mirrors also become possible (same component, vertical plane)

---

## item G — detail sprite renderer

### what and why

outdoor scenes in hl2, portal 2, halo 3 cover ground with hundreds of small
billboarded sprites — grass blades, pebbles, small flowers. these aren't static
meshes (too many to instance individually) and aren't particles (they don't
move). they're a dedicated detail density system: a density map drives how many
sprites appear per square meter, they're clustered into chunks, rendered with
one instanced draw call per chunk.

without this, any outdoor ground surface looks bare regardless of how good the
textures are.

### implementation

**`DetailDensity` component:**
```rust
pub struct DetailDensity {
    /// detail sprite texture (atlas with multiple sprite variants)
    pub texture: Handle<Texture>,
    /// density map: r channel = sprites per m², 0-1 normalized
    pub density_map: Handle<Texture>,
    /// world-space size of the density map in meters
    pub world_size: Vec2,
    /// max distance at which sprites render
    pub max_dist: f32,
    /// sprite height range [min, max] in world units
    pub size_range: [f32; 2],
    /// number of sprite variants in the texture atlas (horizontal strip)
    pub variants: u32,
}
```

**GPU generation:** a compute pass reads the density map and the camera
position, generates instance data (position, scale, variant, rotation) for
all sprites within `max_dist`. uses a deterministic hash of grid position as
the random seed so sprites don't jitter as the camera moves.

output: one instance buffer per `DetailDensity` entity, updated when the
camera moves more than one chunk width.

**rendering:** one `draw_indirect` call per `DetailDensity` entity using the
generated instance buffer. vertex shader: billboard (align quad to camera),
scale by instance size, offset by instance position. fragment shader: alpha
test from atlas sample (discard below 0.5 alpha — no alpha blending needed).

**LOD:** at max_dist × 0.7, linearly reduce density in the compute pass.
no separate LOD meshes needed.

### files
- `crates/lunar-3d/src/mesh.rs` (DetailDensity component)
- `crates/lunar-render-3d/src/detail_sprite.wgsl` (compute + render shaders)
- `crates/lunar-render-3d/src/lib.rs` (pass, instance buffers, draw)

### win
- outdoor environments no longer look barren
- closes the hl2 / portal 2 outdoor ground quality gap
- zero CPU overhead — fully GPU driven after initial buffer generation

---

## recommended order

1. **A (GPU LOD selection)** — completes sprint 7 pipeline, low risk
2. **B (contact shadows)** — 2-3 hours, standalone shader change
3. **C (soft shadows)** — shader work only, high visual payoff
4. **D (ambient probe grid)** — medium, no shader changes needed
5. **E (custom AA)** — TBD; motion vector pass can be done independently first
6. **F (planar reflections)** — full render pass; doesn't depend on E
7. **G (detail sprites)** — standalone new system, do last or in parallel with F

---

## not in this sprint

**visibility buffer / nanite-style** — requires a full architecture change
(geometry pass writing triangle IDs, compute shading pass). sprint 9+ at earliest.

**temporal upscaling** — natural follow-up once the AA approach is settled. sprint 9+.

**skeletal animation GPU skinning** — depends on state of animation system.
if bone matrix computation is CPU-bound, a compute skinning pass would help.
assess after sprint 8.

**volumetric clouds** — substantial new system. sprint 9+.
