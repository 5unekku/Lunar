# lunar engine — roadmap

## smart rendering — performance accessibility

target: 60fps on any modern mid-range CPU (2015 i5 / Ryzen 3 or better) for a full 3d
game, matching the accessibility of Halo CE/2/3, Quake 1/3, and Doom 1/3.

the principle: **earn the right to skip work** at each stage of the pipeline. precompute
what doesn't change. cull early and cheaply. never pay per-frame for something that is
already known. see `plans/accessibility-gap.md` for the full analysis.

---

## phases — all complete

**phase 1 — low-hanging fruit:** parallel ECS system execution, true minimum quality
tier (`QualityPreset::Minimum` with all post-processing off), dirty-flag shadow cascades,
1-frame pipelined GPU cull readback.

**phase 2 — parallel rendering:** parallel render command recording across graph nodes,
async asset streaming (disk reads on worker threads, `Handle<T>` API unchanged).

**phase 3 — smart geometry:** BSP offline compiler + runtime portal culling
(`lunar-bsp`, `lunar-bsp-build`, `bake-pvs` tool), lightmap baker (`lunar-lightmap`,
wired into renderer), auto-LOD generation (`tools/gen-lods`).

**phase 4 — far-geometry and memory:** impostor/billboard system, GPU mip streaming
infrastructure (coverage-based eviction), gamedata build pipeline (`lunar-gamedata-build`).

---

## non-negotiable rules

- **audio is deferred** — all audio work belongs to the moonwalker project. no audio code in this workspace until moonwalker is ready for integration.
- **no async runtime** — async needs are covered by `pollster::block_on` (wgpu init), `std::thread` + crossbeam (asset IO), and `wasm_bindgen_futures::spawn_local` (wasm fetch). rayon only if profiling proves it necessary.
- **prelude is the contract** — game code depends only on `lunar`. `bevy_ecs`, `wgpu`, `sdl3` never appear in a game's `Cargo.toml`. any leak is a bug.
- **editor is downstream** — the editor lives in a separate repo that depends on `lunar`. no editor code in this workspace.
- **performance trinity** — maximum performance, optimized resources, ease of use. YAGNI / KISS / DRY. unsafe only in engine internals with `// SAFETY:` blocks.
- **gpu rules** — see `plans/performance.md` for the hard render rules. violations have known downstream costs.
