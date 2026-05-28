# Lunar Engine — High-Performance Rendering Architecture Reference

A technical reference for the Lunar Rust game engine crate (wgpu + bevy_ecs + glam), targeting Raspberry Pi 4 as the minimum floor through Steam Deck, midrange laptops, and modern desktop GPUs.

## TL;DR
- Adopt a **GPU-driven, render-graph-based architecture** with a clustered forward+ 3D pipeline and a single-pass sprite batcher for 2D, gated behind feature flags; on the Pi 4 floor fall back to plain forward with no compute culling because wgpu's GLES backend does not expose compute, indirect execution, fragment storage, base vertex, read-only depth-stencil, independent blending, vertex storage, or fragment storage — the downlevel warning enumerated in gfx-rs/wgpu#2275 is: "Missing downlevel flags: COMPUTE_SHADERS | FRAGMENT_WRITABLE_STORAGE | INDIRECT_EXECUTION | BASE_VERTEX | READ_ONLY_DEPTH_STENCIL | INDEPENDENT_BLENDING | VERTEX_STORAGE | FRAGMENT_STORAGE".
- Treat **bandwidth as the primary budget on every ARM target** (Pi 4: ≤6.4 GB/s; Mali, Apple, V3D are all tile-based deferred): structure render passes so attachments are `LoadOp::Clear` + `StoreOp::Discard` whenever possible, never read back from the GPU on the same frame, prefer ASTC/ETC2, and keep working sets under the 32 KB tile/threadgroup memory budget on Apple A11+ (Apple Metal Feature Set Tables: "Maximum total threadgroup memory allocation: 32 KB").
- **Hard-pin the codebase to wgpu's default `Limits`** as the lowest common denominator (`max_bind_groups = 4`, `max_dynamic_uniform_buffers_per_pipeline_layout = 8`, `max_uniform_buffer_binding_size = 64 KiB`, `max_storage_buffer_binding_size = 128 MiB`, `max_vertex_buffers = 8`, `max_push_constant_size = 0` unless `Features::PUSH_CONSTANTS` is enabled). Backend-specific lifts (e.g. push constants ≤ 4 KiB on Metal per Apple Metal Feature Set Tables: "Maximum length of inlined buffer contents using setBytes: 4 KB") must be feature-gated, not relied upon.

## Key Findings

1. **wgpu is the right abstraction floor** but is not uniform across backends. Use `Features` and `DownlevelFlags` queries at adapter creation as a capability ladder. A small per-backend "patch" trait is required for: (a) emulating `multi_draw_indirect_count` on Metal (no equivalent native primitive — see gfx-rs/wgpu#2148), (b) the GLES downlevel restriction set, (c) push constants on Metal capped at 4 KiB, and (d) coarse timestamp queries on TBDR (gfx-rs/wgpu#2554 reports `TIMESTAMP_QUERY: false` on Apple M1 Pro).
2. **Bevy's render-graph design (sub-app, extract→prepare→queue→phase-sort→render→cleanup) is the proven pattern** and Lunar should mirror it. RenderGraph nodes have read-only access to the world specifically so multiple nodes can record command buffers in parallel; commands flowing through `RenderContext` dedupe redundant pipeline / bind-group sets ("set_render_pipeline checks if the pipeline is already set... set_bind_group tracks BindGroupId and dynamic offsets to deduplicate bindings" — DeepWiki Bevy RenderGraph).
3. **The Pi 4 floor is real but achievable for 1080p@60.** VideoCore VI runs at 500 MHz (Raspberry Pi BCM2711 docs: "the new VideoCore VI 3D unit now runs at up to 500 MHz") with ~32 GFLOPS theoretical (Idein/py-videocore6 formula: 500 MHz × 2 slice × 4 qpu × 4 core × 2 ops). LPDDR4 bandwidth ≈ 6.4 GB/s. At 1080p×60 the framebuffer alone is 1920·1080·4·60 ≈ 497 MB/s, leaving roughly 12× the framebuffer in bandwidth headroom for *everything else*.
4. **Steam Deck (1.6 TFLOPS RDNA2) is much wider than Pi.** Valve's tech page: "GPU: 8 RDNA 2 CUs, 1.6GHz (1.6 TFlops FP32) ... APU power: 4-15W". Chips and Cheese deep-dive: "four 32-bit channels and runs at 5500MT/s, so the theoretical bandwidth should be 88GB/s". Roughly 50× the Pi's compute headroom — quality scaling must be a *function of detected adapter*, not a build-time flag.
5. **Bevy's ECS storage model dictates render-component design.** Hot every-frame components (Transform, MeshHandle, MaterialHandle, Visibility, AABB) must use Table storage; components that toggle frequently per frame must use SparseSet. Mixing storage types forces the slower archetypal iteration path (DeepWiki bevy: "If all components in a query are table-stored, iteration uses the dense path. Otherwise, it uses the archetype path").
6. **GPU-driven culling is achievable on Vulkan/DX12 desktop, partially on Metal, impossible on GLES.** Ship three culling paths behind one `CullStrategy` enum: CPU SoA SIMD with glam Vec3A/Vec4 (NEON on aarch64, per glam docs: "NEON is enabled by default on aarch64 targets"); single-pass compute + indirect on Vulkan/DX12; two-pass HZB + indirect on hi-end.

## Details

### 1. Rendering Pipeline Architecture

**Render graph.** Model on Bevy's design: a DAG of nodes that own (a) declared input/output resource slots typed by `wgpu::Texture` / `Buffer` / arbitrary slot, (b) read-only access to the render world via `&World`, (c) a `record(&mut RenderContext)` method. Each `RenderContext` owns a `wgpu::CommandEncoder`. Edges express either ordering or slot data flow. Resource lifetime is computed by walking the graph: a transient texture allocated by node A and last-read by node F can be aliased with another transient that does not overlap. On TBDR backends, mark transient render targets equivalent-to-memoryless by setting `StoreOp::Discard` and never sampling them; the driver may then back them entirely in tile memory (Apple WWDC20 #10602 "Harness Apple GPUs with Metal": "Memoryless Render Targets... only backed by on-chip tile memory").

**Forward vs deferred vs forward+.** Three rendering strategies, gated by `RenderTier`:
- `RenderTier::LowGles`: classic forward, no compute. Up to 4 dynamic lights summed in the fragment shader. Used for Pi 4 and any device whose adapter lacks `DownlevelFlags::COMPUTE_SHADERS` (per wgpu docs.rs: "The device supports compiling and using compute shaders. WebGL2, and GLES3.0 devices do not support compute"). Single shadow cascade at 1024×1024 (or 512² on the Pi).
- `RenderTier::Mid`: clustered forward+ with a compute shader that bins lights into 16×16×24 (X×Y×Z) clusters; tile size 16×16 pixels is the Aortiz/sketchpad reference value, with exponential Z slicing per Drobot's SIGGRAPH 2017 "Improved Culling for Tiled and Clustered Rendering". Output is a per-cluster `(offset, count)` table plus a global light-index buffer.
- `RenderTier::HighDeferred`: clustered deferred with a thin G-buffer (RGBA8 albedo+roughness, RG16 octahedral normal, R8 metallic+AO packed). Only enable on devices with ≥ 4 GB VRAM and **not** on TBDR (ARM developer blog: "traditional deferred rendering requires us to do a first pass for all the tiles, save the results to memory and then load them back for the next pass").

**Render-pass organisation in wgpu.** A `wgpu::RenderPass` is the fundamental batching unit and costs an attachment load/store at start/end. Rules:
1. One pass per logical attachment set — never split a forward draw across two passes if the attachments are the same; on TBDR you'll pay tile load/store twice.
2. Sort draws inside a pass by pipeline → bind group 0 → bind group 1 → vertex buffer → index buffer; mirror Bevy's deduplication in Lunar's `RenderCommandEncoder` wrapper.
3. Group passes into render bundles when their state is static; `wgpu::RenderBundle` validates at creation and skips validation when executed, but resets pipeline/bind-group/vertex/index state on entry and exit (toji.dev: "the render pass pipeline, bind group, and vertex/index buffer state is reset both before and after the bundle executes... this in turn allows the validation to be skipped when executing the bundle").

**Multi-pass scheduling.** A typical mid-tier frame:
1. Update buffers (extract).
2. Compute: cluster light culling, skinning (if GPU-skinned), particle simulation.
3. Shadow pass: 3 cascades for the directional light, 1024² each on Steam Deck, 512² on Pi.
4. Z-prepass: half-res depth on mid tier, full-res on hi tier; skipped on Pi.
5. Opaque main pass.
6. Sky pass.
7. Transparent pass (back-to-front sorted).
8. Post pipeline (TAA → bloom → tonemap → vignette/CA → upscale).

**Shadow mapping.** Cascaded shadow maps for directional lights with stable bounds (MJP "A Sampling of Shadow Techniques"): snap projection to texel grid each frame; use orthographic frustum tight-fitted to each slice with a logarithmic-linear blend (`splitLambda = 0.5`, the Engel/Crysis default). Filter with `textureSampleCompare`; the Witness "OptimizedPCF" path uses bilinear PCF samples to implement a uniform filter kernel and is the recommended baseline (per MJP, citing Ignacio Castaño's contribution). Use 3×3 PCF on Pi, 5×5 OptimizedPCF on mid-tier, 7×7 disc on hi-tier (the 5×5 size matches MJP's filter-size discussion and is the empirical sweet-spot between hard 2×2 PCF and the much more expensive 7×7 kernel; for soft shadows beyond that, PCSS with optional VSM/MSM is reserved for Ultra). For point lights use dual-paraboloid (2 maps) on low-end or cubemap (6 maps) on hi-end. Avoid VSM/MSM unless the title's art style needs uniform soft shadows everywhere — their 32-bit float targets blow the Pi 4's bandwidth budget.

**Transparency and sorting.**
1. **Alpha test (cutout):** fragment discard at `alpha < 0.5`; rendered in opaque pass with depth write. Avoid `discard` on Mali (kills HSR — ARM developer blog explicitly warns against early-z-killing fragment-shader side effects); prefer alpha-to-coverage with MSAA where possible.
2. **Alpha blend (translucent):** back-to-front sort by view-space depth, render in transparent pass after opaque + sky. No depth write, depth test enabled.
3. **OIT (weighted-blended, McGuire/Bavoil):** two MRTs (accumulation + revealage), order-independent, resolve in fullscreen pass. Mid-tier and above only.

**Post-processing pipeline.** Apply in this order (compute on hi-tier, fragment on low-tier):
1. **Motion blur** (per-object via velocity buffer; camera blur only on Pi).
2. **Depth of field** (gather DoF; skipped on Pi).
3. **Bloom** with a progressive downsample/upsample pyramid using a tent filter — Jorge Jiménez's "Next Generation Post Processing in Call of Duty: Advanced Warfare" is the canonical reference. Use 5 mip levels on mid-tier and 7 on hi-tier (Jiménez's Advanced Warfare pipeline uses up to 7 dual-filter taps; beyond that, additional mips contribute below visible threshold for typical 1080p output).
4. **Tonemap** (ACES filmic or Khronos PBR neutral; before color grading because grading wants display-referred LDR).
5. **Color grading** via 16³ or 32³ 3D LUT.
6. **Chromatic aberration, vignette, film grain** fused into one shader.
7. **Upscale** (FSR1-style EASU+RCAS on hi-tier; Lanczos on mid; nearest on Pi).
8. **Final tonemapped output to swapchain.**

**Screen-space effects.** SSAO at half resolution using the GTAO formulation (Jorge Jiménez "Practical Realtime Strategies for Accurate Indirect Occlusion", SIGGRAPH 2016) — skip on Pi. SSR at quarter resolution with hierarchical-Z trace (mid+). SSGI only on hi-tier.

**Temporal techniques (TAA, FSR, DLSS-style).** Generate a velocity (motion-vector) buffer by reprojecting last frame's clip-space positions. TAA in a single compute shader: sample current frame, sample reprojected history with neighborhood clamp (Salvi variance clipping), output blended frame. Apply a jittered subpixel projection offset per frame — the standard is the Halton(2,3) low-discrepancy sequence, popularised by Playdead's INSIDE GDC 2016 TAA presentation (the INSIDE TAA slides explicitly list "16 first samples of halton(2,3)" for jitter, and the technique is propagated through Unreal Engine and Unity's post-processing stack). For upscaling: render at half-res internally and run an FSR2-style temporal upscaler. Apply a negative mip bias `log2(renderRes/displayRes) - 1.0` to textures during upscaled rendering (AMD FidelityFX SDK docs: "mipBias = log2(renderResolution/displayResolution) - 1.0").

**Particles and VFX.** GPU particle simulation on mid+ tier (compute shader updates SoA buffers; indirect draw consumes alive count from a counter buffer). On Pi, run particle simulation on CPU with `rayon` and upload a vertex stream once per frame via `Queue::write_buffer`. Particles are camera-aligned billboards by default; expose mesh-particle and ribbon-trail variants. Use texture-array atlases (16 frames per array layer is a common sheet size). Soft particles (depth-faded) require a depth-texture sample in the fragment shader — gate this conditionally, because sampling depth in a transparent pass means an extra subpass on TBDR.

### 2. Extreme Rendering Performance

**Command submission patterns.** One `CommandEncoder` per render-graph node; one `Queue::submit` per frame in the common case, with the entire `Vec<CommandBuffer>` collected and submitted in one shot. Multiple submits per frame are acceptable only when one submit's output feeds another via map-readback (rare; avoid). `CommandEncoder::begin_render_pass` is where most backend cost lives — keep them few.

**Draw call batching.**
- **Static batching:** at level-load, merge meshes sharing a material into one vertex+index buffer. Cost: loss of per-mesh culling — only for static decoration meshes ≤ ~64 KB each.
- **Dynamic batching:** at frame start, gather visible meshes by material and append vertices into a frame-temporary buffer (`wgpu::util::StagingBelt`); only worth it for very small meshes (< 300 verts).
- **Instanced drawing:** the default. Per-instance buffer holds 4×4 model matrix (64 B), material index (4 B), and per-instance tint (4 B). One `draw_indexed` per (mesh, material) with `instance_count` ≥ 1.

**Indirect drawing and GPU-driven rendering.** `RenderPass::multi_draw_indirect` is conditional on `DownlevelFlags::INDIRECT_EXECUTION` (gfx-rs/wgpu CHANGELOG PR #8162: "We have removed Features::MULTI_DRAW_INDIRECT as it was unconditionally available on all platforms. RenderPass::multi_draw_indirect is now available if the device supports downlevel flag DownlevelFlags::INDIRECT_EXECUTION"). On Metal it is emulated as a loop. `multi_draw_indirect_count` requires explicit `Features::MULTI_DRAW_INDIRECT_COUNT` and is **not** available on Metal (gfx-rs/wgpu#2148). The Lunar path: a compute shader produces a packed list of visible (mesh, instance, transform) tuples plus a `DrawIndexedIndirectArgs` struct per bucket; on Vulkan/DX12 issue `multi_draw_indexed_indirect_count`; on Metal read the count back to a CPU-visible buffer and loop on the CPU side (acceptable, < 0.1 ms). `DrawIndexedIndirectArgs` is 20 bytes — pad to 4-byte alignment.

**Pipeline state object (PSO) management.** Pipeline creation is the most expensive single operation in wgpu. Cache by content-hash of `RenderPipelineDescriptor`. Bevy's `PipelineCache` is the reference design: pipelines step through states `Queued → Creating → Ok/Err`; consumers must check readiness in the Queue phase before the RenderGraph runs (DeepWiki: "Systems call pipeline_cache.unwrap() to retrieve the wgpu::RenderPipeline. If the pipeline is not yet compiled, this will panic, which is why systems often check if pipelines are ready during the Queue or Prepare phases before the RenderGraph runs"). Persist `wgpu::PipelineCache` to disk on Vulkan/DX12; on Metal rely on the system shader cache.

**Bind-group layout design.** Always create `BindGroupLayout` and `PipelineLayout` explicitly; never `layout: None` / `auto` in production (toji.dev: "any time you have multiple pipelines that need the same data... you should always prefer to use explicitly defined pipeline layouts. Explicit pipeline layouts allow for bind groups to be re-used between pipelines, which can be a big win for efficiency"). Standard 4-group layout (consumes all four default slots):
- `@group(0)` — view-global (camera matrices, time, cluster table, IBL probes, shadow atlas).
- `@group(1)` — material (texture array indices, PBR factors, blend params).
- `@group(2)` — per-mesh / per-instance (model matrix array, skinning matrices).
- `@group(3)` — pass-specific (SSR ray buffer, particle storage).

Bind group 0 is set once per pass. Group 1 changes on material switch. Group 2 changes on draw via dynamic offsets into one big uniform buffer (`max_dynamic_uniform_buffers_per_pipeline_layout = 8` in defaults). Group 3 changes per logical sub-stage.

**Push constants vs uniform vs storage buffers.**
- **Push constants** (called "immediate data" in newer wgpu): 4 B per write. Vulkan 1.0–1.3 guarantees ≥ 128 bytes of `maxPushConstantsSize`; Vulkan 1.4 raised the guaranteed minimum to 256 bytes (Vulkan Documentation Project, docs.vulkan.org/guide/latest/versions.html: "Vulkan 1.4 introduced changes to some of these limits, notably increasing the guaranteed minimum value for maxPushConstantsSize from 128 bytes to 256 bytes"). Metal allows ≤ 4 KiB via `setBytes:`. Default `max_push_constant_size = 0` — feature-gate.
- **Uniform buffers:** ≤ `max_uniform_buffer_binding_size = 64 KiB` per binding. Use with dynamic offsets for per-instance / per-pass data.
- **Storage buffers:** up to `max_storage_buffer_binding_size = 128 MiB` per binding. Unbounded data: skinning matrices, instance arrays, particle SoA, light list, cluster index list. **Storage buffers in fragment shaders are not supported on GLES** (`FRAGMENT_STORAGE` missing per gfx-rs/wgpu#2275).

**Texturing strategies.**
- **Atlas:** one large 2D texture with manually packed sub-rects. Used when `TEXTURE_BINDING_ARRAY` is unavailable.
- **2D array:** `texture_2d_array<f32>` with `array_layer` indexed from instance data. Best for sprite sheets, terrain splatting, particle frames.
- **Bindless / texture binding arrays:** `Features::TEXTURE_BINDING_ARRAY` + `SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING`. Vulkan/DX12, partial on Metal (Apple GPU family 4+), not on GLES. Bevy ships a "slab" allocator for bindless material groups; mirror it.

**Mesh LOD.** Discrete LOD on all tiers — three levels per asset minimum (full, 1/4, 1/16 triangle count). On Pi, LOD selection uses a screen-space size heuristic CPU-side; mid+ runs it in compute as part of GPU culling. Nanite-style virtualized geometry is out of scope for v1 — wgpu/naga has only partial mesh-shader support and most backends require fallback (per gfx-rs/wgpu v29 mesh_shading doc and bevy discussion #10433). The meshlet approach using indirect indexed draws (compute culls clusters, indirect draw consumes the list) is reasonable as a v2 feature.

**Occlusion culling.** Three tiers:
1. CPU frustum + distance LOD (every tier). SoA layout: `Vec<Vec3A>` for AABB centres and half-extents, processed in 8-AABB SIMD batches with glam's NEON-backed `Vec3A`.
2. Hierarchical Z-buffer occlusion on mid+: build a depth pyramid via progressive 2×2 max-down in a compute shader, then in the culling compute test each instance's AABB against its corresponding HZB mip.
3. Two-pass occlusion (UE5-style): pass-1 draws last frame's visible set, builds HZB, pass-2 culls remaining instances against new HZB.

GPU occlusion queries (`wgpu::QueryType::Occlusion`) have 1–3-frame latency and aren't reliably available as conditional rendering on TBDR Metal/Apple — use HZB instead.

**Frustum culling implementation.** Extract six planes from the view-projection matrix once per frame (Gribb-Hartmann). Test each AABB by computing `n = sign(plane.xyz) * extent` and `dot(plane.xyz, centre + n) + plane.w >= 0` for all 6 planes. With glam this vectorises into ~12 fused mul-adds per AABB; on aarch64 NEON-backed `Vec4` keeps it in registers.

**Visibility determination.** Top-down SAH-built BVH2 over static instances (rebuilt only on level load) is the default broad-phase. Dynamic instances live in a per-frame loose grid (cell size = 2× average dynamic AABB). Octrees and portals are not used; PVS is reserved for level types that explicitly opt in (Quake-style indoor maps).

**GPU profiling.** Use `Features::TIMESTAMP_QUERY` and `TIMESTAMP_QUERY_INSIDE_PASSES` where available; on TBDR (Metal Apple, V3D, Mali) typically not exposed (gfx-rs/wgpu#2554: M1 Pro adapter dump shows `TIMESTAMP_QUERY: false`; wgpu docs: "This is generally not available on tile-based rasterization GPUs"). Fall back to CPU-side `Instant::now()` deltas across submit boundaries; expose RenderDoc / Xcode capture hooks via `wgpu::Instance` with the right backend selection.

**Render-target management.** A `TransientTextureAllocator` aliases textures with matching `(format, extent, sample_count, usage)` and disjoint lifetimes in the render graph. Persistent targets (swapchain, history buffers for TAA, shadow atlas) live outside. On TBDR backends, transient attachments that are never sampled get `StoreOp::Discard` and stay in tile memory.

**Shader permutation management.** WGSL has no preprocessor. Implement a tiny shader assembler that takes a base WGSL + a feature-flag set and produces a permutation string. Hash `(path, feature set)` and cache compiled `ShaderModule`s. Use WGSL `@id` `override` constants (`@id(0) override KEY: u32 = 0u;`) — the wgpu/WGSL equivalent of Vulkan specialisation constants; one shader module yields many pipelines.

**SPIR-V considerations.** Lunar authors in WGSL; naga compiles to SPIR-V (Vulkan), MSL (Metal), HLSL (DX12), GLSL ES (GLES) — per sotrh learn-wgpu: "SPIR-V for Vulkan, MSL for Metal, HLSL for DX12, and GLSL for OpenGL. The conversion is done internally, and we usually don't need to care about the details. In the case of wgpu, it's done by the library called naga." Avoid features naga handles poorly across all backends: recursive functions, pointer arithmetic, certain texture-format reinterpretations. When you must ship raw SPIR-V, `Device::create_shader_module_spirv` works on Vulkan/Metal/DX12 but not GLES; treat SPIR-V as an optional fast-path, never the only path.

### 3. Memory Layout and Cache Optimisation

**ECS data layout (bevy_ecs).** Components used in the hot render extraction loop (Transform, GlobalTransform, MeshHandle, MaterialHandle, RenderLayers, Visibility) **must** use Table storage (the default — taintedcoders: "Archetypes ensure that entities with similar component compositions are stored in contiguous memory locations"). Marker-style components toggled every frame use SparseSet. Don't mix: a query of mostly-Table components plus one SparseSet component takes the slower archetypal iteration path.

Avoid archetype fragmentation: if every entity ends up in its own archetype because of marker churn, query iteration time grows linearly with archetype count. Rule: a marker component you'd toggle every frame **must** be SparseSet, not Table — because Table inserts/removes copy every other component in the row to a new archetype.

**Component layout rules for cache.** Render-hot components should be small (≤ 64 B), `#[repr(C)]`, and `Copy`:
```rust
#[repr(C)]
#[derive(Component, Copy, Clone, Pod, Zeroable)]
struct InstanceData {
    transform: Mat4,        // 64 B (glam Mat4 is 16-aligned, SIMD)
    material_index: u32,
    mesh_index: u32,
    flags: u32,
    _pad: u32,
}                           // total: 80 B, fits in 2 cache lines
```
Pair (Transform, AABB, MeshHandle, MaterialHandle) into a `Bundle` so they sit in the same archetype table. Don't put `String` names in render-hot components; put them in a separate `DebugName` SparseSet component.

**Transform hierarchies.** Hierarchies are a cache disaster at runtime if walked naively. Lunar follows the Bevy pattern: store `Transform` (local) and `GlobalTransform` (computed world), with a propagation system that walks parent → child once per frame in topological order. The GPU-side instance buffer is filled from `GlobalTransform` only; do *not* upload local transforms.

**Mesh data layout.** Interleaved is the default for static geometry (one vertex = position + normal + tangent + uv = 48 B; one `wgpu::VertexBuffer` per mesh). Use separate streams when one is reused across passes (positions-only for shadow + depth-prepass saves 4–6× bandwidth in shadow rendering). Recommended layout:
```
position : vec3<f32>     (12 B)
normal   : i16x4 (oct)   ( 8 B)   // octahedral encoded
tangent  : i16x4         ( 8 B)
uv0      : f16x2         ( 4 B)
uv1      : f16x2         ( 4 B)
color    : unorm8x4      ( 4 B)
joints   : u16x4         ( 8 B)   // skinned only
weights  : unorm8x4      ( 4 B)
total non-skinned: 40 B; skinned: 52 B
```

**Index buffers.** Use `u16` indices whenever vertex count ≤ 65 535 (half the bandwidth of u32). Triangle lists, never strips (strips break instancing and indirect drawing). For large meshes, partition into meshlets — NVIDIA's recommended canonical sizes are 64 vertices and up to 126 primitives (NVIDIA Technical Blog "Introduction to Turing Mesh Shaders": "We recommend using up to 64 vertices and 126 primitives. The '6' in 126 is not a typo. The first generation hardware allocates primitive indices in 128 byte granularity and needs to reserve 4 bytes for the primitive count."). This gives natural occlusion-cluster boundaries and is forward-compatible with a future mesh-shader backend.

**GPU buffer management.**
- **Static buffers:** created once with `mapped_at_creation: true`, populated, unmapped. Used for mesh vertex/index data, material parameter banks. Lifetime = asset lifetime.
- **Frame-temporary buffers:** `wgpu::util::StagingBelt`. One belt per frame in flight (≥ 2). Chunk sizing rule from wgpu docs: "larger than the largest single StagingBelt::write_buffer() operation; 1-4 times less than the total amount of data uploaded per submission". A 4 MiB chunk works for most games.
- **Persistent dynamic buffers:** a ring buffer (3× frame size — one slot per frame in flight) for camera UBO, light UBO, per-frame uniforms. Update via `Queue::write_buffer` (wgpu routes through implicit staging — gfx-rs/wgpu#1438: "in practice we have a deque of staging buffers, internally, that we rotate as the write_xx are issued").

Persistent mapping is not part of WebGPU semantics (gfx-rs/wgpu#1468 confirms). `Features::MAPPABLE_PRIMARY_BUFFERS` (native only) allows direct MAP_WRITE on storage/vertex buffers on unified-memory systems — use it on Apple Silicon and Steam Deck for skinning and particle data to skip the staging copy.

**GPU memory allocator.** Underneath, wgpu uses `gpu-allocator` for backend memory. Build a resource pool on top:
- Buddy allocator for transient render targets (power-of-two extents).
- Slab allocator for many same-sized buffers (instance data slots, particle slabs).
- Pool allocator for per-frame UBO/SSBO slots (ring buffers).

**CPU memory pools.** `bumpalo` (frame arena) for per-frame transients (visibility lists, command sort keys, debug-draw vertex lists). Reset at frame end. `slotmap` for stable IDs into pooled resources.

**SIMD-friendly data layout.** Hot-loop math types are glam's SIMD types: `Vec3A`, `Vec4`, `Quat`, `Mat4` (all 16-byte aligned per glam docs: "these types are all 16 byte aligned"). Frustum-cull SoA layout:
```rust
struct CullSoa {
    centres:    Vec<Vec3A>,    // 16 B/elem
    halfsizes:  Vec<Vec3A>,    // 16 B/elem
    layer_mask: Vec<u32>,
}
```
Process 4 AABBs per loop; glam's NEON path on aarch64 auto-vectorises.

**Cache line alignment (64 B).** Structs that cross thread boundaries should be `#[repr(align(64))]` (Rust RFC 1358: `#[repr(align(N))]` directly supported). Idiomatic Rust pattern: `#[repr(align(64))] struct CacheLine { data: [u8; 64], }`.

**Avoiding false sharing.** When multiple worker threads write to per-thread counters or output buffers, allocate one `CacheLine`-aligned slot per thread. The classic mistake — a `Vec<u32>` with one u32 per thread — pings between cores at ~64 ns/ping on modern x86.

**Asset streaming and GPU memory budget.** Explicit budget per category. Watermark at 75 % of detected adapter VRAM; over the watermark, evict streaming texture mips first, then cached meshes, then sound effects. On Pi 4 (2 GB unified memory), the GPU budget is hard-capped: 256 MB total. On Steam Deck (16 GB unified — Chips and Cheese: "two Samsung chips with 8 GB of capacity each"), 4 GB is a safe budget.

### 4. Frame Scheduling and Consistent Frame Times

**Fixed vs variable timestep.** Decouple simulation from rendering using the accumulator pattern documented in Glenn Fiedler's "Fix Your Timestep!" (gafferongames.com/post/fix_your_timestep/), the canonical reference for this pattern. Simulation runs in `FixedUpdate` at 60 Hz; rendering interpolates between the two most recent simulation states using the time-step accumulator fraction. All physics, AI, gameplay logic use fixed dt; only camera shake, animation playback, and post-processing temporal accumulators use frame dt.

**Render thread architecture.** Use Bevy's sub-app design: one `RenderApp` with its own `World` and `Schedule`. `Extract` is the single synchronisation point — main world reads, render world writes. Once Extract finishes, the main world begins next-frame simulation while the render world drives the GPU for last frame. This is pipelined rendering: at steady state, main thread runs frame N+1 sim while render thread submits frame N (Bevy cheatbook: "Before the runner executes the main schedule, it calls SubApp::extract to synchronize the sub-app with the main world. No schedules can execute on the main world during extraction").

**CPU-GPU synchronisation.** wgpu hides explicit fences/semaphores:
- `Queue::submit` returns a `SubmissionIndex`.
- `Device::poll(Maintain::WaitForSubmissionIndex(idx))` blocks for completion.
- Per-frame, **do not** wait on the previous frame's submit; the driver throttles via the swapchain's `desired_maximum_frame_latency` (default 2). Set this to 2 for triple-buffered presentation, 1 for VR/XR where latency matters most.

**Triple vs double buffering.** Triple (`desired_maximum_frame_latency: 2`, FIFO present mode) is the default — best throughput. Double (`desired_maximum_frame_latency: 1`) reduces latency by ~16 ms but causes stalls when CPU runs faster than 60 fps.

**Present modes.**
- `PresentMode::Fifo` — vsync. Default. Guaranteed support on every backend.
- `PresentMode::Mailbox` — replace-on-arrival triple buffer; low latency, no tearing. Not on all GLES drivers (Pi 4 KMS / Wayland often only supports FIFO).
- `PresentMode::Immediate` — no vsync, tearing. Benchmarking flag.
- `PresentMode::FifoRelaxed` — vsync that allows tearing if late.

**Work submission timing.** GPU starvation happens when the CPU doesn't submit fast enough. Symptom: `Queue::submit` blocks at high frequency; GPU utilisation drops. Mitigation: front-load command recording (start immediately after Extract). Use `Device::poll(Maintain::Poll)` after submit to give the driver a chance to recycle resources.

**Job system.** Use `rayon` for embarrassingly-parallel CPU work (culling, particle sim on Pi, animation sampling, ECS extract). For render command recording, parallelise per render-graph node (Bevy's design — each node has `&World`). Thread pool size: `min(num_logical_cpus - 1, 4)` on Pi 4 (4-core Cortex-A72, leave one for the OS), `num_logical_cpus - 2` on Steam Deck (Zen 2 4c/8t per Valve tech page, leave one for OS and one for audio).

**Async compute.** wgpu currently exposes one `Queue` per `Device`, so true async compute is not portable. Where supported (Vulkan, DX12), implement async compute as a backend-specific patch.

**Precomputed data caching across frames.** `RenderResource`s for: reprojection matrices (last frame's view-projection for TAA/SSR), shadow cascade matrices (for cascades that didn't move), GI probe values (re-evaluate one probe per frame in a round-robin schedule).

**Frame budget allocation (16.67 ms target).**

| Subsystem | Pi 4 (1080p60) | Steam Deck (1080p60) | High-end (1440p144) |
|---|---|---|---|
| Sim + ECS extract | 4 ms | 2 ms | 1 ms |
| Culling | 2 ms | 0.5 ms | 0.2 ms |
| Shadow pass | 3 ms | 1 ms | 0.5 ms |
| Depth prepass | (skip) | 0.5 ms | 0.4 ms |
| Opaque + sky | 5 ms | 4 ms | 2 ms |
| Transparent + particles | 1 ms | 1 ms | 0.5 ms |
| Post + upscale | 1 ms | 2 ms | 1.5 ms |
| Reserve | 0.67 ms | 5.67 ms | 0.85 ms |

**Dynamic resolution scaling.** Each frame, sample the previous frame's GPU time. EMA over 10–20 frames. Above 95 % of budget, drop render resolution by 5 % steps (floor 50 %); below 80 %, bump 5 % until ceiling. Upscale via FSR2-style temporal upscaler (or bilinear on Pi). Cache 4–5 standard resolutions and snap to them to avoid thrashing the allocator.

**Thermal throttling on ARM.** Watch the frame-time trend rather than trying to read temperature directly. If average frame time drifts upward over 60 seconds with the dynamic-resolution scaler already at floor, downgrade `RenderTier`. Raspberry Pi documentation specifies progressive throttling between 80 °C and 85 °C: "When the core temperature is between 80°C and 85°C, the Arm cores will be progressively throttled back. If the temperature reaches 85°C, both the Arm cores and the GPU will be throttled back" (raspberrypi.org/documentation/hardware/raspberrypi/frequency-management.md).

### 5. 2D Rendering Specifics (feature flag: `2d`)

**Sprite batching.** Persistent vertex buffer pre-sized for max visible sprites (default 8192 sprites). Each frame:
1. Frustum-cull sprites by AABB.
2. Sort visible sprites by `(layer, texture_atlas_id, z, y)`.
3. Walk the sorted list, flush a batch whenever atlas id changes.
4. Write sprite data to a per-frame storage buffer (compute path) or vertex buffer (Pi path).

Pi-tier sprite vertex format: 6 verts per sprite (two triangles), `(vec2 pos, vec2 uv, u32 colour_rgba)` = 20 B/vert × 6 = 120 B/sprite. Mid+ uses a compute shader to expand per-sprite SoA into vertex data, packing flags and texture dims as in Cold Bytes Games's "perfect 2D sprite pipeline" approach ("the flip bits and the texture size are packed into the same variable to save memory, each texture dimension is stored within 14 bits enough to store texture size up to 16384, enough for a 16K texture").

**Tilemaps.** Chunked: 32×32 tile chunks (1024 tiles each). Each chunk is a single quad with a vertex shader that fetches its tile indices from a small storage buffer; fragment shader does the atlas lookup. One draw call per visible chunk (4–9 chunks at 1080p). For sparse worlds (Pokémon-style), use a hash map of chunks. For streaming, load 5×5 chunks around the camera and unload at 6×6 (hysteresis).

**2D camera and viewport.** Orthographic projection with `near = -1000, far = +1000`. View matrix translates by `-camera_pos`. For pixel-perfect 2D, camera position must be rounded: `(pos * pixels_per_unit).floor() / pixels_per_unit`.

**Pixel-perfect rendering rules.**
1. Integer scaling only: render to an internal target at native pixel resolution, upscale by integer multiples to the window. Nearest-neighbour upscale (Bevy: `ImagePlugin::default_nearest()`).
2. Texture samplers use `FilterMode::Nearest` everywhere in 2D mode.
3. Round sub-pixel camera positions after physics simulation, before extract.
4. Sprite quads sized in integer pixels; rotation breaks pixel-perfect by definition.

**2D lighting.** Three approaches:
- Baked palette/colour-grade only (Earthbound/RPG Maker): no real-time light.
- Per-sprite normal maps + point lights: each sprite optionally has a normal-map atlas counterpart; fragment shader does N·L per light, summed over ≤ 4 lights.
- 2D shadow casting (Castlevania): for each occluder, build a polygon silhouette extruded from the light; render to a stencil/alpha buffer; modulate the lit fragment. Limit to 8–16 occluders per light per frame.

**Parallax scrolling layers.** Each layer is its own batch with its own scroll offset = `camera_pos * parallax_factor`. Render back-to-front. For infinite scrolling backgrounds, address mode `Repeat` and modulo the UV offset.

**Palette swapping.** Store source image as R8Uint indexed texture into a small palette texture (256×N RGBA). Fragment shader: `texture(palette, vec2(palette_index/256.0, swap_id/N))`. Switching palettes is one bind-group update.

**2D particle systems.** Same engine as 3D particles, constrained to camera plane. Pi tier: CPU sim. Mid+: same compute as 3D.

**Text rendering pipeline.**
- **Bitmap glyph atlas:** for fixed-size pixel fonts. One texture, one quad per glyph, batched with sprites.
- **MSDF (multi-channel SDF):** for scalable text. Generate atlases offline with `msdf-atlas-gen` (Chlumsky). Median-of-RGB shader (per Sihao Lu's writeup: "The only difference is that the distance is calculated using median value of the RGB"). Screen-derivative AA factor: `screenPxRange = fwidth(distance) * pxRange / 2.0`.

**UI rendering.** Retained-mode (Bevy UI / `egui` / `iced`) default for editor and HUD. Immediate-mode (egui) for debug overlays. Nine-slice sprites for resizable panels: vertex shader expands a 3×3 grid of quads from one instance, scaling only the centre column/row.

**Blend modes.**
- Alpha: `src.rgb * src.a + dst.rgb * (1 - src.a)`.
- Additive: `src.rgb + dst.rgb`.
- Multiply: `src.rgb * dst.rgb`.
- Screen: `1 - (1-src) * (1-dst)`.
- Premultiplied alpha preferred when mixing modes in one batch.

**Sorting layers and z-ordering.** Each sprite has `(layer: i32, z: f32)`. Sort key packed as `(layer as u64) << 32 | (z.to_bits() as u64)`. Layers: background = 0, world = 1000, characters = 2000, FX = 3000, UI = 10000.

### 6. 3D Rendering Specifics (feature flag: `3d`)

**Scene graph vs ECS.** Lunar has no separate scene graph. Everything is ECS entities; parent/child use bevy_ecs's `Parent`/`Children`. A "scene" is a `World` snapshot.

**Skeletal animation.**
- **CPU skinning** (Pi tier, low triangle counts only).
- **GPU skinning** (default): upload an array of bone matrices per skeleton (one storage buffer slot per skinned mesh); vertex shader reads `joints` + `weights` and matrix-blends.
- **Dual-quaternion skinning** (optional quality upgrade): each bone is 2 quaternions = 8 floats vs 12 for a 3×4 matrix, saving bandwidth. Eliminates the "candy wrapper" artefact of linear blend skinning (Kavan et al. 2007; theomader.com: "8 float values instead of 12 per joint"). Per-mesh option, default off for compatibility.

Bone matrix arrays live in a storage buffer (preferred). On Pi tier, fall back to a 4×N RGBA32F texture and `textureLoad` because `VERTEX_STORAGE` is missing on GLES.

**Morph targets.** Up to 8 per mesh by default. Store as a 2D texture array (one layer per morph, each layer a delta-vertex stream). Vertex shader samples each active layer, weights, accumulates onto base position/normal. ~3× base vertex shader runtime per active morph.

**PBR material system.** Metallic-roughness (glTF 2.0):
- `baseColor` (RGBA8 sRGB × scalar tint).
- `metallic`, `roughness` (RG8 linear, channel-packed; metallic R, roughness G).
- `normal` (RG8 with reconstructed Z, tangent-space).
- `occlusion` (R8) packed with metallic-roughness.
- `emissive` (RGBA8 sRGB × scalar factor).

Cook-Torrance BRDF, GGX NDF, Schlick Fresnel, Smith-GGX shadowing — Karis "Real Shading in Unreal Engine 4". Specular-glossiness as alternate for legacy assets.

**IBL.** Split-sum approximation (Karis): pre-filtered specular cubemap (mip chain encoding roughness 0..1) + 2D BRDF LUT + irradiance cubemap (or 9 SH L2 coefficients = 108 B). Reference: Nadrin/PBR HLSL ("Split-sum approximation factors for Cook-Torrance specular BRDF... Total specular IBL contribution"). LUT shipped as 16-bit float per channel, clamped at edges.

**Cubemaps, reflection captures.** Static reflection probes (cubemaps, baked offline). Dynamic local reflections use SSR with a fallback to the nearest probe. Real-time cubemap captures are expensive (6 passes); reserve for hero objects (a single mirror) at 256² maximum.

**Atmospheric scattering.** Real-time Rayleigh+Mie scattering via Bruneton/Hillaire model: precomputed transmittance LUT (256×64), single-scattering LUT (32×32×32), multiple-scattering LUT (32×32). Render sky as a fullscreen pass after opaque, before transparent. Skip entirely on Pi tier.

**Water rendering.**
- Pi: animated normal map UV scroll + reflection cubemap, no refraction.
- Mid: Gerstner waves (4 wave components in the vertex shader) + SSR + simple refraction.
- Hi: FFT ocean (Tessendorf) with full refraction and chromatic dispersion.

**Terrain rendering.** Heightmap-based with geometry clipmaps (Losasso/Hoppe 2004 "Geometry clipmaps: terrain rendering using nested regular grids"). Reference implementation: tschie/geo-clipmap. Cracks between rings fixed by a one-cell skirt. On Pi, single LOD with distance fade.

**Level streaming.** Chunk-based open worlds: 64 m square chunks, streamed asynchronously when the camera enters a chunk's 4×4 neighbour radius; unloaded at 6×6. Per-chunk LOD: chunks beyond ~256 m render only LOD2 + impostor billboards.

**Decals.** Box-projected: each decal a unit cube in world space, rendered as transparent draw that samples scene depth in the fragment shader to reconstruct world position, then projects into decal-local space. Limit ~256 decals per frame.

**Lightmapping / baked GI.** Offline-bake an irradiance texture per static mesh (UV1 = lightmap UVs). Runtime samples lightmap in addition to direct lights. For sparse outdoor scenes, replace per-mesh lightmaps with a volumetric irradiance grid (3D texture of L1 SH).

**Probe-based indirect lighting.**
- Irradiance probes: 3D grid, each storing 9 SH coefficients per RGB channel. Trilinear sample at fragment position; modulate by visibility ratio.
- Reflection probes: cubemaps placed by the artist; sample two nearest and blend by influence distance.

**Volumetric effects.** Froxel-based volumetric fog. Per Bart Wronski, "Volumetric Fog: Unified, Compute Shader Based Solution to Atmospheric Scattering" (SIGGRAPH 2014 Advances in Real-Time Rendering): "We used volumes sized 160x90x64 or 160x90x128 depending on the platform. It provides fixed cost of almost all of the passes, not dependent on the screen resolution." Each froxel stores RGBA (scattering colour + extinction). God rays in 2D as a fallback: radial blur from the sun position in screen space at quarter resolution.

**Subsurface scattering.** Approximation: pre-integrated skin shading using a lookup texture indexed by `(NdotL, curvature)` — Eric Penner's "Pre-Integrated Skin Shading" (GPU Pro 2) is the canonical reference, with the Jorge Jiménez "Separable Subsurface Scattering" providing the practical real-time path. Per-mesh opt-in via a material flag.

**Hair and fur.** Pi/mid: shell rendering (4–8 extruded shells with alpha-textured strands). Hi: card-based hair (textured strips with anisotropic Kajiya-Kay shading).

**Destruction and deformation.** Pre-fractured meshes (offline Voronoi shatter); morph-target damage masks for partial destruction; procedural cracks via runtime UV decals. Engine ships the rendering primitive (instanced fragments + shader); physics integration is left to the user.

### 7. Look vs Performance Balance

**Scalability system.** A `QualitySettings` resource has both a coarse tier (`Low / Medium / High / Ultra`) and individual feature toggles.

Per-tier defaults:

| Feature | Low (Pi) | Medium (Deck) | High | Ultra |
|---|---|---|---|---|
| Render API | GLES if needed | Vulkan | Vulkan | Vulkan |
| Shadow res | 512² × 1 cascade | 1024² × 3 | 2048² × 4 | 4096² × 4 |
| Shadow filter | 3×3 PCF | 5×5 OptimizedPCF | 7×7 disc | 7×7 + PCSS |
| SSAO | off | half-res GTAO | full-res GTAO | GTAO + bent normals |
| SSR | off | half-res | full-res | full-res |
| Bloom | 3 mips | 5 mips | 7 mips | 7 mips |
| TAA | off | on | on | on |
| Resolution scale | 1.0 (1080p) | dynamic 0.7-1.0 | 1.0 | 1.0 (4K) |
| Volumetric fog | off | low-res | full-res (160×90×64) | full-res (160×90×128) |
| Particle count cap | 1024 | 8192 | 32768 | unlimited |
| IBL | low-res cubemap | full | full | full + parallax-corrected |

**LOD-aware shaders.** Author each material's fragment shader as one WGSL file with `@id`-overridable feature flags. Permutations: `HAS_NORMAL_MAP`, `HAS_OCCLUSION`, `HAS_EMISSIVE`, `IBL_QUALITY` (0=SH only, 1=SH+specular, 2=full split-sum). The chosen LOD tier dictates the permutation. Specialisation constants avoid combinatorial explosion in the shader cache.

**Baked vs real-time lighting.** Static scenes (Castlevania-style castles, Trails-style towns) lit primarily by baked lightmaps + occasional real-time hero lights. Dynamic scenes (open-world Yakuza, Elden Ring) use real-time clustered lighting + sparse probes. Rule of thumb: anything that doesn't move and won't be relit by the player should be baked.

**Checkerboard rendering.** Alternative to FSR upscaling: render half pixels each frame in a checkerboard, reconstruct via TAA history. Useful when GPU is fragment-bound but not bandwidth-bound (Steam Deck pattern). One-line jitter to projection matrix + checkerboard-aware TAA resolver.

**Approximations that look close.**
- SSAO vs GTAO: GTAO is barely more expensive and looks dramatically better. Always GTAO over SSAO. Bent normals require a second buffer; hi-tier only.
- Soft shadows: 5×5 OptimizedPCF (per MJP/Castaño Witness PCF) is the sweet spot.
- Reflections: SSR + probe fallback hides 90 % of SSR's miss artefacts at low cost.
- Bloom: 5 mip levels is the perceptual ceiling for 1080p.

**Dithering.**
- Alpha dither for transparency: screen-space Bayer/blue-noise pattern, threshold by alpha, then `discard` (or alpha-to-coverage on MSAA). Use for foliage, hair shells, distant fades.
- LOD transition dither: blend between two LOD meshes by dithering during a 0.5 s window when LOD switches. Eliminates LOD pop.

**Worth-the-cost matrix for Pi 4 tier:**

| Effect | Cost (ms) | Worth it? |
|---|---|---|
| Bloom (3 mips) | 0.4 | Yes |
| Tonemap | 0.1 | Yes |
| Color grading LUT | 0.05 | Yes |
| TAA | 0.8 | No (use jittered MSAA 2× if needed) |
| SSAO | 1.5 | No |
| SSR | 2.5 | No |
| Volumetric fog | 3.0 | No |
| Real-time shadows (1 cascade 512²) | 1.2 | Yes |
| 3 cascades 1024² | 4.0 | No |

**Profile-guided quality decisions.** On first run, execute a 30-second sustained-load benchmark on ARM (5-second on desktop) and pick the highest tier whose 95th-percentile frame time stays under 14 ms. Persist the choice. Re-run when the GPU adapter or driver version changes.

### 8. wgpu-Specific Rules and Gotchas

**Resource creation costs.** In descending order: `RenderPipeline` (ms-scale), `BindGroupLayout` and `PipelineLayout` (μs), `BindGroup` (μs), `Texture` (driver-dependent, expensive — allocates VRAM), `Buffer` (cheap to create, expensive to map), `ShaderModule` (expensive on first creation because of naga translation). Cache aggressively by content hash.

**Bind-group layout compatibility.** Two pipelines share a `PipelineLayout` iff every `BindGroupLayout` is identical entry-by-entry (binding number, visibility, type, count, has_dynamic_offset, min_binding_size all match). Define the four standard layouts as singletons in a `Layouts` resource.

**Pipeline caching.** Use `wgpu::PipelineCache` (Vulkan/DX12; not yet Metal) to persist driver-compiled shader binaries. Key by `(adapter name, driver version, engine version)`.

**Dynamic offsets.** Dynamic uniform offsets are 256-aligned by default (`min_uniform_buffer_offset_alignment: 256`); dynamic storage offsets the same. Plan per-instance UBO slots in 256-byte slots; one fits a 4×4 model matrix (64 B), an inverse-transpose 3×3 (48 B), and up to 144 B of misc.

**wgpu feature flags relevant to performance.**

| Feature | Use case | Backends |
|---|---|---|
| `INDIRECT_FIRST_INSTANCE` | per-instance batch keys | Vulkan/DX12 reliably; Metal varies |
| `TIMESTAMP_QUERY` | profiling | Vulkan/DX12; not TBDR |
| `PIPELINE_STATISTICS_QUERY` | profiling | Vulkan/DX12 desktop |
| `MULTI_DRAW_INDIRECT` | GPU-driven | Under `DownlevelFlags::INDIRECT_EXECUTION` per PR #8162 |
| `MULTI_DRAW_INDIRECT_COUNT` | GPU-driven w/ variable count | Vulkan/DX12 only; never Metal |
| `PUSH_CONSTANTS` | tiny per-draw data | Vulkan/DX12/Metal; not GLES |
| `TEXTURE_BINDING_ARRAY` | bindless | Vulkan/DX12/Metal (Apple4+) |
| `MAPPABLE_PRIMARY_BUFFERS` | direct GPU buffer map | Unified memory only |
| `SHADER_F16` | half-precision compute | Vulkan; not GLES |
| `SUBGROUP` | wave-ops | Vulkan/DX12 |
| `TIMESTAMP_QUERY_INSIDE_ENCODERS` / `_INSIDE_PASSES` | finer profiling | Native-only |

**Backend-specific patches.** Maintain a `BackendPatch` module:
- **Vulkan**: enable `VK_KHR_synchronization2` paths if available, use timeline semaphores instead of binary, use dedicated transfer queue, prefer `vkCmdDrawIndexedIndirectCount`. Note Vulkan 1.4 raised `maxPushConstantsSize` minimum from 128 to 256 bytes per the Vulkan Documentation Project: "Vulkan 1.4 introduced changes to some of these limits, notably increasing the guaranteed minimum value for maxPushConstantsSize from 128 bytes to 256 bytes."
- **Metal**: emulate `multi_draw_indirect_count` via CPU readback; cap push constants at 4 KiB (Apple Metal Feature Set Tables `setBytes:` 4 KB max); use `MTLArgumentBuffer` for bindless if the engine asks for `TEXTURE_BINDING_ARRAY`; prefer memoryless storage for transient attachments; set hazard tracking to `untracked` where Lunar synchronises manually.
- **DX12**: use `ExecuteIndirect` for multi-draw; root constants for push constants (≤ 32 DWORDs / 128 B at root signature limit unless full constant buffers).
- **GLES**: detect downlevel flag set at startup; refuse to enable any tier above `Low`; force forward renderer; force MSAA off (too much bandwidth on V3D); use `glBufferSubData` semantics for per-frame uploads.

**GLES/OpenGL fallback for Pi VideoCore VI / Mali.** The GLES backend lacks compute, indirect execution, fragment storage, vertex storage, base vertex, independent blending, read-only depth-stencil (gfx-rs/wgpu#2275 warning enumeration). The engine must:
1. Refuse to compile materials that use storage buffers in their fragment shader.
2. Refuse to register render-graph nodes that dispatch compute.
3. Emulate skinning bone matrices via a 2D RGBA32F texture sampled with `textureLoad`.
4. Hard-cap MRT count at 4 (V3D and OpenGL ES minimum).
5. Use only `u16` indices (some GLES drivers slow-path u32).

**Surface configuration.** wgpu v29+ changed the API: `Surface::get_current_texture` returns a `CurrentSurfaceTexture` enum with variants `Success`, `Suboptimal(frame)`, `Timeout`, `Occluded`, `Outdated`, `Lost`, `Validation` (per the wgpu CHANGELOG, PRs #9141, #9257). Handling rules:
- `Success` → render normally.
- `Suboptimal(frame)` → render this frame, then reconfigure surface at frame end.
- `Outdated` → reconfigure immediately, skip this frame.
- `Timeout` / `Occluded` → skip this frame.
- `Lost` → reconfigure surface; if it fails, recreate the device (device-lost recovery).
- `Validation` → log and skip.

**Error handling and device lost.** Install error scope (`Device::push_error_scope` / `pop_error_scope`) around pipeline creation in debug. In release, listen to `Device::on_uncaptured_error`. On `DeviceLost`, recreate device, reupload all GPU resources from CPU mirror copies (so every asset retains a CPU-side decoded form for the lifetime of the run), and rebuild all pipelines. Plan for 1–2 s freeze; show a "reconnecting GPU" overlay.

### 9. Hard Rules and Axioms

**Absolute hard rules (codebase-wide).**
1. **No CPU-side wait on `Device::poll(WaitForSubmissionIndex)` from the render thread on the steady-state path.** Only acceptable in startup/shutdown/asset-load.
2. **No allocations in the render hot path.** Use the per-frame bump arena.
3. **No `Mutex`/`RwLock` in the render hot path.** Use `&World`, `Res`/`ResMut`, `Atomic*`, or sharded per-thread data.
4. **No GPU readback in the steady-state path on any backend.** Especially fatal on ARM. Even `Queue::on_submitted_work_done` is acceptable; the rule is never *await* GPU data.
5. **No shader compilation on the render thread mid-frame.** Compile during Queue stage (Bevy pattern). Show a "compiling shaders" splash when pipelines are pending at level-load.
6. **Every render-graph node must declare every resource it reads or writes.** No hidden aliasing.
7. **Every `wgpu::Buffer` and `wgpu::Texture` must have a non-empty `label`.** For RenderDoc / Xcode capture sanity.
8. **All structs that go to the GPU are `#[repr(C)]` + `bytemuck::Pod + Zeroable`.** No `#[repr(Rust)]` reaching the GPU.
9. **All matrices follow column-major + reverse-z convention.** Reverse-z (near=1, far=0) gives substantially better depth precision.
10. **No `Vec3` (12-byte) in GPU structs; use `Vec4` or `[f32; 4]`** because std140/std430 packing rules silently expand Vec3 to Vec4 anyway.
11. **A render-graph node must complete in ≤ 2 ms on Pi 4 hardware** unless it is the main opaque pass.
12. **No mutable global statics.** Use Bevy resources.
13. **Every feature gated behind `2d` / `3d` cargo features is independently testable.** Both must compile clean on every target.

**Anti-patterns to never do.**
- Creating `RenderPipeline` inside the render hot path.
- Creating `BindGroup` per draw call (only per material / per instance batch).
- Using `wgpu::Buffer::map_async` and awaiting it inside the render path.
- Stalling on the swapchain (`Surface::get_current_texture` blocks) without a timeout fallback.
- Submitting individual command buffers per draw; always batch into one submit per frame.
- Allocating textures inside the render path; only at level-load or via the transient aliasing allocator.
- Calling `Device::poll(Maintain::Wait)` on the render thread in steady state.
- Mixing `f64` into GPU structs (most backends only handle `f32` / `f16`).
- Re-encoding render bundles per frame.
- `#[derive(Component)]` on a 1 KB+ struct then querying it per frame (split into hot + cold components).

**Shader authorship (WGSL) rules.**
- `let` for non-mutable bindings, `var` for mutable. Prefer `let`.
- Pre-multiply at compile time when possible (`const` evaluator handles `const` arithmetic).
- Avoid `discard` on TBDR. Use alpha-to-coverage for cutouts with MSAA.
- Branch on uniform values freely; per-invocation branching sparingly — divergence hurts wavefront utilisation.
- Use `select(a, b, cond)` instead of `if/else` where a single expression suffices.
- Non-uniform texture-array indexing requires `Features::SAMPLED_TEXTURE_AND_STORAGE_BUFFER_ARRAY_NON_UNIFORM_INDEXING`.
- Compute workgroup size respects `max_compute_invocations_per_workgroup = 256` and per-dim limits (`x: 256, y: 256, z: 64`). Standards: `(8,8,1)` image-space; `(64,1,1)` linear arrays; `(8,8,8)` 3D volumes.
- `workgroupBarrier` is the only cross-invocation sync; use sparingly.
- Entry points must be named distinctly (e.g. `vs_main`, `fs_main`, `cs_main`) per modern WGSL.

**ECS component design rules (rendering-affecting).**
- A component used by every frame's render extract must be `Copy`, ≤ 128 B, and Table-stored.
- A component that toggles state every frame must be SparseSet-stored.
- Never store handles by value if they're > 8 B; use `Handle<T>` (8 B).
- Pair commonly-co-queried components into a `Bundle` so they land in the same archetype table.
- Avoid generic `<T>` components in render-hot paths; each `T` generates a distinct archetype.

**Asset pipeline design rules.**
- Source assets (`.png`, `.gltf`, `.wav`) processed offline into engine-native (`.mrtex`, `.mrmesh`, `.mrmat`).
- Engine-native formats are zero-copy: `mmap` the file, cast to a header via `bytemuck`, use slices as GPU upload sources.
- Texture compression: ASTC LDR 6×6 default for non-Apple PC + mobile (ARM-software/astc-encoder Format Overview: "ASTC at 3.56 bpt outperforms PVRTC and BC1 at 4 bpt by ~1.5dB, and ETC2 by ~0.7dB"); BC7 for Windows desktop (per Aras Pranckevičius: "BC7 achieving very similar quality to ASTC 4x4, while being faster to compress"). Ship every texture twice (BC7 + ASTC) and pick at install.
- Mesh positions f32 by default; quantised i16 positions as opt-in for huge worlds.
- Audio handled entirely by Moonwalker; render pipeline must not touch audio buffers.

**Naming and organisation.**
- Crates: `lunar-core` (ECS, math, time), `lunar-render` (engine), `lunar-2d`, `lunar-3d`, `lunar-asset`, `lunar-backend`.
- Module per render-graph node: `mod opaque_pass`, `mod shadow_pass`, `mod post`.
- Public API `snake_case`, types `CamelCase`, render-graph node IDs `SCREAMING_SNAKE_CASE` consts.
- Shader files in `assets/shaders/`, organised by feature: `pbr/`, `2d/`, `post/`, `compute/`.
- Buffer labels: `[subsystem] descriptor` (e.g. `[shadow] cascade-0 depth`).

### 10. ARM / AARCH64 Specific Considerations

**Tile-based deferred rendering (TBDR).** All ARM-class GPUs Lunar targets (Mali on Steam Frame, Apple Silicon, VideoCore VI on Pi 4) are tile-based. Arm developer docs: "Mali GPUs use a tile-based rendering architecture. This means that the GPU renders the output framebuffer as several distinct smaller sub-regions called tiles." Apple A11+ provides 32 KB of threadgroup memory and 32 KB of explicit imageblock allocation per tile (Apple Metal Feature Set Tables). V3D on Pi 4 uses 64×64 (non-MSAA) / 32×32 (MSAA) tiles per the VC4 family Mesa docs: "VC4 is a tiled renderer, chopping the screen into 64x64 (non-MSAA) or 32x32 (MSAA) tiles and rendering the scene per tile." V3DV's "double buffer mode" reduces tile size to overlap stores with the next tile's processing (Igalia blog 2022: "we have implemented double buffer mode... split the tile buffer size in half, so the driver could start processing the next tile while the current one is being stored in memory").

**Bandwidth is the primary bottleneck.** Pi 4 LPDDR4: peak ~6.4 GB/s. Steam Deck LPDDR5: 88 GB/s (Chips and Cheese: "four 32-bit channels and runs at 5500MT/s, so the theoretical bandwidth should be 88GB/s"). Apple M-series unified: ~100 GB/s baseline. At 1080p60 the framebuffer alone is 0.5 GB/s; a typical hi-end deferred pipeline can hit 8 GB/s. On Pi 4 you cannot afford deferred at all; on Steam Deck it's affordable but expensive. Arm Mali docs (G720 Deferred Vertex Shading): bandwidth savings translate "directly into lower power consumption and extended thermal headroom"; specific per-title figures of 40% reduction often quoted from Arm's marketing should be treated as ranges (the Arm AFBC product page states only that combined techniques can save "up to 50 percent" of system-level bandwidth).

**Render-pass load/store ops — the bandwidth lever.** Every `wgpu::Operations { load, store }` choice has direct bandwidth cost on TBDR:
- `LoadOp::Load` → driver fetches previous attachment contents into tile memory at pass start. **Expensive.** Only when blending/compositing over existing content.
- `LoadOp::Clear` → no fetch; tile memory initialised to clear value. **Cheap.** Use whenever overwriting.
- `StoreOp::Store` → tile memory written back to DRAM at end of pass. **Expensive.**
- `StoreOp::Discard` → tile memory never persisted; resource is memoryless for this pass. **Free.** Use for depth/stencil that's only needed within the pass and intermediate G-buffers when using on-tile deferred (Apple PLS / Vulkan input attachments).

Hard rule: every render pass attachment must explicitly justify any `LoadOp::Load` or `StoreOp::Store`.

**Avoid GPU readback.** Readback on ARM forces full pipeline flush and stalls the renderer until the GPU is idle (~1 full frame on Mali, comparable on V3D). Never readback in the steady-state loop. If you need data from the GPU (picking, screenshot, occlusion results), schedule async and accept N-frame latency. On Metal: never set `MTLBlitCommandEncoder::synchronize` on the steady path.

**Texture formats for ARM.** ASTC is universally available on Mali, Apple, and modern Adreno; V3D advertises BC and ETC2 (not ASTC). Defaults:
- Apple, Mali: ASTC 6×6 for albedo (3.56 bpp), ASTC 4×4 for normals (8 bpp).
- V3D / Pi 4: ETC2 RGB at 4 bpp for albedo; EAC RG for normal maps.

Always generate complete mip chains; runtime mip generation kills bandwidth.

**Memory pressure with 2 GB unified.** Pi 4 2GB model: budget 256 MB for GPU resources, 256 MB for the engine + game code + audio (Moonwalker), and leave the rest for OS / browser / web view. Strategies:
- Stream textures mip-by-mip. Never load full mip chain for distant assets.
- Cap shadow atlas at 512² × 1 cascade (512 KB at D32).
- Single colour + single depth target at 1080p ≈ 16 MB total; transient allocator must not exceed 32 MB live.
- Disable particle GPU sim (no compute on GLES); fall back to CPU sim.

**NEON SIMD for CPU-side math.** glam enables NEON by default on aarch64 (glam docs: "NEON is enabled by default on aarch64 targets"). Confirm via `cfg(target_feature = "neon")`. For hand-tuned math, use `core::arch::aarch64` intrinsics directly. Patterns:
- 4 plane tests per AABB: load `plane.xyzw` into one NEON register; broadcast AABB centre + extent; one `vfmaq_f32` per plane.
- 4 quaternion multiplies in parallel: load 4 quats into 4 registers, swizzle and FMA.

Pi 4's Cortex-A72 has 128-bit NEON dual-issue (Arm Cortex-A72 reference manual specifies dual-issue ASIMD/NEON pipelines). A well-tuned NEON culling kernel sustains tens of millions of AABB tests per second per core; treat it as roughly 4× the scalar throughput rather than a fixed GFLOPS figure (vendor-published peak FLOPS for the A72 vary, so use measured benchmarks per workload).

**Thermal envelope and sustained performance.** Pi 4 throttles between 80 °C and 85 °C per Raspberry Pi documentation. Steam Deck has a 4–15 W APU power envelope (Valve tech page: "APU power: 4-15W"). Strategies:
- Target 30 % thermal margin on first run; under-use the chip during initial benchmark so steady-state thermals stay manageable.
- Always cap framerate to vsync; never run uncapped on ARM.
- Disable optional features (volumetric fog, hi-res shadows) for adapters in a known thermally-constrained class.
- Run a sustained-load benchmark for 30 s, not 5 s, when picking a default quality tier on ARM.

## Recommendations

1. **Start with the render-graph + sub-app pattern from day one.** Don't retrofit. Implement the four standard `BindGroupLayout`s as the foundation; everything else builds on them.
2. **Build the Pi 4 path first.** Counter-intuitive but correct: if you build the high-end first, you'll bake assumptions (compute culling, indirect drawing, push constants > 0) that you cannot back out of for the GLES fallback. The Pi imposes the discipline that produces a portable engine.
3. **Adopt the wgpu default `Limits`** as the engine's common floor and only request elevated limits per-tier. Anything that needs > `max_bind_groups = 4` is a design smell.
4. **Ship a benchmark scene with the engine.** It chooses the quality tier on first launch. Re-run when the GPU adapter or driver version changes.
5. **Maintain a `BackendPatch` trait** with explicit per-backend implementations. Don't paper over backend differences with abstraction — make them visible.
6. **CI matrix:** compile `--features 2d`, `--features 3d`, both, and neither on x86_64-linux, x86_64-windows, aarch64-linux (cross from Pi sysroot), aarch64-apple-darwin. Catch feature-gate regressions early.
7. **For 2D titles, the 2D feature alone is enough — do not enable 3D.** The compile-time savings (~30 % shader cache, no PBR module) pay for themselves.
8. **For first-person 3D titles (Postal/Quake/Halo style), enable both `3d` and `2d`** (you need 2D for HUD and menus).

**Benchmarks that change recommendations:**
- If sustained average frame time on Pi 4 at 1080p in the standard test scene exceeds **14 ms**, drop to 900p internally and upscale.
- If GPU memory on Steam Deck regularly exceeds **3.5 GB** in test scenes, increase streaming aggressiveness (smaller chunk radius).
- If `wgpu::Adapter::limits()` reports `max_push_constant_size = 0` on a target you care about, use UBOs for per-draw data — don't insist on push constants.
- If GPU time for the shadow pass exceeds **2 ms** on mid tier, reduce cascade resolution before any other quality.

## Caveats

- **wgpu API has shifted across versions**, notably `Limits` field types (u32 → u64 for buffer-binding limits in recent versions) and `SurfaceError` (replaced in v29+ with `CurrentSurfaceTexture` enum). Pin to a specific wgpu version per Lunar release and re-vet this document on each upgrade.
- **Mesh shaders are partially landed in wgpu** (per the gfx-rs/wgpu v29 mesh_shading API doc) but require backend support (Vulkan with `VK_EXT_mesh_shader`, Metal 3 with mesh stages). Treat as v2 optimisation.
- **Persistent buffer mapping is not part of WebGPU semantics**, so wgpu does not expose it outside `MAPPABLE_PRIMARY_BUFFERS` (gfx-rs/wgpu#1468). On WebAssembly targets, always go through the staging belt.
- **Memory-less / lazily-allocated render targets** (the bandwidth-saving trick on TBDR) are not yet a portable wgpu concept. Achieve the effect through `StoreOp::Discard` and not sampling the attachment; driver heuristics handle the rest, but you cannot enforce it.
- **The Pi 4 GFLOPS number varies by counting methodology**: Idein's primary-source breakdown gives 32 GFLOPS theoretical; other reputable sources cite 16 or ~4.4 GFLOPS for the same hardware. Always specify the formula when quoting performance numbers.
- **The Pi 4 memory speed varies by board revision**: original Raspberry Pi docs cite LPDDR4-2400; some secondary sources cite LPDDR4-3200 for later revisions. Detect the board revision at startup if your memory-budget heuristic depends on bandwidth.
- **Steam Frame ARM target is speculative as of May 2026** — Valve has shown prototypes but final specs are not public. Treat as "Steam Deck class but ARM" until confirmed; assume Mali-class TBDR, 8 GB unified memory, lower clock.
- **Apple Silicon GPU exposes timestamp queries differently than the Vulkan model.** On wgpu Metal, `TIMESTAMP_QUERY: false` is reported on at least M1 Pro adapters; profile via Xcode's Metal capture instead. `MTLCounterSamplingPoint::AtStageBoundary` exists but is not surfaced through wgpu.
- **Many specific numerical defaults quoted here are from the docs.rs `wgpu::Limits` page** and reflect wgpu trunk as of mid-2025. They are stable in the WebGPU spec, but native-only knobs (`max_push_constant_size`, etc.) can drift.
- **The 2D / 3D feature lists in the task spec mix engine technique requirements** (e.g., "God of War 2018" needs PBR + IBL + atmospherics; "Earthbound" needs only a 2D sprite batcher). The engine targets the *technical superset* but ships them behind cargo features so an Earthbound-style game compiles without PBR shaders, atmospherics LUTs, or any 3D code.
- **Bandwidth-savings figures attributed to Arm Mali DVS for specific titles (e.g. 40 % for Genshin Impact/Fortnite)** circulate in third-party coverage but were not confirmed in primary Arm sources during research. Treat such figures as marketing-range estimates rather than hard contractual savings.