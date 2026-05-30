# sprint 7 — loop correctness, motion smoothness, culling, geometry

four items. no new visual features. closes the remaining performance gaps
vs halo 3 outdoor (LOD) and q3/hl2 indoor (PVS), and makes high-refresh
displays actually worthwhile (interpolation alpha).

---

## item A — TickRateConfig world resource

### what and why

the tick rate is currently passed to `run()`/`run_with_events()` and locked
into the `GameLoop` local. game code cannot change it. a settings menu has
no way to switch from Hz60 to Hz120 at runtime without restarting.

fix: insert a `TickRateConfig` resource at startup, have the game loop watch
it each frame and call `set_tick_rate()` if it changed.

### implementation

**`crates/lunar-core/src/app.rs`:**
```rust
#[derive(Resource, Clone, Copy)]
pub struct TickRateConfig {
    pub rate: TickRate,
}
```

insert at startup from the passed tick_rate param, before the game loop
starts. inside `run_with_events`, after `game_loop.apply_frame_cap()`:

```rust
if let Some(cfg) = self.engine.world().get_resource::<TickRateConfig>() {
    if cfg.rate != game_loop.tick_rate() {
        game_loop.set_tick_rate(cfg.rate);
        fixed_delta = cfg.rate.delta_seconds();
    }
}
```

also update `Time::advance()` on the fly — `fixed_delta` is now a local
that updates when the config changes rather than a constant captured at
loop start.

### files
- `crates/lunar-core/src/app.rs` (TickRateConfig resource, loop watch)
- `crates/lunar-core/src/lib.rs` + `prelude.rs` (re-export TickRateConfig)
- `crates/lunar-render/src/lib.rs` (insert resource in plugin init)
- `crates/lunar-render-3d/src/lib.rs` (same)

### win
- settings menu can switch tick rate at runtime without restart
- one world resource controls it, readable and writable from any system

---

## item B — render interpolation alpha

### what and why

at 60hz tick rate and 300fps render, 4 out of every 5 frames show the same
simulation state — identical positions, identical animation pose. without
interpolation this shows as microstutter in character movement even at high
frame rates. with interpolation every frame is visually distinct.

the `GameLoop` accumulator already tracks how far we are between ticks.
`alpha = accumulator / tick_interval` (0 = just ticked, 1 = about to tick).
entities lerp between their previous-tick transform and current-tick transform
by alpha at render time. every render frame produces a unique interpolated
position regardless of tick rate.

input lag tradeoff: you're always rendering up to one tick behind the
simulation (16.7ms at 60hz, 8.3ms at 120hz). imperceptible for most games.
for first-person mouse look, apply camera rotation from raw input directly
in the render stage (outside the fixed tick loop) to avoid any perceived lag.

### implementation

**`crates/lunar-core/src/game_loop.rs`:**
```rust
pub fn interpolation_alpha(&self) -> f32 {
    let interval = self.tick_rate.interval().as_secs_f32();
    (self.accumulator.as_secs_f32() / interval).clamp(0.0, 1.0)
}
```

**`crates/lunar-core/src/app.rs`** — expose via `Time`:
```rust
// field on Time
interp_alpha: f32,

// set once per render frame, before the tick loop
pub fn set_interp_alpha(&mut self, alpha: f32) { self.interp_alpha = alpha; }
pub fn interp_alpha(&self) -> f32 { self.interp_alpha }
```

in `run_with_events`, after `game_loop.tick()` returns, before the tick loop:
```rust
let alpha = game_loop.interpolation_alpha();
if let Some(mut time) = world.get_resource_mut::<Time>() {
    time.set_interp_alpha(alpha);
}
```

**`crates/lunar-3d/src/mesh.rs`** — add `PrevWorldTransform3d` component:
```rust
#[derive(Component, Clone, Copy, Default)]
pub struct PrevWorldTransform3d(pub WorldTransform3d);
```

**system in `crates/lunar-3d/src/systems.rs`** (runs at PostUpdate, end of
each tick, after all transform propagation):
```rust
fn copy_prev_transforms(
    mut q: Query<(&WorldTransform3d, &mut PrevWorldTransform3d)>
) {
    for (cur, mut prev) in &mut q {
        prev.0 = *cur;
    }
}
```

**`crates/lunar-render-3d/src/lib.rs`** — in draw_scratch gathering, lerp
transforms when both components are present:

```rust
let render_transform = if let Some(prev) = world.get::<PrevWorldTransform3d>(entity) {
    let alpha = time.interp_alpha();
    prev.0.lerp(&world_transform, alpha)
} else {
    world_transform
};
```

`WorldTransform3d` needs a `lerp()` method (position lerp, rotation slerp).

### note: first-person camera

first-person mouse look should NOT go through the tick/interpolation path.
a camera system that reads raw mouse delta each render frame (via
`time.real_delta_seconds()`) and applies it directly to view rotation stays
at render rate with zero lag. camera position (following the character's
physics position) DOES use interpolation as normal.

### files
- `crates/lunar-core/src/game_loop.rs` (interpolation_alpha)
- `crates/lunar-core/src/app.rs` (Time::interp_alpha, set in loop)
- `crates/lunar-3d/src/mesh.rs` (PrevWorldTransform3d component)
- `crates/lunar-3d/src/systems.rs` (copy_prev_transforms system)
- `crates/lunar-3d/src/lib.rs` (WorldTransform3d::lerp method, export PrevWorldTransform3d)
- `crates/lunar-render-3d/src/lib.rs` (lerp in draw_scratch gather)

### win
- 60hz logic looks smooth at any render frame rate
- eliminates microstutter on high-refresh displays
- 120hz tick is now genuinely worthwhile on a 300hz monitor (alpha stays in
  range [0, 1] across the ~2.5 render frames between ticks)

---

## item C — auto-LOD generation tool

### what and why

`MeshLod` and CPU LOD selection already exist. what's missing is the
pipeline that generates the LOD meshes. currently devs must manually create
and register 3-4 simplified versions of every mesh — friction that means
most entities ship with no LOD at all. without LOD, distant geometry renders
full-detail: a rock 300m away submits the same 10K triangles as one at 2m.

fix: `tools/gen-lods/` uses meshopt to generate 4 LOD levels per input
mesh at fixed simplification ratios, writes them as binary mesh data, and
emits a `.lod` descriptor file the game can load to build `MeshLod` automatically.

### simplification ratios and thresholds

| LOD | ratio | max_dist |
|-----|-------|----------|
| 0   | 1.0   | 15m      |
| 1   | 0.5   | 50m      |
| 2   | 0.25  | 150m     |
| 3   | 0.10  | 400m     |
| 4   | 0.05  | ∞        |

these are defaults. the tool accepts `--thresholds` to override per asset.

### implementation

**`tools/gen-lods/Cargo.toml`:**
```toml
[dependencies]
meshopt  = "0.4"   # rust bindings for meshoptimizer
bytemuck = "1"
glob     = "0.3"
```

**algorithm per mesh:**
1. load base mesh vertices + indices
2. for each LOD level: `meshopt::simplify(&indices, &vertices, target_count, error_threshold)`
3. run `meshopt::optimize_vertex_cache()` on the simplified indices (same as
   our upload path does — but baking it means load time is cheaper)
4. write each level as a flat binary: `[u32 vertex_count, u32 index_count,
   bytes...]` matching our existing `MeshData` binary layout
5. write a `.lod` descriptor: JSON or binary listing the files + thresholds

**asset server integration:**
add a `MeshLodLoader` to `lunar-assets` that reads `.lod` descriptors and
registers all levels into `MeshRegistry`, returning a `Handle<MeshLodSet>`
that the renderer can attach as `MeshLod` automatically.

or simpler first pass: the tool emits a `.lod.ron` that game startup code
reads to build `MeshLod` components — no new loader needed.

### files
- `tools/gen-lods/` (new tool — Cargo.toml, src/main.rs)
- `crates/lunar-3d/src/mesh.rs` (no changes needed — MeshLod already correct)
- `crates/lunar-render-3d/src/lib.rs` (no changes — LOD selection already correct)

### win
- outdoor triangle count matches halo 3's managed geometry budget
- devs get LOD for free on any mesh run through the tool
- no render code changes needed — MeshLod selection path already correct

---

## item D — PVS baking tool

### what and why (shorter than expected)

**the runtime infrastructure already exists.** `BspBlob` has `pvs: Vec<u64>`,
`pvs_stride`, `leaf_count`. `BspLevel::visible_leaves()` does the O(1)
bitmask lookup and falls back to all-leaves when `pvs_stride == 0`. the
renderer already calls `visible_leaves()` to build the visible area set.

what's missing: an offline tool that floods the BSP portal graph and fills
those bitmasks. once the tool runs and the blob is loaded with a non-zero
`pvs_stride`, the renderer automatically gets fast PVS culling with no
further code changes.

### algorithm

the BSP portal graph is an area adjacency graph: `PortalData` gives
`(area_a, area_b)` pairs, and the area_map gives `(leaf_index, area_id)`.
for indoor games the correct approach is area flood:

1. build area adjacency: `HashMap<u32, Vec<u32>>` from `PortalData`
2. build area→leaves: `HashMap<u32, Vec<u32>>` from area_map
3. for each leaf L:
   a. find its area A
   b. BFS through area adjacency from A, collecting all reachable areas
   c. collect all leaf indices in those reachable areas
   d. set their bits in L's PVS row
4. pack into `pvs: Vec<u64>` with `pvs_stride = ceil(leaf_count / 64)`
5. serialize the completed `BspBlob` back to disk

this gives a conservative PVS (some over-inclusion is fine — it's
"potentially visible", not "definitely visible"). an optional second pass
could test frustum intersection through portal chains for a tighter set,
but area-flood is correct and fast.

### portals vs open worlds

for outdoor levels with no portals, area_map is empty and pvs_stride stays
0. the renderer falls back to BVH frustum culling as it does today. the tool
detects this and skips baking (no portals = no indoor PVS to bake).

### files
- `tools/bake-pvs/` (new tool — Cargo.toml, src/main.rs)
  - reads a serialized `BspBlob` (bincode or similar)
  - runs the flood algorithm
  - writes the blob back with pvs filled
- `crates/lunar-bsp/src/level.rs` (no runtime changes needed)
- `crates/lunar-render-3d/src/lib.rs` (no changes — already uses visible_leaves)

### win
- indoor culling: O(1) bitmask AND per frame vs O(portal count) BFS traversal
- for a 300-room level: ~10ns per frame culling cost vs ~100µs
- closes the q3/hl2 indoor performance gap

---

## recommended order

1. **A (TickRateConfig)** — 1-2 hours, unblocks settings menu work downstream
2. **B (interpolation alpha)** — 1 day, touches core loop + renderer
3. **D (PVS baker)** — 1 day, runtime already done, just the tool
4. **C (auto-LOD tool)** — 1-2 days, standalone tool, no renderer changes

D before C because D's runtime path is already wired — shipping the tool
immediately unlocks the indoor culling win. C requires more tooling work
but also has no renderer risk.

---

## not in this sprint

**GPU-driven LOD selection** — once auto-LOD generates levels and CPU
selection proves the pipeline correct, a follow-up sprint can move selection
to a compute pass. doing it before the LOD generation tool exists would be
premature.

**TAA** — natural follow-up to interpolation alpha (TAA uses the same
sub-frame jitter concept and benefits from the prev-transform infrastructure).
a clean visual feature sprint on its own.

**ambient light probes per volume** — halo 3 visual quality gap, sprint 8+.
