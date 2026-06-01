//! 2d physics: gravity, velocity integration, and AABB collision response.
//!
//! this crate extends [`lunar_2d::CollisionWorld`] with simulation. it does not
//! replace the collision detection layer — it reads `CollisionWorld` each frame
//! and resolves overlaps by pushing entities apart and zeroing velocity on the
//! collision axis.
//!
//! # getting started
//!
//! ```ignore
//! use lunar_physics_2d::PhysicsPlugin2d;
//!
//! app.add_plugin(PhysicsPlugin2d);
//!
//! // spawn a physics entity
//! commands.spawn((
//!     LocalTransform::from_xy(100.0, 0.0),
//!     Collider::aabb(Vec2::new(16.0, 24.0)),
//!     Velocity2d::default(),
//! ));
//!
//! // gravity is a resource — change it at runtime if needed
//! commands.insert_resource(Gravity2d(980.0)); // pixels per second²
//! ```

use bevy_ecs::prelude::*;
use lunar_2d::{Collider, ColliderShape, CollisionWorld};
use lunar_core::{App, GamePlugin};
use lunar_math::{LocalTransform, Vec2};

/// 2d velocity component. attach to any entity with a `LocalTransform` to
/// enable velocity integration and physics response.
#[derive(Debug, Clone, Copy, Component, Default)]
pub struct Velocity2d {
    /// linear velocity in world units per second
    pub linear: Vec2,
    /// angular velocity in radians per second
    pub angular: f32,
}

impl Velocity2d {
    /// create from a linear velocity, angular = 0
    #[must_use]
    pub const fn linear(velocity: Vec2) -> Self {
        Self { linear: velocity, angular: 0.0 }
    }
}

/// gravitational acceleration in world units per second squared.
///
/// applied to every entity with [`Velocity2d`] each frame. positive Y = downward
/// (screen space). set to `Gravity2d(0.0)` for space/top-down games.
#[derive(Resource, Debug, Clone, Copy)]
pub struct Gravity2d(pub f32);

impl Default for Gravity2d {
    fn default() -> Self {
        Self(980.0)
    }
}

/// marker component: this collider is a one-way platform.
///
/// entities can pass through from below but are blocked from above.
/// "above" means the colliding entity's bottom edge is above the platform's top edge.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct OneWayPlatform;

/// plugin that registers the physics systems in the correct stage order.
///
/// register after [`Plugin2d`](lunar_2d::Plugin2d) so that `CollisionWorld` is
/// built before `resolve_collisions` runs.
pub struct PhysicsPlugin2d;

impl GamePlugin for PhysicsPlugin2d {
    fn name(&self) -> &'static str {
        "PhysicsPlugin2d"
    }

    fn build(&mut self, app: &mut App) {
        app.insert_resource(Gravity2d::default());
        // apply gravity then integrate, both in the Physics stage
        app.add_system_to_stage(lunar_core::UpdateStage::Physics, apply_gravity);
        app.add_system_to_stage(
            lunar_core::UpdateStage::Physics,
            (integrate_velocity, resolve_collisions).chain(),
        );
    }
}

/// apply gravitational acceleration to all entities with `Velocity2d`.
pub fn apply_gravity(
    gravity: Res<Gravity2d>,
    time: Res<lunar_core::Time>,
    mut query: Query<&mut Velocity2d>,
) {
    let delta = time.delta_seconds();
    if delta <= 0.0 { return; }
    for mut velocity in &mut query {
        velocity.linear.y += gravity.0 * delta;
    }
}

/// integrate `Velocity2d` into `LocalTransform` using semi-implicit Euler.
pub fn integrate_velocity(
    time: Res<lunar_core::Time>,
    mut query: Query<(&mut LocalTransform, &Velocity2d)>,
) {
    let delta = time.delta_seconds();
    if delta <= 0.0 { return; }
    for (mut transform, velocity) in &mut query {
        transform.translation.x += velocity.linear.x * delta;
        transform.translation.y += velocity.linear.y * delta;
        transform.rotation += velocity.angular * delta;
    }
}

/// resolve AABB overlaps: push entities out of solid colliders and zero velocity
/// on the collision axis. entities with `OneWayPlatform` only block downward movement.
pub fn resolve_collisions(
    collision_world: Res<CollisionWorld>,
    mut query: Query<(Entity, &mut LocalTransform, &mut Velocity2d, &Collider), Without<OneWayPlatform>>,
    platform_query: Query<&LocalTransform, With<OneWayPlatform>>,
    collider_query: Query<(&Collider, &LocalTransform)>,
) {
    for (entity, mut transform, mut velocity, collider) in &mut query {
        let ColliderShape::Aabb { half_extents } = collider.shape else { continue };

        for other_entity in collision_world.overlapping(entity) {
            // skip self
            if other_entity == entity { continue; }

            let Ok((other_collider, other_transform)) = collider_query.get(other_entity) else { continue };
            let ColliderShape::Aabb { half_extents: other_half } = other_collider.shape else { continue };

            // one-way platform: only block if we're moving downward and were above the platform
            if platform_query.get(other_entity).is_ok() {
                let platform_top = other_transform.translation.y - other_half.y;
                let entity_bottom = transform.translation.y + half_extents.y;
                // block only if descending and entity bottom is near platform top
                if velocity.linear.y <= 0.0 || entity_bottom > platform_top + 2.0 {
                    continue;
                }
                // push up and zero Y velocity
                transform.translation.y = platform_top - half_extents.y;
                velocity.linear.y = 0.0;
                continue;
            }

            // solid collider: compute minimum separation vector and push out
            let delta_x = transform.translation.x - other_transform.translation.x;
            let delta_y = transform.translation.y - other_transform.translation.y;
            let overlap_x = (half_extents.x + other_half.x) - delta_x.abs();
            let overlap_y = (half_extents.y + other_half.y) - delta_y.abs();

            if overlap_x <= 0.0 || overlap_y <= 0.0 { continue; }

            // push along the axis with the smaller overlap
            if overlap_x < overlap_y {
                let sign = delta_x.signum();
                transform.translation.x += sign * overlap_x;
                velocity.linear.x = 0.0;
            } else {
                let sign = delta_y.signum();
                transform.translation.y += sign * overlap_y;
                if delta_y > 0.0 {
                    // entity below other = hit ceiling: zero upward velocity
                    velocity.linear.y = velocity.linear.y.max(0.0);
                } else {
                    // entity above other = landed on top: zero downward velocity
                    velocity.linear.y = velocity.linear.y.min(0.0);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lunar_core::Time;

    fn world_with_gravity() -> World {
        let mut world = World::new();
        world.insert_resource(Gravity2d(100.0));
        let mut time = Time::default();
        time.set_delta_seconds(1.0);
        world.insert_resource(time);
        world
    }

    #[test]
    fn gravity_increases_y_velocity() {
        let mut world = world_with_gravity();
        let entity = world.spawn(Velocity2d::default()).id();

        let mut system = IntoSystem::into_system(apply_gravity);
        system.initialize(&mut world);
        let _ = system.run((), &mut world);

        let velocity = world.get::<Velocity2d>(entity).unwrap();
        assert!((velocity.linear.y - 100.0).abs() < 0.01);
    }

    #[test]
    fn integration_moves_transform() {
        let mut world = World::new();
        let mut time = Time::default();
        time.set_delta_seconds(0.5);
        world.insert_resource(time);

        let entity = world.spawn((
            LocalTransform::from_xy(0.0, 0.0),
            Velocity2d::linear(Vec2::new(20.0, 0.0)),
        )).id();

        let mut system = IntoSystem::into_system(integrate_velocity);
        system.initialize(&mut world);
        let _ = system.run((), &mut world);

        let transform = world.get::<LocalTransform>(entity).unwrap();
        assert!((transform.translation.x - 10.0).abs() < 0.001);
    }

    #[test]
    fn zero_delta_skips_gravity() {
        let mut world = World::new();
        world.insert_resource(Gravity2d(1000.0));
        world.insert_resource(Time::default()); // delta = 0

        let entity = world.spawn(Velocity2d::default()).id();
        let mut system = IntoSystem::into_system(apply_gravity);
        system.initialize(&mut world);
        let _ = system.run((), &mut world);

        let velocity = world.get::<Velocity2d>(entity).unwrap();
        assert!((velocity.linear.y - 0.0).abs() < 0.001);
    }
}

/// common, game-facing 2D physics types for `use lunar::prelude::*`.
pub mod prelude {
    pub use crate::{Gravity2d, OneWayPlatform, PhysicsPlugin2d, Velocity2d};
}
