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
- [ ] 1.1 Integrate bevy_ecs World into engine-core
  - [ ] 1.1.1 Add bevy_ecs dependency to engine-core
  - [ ] 1.1.2 Create World wrapper in engine-core
  - [ ] 1.1.3 Re-export World through engine-api
  - [ ] 1.1.4 Re-export Event, EventReader, EventWriter
  - [ ] 1.1.5 Re-export With/Without query filters
- [ ] 1.2 Implement Schedule system
  - [ ] 1.2.1 Schedule struct with system registration
  - [ ] 1.2.2 System trait for ECS functions
  - [ ] 1.2.3 System execution loop
  - [ ] 1.2.4 System ordering (registration order by default)
  - [ ] 1.2.5 Define UpdateStage enum (Input, Physics, Update, Render)
  - [ ] 1.2.6 Define StageOrder enum (Before, After, Between)
- [ ] 1.3 Commands system
  - [ ] 1.3.1 Commands struct with spawn/despawn/entity access
  - [ ] 1.3.2 Command queue deferred execution
  - [ ] 1.3.3 Re-export through engine-api
  - [ ] 1.3.4 commands.entity() → EntityCommands builder

### 2. Plugin System
- [ ] 2.1 GamePlugin trait
  - [ ] 2.1.1 Define trait with build() and finish()
  - [ ] 2.1.2 PluginDependencies trait for dependency declaration
- [ ] 2.2 App builder
  - [ ] 2.2.1 App struct with World, Schedule, plugins
  - [ ] 2.2.2 add_plugin(), add_system(), add_startup_system()
  - [ ] 2.2.3 insert_resource(), world_mut()
  - [ ] 2.2.4 run() method that starts game loop
  - [ ] 2.2.5 Topological sort by plugin dependencies
- [ ] 2.3 Built-in engine plugins
  - [ ] 2.3.1 LogPlugin (env_logger init)
  - [ ] 2.3.2 TimePlugin (delta time tracking)
  - [ ] 2.3.3 InputPlugin (SDL3 input setup)
  - [ ] 2.3.4 RenderPlugin (wgpu setup)
  - [ ] 2.3.5 AudioPlugin (stub for now)

### 3. System Scheduling
- [ ] 3.1 Registration order execution
  - [ ] 3.1.1 Default schedule runs systems in add order
  - [ ] 3.1.2 Startup systems run once before main loop
- [ ] 3.2 Optional stage system
  - [ ] 3.2.1 StageLabel type
  - [ ] 3.2.2 Built-in stages: Input, Physics, Update, Render
  - [ ] 3.2.3 add_system_to_stage()
  - [ ] 3.2.4 add_stage() with ordering (Before/After/Between)
  - [ ] 3.2.5 Custom stage support

---

## Phase 2: Subsystem Implementation

### 4. Time System
- [ ] 4.1 Time resource
  - [ ] 4.1.1 Time struct with delta_seconds, elapsed_seconds
  - [ ] 4.1.2 time_scale, set_time_scale
  - [ ] 4.1.3 frame_count
  - [ ] 4.1.4 raw_delta_seconds (unscaled)
- [ ] 4.2 TimePlugin integration
  - [ ] 4.2.1 Update Time each frame in game loop
  - [ ] 4.2.2 Insert as resource in App

### 5. Input System
- [ ] 5.1 InputState resource
  - [ ] 5.1.1 SDL3 event polling integration
  - [ ] 5.1.2 is_key_held(), is_key_just_pressed(), is_key_just_released()
  - [ ] 5.1.3 mouse_position(), mouse_delta()
  - [ ] 5.1.4 mouse button methods
  - [ ] 5.1.5 gamepad() method (stub for now)
- [ ] 5.2 KeyCode and MouseButton enums
  - [ ] 5.2.1 Map SDL3 keycodes to engine KeyCode
  - [ ] 5.2.2 Map SDL3 buttons to MouseButton
- [ ] 5.3 ActionMap resource (optional convenience)
  - [ ] 5.3.1 bind(action, InputBinding)
  - [ ] 5.3.2 is_action_held(), is_action_just_pressed()
  - [ ] 5.3.3 InputBinding enum (Key, MouseButton, GamepadButton, etc.)
- [ ] 5.4 InputPlugin
  - [ ] 5.4.1 Initialize SDL3 input subsystem
  - [ ] 5.4.2 Update InputState each frame from events

### 6. Render System
- [ ] 6.1 RenderQueue resource
  - [ ] 6.1.1 Internal command buffer
  - [ ] 6.1.2 draw_sprite(texture, position, size)
  - [ ] 6.1.3 draw_sprite_transformed(position, size, rotation, origin, color)
  - [ ] 6.1.4 draw_rect(rect, color)
  - [ ] 6.1.5 draw_line(start, end, color, thickness)
  - [ ] 6.1.6 draw_text(font, text, position, size, color)
  - [ ] 6.1.7 clear(color)
  - [ ] 6.1.8 set_target(render_target)
- [ ] 6.2 Sprite rendering backend
  - [ ] 6.2.1 Texture loading from Handle<Texture>
  - [ ] 6.2.2 Batch sprite rendering (single draw call per texture)
  - [ ] 6.2.3 Orthographic projection matrix
  - [ ] 6.2.4 Vertex/instance buffers for sprites
- [ ] 6.3 Text rendering
  - [ ] 6.3.1 Font loading
  - [ ] 6.3.2 Glyph rasterization (glyph_brain or similar)
  - [ ] 6.3.3 Text layout and rendering to sprites
- [ ] 6.4 Camera resource
  - [ ] 6.4.1 Camera with position, zoom, rotation, viewport
  - [ ] 6.4.2 Camera affects render queue output
- [ ] 6.5 RenderInfo resource
  - [ ] 6.5.1 window_size, fps, frame_time_ms
  - [ ] 6.5.2 draw_calls, sprite_count
- [ ] 6.6 RenderPlugin
  - [ ] 6.6.1 Process RenderQueue each render stage
  - [ ] 6.6.2 Submit to wgpu

### 7. Audio System
- [ ] 7.1 AudioEngine resource
  - [ ] 7.1.1 play_sound(handle, volume, pitch) — fire-and-forget
  - [ ] 7.1.2 play_sound_controlled() → SoundInstanceHandle
  - [ ] 7.1.3 play_music(handle, volume)
  - [ ] 7.1.4 stop_music(), fade_music()
  - [ ] 7.1.5 set_master_volume(), master_volume()
- [ ] 7.2 SoundInstanceHandle
  - [ ] 7.2.1 set_volume(), set_pitch(), stop(), is_playing()
- [ ] 7.3 AudioPlugin
  - [ ] 7.3.1 Initialize audio backend (miniaudio or similar)
  - [ ] 7.3.2 Process audio commands each frame

---

## Phase 3: Asset System

### 8. Asset Server
- [ ] 8.1 Handle<T> system
  - [ ] 8.1.1 Handle struct with id, generation, PhantomData
  - [ ] 8.1.2 Asset trait (replaces ResourceMarker)
  - [ ] 8.1.3 Concrete handle types (TextureHandle, SoundHandle, etc.)
  - [ ] 8.1.4 Clone, Copy, PartialEq, Eq, Hash derives
- [ ] 8.2 AssetStore<T> internals
  - [ ] 8.2.1 entries Vec with generation tracking
  - [ ] 8.2.2 ref_count for handle lifetime
  - [ ] 8.2.3 load_state enum (Loading, Loaded, Failed)
  - [ ] 8.2.4 path_index HashMap for deduplication
- [ ] 8.3 AssetServer resource
  - [ ] 8.3.1 load<T>(path) → Handle<T> (returns immediately)
  - [ ] 8.3.2 load_batch<T>(paths) → Vec<Handle<T>>
  - [ ] 8.3.3 is_loaded<T>(handle) → bool
  - [ ] 8.3.4 is_ready<T>(handle) → bool
  - [ ] 8.3.5 get_info<T>(handle) → Option<&AssetInfo>
  - [ ] 8.3.6 wait_for_all() (blocking)
  - [ ] 8.3.7 loading_count() → usize
- [ ] 8.4 Async loading architecture
  - [ ] 8.4.1 IoTaskPool for file I/O
  - [ ] 8.4.2 AssetLoaders map (extension → loader)
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
- [ ] 9.1 Transform component
  - [ ] 9.1.1 translation: Vec3, rotation: f32, scale: Vec2
  - [ ] 9.1.2 from_translation(), from_xy() constructors
- [ ] 9.2 Color type
  - [ ] 9.2.1 RGBA struct with const presets
  - [ ] 9.2.2 rgb(), rgba() constructors
- [ ] 9.3 Rect type
  - [ ] 9.3.1 x, y, w, h struct
  - [ ] 9.3.2 contains(), intersects() methods

### 10. App.run() Integration
- [ ] 9.1 Wire App.run() to existing GameLoop
  - [ ] 9.1.1 App.run() creates GameLoop
  - [ ] 9.1.2 GameLoop.tick() drives Schedule.execute()
  - [ ] 9.1.3 Handle frame cap sleep
- [ ] 9.2 Event processing
  - [ ] 9.2.1 SDL3 event pump in game loop
  - [ ] 9.2.2 Forward events to InputPlugin
  - [ ] 9.2.3 Handle quit event
- [ ] 9.3 Render loop
  - [ ] 9.3.1 begin_frame() before render stage
  - [ ] 9.3.2 present() after render stage
  - [ ] 9.3.3 Handle surface texture errors

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
- [ ] 11.1 Zone trait
  - [ ] 11.1.1 on_load(), on_enter(), on_exit()
  - [ ] 11.1.2 transitions() → Vec<ZoneTransition>
- [ ] 11.2 WorldManager resource
  - [ ] 11.2.1 register_zone(), enter_zone()
  - [ ] 11.2.2 current_zone(), world_data()
  - [ ] 11.2.3 Zone transition with fade support
- [ ] 11.3 ZoneTransition
  - [ ] 11.3.1 trigger_area, target_zone, spawn_position
  - [ ] 11.3.2 fade config (duration, color)

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
- [ ] 14.2 Runtime system
  - [ ] 14.2.1 Dialogue resource
  - [ ] 14.2.2 Speaker ID system (string → numeric)
  - [ ] 14.2.3 Multi-stage text support
  - [ ] 14.2.4 Branching choices
  - [ ] 14.2.5 Sprite/emotion triggers during dialogue
  - [ ] 14.2.6 Narrator text (no speaker)
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
- [ ] 15.1 Build configuration
  - [ ] 15.1.1 Add wasm32-unknown-unknown target support
  - [ ] 15.1.2 Conditional compilation gates
  - [ ] 15.1.3 Feature flags for native vs web
- [ ] 15.2 Web-compatible async
  - [ ] 15.2.1 Replace tokio with web-compatible async for wasm
  - [ ] 15.2.2 No std::thread on wasm
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
- [ ] 15.6 Web build tooling
  - [ ] 15.6.1 wasm-pack or trunk integration
  - [ ] 15.6.2 HTML template with canvas
  - [ ] 15.6.3 Build script for web target

---

## Phase 9: Polish & Extras

### 16. Extensibility
- [ ] 16.1 Custom render passes
  - [ ] 16.1.1 RenderPass trait
  - [ ] 16.1.2 add_render_pass()
- [ ] 16.2 Custom asset loaders
  - [ ] 16.2.1 register_asset_loader(extension, loader)
- [ ] 16.3 Engine forking
  - [ ] 16.3.1 Ensure loose coupling between crates
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
