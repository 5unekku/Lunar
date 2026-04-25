# Entity/Component Model

## Architecture

Lunar uses `bevy_ecs` as its ECS backend, but game code never imports `bevy_ecs` directly. All ECS types are re-exported through `engine-api` with a stable interface.

```
engine-api          <-- game code imports from here
    └── re-exports bevy_ecs types:
        - Entity
        - Component (derive macro)
        - Resource (derive macro)
        - Query
        - Res / ResMut
        - Commands
        - World
        - Schedule
        - Event / EventReader / EventWriter
        - With / Without (query filters)
        - SystemStage
        - etc.
```

## Component Definition

```rust
use lunar::prelude::*;

#[derive(Component)]
struct Health {
    current: u32,
    max: u32,
}

// Components can be unit structs
#[derive(Component)]
struct Enemy;

// Tuple components
#[derive(Component)]
struct Position(Vec2);
```

## Resource Definition

```rust
use lunar::prelude::*;

#[derive(Resource)]
struct GameScore(u32);

#[derive(Resource)]
struct LevelConfig {
    time_limit: f32,
    spawn_rate: f32,
}
```

## System Signatures

Systems are functions with parameters that the ECS scheduler resolves:

```rust
// Read-only query
fn read_positions(query: Query<&Position>) {
    for pos in query.iter() {
        // ...
    }
}

// Mutable query
fn update_positions(mut query: Query<(&mut Position, &Velocity)>, time: Res<Time>) {
    let dt = time.0;
    for (mut pos, vel) in query.iter_mut() {
        pos.0 += vel.0 * dt;
    }
}

// With filters
fn process_enemies(
    mut query: Query<(&mut Health, &Enemy), (With<Enemy>, Without<Dead>)>,
) {
    // ...
}

// Commands for entity manipulation
fn spawn_enemy(mut commands: Commands, assets: Res<AssetServer>) {
    commands.spawn((
        Enemy,
        Position(Vec2::new(100.0, 100.0)),
        Health { current: 100, max: 100 },
    ));
}

// Commands can also access existing entities
fn check_entity(commands: Commands, entity: Entity) {
    // commands.entity(entity) returns an EntityCommands builder
    // for inserting components, despawning, etc.
}
```

## System Scheduling

**By default, systems run in the order they are registered.** This is simple and predictable:

```rust
app.add_system(player_input_system)
   .add_system(physics_system)
   .add_system(enemy_ai_system)
   .add_system(render_system);
```

For games that need explicit ordering control, optional **stages** are available:

```rust
app.add_system_to_stage(UpdateStage::Input, player_input_system)
   .add_system_to_stage(UpdateStage::Physics, physics_system)
   .add_system_to_stage(UpdateStage::Update, game_logic_system)
   .add_system_to_stage(UpdateStage::Render, render_system);
```

Built-in stages (executed in order each tick):

| Stage | Purpose |
|---------|-------|
| `Input` | Poll input, update input state |
| `Physics` | Collision detection, physics simulation |
| `Update` | General game logic |
| `Render` | Queue render commands |

Most games can use registration order and never touch stages. Stages are there when you need them.

Game code can also define custom stages:

```rust
app.add_stage(CustomStage::AfterPhysics, StageOrder::After(UpdateStage::Physics))
   .add_system_to_stage(CustomStage::AfterPhysics, post_physics_system);
```

## Entity References

Game code uses `bevy_ecs::Entity` directly for entity references. The ECS already manages entity lifetimes with generationIDs internally. Only resources (textures, sounds, fonts) get wrapped in `Handle<T>`.

```rust
#[derive(Component)]
struct Bullet {
    owner: Entity,  // direct reference to the player entity
    damage: u32,
}
```

If you need to check if an entity is still alive:

```rust
fn check_target(target: Entity, query: Query<&Health>) {
    if query.get(target).is_ok() {
        // safe to use
    }
}
```

## Built-in Types

### UpdateStage Enum

```rust
/// Built-in system execution stages
pub enum UpdateStage {
    Input,    // Poll input, update input state
    Physics,  // Collision detection, physics simulation
    Update,   // General game logic
    Render,   // Queue render commands
}
```

### StageOrder Enum

```rust
/// Ordering for custom stages relative to built-in stages
pub enum StageOrder {
    Before(UpdateStage),  // Run before a built-in stage
    After(UpdateStage),   // Run after a built-in stage
    Between(UpdateStage, UpdateStage),  // Run between two built-in stages
}
```

### Transform Component

```rust
/// 2D transform component — attached to entities that need position/rotation/scale
#[derive(Component, Clone, Copy, Debug)]
pub struct Transform {
    pub translation: Vec3,  // x, y position + z layer (for render ordering)
    pub rotation: f32,       // radians
    pub scale: Vec2,         // x, y scale
}

impl Transform {
    pub fn from_translation(pos: Vec3) -> Self;
    pub fn from_xy(x: f32, y: f32) -> Self;
}
```

### Color Type

```rust
/// RGBA color
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const WHITE: Color = Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
    pub const BLACK: Color = Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };
    pub const RED: Color = Color { r: 1.0, g: 0.0, b: 0.0, a: 1.0 };
    pub const GREEN: Color = Color { r: 0.0, g: 1.0, b: 0.0, a: 1.0 };
    pub const BLUE: Color = Color { r: 0.0, g: 0.0, b: 1.0, a: 1.0 };
    pub const TRANSPARENT: Color = Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };

    pub fn rgb(r: f32, g: f32, b: f32) -> Self;
    pub fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self;
}
```

### Rect Type

```rust
/// Axis-aligned rectangle
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self;
    pub fn contains(&self, point: Vec2) -> bool;
    pub fn intersects(&self, other: Rect) -> bool;
}
```

---

[← Back to Developer Experience](01-developer-experience.md) | [Next: Handle System →](03-handle-system.md)
