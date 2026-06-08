# game loop

## fixed timestep

lunar uses a **fixed-timestep logic loop** decoupled from the render frame rate.
game logic runs at a fixed rate (default 60 hz). the renderer runs as fast
as the hardware allows (or up to a frame cap).

this means:
- `time.delta_seconds()` inside systems is always exactly `1 / tick_hz`
- physics and movement are deterministic regardless of frame rate
- on a slow frame, the engine may run 2-3 logic ticks before rendering

## startup vs update systems

startup systems run **once** before the loop starts. update systems run every tick.

```rust
impl GamePlugin for MyGame {
    fn build(&mut self, app: &mut App) {
        app.add_startup_system(setup);   // once
        app.add_system(update);          // every tick
    }
}

fn setup(mut commands: Commands, mut assets: ResMut<AssetServer>) {
    // load assets, spawn initial entities
}

fn update(time: Res<Time>, mut query: Query<&mut Transform>) {
    // runs 60 times per second by default
}
```

## the time resource

`Res<Time>` is always available. prefer `delta_seconds()` for all game logic:

```rust
fn move_system(time: Res<Time>, mut query: Query<(&Speed, &mut Transform)>) {
    for (speed, mut transform) in &mut query {
        transform.translation.x += speed.0 * time.delta_seconds();
    }
}
```

| method | what it gives you |
|--------|------------------|
| `delta_seconds()` | fixed tick delta (scaled). use for all game logic |
| `raw_delta_seconds()` | fixed tick delta (unscaled, ignores time_scale) |
| `real_delta_seconds()` | wall-clock render frame time. use for animation lerp only |
| `elapsed_seconds()` | total simulated time since start |
| `frame_count()` | total logic ticks since start |
| `time_scale()` | current time multiplier (1.0 = normal) |
| `set_time_scale(f)` | slow motion / fast forward. 0.0 = frozen |

## tick rate

the default tick rate is 60 hz. change it in `LoopConfig` or at runtime:

```rust
// change at startup via RenderConfig (2d) or RenderConfig3d (3d)
// the bootstrap functions derive a LoopConfig from these

// change at runtime by writing TickRateConfig:
fn increase_tick_rate(mut config: ResMut<TickRateConfig>) {
    config.rate = TickRate::Hz120;
}
```

available tick rates: `Hz30`, `Hz60`, `Hz90`, `Hz120`, `Hz144`, `Hz240`.

## update stages

systems are ordered into stages that run every tick in this sequence:

```
Input → Physics → Update → Render → PostUpdate
```

| stage | purpose |
|-------|---------|
| `UpdateStage::Input` | poll and update input state |
| `UpdateStage::Physics` | collision detection, physics |
| `UpdateStage::Update` | general game logic (default) |
| `UpdateStage::Render` | queue render commands |
| `UpdateStage::PostUpdate` | end-of-tick cleanup |

`app.add_system(f)` adds to `Update`. to target a specific stage:

```rust
app.add_system_to_stage(UpdateStage::Physics, my_collision_system);
app.add_system_to_stage(UpdateStage::Render, draw_debug_overlay);
```

## system ordering

within a stage, systems run in parallel by default. to enforce ordering
when two systems share mutable access to the same resource, chain them:

```rust
// a runs before b — use when they'd otherwise conflict on ResMut
app.add_ordered_systems((system_a, system_b));

// same but in a specific stage
app.add_ordered_systems_to_stage(UpdateStage::Physics, (detect_collisions, resolve_collisions));
```

## stopping the game

write `EngineState::Stopping` to the resource to exit the loop cleanly:

```rust
use lunar_core::EngineState;

fn check_quit(input: Res<InputState>, mut state: ResMut<EngineState>) {
    if input.is_key_just_pressed(KeyCode::Escape) {
        *state = EngineState::Stopping;
    }
}
```
