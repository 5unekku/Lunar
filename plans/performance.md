# performance plan

reference: `plans/optimize.md` — the full GPU-driven architecture research doc.
this file tracks what's done, what's next, and the reasoning behind decisions.

## done

- fixed timestep accumulator (fiedler pattern) in `game_loop.rs`
- vsync fast path: `frame_cap == 0` returns 1 tick immediately, no accumulator jitter
- `desired_maximum_frame_latency: 2` (triple-buffered presentation) in both render crates
- `wgpu::Limits::default()` — no elevated limits requested, portable floor
- gribb-hartmann frustum extraction and AABB test in `visibility.rs`
- scratch resources (`TransformScratch3d`, `VisibilityScratch`, `CullSoa`) — clear+reuse each frame, no per-frame alloc in steady state
- `Vec3A` for `Aabb3d::center` and `half_extents` — 16-byte aligned, SIMD register fit on SSE2 and NEON
- `CullSoa` resource: parallel world-space AABB arrays built each frame after visibility propagation
- frustum culling wired into `render_3d_system`: entities with `Aabb3d` that fail `Frustum::intersects_aabb` are skipped; entities without `Aabb3d` always pass through
- per-frame `Vec` allocations eliminated from `render_3d_system`: `raw_scratch`, `draw_scratch`, `frustum_visible` are fields on `RenderEngine3d`, cleared each frame
- frustum test vectorised: `(half_extents * normal.abs()).dot(Vec3A::ONE)` replaces three scalar muls+adds
- transform and visibility propagation moved to Render stage so game logic in Update sees this frame's positions immediately
- per-frame event processing before ECS tick so fps_controller reads freshest input
- `PipelineCache` wired into the 2D renderer
- dynamic entity UBO in `lunar-render-3d`: single buffer, `has_dynamic_offset: true`, all entity transforms packed and uploaded in one `queue.write_buffer` call per frame — eliminates per-entity GPU buffers and the `entity_draws` HashMap leak
- `RenderTier` enum queried from wgpu `DownlevelFlags` at adapter creation; inserted as a Resource so render features can gate on capability
- `text_quads` in 2D renderer: replaced per-frame `Vec` construction with `layout_text_into` / `layout_text_wrapped_into` variants + entry API, inner vecs reused across frames
- improved buffer/texture labels in 3D renderer: `[subsystem] descriptor` format throughout

## principles

- sacrifice memory before speed, but intelligently: alignment padding (`Vec3` → `Vec3A`) is always worth it, bulk data duplication is not
- no allocations in the render hot path; use pre-allocated scratch resources that clear each frame
- profile before optimising; the scratch resource pattern handles the known bottlenecks

## next

### short term

**StagingBelt for per-frame uniform uploads**
`queue.write_buffer` routes through wgpu's implicit staging internally, but an explicit `StagingBelt` with a tuned chunk size (4 MiB is a reasonable starting point per the reference doc) avoids repeated small allocations and gives predictable upload timing.

### medium term

**render tiers / capability ladder**
query `DownlevelFlags` at adapter creation and select a tier:
- `LowGles`: no compute, forward only — Pi 4 / GLES floor
- `Mid`: clustered forward+, compute culling (later)
- `High`: deferred, HZB, full post stack (later)

this gates future features cleanly and keeps the Pi 4 path working.

**proper PBR lighting**
current renderer is unlit (base_color only). next step is directional + point lights using the Cook-Torrance BRDF. one shadow cascade at 1024² for the directional light. reference doc has the full shadow filter progression (3×3 PCF low → 5×5 OptimizedPCF mid).

**bind group layout standardisation**
consolidate to the 4-group layout from the reference doc:
- group 0: view-global (camera, time)
- group 1: material
- group 2: per-mesh / per-instance
- group 3: pass-specific

currently both render crates have ad-hoc layouts. making these explicit singletons allows pipelines to share bind groups and reduces redundant state changes.

### far term (post-v1)

- render graph DAG (extract → prepare → queue → render → cleanup, modelled on Bevy's design)
- GPU-driven culling via compute + indirect drawing (Vulkan/DX12 only, `DownlevelFlags::INDIRECT_EXECUTION`)
- HZB two-pass occlusion culling
- TAA + FSR-style temporal upscaler
- post stack: bloom (progressive downsample pyramid), tonemap (ACES), color grading LUT, CA/vignette fused pass
- meshlet/virtualized geometry (v2, blocked on wgpu mesh shader support)

## rules carried forward from the reference doc

these are hard rules — violation has known downstream costs:

1. no CPU wait on `Device::poll(WaitForSubmissionIndex)` from the render thread steady-state path
2. no allocations in the render hot path
3. no GPU readback in steady state (fatal on ARM — full pipeline stall)
4. no shader compilation mid-frame (compile in queue/prepare stage)
5. every `wgpu::Buffer` and `wgpu::Texture` must have a non-empty label
6. all GPU-bound structs: `#[repr(C)]` + `bytemuck::Pod + Zeroable`, no `Vec3` (use `Vec4` or `[f32; 4]` — std140 expands Vec3 silently)
7. all matrices: column-major, reverse-z convention (near=1, far=0)
8. `wgpu::Limits::default()` is the floor — never rely on elevated limits without a feature gate
