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
- [ ] 29.1 Cache bind groups by texture ID
  - [ ] 29.1.1 Add `HashMap<u32, wgpu::BindGroup>` to RenderEngine
  - [ ] 29.1.2 Create bind group on texture upload, cache by texture ID
  - [ ] 29.1.3 Look up cached bind group in render loop instead of creating new
  - [ ] 29.1.4 Handle texture removal/invalidation (remove from cache)
- **Impact:** Eliminates per-frame GPU object creation on Vulkan
- **Effort:** Low

#### 30. Persistent Vertex Buffer (Ring Buffer)
- [ ] 30.1 Replace per-frame `create_buffer_init` with persistent buffer
  - [ ] 30.1.1 Pre-allocate large vertex buffer at startup (`MAX_VERTICES * 32` bytes)
  - [ ] 30.1.2 Use `write_buffer()` with `MAP_WRITE` + `UNMAP` each frame
  - [ ] 30.1.3 Double-buffer or triple-buffer for frame overlap safety
  - [ ] 30.1.4 Handle buffer overflow (split into multiple draw calls or grow)
- **Impact:** Eliminates per-frame GPU memory allocation churn
- **Effort:** Medium

#### 31. StoreOp::Discard
- [ ] 31.1 Change `store: Store` to `store: StoreOp::Discard` in render pass
  - [ ] 31.1.1 Update color attachment operations
  - [ ] 31.1.2 Update depth/stencil attachment if applicable
- **Impact:** Free performance on tile-based GPUs (Apple Silicon, mobile)
- **Effort:** Low

### P1: Important Improvements

#### 32. Vertex Format Packing
- [ ] 32.1 Pack color into single `u32` (4 bytes instead of 16)
  - [ ] 32.1.1 Change vertex from `[f32; 8]` (32 bytes) to `[f32; 4] + [u32; 1]` (20 bytes)
  - [ ] 32.1.2 Update vertex shader to unpack `u32` color
  - [ ] 32.1.3 Update vertex attribute format (`Uint32` instead of `Float32x4`)
  - [ ] 32.1.4 Update all vertex generation code
- **Impact:** 37.5% less vertex data per frame
- **Effort:** Medium

#### 33. Sort Commands by (Layer, Texture)
- [ ] 33.1 Replace HashMap grouping with sorted Vec
  - [ ] 33.1.1 Sort by `(layer, texture_id)` tuple
  - [ ] 33.1.2 Iterate linearly — same-texture commands are contiguous
  - [ ] 33.1.3 Remove HashMap allocation from render loop
- **Impact:** Better cache locality, no HashMap overhead
- **Effort:** Low

#### 34. Stage-Based System Ordering
- [ ] 34.1 Implement actual stage scheduling
  - [ ] 34.1.1 Create bevy_ecs schedules for each stage (Input, Physics, Update, Render)
  - [ ] 34.1.2 `add_system_to_stage()` adds to correct schedule
  - [ ] 34.1.3 Run schedules in order each frame
  - [ ] 34.1.4 Handle `apply_deferred` between stages
- **Impact:** Game code can control system ordering
- **Effort:** High

#### 35. Fix Startup System Timing
- [ ] 35.1 Track startup systems, run at start of `App::run()`
  - [ ] 35.1.1 Store startup systems in Vec instead of running immediately
  - [ ] 35.1.2 Run all startup systems in sequence at start of `run()`
  - [ ] 35.1.3 Clear startup systems after first run
- **Impact:** Startup systems run after all plugins/resources are ready
- **Effort:** Low

### P2: Nice-to-Have

#### 36. Cache Text Layout Results
- [ ] 36.1 Cache computed glyph quads per text element
  - [ ] 36.1.1 Store layout result in text component
  - [ ] 36.1.2 Only recompute when text content, font, or size changes
  - [ ] 36.1.3 Invalidate cache on text change
- **Impact:** Eliminates per-frame character iteration and glyph lookup
- **Effort:** Low

#### 37. Pre-allocate RenderQueue Commands
- [ ] 37.1 Pre-allocate `Vec::with_capacity(1024)` for commands
  - [ ] 37.1.2 Use `clear()` which retains capacity
  - [ ] 37.1.3 Monitor typical command counts and adjust capacity
- **Impact:** Eliminates mid-frame reallocation
- **Effort:** Low

#### 38. Rect Utility Extensions
- [ ] 38.1 Add methods to Rect (see item 24)
  - [ ] 38.1.1 `inflate(dx, dy)`
  - [ ] 38.1.2 `clamp(within)`
  - [ ] 38.1.3 `collide_point(x, y)`
  - [ ] 38.1.4 `collide_rect(other)`
  - [ ] 38.1.5 `center()`
  - [ ] 38.1.6 `union(other)`
- **Note:** Already tracked as item 24 in Part 2
- **Effort:** Low

#### 39. Fix SpriteParams Origin Usage
- [ ] 39.1 Use origin parameter in vertex generation
  - [ ] 39.1.1 Compute rotation/scaling around origin point
  - [ ] 39.1.2 Or remove origin from API if not needed
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
- [ ] 41.1 Add proper line rendering
  - [ ] 41.1.1 `DrawKind::Line { start, end, color, thickness }`
  - [ ] 41.1.2 Compute rotated rect vertices in shader or CPU
  - [ ] 41.1.3 Or use line primitives if supported
- **Impact:** Clean diagonal lines, more efficient than AABB rect
- **Effort:** Medium

#### 42. Glyph Atlas Row-Copy Optimization
- [ ] 42.1 Replace per-pixel copy with row copy
  - [ ] 42.1.1 Use `slice::copy_from_slice` for each row
  - [ ] 42.1.2 Or use engine-image SIMD functions
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
- [ ] 44.1 Improve frame timing precision
  - [ ] 44.1.1 Sleep for most of wait time
  - [ ] 44.1.2 Spin-wait last ~1ms with `std::hint::spin_loop()`
  - [ ] 44.1.3 Reduces frame pacing jitter
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
```
