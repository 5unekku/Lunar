# renderer sprint 3

completing the items incorrectly deferred from sprint 2.

---

## item H — R11G11B10Float HDR render target

### problem

HDR intermediate target is `Rgba16Float` — 8 bytes/texel. at 1920×1080 that is
~16MB read and written by every post-processing pass. `Rg11b10Ufloat` stores the
same range in 4 bytes/texel (half the bandwidth) at the cost of no negative values
and 5-bit blue channel precision, both of which are fine for typical scene lighting.

### fix

at adapter init, check for `wgpu::Features::RG11B10UFLOAT_RENDERABLE`. if present,
request the feature and use `Rg11b10Ufloat` for the HDR target. if absent (GLES, some
older DX11 hardware), fall back to `Rgba16Float` as before.

the format is determined once at `from_surface` time and stored as a field. all
pipelines (bloom, composite, FXAA, SSR, fog, etc.) are compiled with that format;
no runtime pipeline recompilation needed.

```rust
// at adapter query time:
let has_r11 = adapter.features().contains(wgpu::Features::RG11B10UFLOAT_RENDERABLE);
let requested_features = if has_r11 {
    wgpu::Features::RG11B10UFLOAT_RENDERABLE
} else {
    wgpu::Features::empty()
};
let hdr_format = if has_r11 {
    wgpu::TextureFormat::Rg11b10Ufloat
} else {
    wgpu::TextureFormat::Rgba16Float
};
// store hdr_format as a field on RenderEngine3d
// replace the HDR_FORMAT const with self.hdr_format everywhere
```

### acceptance criteria

- on Vulkan/DX12/Metal: `Rg11b10Ufloat` is selected and HDR bandwidth drops 2×
- on GLES / unsupported hardware: `Rgba16Float` is used, no regression
- all post-processing passes (bloom, composite, SSR, fog) work correctly with both
  formats

---

## item K — RenderBundle for static geometry

### problem

the opaque draw loop issues N individual wgpu draw calls per frame for static level
geometry (walls, floors, ceilings) even though their transforms and materials never
change. recording a `wgpu::RenderBundle` once and replaying it eliminates per-draw
API overhead for these entities — the modern equivalent of Quake's display lists.

### design

**new component**: `StaticMesh` marker in `lunar-3d`. tagging an entity with this
tells the renderer its transforms/materials are immutable across frames.

**stable slot layout**: static entities occupy the FIRST N slots of the entity
storage buffer (after the fixed SLOT_DOME and SLOT_SUN). dynamic entities follow.
this lets the bundle hardcode instance ranges that remain valid across frames.

```
slot 0           = SLOT_DOME (sky dome)
slot 1           = SLOT_SUN
slot 2..2+Ns     = static entities (stable, sorted by mesh_id/mat_id/lm_id)
slot 2+Ns..total = dynamic entities (rebuilt every frame)
```

**recording trigger**: when the static entity set changes (entity added, removed, or
any static entity's mesh/material changes), re-sort, re-upload slot data, re-record
the bundle. in steady state (no changes), the bundle is replayed from cache.

**bundle invalidation**: a `RenderBundle` is tied to a specific (color_format,
depth_format, sample_count) at record time. store these alongside the bundle and
re-record if any changes (e.g. MSAA toggle, HDR format change on resize).

### per-frame cost

static geometry: `execute_bundles(&[&bundle])` — one wgpu call regardless of N entities.
dynamic geometry: unchanged draw loop.

### acceptance criteria

- entities with `StaticMesh` are drawn via bundle replay (zero per-entity draw calls on the CPU)
- entities without `StaticMesh` continue to use the normal per-frame draw loop
- adding or removing a `StaticMesh` entity causes the bundle to be re-recorded next frame
- the bundle is re-recorded when MSAA sample count or HDR format changes

---

## item G (renderer side) — compressed texture upload

### problem

lightmap textures and any pre-compressed asset textures are uploaded as `Rgba8Unorm`
even when BC3 compressed data is available. the GPU accepts compressed formats
directly via `queue.write_texture`; the renderer just needs to detect the format and
use it.

### fix

add a `compression` field to `lunar_assets::Texture`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextureCompression { #[default] None, Bc3 }

pub struct Texture {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    pub mips: Vec<Vec<u8>>,
    pub compression: TextureCompression, // new
}
```

in `lunar-render-3d`, when uploading a lightmap texture, check `tex.compression`:
- `None` → `Rgba8UnormSrgb` with `bytes_per_row = width * 4` (current behaviour)
- `Bc3` → `Bc3RgbaUnorm` with `bytes_per_row = ceil(width/4) * 16`

the offline compression step (producing BC3-compressed pixels) is a separate tool;
this sprint only adds the plumbing that would use it.

### acceptance criteria

- `Texture { compression: TextureCompression::None }` behaves identically to today
- `Texture { compression: TextureCompression::Bc3 }` uploads to a BC3 wgpu texture
- the `TextureCompression` field defaults to `None` so all existing code is unchanged

---

## item M — late input polling

### problem

`process_events` runs at the START of each frame before the ECS tick. the input it
captures is already stale by the time the renderer reads the camera transform
(Input → Physics → Update → Render stages later). total input-to-display latency
is ~2 frames.

### fix

move `process_events` to run AFTER `run_stages` and AFTER `apply_frame_cap`.
after vsync, the CPU is unblocked; polling input at that moment captures the
most recent device state before the next frame's simulation starts.

```
// current order:
process_events → run_stages → apply_frame_cap

// new order:
run_stages → apply_frame_cap → process_events
```

the change is in `run_with_events` in `lunar-core/src/app.rs`. the docstring
must be updated to reflect the new timing guarantee.

because `process_events` now runs AFTER the tick, the first frame has no input
(the callback hasn't run yet). this is acceptable: the first frame is typically
an empty world before any startup systems produce visible output.

also update `bootstrap_3d.rs`: window management (resize, fullscreen, cursor lock)
is in the same callback — moving it late is also correct since SDL window state
changes apply next frame anyway.

### acceptance criteria

- input captured at end of frame N is consumed by frame N+1's Input stage
- no input events are dropped between frames
- first frame runs with empty input state (same as before — no regressions)
- `run_with_events` docstring updated to describe the new order

---

## deferred

**L — CPU-GPU frame pipelining**: requires double-buffering all per-frame GPU
buffers (globals, material, entity) with a 2-slot ring, plus restructuring the
main loop so frame N+1 game logic runs during frame N's GPU render. the gain is
only real on CPU-bound scenes; GPU-bound scenes see no improvement. not worth the
risk-to-gain ratio until profiling shows the CPU idle gap is actually the bottleneck.

**J — GPU-driven indirect**: requires `Features::MULTI_DRAW_INDIRECT` for any
speedup without the full GPU-cull-writes-commands path. the multi-draw path is
Vulkan/DX12 only and requires significant compute shader restructuring. deferred
until the GPU cull pipeline is profiled and the per-draw overhead is confirmed as
a bottleneck.

**G (offline compression)**: a BC3 compressor (texpresso, squish, or custom) needs
to run at asset import time, not at runtime. this belongs in a `lunar-asset-compress`
build tool or a build.rs hook. deferred as a separate tooling sprint.
