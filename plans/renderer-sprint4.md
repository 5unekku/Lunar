# renderer sprint 4 — L and J

items L (CPU-GPU pipelining) and J (GPU-driven indirect rendering).
these interact: J eliminates the cull readback stall that L is partially
trying to fix. the right order is L first (clean up the sync points),
then J (replace the draw loop entirely).

---

## current sync points

before starting either item, the exact stall locations:

**line 4130**: `device.poll(wgpu::PollType::wait_indefinitely())` — cull readback.
despite the "non-blocking" comment above it, this blocks the CPU until the GPU
finishes the previous frame's cull compute pass. with vsync this rarely stalls
in practice (the GPU finishes in ~8ms, vsync gives it 16ms), but it *can* stall
on fast CPUs or when GPU cull takes longer than expected.

**line 4245**: same pattern for HZB occlusion cull readback.

**line 6151**: `staging_belt.recall()` — called immediately after `frame.present()`.
this signals the staging belt that previously submitted writes can be recycled.
wgpu's `StagingBelt::recall` is *not* blocking on its own, but it is called
before the GPU has consumed the staged uploads for the current frame. wgpu
handles this correctly internally — recall marks chunks for potential reuse,
but a chunk is only actually reused after the submitted commands that read it
have completed. this is safe and non-stalling as written.

**summary**: the two `wait_indefinitely()` calls are the only real CPU stalls.
`staging_belt.recall` and `frame.present` do not stall the CPU on wgpu/wgsl
with a properly-configured swap chain.

---

## item L — eliminate the wait_indefinitely stalls

### root cause

the existing "1-frame pipelined" cull readback is implemented incorrectly:
it calls `map_async` then immediately `wait_indefinitely()`, which is identical
to a synchronous stall. the "1-frame" part is that the *data* is from the
previous frame, but the *CPU* still waits for that frame's GPU work to complete
before proceeding.

correct 1-frame pipelining reads staging data that has already had an entire
frame to finish, without blocking:

```
frame N:
  (GPU): dispatch cull → copy flags to staging_buf → submit
  (CPU): does NOT read staging_buf

frame N+1 start:
  device.poll(Poll)          // flush any completed callbacks (non-blocking)
  if buffer is already mapped:
      read data immediately
  else:
      use stale result from N-1 (no stall, just one extra frame of latency)
```

### implementation plan

**step 1: replace blocking poll with a pre-mapped staging buffer**

use two staging buffer slots (`cull_flags_staging: [Option<wgpu::Buffer>; 2]`) and a
frame parity bit. frame N writes to slot A; frame N+1 reads from slot A (which had
a full frame to complete) and writes to slot B.

at the end of frame N, after submitting:
```rust
// map slot A for reading NEXT frame — purely registers intent, does not block
let staging_slice = staging_buf.slice(0..size);
staging_slice.map_async(wgpu::MapMode::Read, move |result| {
    if result.is_ok() { parity_ready.store(true, Ordering::Release); }
});
```

at the start of frame N+1:
```rust
device.poll(wgpu::PollType::Poll); // non-blocking flush
if parity_ready.load(Ordering::Acquire) {
    // read mapped data
    staging_buf.unmap();
    parity_ready.store(false, Ordering::Release);
} else {
    // GPU not done yet — use last known flags, no stall
    log::warn!("cull staging not ready, using stale flags");
}
```

the `Arc<AtomicBool>` is shared between the `map_async` callback closure and the
main render thread. this is the only synchronization primitive needed.

**step 2: same fix for HZB staging (lines 4239–4258)**

identical pattern. two staging slots, AtomicBool, map at end of frame, read
at start of next.

**step 3: verify staging_belt.recall placement**

`staging_belt.recall()` at line 6151 is called after `queue.submit()`. this is
correct but sub-optimal: it reclaims staging memory that may still be in the GPU
queue. wgpu tracks this correctly (a reclaimed chunk is only reused after its
commands complete), so there is no correctness issue. no change needed.

**step 4: remove the `device.poll(Poll)` pre-call at lines 4127 and 4242**

these were added as a workaround before the real fix. once the AtomicBool pattern
is in place, the pre-call is unnecessary.

### files

- `crates/lunar-render-3d/src/lib.rs` — all changes
  - two new fields: `cull_staging_ready: Arc<AtomicBool>`, `hzb_staging_ready: Arc<AtomicBool>`
  - ping-pong staging buffers: `cull_flags_staging: [Option<wgpu::Buffer>; 2]`
  - `hzb_occ_staging: [Option<wgpu::Buffer>; 2]` (already `Option<>`, add second slot)
  - remove both `wait_indefinitely()` calls
  - `staging_belt.recall()` can be called immediately after submit (already is — no change)

### expected outcome

the CPU never blocks on GPU work. on a frame where the GPU takes longer than the
CPU (GPU-bound scene at 30fps), the CPU uses the previous frame's cull result
(2 frames old instead of 1) for that one frame. this is imperceptible.

---

## item J — GPU-driven indirect rendering

### why the existing implementation doesn't pipeline

the cull shader writes `visible_flags[i] = 1` or `0`. the CPU reads this back,
builds draw calls in `draw_scratch`, and issues `draw_indexed` per batch. this
means the CPU is doing work proportional to `O(visible_entities)` every frame
regardless of whether L fixes the stall.

GPU-driven indirect eliminates the per-entity CPU draw cost entirely: the GPU
decides what is visible and writes the draw commands; the CPU issues one call.

### prerequisite features

all three are gated behind `RenderTier::High`:

| feature | wgpu flag | available on |
|---|---|---|
| indirect execution | `DownlevelFlags::INDIRECT_EXECUTION` | already checked for High tier |
| multi draw indirect | `Features::MULTI_DRAW_INDIRECT` | Vulkan 1.1+, DX12 |
| indirect first instance | `Features::INDIRECT_FIRST_INSTANCE` | same |

request both at device creation (alongside `RG11B10UFLOAT_RENDERABLE`):
```rust
let has_indirect = adapter.features().contains(
    wgpu::Features::MULTI_DRAW_INDIRECT | wgpu::Features::INDIRECT_FIRST_INSTANCE
);
```

add `has_indirect: bool` to `RenderEngine3d`. gate all J work behind this flag.

### the bind group problem

`multi_draw_indexed_indirect` submits N draw calls with the same bind groups. we
currently change bind groups 1 (material) and 4 (lightmap) between batches. that
must move to per-entity indexing inside the shader.

**material bind group (group 1)**: change from dynamic-offset UBO to a flat
storage buffer indexed by `@builtin(instance_index)`:
```wgsl
// group 1 (new)
@group(1) @binding(0) var<storage, read> materials: array<MaterialUniforms>;
// in fragment shader:
let material = materials[in.instance_id];
```
this requires passing `instance_index` through from vertex to fragment, and
removing the dynamic offset from `material_bgl`.

**lightmap bind group (group 4)**: binding per draw is impossible with multi-draw.
two options:

**option A — texture atlas** (preferred, more portable): pack all lightmap textures
into one texture atlas at level load time. entities store a `lm_uv_offset: vec2`
and `lm_uv_scale: vec2` in their material data. the fragment shader computes
`atlas_uv = lm_uv_offset + in.uv_lightmap * lm_uv_scale`. group 4 becomes a
single atlas texture bound once per frame.

**option B — binding array** (requires `TEXTURE_BINDING_ARRAY`): declare
`binding_array<texture_2d<f32>, MAX_LIGHTMAPS>` in the shader and index by a
per-entity lightmap index stored in material data. requires
`Features::TEXTURE_BINDING_ARRAY | Features::PARTIALLY_BOUND_BINDING_ARRAY`.
not available on all Metal configurations.

the plan uses option A. it requires a lightmap atlas builder (offline) but avoids
the binding array feature requirement.

### implementation phases

**phase 1: material storage buffer (independent of indirect)**

this is a standalone improvement with value regardless of J. do it first.

- change `material_bgl` from dynamic-offset UBO to storage read-only
- remove `has_dynamic_offset: true` and `min_binding_size` constraint
- material_buf becomes `STORAGE | COPY_DST` (not UNIFORM)
- in `shader.wgsl`: `@group(1) @binding(0) var<storage, read> materials: array<MaterialUniforms>`
- vertex shader: pass `instance_id: u32` from `@builtin(instance_index)` through to fragment
- fragment shader: `let material = materials[in.instance_id]`
- draw loop: no longer needs `set_bind_group(1, material_bg, &[slot_offset(i)])` per batch —
  set once at pass start
- `static_bundle`: remove material set_bind_group calls

this simplifies the draw loop significantly and is a prerequisite for J.

**phase 2: indirect draw buffer (CPU writes, no GPU cull yet)**

new buffers:
```rust
indirect_buf: wgpu::Buffer  // DrawIndexedIndirect × max_entities
indirect_count_buf: wgpu::Buffer  // u32 draw count
```

`DrawIndexedIndirect` layout (wgpu `util::DrawIndexedIndirectArgs`):
```
index_count: u32,
instance_count: u32,   // = 1 for now
first_index: u32,
base_vertex: i32,
first_instance: u32,   // = entity's slot in entity_buf
```

after sorting draw_scratch, CPU builds the indirect buffer instead of issuing
draw calls. one entry per entity (not per batch — instancing collapses same-mesh
runs). issue with `draw_indexed_indirect` per entity initially (same call count,
but sets up the buffer infrastructure).

at this point: no perf gain yet, but the data pipeline is in place.

**phase 3: lightmap atlas**

build a `LightmapAtlas` at level load: pack all lightmap textures into one RGBA8
texture (max 4096×4096). store per-entity `(uv_offset, uv_scale)` in the material
storage buffer. group 4 becomes a single atlas bind group set once per pass.

this requires an offline atlas packer (separate sprint) or a runtime packer on
first use. for the plan, assume atlas is built at level load from already-loaded
lightmap textures.

**phase 4: GPU cull writes DrawIndexedIndirect**

extend `cull.wgsl`:
- add input buffer: per-entity draw params `{index_count, first_index, base_vertex}`
  (CPU uploads this once per frame alongside AABBs, or only when draw_scratch changes)
- add output buffer: `DrawIndexedIndirect` array
- add output counter: `atomic<u32>` count of visible draws
- for each visible entity: `output[atomicAdd(count, 1)] = DrawIndexedIndirect { ..., first_instance: i }`

new bind group layout for cull:
```
binding 0: aabbs (storage read)
binding 1: params (uniform: frustum planes + entity_count)
binding 2: visible_flags (storage read_write — keep for L compatibility)
binding 3: draw_params (storage read — new)
binding 4: indirect_out (storage read_write — new)
binding 5: indirect_count (storage read_write, atomic — new)
```

CPU path becomes:
```rust
// zero the count
queue.write_buffer(&indirect_count_buf, 0, bytemuck::bytes_of(&0u32));
// dispatch cull (writes both visible_flags and indirect_out)
// ...
// draw
pass.multi_draw_indexed_indirect_count(
    &indirect_buf, 0,
    &indirect_count_buf, 0,
    max_draws,
);
```

the `visible_flags` buffer is still written (for game code and shadow lists that
still read it). the readback and staging from L becomes optional since game code
is the only consumer — and game code can read from the CPU-side cached flags.

**phase 5: remove cull readback entirely (L + J converge)**

once the GPU writes draw commands directly, the CPU never needs `visible_flags`
for rendering. the L staging readback can be:
- removed entirely if no game code uses `frustum_visible` 
- or kept as an async background readback for game-code queries (AI LOS, etc.)
  but without blocking render submission

### files

- `crates/lunar-render-3d/src/lib.rs` — phases 1, 2, 4, 5
- `crates/lunar-render-3d/src/shader.wgsl` — phase 1 (material storage), pass instance_id
- `crates/lunar-render-3d/src/cull.wgsl` — phase 4 (add indirect output)
- `crates/lunar-3d/src/lib.rs` / new crate — phase 3 (lightmap atlas builder)

### recommended order

1. phase 1 (material storage buffer) — standalone, simplifies draw loop, low risk
2. item L (async cull readback) — eliminates the wait_indefinitely stalls
3. phase 2 (indirect buffer, CPU-filled) — sets up infrastructure
4. phase 3 (lightmap atlas) — requires offline tool or runtime packer
5. phase 4 (GPU cull writes indirect) — final payoff: O(1) CPU draw submission
6. phase 5 (remove readback) — cleanup after 4 is stable

### expected outcome after all phases

| metric | before | after |
|---|---|---|
| CPU draw submission | O(visible_entities) | O(1) |
| cull readback stall | 0.1–0.5ms (sporadic) | 0 |
| material bind group changes/frame | N per pass | 0 |
| lightmap bind group changes/frame | N per pass | 0 |
| multi_draw_indirect support required | no | yes (High tier only) |

on scenes with 1000 visible entities (heavy outdoor level), CPU render time
for the opaque pass drops from ~2ms to ~0.05ms. the GPU does the same work;
only CPU overhead is eliminated.
