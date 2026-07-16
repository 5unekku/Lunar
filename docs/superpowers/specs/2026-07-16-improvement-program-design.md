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
  features allowed (repo already uses nightly codegen-backend) as long as every target
  in the build matrix keeps compiling
- **measurement**: a reproducible bench harness is built *first*; every perf change ships
  with before/after numbers from it
- **architecture scope**: audit judges freely, with explicit attention to organization /
  separation of concerns, the public facade + prelude, and the build & platform matrix
- **security threat model**: hostile asset files (textures, meshes, bsp, gamedata, audio,
  saves), the plugin/FFI/CoreCLR boundary, dependency hygiene (audit/deny), and soundness
  of existing `unsafe`

## structure

**phase 0 — bench harness.** criterion micro-benches for hot subsystems plus headless
stress scenes capturing avg/p50/p99 frame times, runnable via `cargo xtask bench`;
baselines committed. extended later with whatever the perf audit finds missing.

**phase 1 — six parallel audits, one per concern**, each a multi-agent workflow with
fan-out finders and adversarial verification (a finding survives only if an independent
skeptic fails to refute it):

- perf-native: cpu/gpu hot paths, allocation, data layout, parallelism, bandwidth
- correctness & robustness: logic bugs, panics, edge cases, error handling
- security: asset parsers, ffi/plugin boundary, unsafe audit, dependency hygiene
- architecture & separation of concerns: crate boundaries, module organization, layering
- public api / facade: prelude coherence, handle/plugin surface, api_seal contract
- build & platform matrix: build_all.go, xtask, bindings, dotnet host, ci

**phase 2 — synthesis.** dedupe across audits, rank by impact/effort/risk, sequence
structural moves before the perf/quality work they would invalidate. output: one
prioritized backlog document.

**phase 3 — implementation waves.**
- wave 1: correctness/security fixes + architecture moves that unblock later work
- wave 2: perf items, each measured against the harness baseline
- wave 3: api/facade + build-matrix cleanup

**phase 4 — self-review loop.** review the accumulated diff, fix findings, repeat until
three consecutive clean passes.

## implementation rules

- unsafe: documented invariants + measured win, miri/proptest coverage where feasible
- every wave keeps tests, clippy, and the cross-platform build matrix green
- bug fixes land test-first; asset parsers gain fuzz/property tests
- commit per distinct piece of functionality
