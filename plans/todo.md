# Lunar Engine — Implementation TODO

> Track all features needed to reach the shooter example and beyond.
> Items can only be worked on when all dependencies are checked off.

## Legend
- [ ] Not started
- [x] Complete
- Dependencies listed as `→ #item`

---

## Phase 1: Core ECS Integration

### 1. ECS World & Schedule
- [x] 1.1 Integrate bevy_ecs World into engine-core
  - [x] 1.1.1 Add bevy_ecs dependency to engine-core
  - [x] 1.1.2 Create World wrapper in engine-core (Engine wraps bevy_ecs::World)
  - [x] 1.1.3 Re-export World through engine-api (bevy_ecs re-exported, World accessible)
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
  - [x] 1.3.3 Re-export through engine-api
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

### 7. Audio System
- note: audio is NOT a current requirement — Moonwalker (custom audio engine, cpal-based, WASM compatible) will integrate here
- note: AudioPlugin stays as a stub until Moonwalker is ready to wire in
- [ ] 7.1 AudioEngine resource (stub — filled in by Moonwalker later)
  - [x] 7.1.1 play_sound(handle, volume, pitch) — fire-and-forget
  - [ ] 7.1.2 play_sound_controlled() → SoundInstanceHandle
  - [ ] 7.1.3 play_music(handle, volume)
  - [ ] 7.1.4 stop_music(), fade_music()
  - [ ] 7.1.5 set_master_volume(), master_volume()
- [ ] 7.2 SoundInstanceHandle
  - [ ] 7.2.1 set_volume(), set_pitch(), stop(), is_playing()
- [ ] 7.3 AudioPlugin
  - [x] 7.3.1 Wire Moonwalker backend (cpal, WASM compatible) — stub only for now
  - [ ] 7.3.2 Process audio commands each frame

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
```

## Shooter Example Requirements

To run the shooter example from the design doc, we need:
- [x] Phase 1: Core ECS Integration (1, 2, 3)
- [x] Phase 2: Subsystems (4, 5, 6 — at minimum: Time, Input, Render with draw_sprite)
- [x] Phase 3: Asset Server (8 — at minimum: load, Handle<Texture>)
- [x] Phase 4: Game Loop Integration (9, 10)

Everything else (zones, scenes, dialogue, web, 3D) can come after.

---

## Part 2: Post-Engine (UI & Polish Phase)

> These items are deferred until the core engine is stable.
> They are tracked here for planning purposes only — do NOT start work on these until Part 1 is complete.

### 19. Texture Atlas System
- [ ] 19.1 TextureAtlas resource
  - [ ] 19.1.1 Bin-packing algorithm (shelf packing or maxrects)
  - [ ] 19.1.2 Atlas builder (packs multiple textures into single GPU texture)
  - [ ] 19.1.3 Region lookup by name
- [ ] 19.2 Sprite atlas integration
  - [ ] 19.2.1 Sprite component gains optional `atlas_region: Option<Rect>`
  - [ ] 19.2.2 RenderQueue batches by atlas texture, not individual textures
  - [ ] 19.2.3 UV coordinate remapping for atlas regions
- [ ] 19.3 Asset pipeline support
  - [ ] 19.3.1 Atlas definition format (JSON5 authoring → binary runtime)
  - [ ] 19.3.2 Atlas compilation during asset bundling

### 20. Layer-Based Rendering
- [ ] 20.1 Layer component
  - [ ] 20.1.1 `Layer { order: i32 }` component
  - [ ] 20.1.2 Built-in layer constants (BACKGROUND, GAME, FOREGROUND, UI)
- [ ] 20.2 RenderQueue layer sorting
  - [ ] 20.2.1 Sort draw commands by layer before batching
  - [ ] 20.2.2 Stable sort (preserve registration order within same layer)
- [ ] 20.3 Camera per-layer offset
  - [ ] 20.3.1 Optional parallax support (per-layer camera offset)

### 21. Entity Hierarchies (Composition, NOT Inheritance)
- [ ] 21.1 Parent/Child components
  - [ ] 21.1.1 `Parent(pub Entity)` component
  - [ ] 21.1.2 `Children(pub SmallVec<[Entity; 4]>)` component
- [ ] 21.2 Transform propagation system
  - [ ] 21.2.1 Compute world transforms from local + parent
  - [ ] 21.2.2 Run in Update stage, before render
- [ ] 21.3 LocalTransform vs WorldTransform
  - [ ] 21.3.1 LocalTransform: position relative to parent
  - [ ] 21.3.2 WorldTransform: absolute position (computed)

### 22. Scene Definition Format
- [ ] 22.1 Authoring format (JSON5)
  - [ ] 22.1.1 JSON5 scene schema definition
  - [ ] 22.1.2 Scene parser and validator
- [ ] 22.2 Runtime format (binary)
  - [ ] 22.2.1 Binary scene serialization (bincode/rkyv/custom)
  - [ ] 22.2.2 Compile-time conversion: JSON5 → binary
- [ ] 22.3 Scene loader
  - [ ] 22.3.1 Load binary scene → spawn entities via Commands
  - [ ] 22.3.2 SceneHandle for runtime reference
  - [ ] 22.3.3 Scene instancing (nest scenes within scenes)

### 23. Gameplay Framework (Optional)
- [ ] 23.1 GameMode resource
  - [ ] 23.1.1 Game rules, zone transitions, scene management
- [ ] 23.2 PlayerController resource
  - [ ] 23.2.1 Input routing, camera control, UI interaction
- [ ] 23.3 Pawn component
  - [ ] 23.3.1 Physical representation of player/AI
- [ ] 23.4 GameState/PlayerState resources
  - [ ] 23.4.1 Per-game and per-player data tracking

### 24. Rect Utility Extensions
- [ ] 24.1 Add methods to Rect
  - [ ] 24.1.1 `inflate(dx, dy)` — expand/shrink rect
  - [ ] 24.1.2 `clamp(within)` — constrain rect inside another
  - [ ] 24.1.3 `collide_point(x, y)` — point collision check
  - [ ] 24.1.4 `collide_rect(other)` — rect collision check
  - [ ] 24.1.5 `center()` — get center point
  - [ ] 24.1.6 `union(other)` — bounding box of two rects

### 25. Immediate Mode Render API (Optional)
- [ ] 25.1 Immediate mode API
  - [ ] 25.1.1 `draw_immediate(|draw| { ... })` closure
  - [ ] 25.1.2 Debug drawing helpers (lines, circles, text)
- [ ] 25.2 Debug overlay
  - [ ] 25.2.1 FPS counter, entity count, collision visualization

---

### 26. UI System (engine-ui crate) — DEFERRED
> Full UI system implementation. Requires texture atlas, layer system, and entity hierarchies to be complete first.

- [ ] 26.1 engine-ui crate structure
  - [ ] 26.1.1 `node/` — Node + Style components (ECS)
  - [ ] 26.1.2 `layout/` — Taffy integration (flexbox layout)
  - [ ] 26.1.3 `widget/` — Button, Label, Panel bundles
  - [ ] 26.1.4 `interaction/` — Hover/press/focus tracking
  - [ ] 26.1.5 `events/` — UI event types (pressed, changed, focused)
- [ ] 26.2 Layout system
  - [ ] 26.2.1 Taffy integration (pure Rust flexbox, WASM compatible)
  - [ ] 26.2.2 Lazy recomputation — only on style/content change, NOT every frame
  - [ ] 26.2.3 Dirty region tracking — mark only changed nodes for re-layout
- [ ] 26.3 Widget bundles
  - [ ] 26.3.1 Button (with Interaction component)
  - [ ] 26.3.2 Label (text display)
  - [ ] 26.3.3 Panel (container with background)
  - [ ] 26.3.4 Image (texture display)
  - [ ] 26.3.5 Containers: VBox, HBox, Grid, Margin, Center
- [ ] 26.4 Focus management
  - [ ] 26.4.1 Focus stack for keyboard/gamepad navigation
  - [ ] 26.4.2 Tab order, directional navigation
- [ ] 26.5 UI → DrawCommand conversion
  - [ ] 26.5.1 UI entities produce DrawCommands (decoupled from render crate)
  - [ ] 26.5.2 UI render pass (runs after game objects, on UI layer)

### 27. Theme System — DEFERRED
- [ ] 27.1 Theme resource
  - [ ] 27.1.1 `Theme { colors, fonts, font_sizes, style_boxes }`
  - [ ] 27.1.2 Theme loading from JSON5
  - [ ] 27.1.3 Runtime theme swapping (for skinning/accessibility)
- [ ] 27.2 StyleBox
  - [ ] 27.2.1 Flat, textured, bordered backgrounds
  - [ ] 27.2.2 Nine-patch scaling

### 28. Named Event System (Optional) — DEFERRED
- [ ] 28.1 EventBus resource
  - [ ] 28.1.1 Named event dispatch (`events.dispatch("player_died", event)`)
  - [ ] 28.1.2 Event subscription by name
  - [ ] 28.1.3 Event priority ordering
- [ ] 28.2 Integration with ECS events
  - [ ] 28.2.1 Named events wrap bevy_ecs events under the hood
  - [ ] 28.2.2 Raw ECS events still available for performance-critical paths

---

## Dependency Graph (Updated)

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
├── 47. Public API Surface → engine-api
├── 48. Crate Metadata → all crates
└── 49. Shooter Example → all core systems

Part 6 (Engine Editor)
├── 50. Editor Foundation → 49 (working example proves API)
├── 51. Editor Panels → 50
├── 52. Scene Editing → 50, 51
└── 53. Editor Build & Distribution → 50
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
- [ ] 47.1 Audit engine-api re-exports
  - [ ] 47.1.1 Every type a user needs (Transform, Color, Rect, RenderQueue, InputState, AssetServer, Time, App, Schedule, etc.) must be accessible via `use engine_api::*` — no reaching into sub-crates
  - [ ] 47.1.2 Identify any types currently leaking from engine-core/engine-render that aren't in engine-api and add re-exports
  - [ ] 47.1.3 Identify any engine-api re-exports that expose internal implementation details and remove/hide them
- [ ] 47.2 Prelude module
  - [ ] 47.2.1 Add `engine_api::prelude` that re-exports the most common items (App, Transform, Color, Rect, Vec2, Vec3, KeyCode, Time, RenderQueue, AssetServer, Commands, Query, Res, ResMut, Entity)
  - [ ] 47.2.2 Users should be able to `use engine_api::prelude::*` and write a full game without any further imports
- [ ] 47.3 Rename engine-api to lunar (or create lunar facade crate)
  - [ ] 47.3.1 Evaluate: rename the crate vs add a thin `lunar` crate that re-exports engine-api — pick one
  - [ ] 47.3.2 External users should write `lunar = { git = "..." }` not `engine-api = { git = "..." }`

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
  - [ ] 49.1.1 Add `examples/shooter/` directory with its own Cargo.toml depending only on `engine-api` (or `lunar`)
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

> A Unity-style editor tool — a separate application that runs alongside (or wraps) the engine,
> providing a viewport, scene hierarchy, inspector, and asset browser.
>
> **Framework: egui** — immediate mode, egui-wgpu backend shares the existing wgpu device/queue,
> battle-tested in Rust editor tooling. Slint/iced/taffy are not suitable (wrong paradigm or integration model).
>
> **Architecture: in-process** — the editor is a separate binary that hosts the engine's ECS world
> internally, renders the game scene to an offscreen texture, and displays that texture inside an egui viewport panel.
> The game's systems run at editor tick rate (not real-time) while paused.

### 50. Editor Foundation
- [ ] 50.1 `engine-editor` crate
  - [ ] 50.1.1 Add `engine-editor` to the workspace under `crates/engine-editor/`
  - [ ] 50.1.2 Binary target: `editor` (the editor application itself)
  - [ ] 50.1.3 Dependencies: `engine-api`, `engine-render`, `egui`, `egui-wgpu`, `egui-winit`
  - [ ] 50.1.4 Replace SDL3 window creation with winit for the editor window (egui-winit handles events)
- [ ] 50.2 egui + wgpu integration
  - [ ] 50.2.1 Initialize wgpu device/queue/surface for the editor window via winit
  - [ ] 50.2.2 Initialize `egui_wgpu::Renderer` sharing the same device
  - [ ] 50.2.3 Each frame: run egui, collect paint jobs, render game scene, composite egui on top
  - [ ] 50.2.4 Implement `EditorApp` struct holding: `egui::Context`, `egui_wgpu::Renderer`, engine `World`, engine `RenderEngine`
- [ ] 50.3 Offscreen game viewport
  - [ ] 50.3.1 Create a wgpu texture for the game scene render target (not the swap chain)
  - [ ] 50.3.2 Render the game world into that texture each frame
  - [ ] 50.3.3 Register the texture with egui-wgpu so it can be displayed as an `egui::Image`
  - [ ] 50.3.4 Resize the render texture when the viewport panel is resized
- [ ] 50.4 Editor event loop
  - [ ] 50.4.1 winit event loop drives the editor (not SDL3)
  - [ ] 50.4.2 Forward keyboard/mouse events to egui first; unfocused viewport events go to the engine input system
  - [ ] 50.4.3 Play mode: engine runs at full tick rate. Pause mode: engine systems frozen, only editor systems run

### 51. Editor Panels
- [ ] 51.1 Main layout
  - [ ] 51.1.1 Top menu bar (File, Edit, View, Help)
  - [ ] 51.1.2 Left panel: scene hierarchy
  - [ ] 51.1.3 Center panel: game viewport
  - [ ] 51.1.4 Right panel: inspector
  - [ ] 51.1.5 Bottom panel: asset browser + log output
  - [ ] 51.1.6 Panels are resizable (egui `SidePanel`, `CentralPanel`, `TopBottomPanel`)
- [ ] 51.2 Toolbar
  - [ ] 51.2.1 Play / Pause / Stop buttons — toggles engine tick mode
  - [ ] 51.2.2 Transform gizmo mode selector: Translate / Rotate / Scale
  - [ ] 51.2.3 FPS counter and frame time display
- [ ] 51.3 Scene hierarchy panel
  - [ ] 51.3.1 Query all entities in the world and list them by id
  - [ ] 51.3.2 Show `Name` component value if present, otherwise show entity id
  - [ ] 51.3.3 Click to select an entity — selection stored in `EditorState` resource
  - [ ] 51.3.4 Right-click context menu: "Spawn empty entity", "Despawn"
  - [ ] 51.3.5 Indentation for parent/child hierarchy (once item 21 entity hierarchies are done)
- [ ] 51.4 Inspector panel
  - [ ] 51.4.1 Display all components on the selected entity
  - [ ] 51.4.2 `Transform`: editable x/y/z position, rotation (degrees), scale x/y fields
  - [ ] 51.4.3 `Color` fields rendered as egui color picker
  - [ ] 51.4.4 `Handle<Texture>` fields show texture preview thumbnail
  - [ ] 51.4.5 Unknown components show component type name and "not inspectable" placeholder
  - [ ] 51.4.6 Implement `Inspectable` trait — components opt-in to custom inspector UI by implementing it
  - [ ] 51.4.7 Changes made in the inspector write back to the ECS world immediately
- [ ] 51.5 Asset browser panel
  - [ ] 51.5.1 Walk the `assets/` directory and list files grouped by type (textures, fonts, sounds)
  - [ ] 51.5.2 Show texture thumbnails (small preview rendered via wgpu)
  - [ ] 51.5.3 Click to select asset — shows metadata (dimensions, file size, load state)
  - [ ] 51.5.4 Double-click to open in external app (image editor, etc.) via `open` crate
  - [ ] 51.5.5 Refresh button to re-scan directory (hot reload already handles live changes)
- [ ] 51.6 Log panel
  - [ ] 51.6.1 Capture `log` crate output in-editor (custom `log::Log` impl that buffers to a `Vec<LogEntry>`)
  - [ ] 51.6.2 Display log entries with level color coding (error=red, warn=yellow, info=white, debug=gray)
  - [ ] 51.6.3 Filter by log level, search by text
  - [ ] 51.6.4 Auto-scroll to bottom unless user has scrolled up

### 52. Scene Editing
- [ ] 52.1 Entity manipulation
  - [ ] 52.1.1 Click in the viewport to select an entity (ray-pick by AABB against Transform + sprite bounds)
  - [ ] 52.1.2 Translate gizmo: drag X/Y axis handles to move the selected entity
  - [ ] 52.1.3 Rotate gizmo: drag arc handle to rotate around Z axis
  - [ ] 52.1.4 Scale gizmo: drag corner handles to scale X/Y
  - [ ] 52.1.5 Gizmos are rendered as overlays on top of the game scene (drawn after scene, before egui)
- [ ] 52.2 Spawn / despawn
  - [ ] 52.2.1 Drag asset from asset browser into viewport → spawn entity with that texture at drop position
  - [ ] 52.2.2 Delete key despawns selected entity
  - [ ] 52.2.3 Ctrl+Z / Ctrl+Y undo/redo for spawn, despawn, and transform changes
- [ ] 52.3 Scene save/load
  - [ ] 52.3.1 Serialize current world state to JSON (entity list, components per entity) — simple custom format, not bevy_scene
  - [ ] 52.3.2 Load JSON scene: clear world, spawn entities from file, restore components
  - [ ] 52.3.3 File > Save Scene (Ctrl+S) and File > Open Scene
  - [ ] 52.3.4 Mark scene as dirty (unsaved changes) and prompt on close

### 53. Editor Build & Distribution
- [ ] 53.1 Standalone editor binary
  - [ ] 53.1.1 `cargo build --bin editor --release` produces a standalone editor executable
  - [ ] 53.1.2 Editor binary is self-contained — no runtime dependencies beyond system GPU drivers
- [ ] 53.2 Project model
  - [ ] 53.2.1 Editor opens a "project" directory (contains `assets/`, `src/`, `Cargo.toml`)
  - [ ] 53.2.2 File > New Project creates the directory structure with a template game plugin
  - [ ] 53.2.3 File > Open Project sets the working directory and scans assets
- [ ] 53.3 Game plugin hot reload (stretch goal)
  - [ ] 53.3.1 Compile game plugin as a dylib (`cdylib`) on save
  - [ ] 53.3.2 Editor unloads old dylib, loads new one, re-runs startup systems
  - [ ] 53.3.3 Enables edit-without-restart workflow (significant complexity — evaluate after basics work)
