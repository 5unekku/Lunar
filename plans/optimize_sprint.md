# optimize sprint

performance work derived from `plans/optimize.md`. ordered by roi. none of this is
feature work — everything here makes existing features faster or cheaper.

---

## 1. gpu instancing

every entity is its own draw call. batching entities that share a `(mesh_id, material_id)`
into one `draw_indexed` with an instance buffer is the single highest-roi change.

what to do:
- in the gather pass, group `draw_scratch` entries by `(mesh_id, material_id)` key
- build a per-group instance buffer: `[model_matrix: Mat4, material_index: u32, _pad: [u32; 3]]` per instance
- issue one `draw_indexed` per group with `instance_count = group.len()`
- vertex shader reads transform from instance buffer via `@builtin(instance_index)`

existing dynamic entity UBO already packs transforms contiguously — the upload side is
close to ready. the main work is the draw call consolidation and shader change.

gate: all tiers. no downlevel flags required for basic instancing.

---

## 2. separate position-only vertex stream for shadow/depth passes

shadow passes currently pull full interleaved vertex data (40 bytes/vertex) when they
only need positions (12 bytes). that is 3.3× the bandwidth for no benefit.

what to do:
- split mesh uploads into two buffers: `positions: Vec<[f32; 3]>` and `attributes: Vec<VertexAttributes>`
- shadow pass and z-prepass bind only the position buffer
- main opaque pass binds both (interleaved or split, whichever benchmarks better)

impact: 4–6× bandwidth reduction in shadow passes per optimize.md. on Pi 4 where shadow
passes cost ~1.2ms of a 16.67ms budget, this is meaningful.

---

## 3. u16 index buffers

use `IndexFormat::Uint16` wherever vertex count ≤ 65535. halves index buffer size and
bandwidth. most meshes (characters, props, foliage) are well under this limit.

what to do:
- in `gen-lods` and mesh upload, check `vertex_count <= u16::MAX`
- emit `Uint16` indices when true, `Uint32` only when needed
- update upload and draw calls to use the correct format

mechanical change with broad impact.

---

## 4. `StoreOp::Discard` and `LoadOp::Clear` audit

on TBDR backends (Mali, Apple, V3D), every incorrect load/store op costs a full
framebuffer read or write. currently unaudited.

what to do:
- every render pass attachment must justify `LoadOp::Load` (only when blending over
  prior content) or `StoreOp::Store` (only when the result is sampled later)
- intermediate targets that are not sampled afterward: `StoreOp::Discard`
- depth/stencil within a pass only: `StoreOp::Discard`
- MSAA resolve source after resolve: `StoreOp::Discard`
- GTAO half-res intermediate: `StoreOp::Discard`

add a lint: in debug builds, warn when a `StoreOp::Store` attachment is never subsequently
bound as a texture or sampled.

---

## 5. WGSL `@id` specialization constants

currently each material feature combination (has_normal_map, has_emissive, ibl_quality, etc.)
likely produces a separate shader module. wgpu/WGSL supports `@id`-overridable constants
that let one module yield many pipelines via compile-time specialization:

```wgsl
@id(0) override HAS_NORMAL_MAP: bool = false;
@id(1) override IBL_QUALITY: u32 = 1u;
```

pass `PipelineDescriptor::constants` map at pipeline creation time. one module, many
pipelines, smaller shader cache, faster cold startup.

what to do:
- identify the current permutation axes in `shader.wgsl` (feature flags that drive
  `#ifdef`-style branching or separate shader variants)
- convert to `@id override` constants
- update pipeline creation to pass the constant map per-material

---

## 6. `MAPPABLE_PRIMARY_BUFFERS` on unified memory

on Apple Silicon and Steam Deck (unified CPU/GPU memory), `Queue::write_buffer` copies
through an implicit staging buffer. with `Features::MAPPABLE_PRIMARY_BUFFERS` you write
directly into the GPU buffer, eliminating one copy for skinning matrices, particle data,
and per-frame uniforms.

what to do:
- detect unified memory at adapter creation: check `AdapterInfo::device_type ==
  DeviceType::IntegratedGpu` and request `MAPPABLE_PRIMARY_BUFFERS` when available
- for skinning and particle SoA buffers, allocate with `MAP_WRITE | STORAGE` usage and
  map directly each frame instead of going through staging

gate behind the feature flag; fall back to current staging belt path on discrete GPUs.

---

## 7. FSR 3 upscaling (accessibility tier)

upscaling exists as an accessibility lever for minimum-spec hardware, not as a crutch
for poor performance. high-end targets run at native resolution. low tier drops resolution
and upscales. no frame generation — generated frames add input latency and are not a
substitute for native performance.

**fsr 3 source situation:** AMD ships FidelityFX SDK under MIT at
`GPUOpen-LibraryAndSDKs/FidelityFX-SDK`. the upscaling passes (EASU + RCAS) are HLSL.
port to WGSL — EASU is ~300 lines, RCAS ~100. separate from the frame generation
component, which we are not implementing.

what to do:
- port FSR 3.1 EASU and RCAS passes from the FidelityFX SDK to WGSL
- wire as the final pass on `QualityPreset::Minimum` when render resolution < display
  resolution (default 0.75× on Pi tier)
- `QualityPreset::Medium` and above: native resolution, upscaling off
- expose `render_scale: f32` in `QualitySettings`; dynamic resolution scaler drives it;
  FSR activates when `render_scale < 1.0`

---

## non-items

- **motion blur**: excluded. adds latency perception, rarely improves the experience,
  and costs a full-res velocity-buffer pass. not worth it.
- **frame generation (FSR 3 FG)**: excluded. adds input latency and is not a substitute
  for native performance.
- **depth of field**: feature, not performance work. out of scope here.
- **OIT**: feature work. out of scope.
