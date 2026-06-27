# Lunar perf + footprint audit, design

date: 2026-06-27

## goal

Two deliverables on the Lunar engine (~50k Rust LOC, 25 crates, 543 deps in the lock):

1. a performance gap audit against the bar "a simple game should run on a potato"
   (half-life 2 / portal 2 / quake / doom class hardware).
2. a binary-size / bloat audit with a concrete footprint target: a simple,
   NES-styled game (no fancy rendering) should be negligible in size. under
   100 MB is the ceiling for simple games, smaller is better, but never at a
   cost to the game or the developer.

Action scope: report + low-risk wins. Apply only safe, measurable
size/perf changes (size-optimized profile, feature gates). Anything riskier
ships as a written recommendation for the user to greenlight.

## non-goals

- no blind sweep of already-optimized code (NEON/SSE2 cull, FSR3, SIMD SoA
  cull, wasm pump_frame, baked tables, etc. are already landed and verified)
- no aggressive dep removal or refactors this session
- no regressions to the high end (the engine must still reach "the prettiest
  games"); size wins must be opt-in, not forced on every build

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
  low-end discrete card.
- lunar-3d, lunar-core, lunar-2d: ECS query patterns, culling, schedule
  shape, allocations inside the per-tick loop.
- lunar-math, lunar-image, lunar-atlas, lunar-bsp, lunar-lightmap:
  algorithmic hot spots, unnecessary precision/work.

Prioritization: this is time-boxed, not exhaustive. Crates are read in
descending order of hot-path weight (render-3d, 3d, render first, since the
per-frame cost lives there), and the audit stops at diminishing returns
rather than forcing a full read of all 50k LOC. The report states which
crates got a deep read vs. a skim so coverage is honest.

Output: a ranked gap list (impact x effort), each entry carrying evidence
and a proposed fix. Known-good areas are explicitly called out as "already
optimal, do not touch" so the report is honest about diminishing returns.

## part B: binary-size / bloat audit (report + measurement)

1. baseline: build `lunar` in release, measure the stripped size. run
   `cargo bloat` (crate + symbol attribution) and `cargo tree` for the dep
   graph. capture real numbers, not estimates.
2. wasm bundle size: for web targets the `.wasm` is the dominant size
   metric, so it is measured alongside the native binary. build the
   `lunar-web` target, measure the raw and (where the toolchain supports it)
   `wasm-opt` + gzip/brotli sizes, and attribute bloat the same way. this
   ties size work back to the original web-accessibility goal.
3. dep-surface map of the 543 crates: mandatory vs feature-gated vs
   removable; duplicate versions; the fat deps (sdl3, lunar-dotnet-host /
   CoreCLR, zstd, wgpu backends, cubeb audio).
4. feature-gating analysis: can a "simple game" build drop .NET hosting,
   unused audio backends, unused wgpu backends, and 2d-only or 3d-only?
   what does each drop save?
5. footprint target doc: what an empty/simple Lunar game floors at today
   vs. the sub-100-MB goal (native and wasm), and which levers (feature
   strip, size profile, procedural assets in the kkrieger spirit) close the
   gap. NES-game reference points included for philosophy, not as a literal
   40 KB target.

practical risks (expected, not blockers):
- a `release` build with `lto = "fat"` + `codegen-units = 1` is slow
  (minutes); the baseline build is budgeted for, not treated as a surprise.
- `cargo bloat` / `wasm-opt` may not be installed; the plan installs them or
  falls back to `cargo tree` + raw `size`/`ls` attribution.
- a new custom profile name spins up a fresh `target/<name>/` dir and can
  retrigger the cmake/cubeb-sys profile-dir interaction the existing
  Cargo.toml comment documents; part C validates a clean build before
  trusting any size delta.

## part C: low-risk implementation (safe wins only)

- add a size-optimized release profile (e.g. `[profile.release-min]` with
  opt-level `z`/`s`, lto fat, panic abort, strip) and measure before/after.
  validate a clean build links before trusting the delta (see cubeb-sys
  risk above).
- add or tighten feature flags so a small game can compile out .NET, extra
  audio backends, and unused wgpu backends. each gate measured, native and
  where relevant wasm.
- everything riskier (removing a dep outright, algorithmic rewrites from
  part A) stays a recommendation in the report.

## output artifact

- committed report at `docs/audit-perf-footprint.md` covering parts A and B
  plus the part-C results with before/after numbers.
- the low-risk code changes (profile, feature gates) committed separately
  with measurements.

## success criteria

- every perf gap and every size claim is backed by a real measurement or a
  file:line, never a guess.
- a documented, reproducible path to a meaningfully smaller "simple game"
  binary (native and wasm), achieved without harming the high end or
  burdening the developer.
- the report clearly separates "already optimal" from "real opportunity".
