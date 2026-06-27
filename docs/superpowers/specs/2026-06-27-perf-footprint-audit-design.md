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

Output: a ranked gap list (impact x effort), each entry carrying evidence
and a proposed fix. Known-good areas are explicitly called out as "already
optimal, do not touch" so the report is honest about diminishing returns.

## part B: binary-size / bloat audit (report + measurement)

1. baseline: build `lunar` in release, measure the stripped size. run
   `cargo bloat` (crate + symbol attribution) and `cargo tree` for the dep
   graph. capture real numbers, not estimates.
2. dep-surface map of the 543 crates: mandatory vs feature-gated vs
   removable; duplicate versions; the fat deps (sdl3, lunar-dotnet-host /
   CoreCLR, zstd, wgpu backends, cubeb audio).
3. feature-gating analysis: can a "simple game" build drop .NET hosting,
   unused audio backends, unused wgpu backends, and 2d-only or 3d-only?
   what does each drop save?
4. footprint target doc: what an empty/simple Lunar game floors at today
   vs. the sub-100-MB goal, and which levers (feature strip, size profile,
   procedural assets in the kkrieger spirit) close the gap. NES-game
   reference points included for philosophy, not as a literal 40 KB target.

## part C: low-risk implementation (safe wins only)

- add a size-optimized release profile (e.g. `[profile.release-min]` with
  opt-level `z`/`s`, lto fat, panic abort, strip) and measure before/after.
- add or tighten feature flags so a small game can compile out .NET, extra
  audio backends, and unused wgpu backends. each gate measured.
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
  binary, achieved without harming the high end or burdening the developer.
- the report clearly separates "already optimal" from "real opportunity".
