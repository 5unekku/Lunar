# Lunar

a 2D game engine built in Rust.

## stack

- **wgpu** — cross-platform graphics API (Vulkan/DX12/Metal/WebGPU)
- **SDL3** — windowing and input
- **bevy_ecs** — entity component system (standalone)
- **glam** — math library

## architecture

- rendering is decoupled from game logic
- engine owns all memory, game logic operates on handles
- fixed tickrate correlated to frame cap with three buckets:
  - frame cap 1-60: 60hz tick
  - frame cap 61-120: 120hz tick
  - frame cap 121+: 240hz tick
- rendering runs uncapped for smooth high-framerate gameplay

## crates

| crate | purpose |
|---------|
| `engine-core` | game loop, ECS wiring, engine state, command registry, zones, dialogue |
| `engine-render` | wgpu 2D rendering with RenderQueue and DrawCommand |
| `engine-input` | SDL3 input handling with KeyCode, MouseButton, InputState |
| `engine-audio` | audio subsystem stub with AudioPlugin |
| `engine-math` | glam re-exports, Transform, Color, Rect types |
| `engine-api` | public API for game logic |
| `engine-assets` | asset system with Handle<T>, AssetServer, AssetPlugin |

## getting started

```bash
cargo run
```

### running the shooter example

```bash
cargo run --bin shooter
```

## targets

- Windows 7+
- Windows 10/11
- Linux
- macOS
- Web (via WebGPU + WASM)

## building for web

```bash
rustup target add wasm32-unknown
cargo build --target wasm32-unknown-unknown --bin lunar-web
wasm-bindgen --out-dir pkg target/wasm32-unknown-unknown/debug/lunar-web.wasm
```

## plugin system

plugins implement the `GamePlugin` trait and are registered via `App::add_plugin()`.
the engine resolves plugin dependencies using topological sort (Kahn's algorithm).

```rust
struct MyPlugin;

impl GamePlugin for MyPlugin {
    fn name(&self) -> &str { "MyPlugin" }
    fn dependencies(&self) -> &[&str] { &["InputPlugin", "RenderPlugin"] }
    fn build(&mut self, app: &mut App) {
        app.insert_resource(MyResource::new());
        app.add_system(my_system);
    }
}
```

## subsystems

### input

```rust
fn my_system(input: Res<InputState>) {
    if input.is_key_just_pressed(KeyCode::Space) {
        // jump!
    }
}
```

### rendering

```rust
fn render_system(mut queue: ResMut<RenderQueue>) {
    queue.push(DrawCommand {
        entity: 0,
        kind: DrawKind::Rect {
            position: (100.0, 100.0),
            size: (32.0, 32.0),
            color: (1.0, 0.0, 0.0, 1.0),
        },
    });
}
```

### assets

```rust
fn load_system(mut assets: ResMut<AssetServer>) {
    let tex = assets.load_texture("textures/player.png");
    if assets.is_texture_ready(&tex) {
        // use texture
    }
}
```

### world zones

```rust
struct TownZone;

impl Zone for TownZone {
    fn on_enter(&mut self, app: &mut App) {
        // spawn NPCs, set music, etc.
    }
    fn on_exit(&mut self, app: &mut App) {
        // cleanup
    }
}

// register and enter
world_manager.register_zone("town", TownZone);
world_manager.enter_zone("town");
```

### dialogue

```rust
let dialogue = DialogueBuilder::new("start")
    .line("greeting", Some("npc"), "hello traveler!", Some("choice"))
    .choice_line("choice", Some("npc"), "what do you want?", vec![
        ("buy items", "shop"),
        ("leave", "end"),
    ])
    .build();

dialogue_manager.register("town_npc", dialogue);
dialogue_manager.start("town_npc");
```

## project structure

```
lunar/
├── crates/
│   ├── engine-core/      # game loop, ECS, plugins, zones, dialogue
│   ├── engine-render/    # wgpu rendering
│   ├── engine-input/     # SDL3 input
│   ├── engine-audio/     # audio stub
│   ├── engine-math/      # math types
│   ├── engine-api/       # public API
│   └── engine-assets/    # asset system
├── examples/
│   └── shooter/          # top-down shooter example
├── plans/                # design documents
└── src/
    ├── main.rs           # native entry point
    └── web.rs            # WASM entry point
```
