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

1. Depend on `engine-api` for the stable public API
2. Depend on individual engine crates (`engine-render`, `engine-input`) if they need lower-level access
3. Fork and replace any engine crate without affecting others (loose coupling)

---

[← Back to Error Handling](10-error-handling.md) | [Next: Crate Dependency Graph →](12-dependency-graph.md)
