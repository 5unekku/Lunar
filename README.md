# Lunar

a 2D game engine built in Rust.

## what it is

- **2D first.** Strict 2D scope. 3D, if it ever exists, will be a sister engine — not an extension. Current code targets the 2D path exclusively.
- **One dependency for game code.** Games depend on `lunar` and nothing else. Backends (windowing, GPU, ECS) are internal implementation details and can be swapped without breaking game code.
- **No `unsafe` required.** Game code never needs `unsafe`. The engine uses `unsafe` only for tightly-scoped, documented optimizations.
- **WASM-compatible.** Compile-time platform selection via `cfg`; the same game code runs on Linux, Windows, macOS, and the web.

## non-goals

- **Audio** — handled by Moonwalker, a separate project. Will return as a crate when mature. Not part of this workspace today.
- **3D** — out of scope. See `plans/design/appendix-c-3d-future.md`.
- **Visual editor** — a downstream project that will consume `lunar`, not part of this repo.

## architecture

- rendering decoupled from game logic — the engine renders `Sprite` / `Text` components automatically; immediate-mode helpers cover HUD and debug overlays
- engine owns all memory; game code holds typed `Handle<T>` references
- fixed tick rate bucketed by frame cap (60 / 120 / 240 Hz); rendering runs uncapped
- bevy_ecs powers the scheduler under the hood — sealed behind the `lunar::prelude` so game code never names it

## crates

| crate | purpose |
|-------|---------|
| `lunar` | public API facade — the one crate game code depends on |
| `lunar-core` | game loop, scheduler, plugin system, time, scene, hierarchy |
| `lunar-render` | wgpu 2D rendering pipeline (internal) |
| `lunar-input` | input handling (internal) |
| `lunar-math` | math types (`Vec2`, `Mat3`, `Transform`, `Color`, `Rect`) |
| `lunar-assets` | handle-based asset server, async loading, hot-reload |
| `lunar-image` | custom image format (zstd-compressed) |
| `lunar-atlas` | texture atlas packer |

## getting started

### native

```bash
cargo run                          # smoke test (opens a window)
cargo run --bin rpg-example        # full RPG-style example
```

### browser (WASM)

```bash
./scripts/build-web.sh             # build wasm + dist/
go run scripts/serve.go            # serve at http://localhost:8080
```

requirements:
- a browser with WebGPU support (Chrome 113+, Firefox Nightly with `dom.webgpu.enabled`)
- `wasm-bindgen-cli` (installed automatically by the build script)
- `go` 1.21+ (for the dev server)

## targets

- Windows 10 / 11
- Linux
- macOS
- Web (WebGPU + WASM)

## minimal game

```rust
use lunar::prelude::*;

#[derive(Default)]
struct MyGame;

impl GamePlugin for MyGame {
    fn name(&self) -> &str { "MyGame" }
    fn build(&mut self, app: &mut App) {
        app.add_startup_system(spawn_player);
        app.add_system(move_player);
    }
}

fn spawn_player(mut commands: Commands, mut assets: ResMut<AssetServer>) {
    let texture = assets.load_texture("player.png");
    commands.spawn((
        Transform::from_xy(0.0, 0.0),
        Sprite::new(texture),
    ));
}

fn move_player(input: Res<InputState>, mut query: Query<&mut Transform, With<Sprite>>) {
    for mut transform in &mut query {
        if input.is_key_held(KeyCode::Right) {
            transform.translation.x += 100.0 * 0.016;
        }
    }
}

lunar::lunar_app!(MyGame);
```

## subsystems

### input

```rust
fn jump(input: Res<InputState>) {
    if input.is_key_just_pressed(KeyCode::Space) {
        // jump!
    }
}
```

### rendering

Game code spawns components; the engine renders them. The render system queries `(Transform, Sprite)` and `(Transform, Text)` each frame and submits draws automatically:

```rust
fn spawn_label(mut commands: Commands, mut assets: ResMut<AssetServer>) {
    let font = assets.load_font("ui.ttf");
    commands.spawn((
        Transform::from_xy(20.0, 35.0),
        Text::new("score: 42", font).with_size(16.0),
    ));
}
```

Immediate-mode helpers cover HUD overlays and debug primitives — useful when the thing you're drawing isn't a persistent entity:

```rust
fn hud_background(mut queue: ResMut<RenderQueue>) {
    queue.draw_rect(Vec2::new(10.0, 10.0), Vec2::new(200.0, 30.0), Color::BLACK);
}
```

Internals (`DrawCommand`, `DrawKind`, `RenderQueue::push`) are hidden — game code never constructs them.

### assets

```rust
fn load(mut assets: ResMut<AssetServer>) {
    let texture = assets.load_texture("textures/player.png");
    if assets.is_texture_ready(&texture) {
        // ready to use
    }
}
```

## plugins

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

The engine resolves plugin dependencies via topological sort.

## project structure

```
lunar/
├── crates/
│   ├── lunar/         # public API facade
│   ├── lunar-core/      # game loop, ECS wiring, plugins
│   ├── lunar-render/    # wgpu rendering
│   ├── lunar-input/     # input handling
│   ├── lunar-math/      # math types
│   ├── lunar-assets/    # asset server
│   ├── lunar-image/     # zstd-compressed image format
│   └── lunar-atlas/     # texture atlas packer
├── plans/                # design documents and implementation TODO
└── src/
    ├── main.rs           # native smoke-test entry point
    └── web.rs            # WASM entry point
```

## contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).
