# 3d setup

requires the `3d` feature.

## cargo.toml

```toml
[dependencies]
lunar = { version = "1", features = ["3d"] }
# or for 3d + audio:
lunar = { version = "1", features = ["full"] }
```

## bootstrap

use `bootstrap_3d` instead of the `lunar_app!` macro. pass a `RenderConfig3d`
to configure the window and render settings:

```rust
use lunar::prelude::*;
use lunar::lunar_render_3d::RenderConfig3d;

fn main() {
    lunar::bootstrap_3d::<MyGame>(RenderConfig3d {
        width: 1280,
        height: 720,
        vsync: true,
        title: "My Game".to_string(),
        ..Default::default()
    });
}

#[derive(Default)]
struct MyGame;

impl GamePlugin for MyGame {
    fn name(&self) -> &str { "MyGame" }

    fn build(&mut self, app: &mut App) {
        app.add_plugin(Plugin3d);
        app.add_startup_system(setup);
        app.add_system(update);
    }
}
```

`RenderConfig3d` fields (all have defaults):

| field | type | default | description |
|-------|------|---------|-------------|
| `width` | `u32` | 1280 | initial window width |
| `height` | `u32` | 720 | initial window height |
| `vsync` | `bool` | true | enable vsync |
| `frame_cap` | `u32` | 0 | fps cap (0 = vsync-limited) |
| `tick_rate` | `TickRate` | `Hz60` | logic tick rate |
| `title` | `String` | "Lunar" | window title |
| `target_aspect` | `Option<f32>` | None | lock aspect ratio on resize |
| `allow_resize` | `bool` | true | user can resize window |

## plugins

`Plugin3d` must be registered. it adds transform propagation, frustum culling,
3d collision, and all 3d systems. `RenderPlugin3d` is added automatically by
`bootstrap_3d` — you don't need to add it manually.

```rust
impl GamePlugin for MyGame {
    fn build(&mut self, app: &mut App) {
        app.add_plugin(Plugin3d);
        // optional:
        app.add_plugin(BspPlugin);    // portal culling for indoor levels
        app.add_plugin(BvhPlugin);    // BVH frustum culling for open worlds
    }
}
```

for WASM, use `bootstrap_wasm_3d` instead of `bootstrap_3d`.
