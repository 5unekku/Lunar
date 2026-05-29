# lunar engine — roadmap

## smart rendering — performance accessibility

target: 60fps on any modern mid-range CPU (2015 i5 / Ryzen 3 or better) for a full 3d
game, matching the accessibility of Halo CE/2/3, Quake 1/3, and Doom 1/3.

the principle: **earn the right to skip work** at each stage of the pipeline. precompute
what doesn't change. cull early and cheaply. never pay per-frame for something that is
already known. see `plans/accessibility-gap.md` for the full analysis.

items are ordered by return on investment. the first four are wiring work (days to weeks).
the rest are systems work (weeks to months).

---

### phase 1 — low-hanging fruit (days each)

~~**parallel ECS system execution**~~
- bevy_ecs supports parallel system scheduling natively; its scheduler can run
  non-conflicting systems concurrently
- `lunar-core`'s stage runner currently dispatches systems sequentially on the main thread
- wire `Schedule` to use a rayon-backed parallel executor
- game logic systems (AI, physics, animation, particles) run simultaneously instead of
  in sequence; near-linear scaling with core count
- home: `lunar-core` stage executor
- blocks nothing; pure performance multiplier on all games

~~**true minimum quality tier**~~
- `QualityPreset::Low` currently runs several post-processing passes at reduced resolution
- minimum should mean *none*: GTAO off, SSR off, volumetric fog off, bloom off, FXAA off
- composite pass reduces to tonemap + gamma only
- add `QualityPreset::Minimum` that sets all `QualitySettings` booleans to false
- all existing quality toggles already exist; this is just setting their defaults
- home: `lunar-render-3d` quality settings
- Quake 3 had zero post-processing; if a game ran in 1999, it should run on minimum today

~~**dirty-flag shadow cascades**~~
- all 3 CSM cascades re-render every frame, even when nothing in the scene has moved
- each cascade should track a dirty flag: re-render only when geometry or light direction
  within its frustum has changed
- static scenes (no moving objects, fixed sun) can skip all 3 passes every frame
- home: `lunar-render-3d` shadow pass
- for most game levels, shadow passes drop from 3 per frame to 0–1 per frame

~~**1-frame pipelined GPU cull readback**~~
- the tier-4 GPU frustum cull and HZB cull both block on `device.poll(Wait)` each frame
- fix: copy cull results to staging, submit, read the *previous* frame's results
  (1-frame lag is imperceptible for culling decisions)
- eliminates the CPU stall entirely; ~0.3ms recovered per frame
- home: `lunar-render-3d` frustum cull section

---

### phase 2 — parallel rendering (weeks)

~~**parallel render command recording**~~
- all render passes currently recorded on the main thread sequentially
- wgpu `CommandEncoder` is `Send`; each pass can record on a separate rayon thread
- the render graph DAG already models pass dependencies and topological order
- shadow pass, z-prepass, opaque pass, post-processing — all independent from each other
  once the entity buffer is uploaded
- submit all encoders in topological order after all threads complete
- home: `lunar-render-3d` render graph dispatch
- near-linear CPU scaling with pass count; 6–8 independent passes on 4 cores = ~3× speedup
  on the command recording phase

~~**async asset streaming (I/O only)**~~
- textures and meshes currently block the main thread on load
- move all disk reads to `std::thread` workers with a channel back to the asset server
- game code sees the same `Handle<T>` API; asset becomes available in a future frame
- do not stream GPU mip levels yet (that's phase 4)
- home: `lunar-assets` loader backend
- eliminates frame stalls during level load; levels can stream in behind a loading screen
  without blocking rendering

---

### phase 3 — smart geometry (months)

~~**BSP offline compiler + runtime portal culling**~~
- the single largest gap against classic engines; see `plans/accessibility-gap.md`
- offline tool: takes a level mesh (GLTF or lunar scene), builds a BSP tree, computes
  PVS bitsets for all leaves, writes a binary `.bsp` asset
- runtime: `BspLevel` component; `BspCulling` resource; per-frame walk from camera leaf,
  mark visible leaves via PVS bitset, only submit geometry from visible leaves
- add area/portal on top: designer-placed `Portal` planes that connect areas; a closed
  door with no visible portal eliminates an entire wing of the level at zero GPU cost
- reference: Quake 1 `bspfile.h` + `world.c` (GPL, logic reference only);
  Godot `CSGShape`-based BSP (MIT, copy logic freely)
- home: new `lunar-bsp` crate (compiler) + `lunar-3d` BSP scene components
- impact on indoor levels: 80–95% reduction in visible geometry per frame; everything
  downstream (draw calls, shadows, lighting, post-processing cost) scales down with it

~~**lightmap baker**~~
- `Vertex3d` already has `uv_lightmap`; the shader already reads it; the infrastructure
  is there and unused
- offline tool: unwrap static geometry to lightmap UVs (xatlas or equivalent); trace
  irradiance samples per texel; output an RGBA8 or BC3-compressed lightmap texture
- runtime: bind lightmap texture per mesh; shader multiplies base color by lightmap sample
  for static geometry; dynamic objects continue using runtime PBR
- reference: Quake 1 lighted BSP faces (GPL, logic only); Godot `LightmapGI` (MIT)
- home: new `lunar-lightmap` crate (baker tool) + shader changes in `lunar-render-3d`
- impact: eliminates runtime directional light cost for all static geometry; GTAO and
  runtime lighting apply only to dynamic entities (characters, projectiles, vehicles)

---

### phase 4 — far-geometry and memory (months)

~~**impostor / billboard system**~~
- `MeshLod` already supports coarser meshes at distance; impostors are the final LOD level
- for entities beyond the last LOD threshold, render a camera-facing quad pre-rendered
  with the object's silhouette and lighting from multiple angles (impostor atlas)
- baker: renders entity from N angles, packs into an atlas texture; runtime selects the
  closest angle and blends between adjacent ones
- reference: Ogre `StaticGeometry` impostor page (MIT); Halo 3's impostor system (reference only)
- home: `lunar-3d` `MeshImpostor` component + `lunar-render-3d` impostor pass

~~**GPU mip streaming**~~
- fixed VRAM budget; textures stream in finer mips as entities approach camera
- mip residency tracked per texture; background thread uploads resident mips; renderer
  samples the highest resident mip available
- pairs with BSP culling: non-visible areas don't need fine mips loaded
- home: `lunar-assets` + `lunar-render-3d` mip residency tracker

~~**gamedata build pipeline**~~
- `WorldManifest` and `SceneDefinition` currently parse RON/XML at runtime every level load
- `build.rs` compiler: TOML/RON/XML source → flat binary blob embedded via `include_bytes!`
- O(1) access at runtime; zero parsing cost; pairs well with async streaming
- already in the feature roadmap; noting it here because runtime parsing is a
  frame-budget cost during level load
- home: `lunar-gamedata-build` compiler (already scaffolded)

---

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
- ~~`AssetServer` eviction → `RenderEngine::remove_texture` wiring~~ — done; `release_texture` decrements ref count, drains evicted IDs each frame, GPU bind group cleanup triggered

### ~~shooter example~~
done — `examples/shooter_example`: player, bullets, enemies, AABB collision, score/lives display. depends only on `lunar::prelude`.

### crate metadata
- ~~`[workspace.package]` fields — done~~
- ~~per-crate `description` fields — done~~
- ~~`cargo doc --no-deps` clean pass~~ — done; all intra-doc link warnings resolved across workspace

### wasm: upstream wgpu patch
the `GPUCanvasContext` `instanceof` fix is vendored locally. submit PR to gfx-rs/wgpu and track until merged. downstream users currently need a `[patch.crates-io]` entry in their workspace.

### ~~rpg example migration~~
confirmed clean — no `bevy_ecs` imports in `examples/rpg_example/`. all files use `lunar::prelude` only.

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
| ~~**render to texture**~~ | ~~`lunar-render` + `lunar-render-3d`~~ | ~~high~~ | done — `RenderTargetId`, `RenderTargetStore`, `Camera.target`; GPU tex with `RENDER_ATTACHMENT \| TEXTURE_BINDING`; sample view exposed as `Handle<Texture>` |
| ~~**post-processing framework**~~ | ~~`lunar-render`~~ | ~~medium~~ | done — `PostProcessStack` resource; `ScreenFlash` (auto-decay), `ColorTint`; custom `PostEffect` trait; `draw_screen_rect` at `layers::POST_PROCESS` |
| ~~**screen shake**~~ | ~~`lunar-render`~~ | ~~medium~~ | done — `ScreenShake` resource, trauma² noise offset |
| ~~**multiview / split-screen**~~ | ~~`lunar-render` + `lunar-render-3d`~~ | ~~low~~ | done — `ViewportRect` component on Camera3d + `ActiveViewports` resource; renderer applies scissor/viewport for primary camera; multi-camera via render-to-texture for secondary cameras |

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
| ~~**proper PBR lighting**~~ | ~~`lunar-render-3d`~~ | ~~high~~ | done — Cook-Torrance BRDF (GGX NDF, Smith-GGX geometry, Schlick Fresnel); directional + 8 point lights; 3-cascade CSM at 1024² with 5×5 PCF; Z-prepass on mid/high tier; dynamic resolution scaling |
| ~~**bind group layout standardisation**~~ | ~~`lunar-render-3d`~~ | ~~medium~~ | done — 4-group layout: group 0 globals, group 1 material, group 2 per-mesh, group 3 lights+shadow |
| ~~**post-processing pipeline**~~ | ~~`lunar-render-3d`~~ | ~~medium~~ | done — HDR RGBA16Float intermediate target; 4×MSAA on mid/high; Kawase bloom (5 mips mid, 7 mips high); ACES filmic tonemap; composite pass: vignette, chromatic aberration, film grain; `QualitySettings` resource with per-tier defaults |
| ~~**GTAO ambient occlusion**~~ | ~~`lunar-render-3d`~~ | ~~medium~~ | done — half-res horizon-based AO (XeGTAO-style); depth reconstruction from z-prepass; 5-tap bilateral blur; enabled mid/high via `QualitySettings.ssao` |

---

### plugins — optional, opt-in crates

game code adds these to its `Cargo.toml` only when needed.

| plugin crate | priority | what it provides |
|---|---|---|
| ~~`lunar-physics-2d`~~ | ~~critical~~ | done — gravity, velocity integration, AABB collision response, one-way platforms |
| ~~`lunar-physics-3d`~~ | ~~high~~ | done — `KinematicBody3d` component; `move_and_slide_3d` with iterative AABB depenetration; slope detection; gravity; `ColliderEntryRef` added to `CollisionWorld3d` |
| ~~`lunar-particles`~~ | ~~high~~ | done — `ParticleEmitter` + `ParticlePool`, tick + draw systems |
| ~~`lunar-pathfinding-rt`~~ | ~~high~~ | done — A* on `NavGrid` resource; 4-dir and 8-dir movement; per-tile cost; corner-cut prevention; max_nodes cap |
| ~~`lunar-pathfinding-pre`~~ | ~~medium~~ | done — Dijkstra flow field baked at level load; O(1) per-agent step query; corner-cut prevention; `cost_at` for debug |
| ~~`lunar-ai`~~ | ~~medium~~ | done — `BehaviorTree` component; `Selector`, `Sequence`, `Invert`, `Condition`, `Action`; `tick_behavior_trees` exclusive system; multi-frame Running support |
| ~~`lunar-camera-3d`~~ | ~~medium~~ | done — `SpringArm3d` component; yaw/pitch orbit; arm shortens on raycast hit, recovers smoothly; `spring_arm_system` |
| ~~`lunar-spline`~~ | ~~low~~ | done — catmull-rom `Spline` asset; `SplineStore` resource; `PathFollower` component; `advance_path_followers` system; arc-length parameterization |
| ~~`lunar-timeline`~~ | ~~low~~ | done — `Timeline` component; `TimelineTrack` + `TimelineKey`; `MoveTo`, `TeleportTo`, `SetVisible`, `FireEvent` actions; `TimelineEvents` buffer resource; loop support |
| `lunar-audio-sync` | low | beat offset + hit window utilities for rhythm games; reads audio playback timestamp (moonwalker dependency — deferred until moonwalker) |

---

## deferred / out of scope

- audio (moonwalker)
- replay system
- day/night cycling (game-specific, not an engine concern)
- network multiplayer
- editor (downstream project)
