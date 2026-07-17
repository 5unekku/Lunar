# public api & facade design audit

date: 2026-07-16
concern: public api and facade design

method: multi-agent fan-out (one finder agent per lens), location dedup, then every
surviving finding was handed to an independent adversarial verifier whose default was
to refute it — only findings that held up under the verifier's own reading of the code
are listed below. read-only audit; no source was modified. this doc was transcribed by
the orchestrator from the verified finding set after the workflow's writer stage was
cut off by a subagent model-credit limit — the findings and verdicts are the agents', unedited.

scope: the lunar facade + prelude, handle/asset api, the api_seal contract and doc drift, and the plugin-facing configuration surface.

**14 confirmed findings** (2 critical, 4 high, 6 medium, 2 low).

discovery stats: 60 raw findings from 4 lenses (+0 gap follow-ups), 57 after dedup, 14 confirmed, 0 refuted.

every finding carries: id, location (file:line), impact, effort (S/M/L), and the
verified claim (verifier-corrected wording where the skeptic adjusted it). ids are
assigned in severity order and are stable references for the phase-2 backlog synthesis.

---

## api-01 — crates/lunar-assets/src/lib.rs:1350

- **impact:** critical
- **effort:** S
- **verdict:** confirmed (survived refute-by-default verification)

release_texture ignores the handle's generation and panics on out-of-range ids, so the one API that frees assets bypasses the generational safety the Handle type exists to provide.

## api-02 — crates/lunar-assets/src/lib.rs:1467

- **impact:** critical
- **effort:** S
- **verdict:** confirmed (survived refute-by-default verification)

wait_for_all, block_until_all_ready, block_until_texture_ready, and block_until_font_ready spin forever on any pending load because they poll state that only update(&mut self) can transition, and they hold &self while sleeping.

## api-03 — crates/lunar-render-3d/src/hooks.rs:30

- **impact:** high
- **effort:** S
- **verdict:** confirmed (survived refute-by-default verification)

The facade prelude exports ShadowProvider/ShadowCtx whose public fields are raw wgpu types, and the public constructors RenderEngine::from_surface/headless and RenderEngine3d::from_surface/headless take &wgpu::Instance, but no workspace crate re-exports wgpu — so any game doing real GPU work through the hook seam (constructing wgpu descriptors, encoders, or pipelines) or calling the instance-taking constructors must add its own wgpu dependency version-matched to 29.x, violating the api_seal contract's stated rule that wgpu appearing as a direct game dependency is an abstraction regression (only a degenerate no-op hook impl avoids this). Fix: `pub use wgpu;` from the facade (or lunar-render); additive, no migration needed.

## api-04 — crates/lunar/src/prelude.rs:88

- **impact:** high
- **effort:** M
- **verdict:** confirmed (survived refute-by-default verification)

The prelude re-exports bevy_ecs's raw `Bundle` derive, which emits `bevy_ecs::` paths that cannot resolve in a game depending only on `lunar`, so the advertised `#[derive(Bundle)]` breaks the API seal and fails to compile for sealed games; it is also arbitrarily gated behind the `3d` feature so default (2d) games do not get it at all.

## api-05 — docs/01-setup.md:75

- **impact:** high
- **effort:** S
- **verdict:** confirmed (survived refute-by-default verification)

docs/01-setup.md claims '`lunar_app!` handles all four cases automatically based on the compile target and enabled features', but the macro only expands to the native 2d `bootstrap` call and does not even exist on wasm (its module is cfg'd out), so a doc-following wasm or 3d game gets a compile error or the wrong bootstrap.

## api-06 — docs/01-setup.md:9

- **impact:** high
- **effort:** S
- **verdict:** confirmed (survived refute-by-default verification)

The documented first-step dependency line `lunar = { version = "1", features = ["full"] }` cannot resolve to this engine: the facade is published on crates.io as `lunar-engine` (1.0.0), while the crates.io name `lunar` is taken by an unrelated 0.1.0 crate — so the snippet as written fails version resolution outright (no `lunar` 1.x exists). The same broken snippet appears in docs/plugins.md:15 and docs/3d/setup.md:9,11. Fix: add `package = "lunar-engine"` to all four doc snippets (renaming the package to `lunar` is not an option — the name is taken). Migration note: `lunar = { version = "1", ... }` -> `lunar = { version = "1", package = "lunar-engine", ... }`.

## api-07 — crates/lunar-macros/src/lib.rs:398

- **impact:** medium
- **effort:** M
- **verdict:** confirmed (survived refute-by-default verification)

The sealed `#[derive(Component)]` wrapper accepts no helper attributes and hardcodes Table/Mutable, so bevy_ecs 0.18 capabilities games may reasonably want — required components (`#[require(...)]`), relationships, hooks, sparse-set storage, and immutable components — are absent from the sealed surface: any attempt fails with 'cannot find attribute', and the escape hatch the crate docs prescribe (bevy's raw derive via `lunar::__bevy_ecs`) does not compile under the seal because its emitted paths resolve against the game's Cargo.toml, forcing a direct bevy_ecs dependency the api_seal contract forbids.

## api-08 — crates/lunar/Cargo.toml:13

- **impact:** medium
- **effort:** L
- **verdict:** confirmed (survived refute-by-default verification)

The facade's feature design promises headless/text games ('enable any combination, or neither for headless/text games') but no-features builds still compile the full windowed stack: lunar-render, lunar-input, lunar-assets, wgpu (vulkan+gles), and sdl3 (build-from-source-static, requiring a C toolchain) are all unconditional — both as facade deps and inside lunar-render/lunar-input themselves — and the only native bootstrap always opens an SDL window. Runtime headless loops work, and release fat-LTO strips most unused render code from the final binary, so the unconditional cost is compile time, toolchain requirements, and dependency surface rather than release binary size. Separately, the lib.rs:48 feature table is factually wrong: '2d' does not gate 'sprite/text rendering' (that lives in unconditional lunar-render); it gates only lunar-2d collision/animation/transform-propagation. Fix: introduce a default-on 'render' feature gating the lunar-render dep (and sdl3 in lunar-input) with '2d'/'3d' requiring it (migration: headless games set default-features = false; windowed games unaffected), or narrow the comment and correct the doc table to stop promising a render-free build.

## api-09 — crates/lunar/src/lib.rs:78

- **impact:** medium
- **effort:** S
- **verdict:** confirmed (survived refute-by-default verification)

The facade root re-export list (crates/lunar/src/lib.rs:78-92) omits `lunar_lightmap` and `lunar_gamedata` even though both are deps feeding the facade prelude (prelude.rs:84 globs the lightmap prelude; prelude.rs:93 re-exports the four gamedata root types). The lightmap crate's documented crate-root escape hatch (`LightmapBaker`, `BakeResult`, `BakeDirectional` — a load-time, game-facing API) is unreachable through the facade, and docs/3d/lighting.md:122 directs users to `lunar::lunar_lightmap`, a path that does not compile — forcing a direct `lunar-lightmap` dependency, which tests/api_seal/Cargo.toml explicitly defines as an abstraction-boundary regression. The gamedata omission is consistency-only (its whole public surface is already in the prelude) but closes the same class of gap. Fix: add `#[cfg(feature = "3d")] pub use lunar_lightmap;` and `pub use lunar_gamedata;`, matching the precedent of commit 23d55f1 which fixed the identical gap for lunar-bsp. Additive, no migration needed.

## api-10 — crates/lunar/src/prelude.rs:71

- **impact:** medium
- **effort:** S
- **verdict:** confirmed (survived refute-by-default verification)

The `audio` feature is the only dimension feature that contributes nothing to the prelude: lunar-audio has no `prelude` module and `AudioPlayer`/`AudioPlugin`/`PlaybackOptions`/`AudioSource` must be imported via `lunar::lunar_audio::…`, while the equivalent 2d/3d plugin types (`Plugin2d`, `Plugin3d`, `RenderPlugin3d`) are all in the prelude — an inconsistent tier treatment the audio docs then have to teach around.

## api-11 — crates/lunar/src/window_host.rs:33

- **impact:** medium
- **effort:** S
- **verdict:** confirmed (survived refute-by-default verification)

`WindowHost` is public facade API explicitly documented for custom native loops, but its constructor and accessor require `sdl3::video::Window`/`sdl3::mouse::MouseUtil` while the facade re-exports neither `sdl3`, `raw-window-handle`, nor `wgpu` — and the input side (`lunar_input::process_events`, `SdlGamepadProvider::new`) and `RenderEngine::from_surface` equally require sdl3/wgpu types, while the tuned `engine_wgpu_instance()` is pub(crate). The documented custom-loop story is therefore impossible against the facade alone. Fix (additive, S): `pub use sdl3; pub use raw_window_handle;` behind the existing non-wasm cfg plus `pub use wgpu;`, and make `engine_wgpu_instance` public (or wrap window+surface+instance creation in a facade helper so the unsafe bootstrap.rs:86-100 block need not be replicated).

## api-12 — docs/02-ecs.md:232

- **impact:** medium
- **effort:** S
- **verdict:** confirmed (survived refute-by-default verification)

The docs teach games to import from `lunar_core` directly (`use lunar_core::Parent;`, `use lunar_core::{resolutions_for_aspect, STANDARD_RESOLUTIONS};`), which requires adding lunar-core as a direct dependency and violates the facade contract; the root cause is that the documented hierarchy workflow types (Parent, Children, LocalTransform, WorldTransform) and settings-menu types (AvailableResolutions, DisplayResolution) are absent from the facade prelude.

## api-13 — Cargo.toml:118

- **impact:** low
- **effort:** S
- **verdict:** confirmed (survived refute-by-default verification)

The root `lunar-game` template manifest contradicts its own "`lunar` is the only dependency a real game needs" comment (Cargo.toml:114-116) and the facade's seal doctrine (crates/lunar/src/lib.rs:63): `log` (Cargo.toml:120) is completely unused and can simply be deleted, and `bevy_ecs` (Cargo.toml:119) is unused by any code target — the lunar-macros wrapped derives route through `::lunar::__bevy_ecs` exactly so game crates never declare it — but it is NOT deletable as-is because the `debug` feature at Cargo.toml:73 (`debug = ["bevy_ecs/debug"]`) references it; deleting the dep line alone hard-errors the whole workspace manifest. Fix: rewrite the feature to forward through the facade, which already exposes it (`debug = ["lunar/debug"]`, see crates/lunar/Cargo.toml), then delete both dep lines. Migration note: template consumers with `debug = ["bevy_ecs/debug"]` + `bevy_ecs = …` in their game Cargo.toml -> `debug = ["lunar/debug"]`, drop the `bevy_ecs` and `log` dependency lines.

## api-14 — crates/lunar/src/lib.rs:162

- **impact:** low
- **effort:** S
- **verdict:** confirmed (survived refute-by-default verification)

`GameComponent` and `GameResource` are dead API: no engine code bounds on, implements, or otherwise references them anywhere in the workspace, so implementing them (as lib.rs and docs/prelude.md instruct) has zero effect while occupying two prelude slots and implying a registration mechanism that does not exist.
