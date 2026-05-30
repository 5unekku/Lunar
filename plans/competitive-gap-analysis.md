# competitive gap analysis

reference targets: quake 1, quake 3, doom 3, half-life 2, portal 2, halo ce, halo 3.
written after sprint 5 completes. this is NOT a sprint — it's a living reference for what
still separates us from each target's full visual and performance ceiling.

---

## where we stand (current)

feature parity achieved:
- lightmapped indoor rendering (q1/q3 level) ✓
- surface shader system (q3 animated surfaces) ✓
- directional lightmaps (hl2 radiosity) ✓
- csm directional shadows + soft shadows (pcss + soft point pcf) ✓
- point light cube shadows (doom 3 flashlight) ✓
- clustered forward lighting up to 256 lights ✓
- ssao, bloom, ssr, volumetric fog ✓
- fxaa, msaa up to 8×, staa ✓
- gpu-driven indirect rendering ✓
- hzb occlusion culling ✓
- pvs offline bake + runtime bsp portal culling ✓
- lightmap baker (`lunar-lightmap`) wired into renderer ✓
- auto-lod generation tool (`gen-lods`) ✓
- gpu-driven lod selection ✓
- ambient light probe grid ✓
- detail sprites, planar reflections ✓
- contact shadows, motion vectors ✓
- mip streaming infrastructure ✓
- spirv pre-compilation via build.rs ✓
- bc1/bc6h/bc7 texture compression ✓
- vertex quantization (60→32 bytes/vertex) ✓

---

## remaining gaps by game

### quake 1 (1996)
nothing left. we exceed it on every axis.

### quake 3 (1999)
nothing left. bsp/pvs precomputed culling is done (`lunar-bsp-build`, `bake-pvs` tool,
`a9f711b`). we exceed q3 on every axis.

### doom 3 (2004)
remaining:
- **decal stacking** — doom 3 had robust decal depth-sorting; ours is functional but unverified
  under heavy decal load

(soft shadows matched via pcss + soft point pcf since `0621c2c`.)

### half-life 2 (2004)
remaining:
- **displacement terrain** — source engine's displacement meshes for outdoor ground surfaces.
  our terrain is heightmap-based (clipmap), which is different. displacement is patch-based with
  blend weights and works inside source bsp. this is source-specific and probably not worth
  replicating exactly, but a patch/vertex-blend terrain variant would cover it
- **facial animation system** — hl2 flex-based morph system for character faces. we have
  skeletal animation but no vertex-level morph blending with the flex controller api

(detail sprites done since `e3a9628`. planar reflections done since `e3a9628` — water now
reflects the full scene.)

### portal 2 (2011)
remaining:
- **portal renderer** — portal 2's defining feature: rendering the world through a portal with
  correct perspective. our portal culling exists for pvs but doesn't recurse the render pass
  through a portal viewport. implementing this requires a second full render pass per portal pair
  visible in the frame (expensive but finite: at most 2-4 portal surfaces per scene)
- **full taa** — portal 2 era games moved from fxaa to taa for better quality. we have STAA
  (selective temporal AA, per-pixel mask on top of msaa), which handles specular shimmering on
  non-moving geometry. full frame-history taa with velocity reprojection is not yet implemented

### halo ce (2001)
nothing left. we exceed it on every axis.

### halo 3 (2007)
remaining (hardest target):
- **deferred lighting for high-density outdoor scenes** — halo 3 used deferred shading for outdoor
  areas with 30-60 simultaneous dynamic lights. our clustered forward handles this fine up to ~50
  lights, but deferred removes the per-fragment lighting cost entirely for high-overdraw outdoor
  scenes. see the forward+ vs deferred section below
- **texture virtual texturing or mega-texture** — halo 3 streamed unique per-polygon texture detail
  via texture atlases and streaming. our mip streaming does coverage-based quality reduction but
  doesn't support unique textures per poly (mega-texture style). this matters for large unique
  outdoor environments; most indoor games don't need it
- **skin/subsurface scattering** — character skin in halo 3 had a subtle wrap-lighting and sss
  approximation. our pbr has no sss term. for games with prominent character close-ups this is
  visible

(auto-lod generation done since `2ca524f`. ambient light probes per-volume done since `0621c2c`.
gpu-driven lod selection done since `f8a17b9`.)

---

## compile-time wins (aot computation)

the project already does significant aot. here's what would give real performance wins.

### high impact

**shader spirv pre-compilation (build.rs)**
currently: wgpu compiles wgsl → nir/spirv at `create_shader_module` time. on first run this
takes 50-300ms per pipeline × ~20 pipelines = up to 6 seconds of startup stall that shows
as a white screen or freeze.
fix: build.rs uses `naga` to validate wgsl and emit spirv blobs. at runtime, load the blob
with `ShaderSource::SpirV`. compile time moves from user's machine to ci/dev machine.
on subsequent runs, wgpu still creates the pipeline from the pre-compiled module, but the
naga parse+validate step is skipped (~10-30ms per pipeline, not zero).
win: eliminates the first-launch shader compilation stall entirely. warm cache startup
goes from 1-6s to <100ms for shader setup.

**mesh preprocessing pipeline (build.rs or offline tool)**
currently: meshes are loaded from binary at runtime with no preprocessing.
what to do at build time:
- vertex cache optimization (forsyth or meshopt) — gpu vertex post-transform cache hit rate
  goes from ~50% to ~95%, cutting vertex shader invocations nearly in half
- vertex quantization — positions as u16, normals as 10-10-10-2, uvs as u16. cuts vertex
  buffer size 2-3× and improves cache coherency
- mesh simplification for lod generation at n ratios (0.5, 0.25, 0.1, 0.05) — replaces manual
  MeshLod component. build.rs writes the simplified meshes adjacent to the base
- overdraw optimization — reorder triangles to minimize fillrate waste
- normal + tangent computation from positions + uvs (for assets that don't store them)
win: vertex shader throughput doubles, vram usage for geometry halves, lod is free.

**pvs baking (offline tool)**
currently: bsp portal culling is runtime (frustum-based portal traversal).
quake 1 and q3 precomputed which leaves are visible from each leaf (pvs tables). at runtime
visible-set lookup is a bitmask AND — nanoseconds instead of microseconds.
this is the single biggest culling win for indoor games. for a 300-room level, pvs lookup
takes ~10ns vs our runtime portal traversal taking ~50-200μs.
this is NOT build.rs (it requires running a radiosity solver / flooding algorithm on the bsp)
but it IS a one-time offline step tied to the asset pipeline.
win: culling cost for indoor scenes drops by 10-100×. the bsp plugin already has the data
structures; pvs just needs a bake step and a lookup path.

**lightmap baking as a build step**
currently: LightmapBaker is a runtime type — the game calls it at startup or we bake at
edit time and store the result as a texture asset.
what's missing: a build.rs hook that auto-bakes lightmaps for all static mesh groups
referenced in a level file. the baker already exists; the build integration doesn't.
the runtime cost of NOT doing this is that games currently have to either bake at startup
(slow first frame) or do it manually offline. a build-integrated bake means the .tex files
are always up to date and load instantly.

**const geometry for primitive shapes**
currently: `sphere_mesh`, `quad_mesh` etc. in primitives.rs are computed at runtime (called
once in init, then kept in memory).
fix: make these `const fn` or emit them as static binary data. for the sky dome (sphere_mesh
with fixed params) and the sun quad, the vertex data is fully deterministic. embedding them
as `include_bytes!` constants eliminates the sphere tessellation computation from startup.
win: minor (microseconds) but free.

### medium impact

**texture atlas pre-packing**
currently: the lightmap atlas is packed at runtime the first time all lightmaps are loaded.
for a game with a known fixed set of textures (most games), the atlas layout can be
precomputed at build time and stored as a binary descriptor + packed atlas image.
win: atlas packing (currently O(n²) shelf packing) moves from runtime to build time.
first frame with lightmaps no longer stalls.

**sh coefficient pre-baking**
our IrradianceSH resource is typically set from an offline probe capture. the 9 L2
coefficients can be precomputed from an hdr panorama at build time (or offline tool) and
embedded as a `const [f32; 27]`. eliminates the probe projection step at runtime.

**cluster tile boundary precomputation**
the frustum planes for each cluster (16×9×24 = 3456 cells) are constant for a given fov
and screen resolution. they're currently recomputed in the compute shader per frame from
the proj matrix. a build-time table (or one-time init-time table) of cluster frustum planes
would let the compute shader skip that math.
this is a 3456-entry constant that would be ~110KB of storage but saves ~3456 matrix
operations per frame in the compute shader.

**trigonometric lookup tables**
the surface shader system evaluates `sin`/`cos` for UV rotation each frame. a compile-time
sin/cos table (256-entry, fixed-point) is faster than hardware sin/cos on some platforms
and usable from `const` contexts. marginal on desktop but meaningful on embedded/wasm targets.

### low impact (but worth doing)

**shader source hashing**
build.rs hashes shader source files and stores the hash. if unchanged, skip spirv
recompilation. avoids redundant work on incremental builds.

**asset bundle packing**
all assets for a level pre-packed into a single file with a content-addressed index.
eliminates individual file I/O at load time. load becomes one `mmap` + index parse.

---

## forward+ vs deferred — current state and recommendation

**what we currently have:**
our architecture is already forward+ (the academic term, not a marketing term). specifically:
1. z-prepass (depth only) — populates the depth buffer so the main color pass uses early-z
2. cluster assignment compute pass — assigns lights to 16×9×24 view-frustum tiles
3. main color pass — fragment shader looks up its cluster, iterates only those lights
4. shadow pre-passes — csm + point cube maps rendered before the main pass

this IS forward+. we are not doing deferred. the common misconception is that clustered
lighting requires deferred — it doesn't. forward+ just means forward shading + clustered
light assignment.

**how much rendering is "deferred" (zero):**
none. every fragment is shaded exactly once in the main pass with full material data.
there is no g-buffer, no screen-space position reconstruction, no deferred lighting accumulation.

**would switching to deferred help?**
for the games in our target list: **no for most, possibly for halo 3 style outdoor.**

where deferred wins:
- extremely high dynamic light counts (100+ simultaneously visible, overlapping)
- games where overdraw is high (outdoor, no z-prepass effectiveness)
- when you have many material-agnostic lights (pure rgb point lights with no special material
  interaction)
- avoids recomputing material properties per-light (compute albedo/normal once, shade many times)

where forward+ wins (our case):
- msaa works natively (deferred msaa requires per-sample shading or resolve hacks)
- transparent geometry handled in the same pass
- complex materials (surface shaders, subsurface) don't need a g-buffer expansion
- fewer render targets = less bandwidth
- z-prepass eliminates most overdraw cost anyway — the one fragment that survives early-z
  is cheap to shade even with many lights when the cluster is small

**the real recommendation:**
instead of switching to deferred, the higher-leverage move for matching halo 3 outdoors is
a **visibility buffer** (sometimes called deferred texturing or geometry pass). this is a
hybrid:
1. render pass 1: write (meshlet_id, triangle_id, bary_coords) to a u64 render target
2. compute pass: for each pixel, look up the triangle, compute attributes (uv, normal, tangent)
   by interpolating, then shade
this gives deferred's "shade each fragment once" guarantee without the g-buffer bandwidth
cost of storing pre-interpolated attributes, and it preserves msaa compatibility.
it also enables software-rasterized small triangles and meshlet culling (nanite-style).

this is a sprint 7+ item and requires the mesh data to be addressable per-triangle on the gpu
(mega-buffer already in place — the infrastructure exists).

**short-term recommendation:**
stay on forward+. the remaining perf gaps vs halo 3 are better addressed by:
1. auto-lod (halves triangle count for background geometry)
2. pvs culling (eliminates most culling work indoors)
3. gpu-driven lod selection (removes lod cpu cost)

none of these require switching to deferred.

---

## open items by priority

| item                              | target           | effort | impact |
|-----------------------------------|------------------|--------|--------|
| full taa (history + reprojection) | portal 2, modern | medium | medium |
| portal viewport rendering         | portal 2         | high   | medium |
| displacement terrain              | hl2 outdoor      | high   | low    |
| subsurface scattering             | character games  | medium | low    |
| texture virtual texturing         | halo 3 outdoor   | high   | low    |
| visibility buffer (nanite-style)  | halo 3 extreme   | high   | future |
| decal stacking robustness         | doom 3           | low    | low    |
| facial animation (flex morphs)    | hl2 characters   | high   | low    |
