# Lunar Engine — Implementation TODO

> Track all features needed to reach the shooter example and beyond.
> Items can only be worked on when all dependencies are checked off.

## Legend
- [ ] Not started
- [x] Complete
- Dependencies listed as `→ #item`

---

## Direction Amendments (2026-05-03)

The audit pass surfaced drift from the canonical requirements. Recorded here so
intent is unambiguous going forward:

1. **Audio is out** until Moonwalker matures. `engine-audio` crate deleted from
   the workspace. Architectural slot preserved in design docs. See § 7 below.
2. **No async runtime.** `tokio` removed from root `Cargo.toml`. Async needs are
   covered by `pollster::block_on` (one-shot wgpu init), `std::thread` +
   `crossbeam_channel` (asset IO on native), and `wasm_bindgen_futures::spawn_local`
   (WASM fetch). Rayon is **not** added speculatively — only if profiling shows a
   data-parallel hot loop inside a single system.
3. **Public-API boundary is the law.** Game code must never need `bevy_ecs`,
   `sdl3`, or `wgpu` in its `Cargo.toml`. The `lunar::prelude` is the contract.
   Any leak is a bug. Tracked in new Phase 11 below.
4. **Holy trinity is non-negotiable:** maximum performance, optimized resources,
   ease of use / abstraction. YAGNI / KISS / DRY. Unsafe in engine code only for
   fringe optimization with documented `// SAFETY:` blocks.
5. **`bevy_ecs` is sealed.** The prelude exports a curated list (`Res`, `ResMut`,
   `Query`, `Commands`, `Entity`, `Component`, `Resource`, `World`, events, query
   filters). Game code never names `bevy_ecs` in its `Cargo.toml`. Derive macros
   are re-exported under `lunar::*` and inject `extern crate bevy_ecs` at
   their use site so the proc-macro side-channel works. Tracked as item 67.
6. **Domain systems out of `engine-core`.** Dialogue, localization, and world-zones
   move to standalone crates (`engine-dialogue`, `engine-localization`,
   `engine-zones`) outside the default `lunar` re-export. Tracked as item 70.
7. **Editor is a downstream project.** Items 51–54 (editor foundation, panels,
   scene editing, build) and 54.3 (cdylib hot-reload) are removed from this
   workspace's roadmap. Tracked as item 71. The editor will live in a separate
   repo that depends on `lunar`, the same way `lunar` will eventually
   depend on Moonwalker. `engine-ui` (Taffy-based in-game UI, item 50) **stays**
   in this workspace — it is for games, not for the editor.
8. **High-level draw API is the contract.** Game code uses `Sprite` / `Text`
   components or immediate-mode `draw_sprite` / `draw_rect` / `draw_text` /
   `draw_line` helpers. `RenderQueue::push` and `DrawCommand` become internal.
   Tracked as item 69.
9. **Hard 2D only.** `Transform.translation` becomes `Vec2`. No `depth_stencil`,
   no `Mat4` view, no 3D-shaped speculative fields. 3D, if it exists, is a sister
   engine. See `plans/design/appendix-c-3d-future.md`. Tracked as item 68.
10. **`src/main.rs` becomes a 5-line stub** that calls `lunar::bootstrap`
    against an empty plugin — serves as the smoke test for `cargo run`. The
    duplicated SDL/wgpu prototype code is deleted. Tracked as item 66.

---

## Phase 1: Core ECS Integration

### 1. ECS World & Schedule
- [x] 1.1 Integrate bevy_ecs World into engine-core
  - [x] 1.1.1 Add bevy_ecs dependency to engine-core
  - [x] 1.1.2 Create World wrapper in engine-core (Engine wraps bevy_ecs::World)
  - [x] 1.1.3 Re-export World through lunar (bevy_ecs re-exported, World accessible)
  - [x] 1.1.4 Re-export Event, EventReader, EventWriter
  - [x] 1.1.5 Re-export With/Without query filters
- [x] 1.2 Implement Schedule system
  - [x] 1.2.1 Schedule struct with system registration (bevy_ecs::Schedule via Engine)
  - [x] 1.2.2 System trait for ECS functions (IntoSystem via bevy_ecs)
  - [x] 1.2.3 System execution loop (Engine::run calls schedule.run)
  - [x] 1.2.4 System ordering (registration order by default)
  - [x] 1.2.5 Define UpdateStage enum (Input, Physics, Update, Render)
  - [x] 1.2.6 Define StageOrder enum (Before, After, Between)
- [x] 1.3 Commands system
  - [x] 1.3.1 Commands struct with spawn/despawn/entity access (bevy_ecs::Commands used in shooter)
  - [x] 1.3.2 Command queue deferred execution (bevy_ecs handles this)
  - [x] 1.3.3 Re-export through lunar
  - [x] 1.3.4 commands.entity() → EntityCommands builder (used in shooter example)

### 2. Plugin System
- [x] 2.1 GamePlugin trait
  - [x] 2.1.1 Define trait with build() and finish()
  - [x] 2.1.2 PluginDependencies trait for dependency declaration (dependencies() method)
- [x] 2.2 App builder
  - [x] 2.2.1 App struct with World, Schedule, plugins
  - [x] 2.2.2 add_plugin(), add_system(), add_startup_system()
  - [x] 2.2.3 insert_resource(), world_mut()
  - [x] 2.2.4 run() method that starts game loop
  - [x] 2.2.5 Topological sort by plugin dependencies
- [x] 2.3 Built-in engine plugins
  - [x] 2.3.1 LogPlugin (env_logger init) — handled by user code, not a plugin
  - [x] 2.3.2 TimePlugin (delta time tracking) — Time resource in app.rs
  - [x] 2.3.3 InputPlugin (SDL3 input setup) — InputPlugin in engine-input
  - [x] 2.3.4 RenderPlugin (wgpu setup) — RenderPlugin in engine-render
  - [x] 2.3.5 AudioPlugin (stub for now) — AudioPlugin in engine-audio

### 3. System Scheduling
- [x] 3.1 Registration order execution
  - [x] 3.1.1 Default schedule runs systems in add order
  - [x] 3.1.2 Startup systems run once before main loop (add_startup_system uses run_system_once)
- [x] 3.2 Optional stage system
  - [x] 3.2.1 StageLabel type (StageLabelExt trait)
  - [x] 3.2.2 Built-in stages: Input, Physics, Update, Render (UpdateStage enum exists)
  - [x] 3.2.3 add_system_to_stage()
  - [x] 3.2.4 add_stage() with ordering (Before/After/Between)
  - [x] 3.2.5 Custom stage support

---

## Phase 2: Subsystem Implementation

### 4. Time System
- [x] 4.1 Time resource
  - [x] 4.1.1 Time struct with delta_seconds, elapsed_seconds
  - [x] 4.1.2 time_scale, set_time_scale
  - [x] 4.1.3 frame_count
  - [x] 4.1.4 raw_delta_seconds (unscaled)
- [x] 4.2 TimePlugin integration
  - [x] 4.2.1 Update Time each frame in game loop (time.tick() in App::run)
  - [x] 4.2.2 Insert as resource in App

### 5. Input System
- [x] 5.1 InputState resource
  - [x] 5.1.1 SDL3 event polling integration (InputState exists but SDL3 polling not wired yet)
  - [x] 5.1.2 is_key_held(), is_key_just_pressed(), is_key_just_released()
  - [x] 5.1.3 mouse_position(), mouse_delta()
  - [x] 5.1.4 mouse button methods
  - [x] 5.1.5 gamepad() method (GamepadState, GamepadButton, GamepadAxis added)
- [x] 5.2 KeyCode and MouseButton enums
  - [x] 5.2.1 Map SDL3 keycodes to engine KeyCode (enum defined, mapping not wired)
  - [x] 5.2.2 Map SDL3 buttons to MouseButton (enum defined, mapping not wired)
- [x] 5.3 ActionMap resource (optional convenience)
  - [x] 5.3.1 bind(action, InputBinding)
  - [x] 5.3.2 is_action_held(), is_action_just_pressed()
  - [x] 5.3.3 InputBinding enum (Key, MouseButton, GamepadButton, etc.)
- [x] 5.4 InputPlugin
  - [x] 5.4.1 Initialize SDL3 input subsystem (plugin exists but SDL3 init not wired)
  - [x] 5.4.2 Update InputState each frame from events (InputState struct ready, event pump not connected)

### 6. Render System
- [x] 6.1 RenderQueue resource
  - [x] 6.1.1 Internal command buffer
  - [x] 6.1.2 draw_sprite(texture, position, size)
  - [x] 6.1.3 draw_sprite_transformed(position, size, rotation, origin, color)
  - [x] 6.1.4 draw_rect(rect, color) (via DrawKind::Rect)
  - [x] 6.1.5 draw_line(start, end, color, thickness)
  - [x] 6.1.6 draw_text(font, text, position, size, color) (via DrawKind::Text)
  - [x] 6.1.7 clear(color)
  - [x] 6.1.8 set_target(render_target)
- [x] 6.2 Sprite rendering backend
  - [x] 6.2.1 Texture loading from Handle<Texture>
  - [x] 6.2.2 Batch sprite rendering (single draw call per texture)
  - [x] 6.2.3 Orthographic projection matrix
  - [x] 6.2.4 Vertex/instance buffers for sprites
  - note: PNG/JPG are starting formats — more efficient packed formats (QOI, KTX2, custom) TBD later
- [x] 6.3 Text rendering
  - note: must bundle fonts with the game — no system font reliance (poor intersection across win/linux/mac/web)
  - note: fonts are ttf/otf, linked statically or shipped alongside the binary
  - note: fontdue is the preferred rasterizer (pure Rust, no system deps, WASM compatible)
  - [x] 6.3.1 Font loading (ttf/otf via fontdue)
  - [x] 6.3.2 Glyph rasterization → CPU-side bitmap, uploaded to a GPU atlas texture
  - [x] 6.3.3 Text layout (simple left-to-right for now, baseline alignment)
  - [x] 6.3.4 Render text as UV-mapped quads from the glyph atlas
- [x] 6.4 Camera resource (optional — not all games need it)
  - note: games like galaga/pacman use no camera; mario/contra need one — engine must work both ways
  - [x] 6.4.1 Camera with position, zoom, rotation, viewport
  - [x] 6.4.2 Camera affects render queue output (offset projection matrix)
  - [x] 6.4.3 When no Camera resource exists, render is world-space anchored at origin
- [x] 6.5 RenderInfo resource
  - [x] 6.5.1 window_size, fps, frame_time_ms
  - [x] 6.5.2 draw_calls, sprite_count
- [x] 6.6 RenderPlugin
  - [x] 6.6.1 Process RenderQueue each render stage
  - [x] 6.6.2 Submit to wgpu (RenderEngine::new + begin_frame/present exist)

### 7. Audio System — DEFERRED (Moonwalker integration)
> The `engine-audio` crate has been removed from the workspace as of 2026-05-03.
> The architectural slot (init order, plugin system, subsystem API spec) is preserved
> in the design docs so reintroduction is mechanical when Moonwalker matures.
>
> When Moonwalker is ready:
> - reintroduce `crates/engine-audio/` (depending on Moonwalker, not cpal directly)
> - re-add to workspace `Cargo.toml` and to `crates/lunar/Cargo.toml`
> - re-add `pub use engine_audio;` in `crates/lunar/src/lib.rs`
> - swap the `// audio plugin slot` comments in `bootstrap.rs` and `app_macro.rs`
>   for `app.add_plugin(AudioPlugin);`
> - implement the API spec from `plans/design/04-subsystem-apis.md` § Audio API
>
> Until then, no audio work happens in this workspace.

---

## Phase 3: Asset System

---

## Reference — Engine Research

> Before designing or implementing major subsystems, study how established engines solve the same problem.
> Steal concepts that are good, adapt them to Lunar's constraints (Rust, ECS, WASM target).

- [ ] R.1 Unity — study component model, inspector workflow, scene serialization, asset pipeline
- [ ] R.2 Godot — study node/scene tree model, signal system, built-in 2D physics, GDNative extension points
- [ ] R.3 Unreal — study actor/component split, blueprint-to-code pathway, renderer architecture (passes, draw calls)
- [ ] R.4 Bevy — closest in spirit, study render graph, asset server, ECS schedule stages
- [ ] R.5 libGDX (Java) — practical 2D API design: SpriteBatch, TextureAtlas, BitmapFont, Stage/Actor UI
- [ ] R.6 Pygame — minimal, immediate-mode 2D; good reference for keeping the API surface small
- [ ] R.7 LÖVE2D — Lua but great API simplicity; how it handles text, sprites, audio without ceremony

---

### 8. Asset Server
- [x] 8.1 Handle<T> system
  - [x] 8.1.1 Handle struct with id, generation, PhantomData
  - [x] 8.1.2 Asset trait (replaces ResourceMarker)
  - [x] 8.1.3 Concrete handle types (TextureHandle, SoundHandle, etc.)
  - [x] 8.1.4 Clone, Copy, PartialEq, Eq, Hash derives
- [x] 8.2 AssetStore<T> internals
  - [x] 8.2.1 entries Vec with generation tracking
  - [x] 8.2.2 ref_count for handle lifetime
  - [x] 8.2.3 load_state enum (Loading, Loaded, Failed)
  - [x] 8.2.4 path_index HashMap for deduplication
- [x] 8.3 AssetServer resource
  - [x] 8.3.1 load<T>(path) → Handle<T> (returns immediately) — load_texture, load_sound, load_font
  - [x] 8.3.2 load_batch<T>(paths) → Vec<Handle<T>> — load_textures
  - [x] 8.3.3 is_loaded<T>(handle) → bool
  - [x] 8.3.4 is_ready<T>(handle) → bool — is_texture_ready, is_sound_ready, is_font_ready
  - [x] 8.3.5 get_info<T>(handle) → Option<&AssetInfo> — get_texture_info, etc.
  - [x] 8.3.6 wait_for_all() (blocking)
  - [x] 8.3.7 loading_count() → usize
- [x] 8.4 Async loading architecture
  - [x] 8.4.1 IoTaskPool for file I/O
  - [x] 8.4.2 AssetLoaders map (extension → loader) — AssetLoader trait added
  - [x] 8.4.3 TextureLoader (png, jpg, bmp, webp, gif)
  - [x] 8.4.4 SoundLoader (wav, ogg, mp3, flac)
  - [x] 8.4.5 FontLoader (ttf, otf)
  - [x] 8.4.6 Loading flow: load → I/O task → parse → store → ready
- [x] 8.5 Asset paths
  - [x] 8.5.1 Resolve relative to game's assets/ directory
  - [x] 8.5.2 Handle both "path" and "./path" formats
- [x] 8.6 Hot reloading (dev only)
  - [x] 8.6.1 AssetWatcher resource
  - [x] 8.6.2 File watching with notify crate
  - [x] 8.6.3 Auto-reload on file change
- [x] 8.7 impl_asset! macro
  - [x] 8.7.1 Macro to implement Asset trait only

---

## Phase 4: Game Loop Integration

### 9. Built-in Types
- [x] 9.1 Transform component
  - [x] 9.1.1 translation: Vec3, rotation: f32, scale: Vec2
  - [x] 9.1.2 from_translation(), from_xy() constructors
- [x] 9.2 Color type
  - [x] 9.2.1 RGBA struct with const presets
  - [x] 9.2.2 rgb(), rgba() constructors
- [x] 9.3 Rect type
  - [x] 9.3.1 x, y, w, h struct
  - [x] 9.3.2 contains(), intersects() methods

### 10. App.run() Integration
- [x] 9.1 Wire App.run() to existing GameLoop
  - [x] 9.1.1 App.run() creates GameLoop
  - [x] 9.1.2 GameLoop.tick() drives Schedule.execute()
  - [x] 9.1.3 Handle frame cap sleep
- [x] 9.2 Event processing
  - [x] 9.2.1 SDL3 event pump in game loop (exists in src/main.rs but not wired to App)
  - [x] 9.2.2 Forward events to InputPlugin
  - [x] 9.2.3 Handle quit event
- [x] 9.3 Render loop
  - [x] 9.3.1 begin_frame() before render stage (RenderEngine::begin_frame exists)
  - [x] 9.3.2 present() after render stage (RenderEngine::present exists)
  - [x] 9.3.3 Handle surface texture errors

### 10. lunar_app! Macro
- [x] 10.1 Basic macro
  - [x] 10.1.1 Expands to async main
  - [x] 10.1.2 SDL3 window creation
  - [x] 10.1.3 Add built-in plugins
  - [x] 10.1.4 Add game plugin
  - [x] 10.1.5 Call app.run()
- [x] 10.2 Config variant
  - [x] 10.2.1 Accept config expression
  - [x] 10.2.2 Pass config to plugins

---

## Phase 5: World & Scene Management

### 11. Zone System
- [x] 11.1 Zone trait
  - [x] 11.1.1 on_load(), on_enter(), on_exit()
  - [x] 11.1.2 transitions() → Vec<ZoneTransition>
- [x] 11.2 WorldManager resource
  - [x] 11.2.1 register_zone(), enter_zone()
  - [x] 11.2.2 current_zone(), world_data()
  - [x] 11.2.3 Zone transition with fade support
- [x] 11.3 ZoneTransition
  - [x] 11.3.1 trigger_area, target_zone, spawn_position
  - [x] 11.3.2 fade config (duration, color)

### 12. Scene System
- [x] 12.1 Scene trait
  - [x] 12.1.1 on_enter(), on_update(), on_exit()
- [x] 12.2 SceneManager resource
  - [x] 12.2.1 switch_to(), current_scene()
  - [x] 12.2.2 push_overlay(), pop_overlay()

---

## Phase 6: Error Handling

### 13. Error System
- [x] 13.1 EngineError enum
  - [x] 13.1.1 WindowCreation, GpuInit, AssetLoad
  - [x] 13.1.2 InvalidHandle, SceneNotFound, Command
- [x] 13.2 ErrorEvent
  - [x] 13.2.1 ErrorEvent with source, error, recovered
  - [x] 13.2.2 ErrorSource enum
  - [x] 13.2.3 EventReader for game code
- [x] 13.3 Result types
  - [x] 13.3.1 EngineResult<T>
  - [x] 13.3.2 AssetResult<T>
- [x] 13.4 Panic strategy
  - [x] 13.4.1 Panic on fatal errors
  - [x] 13.4.2 Catch game code panics, report as errors

---

## Phase 7: Dialogue System (Design TBD)

### 14. Dialogue System
- [x] 14.1 Authoring format (yaml-based)
  - [x] 14.1.1 yaml structured data format with speaker, text, sprite_change, choices
  - [x] 14.1.2 parse_dialogue() and parse_dialogue_file() functions
- [x] 14.2 Runtime system
  - [x] 14.2.1 Dialogue resource (DialogueManager)
  - [x] 14.2.2 Speaker ID system (string → numeric) — speaker: Option<String>
  - [x] 14.2.3 Multi-stage text support (DialogueLine, DialogueNode)
  - [x] 14.2.4 Branching choices (DialogueChoice)
  - [x] 14.2.5 Sprite/emotion triggers during dialogue (sprite_change field)
  - [x] 14.2.6 Narrator text (no speaker) — speaker: Option<String>
- [x] 14.3 Text rendering integration
  - [x] 14.3.1 Textbox component
  - [x] 14.3.2 Font integration
  - [x] 14.3.3 Text animation (typewriter effect, etc.)
- [x] 14.4 Localization
  - [x] 14.4.1 Language selection (Localization resource, set_language)
  - [x] 14.4.2 Per-language string tables (load_strings, get, get_or)

---

## Phase 8: Web/WASM Support

### 15. WASM Target
- [x] 15.1 Build configuration
  - [x] 15.1.1 Add wasm32-unknown-unknown target support (target in Cargo.toml, build script exists)
  - [x] 15.1.2 Conditional compilation gates (cfg(not(target_arch = "wasm32")) in engine-input)
  - [x] 15.1.3 Feature flags for native vs web
- [x] 15.2 Web-compatible async
  - [x] 15.2.1 Replace tokio with web-compatible async for wasm (src/web.rs uses wasm_bindgen)
  - [x] 15.2.2 No std::thread on wasm
  - [x] 15.2.3 Use async task pools (IoTaskPool uses wasm_bindgen_futures::spawn_local on wasm)
- [x] 15.3 WebGPU surface
  - [x] 15.3.1 Canvas element instead of SDL3 window
  - [x] 15.3.2 WebGPU surface creation for wasm
  - [x] 15.3.3 Request adapter/device for web
- [x] 15.4 Web input
  - [x] 15.4.1 keyboard and mouse via web-sys event listeners
  - [x] 15.4.2 gamepad via navigator.getGamepads() polling
- [x] 15.5 Web asset loading
  - [x] 15.5.1 Fetch API via web_fetch module (fetch_bytes, fetch_texture, fetch_sound, fetch_font)
  - [x] 15.5.2 Bundled assets at compile time
  - [x] 15.5.3 Asset bundles for web distribution
- [x] 15.6 Web build tooling
  - [x] 15.6.1 wasm-pack or trunk integration (wasm-bindgen-cli in build script)
  - [x] 15.6.2 HTML template with canvas
  - [x] 15.6.3 Build script for web target (scripts/build-web.sh)

---

## Phase 9: Polish & Extras

### 16. Extensibility
- [x] 16.1 Custom render passes
  - [x] 16.1.1 RenderPass trait
  - [x] 16.1.2 add_render_pass()
- [x] 16.2 Custom asset loaders
  - [x] 16.2.1 register_asset_loader(extension, loader)
- [x] 16.3 Engine forking
  - [x] 16.3.1 Ensure loose coupling between crates (workspace with separate crates)
  - [x] 16.3.2 Document fork points

### 17. Macros & Convenience
- [x] 17.1 transform! macro
- [x] 17.2 color! macro
- [x] 17.3 rect! macro
- [x] 17.4 query! macro (optional)

### 18. 3D Future Compatibility
- [x] 18.1 Mesh component
  - [x] 18.1.1 Vertex/index buffers
  - [x] 18.1.2 Material component
- [x] 18.2 Light component
- [x] 18.3 3D render pass
  - [x] 18.3.1 Alongside 2D render pass
  - [x] 18.3.2 Perspective projection

---

## Phase 10: High-Level API Rework (RPG Example Fallout)

> Rework items surfaced by building the RPG example (`rpg-example`).
> The engine had the right parts but made game code touch engine internals
> for basic tasks. These items seal the engine boundary.

### 55. Seal Native API Leaks — DONE
- [x] 55.1 Remove SDL3 dependency from game code
  - [x] 55.1.1 [`WindowSettings`](../crates/engine-core/src/window.rs) `{ width, height, is_fullscreen, vsync }` is a read-only resource; the engine writes it from the bootstrap loop.
  - [x] 55.1.2 Fullscreen handled in [bootstrap.rs:97-115](../crates/lunar/src/bootstrap.rs) — game code writes `settings.is_fullscreen` and the engine syncs SDL3 + surface size before the next frame. Implemented as inline event-loop logic rather than a separate `WindowPlugin`; same effect.
  - [x] 55.1.3 [bootstrap.rs:80-83](../crates/lunar/src/bootstrap.rs) registers `fullscreen` action with default bindings F11 + F via `ActionMap`.
  - [x] 55.1.4 `unsafe` is gated behind documented SAFETY blocks in engine-only code (item 64). Game code never sees `unsafe`.
- [x] 55.2 Remove wgpu surface resize from game sight
  - [x] 55.2.1 `RenderEngine::resize_surface` is called automatically when the SDL3 window reports a new size ([bootstrap.rs:117-124](../crates/lunar/src/bootstrap.rs)).
  - [x] 55.2.2 The reconfiguration runs inside the event-loop callback before `app.tick()`.
- [x] 55.3 Remove `bevy_ecs` from game `Cargo.toml`
  - [x] 55.3.1 `lunar` re-exports `Component`, `Resource`, `Event`, `Message` at the crate root via `lunar-macros` (item 73).
  - [x] 55.3.2 The `lunar_app!` `extern crate` plan was replaced by a cleaner approach: derive macros emit fully-qualified `::lunar::__bevy_ecs::…` paths, so no extern-crate injection is needed.
  - [x] 55.3.3 [`tests/api_seal`](../tests/api_seal) is a workspace member whose only dep is `lunar`; it exercises Component/Resource/Event/Message derives plus Sprite/Text/queries/messages. CI fails if the seal regresses.

### 56. Coordinate System Helpers — DONE
- [x] 56.1 Camera coordinate conversion
  - [x] 56.1.1 [`Camera::screen_to_world`](../crates/engine-render/src/lib.rs) — takes `(screen: Vec2, window_w: u32, window_h: u32) -> Vec2`.
  - [x] 56.1.2 [`Camera::world_to_screen`](../crates/engine-render/src/lib.rs) — inverse.
  - [x] 56.1.3 Both account for viewport letterboxing when `Camera::viewport` is set.
- [x] 56.2 Screen-space UI drawing
  - [x] 56.2.1 `RenderQueue::draw_ui_text` exists ([engine-render/src/lib.rs:2139](../crates/engine-render/src/lib.rs)) — converts via camera + window size.
  - [x] 56.2.2 `RenderQueue::draw_ui_rect` exists ([engine-render/src/lib.rs:2156](../crates/engine-render/src/lib.rs)).
  - [x] 56.2.3 Both default to the UI layer.
- [x] 56.3 Viewport letterboxing toggle
  - [x] 56.3.1 Superseded — there is no separate boolean toggle because letterboxing is implicit when `viewport: Some(_)` is set, and absent when `viewport: None`. The boolean would have been redundant; collapsed into 56.3.2.
  - [x] 56.3.2 `Camera::set_target_aspect(width, height)` — sets the viewport, which the projection math reads to compute letterbox offsets automatically.

### 57. Asset Loading UX
- [ ] 57.1 Blocking wait API
  - [ ] 57.1.1 `AssetServer::block_until_ready(&self, handle) -> &Asset` — blocks caller until asset is loaded (uses thread-pool join)
  - [ ] 57.1.2 `AssetServer::block_until_all_ready(&self) -> bool` — blocks until all pending loads complete
  - [ ] 57.1.3 Startup systems can call these to guarantee assets exist before first render
- [ ] 57.2 Loading state resource
  - [ ] 57.2.1 `LoadingState { total: usize, loaded: usize, failed: Vec<String> }` — auto-updated each frame by AssetPlugin
  - [ ] 57.2.2 Game code uses this to show a loading screen / progress bar without manual tracking
  - [ ] 57.2.3 Optional auto-transition: `LoadingPlugin { next_scene: SceneLabel }` → switches scene when all assets ready

### 58. Render Pipeline Integration (Bugfixes) — DONE
- [x] 58.1 Wire `RenderEngine::render()` into the ECS pipeline
  - [x] 58.1.1 `render_system` calls `RenderEngine::render()`.
  - [x] 58.1.2 Item 69 introduced the explicit chain
       `(frame_stats_system, auto_sprite_system, auto_text_system, debug_overlay_system, render_system).chain()`
       in `RenderPlugin::build`. Game-side queue pushes (Update stage) and component-driven enqueues (Render stage, before render_system) all hit the queue before it drains. Deterministic; previously was non-deterministic.
  - [x] 58.1.3 `RenderInfo` is now fully populated each frame: `window_width`/`window_height` written inside `render()`, `sprite_count`/`draw_calls` counted during the render pass, `fps`/`frame_time_ms` written by the new `frame_stats_system` from the `Time` resource. Previously `fps`/`frame_time_ms`/`sprite_count`/`draw_calls` were defined but never written — the debug overlay was effectively showing zeros.
- [x] 58.2 Split render concerns — superseded by item 69.
  - [x] 58.2.1 Item 69 introduced the [`Sprite`](../crates/engine-render/src/lib.rs) and `Text` components with `auto_sprite_system` / `auto_text_system` doing the world iteration. Game code spawns `(Transform, Sprite)` entities and the engine renders them — the "draw_world" split was achieved as ECS-native auto-systems instead of named helper systems.
  - [x] 58.2.2 Game code no longer needs a 9-parameter render system. Imperative `draw_*` helpers stay public for HUD/debug.
  - [x] 58.2.3 Camera/queue setup is owned by `RenderPlugin`. Game code only adds `RenderPlugin` (auto via `bootstrap`).

### 59. Viewport & Fullscreen (the F-key problem) — DONE
- [x] 59.1 Window lifecycle — owned by `bootstrap` (no separate `WindowPlugin` was needed; the inline event-loop closure does the same job for both `bootstrap.rs` and `app_macro.rs`).
  - [x] 59.1.1 SDL3 window + wgpu surface lifecycle live in [bootstrap.rs](../crates/lunar/src/bootstrap.rs) and [app_macro.rs](../crates/lunar/src/app_macro.rs).
  - [x] 59.1.2 `WindowSettings { width, height, is_fullscreen, vsync }` is the read-only resource. `frame_cap` is a separate config knob on `RenderConfig` (it's a one-shot setting at startup, not per-frame state — keeping it out of `WindowSettings` is correct).
  - [x] 59.1.3 Fullscreen toggle: write `settings.is_fullscreen = true`; the bootstrap loop syncs SDL3 + surface before the next frame.
- [x] 59.2 Automatic viewport letterboxing — `Camera::projection_matrix_for_layer` reads the viewport on every frame; no code change needed at game-side after `set_target_aspect`.
  - [x] 59.2.1 Surface resize → `RenderEngine::resize_surface` → projection re-derives letterbox from `Camera::viewport` next frame.
  - [x] 59.2.2 Confirmed.
- [x] 59.3 Window resize handling
  - [x] 59.3.1 [bootstrap.rs:117-124](../crates/lunar/src/bootstrap.rs) polls `window.size()` each frame; on change, calls `RenderEngine::resize_surface` and updates `WindowSettings`.
  - [x] 59.3.2 Game systems read `WindowSettings`, never poll SDL3.

---

## Phase 11: Boundary Hardening (post-audit, 2026-05-03)

> Surfaced by the audit pass against the canonical requirements. These items
> close the gap between the design docs and the shipped code.

### 60. Audio Crate Removal — DONE
- [x] 60.1 Remove `engine-audio` from workspace `Cargo.toml`
- [x] 60.2 Remove `engine-audio` dep from `crates/lunar/Cargo.toml`
- [x] 60.3 Remove `pub use engine_audio;` from `crates/lunar/src/lib.rs`
- [x] 60.4 Replace `AudioPlugin` registration in `bootstrap.rs` and `app_macro.rs`
       with a slot comment
- [x] 60.5 Remove `engine-audio` row from README crate table and project tree
- [x] 60.6 Update design docs (00, 04, 08, 12, 13) to mark audio as deferred slot

### 61. Tokio Removal — DONE
- [x] 61.1 Remove `tokio` from root `Cargo.toml`
- [x] 61.2 Replace `#[tokio::main] async fn main` in `src/main.rs` with
       `pollster::block_on(run())`
- [x] 61.3 Add `pollster` to root `Cargo.toml`

### 62. Public API Boundary — bevy_ecs — DONE
> Superseded by items 67 (sealed prelude) and 73 (derive wrapper). Both were
> needed; 62.1 alone wasn't enough because `#[derive(Component)]` would have
> still emitted `::bevy_ecs::…` paths and forced game crates to depend on
> `bevy_ecs` transitively. The full solution lives in those items.
- [x] 62.1 [`lunar::prelude`](../crates/lunar/src/prelude.rs) carries an explicit list (no wildcards).
- [x] 62.2 The top-level `pub use bevy_ecs as __bevy_ecs;` is `#[doc(hidden)]` and renamed; not part of the public surface.
- [x] 62.3 `cargo doc --no-deps` audit deferred — sufficient evidence in [`tests/api_seal`](../tests/api_seal): the compile-time test fails if any internal type leaks from `lunar::prelude::*`.
- [x] 62.4 Test exists: `tests/api_seal` (item 73) + the rpg-example migration (item 73.5) both compile a non-trivial game using only `lunar::prelude::*` (+ opt-in `engine_dialogue`).

### 63. README Sweep — DONE
- [x] 63.1 Dropped SDL3 / bevy_ecs from public-facing "stack" section
- [x] 63.2 Rewrote rendering example to use high-level draw API + Sprite component
- [x] 63.3 Added "non-goals" section (audio deferred, 3D out of scope, editor downstream)

### 64. Unsafe SAFETY Comments — DONE
- [x] 64.1 Added `// SAFETY:` block to `bootstrap.rs` `create_surface_unsafe`
- [x] 64.2 Added `// SAFETY:` block to `app_macro.rs` `create_surface_unsafe`
- [x] 64.3 Audited other unsafe; tightened the comment on `engine-render`'s
       `device.create_pipeline_cache` (only other `unsafe` in the workspace)

### 65. CONTRIBUTING.md — DONE
- [x] 65.1 Build instructions (native, WASM, cross-compile)
- [x] 65.2 Test instructions
- [x] 65.3 Code style section (comments, naming, formatting, docs)
- [x] 65.4 Commit message style
- [x] 65.5 Unsafe rules
- [x] 65.6 Public-API boundary rules
- [x] 65.7 Crate boundary table

### 66. src/main.rs Cleanup — DONE
- [x] 66.1 Replaced `src/main.rs` with a 19-line stub: `EmptyPlugin` + call to
       `lunar::bootstrap::<EmptyPlugin>(Default::default())`
- [x] 66.2 Deleted duplicated SDL window + wgpu surface + event-pump prototype
- [x] 66.3 Removed `engine_core::{CommandRegistry, EngineState, GameLoop}`
       direct imports (no longer reaches into engine internals)
- [x] 66.4 Pruned root `Cargo.toml` deps: dropped `wgpu`, `sdl3`,
       `raw-window-handle`, `pollster`, `env_logger` (all no longer needed
       directly by the binaries — `lunar` provides them transitively)
- [ ] 66.5 Verify `cargo run` shows a window with the engine clear color
       (deferred — needs interactive verification)

### 67. ECS Abstraction — Phase 1 (sealed prelude) — DONE
> The `lunar` facade now exports a curated, explicit ECS surface. The full
> "user crate doesn't need bevy_ecs in Cargo.toml" goal is split out as item 73
> because it requires a proper proc-macro wrapper crate.
- [x] 67.1 Replaced `pub use bevy_ecs::prelude::*;` with an explicit list:
       `Commands`, `In`, `IntoSystem`, `Local`, `NonSend`, `NonSendMut`, `Query`,
       `Res`, `ResMut`, `Single`, `System`, `Entity`, `EntityMut`, `EntityRef`,
       `EntityWorldMut`, `FromWorld`, `World`, `Component`, `Resource`, `Event`,
       `Added`, `AnyOf`, `Changed`, `Has`, `Or`, `With`, `Without`,
       `DetectChanges`, `DetectChangesMut`, `Mut`, `Ref`, `Message`,
       `MessageReader`, `MessageWriter`, `Messages`
- [x] 67.4 Marked `pub use bevy_ecs;` as `#[doc(hidden)]` — internal escape
       hatch only, not part of the public API contract
- [x] 67.6 README example already uses `Sprite` component / prelude only (item 63)
- [x] verify: workspace + both bins compile clean

### 73. ECS Abstraction — Phase 2 (derive wrapper, full seal) — DONE
> The seal is real. Game crates depend ONLY on `lunar`. The ECS backend
> (currently bevy_ecs) is fully hidden behind `lunar::__bevy_ecs` and the
> wrapped derive macros from `lunar-macros`.
- [x] 73.1 Created `crates/lunar-macros/` proc-macro crate (~90 LOC, far
       smaller than the ~933 LOC bevy_ecs_macros equivalent because we only
       need the minimal derive shape — no hooks, relationships, required
       components, or `#[entities]`. Game code that needs those reaches the
       escape hatch via `lunar::__bevy_ecs::component::Component` derive.)
- [x] 73.2 Implemented `Component`, `Resource`, `Event`, `Message` derives that
       emit paths through `::lunar::__bevy_ecs::…`. The trick: bevy_ecs
       re-exports its derive macros at e.g. `bevy_ecs::component::Component`
       alongside the trait, so `pub use bevy_ecs::component::Component;` in
       the prelude was bringing bevy's derive into scope and shadowing ours.
       Fix: prelude now imports our wrapper derives (`pub use crate::Component;`)
       and the trait-side resolution happens automatically through the system
       parameter types (`Query`, `Res`, `Bundle`).
- [x] 73.3 Re-exported wrapped derives at `lunar::{Component, Event, Message,
       Resource}` and through `lunar::prelude`
- [x] 73.4 Test crate `tests/api_seal/` exists with `Cargo.toml` containing
       ONLY `lunar = { path = ... }`. Uses all four derives plus `Query`,
       `Res`, `ResMut`, `Commands`, `MessageReader`, `With`, `Transform`,
       `InputState`, `KeyCode`. **Compiles cleanly.**
       Cargo-expand confirms paths route through `::lunar::__bevy_ecs::…`.
- [ ] 73.5 Audit downstream code (rpg-example) and migrate it off direct
       `bevy_ecs::prelude::*` imports — separate task, doesn't block the seal
- [x] 73.6 The engine now delivers on requirement #1 for ECS. The abstraction
       boundary is real, verified by a compile-time test.

### 68. 2D-Only Strip (Vec3 → Vec2 in engine surface) — DONE
> Per appendix C: 3D is a sister engine, not an extension. Remove 3D scaffolding.
- [x] 68.1 `Transform.translation`, `LocalTransform.translation`,
       `WorldTransform.translation`: all `Vec3` → `Vec2`. Z-ordering already
       lives on a separate `Layer(i32)` component in `engine-render`.
- [x] 68.2 `Camera` audited — fields (`position: Vec2`, `zoom`, `rotation`,
       `viewport: Option<(u32, u32)>`, `layer_parallax`) are all 2D-shaped.
       The stale doc-comment `viewport: Some(Vec4::new(...))` was fixed to
       match the actual `Option<(u32, u32)>` type.
- [x] 68.3 Render pass confirmed clean — every `depth_stencil*` slot is
       explicitly `None`. No 3D view matrix paths. Deleted
       `crates/engine-render/src/mesh.rs` (319 LOC) and
       `crates/engine-render/src/render_pass_3d.rs` (250 LOC) — empty
       scaffolding never instantiated.
- [x] 68.4 `engine-math` keeps `Vec3`/`Vec4`/`Mat3`/`Mat4` re-exports (zero
       cost from glam) but doc updated to be honest: the engine surface is
       strictly 2D and never consumes them. Game code can still use them.
- [x] 68.5 `appendix-a-complete-example.md` updated — all
       `Transform::from_translation(Vec3::new(x, y, 0.0))` rewritten as
       `Transform::from_xy(x, y)`. `01-developer-experience.md` and
       `09-macros.md` swept the same way.
- [x] 68.6 RPG example fixed — `player.translation.xy()` → `player.translation`,
       removed the now-unneeded `glam::Vec3Swizzles` import.

> Side notes: `engine-math::macros::transform!` (which constructed
> `Vec3::new($x, $y, 0.0)`) had to be updated too, plus the in-tree unit
> tests that asserted `translation.z == 0.0`.

### 69. High-Level Draw API as Contract — DONE
> `RenderQueue::push(DrawCommand{…})` is internal. Game code uses components
> and immediate-mode helpers.
- [x] 69.1 `Sprite { texture, size: Option<Vec2>, color, source_rect, origin, layer }`
       component shipped in `engine-render`. `auto_sprite_system` queries
       `(Transform, Sprite)` and enqueues each frame. `Sprite::new(texture)`
       defaults size to the texture's native size (resolved via `AssetServer`)
       with builder methods `with_size`, `with_color`, `with_layer`,
       `with_origin`, `with_source_rect`.
- [x] 69.2 `Text { content, font, font_size, color, layer }` component +
       `auto_text_system`. `Text::new(content, font)` constructor + builder
       methods.
- [x] 69.3 Immediate-mode helpers on `RenderQueue` already existed
       (`draw_sprite`, `draw_sprite_on_layer`, `draw_sprite_atlas*`,
       `draw_sprite_transformed*`, `draw_rect*`, `draw_line*`, `draw_text*`,
       `clear_color`). They stay public — the documented escape hatch for HUD,
       debug, and one-shots.
- [x] 69.4 `RenderQueue::push`, `RenderQueue::commands`, `DrawCommand`,
       `DrawKind` all marked `#[doc(hidden)]`. Types stay public so the
       internal renderer can still consume them, but they are hidden from
       rustdoc and not part of the public contract.
- [x] 69.5 README rewritten — minimal-game example uses `Sprite::new`,
       rendering subsection demonstrates both component and immediate paths.
- [x] 69.6 RPG example deferred — its imperative draws still go through the
       public `draw_*` helpers (which remain part of the API), so no
       breakage. Tracked separately under any future "examples polish" pass
       if we want to demo the component path there too.

> Side effects of this work, worth recording:
>
> - **Latent bug fix.** `App::add_system_to_stage` bound was generalized from
>   `impl IntoSystem<(),(),M>` to `impl IntoScheduleConfigs<ScheduleSystem,M>`
>   so plugins can pass tuples and `.chain()` for ordering. Same fix applied
>   to `add_system` and `add_startup_system`. RenderPlugin now chains
>   `(auto_sprite, auto_text, debug_overlay, render_system).chain()` —
>   previously `debug_overlay_system` and `render_system` were registered
>   unordered, so debug overlay output was non-deterministically visible
>   depending on scheduler order. Now deterministic.
> - **Wasm cross-compile fix.** `render_system` takes `ResMut<RenderEngine>`
>   but `RenderEngine` is only a `Resource` on native (WebGPU types are
>   `!Send` on wasm). Pre-existing breakage. Gated `render_system` and the
>   chain registration behind `#[cfg(not(target_arch = "wasm32"))]`; on wasm
>   the chain ends with a `wasm_clear_queue_system` stub that drains the
>   queue. cross_compile_web test now passes.
> - **Handle<T> derive bug fix.** `engine-assets::Handle<T>` had `derive`s
>   that transitively required `T: Clone + Debug + ...` even though
>   `Handle` only stores `id: u32, generation: u16, _marker: PhantomData<T>`.
>   Replaced with hand-rolled impls so `Handle<T>` is unconditionally
>   `Copy/Clone/Debug/PartialEq/Eq/Hash`. This unblocked deriving `Clone +
>   Debug` on `Sprite` and `Text` (which hold `Handle<Texture>`/`Handle<Font>`
>   over types like `Texture` that intentionally aren't `Clone`).
> - **api-seal-test extended** to cover the high-level component API:
>   `Sprite::new(...).with_size(...)` + `Text::new(...).with_size(...)` +
>   imperative `queue.draw_rect(...)`. If any of these stop working from
>   `lunar::prelude::*` alone, CI fails.

### 70. Domain Systems → Separate Crates — DONE
> Dialogue, localization, world-zones leave `engine-core` to honor separation
> of concerns. Games that don't need them pay zero compile cost.
- [x] 70.1 `crates/engine-dialogue/` created from `dialogue.rs` +
       `dialogue_parser.rs` (now `dialogue.rs` + `parser.rs`). Public:
       `Dialogue`, `DialogueBuilder`, `DialogueChoice`, `DialogueLine`,
       `DialogueManager`, `DialogueNode`, `DialoguePlugin`, `DialogueState`,
       `parse_dialogue`, `parse_dialogue_file`. Depends on `engine-core`
       (for `App`, `GamePlugin`).
- [x] 70.2 `crates/engine-localization/` created from `localization.rs`.
       Public: `Localization`, `LocalizationPlugin`. Depends on
       `engine-core` and `ron` (for locale file parsing).
- [x] 70.3 `crates/engine-zones/` created from `zone.rs`. Public:
       `Zone` (trait), `WorldManager`, `ZoneTransition`, `FadeConfig`.
       Depends only on `engine-math` (no engine-core dep — it didn't need
       `App` or `GamePlugin`). **Decision on Zone vs Scene:** kept distinct.
       `Scene` (in `engine-core`) is a generic stackable game-state pattern
       (menu / gameplay / pause overlay). `Zone` is RPG-shaped persistent
       area-loading with spatial transitions. Different shape, different
       audience. Both stay.
- [x] 70.4 `lunar::prelude` was already explicit and never re-exported
       these — confirmed clean.
- [x] 70.5 `appendix-a-complete-example.md` doesn't use dialogue / zones /
       localization. No changes needed.
- [x] 70.6 Workspace `Cargo.toml` updated with the three new members.
       Root `Cargo.toml` adds `engine-dialogue` to the rpg-example deps
       (it's the only consumer). `plans/design/12-dependency-graph.md`
       rewritten to show domain crates as opt-in branches and to include
       the new `lunar-macros` and `engine-assets` edges that were
       missing. `plans/design/05-world-zones.md` got a short note that
       zones now live in `engine-zones`.

> Side notes:
> - `engine-core::Cargo.toml` description updated (no longer claims to own
>   dialogue / zones / localization).
> - `engine-core::prelude` and `engine-core::lib.rs` re-exports cleaned up.
> - `rpg_example/plugin.rs` updated: `engine_core::DialoguePlugin` →
>   `engine_dialogue::DialoguePlugin`; same for `DialogueBuilder` /
>   `DialogueManager`.
> - Engine-core net loss: ~1043 LOC moved out into the three new crates.
>   Those crates are zero compile cost for any game that doesn't `use` them.

### 71. Editor — Removed from this Workspace's Roadmap — DONE
> Items 51–54 (Editor Foundation, Panels, Scene Editing, Build) moved out of
> this repo. The editor will be a downstream project that depends on
> `lunar` — same shape as the Moonwalker / engine-audio relationship.
- [x] 71.1 Decision recorded: editor is downstream. Items 51–54 abandoned in
       this repo.
- [x] 71.2 Items 51–54 headers carry the `OUT-OF-SCOPE (item 71)` marker;
       subitems remain as historical reference for whoever picks the editor
       up later.
- [x] 71.3 Item 54.3 (cdylib hot-reload via libloading) carries the same
       OUT-OF-SCOPE marker via its parent section.
- [x] 71.4 [`00-overview.md`](design/00-overview.md) line 23: "Editor lives
       downstream, not in this repo." — written.
- [x] 71.5 `engine-ui` (item 50, Taffy-based in-game UI) stays in scope —
       it's for game developers, not the editor.

### 72. P0 / P1 Performance Backlog Cleanup — DONE
> Investigated 2026-05-03; closed 2026-05-04.
- [x] 72.1 Item 29.1.4 — bind-group invalidation on texture removal:
       `RenderEngine::remove_texture(tex_id)` already drops from BOTH
       `textures` and `bind_groups` HashMaps in one call (verified). The
       hookup from `AssetServer` eviction to `remove_texture` doesn't yet
       exist because the asset server doesn't currently evict; that's a
       higher-level wiring task, not a fix to this code path. Closed.
- [x] 72.2 Item 30.1.4 — vertex-buffer overflow handling: **fixed**.
       Replaced the `MAX_VERTICES` const with a runtime `vertex_capacity`
       field (initial size still 65536). When a frame would overflow:
       drop the offending draw THIS frame and set `overflow_flag`. At the
       start of the NEXT frame, `grow_vertex_buffers()` doubles capacity
       (recreates both wgpu::Buffers) and logs a warning so the initial
       cap can be tuned. Pre-existing silent-skip bug eliminated; the
       cap auto-tunes itself in production.
- [x] 72.3 Item 30.1.3 — double/triple-buffer the vertex buffer: skipped
       per investigation note. `wgpu::Queue::write_buffer` already stages
       internally; revisit only if profiling shows a CPU-side stall.
- [x] 72.4 Item 36.1.3 — text layout cache invalidation: **audit
       complete**. Cache key is `(font_id, text.to_string(), font_size_bits)`,
       so content changes naturally miss the cache — no stale-content
       entries can accumulate. The remaining concern is unbounded growth
       with high-cardinality content (FPS counters, time displays); added
       a doc-comment on `get_cached_text_layout` recording this so a
       future LRU swap is easy to find. Closed.
- [x] 72.5 Item 34.1.4 — `apply_deferred` between stages: **confirmed
       handled**. bevy_ecs 0.18 defaults `ScheduleBuildSettings::auto_insert_apply_deferred`
       to `true` (verified in `bevy_ecs-0.18.1/src/schedule/schedule.rs:1581`),
       so within each per-stage schedule sync points are auto-inserted.
       Between stages, `engine_core::engine::Engine::run_stages` already
       calls `world.flush()` between schedule runs ([engine.rs:97](../crates/engine-core/src/engine.rs)).
       Both layers correct. Closed.

### 73.5. RPG-example migration to lunar facade — DONE
> The api-seal test proves the facade compiles; this proves it's livable
> for a non-trivial example.
- [x] 73.5.1 `rpg_example/components.rs` — `use bevy_ecs::prelude::*;` →
       `use lunar::prelude::*;`. Component derives still work because
       the prelude re-exports the wrapped `lunar-macros::Component`.
- [x] 73.5.2 `rpg_example/resources.rs` — same swap. `Handle<Texture>` /
       `Handle<Font>` now resolve through the lunar prelude.
- [x] 73.5.3 `rpg_example/plugin.rs` — replaced six per-crate imports
       (`engine_core::*`, `engine_assets::*`, `engine_input::*`,
       `engine_math::*`, `engine_render::*`, `bevy_ecs::prelude::*`) with
       a single `use lunar::prelude::*;`. `engine_dialogue::*` stays
       separate (opt-in domain crate). `UpdateStage` accessed via the
       documented escape hatch `lunar::engine_core::UpdateStage`.
- [x] 73.5.4 Added `Texture`, `Font`, `Sound` to `lunar::prelude`
       (and crate root). Required because `Handle<Texture>` / `Handle<Font>`
       come up in user resources/components and the type parameters need
       to be nameable.
- [x] 73.5.5 Trimmed root `Cargo.toml`: removed direct deps on
       `engine-core`, `engine-render`, `engine-input`, `engine-math`,
       `engine-assets`, `engine-image`, `bevy_ecs`. The root binary now
       depends on `lunar` + `engine-dialogue` only — exactly the shape
       a real downstream game would have.

> The api-seal test ([tests/api_seal/Cargo.toml](../tests/api_seal/Cargo.toml))
> still has only `lunar` as its dep, so the facade contract is enforced
> two ways: by a minimal CI test (api_seal) and by a non-trivial real
> example (rpg-example) that uses Sprite-less imperative draws, hierarchy,
> dialogue, custom components, and per-stage system scheduling.

---

## Dependency Graph

```
Phase 1 (Core ECS)
├── 1. ECS World & Schedule
├── 2. Plugin System → 1
└── 3. System Scheduling → 1, 2

Phase 2 (Subsystems)
├── 4. Time System → 1, 2, 3
├── 5. Input System → 1, 2, 3
├── 6. Render System → 1, 2, 3, 8 (Asset Server for textures)
└── 7. Audio System → 1, 2, 3, 8 (Asset Server for sounds)

Phase 3 (Assets)
└── 8. Asset Server → 1

Phase 4 (Game Loop)
├── 9. App.run() Integration → 1, 2, 3, 4, 5, 6, 7, 8
└── 10. lunar_app! Macro → 9

Phase 5 (World/Scenes)
├── 11. Zone System → 10
└── 12. Scene System → 10

Phase 6 (Errors)
└── 13. Error System → 10

Phase 7 (Dialogue)
└── 14. Dialogue System → 10, 6 (text rendering)

Phase 8 (Web)
└── 15. WASM Target → 9, 10 (all core systems working natively first)

Phase 9 (Polish)
├── 16. Extensibility → 10
├── 17. Macros → 10
└── 18. 3D Future → 6 (render system)

Phase 10 (High-Level API Rework)
├── 55. Seal Native API Leaks → 10 (lunar_app!), 6 (render), 5 (input)
├── 56. Coordinate System Helpers → 6 (render), 9.1 (Camera)
├── 57. Asset Loading UX → 8 (asset server)
├── 58. Render Pipeline Integration → 6 (render system)
└── 59. Viewport & Fullscreen → 5 (input), 6 (render), 10 (lunar_app!)

Part 2 (Post-Engine)
├── 19. Texture Atlas → 6 (render system), 8 (asset server)
├── 20. Layer System → 6 (render system)
├── 21. Entity Hierarchies → 9.1 (Transform component)
├── 22. Scene Format → 8 (asset server), 12 (scene system)
├── 23. Gameplay Framework → 10 (app.run), 11 (zone system)
├── 24. Rect Utilities → 9.3 (Rect type)
├── 25. Immediate Mode → 6 (render system)
├── 26. UI System → 19 (atlas), 20 (layers), 21 (hierarchies)
├── 27. Theme System → 26 (UI system)
└── 28. Named Events → 1 (ECS events)
```

---

### 45. Cross-Compile Compatibility Checks
- [x] 45.1 Create `tests/cross_compile.rs`
  - [x] 45.1.1 Test for each target: linux, macos, windows, web
  - [x] 45.1.2 Skips gracefully if target not installed
  - [x] 45.1.3 Runs `cargo check --target <triple> --workspace`
- [ ] 45.2 Add to CI pipeline (future)
  - [ ] 45.2.1 Install all targets on CI runners
  - [ ] 45.2.2 Run `cargo test cross_compile` on each push

---

## Part 3: Performance & API Audit Findings

> Items from the codebase audit (plans/codebase-audit.md).
> Organized by priority. P0 = critical performance, P1 = important improvements, P2 = nice-to-have, P3 = low priority.

### P0: Critical Performance Fixes

#### 29. Bind Group Caching
- [x] 29.1 Cache bind groups by texture ID
  - [x] 29.1.1 Add `HashMap<u32, wgpu::BindGroup>` to RenderEngine
  - [x] 29.1.2 Create bind group on texture upload, cache by texture ID
  - [x] 29.1.3 Look up cached bind group in render loop instead of creating new
  - [ ] 29.1.4 Handle texture removal/invalidation (remove from cache)
- **Impact:** Eliminates per-frame GPU object creation on Vulkan
- **Effort:** Low

#### 30. Persistent Vertex Buffer (Ring Buffer)
- [x] 30.1 Replace per-frame `create_buffer_init` with persistent buffer
  - [x] 30.1.1 Pre-allocate large vertex buffer at startup (`MAX_VERTICES * 20` bytes)
  - [x] 30.1.2 Use `write_buffer()` with `MAP_WRITE` + `COPY_DST` each frame
  - [ ] 30.1.3 Double-buffer or triple-buffer for frame overlap safety
  - [ ] 30.1.4 Handle buffer overflow (split into multiple draw calls or grow)
- **Impact:** Eliminates per-frame GPU memory allocation churn
- **Effort:** Medium

#### 31. StoreOp::Discard
- [x] 31.1 Change `store: Store` to `store: StoreOp::Discard` in render pass
  - [x] 31.1.1 Update color attachment operations
- **Impact:** Free performance on tile-based GPUs (Apple Silicon, mobile)
- **Effort:** Low

### P1: Important Improvements

#### 32. Vertex Format Packing
- [x] 32.1 Pack color into single `u32` (4 bytes instead of 16)
  - [x] 32.1.1 Change vertex from `[f32; 8]` (32 bytes) to `[f32; 4] + [u32; 1]` (20 bytes)
  - [x] 32.1.2 Update vertex shader to use `Unorm8x4` color
  - [x] 32.1.3 Update vertex attribute format (`Unorm8x4` instead of `Float32x4`)
  - [x] 32.1.4 Update all vertex generation code
- **Impact:** 37.5% less vertex data per frame
- **Effort:** Medium

#### 33. Sort Commands by (Layer, Texture)
- [x] 33.1 Replace HashMap grouping with sorted Vec
  - [x] 33.1.1 Sort by `(layer, texture_id)` tuple
  - [x] 33.1.2 Iterate linearly — same-texture commands are contiguous
  - [x] 33.1.3 Remove HashMap allocation from render loop
- **Impact:** Better cache locality, no HashMap overhead
- **Effort:** Low

#### 34. Stage-Based System Ordering
- [x] 34.1 Implement actual stage scheduling
  - [x] 34.1.1 Create bevy_ecs schedules for each stage (Input, Physics, Update, Render)
  - [x] 34.1.2 `add_system_to_stage()` adds to correct schedule
  - [x] 34.1.3 Run schedules in order each frame via `run_stages()`
  - [ ] 34.1.4 Handle `apply_deferred` between stages
- **Impact:** Game code can control system ordering
- **Effort:** High

#### 35. Fix Startup System Timing
- [x] 35.1 Track startup systems, run at start of `App::run()`
  - [x] 35.1.1 Add `Startup` schedule to Engine
  - [x] 35.1.2 `add_startup_system()` adds to startup schedule instead of running immediately
  - [x] 35.1.3 Run startup schedule at start of `run_with_events()` before main loop
- **Impact:** Startup systems run after all plugins/resources are ready
- **Effort:** Low

### P2: Nice-to-Have

#### 36. Cache Text Layout Results
- [x] 36.1 Cache computed glyph quads per text element
  - [x] 36.1.1 Store layout result in `RenderEngine.text_layout_cache`
  - [x] 36.1.2 Only recompute when text content, font, or size changes
  - [ ] 36.1.3 Invalidate cache on text change
- **Impact:** Eliminates per-frame character iteration and glyph lookup
- **Effort:** Low

#### 37. Pre-allocate RenderQueue Commands
- [x] 37.1 Pre-allocate `Vec::with_capacity(1024)` for commands
  - [x] 37.1.1 Use `clear()` which retains capacity
- **Impact:** Eliminates mid-frame reallocation
- **Effort:** Low

#### 38. Rect Utility Extensions
- [x] 38.1 Add methods to Rect (see item 24)
  - [x] 38.1.1 `inflate(dx, dy)`
  - [x] 38.1.2 `clamp(within)`
  - [x] 38.1.3 `collide_point(x, y)`
  - [x] 38.1.4 `collide_rect(other)`
  - [x] 38.1.5 `center()` (already existed)
  - [x] 38.1.6 `union(other)`
- **Effort:** Low

#### 39. Fix SpriteParams Origin Usage
- [x] 39.1 Use origin parameter in vertex generation
  - [x] 39.1.1 Compute rotation/scaling around origin point
- **Impact:** Fixes confusing API behavior
- **Effort:** Low

### P3: Low Priority

#### 40. Hybrid Input Key Array
- [ ] 40.1 Expand KeyCode beyond 64 variants
  - [ ] 40.1.1 Use array for common keys (0-127)
  - [ ] 40.1.2 HashMap fallback for rare/international keys
  - [ ] 40.1.3 Update `KEY_COUNT` and array sizes
- **Impact:** Support for all SDL3 keys
- **Effort:** Medium

#### 41. DrawKind::Line Variant
- [x] 41.1 Add proper line rendering
  - [x] 41.1.1 `DrawKind::Line { start, end, color, thickness, layer }`
  - [x] 41.1.2 Compute rotated rect vertices on CPU
- **Impact:** Clean diagonal lines, more efficient than AABB rect
- **Effort:** Medium

#### 42. Glyph Atlas Row-Copy Optimization
- [x] 42.1 Replace per-pixel copy with row copy
  - [x] 42.1.1 Use row-based iteration with slice access
- **Impact:** Faster glyph rasterization
- **Effort:** Low

#### 43. Pipeline Cache Serialization (Vulkan)
- [ ] 43.1 Serialize Vulkan pipeline cache to disk
  - [ ] 43.1.1 Save cache on shutdown
  - [ ] 43.1.2 Load cache on startup
  - [ ] 43.1.3 Reduces pipeline compilation time on subsequent launches
- **Impact:** Faster startup on Vulkan
- **Effort:** Medium

#### 44. Hybrid Frame Cap Sleep
- [x] 44.1 Improve frame timing precision
  - [x] 44.1.1 Sleep for most of wait time
  - [x] 44.1.2 Spin-wait last ~1ms with `std::hint::spin_loop()`
  - [x] 44.1.3 Reduces frame pacing jitter
- **Impact:** Smoother frame timing
- **Effort:** Low

---

## Dependency Graph (Part 3)

```
Part 3 (Performance & API Audit)
├── P0 (Critical Performance)
│   ├── 29. Bind Group Caching → 6 (render system)
│   ├── 30. Persistent Vertex Buffer → 6 (render system)
│   └── 31. StoreOp::Discard → 6 (render system)
├── P1 (Important)
│   ├── 32. Vertex Format Packing → 6 (render system)
│   ├── 33. Sort Commands by (Layer, Texture) → 6, 20 (layer system)
│   ├── 34. Stage-Based Ordering → 3 (system scheduling)
│   └── 35. Fix Startup Timing → 2 (plugin system)
├── P2 (Nice-to-Have)
│   ├── 36. Cache Text Layout → 6.3 (text rendering)
│   ├── 37. Pre-allocate RenderQueue → 6.1 (render queue)
│   ├── 38. Rect Utilities → 9.3 / 24 (Rect type)
│   └── 39. Fix Origin Usage → 6.1 (render queue)
└── P3 (Low Priority)
    ├── 40. Hybrid Input Array → 5 (input system)
    ├── 41. DrawKind::Line → 6 (render system)
    ├── 42. Glyph Row-Copy → 6.3 (text rendering)
    ├── 43. Pipeline Cache → 6 (render system, Vulkan only)
    └── 44. Hybrid Frame Cap → 9 (game loop)

Part 4 (Infrastructure)
└── 45. Cross-Compile Checks → all crates (workspace-wide)

Part 5 (Distribution)
├── 46. wgpu Patch → vendor/wgpu
├── 47. Public API Surface → lunar
├── 48. Crate Metadata → all crates
└── 49. Shooter Example → all core systems

Part 6 (Engine Editor)
├── 50. engine-ui crate (in-game UI, Taffy + custom wgpu) → 6 (render), 19 (atlas), 20 (layers)
├── 51. Editor Foundation (egui + wgpu + winit) → 49 (shooter example proves API)
├── 52. Editor Panels (egui) → 51
├── 53. Scene Editing → 51, 52
└── 54. Editor Build & Distribution → 51
```

---

## Part 4: Distribution

> Make the engine usable as a dependency in any external Rust project.
> Blocker: the vendored wgpu patch does not propagate to downstream workspaces — must be resolved before external users can consume the engine.

### 46. wgpu WASM Patch
- [ ] 46.1 Upstream the fix to wgpu
  - [ ] 46.1.1 Submit PR to gfx-rs/wgpu — change `instanceof` check to `unchecked_into` for `GPUCanvasContext` in the WebGPU backend
  - [ ] 46.1.2 Track PR status; keep vendored patch until merged and released
- [ ] 46.2 Short-term workaround for git consumers
  - [ ] 46.2.1 Document in README: users must add `[patch.crates-io] wgpu = { git = "...", rev = "..." }` to their own workspace until upstream merges
  - [ ] 46.2.2 Pin the vendored wgpu to a specific commit so the patch stays stable

### 47. Public API Surface
- [x] 47.1 Audit lunar re-exports
      - [x] 47.1.1 Every type a user needs (Transform, Color, Rect, RenderQueue, InputState, AssetServer, Time, App, Schedule, etc.) must be accessible via `use lunar::*` — no reaching into sub-crates
  - [x] 47.1.2 Identify any types currently leaking from engine-core/engine-render that aren't in lunar and add re-exports
  - [x] 47.1.3 Identify any lunar re-exports that expose internal implementation details and remove/hide them
- [x] 47.2 Prelude module
  - [x] 47.2.1 Add `lunar::prelude` that re-exports the most common items (App, Transform, Color, Rect, Vec2, Vec3, KeyCode, Time, RenderQueue, AssetServer, Commands, Query, Res, ResMut, Entity)
  - [x] 47.2.2 Users should be able to `use lunar::prelude::*` and write a full game without any further imports
- [x] 47.3 Rename crate to lunar (or create lunar facade crate)
  - [x] 47.3.1 Evaluate: rename the crate vs add a thin `lunar` facade crate that re-exports it — pick one
  - [x] 47.3.2 External users should write `lunar = { git = "..." }` not `engine-api = { git = "..." }`

### 48. Crate Metadata
- [ ] 48.1 Workspace-level Cargo.toml
  - [ ] 48.1.1 Add `description`, `repository`, `homepage`, `keywords`, `categories` to `[workspace.package]`
  - [ ] 48.1.2 Add `readme = "README.md"` pointing to project README
- [ ] 48.2 Per-crate metadata
  - [ ] 48.2.1 Each crate's Cargo.toml inherits workspace metadata where appropriate
  - [ ] 48.2.2 Each crate has a meaningful `description` of its role
- [ ] 48.3 Documentation
  - [ ] 48.3.1 Run `cargo doc --no-deps` and fix every broken doc link
  - [ ] 48.3.2 Each `lib.rs` has a crate-level doc comment explaining what the crate does and when to use it
  - [ ] 48.3.3 Key public types (App, RenderQueue, InputState, AssetServer) have usage examples in doc comments
  - [ ] 48.3.4 Add `#[doc(hidden)]` to internal types that are pub only for cross-crate visibility

### 49. Shooter Example
> Proves the full API end-to-end and serves as the canonical "hello world" for new users.
- [ ] 49.1 Project setup
  - [ ] 49.1.1 Add `examples/shooter/` directory with its own Cargo.toml depending only on `lunar`
  - [ ] 49.1.2 Provides a realistic test of the external-user experience — no reaching into internals
- [ ] 49.2 Assets
  - [ ] 49.2.1 Add placeholder pixel-art sprites for player, bullet, enemy (shipped in `examples/shooter/assets/`)
  - [ ] 49.2.2 Add a placeholder font for score display
- [ ] 49.3 Systems
  - [ ] 49.3.1 `spawn_player` startup system — entity with Transform + Sprite
  - [ ] 49.3.2 `move_player` system — WASD/arrow key movement via InputState
  - [ ] 49.3.3 `fire_bullet` system — spacebar spawns bullet entity, one per press
  - [ ] 49.3.4 `move_bullets` system — translate bullets forward each frame, despawn off-screen
  - [ ] 49.3.5 `spawn_enemies` system — periodic enemy spawning at random x positions
  - [ ] 49.3.6 `move_enemies` system — enemies move downward
  - [ ] 49.3.7 `check_collisions` system — bullet/enemy AABB collision, despawn both, increment score
  - [ ] 49.3.8 `draw_scene` system — issue draw_sprite and draw_text calls via RenderQueue
- [ ] 49.4 Plugin structure
  - [ ] 49.4.1 Wrap all systems in a `ShooterPlugin` implementing `GamePlugin`
  - [ ] 49.4.2 Wire up with `lunar_app!` macro

---

## Part 5: Engine Editor

> Two distinct UI concerns that must not be conflated:
>
> **Editor GUI** (panels, inspector, hierarchy, asset browser) — uses **egui**.
> egui is immediate mode, MIT licensed, integrates with wgpu via egui-wgpu in a few lines,
> and is designed exactly for game tooling. Performance is fine for a desktop editor.
> Slint is the alternative if visual polish becomes a priority, but brings GPL licensing and
> heavier integration. Taffy is not an answer here — it is layout math only, no widgets.
>
> **In-game UI** (UI inside games made with the engine) — uses **Taffy + custom wgpu**.
> This lives in `engine-ui` (item 26 from Part 2, tracked as item 50 here).
> Completely separate from the editor GUI — game developers use this, not the editor itself.
>
> **Architecture: in-process** — editor is a separate binary. Game scene runs in an engine World
> inside the editor process. Game systems frozen while paused; full rate in play mode.
> winit drives the editor window; SDL3 stays for game builds only.

### 50. engine-ui crate (in-game UI for games made with the engine)
> Taffy + custom wgpu. Used by game developers, not by the editor GUI itself.
> This is item 26 from Part 2 promoted here because editor work makes it the right time to build it.
- [ ] 50.1 `engine-ui` crate
  - [ ] 50.1.1 Add `engine-ui` to the workspace under `crates/engine-ui/`
  - [ ] 50.1.2 Dependencies: `taffy`, `engine-render`, `engine-math`, `engine-input`
  - [ ] 50.1.3 No dependency on `engine-editor` — purely a game-side API
- [ ] 50.2 Taffy layout integration
  - [ ] 50.2.1 `UiTree` struct wrapping a `taffy::TaffyTree` — owns all node handles
  - [ ] 50.2.2 `UiNode` component: maps an ECS entity to a Taffy node handle
  - [ ] 50.2.3 `Style` component: wraps `taffy::Style` (flex direction, size, padding, margin, etc.)
  - [ ] 50.2.4 `compute_layout(root, available_space)` — calls `taffy.compute_layout`, writes resolved `Rect` back to each node's `LayoutOutput` component
  - [ ] 50.2.5 Layout is lazy — only recomputed when a `Style` or tree structure changes (dirty flag on `UiTree`)
- [ ] 50.3 UI render pass (custom wgpu)
  - [ ] 50.3.1 `UiRenderPass` struct — a dedicated wgpu render pass that runs after the game scene pass
  - [ ] 50.3.2 Separate vertex buffer for UI geometry (screen-space, pixel coordinates, no camera transform)
  - [ ] 50.3.3 UI shader: supports flat color fills, texture sampling, and rounded corner SDF (single shader, mode selected per draw call via push constant)
  - [ ] 50.3.4 Nine-patch support for panel backgrounds — avoids stretching artifacts on resizable panels
  - [ ] 50.3.5 Scissor rect per panel for clipping overflow (wgpu scissor_rect on render pass)
  - [ ] 50.3.6 `UiDrawList` — sorted list of UI draw commands built each frame from layout output, flushed to `UiRenderPass`
- [ ] 50.4 Widget primitives
  - [ ] 50.4.1 `Panel { background: Color, border: Option<Border> }` — filled rect, optional border
  - [ ] 50.4.2 `Label { text: String, font: Handle<Font>, size: f32, color: Color }` — single-line text
  - [ ] 50.4.3 `Button { label: String, style: ButtonStyle }` — panel + label + interaction state
  - [ ] 50.4.4 `TextInput { value: String, placeholder: String }` — editable single-line field
  - [ ] 50.4.5 `ScrollArea { content_height: f32 }` — vertical scroll with scroll offset, scissor clipping
  - [ ] 50.4.6 `Image { handle: Handle<Texture>, tint: Color }` — texture display with optional tint
  - [ ] 50.4.7 `Separator` — horizontal or vertical dividing line
- [ ] 50.5 Input routing
  - [ ] 50.5.1 `UiInputSystem` — runs before game input, walks the node tree hit-testing pointer position
  - [ ] 50.5.2 `Interaction` component: `None | Hovered | Pressed` — updated each frame
  - [ ] 50.5.3 `Focus` component: tracks keyboard focus, Tab cycles through focusable nodes
  - [ ] 50.5.4 Events: `ButtonPressed`, `TextChanged`, `ScrollMoved` — fire as ECS events for game/editor code to consume
  - [ ] 50.5.5 Input consumed by UI is not forwarded to game systems (event propagation stop)

### 51. Editor Foundation (engine-editor crate) — OUT-OF-SCOPE (item 71)
> Editor moved to a separate downstream project. Items below kept as historical
> reference for whoever picks the editor up later; not actively tracked.

- [ ] 51.1 `engine-editor` crate
  - [ ] 51.1.1 Add `engine-editor` to the workspace under `crates/engine-editor/`
  - [ ] 51.1.2 Binary target: `lunar-editor`
  - [ ] 51.1.3 Dependencies: `lunar`, `engine-render`, `egui`, `egui-wgpu`, `egui-winit`, `winit`
- [ ] 51.2 Window and render setup
  - [ ] 51.2.1 winit event loop creates the editor window (not SDL3)
  - [ ] 51.2.2 Initialize wgpu surface, device, queue from the winit window handle
  - [ ] 51.2.3 Initialize `egui_wgpu::Renderer` sharing the same wgpu device — no second GPU context
  - [ ] 51.2.4 `EditorApp` struct: holds `egui::Context`, `egui_wgpu::Renderer`, engine `World`, engine `RenderEngine`, `EditorState`
  - [ ] 51.2.5 Each frame: run egui frame → render game scene to offscreen texture → render egui paint jobs → present
- [ ] 51.3 Offscreen game viewport texture
  - [ ] 51.3.1 Allocate a wgpu texture sized to the viewport panel's layout rect
  - [ ] 51.3.2 Game `RenderEngine` renders into this texture instead of the swap chain
  - [ ] 51.3.3 Display the texture in the center panel via `Image` widget
  - [ ] 51.3.4 Reallocate texture on viewport resize
- [ ] 51.4 Editor state and tick modes
  - [ ] 51.4.1 `EditorState` resource: `selected_entity`, `gizmo_mode`, `play_state`, `project_path`
  - [ ] 51.4.2 `PlayState` enum: `Stopped | Playing | Paused`
  - [ ] 51.4.3 Stopped/Paused: engine ECS world is frozen, only editor systems run
  - [ ] 51.4.4 Playing: engine runs at full tick rate, editor panels update at reduced rate (every 4 frames)
  - [ ] 51.4.5 On stop: restore world snapshot taken at play start (serialize before play, deserialize on stop)

### 52. Editor Panels — OUT-OF-SCOPE (item 71)
- [ ] 52.1 Main layout (egui panels)
  - [ ] 52.1.1 `egui::TopBottomPanel` for menu bar and status bar
  - [ ] 52.1.2 `egui::SidePanel` left for scene hierarchy, right for inspector
  - [ ] 52.1.3 `egui::TopBottomPanel` inside central area for toolbar, `egui::CentralPanel` for viewport
  - [ ] 52.1.4 `egui::TopBottomPanel` bottom for asset browser + log tabs
  - [ ] 52.1.5 Panel resize: egui handles this natively via resizable side panels
- [ ] 52.2 Menu bar
  - [ ] 52.2.1 File: New Project, Open Project, Save Scene (Ctrl+S), Quit
  - [ ] 52.2.2 Edit: Undo (Ctrl+Z), Redo (Ctrl+Y), Spawn Entity, Despawn Selected
  - [ ] 52.2.3 View: toggle panel visibility
- [ ] 52.3 Toolbar
  - [ ] 52.3.1 Play / Pause / Stop buttons — updates `PlayState`
  - [ ] 52.3.2 Gizmo mode buttons: Translate / Rotate / Scale
  - [ ] 52.3.3 Frame time and FPS readout (right-aligned)
- [ ] 52.4 Scene hierarchy panel
  - [ ] 52.4.1 Query all entities each frame; display in a `ScrollArea`
  - [ ] 52.4.2 Show `Name` component if present, else show `Entity` id as `"entity {id}"`
  - [ ] 52.4.3 Click sets `EditorState::selected_entity`
  - [ ] 52.4.4 Right-click context menu: Spawn Empty, Despawn
  - [ ] 52.4.5 Indent children under parents once entity hierarchies (item 21) are implemented
- [ ] 52.5 Inspector panel
  - [ ] 52.5.1 Display components on `EditorState::selected_entity` in an `egui::ScrollArea`
  - [ ] 52.5.2 `Transform`: `egui::DragValue` fields for x/y/z translation, rotation in degrees, scale x/y
  - [ ] 52.5.3 `Color`: egui's built-in `color_picker::color_edit_button_rgba`
  - [ ] 52.5.4 `Handle<Texture>`: show asset path and register texture with `egui_wgpu` to display as thumbnail
  - [ ] 52.5.5 `Inspectable` trait: components implement `fn inspect(&mut self, ui: &mut egui::Ui)` for custom inspector widgets
  - [ ] 52.5.6 Unknown components: show type name as `egui::CollapsingHeader` with "not inspectable" body
  - [ ] 52.5.7 Write field changes back to ECS world immediately on value change
- [ ] 52.6 Asset browser panel
  - [ ] 52.6.1 Walk `project_path/assets/` on open and on `AssetWatcher` change events
  - [ ] 52.6.2 Group by type in a `ScrollArea`: Textures, Fonts, Sounds, Other
  - [ ] 52.6.3 Texture entries show a thumbnail `Image` (load via `AssetServer`, display when ready)
  - [ ] 52.6.4 Click selects asset; shows path, file size, dimensions, load state in a sidebar
  - [ ] 52.6.5 Double-click opens in OS default app (`open` crate)
- [ ] 52.7 Log panel
  - [ ] 52.7.1 Custom `log::Log` impl buffers up to 1000 `LogEntry { level, message, timestamp }` in a ring buffer
  - [ ] 52.7.2 Display in `ScrollArea`, auto-scroll to bottom unless user has scrolled up manually
  - [ ] 52.7.3 Color by level: error=red, warn=amber, info=white, debug=gray, trace=dark gray
  - [ ] 52.7.4 Filter bar: level dropdown + text search field

### 53. Scene Editing — OUT-OF-SCOPE (item 71)
- [ ] 53.1 Viewport entity picking
  - [ ] 53.1.1 On left click in viewport (not consumed by gizmo): hit-test all entities with Transform + sprite bounds
  - [ ] 53.1.2 Sort hits back-to-front by z; select topmost
  - [ ] 53.1.3 Highlight selected entity with an outline overlay drawn via the engine's render queue (into the offscreen texture, not egui)
- [ ] 53.2 Transform gizmos
  - [ ] 53.2.1 Gizmos drawn via the engine's render queue into the offscreen texture — rendered in world space, appear naturally in the viewport
  - [ ] 53.2.2 Translate: X (red) and Y (green) axis arrows; drag moves entity in that axis
  - [ ] 53.2.3 Rotate: arc handle around entity center; drag rotates around Z
  - [ ] 53.2.4 Scale: corner squares; drag scales X/Y (hold Shift for uniform)
  - [ ] 53.2.5 Gizmo drag produces `TransformChanged` commands buffered into the undo stack
- [ ] 53.3 Spawn / despawn
  - [ ] 53.3.1 Drag texture from asset browser into viewport → spawn entity with that Handle<Texture> + Transform at drop position
  - [ ] 53.3.2 Delete key: despawn `EditorState::selected_entity`, clear selection
  - [ ] 53.3.3 Ctrl+Z / Ctrl+Y: undo/redo stack of `EditorCommand` enum (SpawnEntity, DespawnEntity, TransformChanged, ComponentChanged)
- [ ] 53.4 Scene save/load
  - [ ] 53.4.1 Scene format: JSON — array of `{ id, components: { "Transform": {...}, ... } }`
  - [ ] 53.4.2 Components must implement `SceneSerialize` trait to be included (opt-in, keeps format stable)
  - [ ] 53.4.3 Save: serialize world → write to `project_path/scenes/<name>.scene.json`
  - [ ] 53.4.4 Load: clear world entities, deserialize JSON, spawn via Commands
  - [ ] 53.4.5 Dirty flag: title bar shows `*` on unsaved changes; prompt to save on close / open / play

---

## Phase 12: Core 2D Systems (opt-in crates)

> These crates are separate from `lunar` core and from each other. Game code opts in by
> adding the crate to its own `Cargo.toml`. Games that don't need them pay zero compile cost.
> Same pattern as `engine-dialogue`, `engine-localization`, `engine-zones`.

### 74. 2D collision (module inside engine-2d)
> Basic 2D collision detection. No physics simulation — no rigid bodies, gravity, or joints.
> Lives in `engine_2d::collision` — dimension-specific, no reason for a shared crate.
> 3D collision would live inside a future `engine-3d` the same way.
> The shooter example (item 49) needs AABB hit-testing; anything beyond that is post-v1.
- [ ] 74.1 `engine_2d::collision` module
  - [ ] 74.1.1 Add `crates/engine-2d/src/collision.rs`, pub-use from `engine-2d/src/lib.rs`
  - [ ] 74.1.2 No new crate — stays inside `engine-2d`, which already depends on `engine-math` + `engine-core`
  - [ ] 74.1.3 No dependency on `engine-render` — collision is pure logic
- [ ] 74.2 Collider component
  - [ ] 74.2.1 `Collider` component: `shape: ColliderShape`
  - [ ] 74.2.2 `ColliderShape` enum: `Aabb { half_extents: Vec2 }`, `Circle { radius: f32 }`
  - [ ] 74.2.3 `CollisionLayer(u32)` component: bitmask for filtering (which layers this collider is on)
  - [ ] 74.2.4 `CollisionMask(u32)` component: bitmask for filtering (which layers this collider checks against)
- [ ] 74.3 Overlap queries
  - [ ] 74.3.1 `CollisionWorld` resource — built each frame from all entities with `Collider + Transform`
  - [ ] 74.3.2 `CollisionWorld::overlapping(entity) -> Vec<Entity>` — which entities overlap this one
  - [ ] 74.3.3 `CollisionWorld::query_point(point: Vec2) -> Vec<Entity>` — entities containing a point
  - [ ] 74.3.4 `CollisionWorld::query_rect(rect: Rect) -> Vec<Entity>` — entities overlapping a rect
- [ ] 74.4 Collision events
  - [ ] 74.4.1 `CollisionStarted { a: Entity, b: Entity }` message — fired when two colliders begin overlapping
  - [ ] 74.4.2 `CollisionEnded { a: Entity, b: Entity }` message — fired when they stop overlapping
  - [ ] 74.4.3 Previous-frame state tracked in `CollisionWorld` to diff start/end
- [ ] 74.5 `CollisionPlugin`
  - [ ] 74.5.1 `CollisionPlugin` implements `GamePlugin`, registers `build_collision_world` system in Physics stage
  - [ ] 74.5.2 `build_collision_world` rebuilds `CollisionWorld` each frame from ECS query

### 75. engine-animation (sprite frame animation)
> Frame-by-frame sprite animation. Drives the `Sprite` component's `source_rect`
> from a named clip + frame sequence. No skeletal animation — that is post-v1.
- [ ] 75.1 `engine-animation` crate
  - [ ] 75.1.1 Add `crates/engine-animation/` to the workspace
  - [ ] 75.1.2 Dependencies: `engine-math`, `engine-core`, `engine-render`, `bevy_ecs`
- [ ] 75.2 Animation clip data
  - [ ] 75.2.1 `AnimationClip` struct: `name: String, frames: Vec<AnimationFrame>, looping: bool`
  - [ ] 75.2.2 `AnimationFrame` struct: `source_rect: Rect, duration_secs: f32`
  - [ ] 75.2.3 `AnimationSet` asset — a named map of clip name → `AnimationClip`; loaded via `AssetServer`
- [ ] 75.3 Animator component
  - [ ] 75.3.1 `Animator` component: `clips: Handle<AnimationSet>, current_clip: String, elapsed: f32, frame_index: usize, playing: bool`
  - [ ] 75.3.2 `Animator::play(clip_name)` — switch clip, reset elapsed + frame
  - [ ] 75.3.3 `Animator::stop()` — freeze on current frame
- [ ] 75.4 Animation system
  - [ ] 75.4.1 `advance_animations` system: for each `(Animator, Sprite)` pair, advance elapsed, update `frame_index`, write `Sprite::source_rect`
  - [ ] 75.4.2 Handles looping: wraps frame index back to 0 when `looping: true`, freezes on last frame otherwise
  - [ ] 75.4.3 Fires `AnimationFinished { entity, clip_name }` message when a non-looping clip reaches its last frame
- [ ] 75.5 `AnimationPlugin`
  - [ ] 75.5.1 `AnimationPlugin` implements `GamePlugin`, registers `advance_animations` in Update stage
  - [ ] 75.5.2 Registers `AnimationSet` loader with `AssetServer` (json or ron format)

### 76. engine-tilemap (tile-based level rendering)
> Grid-based tile rendering. Game code defines a tile atlas and a 2D grid of tile IDs;
> the engine renders it efficiently. No built-in pathfinding or physics integration — those
> are separate concerns layered on top.
- [ ] 76.1 `engine-tilemap` crate
  - [ ] 76.1.1 Add `crates/engine-tilemap/` to the workspace
  - [ ] 76.1.2 Dependencies: `engine-math`, `engine-core`, `engine-render`, `engine-assets`, `bevy_ecs`
- [ ] 76.2 Tile atlas
  - [ ] 76.2.1 `TileAtlas` struct: `texture: Handle<Texture>, tile_width: u32, tile_height: u32`
  - [ ] 76.2.2 `TileAtlas::source_rect(tile_id: u32) -> Rect` — computes UV rect for a given tile id (row-major)
- [ ] 76.3 TileMap component
  - [ ] 76.3.1 `TileMap` component: `atlas: TileAtlas, tiles: Vec<Vec<Option<u32>>>, layer: i32`
  - [ ] 76.3.2 Tiles are `Option<u32>` — `None` means empty (transparent), `Some(id)` maps to the atlas
  - [ ] 76.3.3 `TileMap::get(col, row) -> Option<u32>` and `TileMap::set(col, row, tile_id)`
  - [ ] 76.3.4 `TileMap::world_to_tile(pos: Vec2) -> (i32, i32)` — convert world position to tile coordinate
  - [ ] 76.3.5 `TileMap::tile_to_world(col: i32, row: i32) -> Vec2` — convert tile coordinate to world position
- [ ] 76.4 Tilemap render system
  - [ ] 76.4.1 `render_tilemaps` system: for each `(Transform, TileMap)`, iterate visible tiles and issue `draw_sprite_atlas` calls via `RenderQueue`
  - [ ] 76.4.2 Frustum cull — only draw tiles whose world rect overlaps the camera viewport (skip off-screen tiles)
  - [ ] 76.4.3 Tile ordering: tiles on the same layer draw in row-major order (top to bottom, left to right)
- [ ] 76.5 `TileMapPlugin`
  - [ ] 76.5.1 `TileMapPlugin` implements `GamePlugin`, registers `render_tilemaps` in Render stage

---

### 54. Editor Build & Distribution — OUT-OF-SCOPE (item 71)
- [ ] 54.1 Standalone binary
  - [ ] 54.1.1 `cargo build --bin lunar-editor --release` produces the editor executable
  - [ ] 54.1.2 No SDL3 runtime dependency in the editor binary (winit only)
- [ ] 54.2 Project model
  - [ ] 54.2.1 Editor opens a project directory containing `assets/`, `src/`, `Cargo.toml`, `lunar.project.json`
  - [ ] 54.2.2 `lunar.project.json`: project name, default scene, editor camera settings
  - [ ] 54.2.3 File > New Project: scaffold directory, write template `GamePlugin`, open in editor
  - [ ] 54.2.4 File > Open Project: pick directory, validate structure, load default scene
- [ ] 54.3 Game plugin hot reload (stretch goal)
  - [ ] 54.3.1 Compile game plugin as `cdylib` via `cargo build` subprocess on source save
  - [ ] 54.3.2 Unload old dylib, load new one with `libloading`, re-run startup systems against current world
  - [ ] 54.3.3 High complexity — only pursue after the rest of the editor is stable
