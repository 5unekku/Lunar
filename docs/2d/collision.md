# 2d collision

requires `Plugin2d` (see `2d/rendering.md`).

`Plugin2d` builds and maintains a `CollisionWorld` resource each physics tick.
spawn entities with `Collider` to register them:

```rust
commands.spawn((
    Transform::from_xy(200.0, 200.0),
    Collider { shape: ColliderShape::Rect(32.0, 64.0) },
));

commands.spawn((
    Transform::from_xy(300.0, 200.0),
    Collider { shape: ColliderShape::Circle(16.0) },
));
```

`ColliderShape` variants:
- `Rect(width, height)` — axis-aligned box centered on the entity's position
- `Circle(radius)` — circle centered on the entity's position

## overlap queries

query which entities overlap a given entity in a Physics or Update system:

```rust
fn check_pickups(
    collision_world: Res<CollisionWorld>,
    players: Query<Entity, With<Player>>,
    pickups: Query<Entity, With<Pickup>>,
    mut commands: Commands,
) {
    for player_entity in &players {
        for other in collision_world.overlapping(player_entity) {
            if pickups.get(other).is_ok() {
                commands.entity(other).despawn();
            }
        }
    }
}
```

## raycasting

```rust
use lunar::lunar_2d::ray_cast_2d;

fn shoot(collision_world: Res<CollisionWorld>) {
    let origin = Vec2::new(0.0, 0.0);
    let direction = Vec2::new(1.0, 0.0);  // must be normalized
    let max_distance = 500.0;

    let hits = ray_cast_2d(&collision_world, origin, direction, max_distance);
    for hit in hits {
        println!("hit {:?} at distance {:.1}", hit.entity, hit.distance);
    }
}
```

`RayHit2d` fields:
- `entity: Entity` — the hit entity
- `distance: f32` — distance from origin along the ray

hits are returned in order of increasing distance.
