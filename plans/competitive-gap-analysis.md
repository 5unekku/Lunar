# competitive gap analysis

reference targets: quake 1, quake 3, doom 3, half-life 2, portal 2, halo ce, halo 3.

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

we exceed q1, q3, doom 3, hl2, portal 2, and halo ce on all relevant axes. the remaining
gaps against halo 3 are mega-texture (unique per-polygon streaming) and gpu instancing.

---

## open items

| item                         | scope                    | effort | impact |
|------------------------------|--------------------------|--------|--------|
| gpu instancing               | all scenes w/ repetition | medium | high   |
| texture virtual texturing    | large outdoor worlds     | high   | medium |
| visibility buffer (nanite)   | extreme triangle density | high   | future |

**gpu instancing**: every entity is its own draw call. batching entities sharing a mesh
and material into one instanced draw is the next high-impact performance item. affects any
scene with trees, rocks, enemies, buildings — i.e. everything.

**texture virtual texturing (mega-texture)**: mip streaming does coverage-based quality
reduction but every texture needs its own atlas slot. unique per-polygon texture detail
for large outdoor environments requires a virtual texture system. most indoor games don't
need this; large open-world games do.

**visibility buffer**: render pass 1 writes `(triangle_id, bary_coords)` to a u64 target;
a compute pass reconstructs attributes per-pixel and shades once. gives deferred's
"shade each pixel once" guarantee without g-buffer bandwidth cost, preserves msaa
compatibility, and enables nanite-style software rasterization of small triangles. blocked
on wgpu mesh shader support for full meshlet culling; the mega-buffer for per-triangle gpu
data is already in place.

---

## compile-time wins (still pending)

done: spirv pre-compilation, vertex quantization, lod generation, pvs baking, lightmap baking.

remaining:

**vertex cache optimization + overdraw reordering** — `gen-lods` handles simplification;
`meshopt::optimizeVertexCache` and `meshopt::optimizeOverdraw` are not yet called on the
base mesh. gpu vertex post-transform cache hit rate ~50% today, could reach ~95%.
win: vertex shader invocations cut nearly in half.

**const geometry for primitive shapes** — `sphere_mesh`, `quad_mesh` etc. in primitives.rs
are computed at runtime. sky dome and sun quad vertex data is fully deterministic; embed as
`include_bytes!` constants. win: minor (microseconds) but free.

**texture atlas pre-packing** — lightmap atlas is packed at runtime on first load. for a
fixed asset set, precompute the layout at build time. win: first-frame atlas stall gone.

**sh coefficient pre-baking** — 9 L2 irradiance coefficients can be precomputed from an
hdr panorama at build time and embedded as `const [f32; 27]`. win: eliminates probe
projection step at runtime.

**cluster tile boundary precomputation** — frustum planes for all 16×9×24 clusters are
constant for a given fov. currently recomputed in the compute shader each frame from the
proj matrix. a one-time init-time table saves ~3456 matrix ops per frame.

**asset bundle packing** — assets for a level pre-packed into a single file with a
content-addressed index. load becomes one `mmap` + index parse instead of per-file I/O.

---

## forward+ vs deferred — decided: stay on forward+

our architecture is clustered forward+ (z-prepass → cluster compute → main color pass).
switching to deferred does not help for our target games:

- msaa works natively on forward+; deferred requires per-sample shading or resolve hacks
- transparent geometry handled in the same pass
- z-prepass eliminates overdraw — the surviving fragment is cheap to shade even with
  many lights when the cluster is small
- forward+ handles up to ~50 simultaneous dynamic lights with no per-pixel cost increase

deferred wins only at 100+ overlapping dynamic lights with high overdraw, which is not
a realistic scenario for any game in our target range.

the higher-leverage alternative for halo 3 style extreme density is the visibility buffer
(see open items above), not switching to deferred.
