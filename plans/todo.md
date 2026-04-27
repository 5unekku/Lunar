# Lunar Engine — Implementation TODO

> Track all features needed to reach the shooter example and beyond.
> Items can only be worked on when all dependencies are checked off.

## Legend
- [ ] Not started
- [-] In progress
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
- [-] 2.3 Built-in engine plugins
  - [ ] 2.3.1 LogPlugin (env_logger init) — handled by user code, not a plugin
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
- [ ] 5.3 ActionMap resource (optional convenience)
  - [ ] 5.3.1 bind(action, InputBinding)
  - [ ] 5.3.2 is_action_held(), is_action_just_pressed()
  - [ ] 5.3.3 InputBinding enum (Key, MouseButton, GamepadButton, etc.)
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
- [ ] 6.2 Sprite rendering backend
  - [ ] 6.2.1 Texture loading from Handle<Texture>
  - [ ] 6.2.2 Batch sprite rendering (single draw call per texture)
  - [ ] 6.2.3 Orthographic projection matrix
  - [ ] 6.2.4 Vertex/instance buffers for sprites
  - note: PNG/JPG are starting formats — more efficient packed formats (QOI, KTX2, custom) TBD later
- [ ] 6.3 Text rendering
  - note: must bundle fonts with the game — no system font reliance (poor intersection across win/linux/mac/web)
  - note: fonts are ttf/otf, linked statically or shipped alongside the binary
  - note: fontdue is the preferred rasterizer (pure Rust, no system deps, WASM compatible)
  - [ ] 6.3.1 Font loading (ttf/otf via fontdue)
  - [ ] 6.3.2 Glyph rasterization → CPU-side bitmap, uploaded to a GPU atlas texture
  - [ ] 6.3.3 Text layout (simple left-to-right for now, baseline alignment)
  - [ ] 6.3.4 Render text as UV-mapped quads from the glyph atlas
- [ ] 6.4 Camera resource (optional — not all games need it)
  - note: games like galaga/pacman use no camera; mario/contra need one — engine must work both ways
  - [ ] 6.4.1 Camera with position, zoom, rotation, viewport
  - [ ] 6.4.2 Camera affects render queue output (offset projection matrix)
  - [ ] 6.4.3 When no Camera resource exists, render is world-space anchored at origin
- [ ] 6.5 RenderInfo resource
  - [ ] 6.5.1 window_size, fps, frame_time_ms
  - [ ] 6.5.2 draw_calls, sprite_count
- [x] 6.6 RenderPlugin
  - [x] 6.6.1 Process RenderQueue each render stage
  - [x] 6.6.2 Submit to wgpu (RenderEngine::new + begin_frame/present exist)

### 7. Audio System
- note: audio is NOT a current requirement — Moonwalker (custom audio engine, cpal-based, WASM compatible) will integrate here
- note: AudioPlugin stays as a stub until Moonwalker is ready to wire in
- [x] 7.1 AudioEngine resource (stub — filled in by Moonwalker later)
  - [x] 7.1.1 play_sound(handle, volume, pitch) — fire-and-forget
  - [ ] 7.1.2 play_sound_controlled() → SoundInstanceHandle
  - [ ] 7.1.3 play_music(handle, volume)
  - [ ] 7.1.4 stop_music(), fade_music()
  - [ ] 7.1.5 set_master_volume(), master_volume()
- [ ] 7.2 SoundInstanceHandle
  - [ ] 7.2.1 set_volume(), set_pitch(), stop(), is_playing()
- [x] 7.3 AudioPlugin
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
- [-] 8.4 Async loading architecture
  - [ ] 8.4.1 IoTaskPool for file I/O
  - [x] 8.4.2 AssetLoaders map (extension → loader) — AssetLoader trait added
  - [ ] 8.4.3 TextureLoader (png, jpg, bmp, webp, gif)
  - [ ] 8.4.4 SoundLoader (wav, ogg, mp3, flac)
  - [ ] 8.4.5 FontLoader (ttf, otf)
  - [ ] 8.4.6 Loading flow: load → I/O task → parse → store → ready
- [ ] 8.5 Asset paths
  - [ ] 8.5.1 Resolve relative to game's assets/ directory
  - [ ] 8.5.2 Handle both "path" and "./path" formats
- [ ] 8.6 Hot reloading (dev only)
  - [ ] 8.6.1 AssetWatcher resource
  - [ ] 8.6.2 File watching with notify crate
  - [ ] 8.6.3 Auto-reload on file change
- [ ] 8.7 impl_asset! macro
  - [ ] 8.7.1 Macro to implement Asset trait only

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
- [ ] 9.2 Event processing
  - [ ] 9.2.1 SDL3 event pump in game loop (exists in src/main.rs but not wired to App)
  - [ ] 9.2.2 Forward events to InputPlugin
  - [ ] 9.2.3 Handle quit event
- [x] 9.3 Render loop
  - [x] 9.3.1 begin_frame() before render stage (RenderEngine::begin_frame exists)
  - [x] 9.3.2 present() after render stage (RenderEngine::present exists)
  - [x] 9.3.3 Handle surface texture errors

### 10. lunar_app! Macro
- [ ] 10.1 Basic macro
  - [ ] 10.1.1 Expands to async main
  - [ ] 10.1.2 SDL3 window creation
  - [ ] 10.1.3 Add built-in plugins
  - [ ] 10.1.4 Add game plugin
  - [ ] 10.1.5 Call app.run()
- [ ] 10.2 Config variant
  - [ ] 10.2.1 Accept config expression
  - [ ] 10.2.2 Pass config to plugins

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
- [ ] 12.1 Scene trait
  - [ ] 12.1.1 on_enter(), on_update(), on_exit()
- [ ] 12.2 SceneManager resource
  - [ ] 12.2.1 switch_to(), current_scene()
  - [ ] 12.2.2 push_overlay(), pop_overlay()

---

## Phase 6: Error Handling

### 13. Error System
- [ ] 13.1 EngineError enum
  - [ ] 13.1.1 WindowCreation, GpuInit, AssetLoad
  - [ ] 13.1.2 InvalidHandle, SceneNotFound, Command
- [ ] 13.2 ErrorEvent
  - [ ] 13.2.1 ErrorEvent with source, error, recovered
  - [ ] 13.2.2 ErrorSource enum
  - [ ] 13.2.3 EventReader for game code
- [ ] 13.3 Result types
  - [ ] 13.3.1 EngineResult<T>
  - [ ] 13.3.2 AssetResult<T>
- [ ] 13.4 Panic strategy
  - [ ] 13.4.1 Panic on fatal errors
  - [ ] 13.4.2 Catch game code panics, report as errors

---

## Phase 7: Dialogue System (Design TBD)

### 14. Dialogue System
- [ ] 14.1 Authoring format (TBD)
  - [ ] 14.1.1 Decide: custom DSL vs structured data vs other
  - [ ] 14.1.2 Compiler to binary format
- [x] 14.2 Runtime system
  - [x] 14.2.1 Dialogue resource (DialogueManager)
  - [x] 14.2.2 Speaker ID system (string → numeric) — speaker: Option<String>
  - [x] 14.2.3 Multi-stage text support (DialogueLine, DialogueNode)
  - [x] 14.2.4 Branching choices (DialogueChoice)
  - [x] 14.2.5 Sprite/emotion triggers during dialogue (sprite_change field)
  - [x] 14.2.6 Narrator text (no speaker) — speaker: Option<String>
- [ ] 14.3 Text rendering integration
  - [ ] 14.3.1 Textbox component
  - [ ] 14.3.2 Font integration
  - [ ] 14.3.3 Text animation (typewriter effect, etc.)
- [ ] 14.4 Localization
  - [ ] 14.4.1 Language selection
  - [ ] 14.4.2 Per-language dialogue files

---

## Phase 8: Web/WASM Support

### 15. WASM Target
- [-] 15.1 Build configuration
  - [x] 15.1.1 Add wasm32-unknown-unknown target support (target in Cargo.toml, build script exists)
  - [x] 15.1.2 Conditional compilation gates (cfg(not(target_arch = "wasm32")) in engine-input)
  - [ ] 15.1.3 Feature flags for native vs web
- [-] 15.2 Web-compatible async
  - [x] 15.2.1 Replace tokio with web-compatible async for wasm (src/web.rs uses wasm_bindgen)
  - [x] 15.2.2 No std::thread on wasm
  - [ ] 15.2.3 Use async task pools
- [ ] 15.3 WebGPU surface
  - [ ] 15.3.1 Canvas element instead of SDL3 window
  - [ ] 15.3.2 WebGPU surface creation for wasm
  - [ ] 15.3.3 Request adapter/device for web
- [ ] 15.4 Web input
  - [ ] 15.4.1 SDL3 Emscripten input or web-specific handling
  - [ ] 15.4.2 Keyboard, mouse, gamepad on web
- [ ] 15.5 Web asset loading
  - [ ] 15.5.1 Fetch API instead of file I/O
  - [ ] 15.5.2 Bundled assets at compile time
  - [ ] 15.5.3 Asset bundles for web distribution
- [x] 15.6 Web build tooling
  - [x] 15.6.1 wasm-pack or trunk integration (wasm-bindgen-cli in build script)
  - [x] 15.6.2 HTML template with canvas
  - [x] 15.6.3 Build script for web target (scripts/build-web.sh)

---

## Phase 9: Polish & Extras

### 16. Extensibility
- [ ] 16.1 Custom render passes
  - [ ] 16.1.1 RenderPass trait
  - [ ] 16.1.2 add_render_pass()
- [ ] 16.2 Custom asset loaders
  - [ ] 16.2.1 register_asset_loader(extension, loader)
- [-] 16.3 Engine forking
  - [x] 16.3.1 Ensure loose coupling between crates (workspace with separate crates)
  - [ ] 16.3.2 Document fork points

### 17. Macros & Convenience
- [ ] 17.1 transform! macro
- [ ] 17.2 color! macro
- [ ] 17.3 rect! macro
- [ ] 17.4 query! macro (optional)

### 18. 3D Future Compatibility
- [ ] 18.1 Mesh component
  - [ ] 18.1.1 Vertex/index buffers
  - [ ] 18.1.2 Material component
- [ ] 18.2 Light component
- [ ] 18.3 3D render pass
  - [ ] 18.3.1 Alongside 2D render pass
  - [ ] 18.3.2 Perspective projection

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
