# setup

## cargo.toml

add `lunar` as a dependency. use the `full` feature to enable 2d, 3d, and audio:

```toml
[dependencies]
lunar = { version = "1", features = ["full"] }
```

available features:

| feature | what it enables |
|---------|----------------|
| `2d` *(default)* | 2d sprite/text rendering, 2d collision |
| `3d` | clustered-forward PBR renderer, BSP/PVS culling, lightmaps |
| `audio` | symphonia decoding, cubeb (native) / WebAudio (WASM) |
| `full` | `2d` + `3d` + `audio` |

## your first game

a lunar game is defined by implementing the `GamePlugin` trait. every game
has exactly one root plugin — it adds systems, resources, and sub-plugins:

```rust
use lunar::prelude::*;

#[derive(Default)]
struct MyGame;

impl GamePlugin for MyGame {
    fn name(&self) -> &str { "MyGame" }

    fn build(&mut self, app: &mut App) {
        app.add_startup_system(setup);
        app.add_system(update);
    }
}

fn setup(mut commands: Commands) {
    // runs once before the game loop starts
}

fn update(time: Res<Time>) {
    // runs every logic tick
}
```

## running the game

the `lunar_app!` macro generates a `main` function that calls `bootstrap`:

```rust
lunar_app!(MyGame);
```

this expands to approximately:

```rust
fn main() {
    lunar::bootstrap::<MyGame>();
}
```

for a 3d game, call `bootstrap_3d` manually instead:

```rust
fn main() {
    lunar::bootstrap_3d::<MyGame>(lunar::lunar_render_3d::RenderConfig3d::default());
}
```

for WASM targets the bootstrap functions are `bootstrap_wasm` and `bootstrap_wasm_3d`.
`lunar_app!` handles all four cases automatically based on the compile target and
enabled features, so most games can just use the macro.

## plugin dependencies

if your game uses multiple plugins, declare dependencies so they build in the right order:

```rust
struct AudioManager;

impl GamePlugin for AudioManager {
    fn name(&self) -> &str { "AudioManager" }
    fn dependencies(&self) -> &[&str] { &["AudioPlugin"] }

    fn build(&mut self, app: &mut App) {
        // AudioPlugin is guaranteed to be built before this runs
        app.add_startup_system(load_sounds);
    }
}
```

the engine resolves plugin build order via topological sort. circular dependencies
log a warning and leave the offending plugins unbuilt.

plugins have two lifecycle methods:
- `build` — add systems, resources, sub-plugins
- `finish` — called after all plugins are built; use for cross-plugin wiring
