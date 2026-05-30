# renderer sprint 6 — aot + memory (ram and vram)

every item here moves work from runtime to build time, or reduces how much
data lives in ram/vram at any given moment. no new visual features.

the rendering architecture stays on forward+ (already correct). this sprint makes
the existing pipeline cheaper to run and smaller to hold.

---

## item A — spirv pre-compilation (build.rs)

### what and why

wgpu compiles wgsl → naga ir → spirv (or metal/dxil) at `create_shader_module`.
on first launch this stalls the startup thread for 50-300ms per pipeline × ~22 pipelines
= up to 6 seconds before the first frame. the pipeline_cache we have helps on subsequent
runs for the gpu backend step, but the naga parse+validate still runs every launch.

on vulkan, spirv is the native shader format — shipping pre-compiled spirv eliminates
the naga step entirely (no parse, no validate, no lowering). the gpu driver compiles
spirv to isa on the first `create_render_pipeline` call, which the pipeline_cache already
caches across runs.

on metal/dx12, spirv is still an improvement because the naga step moves to build time.

### implementation

**build.rs:**
```rust
// dependencies: naga (with spv-out feature), glob
for wgsl_path in glob("src/**/*.wgsl") {
    let src = fs::read_to_string(&wgsl_path)?;
    let module = naga::front::wgsl::parse_str(&src)?;
    let info = naga::valid::Validator::new(...).validate(&module)?;
    let spv = naga::back::spv::write_vec(&module, &info, &options, None)?;
    let out = out_dir.join(wgsl_path.with_extension("spv"));
    fs::write(out, bytemuck::cast_slice(&spv))?;
}
```

**lib.rs:** replace all `include_str!("foo.wgsl")` with `include_bytes!(concat!(env!("OUT_DIR"), "/foo.spv"))` and switch `ShaderSource::Wgsl` → `ShaderSource::SpirV(Cow::Borrowed(bytemuck::cast_slice(bytes)))`.

keep wgsl sources as fallback behind a `cfg(debug_assertions)` flag so hot-reload
still works in dev — dev builds use wgsl (immediate feedback on shader errors), release
builds use pre-compiled spirv.

### files
- `Cargo.toml` (build deps: naga, glob, bytemuck)
- `build.rs` (new)
- `crates/lunar-render-3d/src/lib.rs` (all include_str! / ShaderSource replacements)

### win
- first-launch startup: ~6s shader stall → <100ms
- shader errors caught at `cargo build` instead of at runtime
- vulkan path skips naga entirely at runtime

---

## item B — texture compression pipeline (build.rs + assetserver)

### what and why

textures are currently stored and uploaded as rgba8 (uncompressed). on gpu:
- a 1024×1024 rgba8 texture = 4MB vram (+ 5.3MB with full mip chain)
- a 1024×1024 bc7 texture = 0.67MB vram (6:1 compression)
- a 1024×1024 bc1 (rgb, no alpha) = 0.5MB vram (8:1)
- a 1024×1024 bc5 (normal map rg) = 0.5MB vram (8:1)

for a typical level with 50 unique 1024² textures: uncompressed = 265MB vram,
bc-compressed = 33MB vram. **8× vram reduction for texture data.**

the compression also reduces bandwidth: bc-compressed textures decompress in the
texture cache, not in system memory, so effective bandwidth is the same as uncompressed
but actual bytes fetched are 6-8× fewer.

### compression format selection

| texture type     | format | notes                                  |
|-----------------|--------|----------------------------------------|
| albedo (no alpha)| bc1    | 8:1, rgb, slight color degradation     |
| albedo (alpha)  | bc3    | 4:1, rgba, sharp alpha edges           |
| normal map (rg) | bc5    | 8:1, rg only, reconstruct z in shader  |
| lightmap (hdr)  | bc6h   | 6:1, signed float rgb, no alpha        |
| general rgba    | bc7    | 6:1, high quality, all channels        |

### implementation

**build.rs (or offline tool `cargo xtask compress-textures`):**
use the `intel-tex-rs-2` or `bcnencoder-rs` crate to compress source png/jpg assets
at build time. emit `.tex` binary files (our engine format) with the compressed pixels.

**assetserver:** when loading a `.tex` file, check the compression header. if bc1/bc3/bc5/bc6h/bc7,
set `tex.compression` accordingly and create the gpu texture with the matching
`wgpu::TextureFormat`. the byte-per-row calculation already branches on compression
(see the existing bc3/bc5 branches in the lightmap upload helper).

**new wgpu formats to add to the compression enum:**
```rust
pub enum TextureCompression {
    None,
    Bc1,   // was missing
    Bc3,   // exists
    Bc5,   // exists
    Bc6h,  // new — for hdr lightmaps
    Bc7,   // new — general purpose
}
```

### files
- `crates/lunar-assets/src/lib.rs` (compression enum, format mapping)
- `build.rs` or `tools/compress-textures/` (compression tool)
- `crates/lunar-render-3d/src/lib.rs` (bc6h/bc7 upload paths in texture upload helper)

### win
- vram: 6-8× reduction for all textured content
- bandwidth: 6-8× reduction (cache-coherent decompression on gpu)
- ram: compressed textures stored compressed in AssetServer too

---

## item C — cpu texture data eviction after gpu upload

### what and why

currently: after uploading a texture to the gpu, `AssetServer` keeps the full
`pixels: Vec<u8>` and `mips: Vec<Vec<u8>>` in ram forever.

for a 1024² rgba8 texture: 5.3MB in ram AND 5.3MB in vram = 10.6MB for one texture.
ram and vram track exactly. this is pure waste after upload.

the pixels are re-read only if:
1. the texture is re-uploaded after eviction (mip streaming path)
2. the lightmap baker reads texture data for baking (rare, baking happens at level setup)
3. the atlas packer reads pixels (currently reads from assetserver)

### fix

add a `Texture::evict_cpu_data(&mut self)` method that zeroes out `pixels` and `mips`,
keeping only the metadata (`width`, `height`, `format`, `compression`, `mip_count`).

in the renderer, after successfully uploading a texture to gpu (lm_tex_cache, surface_tex_cache,
any future diffuse texture uploads), call `evict_cpu_data` on the asset.

**atlas packer fix:** the atlas packer currently reads `tex.pixels` from the asset server.
before eviction, the atlas packer must run first and store a pre-packed atlas on disk
(or build time). OR: keep cpu data only for atlas-eligible textures (lightmaps ≤ 512×512).
the simpler fix: the atlas builds on first use, then cpu data is evicted.

**flag to opt out:** `Texture::keep_cpu_data: bool` for textures that the baker or
collision system needs to re-read. lightmaps used as input to baking keep their data;
everything else evicts.

### files
- `crates/lunar-assets/src/lib.rs` (evict_cpu_data method, keep_cpu_data flag)
- `crates/lunar-render-3d/src/lib.rs` (call evict after each successful upload)

### win
- ram: drops by (vram texture usage). if vram holds 100MB of textures, ram drops 100MB
- essentially halves the memory cost of every uploaded texture

---

## item D — mesh data eviction after gpu upload

### what and why

`MeshRegistry` keeps full `MeshData` (all vertices + indices) in ram after uploading
to the gpu. for a typical level with 500K triangles, this is:
- vertices: 500K × 56 bytes (Vertex3d) = 28MB cpu ram
- indices: 1.5M × 4 bytes (u32) = 6MB cpu ram
- gpu vbo: 28MB vram, gpu ibo: 6MB vram
- total: 34MB cpu + 34MB gpu = 68MB for geometry that only needs to live in gpu

the cpu copy is needed for:
- collision detection (mesh colliders read vertex positions)
- lightmap baking (baker reads positions and uvs)
- cpu-side raycasting

for entities with `MeshUsage::Static` that have no collision or baking needs,
the cpu copy is dead weight after upload.

### fix

add `MeshUsage::GpuOnly` (or a flag `MeshData::gpu_only: bool`). when set:
after `Self::upload_mesh_data()` succeeds, drop the MeshData from MeshRegistry
(keep the handle valid but return `None` on `get_mesh` for that id).

the `MeshRegistry` currently keeps meshes alive as long as handles exist. add an
explicit eviction path: `registry.evict_cpu_data(handle)` that clears the vec data
but keeps the handle live.

for collision meshes: the `Collider3d` component with `ColliderShape3d::Mesh` holds
its own copy of the vertex data extracted at entity spawn time — it doesn't re-read
from MeshRegistry every frame. so evicting after collision world is built is safe.

### files
- `crates/lunar-3d/src/mesh.rs` (MeshUsage::GpuOnly or evict flag)
- `crates/lunar-3d/src/mesh_registry.rs` (evict_cpu_data method)
- `crates/lunar-render-3d/src/lib.rs` (call evict after upload when flag set)

### win
- ram: ~25-30MB per 500K-triangle level (cpu mesh data freed after upload)
- for large outdoor levels with 2M+ triangles, savings are 100MB+

---

## item E — vertex quantization (aot + shader change)

### what and why

Vertex3d is 56 bytes (all float32). after quantization:
```
position:   vec3<f32>      12 bytes  (stays f32, precision matters)
normal:     snorm8×4       4 bytes   (was 12, w=0 for padding)
tangent:    snorm8×4       4 bytes   (was 16, w=sign bit = 1 byte)
uv:         unorm16×2      4 bytes   (was 8, full [0,1] range in 0.0000153 precision)
uv_lightmap: unorm16×2     4 bytes   (was 8)
color:      unorm8×4       4 bytes   (was 16)
```
total: 32 bytes per vertex (was 56). **1.75× reduction in vertex buffer size.**

VERTEX_STRIDE changes from 56 to 32. all vertex buffers halve. for 500K vertices:
28MB → 16MB vram and (if not evicted) same in ram.

gpu vertex fetch bandwidth halves, which matters on bandwidth-constrained passes
(shadow passes, z-prepass, hzb passes all fetch vertices without sampling textures).

### wgsl changes

vertex shader reads change from `float32x3` to `snorm8x4` etc.:
```wgsl
@location(1) normal:  vec4<i32>,   // snorm8×4 (w is ignored)
// in vs_main: let n = normalize(vec3<f32>(in.normal.xyz) / 127.0);
```
wgpu vertex formats: `VertexFormat::Snorm8x4`, `VertexFormat::Unorm16x2` etc.
all exist in wgpu 29.

the shadow.wgsl and point_shadow.wgsl only read position (location 0), which stays
float32×3, so those shaders are unaffected.

### aot component

vertex quantization is done at build time (build.rs or an offline mesh bake step):
- input: MeshData with f32 vertices
- output: quantized binary blob with the new layout
- loaded directly by AssetServer into gpu-ready binary

this means the runtime never sees unquantized f32 data — the cpu representation IS
the quantized form.

### files
- `crates/lunar-3d/src/mesh.rs` (Vertex3d layout change, or a GpuVertex3d type)
- `crates/lunar-render-3d/src/lib.rs` (VERTEX_STRIDE, vertex attribute formats)
- `crates/lunar-render-3d/src/shader.wgsl` (VertIn layout change)
- `crates/lunar-render-3d/src/shadow.wgsl` (position stays unchanged)
- `crates/lunar-render-3d/src/point_shadow.wgsl` (position stays unchanged)
- `build.rs` (quantization at build time)

### win
- vram: vertex buffers 1.75× smaller
- bandwidth: vertex fetch in shadow/z-prepass passes ~1.75× cheaper
- ram: if cpu data not evicted, same savings

---

## item F — shadow map vram reduction

### what and why

current shadow map vram:
- csm: 3 cascades × 1024² × depth32 = 12MB
- point shadows: 24 faces × 1024² × depth32 = **96MB**

the point shadow maps at 1024² are larger than necessary for most games.
doom 3 shipped at 512² per shadow map. for a flashlight 10m from a surface,
512² gives 2cm/texel resolution — imperceptible.

adding a `QualitySettings.point_shadow_res` separate from `shadow_res` lets
the dev and user trade off vram vs sharpness independently.

default: 512² for point shadows.
user-selectable: 256 / 512 / 1024.

at 512²: 24 × 512² × 4 bytes = **24MB** (down from 96MB, **saves 72MB vram**).

### files
- `crates/lunar-render-3d/src/lib.rs` (read point_shadow_res from QualitySettings,
  recreate point_shadow_tex when it changes)
- `crates/lunar-render-3d/src/lib.rs` constants: `POINT_SHADOW_MAP_SIZE` default 512

### win
- vram: saves 72MB at default settings (going from 1024 to 512 per face)

---

## item G — uniform staging buffer right-sizing

### what and why

`uniform_staging` is a cpu-side vec<u8> sized at `entity_capacity × UNIFORM_STRIDE`
(256 bytes per slot). `entity_capacity` grows to the next power of two.

in steady state with 300 entities: capacity = 512, size = 512 × 256 = 131KB ram.
with 1000 entities: capacity = 1024, size = 262KB. with 2000: 512KB.

this is not large, but it never shrinks. if a loading screen spawns 2000 entities and
then gameplay has 200, the staging buffer stays at 512KB.

fix: shrink the staging buffer if `draw_scratch.len() < entity_capacity / 4` for
N consecutive frames (hysteresis to avoid thrash). halve `entity_capacity` when shrinking.

similarly, `material_staging` uses `entity_capacity × MATERIAL_UNIFORMS_SIZE` (48 bytes).
at 1024 slots: 49KB. same treatment.

the light list buffer (`light_list_buf`) is fixed at `256 × 48 bytes = 12KB` — fine, leave it.

### files
- `crates/lunar-render-3d/src/lib.rs` (add under-utilization counter, shrink on hysteresis)

### win
- ram: minor (KB range), but it establishes the pattern of buffers that breathe with load

---

## recommended order

1. **F (shadow map resolution)** — one afternoon, saves 72MB vram immediately, no risk
2. **C (cpu texture eviction)** — high ram win, straightforward
3. **D (mesh data eviction)** — high ram win, small risk around collision ordering
4. **A (spirv pre-compilation)** — build.rs work, major startup win, dev-only wgsl fallback
5. **B (texture compression)** — most impactful vram win, requires build tooling
6. **E (vertex quantization)** — touching VERTEX_STRIDE ripples through everything;
   do last, test carefully with shadow passes

G is low priority; do it opportunistically.
