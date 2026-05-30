# Lunar

a 2D and 3D game engine built in Rust, designed around speed, portability, and a
small, friendly public API.

## what it is

- **2D and 3D.** Both are first-class. The 2D path (`lunar-2d` / `lunar-render`) and
  the 3D path (`lunar-3d` / `lunar-render-3d`) share the same core, ECS, asset
  pipeline, and game loop. A game pulls in only the dimension it uses via feature flags.
- **Built for low-spec machines.** The engine front-loads expensive work offline
  (BSP/PVS visibility, lightmap baking, LOD generation, texture compression, vertex
  quantization, SPIR-V precompilation) so the runtime frame budget is spent on
  geometry that is actually visible. Target: 60fps on a 2015 mid-range CPU.
- **One dependency for game code.** Games depend on `lunar` and nothing else.
  Backends (windowing via SDL3, GPU via wgpu, ECS via bevy_ecs) are internal details,
  never named in a game's `Cargo.toml`. A compile-time test (`tests/api_seal`) enforces this.
- **Game code never needs `unsafe`.** Inside the engine, `unsafe` is used sparingly and
  only where it is a genuine performance win (e.g. NEON pixel processing), with
  documented safety invariants. Elsewhere, the safe path is preferred.
- **Crossplatform and multiarch.** The same game code runs on Linux, Windows, macOS, and
  the web (WebGPU + WASM). `scripts/build_all.go` cross-compiles 12 target triples.

## non-goals

- **Audio** — handled by Moonwalker, a separate project. Will return as a crate when
  mature. Not part of this workspace today.
- **Visual editor** — a downstream project that will consume `lunar`, not part of this repo.

## architecture

- **rendering decoupled from game logic** — the engine renders `Sprite` / `Text` (2D) and
  `Mesh3d` / light / camera components (3D) automatically; immediate-mode helpers cover
  HUD and debug overlays
- **engine owns all memory** — game code holds typed `Handle<T>` references; the engine
  evicts CPU-side mesh/texture data after GPU upload
- **fixed tick rate decoupled from frame cap** — logic ticks at a fixed 30 / 60 / 120 / 240 Hz
  (accumulator-based, capped at 5 ticks/frame); rendering runs uncapped or at a chosen
  frame cap, with a render interpolation alpha for smooth sub-tick motion
- **parallel by default** — non-conflicting ECS systems run concurrently on a thread pool
  (single-threaded fallback on WASM)
- **bevy_ecs under the hood** — sealed behind `lunar::prelude` so game code never names it

## crates

Game code depends only on `lunar`. The rest are internal or opt-in.

**core**

| crate | purpose |
|-------|---------|
| `lunar` | public API facade — the one crate game code depends on |
| `lunar-core` | game loop, scheduler, plugin system, time, scene, hierarchy, save/load, pooling |
| `lunar-math` | math types (`Vec2/3/4`, `Mat2/3/4`, `Quat`, `Transform`, `Color`, `Rect`) |
| `lunar-input` | keyboard, mouse, gamepad, action maps (internal) |
| `lunar-macros` | derive macros (`Component`, `Resource`, `Event`, …) and the `texture!` macro |

**rendering**

| crate | purpose |
|-------|---------|
| `lunar-2d` / `lunar-render` | 2D ECS components + wgpu sprite/text/camera/layer pipeline |
| `lunar-3d` / `lunar-render-3d` | 3D scene components + clustered-forward PBR renderer (CSM, GTAO, SSR, bloom, reflections) |
| `lunar-camera-3d` | spring-arm / orbit camera |

**asset pipeline**

| crate | purpose |
|-------|---------|
| `lunar-assets` | handle-based asset server, async loading, hot-reload, mip streaming |
| `lunar-image` | custom `.li` image format (LIF: planar + zstd) |
| `lunar-atlas` | texture atlas packer |
| `lunar-gamedata` (+ `-build`) | baked binary game-data tables (zero runtime parsing) |
| `lunar-bsp` (+ `-build`) | BVH / BSP visibility, portal culling |
| `lunar-lightmap` | offline lightmap baker + runtime components |

**opt-in plugins** (added to a game's `Cargo.toml` only when needed)

`lunar-physics-2d`, `lunar-physics-3d`, `lunar-particles`, `lunar-pathfinding-rt`,
`lunar-pathfinding-pre`, `lunar-ai`, `lunar-spline`, `lunar-timeline`, `lunar-animation`,
`lunar-tilemap`, `lunar-dialogue`, `lunar-ui`, `lunar-zones`, `lunar-localization`.

## getting started

### native

```bash
cargo run                              # smoke test (boots the engine, opens a window)
cargo run --example rpg_example        # RPG-style example
cargo run --example platform_demo      # 2D platformer
cargo run --example shooter_example    # top-down shooter
```

### browser (WASM)

```bash
go run scripts/run_wasm.go shooter_example   # build for wasm, run wasm-bindgen, serve + open browser
```

requirements:
- a browser with WebGPU support (Chrome 113+, recent Firefox/Safari with WebGPU enabled)
- `wasm-bindgen-cli` on `PATH`
- `go` 1.21+ (for the dev server in `scripts/`)

### cross-compiling all targets

```bash
go run scripts/build_all.go --release           # all 12 triples (needs cargo-zigbuild)
go run scripts/build_all.go --target x86_64-unknown-linux-musl
```

## targets

- Linux (glibc and musl) — x86_64, aarch64, i686, armv7
- Windows 10 / 11 — x86_64, i686 (gnu), aarch64 (gnullvm)
- macOS — x86_64, aarch64
- Web — WebGPU + WASM

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

fn move_player(time: Res<Time>, input: Res<InputState>, mut query: Query<&mut Transform, With<Sprite>>) {
    for mut transform in &mut query {
        if input.is_key_held(KeyCode::Right) {
            transform.translation.x += 100.0 * time.delta_seconds();
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

Game code spawns components; the engine renders them. The render system queries
`(Transform, Sprite)` and `(Transform, Text)` each frame and submits draws automatically:

```rust
fn spawn_label(mut commands: Commands, mut assets: ResMut<AssetServer>) {
    let font = assets.load_font("ui.ttf");
    commands.spawn((
        Transform::from_xy(20.0, 35.0),
        Text::new("score: 42", font).with_size(16.0),
    ));
}
```

Immediate-mode helpers cover HUD overlays and debug primitives — useful when the thing
you're drawing isn't a persistent entity:

```rust
fn hud_background(mut queue: ResMut<RenderQueue>) {
    queue.draw_rect(Vec2::new(10.0, 10.0), Vec2::new(200.0, 30.0), Color::BLACK);
}
```

Internals (`DrawCommand`, `DrawKind`, `RenderQueue::push`) are hidden — game code never
constructs them.

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
├── crates/              # the engine — one crate per subsystem (see table above)
├── examples/            # rpg_example, platform_demo, shooter_example
├── tools/               # offline pipeline: texture compression, LOD gen, PVS bake, asset gen
├── scripts/             # build_all.go (multiarch), run_wasm.go (wasm dev server)
├── plans/               # design documents
├── tests/api_seal/      # compile-time guard that the prelude seal holds
└── src/
    ├── main.rs          # native smoke-test entry point
    └── web.rs           # WASM entry point
```

## contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).
