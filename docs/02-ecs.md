# ecs — entities, components, systems

lunar uses an entity-component-system (ECS) architecture. game objects are
entities with attached data (components). logic runs in systems that query
for entities matching a component pattern.

## entities and components

an entity is a lightweight id (think: a row id in a database). components are
the data attached to it. you spawn them together via `Commands`:

```rust
use lunar::prelude::*;

// a plain struct becomes a component by deriving Component
#[derive(Component)]
struct Player {
    health: f32,
    speed: f32,
}

fn spawn_player(mut commands: Commands, mut assets: ResMut<AssetServer>) {
    let texture = assets.load_texture("player.png");

    commands.spawn((
        Player { health: 100.0, speed: 200.0 },
        Transform::from_xy(400.0, 300.0),
        Sprite::new(texture),
    ));
}
```

spawning returns an `EntityWorldMut`. if you need the entity id for later use:

```rust
let entity = commands.spawn(Player { health: 100.0, speed: 200.0 }).id();
```

## systems

a system is a plain function. its parameters declare what it needs — the engine
injects them automatically:

```rust
// Res<T>    — shared read access to a resource
// ResMut<T> — exclusive write access to a resource
// Query<...> — read/write components on matching entities
fn move_player(
    time: Res<Time>,
    input: Res<InputState>,
    mut query: Query<(&Player, &mut Transform)>,
) {
    for (player, mut transform) in &mut query {
        if input.is_key_held(KeyCode::Right) {
            transform.translation.x += player.speed * time.delta_seconds();
        }
    }
}
```

## queries

`Query<&Component>` iterates all entities that have that component.
`Query<(&A, &mut B)>` iterates entities that have both A and B.

```rust
// read-only access
fn read_positions(query: Query<&Transform>) {
    for transform in &query {
        println!("entity at {:?}", transform.translation);
    }
}

// mutable access
fn reset_health(mut query: Query<&mut Player>) {
    for mut player in &mut query {
        player.health = 100.0;
    }
}
```

### query filters

filters narrow which entities are matched without accessing the component data:

```rust
// only entities that have Enemy AND are alive
#[derive(Component)]
struct Enemy;

#[derive(Component)]
struct Dead;

fn update_enemies(query: Query<&mut Transform, (With<Enemy>, Without<Dead>)>) {
    for mut transform in query {
        // only live enemies
    }
}
```

filter markers:
- `With<T>` — entity must have T (not accessed)
- `Without<T>` — entity must not have T
- `Added<T>` — entity gained T this tick
- `Changed<T>` — T changed value this tick
- `Or<(A, B)>` — matches if A or B is true

### querying a single entity

when you know there's exactly one (e.g. the player), use `Single`:

```rust
fn follow_player(
    player: Single<&Transform, With<Player>>,
    mut camera: ResMut<Camera>,
) {
    camera.position = player.translation;
}
```

`Single` panics if zero or more than one entity matches. use `Query::get_single`
if the count might vary.

## resources

resources are singleton values stored in the ECS world — not attached to any entity.
use them for global state like the score, a game phase, or engine services.

```rust
#[derive(Resource)]
struct Score(u32);

// insert a resource from a startup system or from App::insert_resource
fn setup(mut commands: Commands) {
    commands.insert_resource(Score(0));
}

fn add_points(mut score: ResMut<Score>) {
    score.0 += 10;
}

fn display_score(score: Res<Score>) {
    println!("score: {}", score.0);
}
```

built-in resources always available (no setup needed):
- `Time` — delta time, elapsed time, frame count
- `InputState` — keyboard, mouse, gamepad state
- `ActionMap` — named action bindings
- `AssetServer` — asset loading
- `RenderQueue` — immediate-mode draw calls (2d)
- `WindowSettings` — window dimensions and fullscreen state
- `Camera` — 2d camera position and zoom (insert this yourself if needed)

## commands

`Commands` is a deferred write buffer — changes apply at the end of the current
stage, not immediately. use it to spawn, despawn, and mutate entities:

```rust
fn cleanup_dead(
    mut commands: Commands,
    query: Query<(Entity, &Player)>,
) {
    for (entity, player) in &query {
        if player.health <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}
```

`commands.entity(id)` returns an `EntityCommands` with:
- `.insert(component)` — add a component
- `.remove::<T>()` — remove a component
- `.despawn()` — destroy the entity

## local state

`Local<T>` is per-system state that persists between ticks, owned by that system only:

```rust
fn spawn_enemies(mut timer: Local<f32>, time: Res<Time>, mut commands: Commands) {
    *timer += time.delta_seconds();
    if *timer >= 3.0 {
        *timer = 0.0;
        commands.spawn(Enemy);
    }
}
```

`T` must implement `Default`. the value initializes to `T::default()` on first run.

## messages (events)

messages are buffered streams — one system writes, another reads. useful for
decoupled communication between systems:

```rust
#[derive(Message)]
struct PlayerDied {
    position: Vec2,
}

fn kill_player(
    query: Query<(Entity, &Player, &Transform)>,
    mut commands: Commands,
    mut writer: MessageWriter<PlayerDied>,
) {
    for (entity, player, transform) in &query {
        if player.health <= 0.0 {
            writer.write(PlayerDied { position: transform.translation });
            commands.entity(entity).despawn();
        }
    }
}

fn on_player_died(mut reader: MessageReader<PlayerDied>) {
    for event in reader.read() {
        println!("player died at {:?}", event.position);
    }
}
```

## hierarchy

entities can be parented. when using `Plugin2d`, transforms propagate from parent
to child automatically:

```rust
use lunar_core::Parent;

fn spawn_weapon(mut commands: Commands, player_entity: Entity) {
    commands.spawn((
        Transform::from_xy(16.0, 0.0),  // relative to parent
        Parent(player_entity),
        Sprite::new(weapon_texture),
    ));
}
```

use `LocalTransform` instead of `Transform` on child entities to express
position relative to the parent. `WorldTransform` is written by the propagation
system and gives the final screen-space position.
