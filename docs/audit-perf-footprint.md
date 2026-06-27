# Lunar perf + footprint audit

## environment

- rustc: 1.98.0-nightly (cb46fbb8c 2026-06-08)
- host triple: x86_64-unknown-linux-gnu
- date: 2026-06-27

## A. performance gaps

coverage: render-3d + render (deep read), 3d + core + 2d (deep read),
math/image/atlas/bsp/lightmap (skim). headline: the per-frame render and tick
paths are already heavily optimized; the one real "runs on a potato" gap is
cpu thread oversubscription on low core counts, and the one real algorithmic
gap is offline lightmap baking. accessibility: native has a genuine GLES
fallback (good), wasm is WebGPU-only with no WebGL2 fallback (a gap).

### ranked summary

| # | gap | impact | effort | file:line |
|---|-----|--------|--------|-----------|
| 1 | two thread pools oversubscribe on low core counts, no single-threaded fallback | high (potato cpu) | small | `lunar-core/src/engine.rs:60` |
| 2 | lightmap visibility rays brute-force O(texels·samples·tris), BVH already exists | high (bake time) | medium | `lunar-lightmap/src/baker.rs:444` |
| 3 | rayon par-iters with no length threshold (small-scene/low-core overhead) | medium | small | `lunar-3d/src/systems.rs:146`, `visibility.rs:465` |
| 4 | per-frame QueryState reconstruction in `&mut World` transform system | medium | medium | `lunar-3d/src/systems.rs:65-266` |
| 5 | `find_surface` scans all triangles per texel O(texels·tris) | medium | small | `lunar-lightmap/src/baker.rs:300` |
| 6 | late gpu cull uses its own encoder + extra `queue.submit`, foldable | medium (needs profiling) | small | `lunar-render-3d/src/frame.rs:1698` |
| 7 | per-frame Vec allocations in feature passes (water/decals/detail/terrain/point-shadow) | low | small | `lunar-render-3d/src/passes.rs:527,674,775,991,1277` |
| 8 | lightmap atlas change-detection allocs+sorts a Vec every frame | low | small | `lunar-render-3d/src/frame.rs:737` |
| 9 | `Rect::inflate` uses `mul_add`, violates the project no-FMA policy | low (policy bug) | trivial | `lunar-math/src/types.rs:324` |
| 10 | misc lightmap (edge recompute, index Vec), late-cull sparse get, x86 deinterleave rayon | low | small-medium | `baker.rs:458,273`, `frame.rs:1599`, `image/simd.rs:68` |

### already optimal, do not touch

frame pacing (exactly one present per vsync, no double-render regression,
verified `app.rs:397/459`, `frame.rs:1892`); gpu cull readback is pipelined
and non-blocking (`cull.rs:60-94`); MDI/indirect draws, passthrough shaders,
SIMD SoA frustum cull (`cull_aabbs_soa`), FSR3 (EASU+RCAS), FxHash containers,
pervasive scratch-buffer reuse, StagingBelt uploads, dirty-flag bind-group
caching; sweep-and-prune broadphase (2d + 3d), iterator queries, animation
track interning, batched column write-back; lunar-math SIMD (Vec3A),
lunar-image NEON deinterleave + baked srgb LUT + delta filter, lunar-bsp BVH +
PVS trailing_zeros scan, lunar-atlas shelf packer. native GLES fallback path
(`RenderTier::LowGles`, the "Pi 4 floor") is real and deliberately supported.

### top finding: low-core thread oversubscription (the potato crux)

`crates/lunar-core/src/engine.rs:60` forces `ExecutorKind::MultiThreaded` on
all native targets with no core-count gate. bevy's `ComputeTaskPool` is never
explicitly sized (lazily grabs `available_parallelism()`), and `build_cull_soa`
+ the flat-scene `propagate_transforms_3d` path run rayon par-iters
(`systems.rs:146`, `visibility.rs:465`) that spin up a SECOND global pool of
the same size, nested inside scheduled systems. on a 2-core box that is ~4 OS
threads fighting over 2 cores, paying fixed per-frame parallel overhead with no
fallback. fix: gate the executor on `available_parallelism()` (use
`SingleThreaded` when cores <= 2 and run the par-iters serial there), and init
one shared pool rather than letting bevy and rayon each grab N threads. this is
the single highest-value change for the stated "runs on a potato" bar.

### lightmap baking (offline but scales badly)

`crates/lunar-lightmap/src/baker.rs:444` `ray_blocked` linearly tests every
triangle per hemisphere sample (default 64) per texel: O(texels·samples·tris).
lunar-bsp already ships an AABB BVH that answers occlusion in O(log tris) with
an any-hit early-out. `find_surface` (`baker.rs:300`) has the same
O(texels·tris) shape, fixable with a coarse UV-grid bin. offline bake time, but
blows up on real meshes.

### accessibility: GPU API floor

native: wgpu compiled with `vulkan + gles` on every desktop (`+ dx12` windows,
`+ metal` apple), no implicit backends (`Cargo.toml:44` default-features=false).
the GLES backend is a real fallback with a dedicated `RenderTier::LowGles` path
(`lunar-render-3d/src/lib.rs:430/438`), dropping the native floor to GLES3 /
GL3.3 ("Pi 4") hardware, much older than a vulkan-only ~2012+ floor. good.

wasm: hard-pinned to `BROWSER_WEBGPU` only (`bootstrap_wasm_3d.rs:39`), the
`gles`/webgl2 feature is NOT in the wasm32 target block, so there is no WebGL2
fallback. older browsers (pre-Chrome-113, Safari < 17, Firefox without the
flag) and WebGPU-less hardware are cut off. RECOMMENDATION: add a `webgl`
fallback feature + an instance-backend retry for wasm to widen web reach. this
is a portability gap, not a perf gap.

size/accessibility tension (carry into Part B): the native `gles` backend is
the single lever keeping pre-vulkan hardware alive; dropping it would shrink the
binary but is a hardware-support regression, NOT a free size win. do not strip
`gles` for size without an explicit decision.

## B. binary size / bloat

### native baseline (platform_demo)

| metric | value |
|--------|-------|
| binary | `target/release/examples/platform_demo` |
| stripped size (shipped) | 12,211,592 bytes (11.6 MiB) |
| profile | release (fat LTO, codegen-units=1, panic=abort, strip=symbols) |
| build command | `cargo build --release --example platform_demo` |
| unique normal-dep crates (lunar-game) | 225 (the 543 lock entries include build + dev deps) |

a pure-rust simple game is ~11.6 MiB, already far under the 100 MB native
target. the native size story is therefore not the binary, it is whatever a
game adds on top (the .NET runtime for C# scripting, assets); see the .NET and
footprint sections below. `cargo bloat` crate attribution is captured in the
dep-surface task (B.1).

### tooling

- cargo-bloat: 0.12.1
- cargo-tree: bundled with cargo (nightly 1.98)
- wasm-opt (binaryen): version 130
- wasm-bindgen: 0.2.123
- gzip: present
- brotli: 1.2.0
- cargo-zigbuild: installed (no --version flag)
- dotnet SDK: 10.0.301

## C. low-risk wins applied

## stretch-goal recommendations
```
