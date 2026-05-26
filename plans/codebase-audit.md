# Codebase Audit — Performance, API Usability, GPU Optimizations

> Date: 2026-04-28
> Scope: All crates in the Lunar engine workspace

---

## 1. Performance Findings

### 1.1 CRITICAL: Vertex Buffer Created Every Frame

**Location:** [`crates/lunar-render/src/lib.rs:793-828`](crates/lunar-render/src/lib.rs:793)

**Problem:** Every frame, for every texture batch, a new vertex buffer is created via `create_buffer_init()`. This allocates GPU memory every frame and throws it away.

**Impact:** Massive GPU allocation churn. At 60fps with 5 texture batches = 300 GPU buffer allocations/second.

**Fix:** Use a **persistent ring buffer** (double-buffered or triple-buffered). Pre-allocate a large vertex buffer at startup, write into it each frame with `write_buffer()`, and cycle between frames. wgpu's `MAP_WRITE` + `UNMAP` pattern is ideal here.

```rust
// Instead of create_buffer_init every frame:
// Pre-allocate at init:
vertex_buffer: device.create_buffer(&BufferDescriptor {
    size: MAX_VERTICES * 32, // 32 bytes per vertex
    usage: BufferUsages::VERTEX | BufferUsages::MAP_WRITE | BufferUsages::COPY_SRC,
    mapped_at_creation: false,
}),

// Each frame: map, write, unmap, draw
```

### 1.2 CRITICAL: Bind Group Created Every Frame Per Texture

**Location:** [`crates/lunar-render/src/lib.rs:751-768`](crates/lunar-render/src/lib.rs:751)

**Problem:** `device.create_bind_group()` is called every frame for every unique texture. Bind group creation is expensive on some backends (especially Vulkan).

**Fix:** **Cache bind groups** by texture ID. Create them once when a texture is uploaded, store in a HashMap, reuse every frame.

```rust
// In RenderEngine:
bind_groups: HashMap<u32, wgpu::BindGroup>,

// On texture upload:
let bind_group = self.device.create_bind_group(...);
self.bind_groups.insert(tex_id, bind_group);

// In render loop:
let bind_group = self.bind_groups.get(tex_id).unwrap();
pass.set_bind_group(0, bind_group, &[]);
```

### 1.3 HIGH: HashMap Grouping Every Frame

**Location:** [`crates/lunar-render/src/lib.rs:692-712`](crates/lunar-render/src/lib.rs:692)

**Problem:** Commands are grouped into `HashMap<u32, Vec<&DrawCommand>>` every frame. HashMap has allocation overhead and non-deterministic iteration order.

**Fix:** Use a **sorted Vec** approach instead. After sorting by layer, also sort by texture ID within the same layer. This eliminates the HashMap entirely and makes iteration cache-friendly.

```rust
// Sort by (layer, texture_id) tuple
sorted_commands.sort_by_key(|cmd| {
    let layer = match &cmd.kind { ... };
    let tex = match &cmd.kind { DrawKind::Sprite { texture, .. } => texture.unwrap_or(u64::MAX), _ => u64::MAX };
    (layer, tex)
});
// Then iterate linearly — same-texture commands are contiguous
```

### 1.4 HIGH: Glyph Atlas Rasterization Uses Naive Pixel Copy

**Location:** [`crates/lunar-render/src/text.rs:138-151`](crates/lunar-render/src/text.rs:138)

**Problem:** Glyph blitting uses nested `for` loops with per-pixel indexing. No SIMD, no batched copy.

**Fix:** Use `slice::copy_from_slice` for each row, or better yet, use the lunar-image SIMD functions for the copy. For a 16px-high glyph, this goes from 256 individual byte writes to 16 row copies.

### 1.5 MEDIUM: Text Layout Recomputes Every Frame

**Location:** [`crates/lunar-render/src/text.rs:195-261`](crates/lunar-render/src/text.rs:195)

**Problem:** `layout_text()` is called every frame for every text element. It iterates over characters, looks up glyphs, and computes quads each time.

**Fix:** **Cache text layout results.** Store the computed quads and only recompute when text content, font, or font size changes.

### 1.6 MEDIUM: RenderQueue Uses Vec<DrawCommand> Without Pre-allocation

**Location:** [`crates/lunar-render/src/lib.rs:1008-1013`](crates/lunar-render/src/lib.rs:1008)

**Problem:** `RenderQueue::commands` starts empty and grows dynamically every frame. Causes reallocation mid-frame.

**Fix:** Pre-allocate with `Vec::with_capacity(1024)` or similar. Use `clear()` which retains capacity.

### 1.7 LOW: Game Loop Uses `std::thread::sleep` for Frame Cap

**Location:** [`crates/lunar-core/src/game_loop.rs:149`](crates/lunar-core/src/game_loop.rs:149)

**Problem:** `thread::sleep` has OS-dependent precision (typically 1-15ms). Can cause frame pacing jitter.

**Fix:** Use **spin-wait for the last ~1ms** before the target frame time. Hybrid approach: sleep for most of the wait, then busy-wait for the remainder.

```rust
let remaining = frame_duration - elapsed;
if remaining > Duration::from_millis(1) {
    std::thread::sleep(remaining - Duration::from_millis(1));
}
// spin-wait the last millisecond
while self.last_frame.elapsed() < frame_duration {
    std::hint::spin_loop();
}
```

---

## 2. API Usability Findings

### 2.1 Stage Ordering Not Implemented

**Location:** [`crates/lunar-core/src/app.rs:174-185`](crates/lunar-core/src/app.rs:174)

**Problem:** `add_system_to_stage()` logs a warning and adds to the default schedule regardless of stage. The `UpdateStage` enum exists but does nothing.

**Impact:** Game code cannot control system ordering. Physics, input, and render systems all run in registration order.

**Fix:** Use bevy_ecs's `ScheduleLabel` and `apply_deferred` to create actual stage-based scheduling. This is a significant but necessary change.

### 2.2 Startup Systems Run Immediately, Not Before Main Loop

**Location:** [`crates/lunar-core/src/app.rs:188-192`](crates/lunar-core/src/app.rs:188)

**Problem:** `add_startup_system()` calls `run_system_once()` immediately during app construction, not before the main loop.

**Impact:** Startup systems run before all plugins are built, before resources are inserted.

**Fix:** Track startup systems in a Vec, run them all in sequence at the start of `App::run()`.

### 2.3 DrawKind::Sprite Has `texture: Option<u64>` Instead of `Handle<Texture>`

**Location:** [`crates/lunar-render/src/lib.rs:1000`](crates/lunar-render/src/lib.rs:1000)

**Problem:** The render queue takes raw `u64` texture IDs, not typed `Handle<Texture>`. Game code must call `.id() as u64` manually.

**Fix:** Change to `Option<u64>` is fine internally, but the public API should accept `&Handle<Texture>` and convert internally. The `draw_sprite` methods already do this — but `DrawKind::Sprite` directly exposes the raw ID.

### 2.4 No `DrawKind::Line` — Lines Drawn as Fat Rects

**Location:** [`crates/lunar-render/src/lib.rs:1130-1168`](crates/lunar-render/src/lib.rs:1130)

**Problem:** `draw_line()` computes an AABB and draws a rect. This is inefficient and doesn't produce clean diagonal lines.

**Fix:** Add a proper `DrawKind::Line { start, end, color, thickness }` variant. In the vertex shader, use line primitives or compute proper rotated rect vertices.

### 2.5 Rect Missing Utility Methods

**Location:** [`crates/lunar-math/src/types.rs:153-199`](crates/lunar-math/src/types.rs:153)

**Problem:** Rect has `contains()` and `intersects()` but is missing `inflate()`, `clamp()`, `union()`, `collide_point()`, `collide_rect()`.

**Fix:** Add the missing utility methods (low effort, high value).

### 2.6 Input System Uses Fixed-Size Arrays (Good) But KeyCode Only Has 64 Variants

**Location:** [`crates/lunar-input/src/lib.rs:36`](crates/lunar-input/src/lib.rs:36)

**Problem:** `KEY_COUNT = 64` limits the number of key codes. SDL3 has hundreds of keys.

**Impact:** Many keys (especially international keys, media keys, etc.) cannot be tracked.

**Fix:** Either increase `KEY_COUNT` or switch to a HashMap for less common keys (hybrid approach: array for common keys, HashMap fallback for rare ones).

### 2.7 No `DrawKind::Sprite` Origin Parameter Actually Used

**Location:** [`crates/lunar-render/src/lib.rs:1111-1120`](crates/lunar-render/src/lib.rs:1111)

**Problem:** `draw_sprite_transformed` takes an `origin` parameter in `SpriteParams` but the render loop ignores it. Rotation and scaling are always around the sprite's center.

**Fix:** Either use the origin in vertex generation, or remove it from the API to avoid confusion.

---

## 3. GPU / Backend Optimizations

### 3.1 Vulkan-Specific: Pipeline Cache

**Current:** wgpu creates pipelines from scratch each time.

**Optimization:** wgpu supports `PipelineCache` for Vulkan. On native targets, serialize the pipeline cache to disk and reload on next launch. This reduces startup time significantly on Vulkan.

**Implementation:** wgpu's `Instance::from_descriptor` accepts `pipeline_cache` option.

### 3.2 Vulkan-Specific: Descriptor Sets

**Current:** Bind groups are created per-frame (see 1.2).

**Optimization:** On Vulkan, use **descriptor pools** with pre-allocated sets. wgpu's bind group system already maps to Vulkan descriptor sets, but caching them (see 1.2) is the key optimization.

### 3.3 DirectX 12: Supported

**Current status (wgpu 29.0.1):** wgpu's first-class backends are **DX12, Vulkan, Metal, and WebGPU**. OpenGL/GLES is supported on a best-effort basis as a fallback.

**What this means for us:**
- On Windows, wgpu will prefer **DX12** by default, giving native Windows GPU performance.
- Our code is already backend-agnostic — wgpu abstracts all the differences. No engine changes needed.
- The DX12 backend uses the Agility SDK, which means we may need to ship `d3d12core.dll` alongside the game on Windows for the latest features.

**Optimizations specific to DX12:**
- DX12 benefits from **descriptor heap pre-allocation** — wgpu's bind group caching (see 1.2) already maps well to this.
- DX12 has explicit resource state management — wgpu handles this internally.
- No additional work needed from us; the P0 bind group caching fix benefits DX12 equally.

### 3.4 Metal-Specific: Command Buffer Reuse

**Current:** New command encoder created every frame.

**Optimization:** On Metal, command buffer creation has overhead. wgpu already handles this internally, but ensuring we submit in one batch (which we do) is the right approach.

### 3.5 All Backends: Texture Format Selection

**Current:** All textures use `Rgba8UnormSrgb`.

**Optimization:** For UI elements and solid-color textures, `Rgba8Unorm` (non-sRGB) is faster because the GPU skips sRGB conversion. For the atlas texture, sRGB is correct. Consider offering a `TextureFormat` hint on load.

### 3.6 All Backends: Sampler Optimization

**Current:** One global sampler is used for all textures.

**Optimization:** This is already optimal. One sampler shared across all textures is the correct approach.

### 3.7 All Backends: Vertex Format

**Current:** 8 floats per vertex (pos.x, pos.y, u, v, r, g, b, a) = 32 bytes.

**Optimization:** Pack color into a single `u32` (4 bytes instead of 16). This reduces vertex size from 32 to 20 bytes — 37.5% less vertex data.

```rust
// Current: [f32; 8] = 32 bytes
// Optimized: [f32; 4] + [u32; 1] = 20 bytes
// Vertex: pos.x, pos.y, u, v, packed_rgba
```

This requires a shader change and a vertex attribute format change (`Uint32` instead of `Float32x4` for color).

### 3.8 All Backends: Render Pass Load/Store Ops

**Current:** `load: Clear(...)` and `store: Store` every frame.

**Optimization:** If the previous frame's content doesn't matter (which it doesn't for a game), use `store: DontCare` instead of `Store`. This tells the GPU (especially tile-based GPUs like Apple Silicon, mobile) that it doesn't need to write the framebuffer back to memory.

```rust
ops: wgpu::Operations {
    load: wgpu::LoadOp::Clear(...),
    store: wgpu::StoreOp::Discard, // was Store
},
```

This is a **significant optimization for mobile/Apple Silicon** GPUs.

---

## 4. Architecture Observations

### 4.1 Good: Clean Crate Separation
- `lunar-math`, `lunar-image`, `lunar-assets`, `lunar-core`, `lunar-render`, `lunar-input`, `engine-audio` are well-separated
- Dependencies flow in one direction (no circular deps)

### 4.2 Good: ECS-First Design
- bevy_ecs integration is clean
- Handle system prevents direct reference to engine resources

### 4.3 Concern: `image` Crate Dependency in lunar-assets

The `image` crate is heavy (pulls in many format decoders). For a lean engine, consider:
- Using only the specific format sub-crates (`png`, `jpeg-decoder`)
- Or relying solely on `.mi` format for shipped games

### 4.4 Concern: `rodio` Dependency in lunar-assets

`rodio` pulls in ALSA, CoreAudio, etc. This is fine for native but adds significant compile time and binary size. The audio stub is already noted as deferred until Moonwalker integration.

### 4.5 Good: WASM Support Throughout
- Conditional compilation gates are consistent
- Web-compatible async paths exist
- Canvas surface creation for WebGPU

---

## 5. Recommended Action Items (Prioritized)

| Priority | Item | Effort | Impact |
|----------|------|--------|--------|
| P0 | Cache bind groups by texture ID | Low | High |
| P0 | Use persistent vertex buffer (ring buffer) | Medium | High |
| P0 | Use `StoreOp::Discard` instead of `Store` | Low | High (mobile) |
| P1 | Pack color into u32 in vertex format | Medium |
| P1 | Sort commands by (layer, texture) instead of HashMap | Low | Medium |
| P1 | Implement stage-based system ordering | High | High |
| P1 | Fix startup system timing | Low | High |
| P2 | Cache text layout results | Low | Medium |
| P2 | Pre-allocate RenderQueue commands Vec | Low | Low |
| P2 | Add Rect utility methods | Low | Low |
| P2 | Fix SpriteParams origin usage | Low | Low |
| P3 | Hybrid input array (common keys + HashMap fallback) | Medium | Low |
| P3 | Add DrawKind::Line variant | Medium | Low |
| P3 | Glyph atlas row-copy optimization | Low | Low |
| P3 | Pipeline cache serialization (Vulkan) | Medium | Low |

---

## 6. Summary

The engine is in good shape architecturally. The biggest performance wins are:

1. **Bind group caching** — eliminates per-frame GPU object creation
2. **Persistent vertex buffer** — eliminates per-frame GPU memory allocation
3. **StoreOp::Discard** — free performance on tile-based GPUs
4. **Vertex format packing** — 37.5% less vertex data

These four changes together could easily halve the render overhead. The API usability issues (stage ordering, startup systems) are more impactful for game developers but require more significant refactoring.
