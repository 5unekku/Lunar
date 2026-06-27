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
footprint sections below.

### dependency surface (cargo bloat + cargo tree)

`.text` section is 8.8 MiB (unstripped file 15.4 MiB, stripped ship 11.6 MiB).
top crate attribution (`CARGO_PROFILE_RELEASE_STRIP=false cargo bloat --release
--example platform_demo --crates`):

| crate | .text size | nature |
|-------|-----------|--------|
| [Unknown] (generics/monomorphization) | 1.9 MiB | shrinks with opt-level z (see Part C) |
| naga | 1.5 MiB | wgpu's runtime WGSL translator, mandatory unless shaders are precompiled |
| std | 1.1 MiB | mandatory |
| sdl3_sys | 900 KiB | windowing/input, mandatory |
| wgpu_core + wgpu_hal + wgpu + wgpu_types | ~1.2 MiB | gpu abstraction, mandatory |
| bevy_ecs | 446 KiB | ecs, mandatory |
| lunar_render_3d | 350 KiB | engine |
| regex_automata + regex_syntax + aho_corasick | ~580 KiB | from env_logger, GATEABLE (see below) |

removable / gateable findings:
- **regex ~580 KiB comes from `env_logger` -> `env_filter` -> `regex`** (`cargo
  tree -i regex-automata`). env_logger pulls regex for log-filter matching. a
  small game does not need regex-based log filtering. disabling env_logger's
  `regex` feature (or gating logging behind the size-min preset) drops the three
  regex crates. this is a concrete low-risk size win, applied in Part C.
- **naga 1.5 MiB comes from wgpu** (mandatory at runtime to translate WGSL).
  VERDICT: not worth pursuing. 1.5 MiB of ~11.6 MiB, in exchange for an entire
  per-backend offline shader pipeline that does not help wasm and gives up the
  ergonomics of runtime WGSL, is a bad trade. keep runtime WGSL. the path is
  documented below only for completeness, NOT recommended.
  path (for the record, not endorsed): precompile shaders offline,
  one blob per backend, and feed them through wgpu's passthrough path
  (`create_shader_module_passthrough`, gated by features like
  `SPIRV_SHADER_PASSTHROUGH`), then build wgpu without its WGSL front-end so
  naga's parser is not linked. concrete per-backend ingestion (correcting the
  common "vulkan/dx natively eat HLSL" shorthand): vulkan eats SPIR-V (not HLSL
  directly; HLSL or WGSL -> SPIR-V offline via DXC or naga-cli), dx12 eats DXIL
  (HLSL -> DXIL via DXC), metal eats MSL/metallib (SPIR-V -> MSL via
  SPIRV-Cross, the "targeted offline step"), GLES eats GLSL. authoring in one
  language (HLSL or WGSL) and cross-compiling to all targets at build time is a
  coherent hub. CAVEATS to verify before committing: (1) naga is a hard dep of
  wgpu-core, so dropping the WGSL front-end shrinks but may not fully remove
  naga's 1.5 MiB, measure the real saving; (2) the GLES LowGles fallback tier
  needs GLSL variants; (3) wasm/WebGPU ingests WGSL (the browser compiles it),
  so this saves nothing on the wasm bundle and only helps native; (4) shipping
  per-backend blobs adds a little asset weight, far less than 1.5 MiB of code.

duplicate dep versions (minor transitive bloat, a few KiB each, mostly
unavoidable until upstreams converge): bitflags 1+2, foldhash 0.1+0.2,
hashbrown 0.15+0.16+0.17, png 0.17+0.18, rustc-hash 1+2, ttf-parser 0.20+0.21,
winnow 0.7+1.0.

### wasm bundle size (lunar-web, the web entry)

measured the `lunar-web` bin (`src/web.rs`), the actual web entry point, built
`--no-default-features` (no coreclr; .NET cannot run on bare wasm anyway).
NOTE: the `--example` path (and `scripts/run_wasm.go`) cannot build for wasm
because examples compile lunar-game's dev-dependency `lunar-plugin-loader`
(libloading), which does not exist on wasm. the bin is the correct wasm
yardstick. (`run_wasm.go` building examples is a latent bug, see findings.)

| stage | bytes | human |
|-------|-------|-------|
| raw `lunar-web.wasm` | 4,469,119 | 4.26 MiB |
| after wasm-bindgen | 3,263,382 | 3.11 MiB |
| after `wasm-opt -O3` | 3,042,542 | 2.90 MiB |
| gzip -9 (served) | 1,076,035 | 1.03 MiB |
| **brotli -q11 (served)** | **782,134** | **764 KiB** |

verdict: the served bundle is ~764 KiB (brotli) / ~1.03 MiB (gzip), comfortably
inside the single-digit-MB web target. the compressed number is what a user
downloads, and it is the number that matters; the pipeline currently never
reports it (run_wasm.go runs wasm-opt but never gzips), so a dev would
mistakenly think the bundle is 2.9 MiB. RECOMMENDATION: have run_wasm.go report
the gzip/brotli size, and serve the wasm pre-compressed. note wasm-opt needs
`--enable-nontrapping-float-to-int` (plus simd/bulk-memory) or it rejects the
rustc output; run_wasm.go's current `-O3 --enable-simd --enable-bulk-memory`
would fail on this binary, another latent run_wasm.go bug.

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

all numbers are platform_demo, release profile, x86_64-unknown-linux-gnu,
rustc 1.98.0-nightly, stripped binary size in bytes.

| change | size | delta |
|--------|------|-------|
| baseline (release) | 12,211,592 | |
| + drop env_logger `regex` feature | 11,328,360 | -883,232 (-7.2%) |
| + release-min profile (opt-level z) on top | 7,465,704 | -3,862,656 (-34% vs release, -39% vs baseline) |

### 1. drop env_logger regex feature (universal, applied)

`env_logger`'s default `regex` feature pulled `regex` + `regex-automata` +
`regex-syntax` + `aho-corasick` (~580 KiB .text, ~862 KiB after LTO removes the
transitive tail) purely for regex-pattern log filtering. set
`env_logger = { default-features = false, features = ["auto-color",
"humantime"] }` in the workspace deps. RUST_LOG module+level filtering still
works, color + timestamps kept. verified: `regex-automata` and `aho-corasick`
are gone from the dep tree, the binary dropped 862 KiB. this is a universal win
(every game benefits), not behind a flag, because regex log-filtering is dev
infrastructure, not game behavior.

### 2. release-min size profile (opt-in, applied)

added `[profile.release-min]` (`inherits = "release"`, `opt-level = "z"`). a
size-first build is now one flag away: `cargo build --profile release-min`.
opt-level z took the binary from 11.3 MiB to 7.1 MiB (-34%). this is opt-in by
design: opt-level z can cost runtime speed, so the default `release` stays
opt-level 3 (max speed) and the dev chooses release-min when size matters more.
the runtime cost was not separately benched (the one workspace bench builds its
own profile); flagged as the dev's explicit size-over-speed tradeoff.

### 3. mul_add no-FMA fix (applied)

`Rect::inflate` (`lunar-math/src/types.rs:324`) used `dx.mul_add(2.0, self.w)`,
violating the project no-FMA policy (baseline x86-64 lacks fma, so mul_add
lowers to a slow libm call). changed to `self.w += dx * 2.0`. correctness is
identical for this use; removes the policy violation.

### note: no separate size-min feature needed (YAGNI)

the plan considered a `size-min` feature bundling gates. investigation showed
it is unnecessary: the existing `2d` / `3d` / `audio` features already ARE the
size presets. default is `2d`, and the heavy 3d subsystem (lunar-render-3d, its
~28 embedded WGSL shaders, lunar-bsp, lunar-lightmap) only compiles in under
the `3d` feature. so a 2d or headless game already drops all of that with no new
flag. the only universal win not already gated was env_logger's regex, applied
above. adding an empty preset would be ceremony with nothing to bundle.

### verification notes (gate is green)

final state: `cargo clippy --workspace --all-targets -- -D warnings` passes,
and `cargo test --workspace --no-fail-fast` is all green (51 test-result lines,
0 failures). the audit's size/perf changes (env_logger, release-min, mul_add)
are verified: release + release-min build and link, and introduce no failure.

getting there required fixing SEVEN pre-existing issues unrelated to the audit
(all reproduced on the base state with the audit changes stashed; all surfaced
because the cranelift crash had been aborting the whole test process early and
masking everything downstream). these were toolchain-drift and platform issues
on the unpinned local nightly (2026-06-08), now fixed:
1. `lunar-3d` lib tests SIGABRT under dev: cranelift cannot codegen the avx2
   intrinsic `llvm.x86.avx.cmp.ps.256` (issue 171) used by the SoA cull. FIX:
   `[profile.dev.package.lunar-3d] codegen-backend = "llvm"` (targeted; the rest
   of the workspace keeps cranelift). note: pinning a nightly would NOT fix this,
   the intrinsic is unimplemented in cranelift, not regressed.
2. `transform3d_layout` tests asserted offset 12 but `Quat`'s 16-byte alignment
   pads the 12-byte `Vec3` up to 16. FIX: assert
   `size_of::<Vec3>().next_multiple_of(align_of::<Quat>())` (robust on SIMD and
   scalar builds). Vec3A/16 layout is intended.
3. `lunar-dotnet-host` clippy: a lowercase `# safety` heading (clippy needs
   `# Safety`) + 2 collapsible-if lints. FIXED.
4. `bindings/c/src/lib.rs`: 35 `pub unsafe extern "C"` fns missing `# Safety`
   docs + 1 redundant import. FIXED (accurate per-fn safety contracts added).
5. `lunar-render-3d/src/config.rs:1187`: `manual_div_ceil` lint. FIXED
   (`.div_ceil(align) * align`).
6. `cross_compile_web` test: it cross-checks the whole workspace on wasm, but
   `lunar-dotnet-host` (hostfxr) and `lunar-plugin-loader` (libloading) cannot
   compile on bare wasm. FIX: exclude both native-only crates from the wasm leg.
7. `lunar-dotnet-host` module doctest called the renamed `get_fn` and mistyped
   args. FIXED to match the real `get_fn_ptr(&Path, ...) -> *const c_void` api.

## stretch-goal recommendations
```
