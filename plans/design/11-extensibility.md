# Extensibility

## Custom Components

Game code defines any component by deriving `Component`:

```rust
#[derive(Component)]
struct CustomData {
    // any fields
}
```

No engine registration needed. The ECS handles it.

## Custom Systems

Any function with valid ECS parameters is a system:

```rust
fn my_custom_system(
    query: Query<&MyComponent>,
    time: Res<Time>,
    mut commands: Commands,
) {
    // ...
}
```

## Custom Stages

```rust
const MY_STAGE: StageLabel = StageLabel("my_stage");

app.add_stage(MY_STAGE, StageOrder::Between(UpdateStage::Update, UpdateStage::Render))
   .add_system_to_stage(MY_STAGE, my_custom_system);
```

## Custom Commands

```rust
struct SpawnWaveCommand {
    count: u32,
    position: Vec2,
}

impl Command for SpawnWaveCommand {
    fn execute(&self, args: &[String]) -> Result<String, String> {
        // parse args, spawn entities via world access
        Ok(format!("spawned {} enemies", self.count))
    }

    fn description(&self) -> &str {
        "spawn a wave of enemies"
    }
}

app.register_command("spawn_wave", Box::new(SpawnWaveCommand { ... }));
```

## Custom Asset Types

```rust
// Define a custom asset type
struct TileMapData {
    tiles: Vec<u32>,
    width: u32,
    height: u32,
}

// Implement Asset trait
impl_asset!(TileMapData);

// Register a custom loader
app.register_asset_loader(".tilemap", TileMapLoader);

// Load it
let tilemap: Handle<TileMapData> = assets.load("levels/level1.tilemap");
```

## Custom Render Passes (Advanced)

For games that need custom rendering:

```rust
/// Custom render pass trait
pub trait RenderPass: Send + Sync + 'static {
    /// Called during the render stage
    fn render(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &wgpu::TextureView,
        render_queue: &RenderQueue,
    );
}

// Register a custom render pass
app.add_render_pass(Box::new(MyCustomPass));
```

## Engine Forking Without Modification

Because the engine is a set of crates, games can:

1. Depend on `lunar` for the stable public API
2. Depend on individual engine crates (`engine-render`, `engine-input`) if they need lower-level access
3. Fork and replace any engine crate without affecting others (loose coupling)

## Fork Points

Each crate in the workspace is an independent fork point. the table below shows what you can replace and what depends on it.

### Crate-level fork points

| crate | purpose | depends on | depended by | fork to replace |
|-------|---------|------------|-------------|-----------------|
| `engine-math` | types (Vec2, Color, Transform) and macros | glam | everything | custom math types or different vector library |
| `engine-image` | image decoding (png, jpeg, webp, etc.) | none | engine-assets | different decoder (stb_image, image-rs, custom format) |
| `engine-assets` | async asset loading, handles, hot reloading | engine-image, notify | engine-render, engine-audio | custom asset pipeline, different io strategy |
| `engine-render` | wgpu 2d rendering, sprite batching, text | wgpu, engine-assets, engine-math | engine-core | different renderer (opengl, software, 3d) |
| `engine-input` | sdl3 / web input handling | sdl3 (native), web-sys (web), engine-math | engine-core | different input backend (glfw, raw x11, custom) |
| `engine-audio` | audio playback (stub) | none | engine-core | miniaudio, cpal, rodio, or any audio library |
| `engine-core` | game loop, app builder, ecs wiring | bevy_ecs, all engine-* crates | lunar binary | different ecs, different game loop strategy |
| `lunar` | re-exports and stable public api | engine-math, bevy_ecs | game projects | custom api surface for your game |

### Recommended fork strategies

**1. drop-in replacement (same api)**

fork a crate, keep the same public api, and swap it via `[patch.crates-io]` or a path override in your game's `Cargo.toml`. no code changes needed in dependent crates.

```toml
# in your game's Cargo.toml
[patch.crates-io]
engine-render = { path = "../my-engine-fork/engine-render" }
```

**2. api extension (additive changes)**

add new methods or types to a forked crate without removing existing ones. dependent crates continue to work. new features are opt-in.

**3. full replacement (breaking changes)**

replace a crate entirely and update all dependents. this is the most invasive option. recommended only when the forked crate's api is fundamentally different.

### Platform-specific forks

the engine uses `cfg(target_arch)` to separate native and web code paths:

- **native**: `engine-input` uses sdl3, `engine-render` uses wgpu with native surface
- **web**: `engine-input` uses web-sys events, `engine-render` uses wgpu with webgpu canvas

to add a new platform, fork `engine-input` and `engine-render` and add a new `cfg` target. the rest of the engine is platform-agnostic.

### What NOT to fork

- `bevy_ecs` — the ecs is the core abstraction. fork the engine crates that use it, not bevy_ecs itself.
- `engine-core` game loop — the `App` builder and `GameLoop` are designed to work together. fork individual plugins instead.

---

[← Back to Error Handling](10-error-handling.md) | [Next: Crate Dependency Graph →](12-dependency-graph.md)
