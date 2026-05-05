# Subsystem APIs

## Design Principles

- Game code **queries** subsystem state, never mutates it directly
- Subsystems expose **read-only views** of their internal state
- Subsystems accept **commands** for operations (queued, not immediate)
- All subsystem state is **inspectable** from outside

## Input API

```rust
/// Input state resource — readable from any system
#[derive(Resource)]
pub struct InputState {
    // internal state, not directly accessible
}

impl InputState {
    /// Check if a key is currently held down
    pub fn is_key_held(&self, key: KeyCode) -> bool;

    /// Check if a key was just pressed this frame
    pub fn is_key_just_pressed(&self, key: KeyCode) -> bool;

    /// Check if a key was just released this frame
    pub fn is_key_just_released(&self, key: KeyCode) -> bool;

    /// Get mouse position in window coordinates
    pub fn mouse_position(&self) -> Vec2;

    /// Get mouse movement delta this frame
    pub fn mouse_delta(&self) -> Vec2;

    /// Check if a mouse button is held
    pub fn is_mouse_button_held(&self, button: MouseButton) -> bool;

    /// Check if a mouse button was just pressed
    pub fn is_mouse_button_just_pressed(&self, button: MouseButton) -> bool;

    /// Get gamepad state (first connected gamepad)
    pub fn gamepad(&self) -> Option<GamepadState>;
}

/// Action mapping system — bind keys to named actions
#[derive(Resource)]
pub struct ActionMap {
    // ...
}

impl ActionMap {
    pub fn bind(&mut self, action: &str, input: InputBinding);
    pub fn is_action_held(&self, action: &str) -> bool;
    pub fn is_action_just_pressed(&self, action: &str) -> bool;
}

// Usage in game code:
fn player_movement(input: Res<InputState>, mut query: Query<&mut Velocity>) {
    for mut vel in query.iter_mut() {
        if input.is_key_held(KeyCode::Left) {
            vel.0.x = -100.0;
        }
    }
}

// Using action maps:
fn player_movement(actions: Res<ActionMap>, mut query: Query<&mut Velocity>) {
    for mut vel in query.iter_mut() {
        if actions.is_action_held("move_left") {
            vel.0.x = -100.0;
        }
    }
}
```

## Render API

Two paths feed the renderer. Game code never calls wgpu and never constructs
`DrawCommand` (which is `#[doc(hidden)]`).

### 1. Component-driven (preferred)

Spawn entities with `Transform` + `Sprite` (or `Text`); the engine's
`auto_sprite_system` / `auto_text_system` query them each frame and enqueue
draws automatically.

```rust
/// Renderable 2D sprite component.
#[derive(Component, Clone, Debug)]
pub struct Sprite {
    pub texture: Handle<Texture>,
    /// None = use the texture's native pixel size at render time.
    pub size: Option<Vec2>,
    pub color: Color,
    /// UV sub-rect for atlas sampling: (uv_min, uv_max) in 0..1 space.
    pub source_rect: Option<(Vec2, Vec2)>,
    /// Pivot in pixels (relative to top-left); None = sprite center.
    pub origin: Option<Vec2>,
    pub layer: i32,
}

impl Sprite {
    pub const fn new(texture: Handle<Texture>) -> Self;
    pub const fn with_size(self, size: Vec2) -> Self;
    pub const fn with_color(self, color: Color) -> Self;
    pub const fn with_layer(self, layer: i32) -> Self;
    pub const fn with_source_rect(self, uv_min: Vec2, uv_max: Vec2) -> Self;
    pub const fn with_origin(self, origin: Vec2) -> Self;
}

/// Renderable text component.
#[derive(Component, Clone, Debug)]
pub struct Text {
    pub content: String,
    pub font: Handle<Font>,
    pub font_size: f32,
    pub color: Color,
    pub layer: i32,
}

impl Text {
    pub fn new(content: impl Into<String>, font: Handle<Font>) -> Self;
    pub const fn with_size(self, font_size: f32) -> Self;
    pub const fn with_color(self, color: Color) -> Self;
    pub const fn with_layer(self, layer: i32) -> Self;
}
```

### 2. Immediate mode (HUD / debug / one-shots)

`RenderQueue` is the imperative escape hatch — useful when the thing you're
drawing isn't a persistent entity:

```rust
/// Render queue resource — internal command buffer with public draw helpers.
#[derive(Resource)]
pub struct RenderQueue { /* fields hidden */ }

impl RenderQueue {
    pub fn draw_sprite(&mut self, texture: &Handle<Texture>, position: Vec2, size: Vec2);
    pub fn draw_sprite_on_layer(&mut self, texture: &Handle<Texture>, position: Vec2, size: Vec2, layer: i32);
    pub fn draw_sprite_atlas(&mut self, texture: &Handle<Texture>, position: Vec2, size: Vec2, region: (Vec2, Vec2));
    pub fn draw_sprite_transformed(&mut self, texture: &Handle<Texture>, params: SpriteParams);
    pub fn draw_rect(&mut self, position: Vec2, size: Vec2, color: Color);
    pub fn draw_line(&mut self, start: Vec2, end: Vec2, color: Color, thickness: f32);
    pub fn draw_text(&mut self, font: &Handle<Font>, content: &str, position: Vec2, font_size: f32, color: Color);
    pub fn clear_color(&mut self, color: Color);
    pub fn set_target(&mut self, target: Option<u32>);

    // internal — hidden from rustdoc; reserved for the engine's render system.
    #[doc(hidden)] pub fn push(&mut self, command: DrawCommand);
    #[doc(hidden)] pub fn commands(&self) -> &[DrawCommand];
}

/// Camera resource — controls what the render queue renders.
/// Optional: if not present, rendering is anchored at world origin (screen-space).
/// Needed for scrolling worlds (mario, contra). Not needed for fixed-screen games (galaga, pacman).
#[derive(Resource)]
pub struct Camera {
    pub position: Vec2,
    pub zoom: f32,
    pub rotation: f32,
    pub viewport: Option<Rect>,
}

/// Render configuration — readable but not mutable from game code
#[derive(Resource)]
pub struct RenderInfo {
    pub window_size: Vec2,
    pub fps: f32,
    pub frame_time_ms: f32,
    pub draw_calls: u32,
    pub sprite_count: u32,
}
```

## Audio API

> **Status:** deferred. The audio engine (Moonwalker) is a separate project and
> not currently part of this workspace. The spec below is the target API contract
> for when it is wired in via a reintroduced `engine-audio` crate. Until then,
> nothing in `lunar` exposes audio.

```rust
/// Audio engine resource
#[derive(Resource)]
pub struct AudioEngine {
    // internal state
}

impl AudioEngine {
    /// Play a sound effect (fire-and-forget)
    pub fn play_sound(&self, sound: &SoundHandle, volume: f32, pitch: f32);

    /// Play a sound effect and return a handle to control it
    pub fn play_sound_controlled(&self, sound: &SoundHandle) -> SoundInstanceHandle;

    /// Set master volume
    pub fn set_master_volume(&self, volume: f32);

    /// Get master volume
    pub fn master_volume(&self) -> f32;

    /// Play music (loops by default)
    pub fn play_music(&self, sound: &SoundHandle, volume: f32);

    /// Stop music
    pub fn stop_music(&self);

    /// Fade music to target volume over time
    pub fn fade_music(&self, target_volume: f32, duration: f32);
}

/// Controlled sound instance
pub struct SoundInstanceHandle;

impl SoundInstanceHandle {
    pub fn set_volume(&self, volume: f32);
    pub fn set_pitch(&self, pitch: f32);
    pub fn stop(&self);
    pub fn is_playing(&self) -> bool;
}
```

## Time API

```rust
#[derive(Resource)]
pub struct Time {
    // ...
}

impl Time {
    /// Delta time in seconds (scaled)
    pub fn delta_seconds(&self) -> f32;

    /// Raw delta time (unscaled)
    pub fn raw_delta_seconds(&self) -> f32;

    /// Time scale (1.0 = normal, 0.5 = half speed, 0.0 = paused)
    pub fn time_scale(&self) -> f32;

    /// Set time scale
    pub fn set_time_scale(&mut self, scale: f32);

    /// Total elapsed time since game start
    pub fn elapsed_seconds(&self) -> f32;

    /// Current frame number
    pub fn frame_count(&self) -> u64;
}
```

---

[← Back to Handle System](03-handle-system.md) | [Next: World and Zone Management →](05-world-zones.md)
