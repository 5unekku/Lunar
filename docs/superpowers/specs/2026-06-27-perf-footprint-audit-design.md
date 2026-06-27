# Lunar perf + footprint audit, design

date: 2026-06-27

## goal

Two deliverables on the Lunar engine (~50k Rust LOC, 25 crates, 543 deps in the lock):

1. a performance gap audit against the bar "a simple game should run on a potato"
   (half-life 2 / portal 2 / quake / doom class hardware).
2. a binary-size / bloat audit with concrete, per-target footprint goals: a
   simple, NES-styled game (no fancy rendering) should be negligible in size,
   smaller is always better, but never at a cost to the game or the
   developer. native ceiling for simple games is 100 MB. the wasm/web ceiling
   is much tighter: 100 MB is meaningless on the web (users abandon a
   multi-MB download), so the web target is single-digit MB compressed,
   tracked separately from native, not under the same 100 MB number.

Action scope: report + low-risk wins. Apply only safe, measurable
size/perf changes (size-optimized profile, feature gates). Anything riskier
ships as a written recommendation for the user to greenlight.

## non-goals

- no blind sweep of already-optimized code (NEON/SSE2 cull, FSR3, SIMD SoA
  cull, wasm pump_frame, baked tables, etc. are already landed and verified)
- no aggressive dep removal or refactors this session
- no regressions to the high end (the engine must still reach "the prettiest
  games"); size wins must be opt-in, not forced on every build
- native cross-arch verification (32-bit, arm) is explicitly deferred this
  pass. it was in the original ask but the refined goal is perf + size; arch
  portability is noted only where a size lever is free across targets, and
  flagged as a separate follow-up rather than silently dropped. note the
  build matrix already exists (`build_all.go`: linux gnu+musl, windows gnu,
  macos; x86_64/i686/aarch64/armv7), so this is verification-of-running, not
  build enablement.

## guiding principle: runtime over compile time

Compile-time cost is of zero concern when it buys runtime speed. This is
engine ideology. The audit never rejects or down-ranks a perf or size win
because it slows the build (more LTO, more monomorphization, more codegen,
heavier const eval, build-script precomputation are all fair game). The
"slow build" note in part B is a scheduling heads-up only, never a reason to
avoid an optimization.

## part A: performance gap audit (report)

Systematic read of hot paths, per subsystem, looking for real gaps with
file:line evidence (not speculation):

- lunar-render-3d, lunar-render: per-frame heap allocations, redundant GPU
  submissions, CPU/GPU sync points, anything that would stall an iGPU or
  low-end discrete card. include frame pacing / present timing: on low-end
  hardware perceived smoothness hinges on one-present-per-vsync and steady
  pacing (this engine already hit a double-render-per-vsync stutter bug), so
  the audit checks the present/schedule path for pacing regressions, not just
  raw throughput.
- lunar-3d, lunar-core, lunar-2d: ECS query patterns, culling, schedule
  shape, allocations inside the per-tick loop. include low-core-count
  scaling: with bevy_ecs multi-threaded + rayon, a 2-core potato can lose to
  thread-pool overhead and oversubscription; check that the engine sizes its
  pool sanely and degrades gracefully toward single-threaded rather than
  assuming many cores.
- lunar-math, lunar-image, lunar-atlas, lunar-bsp, lunar-lightmap:
  algorithmic hot spots, unnecessary precision/work.
- the GPU API floor (accessibility crux for old hardware): "runs on a
  potato" is as much about the minimum graphics backend as CPU work.
  wgpu is `default-features = false`, so which backends are enabled decides
  the oldest GPU that can run at all. determine the enabled backends and
  whether a GLES/GL fallback exists for pre-Vulkan hardware (the era that
  ran HL2/Quake). this is a portability finding, and it constrains the size
  analysis: see the size/accessibility tension in part B.

Prioritization: this is time-boxed, not exhaustive. Crates are read in
descending order of hot-path weight (render-3d, 3d, render first, since the
per-frame cost lives there), and the audit stops at diminishing returns
rather than forcing a full read of all 50k LOC. The report states which
crates got a deep read vs. a skim so coverage is honest.

Already-applied perf levers to credit (not re-litigate): the dist CPU
baseline (`x86-64-v2` on x86_64, `neon+vfp4` on armv7, opt-out via
`--no-cpu-baseline`) already lets LLVM vectorize aggressively, plus the
landed NEON/SSE2 cull, FSR3, SIMD SoA cull, and baked tables. the audit
treats these as the floor and looks for what is left above them.

Output: a ranked gap list (impact x effort), each entry carrying evidence
and a proposed fix. Known-good areas are explicitly called out as "already
optimal, do not touch" so the report is honest about diminishing returns.
Where a gap's runtime impact can't be proven by static reading alone, the
audit *runs* the existing benches (e.g. tools/navmesh-bench) or headless GPU
tests that already cover that hot path to ground the impact with numbers,
since running them is cheap and the action scope allows it. only gaps with
no existing bench are labelled "needs profiling" rather than asserted.

## part B: binary-size / bloat audit (report + measurement)

1. measure the right artifact and the right yardstick. what `build_all.go`
   ships is an **example binary**, not the `lunar` engine bin, and it copies
   only the binary (no runtime/assets bundled). the right "simple game"
   yardstick is therefore a representative example: `platform_demo` (pure
   rust, the size floor) measured against `platform_demo_cs` (C#-scripted).
   caution: the C# delta does NOT show up in the Rust example binary, because
   the .NET runtime and managed assemblies are loaded at runtime, not linked
   in. measuring binary-only would wrongly conclude C# adds nothing. the
   delta lives in the runtime/managed payload shipped alongside the binary,
   which dist does not yet bundle: count that whole tail (runtime + managed
   dlls + default assets) and flag the missing bundling step as a finding.
2. baseline: build the yardstick example(s) in release, measure stripped
   size, run `cargo bloat` (crate + symbol attribution) and `cargo tree` for
   the dep graph. capture real numbers, not estimates. also compare
   gnu-dynamic vs musl-static for the same example, since musl statically
   links libc and changes the size story.
3. the .NET runtime is the prime size suspect for a C#-scripted game.
   NativeAOT is already the default `LoaderBackend` (trimmed native image);
   CoreCLR is feature-gated (`default = ["coreclr"]` for dev hot reload) and
   ships the whole runtime. measure both shipping models' total footprint via
   platform_demo_cs; this is probably *the* deciding size lever, not one
   feature gate among many. confirm NativeAOT as the release recommendation
   if the numbers bear it out.
4. wasm bundle size: for web targets the served `.wasm` is the dominant size
   metric. `run_wasm.go` also builds `--example <name>`, so measure the same
   `platform_demo` yardstick on wasm as on native for an apples-to-apples
   floor. the pipeline already runs wasm-bindgen + `wasm-opt -O3` but never
   measures compressed size, yet servers ship wasm gzip/brotli
   encoded, so the compressed number is what a user actually downloads.
   measure raw, `wasm-opt`, and gzip/brotli sizes; `-Oz`/`-Os` is an untested
   size-vs-speed lever to evaluate (same tradeoff caveat as the native size
   profile).
5. dep-surface map of the 543 crates: mandatory vs feature-gated vs
   removable; duplicate versions; the fat deps (sdl3, lunar-dotnet-host /
   CoreCLR, zstd, wgpu backends, cubeb audio). size/accessibility tension:
   wgpu backends are a tempting size cut, but the GL/GLES backend is exactly
   what lets old/potato GPUs run the game (see part A's GPU API floor). any
   backend-drop recommendation must state the hardware it gives up, and the
   default must keep the broad-compatibility backend even if a size build
   can opt out.
6. feature-gating analysis: can a "simple game" build drop .NET hosting,
   unused audio backends, unused wgpu backends, and 2d-only or 3d-only?
   what does each drop save? concrete embedded-asset target: render-3d
   `include_str!`s ~28 WGSL shaders (shadow, cluster, hzb, point_shadow,
   panorama_sky, etc.) straight into the binary, so a 2d-only or
   no-shadows/no-clustered-lighting game carries dead shader text. measure
   what gating these embeds actually saves.
7. footprint target doc: what an empty/simple Lunar game floors at today
   vs. the per-target goals (native sub-100-MB, wasm single-digit-MB
   compressed), and which levers close each gap.
   the *real, existing* levers are: feature-stripping (incl. embedded
   shaders), the size profile, NativeAOT over CoreCLR, and the asset
   compression/baking pipeline that already exists (`compress-textures`,
   `gen-lods`, `bake-pvs`, the `.li`/zstd image format). be honest that Lunar
   has compression/baking, NOT kkrieger-style runtime procedural generation
   (`gen_assets` is a placeholder that writes flat-color sprites). kkrieger
   and NES are cited as philosophy and a possible future direction
   (procedural/runtime-generated content as a size lever), not as a current
   capability or a literal 40 KB target.

practical risks (expected, not blockers):
- a `release` build with `lto = "fat"` + `codegen-units = 1` is slow
  (minutes); the baseline build is budgeted for, not treated as a surprise.
- `cargo bloat` / `wasm-opt` may not be installed; the plan installs them or
  falls back to `cargo tree` + raw `size`/`ls` attribution.
- `cargo bloat` needs symbols, but `[profile.release]` sets `strip =
  "symbols"`; attribution runs against a temporarily un-stripped build (or a
  dedicated profile) so the final shipped size and the bloat breakdown are
  measured from the right artifacts.
- a new custom profile name spins up a fresh `target/<name>/` dir and can
  retrigger the cmake/cubeb-sys profile-dir interaction the existing
  Cargo.toml comment documents; part C validates a clean build before
  trusting any size delta.

## part C: low-risk implementation (safe wins only)

- add a size-optimized release profile (e.g. `[profile.release-min]` with
  `inherits = "release"` then overriding opt-level to `z`/`s`; lto fat, panic
  abort, strip carry over from release) and measure before/after. without
  `inherits` a custom profile silently loses release's panic/strip/lto, so
  the size delta would be measured against the wrong baseline. validate a
  clean build links before trusting the delta (see cubeb-sys risk above). IMPORTANT: opt-level `z`/`s` can cost runtime speed, which
  collides with "never at a cost to the game." so this profile is an opt-in
  dev choice, not the default; the report records the measured runtime cost
  (via existing benches where available) next to the size saving so a dev
  decides with eyes open. the default release profile stays speed-optimized
  (opt-level 3). if `s`/`z` shows negligible runtime loss for real savings,
  say so; if it tanks a hot path, recommend against it.
- add or tighten feature flags for whatever part B's dep/feature analysis
  actually finds compiled-in-but-unused (candidates: .NET hosting, extra
  audio backends, wgpu backends, embedded 3d shaders for 2d-only builds).
  note wgpu is already `default-features = false`, so backend gating may
  largely exist: confirm before claiming a win. each gate measured, native
  and where relevant wasm.
- keep the developer cost near zero: bundle the size gates behind one
  preset (e.g. a single `size-min` feature or profile that flips the whole
  set) rather than 20 individual flags a dev must learn and combine. "not at
  a cost to the developer" means a small game is one switch away, not a
  research project.
- verify the gated configs, not just the default. feature gates are the
  classic source of broken non-default builds: every new config (size-min,
  2d-only, no-coreclr, etc.) must compile AND pass the existing test suite,
  and the minimal config should be added to the CI matrix so it cannot
  bitrot. a size win that only builds in one feature combination is not a
  win.
- everything riskier (removing a dep outright, algorithmic rewrites from
  part A) stays a recommendation in the report.

## stretch goals (opt-in directions, not required for the core deliverable)

these are explored and written up as recommendations only; implementation is
out of scope for this pass unless a piece turns out to be a trivial,
measurable win.

### split release layout for moddability

- partly exists already: the plugin loader (native via `libloading` +
  NativeAOT/CoreCLR, the C ABI in `bindings/c`, plus C# scripting) and
  data-driven content (scene format, `.li`, RON) already let modders load
  native/managed plugins and swap data without recompiling.
- the new part is making a split layout a first-class *release* option:
  engine-as-shared-lib + thin launcher + game logic as loadable module(s) +
  external data, selectable against the opposite "single fully-static musl
  binary" path. support both; the engine's range (lowest lows to highest
  highs) wants both, and neither is forced.
- the costs are two, and size is the smaller one. first, a runtime cost: a
  dynamic engine/module boundary forfeits the cross-boundary LTO and inlining
  the single static binary gets, and adds indirect-call overhead at the seam.
  per the runtime-over-everything principle this means the single static
  binary stays the default for max performance, and the split layout is an
  opt-in choice a dev makes when moddability is worth that cost (quantify the
  cost where a bench can show it). second, API stability: a mod-facing
  surface must not break casually, which collides with the project's
  "breaking changes are always fine" policy (see feedback_breaking_api). a
  moddable release therefore needs ONE designated stable ABI/API, explicitly
  versioned, separate from the freely-breaking internal API. flag this
  conflict; do not silently assume the breaking policy holds at the mod
  boundary.

### shippable debug symbols without binary bloat

- correct the premise: debug symbols do not meaningfully ease decompilation
  of optimized code (that is governed by opt-level/inlining, not symbol
  names). what they buy is real stack traces, profiling, and modding tooling.
- the bloat worry is solvable: replace today's `strip = "symbols"` with
  `split-debuginfo` so symbols ship as an OPTIONAL sidecar (`.pdb` on
  windows, separate `.debug`/`.dwp` on linux/macos, source maps for wasm).
  the shipped binary stays the same small size; the sidecar is a separate,
  on-demand download. measure the sidecar size so the user's open "how much
  does it bloat" question is answered with a real number, not a guess. note
  this is a different release config from part C's `size-min` (which strips
  hard for the smallest possible artifact): one optimizes for minimum size,
  the other for debuggability/moddability. they are alternative release
  presets, not a single profile trying to do both.
- moddability ranking, honestly: a stable plugin/scripting API + data-driven
  content (strongest) > sidecar symbols for debuggability (useful) >
  "intentionally decompile-friendly" (weakest, since the engine is already
  open-source under MPL-2.0, so a decompile only exposes the dev's own game
  logic). recommend the first two; treat decompile-friendliness as the
  lowest-value lever.

## output artifact

- committed report at `docs/audit-perf-footprint.md` covering parts A and B,
  the part-C results with before/after numbers, and the stretch-goal
  recommendations (split release layout, sidecar symbols) as written-up
  options rather than applied changes.
- the low-risk code changes (profile, feature gates) committed separately
  with measurements.

## success criteria

- every perf gap and every size claim is backed by a real measurement or a
  file:line, never a guess. each measurement records the exact command,
  rustc/toolchain version, and target triple so the numbers reproduce.
- a documented, reproducible path to a meaningfully smaller "simple game"
  binary (native and wasm), achieved without harming the high end or
  burdening the developer.
- the report clearly separates "already optimal" from "real opportunity".
