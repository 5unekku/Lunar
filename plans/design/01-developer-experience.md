# Game Developer Experience

## What It Looks Like to Write a Game

A game project is a separate Rust crate that depends on `lunar` (and optionally `lunar-core` for advanced use). The engine provides a macro that bootstraps the application and a plugin trait for game-specific initialization.

## Minimal Example — A Simple Platformer

```rust
use lunar::prelude::*;

// --- Components ---

#[derive(Component)]
struct Player {
    speed: f32,
    jump_force: f32,
    on_ground: bool,
}

#[derive(Component)]
struct Velocity(pub Vec2);

#[derive(Component)]
struct Sprite {
    texture: TextureHandle,
    size: Vec2,
}

#[derive(Component)]
struct Collider {
    size: Vec2,
    offset: Vec2,
}

#[derive(Component)]
struct Platform;

// --- Resources ---

#[derive(Resource)]
struct Gravity(f32);

// --- Systems ---

fn player_input_system(
    mut query: Query<(&Player, &mut Velocity)>,
    input: Res<InputState>,
) {
    for (player, mut vel) in query.iter_mut() {
        let mut dx = 0.0;
        if input.is_key_held(KeyCode::Left) {
            dx -= 1.0;
        }
        if input.is_key_held(KeyCode::Right) {
            dx += 1.0;
        }
        if input.is_key_just_pressed(KeyCode::Space) && player.on_ground {
            vel.0.y = -player.jump_force;
        }
        vel.0.x = dx * player.speed;
    }
}

fn physics_system(
    mut query: Query<(&mut Transform, &mut Velocity, &mut Player)>,
    gravity: Res<Gravity>,
    time: Res<DeltaTime>,
) {
    let dt = time.0;
    for (mut transform, mut vel, mut player) in query.iter_mut() {
        // apply gravity
        vel.0.y += gravity.0 * dt;

        // integrate
        transform.translation.x += vel.0.x * dt;
        transform.translation.y += vel.0.y * dt;

        // simple ground clamp (real game would use collision detection)
        if transform.translation.y > 500.0 {
            transform.translation.y = 500.0;
            vel.0.y = 0.0;
            player.on_ground = true;
        } else {
            player.on_ground = false;
        }
    }
}

fn render_system(
    mut commands: Commands,
    query: Query<(&Transform, &Sprite)>,
    mut render: ResMut<RenderQueue>,
) {
    for (transform, sprite) in query.iter() {
        render.draw_sprite(
            &sprite.texture,
            transform.translation,
            sprite.size,
        );
    }
}

// --- Plugin ---

struct MyGamePlugin;

impl GamePlugin for MyGamePlugin {
    fn build(&self, app: &mut App) {
        // register resources
        app.insert_resource(Gravity(980.0));

        // register systems in order
        app.add_system(player_input_system)
           .add_system(physics_system)
           .add_system(render_system);

        // spawn initial entities
        app.add_startup_system(setup_world);
    }
}

fn setup_world(
    mut commands: Commands,
    assets: Res<AssetServer>,
) {
    let player_tex = assets.load("textures/player.png");
    let ground_tex = assets.load("textures/ground.png");

    // spawn player
    commands.spawn((
        Player {
            speed: 200.0,
            jump_force: 400.0,
            on_ground: false,
        },
        Velocity(Vec2::ZERO),
        Transform::from_xy(400.0, 400.0),
        Sprite::new(player_tex).with_size(Vec2::new(32.0, 48.0)),
        Collider {
            size: Vec2::new(28.0, 44.0),
            offset: Vec2::new(2.0, 2.0),
        },
    ));

    // spawn ground platform
    commands.spawn((
        Platform,
        Transform::from_xy(640.0, 520.0),
        Sprite::new(ground_tex).with_size(Vec2::new(1280.0, 40.0)),
        Collider {
            size: Vec2::new(1280.0, 40.0),
            offset: Vec2::ZERO,
        },
    ));
}

// --- Entry Point ---

lunar_app!(MyGamePlugin);
```

## Project Structure for a Game

```
my-game/
├── Cargo.toml
├── assets/
│   ├── textures/
│   ├── audio/
│   └── fonts/
└── src/
    ├── main.rs          # lunar_app! macro
    ├── components.rs    # component definitions
    ├── systems/
    │   ├── mod.rs
    │   ├── input.rs
    │   ├── physics.rs
    │   └── render.rs
    ├── plugins/
    │   ├── mod.rs
    │   └── game.rs
    └── resources.rs
```

## Key DX Decisions

- **`lunar_app!` macro** handles all the boilerplate: window creation, subsystem init, game loop startup. Game code just provides a plugin.
- **Components derive from `bevy_ecs`** but are re-exported through `lunar` so game code doesn't directly depend on bevy_ecs version.
- **Systems are plain functions** with ECS query parameters. No custom DSL.
- **Startup systems** run once before the main loop. Regular systems run every tick.
- **`Commands`** is the only way to spawn/despawn entities or queue one-shot operations from systems.

## Hard Rule — Unsafe Is Never Required for Basic Engine Features

The engine must not force `unsafe` onto game code for routine tasks. Seasonal, heavily-optimized game code may have legitimate reasons to dip into `unsafe` — that's fine. The engine shouldn't be in the way of that choice.

What's not acceptable: the engine's own API gaps forcing game code to write `unsafe` just to store a window handle or access a renderer. That means:

- **Window handles, GPU resources, raw pointers** — all wrapped in engine-owned types. Games register systems, not SDL callbacks.
- **Thread-safety** (`Send`/`Sync`) is handled by engine types internally. Game types derive `Component`/`Resource` and get this for free.
- **FFI boundaries** (SDL3, wgpu, OS APIs) are sealed behind engine abstractions. A game crate imports `lunar` and nothing else with native bindings.
- **Derive macros** re-export through `lunar` so game code never writes `use bevy_ecs::...`; the facade crate is the only dependency.

If a game needs `unsafe` to get something basic working (storing a window handle, accessing a renderer), the engine API is incomplete — that's a bug to fix, not a workaround to accept. The game developer can opt into `unsafe` when they want to; the engine should never mandate it.

## Post-1.0 Revision — What the RPG Example Revealed

After building a minimal RPG demo (one room, 3 NPCs, typewriter dialogue, camera scrolling, fullscreen toggle), several gaps in the engine's high-level API were identified:

### Engine-specific Leaks (the game had to touch native APIs)

| Leak | Where | Fix |
|---|---|---|
| **SDL3 Window in ECS** | Game stored `sdl3::video::Window` as a Resource, requiring `unsafe impl Send+Sync` | Engine owns the window; expose `FullscreenToggle`, `WindowInfo` resources from the macro |
| **Fullscreen via raw SDL3 call** | `wh.0.set_fullscreen(true)` called directly | `InputPlugin` handles F11/F keybinding + `ActionMap`, engine manages surface resize + viewport update |
| **Resizing wgpu surface** | `re.resize_surface(...)` called from game system | Automatic when fullscreen toggles |
| **`bevy_ecs` in Cargo.toml** | Derive macros need the crate name to resolve | `lunar` re-exports derives at the crate root so `use lunar::Component` works from game code |

### Missing Convenience Abstractions

| Gap | Impact | Fix |
|---|---|---|
| **No `screen_to_world` / `world_to_screen`** | Every camera-scrolling game with UI has to write `screen_pos + camera.pos - half_viewport` | Add to `Camera` as public methods |
| **No viewport letterboxing** | Had to hack a custom projection matrix path | Camera's `viewport` field + projection already does this; make it a first-class feature toggled via `set_viewport(aspect_ratio)` |
| **No blocking asset load** | Startup spawns before textures/fonts are ready | `AssetServer::wait_for_all()` or a `LoadingState` that auto-transitions |
| **No `draw_ui_text` / `draw_ui_rect`** | UI drawing requires manual coordinate conversion | `RenderQueue` gets `draw_ui_*` variants that take screen-space coords and internally convert through the camera |
| **Render system too wide** | The game's render system takes 9 parameters | Split into `draw_world` + `draw_ui` passes with automatic queue setup |

### Pipeline Bugs Found

| Bug | Detail |
|---|---|
| **`render_system` was a no-op** | `RenderPlugin` registered a system that only called `queue.clear()` — never called `RenderEngine::render()`. The full pipeline existed but was disconnected. |

---

[← Back to Overview](00-overview.md) | [Next: Entity/Component Model →](02-ecs-model.md)
