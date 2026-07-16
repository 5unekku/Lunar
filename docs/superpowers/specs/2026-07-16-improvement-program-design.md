# lunar improvement program — design

date: 2026-07-16
status: approved (user directed autonomous execution)

## goals

1. aggressive performance optimization, native-first (linux, windows), no barriers for mac/wasm
2. code quality, correctness, robustness, security
3. project design and architecture

## approved constraints

- **perf techniques**: new `unsafe` allowed where the win is measured and invariants are
  documented; breaking public API changes allowed with migration notes; nightly-only
  features allowed (repo already pins `channel = "nightly"` in `rust-toolchain.toml` and
  uses nightly codegen-backend profile keys) as long as every target in the build matrix
  keeps compiling
- **measurement**: a reproducible bench harness is built *first*; every perf change ships
  with before/after numbers from it
- **architecture scope**: audit judges freely, with explicit attention to organization /
  separation of concerns, the public facade + prelude, and the build & platform matrix
- **security threat model**: hostile asset files (textures, meshes, bsp, gamedata, audio,
  saves), the plugin/FFI/CoreCLR boundary, dependency hygiene (audit/deny), and soundness
  of existing `unsafe`

## non-goals

- no visual editor work (unchanged repo non-goal)
- no regressions to the high end: the engine must still reach "the prettiest games"
  (carried over from `docs/superpowers/specs/2026-06-27-perf-footprint-audit-design.md`);
  LoFi and size levers stay opt-in. verified via the golden-frame gate (phase 0), not
  by assertion
- no gameplay features: this program changes how the engine runs, not what it does
- no re-litigating deliberately-skipped optimizations without new evidence (see
  "prior-work constraints")

## guiding principles

- **runtime over compile time** (carried from the 2026-06-27 audit): compile cost is
  acceptable when it buys runtime speed
- **evidence before action**: a finding enters the backlog only after adversarial
  verification; a perf change lands only with harness numbers
- **structural before dependent**: architecture moves are sequenced ahead of the
  perf/quality changes that would otherwise target soon-to-move code

## structure

**phase 0 — bench harness.** new workspace member `tools/render-bench` (criterion
precedent: `tools/navmesh-bench`) plus a new `bench` xtask subcommand (xtask currently
has `build`/`dist`/`run`).

- *render-bench binary*: headless `RenderEngine3d` boot (precedent: `examples/headless_probe`;
  CI's test job installs lavapipe so a headless adapter exists there too). procedurally
  generated scenes, no asset downloads:
  - `static-city` — static-mesh-heavy, several textured materials (bindless path);
    exercises cull/HZB/LOD, draw-list gather, multi-draw
  - `dynamic-swarm` — thousands of moving entities + hundreds of point lights; exercises
    transform propagation, clustered lighting, shadow faces
  - `sprite-storm` — 2D pipeline stress (sprites + text) through `lunar-render`
  - `feature-reel` — every scene-content feature pass in one frame: terrain, water,
    decals, detail sprites, particle emitters, panorama/atmos sky, plus a few static and
    dynamic meshes. exists so the golden-frame gate actually covers the passes the other
    scenes never draw
- *metrics per scene*, written as JSON + a markdown table to `docs/bench/` (committed,
  keyed by hostname + adapter string): cold-start → first-frame time; pipeline-cache
  cold vs warm delta; steady-state CPU frame time over ≥500 frames (mean/p50/p99); GPU
  pass timings via `TIMESTAMP_QUERY` where the adapter offers it (optional, never required)
- *golden frames*: each scene renders one deterministic frame (fixed seed, camera, and
  frame index) to a PNG; per-adapter reference images are committed with the baseline.
  every 3D bench scene inserts `DevRenderProfile::full()` and pins `QualitySettings` to
  the maximum tier, set after plugin init (`RenderPlugin3d` overwrites QualitySettings
  from the detected tier during build). both knobs are required: each pass gate ANDs the
  dev profile with quality, a missing profile defaults to `classic()` (SSR, GTAO,
  volumetric fog, bloom, and contact shadows off — only fxaa/staa stay on — plus an
  8-point-light clamp and no point-light shadows, which would also hollow out
  dynamic-swarm's clustered/shadow coverage), and contact shadows are a
  DevRenderProfile-only toggle with no QualitySettings field. with both pinned, SSR,
  GTAO, volumetric fog, STAA, bloom, and contact shadows are active in every 3D golden
  frame; `feature-reel` supplies the scene-content passes. subsequent runs diff against
  the reference with a small per-channel tolerance (absorbs driver-level float
  variance). this is the mechanical check behind the "no high-end regressions" non-goal
  — an intentional pixel change requires explicit user sign-off and a refreshed
  reference, never a silent threshold bump
- *criterion micro-benches* in the crates they measure: `lunar-math` (transform/quat ops),
  `lunar-3d` (cull SoA build, `propagate_transforms_3d` parented + parentless),
  `lunar-image` (decode), `lunar-bsp` (PVS/leaf lookup). CPU-only, so they run in CI
  containers too
- *baseline*: captured on the primary dev machine (RX 7800 XT, RADV) and committed.
  methodology: ≥3 runs, median reported; a claimed win must beat run-to-run noise or be
  justified on other grounds (memory, size) or reverted
- harness lands immediately (it is infrastructure, not a finding), in parallel with phase 1;
  extended later with whatever the perf audit finds missing

**phase 1 — six parallel audits, one per concern**, each a multi-agent workflow with
fan-out finders and adversarial verification (a finding survives only if an independent
skeptic with repo access fails to refute it). each audit writes
`docs/superpowers/audits/2026-07-<concern>.md`. uniform finding schema: id, `file:line`,
claim, evidence, impact class, effort (S/M/L), confidence, verification verdict. every
audit receives the "prior-work constraints" list and must not re-report those items
without new evidence. audits are read-only — no code changes land from phase 1.

- **perf-native**: cpu hot paths (per-frame allocation, data layout, change-detection
  gaps, parallelism), gpu submission (uploads, bind-group churn, pass structure),
  bandwidth, startup. every recommendation states its wasm/mac story. re-checks
  change-detection coverage across per-frame systems (the june review landed early-outs
  for transform propagation and static-slot bookkeeping; verify nothing comparable
  remains unguarded)
- **correctness & robustness**: logic bugs, panic paths, unwrap/expect on fallible paths,
  edge cases (resize, device-lost, empty scenes, zero-sized buffers), error-handling
  gaps. known seeds: (a) `contact_shadow_tex` is created once and never resized —
  creation is gated on `is_none()` (`lunar-render-3d/src/resources.rs:645`) and the
  resize path nulls only the bind group (`config.rs:89`), so the texture stays at the
  original window size; (b) `DevRenderProfile`'s struct-level rustdoc claims `default()`
  is "all on" and that a missing resource makes every feature available
  (`lunar-render-3d/src/lib.rs:960` and `:966-967`), but `Default` actually returns
  `classic()` — the minimal profile, expensive features disabled (`lib.rs:1019-1025`);
  the docs contradict the code
- **security**: (a) hostile asset files — every parser (`lunar-image`, `lunar-bsp`,
  `lunar-gamedata`, `lunar-assets`, save/load, audio) audited for panics, unbounded
  allocation, integer overflow on malformed input; (b) plugin/FFI boundary —
  `lunar-plugin-loader` (libloading), `lunar-dotnet-host` (CoreCLR), `bindings/c`:
  soundness of every FFI signature, lifetime and thread assumptions documented;
  (c) dependency hygiene — cargo-audit/cargo-deny run + CI wiring proposal;
  (d) `unsafe` inventory — every existing block gets a stated invariant and a verdict
  (sound / needs-fix / needs-comment)
- **architecture & separation of concerns**: crate boundaries, module organization
  (whether `lunar-render-3d` at ~17k lines is cohesive or a split candidate — judged on
  merit, not size alone), layering violations, dependency direction, 2D/3D duplication
- **public api / facade**: `lunar` facade + prelude coherence, `Handle<T>` and plugin
  surface, the `tests/api_seal` contract, docs/prelude drift. breaking changes allowed,
  each with a migration note
- **build & platform matrix**: `scripts/build_all.go` (13 triples), `scripts/run_wasm.go`,
  xtask, `bindings/c`, dotnet host packaging, and CI (`.github/workflows/ci.yml`: clippy
  `-D warnings`, tests under lavapipe, wasm32 `lunar-game` build, xvfb smoke, 7-triple
  zigbuild cross matrix) — simplification, gaps (mac leg, bench leg, audit/deny leg),
  duplication between build_all.go and CI

**phase 2 — synthesis.** dedupe findings that share a root cause across audits; rank by
impact × confidence ÷ effort with user-goal weighting (perf and correctness/security
over polish); sequence structural moves before the perf/quality work they would
invalidate. findings are anchored to `file:line` at audit time; synthesis re-anchors
anything displaced by wave-1 structural moves before wave 2 starts. output: one
prioritized backlog document, `docs/superpowers/specs/2026-07-<date>-improvement-backlog-design.md`.

**phase 3 — implementation waves.**
- wave 1: correctness/security fixes + architecture moves that unblock later work
- wave 2: perf items, each measured against the harness baseline
- wave 3: api/facade + build-matrix cleanup

**phase 4 — self-review loop.** review the accumulated diff, fix findings, repeat until
three consecutive clean passes.

## implementation rules

- **unsafe**: allowed with (a) a measured harness win or a soundness fix, (b) a
  `// SAFETY:` comment stating the invariant, (c) miri on the touched code where miri
  can run it (pure cpu, no ffi/gpu), (d) fuzz or property tests when the unsafe sits
  near parser input
- **breaking api changes**: migration note in the commit message + updated docs;
  `tests/api_seal` updated deliberately, never weakened silently
- **nightly features**: must keep the wasm and cross-build CI legs green (they share the
  same pinned nightly)
- **platform floor per landed change**: existing CI stays green — clippy `-D warnings`,
  workspace tests, wasm32 `lunar-game` build, xvfb smoke, 7-triple cross-build. macOS has
  no CI leg today; until the build-matrix audit addresses that, mac-affecting changes
  (target-specific code, new deps) are cfg-gated with portable fallbacks (the existing
  NEON/AVX2 pattern)
- **perf items**: before/after harness numbers in the commit message; ≥3 runs, median;
  within-noise results don't claim a win. golden-frame diffs must stay within tolerance
  on every scene; intentional visual changes go through the user sign-off rule (phase 0)
- **bug fixes**: test-first — the test fails before the fix, passes after
- **asset parsers**: fuzz targets (cargo-fuzz; the toolchain is already nightly) added
  per parser family as wave-1 work; crashers found become test cases
- **git**: commit per distinct piece; lowercase, casual, succinct messages; no
  attribution trailers; no pushing

## prior-work constraints (do not re-litigate without new evidence)

from the 2026-06 passes (project memory + `docs/audit-perf-footprint.md`):

- **`WorldTransform3d` cached-matrix side-field**: rejected (component bloat regresses
  hot query sweeps). the only acceptable form is the full `Affine3A` storage migration —
  now in-bounds since breaking changes are allowed, and a legitimate perf/architecture
  audit candidate, but it must be evaluated as the migration, never as a side-cache
- **frustum-visible `FxHashSet` → bitset**: rejected — gather-side hashing remains either
  way; revisit only with a profile showing the hashes hot
- **static model-matrix cache**: rejected — recompute ≈ lookup, +64 B/entity; revisit
  only if a profile shows `to_matrix` hot
- **`passes.rs` per-frame gathers → self-owned scratch**: initially rejected (borrow
  dance for ~8 allocator ops/frame) but implemented after all in commit 7e012ce
  (2026-07-07, per-frame scratch buffers for feature passes) — treat as landed, do not
  re-propose
- **per-pass cargo features compiling out 3D effects** (SSR/GTAO/fog/water/terrain/
  particles): deferred, not rejected — needs a GPU-verified plan; the build-matrix audit
  may re-propose it as a designed project only
- positions that stand: SPIR-V passthrough is vulkan-only by design; `PIPELINE_CACHE` is
  absent on DX12-via-proton (degrades cleanly) — don't chase it

## risks

- **findings against moved code**: wave-1 restructuring can displace `file:line` anchors
  → synthesis re-anchors before wave 2
- **single-rig perf truth**: all baselines come from one RADV machine. mitigation: record
  adapter/driver in every result; backend-sensitive changes get a native-vulkan
  measurement at minimum and state the DX12 caveat; cpu-side wins (the majority) are
  backend-independent
- **wasm can't run the render bench**: wasm safety is enforced as compile-green + smoke,
  not benchmarks; native fast paths stay cfg-gated with portable fallbacks
- **audit volume**: six parallel audits can produce hundreds of findings; the
  adversarial-verification gate and impact ranking keep the backlog honest and executable
- **nightly drift**: the toolchain is a floating `channel = "nightly"`; a bad nightly can
  break the build independent of this program. not made worse by it; the build-matrix
  audit evaluates pinning a dated nightly

## success criteria

1. the bench harness runs in one command (`cargo xtask bench`); baselines committed under
   `docs/bench/`
2. six audit reports exist under `docs/superpowers/audits/`, every finding carrying a
   verification verdict
3. the backlog document exists and every item traces to a verified finding
4. every landed perf change carries before/after medians; bench-scene p99 frame times
   improve against baseline by the end of wave 2, and golden-frame diffs stay within
   tolerance on every scene (intentional changes carry user sign-off + refreshed
   references)
5. wave 1 ends with: zero known fuzz crashers across asset parsers, every `unsafe` block
   carrying a stated invariant, cargo-audit/deny wired into CI and green, and the seeded
   contact-shadow resize bug fixed with a test
6. CI stays green throughout on every existing leg
