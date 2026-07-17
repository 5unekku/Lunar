# architecture & separation of concerns audit

date: 2026-07-16
concern: architecture and separation of concerns

method: multi-agent fan-out (one finder agent per lens), location dedup, then every
surviving finding was handed to an independent adversarial verifier whose default was
to refute it — only findings that held up under the verifier's own reading of the code
are listed below. read-only audit; no source was modified. this doc was transcribed by
the orchestrator from the verified finding set after the workflow's writer stage was
cut off by a subagent model-credit limit — the findings and verdicts are the agents', unedited.

scope: workspace crate graph and layering, lunar-render-3d internal cohesion, 2d/3d duplication, module organization, and the plugin/ecs composition model.

**13 confirmed findings** (3 high, 6 medium, 4 low).

discovery stats: 69 raw findings from 5 lenses (+0 gap follow-ups), 69 after dedup, 13 confirmed, 0 refuted.

every finding carries: id, location (file:line), impact, effort (S/M/L), and the
verified claim (verifier-corrected wording where the skeptic adjusted it). ids are
assigned in severity order and are stable references for the phase-2 backlog synthesis.

---

## arch-01 — crates/lunar-render-3d/src/frame.rs:1907-1917

- **impact:** high
- **effort:** S
- **verdict:** confirmed (survived refute-by-default verification)

Installing a ShadowProvider hook silently breaks opaque rendering on Mid/High tier: the hook branch (frame.rs:1907-1917) replaces the entire record_shadows call, but the main z-prepass — the only pass that clears and populates depth_view on those tiers (native passes.rs:1731, wasm passes.rs:1978) — lives inside record_shadows. The opaque pass Loads that depth (passes.rs:306-310) and its pipeline does not write depth on Mid/High (config.rs:886, init.rs:1103), so with a hook installed, opaque geometry is depth-tested (LessEqual) against a zero-initialized buffer on first use (rejecting nearly all fragments) and stale/partial depth on later frames. ShadowCtx cannot compensate — it exposes neither depth_view nor the prepass pipeline — and the hooks.rs doc example itself triggers the breakage. Additionally the ShadowCtx doc (hooks.rs:37) promises a 27-slice atlas (3 cascades + 4 point lights * 6 faces) but the passed texture has only NUM_CASCADES=3 layers (init.rs:520-525); point shadows live in the separate, unexposed point_shadow_tex. Structural fix: hoist z-prepass recording out of record_shadows into its own unconditionally-called method, and correct or narrow the ShadowCtx contract doc.

## arch-02 — crates/lunar-render-3d/src/lib.rs:1472-2115

- **impact:** high
- **effort:** L
- **verdict:** confirmed (survived refute-by-default verification)

RenderEngine3d is a 399-field god struct (lib.rs:1474-2119) and all eight sibling modules (frame, cull, passes, post, resources, config, init, mesh) are `impl RenderEngine3d` blocks that can read and mutate its private fields — Rust module privacy provides no enforcement, though current access is convention-localized (e.g. bloom_*/gtao_* touched only from post.rs and config.rs). Apart from the pass-ordering RenderGraph (render_graph.rs, the crate's one unit-tested internal boundary, which owns no GPU resources) and a few small value types (GpuMesh, TerrainGpu, FrameQueries), every feature's GPU state (~30 feature prefixes: bloom, gtao, staa, ssr, fog, hzb, lod, cull, water, decals, terrain, particles, bindless, atlas, shadows, reflections, detail sprites, ...) lives flat on one struct constructed by a single ~3900-line init_with_adapter ending in one giant Self literal (init.rs:3746). Restructuring into per-feature owned structs (fields + new()/resize()/record()) with RenderEngine3d as composition root buys per-feature unit testability, enforced ownership, and is the prerequisite for shrinking init_with_adapter/resize/rebuild_msaa_pipelines; costs a large mechanical refactor plus deliberate design for shared targets and cross-feature reads (hzb_* is consumed by cull, frame, and resources). Sequences first: unblocks the init/resize/msaa findings.

## arch-03 — crates/lunar/Cargo.toml:13-16,34 and crates/lunar/src/lib.rs:90,141-143

- **impact:** high
- **effort:** M
- **verdict:** confirmed (survived refute-by-default verification)

The facade's `2d` feature does not gate the 2D render stack: `2d` enables only lunar-2d (1.1k lines of collision/sprite-animation), while lunar-render (5k-line wgpu 2D renderer + cosmic-text + guillotiere + lunar-atlas), the 2d bootstrap/bootstrap_wasm, and the unconditional lunar_render re-exports (lib.rs:90,141-143; prelude.rs:95-99) compile regardless — so the documented 'enable any combination, or neither for headless/text games' contract is false. The 3d path never uses lunar-render, so a 3d-only build compiles the entire 2D renderer for nothing, and a headless build compiles it plus wgpu and sdl3. Fix scope: `dep:lunar-render` + cfg-gating the re-exports and 2d bootstraps, PLUS gating the facade's own direct wgpu/sdl3 deps (Cargo.toml:43,46-49) and relocating engine_wgpu_instance out of bootstrap.rs (bootstrap_3d.rs:107 reuses it) — the wgpu/sdl3 drop benefits headless builds only. Buys real headless/3d-only graphs and makes a facade-only dep viable for hot-reload behavior dylibs (fixture at crates/lunar-plugin-loader/fixture today deps lunar-core directly, bypassing the facade). Costs a breaking change for 3d-only/headless users who touched 2D types. Sequencing: unblocks the Behavior-derive audience-split finding.

## arch-04 — bindings/c/src/lib.rs:42 and bindings/c/Cargo.toml (lunar-render-3d dep)

- **impact:** medium
- **effort:** M
- **verdict:** confirmed (survived refute-by-default verification)

lunar-ffi hard-depends on the entire 17k-line lunar-render-3d (plus its naga WGSL-to-SPIR-V build script, lunar-bsp, lunar-lightmap) solely for two resource types, so any game embedding C#/native scripting via lunar-plugin-loader — including a 2D-only game — compiles the full 3D renderer stack.

## arch-05 — Cargo.toml:157-162 and crates/lunar-3d/src/simd.rs

- **impact:** medium
- **effort:** S
- **verdict:** confirmed (survived refute-by-default verification)

The AVX2 SIMD cull kernel living inside lunar-3d forces the entire ~5.9k-line crate onto the LLVM backend in dev builds (profile.dev.package.lunar-3d override), sacrificing cranelift iteration for a high-churn crate (second-most-edited in the workspace: components, systems, collision, scene format) when only the 545-line simd.rs needs LLVM. Extracting simd.rs into a leaf crate (or into lunar-math, which it already depends on) and pinning the llvm override there restores cranelift for lunar-3d's own incremental rebuilds — dependents are unaffected either way since the override never applied to them. Costs: one micro-crate + re-export shim, plus relocating the Frustum-consistency test (test module depends on lunar_3d::visibility::Frustum) back into lunar-3d or rewriting it against raw planes.

## arch-06 — crates/lunar-macros/Cargo.toml:19-20 and crates/lunar-macros/src/lib.rs:46-94,224-234

- **impact:** medium
- **effort:** M
- **verdict:** confirmed (survived refute-by-default verification)

The proc-macro crate mixes two unrelated concerns — ECS derive wrappers and a compile-time image-asset pipeline — so every consumer of the derives host-compiles the `image` decoders + zstd, and `texture!` performs side-effectful, untracked I/O during macro expansion (writes a `.lunar/` cache into the consumer's source dir; edits to the source image never trigger recompilation because rustc only tracks the emitted `include_bytes!` cache file).

## arch-07 — crates/lunar-render-3d/src/init.rs:278-4178

- **impact:** medium
- **effort:** L
- **verdict:** confirmed (survived refute-by-default verification)

init_with_adapter is a single ~3,900-line constructor that inline-builds every shader, BGL, pipeline, texture, and buffer for ~29 features and ends in a ~430-line struct literal (init.rs:3746-4178), making initialization unreviewable and forcing every new feature to edit one giant function.

## arch-08 — crates/lunar-render-3d/src/render_graph.rs:1-36

- **impact:** medium
- **effort:** S
- **verdict:** confirmed (survived refute-by-default verification)

render_graph.rs (316 lines + 9 tests) is a dead abstraction: its docs and the RenderEngine3d field comment claim it drives pass execution order, but its only consumer is a debug-only trace log; the real pass order is hardcoded in render_frame.

## arch-09 — crates/lunar-render/src/lib.rs:503-541 vs crates/lunar-render-3d/src/init.rs:15-130 (and three duplicated manifest backend blocks)

- **impact:** medium
- **effort:** L
- **verdict:** confirmed (survived refute-by-default verification)

GPU bring-up policy is split across four uncoordinated sites: instance creation (with engine-tuned validation flags) lives in the facade's bootstrap.rs engine_wgpu_instance(); adapter/device request is implemented independently in lunar-render (three flows, empty features/default limits) and lunar-render-3d (three flows plus a rich negotiate_device handling features, bindless limits, and passthrough); and surface configuration is duplicated with divergent policy (only the 3d path has the desired_maximum_frame_latency=3 Wayland fix). The per-platform wgpu backend feature blocks (vulkan/gles, dx12, metal, webgpu) are hand-mirrored across three manifests — a mirroring lunar-render's own comment concedes — and cosmetic drift already exists (lunar-render's redundant 'web' extra, present since the mirroring was introduced in 29caebb). Note the triplication is partly forced by Cargo's model (standalone-testable crates must declare the backend features they use), so the fix is a small lunar-gpu crate that owns the wgpu dependency + backend feature declarations (re-exporting wgpu) and the instance/adapter/device init policy, parameterized per renderer since the 2d and 3d device policies intentionally differ. Buys: one sync point for backend features, one auditable device/surface-init policy, and the structural precondition for ever sharing a device between the 2d and 3d engines (constructors today take only &Instance and own their device, so compositing is impossible by construction — a latent constraint, as no bootstrap currently instantiates both). Costs: touches both renderers' init paths and every manifest that names wgpu; L-sized. Sequencing: land before any perf work that touches device features/limits.

## arch-10 — crates/lunar-macros/src/lib.rs:272-273,314 vs crates/lunar/src/lib.rs:67-76

- **impact:** low
- **effort:** M
- **verdict:** confirmed (survived refute-by-default verification)

lunar-macros targets two root paths — Component/Resource/Event/Message emit ::lunar::__bevy_ecs::… (requiring the facade) while Behavior emits ::lunar_core::behavior::… (requiring a direct lunar-core dep) — and the lunar facade does not re-export the Behavior derive at all (crates/lunar/src/lib.rs:76). The gap cuts both ways: hot-reload behavior crates cannot follow the 'depend only on lunar' convention (fixture proves it: lunar-core + lunar-macros), and in-process game code depending only on lunar cannot use derive(Behavior) despite BehaviorRegistry documenting in-process registration as intended. The split is documented, intentional design (derive rustdoc; lunar-macros/Cargo.toml:22-25 comment), expressed only as prose — not machine-checkable. Two convergence options: (a) after facade feature-gating, emit ::lunar::lunar_core::behavior::… and add a facade re-export of Behavior — but this enlarges the hot-reload dylib's dependency cone (facade unconditionally pulls lunar-render/wgpu/sdl3 today), slowing the rebuild loop hot reload exists for; (b) cheaper and independent of facade work: have lunar-core re-export the Behavior derive (no cargo cycle; lunar-macros holds lunar-core only as dev-dep), giving behavior crates a single-dep story. Impact remains low: the only current rust consumer of derive(Behavior) is the ABI-fragile, test-gated dylib path (lunar-plugin-loader/src/lib.rs:432-439, pending C-ABI shim); production behaviors are C# plugins that bypass rust derives.

## arch-11 — crates/lunar-plugin-loader/Cargo.toml:2 vs crates/lunar/src/lib.rs:53-55

- **impact:** low
- **effort:** S
- **verdict:** confirmed (survived refute-by-default verification)

The crate name lunar-plugin-loader collides with the documented user-facing `lunar-plugin-*` tier (README.md:77-83 and docs/plugins.md list ~15 external add_plugin crates by that exact pattern), but the crate is the in-repo C#/native scripting host: it dlopens behavior dylibs via `lunar_plugin_init` (NativeAOT) or CoreCLR/hostfxr — an unrelated 'plugin' concept — and the name further invites the misreading that it dynamically loads the lunar-plugins workspace crates, which are actually static Cargo dependencies. Note it IS consumed via add_plugin (CsPlugin: GamePlugin), so the mismatch is tier/workspace membership and terminology, not mechanism; and since it is publish = false no Cargo/registry collision can materialize — the harm is reader confusion in the engine repo (README's crate table never mentions the loader) plus any future glob-based tooling or publishing automation over the lunar-plugin-* pattern. Fix: rename (e.g. lunar-scripting-host or lunar-behavior-host) while it is path-only and unpublished. Buys: unambiguous tier naming and removes a false 'loads the plugin crates' reading. Costs: churn in root Cargo.toml (member list, coreclr feature forward, dev-dep), examples/platform_demo_cs, tests/cross_compile.rs exclude list, the fixture path, and doc comments in bindings/c — xtask is untouched. Impact low, effort S.

## arch-12 — crates/lunar-render-3d/Cargo.toml:26 and crates/lunar-render-3d/src/lib.rs:132-160

- **impact:** low
- **effort:** S
- **verdict:** confirmed (survived refute-by-default verification)

wgpu's `spirv` feature is enabled unconditionally in lunar-render-3d — including for wasm32 — but its only consumers (ShaderSource::SpirV and create_shader_module_passthrough in make_shader!) compile only under cfg(all(not(debug_assertions), not(wasm32))). Because naga is an optional wgpu dependency and `spirv = ["naga/spv-in", ...]` is its sole activator on the webgpu-only wasm graph (wgsl and webgpu only reach naga via ?-gated edges through the absent wgpu-core), moving "spirv" into the existing cfg(not(target_arch = "wasm32")) wgpu block removes the entire naga crate — not just the spv-in front-end — from the CI wasm32 lunar-game build, and matches the target-block convention already used by the sibling lunar and lunar-render crates. Zero native cost: spirv stays enabled for all non-wasm targets.

## arch-13 — crates/lunar/src/bootstrap.rs:40-143 and crates/lunar/src/bootstrap_3d.rs:41-182 (plus bootstrap_wasm.rs, bootstrap_wasm_3d.rs)

- **impact:** low
- **effort:** M
- **verdict:** confirmed (survived refute-by-default verification)

The lunar facade crate, self-described as pure re-exports, also hosts all platform bring-up (701 lines across 4 bootstraps + WindowHost), and the two native bootstraps duplicate ~70 lines verbatim (SDL init, display-mode enumeration, window build, unsafe surface creation, event-loop wiring, teardown) while the two wasm bootstraps duplicate ~30 lines of RAF/canvas scaffolding. Extracting a lunar-platform crate is not the right remedy — bootstrap must construct the render engines and register every built-in plugin, so any crate hosting it reproduces the facade's dependency graph and feature gates one level down, and games rebuild on platform changes either way. Proposal instead: extract private shared helpers inside the facade (native_bring_up returning sdl/video/window/surface/resolutions/instance; a shared wasm RAF-loop driver), following the existing WindowHost/engine_wgpu_instance pattern. Buys: one copy of SDL and RAF wiring, halves the surface for platform bugfixes (e.g. the 17c38c8 drop-order fix had to be applied per-bootstrap). Costs: S refactor, no public-surface or dep-graph change. Effort: S (down from M). Impact: low.
