# window

`WindowSettings` is a resource inserted by the bootstrap at startup. read it to
get window dimensions; write fields to change window behavior. the engine applies
changes before the next frame.

## reading window state

```rust
fn responsive_layout(window: Res<WindowSettings>) {
    let width = window.width;
    let height = window.height;
    let aspect = width as f32 / height as f32;

    if window.is_fullscreen {
        // fullscreen layout
    }
}
```

`WindowSettings` fields:

| field | type | description |
|-------|------|-------------|
| `width` | `u32` | current render area width in pixels |
| `height` | `u32` | current render area height in pixels |
| `is_fullscreen` | `bool` | write true to go fullscreen; false to windowed |
| `vsync` | `bool` | vsync enabled |
| `cursor_locked` | `bool` | cursor relative mode (FPS-style); hides cursor |
| `target_aspect` | `Option<f32>` | lock aspect ratio on resize, e.g. `Some(16.0/9.0)` |
| `allow_resize` | `bool` | whether the user can resize the window |

## fullscreen toggle

```rust
fn toggle_fullscreen(input: Res<InputState>, mut window: ResMut<WindowSettings>) {
    if input.is_key_just_pressed(KeyCode::F11) {
        window.is_fullscreen = !window.is_fullscreen;
    }
}
```

alt+enter and F11 are handled by default in the bootstrap loop — you don't need
to implement them unless you want custom behavior.

## creating the window

window settings at startup are set by passing `WindowSettings::new` to the bootstrap.
`bootstrap` and `bootstrap_3d` accept a closure:

```rust
fn main() {
    lunar::bootstrap_3d::<MyGame>(
        lunar::lunar_render_3d::RenderConfig3d {
            window: WindowSettings::new(1920, 1080, true),  // 1080p, vsync on
            ..Default::default()
        }
    );
}
```

`WindowSettings::new(width, height, vsync)` defaults to: windowed, cursor unlocked,
no aspect lock, resizable.

## resolution helpers

```rust
use lunar_core::{resolutions_for_aspect, STANDARD_RESOLUTIONS};

// all standard resolutions matching 16:9 within 2% tolerance
let options = resolutions_for_aspect(16.0 / 9.0, 0.02);

// if you want only resolutions supported by the current display:
fn settings_menu(resolutions: Res<AvailableResolutions>) {
    let widescreen = resolutions.for_aspect(16.0 / 9.0, 0.02);
    for res in widescreen {
        println!("{}×{}", res.width, res.height);
    }
}
```

`AvailableResolutions` is inserted by the bootstrap from SDL3's display mode list
(native) or falls back to `STANDARD_RESOLUTIONS` on WASM.
