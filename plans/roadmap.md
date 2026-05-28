# lunar engine — roadmap

## non-negotiable rules

- **audio is deferred** — all audio work belongs to the moonwalker project. no audio code in this workspace until moonwalker is ready for integration.
- **no async runtime** — async needs are covered by `pollster::block_on` (wgpu init), `std::thread` + crossbeam (asset IO), and `wasm_bindgen_futures::spawn_local` (wasm fetch). rayon only if profiling proves it necessary.
- **prelude is the contract** — game code depends only on `lunar`. `bevy_ecs`, `wgpu`, `sdl3` never appear in a game's `Cargo.toml`. any leak is a bug.
- **editor is downstream** — the editor lives in a separate repo that depends on `lunar`. no editor code in this workspace.
- **performance trinity** — maximum performance, optimized resources, ease of use. YAGNI / KISS / DRY. unsafe only in engine internals with `// SAFETY:` blocks.
- **gpu rules** — see `plans/performance.md` for the hard render rules. violations have known downstream costs.

---

## open foundation items

these are gaps in what already exists — polish, wiring, and cleanup.

### asset loading UX
- ~~`AssetServer::block_until_all_ready()` — done~~
- ~~`LoadingState { total, loaded, failed }` resource — done~~
- `AssetServer` eviction → `RenderEngine::remove_texture` wiring — currently the asset server doesn't evict, so bind group cleanup is never triggered

### shooter example
ground-truth proof that the external-user experience works end-to-end. game with: player, bullets, enemies, AABB collision, score display. depends only on `lunar`. see old item 49 in git history for full scope.

### crate metadata
- ~~`[workspace.package]` fields — done~~
- ~~per-crate `description` fields — done~~
- `cargo doc --no-deps` clean pass — fix broken doc links, ensure all public types have doc comments

### wasm: upstream wgpu patch
the `GPUCanvasContext` `instanceof` fix is vendored locally. submit PR to gfx-rs/wgpu and track until merged. downstream users currently need a `[patch.crates-io]` entry in their workspace.

### rpg example migration
`rpg-example` still has some direct `bevy_ecs` imports. finish migrating to `lunar::prelude` only — proves the facade is livable on a non-trivial project.

### game data format (`lunar-gamedata`)
baked binary format for static game content (characters, rooms, dialogue nodes, emotions). TOML source → `build.rs` compiler → flat binary blob embedded via `include_bytes!`. O(1) access, zero runtime parsing. two crates: `lunar-gamedata` (reader) + `lunar-gamedata-build` (compiler). design doc needed before implementation.

note: `SceneDefinition` and `CompiledWorld` already have working `to_binary`/`from_binary` implementations but no build pipeline wires them up — `SceneLoader` and `WorldManifest` always parse RON/XML at runtime. the `lunar-gamedata-build` compiler should also handle pre-compiling scenes and world manifests as part of the same build step.

---

## feature roadmap

features are grouped by where they live and roughly ordered by priority within each group.
see `plans/optimize.md` for GPU architecture context behind the render items.

---

### all — every game, dimension-agnostic

these belong in `lunar-core` or the relevant base crate.

| feature | home | priority | notes |
|---|---|---|---|
| ~~**save/load**~~ | ~~`lunar-core/persist`~~ | ~~critical~~ | done — `persist::save/load<T>`, RON, WASM stub |
| ~~**entity pooling**~~ | ~~`lunar-core/pool`~~ | ~~high~~ | done — `Pool` resource, acquire/release/grow |
| **render to texture** | `lunar-render` + `lunar-render-3d` | high | `RenderTarget` handle; `Camera`/`Camera3d` gets optional `target` field; prerequisite for minimap, split-screen, compositor tricks |
| **post-processing framework** | `lunar-render` + `lunar-render-3d` | medium | `PostProcessStack` resource; built-in: `ScreenFlash`, `ColorTint`; custom passes via `RenderPass` trait |
| ~~**screen shake**~~ | ~~`lunar-render`~~ | ~~medium~~ | done — `ScreenShake` resource, trauma² noise offset |
| **multiview / split-screen** | `lunar-render` + `lunar-render-3d` | low | `Viewport { rect: ScreenRect, camera: Entity }` component; multiple cameras scissored to sub-rects; needs render-to-texture first |

---

### 2d — every 2d game, via `lunar-2d` / `lunar-render`

| feature | home | priority | notes |
|---|---|---|---|
| ~~**camera follow**~~ | ~~`lunar-render`~~ | ~~critical~~ | done — `CameraFollow2d` resource, deadzone, bounds, lerp |
| ~~**2d raycasting**~~ | ~~`lunar-2d`~~ | ~~high~~ | done — `ray_cast_2d`, ray vs AABB/circle slab test |
| ~~**y-sort**~~ | ~~`lunar-render`~~ | ~~medium~~ | done — `YSort` marker, sort key by world Y |

---

### 3d — every 3d game, via `lunar-3d` / `lunar-render-3d`

| feature | home | priority | notes |
|---|---|---|---|
| ~~**3d raycasting**~~ | ~~`lunar-3d`~~ | ~~critical~~ | done — `Ray3d`, `RayHit3d`, `raycast_3d`: CullSoa AABB broad phase + Möller–Trumbore triangle narrow phase; AABB fallback for mesh-less entities |
| **proper PBR lighting** | `lunar-render-3d` | high | Cook-Torrance BRDF; directional + point lights; one shadow cascade at 1024²; see `optimize.md` for shadow filter progression |
| **bind group layout standardisation** | `lunar-render-3d` | medium | consolidate to 4-group layout: group 0 view-global, group 1 material, group 2 per-mesh, group 3 pass-specific; lets pipelines share bind groups |

---

### plugins — optional, opt-in crates

game code adds these to its `Cargo.toml` only when needed.

| plugin crate | priority | what it provides |
|---|---|---|
| ~~`lunar-physics-2d`~~ | ~~critical~~ | done — gravity, velocity integration, AABB collision response, one-way platforms |
| `lunar-physics-3d` | high | kinematic character controller (move/slide/slope/step) first; full rigidbody optional backend (rapier3d) |
| ~~`lunar-particles`~~ | ~~high~~ | done — `ParticleEmitter` + `ParticlePool`, tick + draw systems |
| ~~`lunar-pathfinding-rt`~~ | ~~high~~ | done — A* on `NavGrid` resource; 4-dir and 8-dir movement; per-tile cost; corner-cut prevention; max_nodes cap |
| ~~`lunar-pathfinding-pre`~~ | ~~medium~~ | done — Dijkstra flow field baked at level load; O(1) per-agent step query; corner-cut prevention; `cost_at` for debug |
| `lunar-ai` | medium | behavior tree evaluation (`Selector`, `Sequence`, `Condition`, `Action`); tree structure + tick loop only; leaf actions written by game code |
| `lunar-camera-3d` | medium | `SpringArm3d` orbit camera; shortens arm via 3d raycast when blocked; depends on `lunar-3d` raycasting |
| `lunar-spline` | low | catmull-rom `Spline` asset; `PathFollower` component; `advance_path_followers` system |
| `lunar-timeline` | low | timed track sequencer for cutscenes; `Timeline { tracks }` + `TimelineAction` enum (MoveTo, SetVisible, FireEvent, …) |
| `lunar-audio-sync` | low | beat offset + hit window utilities for rhythm games; reads audio playback timestamp (moonwalker dependency — deferred until moonwalker) |

---

## deferred / out of scope

- audio (moonwalker)
- replay system
- day/night cycling (game-specific, not an engine concern)
- network multiplayer
- editor (downstream project)
