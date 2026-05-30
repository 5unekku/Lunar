# contributing

## project rules

- **audio is deferred** — all audio work belongs to the moonwalker project. no audio code in this workspace until moonwalker is ready for integration.
- **no async runtime** — async needs are covered by `pollster::block_on` (wgpu init), `std::thread` + crossbeam (asset IO), and `wasm_bindgen_futures::spawn_local` (wasm fetch). rayon only if profiling proves it necessary.
- **prelude is the contract** — game code depends only on `lunar`. `bevy_ecs`, `wgpu`, `sdl3` never appear in a game's `Cargo.toml`. any leak is a bug.
- **editor is downstream** — the editor lives in a separate repo that depends on `lunar`. no editor code in this workspace.
- **performance trinity** — maximum performance, optimized resources, ease of use. YAGNI / KISS / DRY. unsafe only in engine internals with `// SAFETY:` blocks.
- **breaking changes are fine** — this codebase has no public users. never add backward-compat shims. just change the thing.

## render rules

violations have known downstream costs:

1. no CPU wait on `Device::poll(WaitForSubmissionIndex)` from the render thread steady-state path
2. no allocations in the render hot path — use pre-allocated scratch resources that clear each frame
3. no GPU readback in steady state (fatal on ARM — full pipeline stall)
4. no shader compilation mid-frame (compile in queue/prepare stage)
5. every `wgpu::Buffer` and `wgpu::Texture` must have a non-empty label
6. all GPU-bound structs: `#[repr(C)]` + `bytemuck::Pod + Zeroable`, no `Vec3` (use `Vec4` or `[f32; 4]` — std140 expands Vec3 silently)
7. all matrices: column-major, reverse-z convention (near=1, far=0)
8. `wgpu::Limits::default()` is the floor — never rely on elevated limits without a feature gate

## render principles

- sacrifice memory before speed, but intelligently: alignment padding (`Vec3` → `Vec3A`) is always worth it, bulk data duplication is not
- profile before optimising; the scratch resource pattern handles the known bottlenecks
