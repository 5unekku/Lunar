# renderer sprint 5 — visual feature gaps

cpu render overhead is no longer the bottleneck. the remaining gaps vs reference
games are visual features that didn't exist yet, not performance problems.

---

## gap summary

| feature                    | closes gap vs           | effort |
|----------------------------|-------------------------|--------|
| point light shadows        | doom 3, hl2, halo 3     | high   |
| clustered forward lighting | doom 3, halo 3          | high   |
| surface shader system      | quake 3                 | high   |
| directional lightmaps      | half-life 2             | medium |
| texture streaming          | hl2, halo 3             | medium |

---

## item A — point light shadows

### the gap

our 8 point lights are unshadowed. doom 3's entire visual identity is the
flashlight casting a hard shadow as you sweep it across a corridor. hl2's
flashlight is the same. halo 3's outdoor lights cast real shadows. without this,
no corridor horror game, no moody indoor scene.

### approach: cube shadow maps

each shadowed point light gets one `texture_cube<f32>` depth map (6 faces).
at shadow-render time, the scene is rendered 6 times into the cube faces
using a 90° perspective projection from the light's position.
in the fragment shader, `textureSampleCompare(cube_shadow, direction)` gives
the shadow factor for a surface point.

practical constraints:
- limit shadowed point lights to 4 (arbitrary, but 4 cube maps × 6 faces = 24
  extra render passes per frame — already expensive at 1K per face)
- other point lights remain unshadowed (acceptable — area lights rarely cast
  hard shadows anyway)
- ShadowCaster flag already exists — add a `ShadowCasting` variant to PointLight
  or a separate `ShadowedPointLight` component

implementation steps:

**step 1: shadow cube map array**
allocate `texture_cube_array<f32>` with 4 slots × 6 faces × `shadow_res × shadow_res`
depth texels. reuse shadow_res from QualitySettings.

**step 2: shadow pipeline**
new render pipeline for cube face rendering:
- vertex shader: same as current shadow (transform by light view-proj)
- 6 view-projection matrices per light (one per face, 90° fov, near=0.05, far=light.radius)
- fragment shader: depth only (no color output, same as CSM)

**step 3: shadow pass**
for each of the 4 shadowed point lights, for each of 6 faces:
  - frustum cull against the face frustum (roughly half-space test)
  - render visible shadow casters into that face

dirty-flagging: a cube face is only re-rendered when a shadow caster moves
within the light's radius or the light itself moves. lights with `casts_shadows`
enabled pay the cost; others pay nothing.

**step 4: shader sampling**
in `shader.wgsl`, add group 3 binding for `texture_cube_array` + comparison sampler.
in the point light loop: if `light.shadow_index != 0xff`, sample the cube array:
```wgsl
let shadow_dir = in.world_pos - light.position;
let shadow_factor = textureSampleCompare(
    point_shadow_maps, point_shadow_sampler,
    shadow_dir, i32(light.shadow_index), dist / light.radius - bias
);
```

**step 5: DevRenderProfile flag**
add `point_light_shadows: bool` to DevRenderProfile (default false, since most
games don't need it — doom-style games insert `with_point_light_shadows(true)`).

### files
- `crates/lunar-render-3d/src/lib.rs` — cube map allocation, shadow pass loop
- `crates/lunar-render-3d/src/shadow.wgsl` — repurpose or new entry point for cube faces
- `crates/lunar-render-3d/src/shader.wgsl` — cube shadow sampling in point light loop
- `crates/lunar-3d/src/light.rs` — add `casts_shadows: bool` to PointLight (and SpotLight)

---

## item B — clustered forward lighting

### the gap

we hard-cap at 8 point lights in a UBO. doom 3 levels have 20-40 dynamic
lights visible at once. halo 3 outdoor areas had 15-20. above 8 we currently
just drop them silently.

### approach: clustered shading (view-frustum 3D grid)

divide the view frustum into a 3D grid of clusters (16×9×24 = 3456 clusters
for a typical 1920×1080 view). for each cluster, store which lights affect it.
the fragment shader looks up its cluster by screen position + depth and iterates
only the lights for that cluster.

implementation steps:

**step 1: light assignment compute pass**
before the color pass, dispatch a compute shader that:
- takes the full light list (up to 256 lights)
- for each light, tests which clusters its sphere overlaps
- writes a per-cluster light index list to a storage buffer

cluster grid: `CLUSTER_X × CLUSTER_Y × CLUSTER_Z` (e.g. 16×9×24).
output: `cluster_offsets[cluster_i]` + `cluster_light_indices[]` (compacted list).

**step 2: shader-side cluster lookup**
in fragment shader, compute cluster index from `gl_FragCoord.xy` + `view_depth`,
then iterate `cluster_light_indices[offset..offset+count]`.

**step 3: expand light UBO → light storage buffer**
current `Lights` uniform has 8 point lights hardcoded.
replace with `var<storage, read> light_list: array<PointLightGpu>` (up to 256).
same pattern as the material storage buffer change in sprint 4.

**step 4: DevRenderProfile.max_point_lights**
add `max_point_lights: u32` (default 8 for classic/standard, 256 for full).
gate clustered pass behind `has_indirect` (compute shaders required).

### expected outcome
doom 3 / halo 3 style scenes with 20-40 dynamic lights work at normal cost.
the 8-light UBO path stays as fallback for low/mid tier (no compute).

### files
- `crates/lunar-render-3d/src/cluster.wgsl` — new light assignment compute shader
- `crates/lunar-render-3d/src/shader.wgsl` — cluster lookup in fragment shader
- `crates/lunar-render-3d/src/lib.rs` — cluster buffer, compute pass, light storage

---

## item C — surface shader system

### the gap

quake 3 levels are alive because of surface shaders: scrolling lava, pulsing
energy fields, deforming terrain, animated sky rotations, additive glow layers.
without this, a game that "looks like q3" is just flat static walls.

### approach: material stage pipeline

a `SurfaceShader` asset replaces the single `MaterialData` for surfaces that
need it. it describes N stages (up to 4), each with:
- `texture: Handle<Texture>` — the texture for this stage
- `blend: BlendMode` — Opaque, Add, Multiply, AlphaBlend
- `uv_transform: UvTransform` — scroll, rotate, scale (animatable)
- `tc_gen: TcGen` — Base, Environment, Lightmap
- `alpha_gen: AlphaGen` — Identity, Vertex, Const(f32)
- `vertex_deform: Option<VertexDeform>` — Wave, Bulge, None

the renderer evaluates these on the CPU (UV transforms per-frame) and passes
stage data as a uniform. the fragment shader blends stages in sequence.

this is NOT a GPU shader compilation system (no runtime GLSL/WGSL compilation).
it's a fixed-function multi-stage blender in the existing shader.

**implementation note**: surface shaders only apply when `ShadingModel::Unlit`
is used (no PBR interaction). they are a separate material type, not an
extension of MaterialData.

### files
- `crates/lunar-3d/src/surface_shader.rs` — new types
- `crates/lunar-render-3d/src/lib.rs` — stage evaluation, multi-pass draw
- `crates/lunar-render-3d/src/surface.wgsl` — new shader for surface stages

---

## item D — directional lightmaps

### the gap

hl2's radiosity normal mapping bakes a dominant light direction per lightmap
texel. when a dynamic flashlight hits a wall, it interacts with the stored
normal information and looks correct. our flat lightmaps only store intensity —
dynamic lights can't "know" the wall's local normal from the baked data.

### approach: dominant-direction lightmap baking

in `LightmapBaker`, for each texel, store:
- `irradiance: vec3` (existing, rgb intensity) → compressed to luma if space tight
- `dominant_dir: vec3` — normalized direction of the strongest light source
  contributing to this texel (in world space or tangent space)

baked into a second RGBA8 texture (dominant_dir packed as rgb, irradiance as a) 
or two separate textures (irradiance RGBA8 + direction RGBA8).

in the shader, when a dynamic light hits a lightmapped surface:
```wgsl
let lm_irr = textureSample(lightmap_tex, ...).rgb;
let lm_dir = textureSample(lightmap_dir_tex, ...).rgb * 2.0 - 1.0; // unpack
// blend static baked irradiance with dynamic contribution
// where dynamic light direction differs from baked direction, add specular
```

this is a strict improvement over flat lightmaps with no change to the existing
lightmap UV pipeline, and is backwards-compatible (flat lightmaps continue to work).

### files
- `crates/lunar-lightmap/src/baker.rs` — bake dominant direction alongside irradiance
- `crates/lunar-lightmap/src/lib.rs` — new `DirectionalLightmap` component
- `crates/lunar-render-3d/src/shader.wgsl` — sample dir lightmap when available
- `crates/lunar-render-3d/src/lib.rs` — second lightmap texture binding

---

## item E — texture streaming

### the gap

`MipStreamingConfig` and `TextureVramUsage` types already exist but streaming
is not implemented. hl2 and halo 3 streamed mip levels to stay within VRAM budget.
without it, all textures at all mip levels load at startup, which caps world size.

### approach: async mip streaming

the AssetServer already has `Handle<Texture>` and loading states. extend it to:
1. load only mip 0 (lowest res) at startup
2. track per-texture "desired mip level" based on screen-space coverage (how many
   pixels the surface occupies this frame)
3. async-load higher mips when desired > current, evict lower-priority mips when
   over VRAM budget
4. GPU-side: partial mip upload — upload only the new mip level into the existing
   texture's mip chain via `write_texture` with `mip_level` set

this is primarily an asset/streaming concern, not a renderer concern. the renderer
already supports mip-levelled textures — the streaming is handled in `AssetServer`
and `MipStreamingConfig`.

### files
- `crates/lunar-assets/src/lib.rs` — mip streaming implementation
- `crates/lunar-render-3d/src/lib.rs` — report screen-space coverage per texture
  (via draw_scratch, surface area ÷ distance²)

---

## recommended order

1. **D (directional lightmaps)** — self-contained, medium effort, direct quality improvement
   for any game using lightmaps. no new passes, no new pipelines.
2. **A (point light shadows)** — closes the doom 3 / hl2 gap. standalone new pass.
   highest visual impact for games that need it.
3. **B (clustered forward)** — prerequisite for scenes with many lights. depends on
   having a reason to use 20+ lights, which item A makes more common.
4. **C (surface shaders)** — large scope, only relevant for q3-style games. defer until
   there is a game that specifically needs it.
5. **E (texture streaming)** — defer until a game hits VRAM limits. infrastructure exists,
   trigger when needed.
