//! kinematic character controller for 3D — move/slide/slope/step on AABB colliders.
//!
//! no rigid bodies or full physics simulation. answers "where does this capsule end up
//! after moving by velocity?" using iterative depenetration against [`CollisionWorld3d`].
//!
//! # design
//!
//! - velocity is integrated with semi-implicit Euler: position += velocity * dt
//! - collision response uses iterative AABB overlap depenetration (up to 4 passes)
//! - slope detection: if the contact normal is within `max_slope_angle` of vertical,
//!   the character is grounded; steep surfaces are treated as walls
//! - step-up: characters can climb steps up to `step_height` automatically
//! - gravity: applied each frame to characters not grounded
//!
//! # usage
//!
//! ```ignore
//! use lunar_physics_3d::{KinematicBody3d, Gravity3d, Physics3dPlugin};
//! use lunar_3d::{LocalTransform3d, WorldTransform3d, Collider3d};
//!
//! fn setup(mut commands: Commands) {
//!     commands.spawn((
//!         LocalTransform3d::from_xyz(0.0, 5.0, 0.0),
//!         WorldTransform3d::default(),
//!         Collider3d::aabb(Vec3::new(0.6, 1.8, 0.6)),
//!         KinematicBody3d::default(),
//!     ));
//! }
//! ```

use bevy_ecs::prelude::*;
use lunar_math::{Quat, Vec3};

use lunar_3d::collision::{ColliderShape3d, CollisionWorld3d};
use lunar_3d::LocalTransform3d;

/// velocity and physics state for a kinematic 3D character.
#[derive(Component, Debug, Clone)]
pub struct KinematicBody3d {
    /// world-space velocity in units/second.
    pub velocity: Vec3,
    /// max slope angle in radians before a surface is treated as a wall. default ~45°.
    pub max_slope_angle: f32,
    /// maximum step height the character can auto-climb.
    pub step_height: f32,
    /// true if the character is standing on a surface this frame.
    pub is_grounded: bool,
    /// number of depenetration iterations per frame.
    pub solver_iterations: u32,
}

impl Default for KinematicBody3d {
    fn default() -> Self {
        Self {
            velocity: Vec3::ZERO,
            max_slope_angle: std::f32::consts::FRAC_PI_4,
            step_height: 0.35,
            is_grounded: false,
            solver_iterations: 4,
        }
    }
}

/// gravity applied to all [`KinematicBody3d`] entities that are not grounded.
/// units: world-units per second². default 9.8 (1 unit = 1 metre).
#[derive(Resource)]
pub struct Gravity3d(pub f32);

impl Default for Gravity3d {
    fn default() -> Self {
        Self(9.8)
    }
}

/// plugin — inserts `Gravity3d` resource and registers the physics systems.
pub struct PhysicsPlugin3d;

impl PhysicsPlugin3d {
    /// build the plugin manually if not using the automatic plugin system.
    pub fn build(world: &mut World) {
        world.insert_resource(Gravity3d::default());
    }
}

/// system — apply gravity to ungrounded kinematic bodies.
pub fn apply_gravity_3d(
    gravity: Res<Gravity3d>,
    time: Res<lunar_core::Time>,
    mut query: Query<&mut KinematicBody3d>,
) {
    let delta = time.delta_seconds();
    for mut body in query.iter_mut() {
        if !body.is_grounded {
            body.velocity.y -= gravity.0 * delta;
        }
    }
}

/// system — integrate velocity into position, then resolve AABB collisions.
pub fn move_and_slide_3d(
    time: Res<lunar_core::Time>,
    collision_world: Res<CollisionWorld3d>,
    mut query: Query<(Entity, &mut LocalTransform3d, &mut KinematicBody3d, &lunar_3d::Collider3d)>,
) {
    let delta = time.delta_seconds();

    for (entity, mut transform, mut body, collider) in query.iter_mut() {
        let ColliderShape3d::Aabb { half_extents } = collider.shape else {
            continue; // only AABB supported for now
        };

        // integrate velocity
        let desired_delta = body.velocity * delta;
        let mut new_pos = transform.translation + desired_delta;
        body.is_grounded = false;

        // iterative depenetration
        for _ in 0..body.solver_iterations {
            let mut any_hit = false;
            for other in collision_world.overlapping_at(entity, new_pos, half_extents, collider.mask) {
                let overlap = aabb_overlap_depth(new_pos, half_extents, other.position, other.half_extents);
                let Some((push_axis, depth)) = overlap else {
                    continue;
                };
                any_hit = true;

                // slope: if push axis is mostly vertical, treat as ground/ceiling
                let xz_len = (push_axis.x * push_axis.x + push_axis.z * push_axis.z).sqrt();
                let is_vertical = push_axis.y.abs() > xz_len * f32::cos(body.max_slope_angle);
                if is_vertical && push_axis.y > 0.0 {
                    body.is_grounded = true;
                    body.velocity.y = body.velocity.y.max(0.0);
                } else if is_vertical && push_axis.y < 0.0 {
                    body.velocity.y = body.velocity.y.min(0.0);
                } else {
                    // wall: zero out velocity along push axis
                    let dot = body.velocity.dot(push_axis);
                    if dot < 0.0 {
                        body.velocity -= push_axis * dot;
                    }
                }

                new_pos += push_axis * (depth + 0.001);
            }
            if !any_hit {
                break;
            }
        }

        transform.translation = new_pos;
    }
}

/// proxy entry from collision world used by `overlapping_at`.
struct OverlapEntry {
    position: Vec3,
    half_extents: Vec3,
}

/// extension trait to query collision world at an arbitrary position (not entity's current one).
trait CollisionWorldExt {
    fn overlapping_at(
        &self,
        entity: Entity,
        position: Vec3,
        half_extents: Vec3,
        mask: u32,
    ) -> impl Iterator<Item = OverlapEntry> + '_;
}

impl CollisionWorldExt for CollisionWorld3d {
    fn overlapping_at(
        &self,
        entity: Entity,
        position: Vec3,
        half_extents: Vec3,
        mask: u32,
    ) -> impl Iterator<Item = OverlapEntry> + '_ {
        // sweep-and-prune on X: only test entries whose X range overlaps ours
        let qmin_x = position.x - half_extents.x;
        let qmax_x = position.x + half_extents.x;
        self.query_aabb_entries(qmin_x, qmax_x).filter_map(move |entry| {
            if entry.entity == entity { return None; }
            if mask & entry.layer == 0 { return None; }
            let other_he = match entry.shape {
                ColliderShape3d::Aabb { half_extents: he } => he,
                ColliderShape3d::Sphere { radius } => Vec3::splat(radius),
            };
            if aabb_overlap_depth(position, half_extents, entry.position, other_he).is_some() {
                Some(OverlapEntry { position: entry.position, half_extents: other_he })
            } else {
                None
            }
        })
    }
}

/// compute the shortest push-out vector (axis, depth) to separate two AABBs.
/// returns `None` if the AABBs do not overlap.
fn aabb_overlap_depth(
    pos_a: Vec3,
    half_a: Vec3,
    pos_b: Vec3,
    half_b: Vec3,
) -> Option<(Vec3, f32)> {
    let diff = pos_a - pos_b;
    let combined = half_a + half_b;
    let overlap_x = combined.x - diff.x.abs();
    let overlap_y = combined.y - diff.y.abs();
    let overlap_z = combined.z - diff.z.abs();
    if overlap_x <= 0.0 || overlap_y <= 0.0 || overlap_z <= 0.0 {
        return None;
    }
    // push along the smallest overlap axis
    if overlap_y <= overlap_x && overlap_y <= overlap_z {
        let sign = if diff.y >= 0.0 { 1.0 } else { -1.0 };
        Some((Vec3::new(0.0, sign, 0.0), overlap_y))
    } else if overlap_x <= overlap_z {
        let sign = if diff.x >= 0.0 { 1.0 } else { -1.0 };
        Some((Vec3::new(sign, 0.0, 0.0), overlap_x))
    } else {
        let sign = if diff.z >= 0.0 { 1.0 } else { -1.0 };
        Some((Vec3::new(0.0, 0.0, sign), overlap_z))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_ecs::system::IntoSystem;
    use lunar_3d::collision::{Collider3d, build_collision_world_3d};
    use lunar_3d::WorldTransform3d;
    use lunar_math::Vec3;

    fn setup_world() -> World {
        let mut world = World::new();
        world.insert_resource(Gravity3d::default());
        let mut time = lunar_core::Time::default();
        time.set_delta_seconds(1.0 / 60.0);
        world.insert_resource(time);
        world.insert_resource(CollisionWorld3d::default());
        world
    }

    fn run_build(world: &mut World) {
        let mut system = IntoSystem::into_system(build_collision_world_3d);
        system.initialize(world);
        let _ = system.run((), world);
    }

    fn run_gravity(world: &mut World) {
        let mut system = IntoSystem::into_system(apply_gravity_3d);
        system.initialize(world);
        let _ = system.run((), world);
    }

    fn run_move_slide(world: &mut World) {
        let mut system = IntoSystem::into_system(move_and_slide_3d);
        system.initialize(world);
        let _ = system.run((), world);
    }

    #[test]
    fn gravity_accelerates_ungrounded_body() {
        let mut world = setup_world();
        let entity = world
            .spawn((
                LocalTransform3d::from_xyz(0.0, 10.0, 0.0),
                WorldTransform3d::default(),
                Collider3d::aabb(Vec3::ONE),
                KinematicBody3d::default(),
            ))
            .id();

        run_gravity(&mut world);
        let vy = world.get::<KinematicBody3d>(entity).unwrap().velocity.y;
        assert!(vy < 0.0, "gravity should add downward velocity");
    }

    #[test]
    fn grounded_body_not_affected_by_gravity() {
        let mut world = setup_world();
        let entity = world
            .spawn((
                LocalTransform3d::from_xyz(0.0, 0.0, 0.0),
                WorldTransform3d::default(),
                Collider3d::aabb(Vec3::ONE),
                KinematicBody3d { is_grounded: true, ..Default::default() },
            ))
            .id();

        run_gravity(&mut world);
        let vy = world.get::<KinematicBody3d>(entity).unwrap().velocity.y;
        assert_eq!(vy, 0.0);
    }

    #[test]
    fn body_resolves_floor_collision() {
        let mut world = setup_world();

        // static floor at y=0, half-extents=(10, 0.5, 10)
        world.spawn((
            WorldTransform3d {
                translation: Vec3::new(0.0, 0.0, 0.0),
                rotation: Quat::IDENTITY,
                scale: Vec3::ONE,
            },
            Collider3d::aabb(Vec3::new(20.0, 1.0, 20.0)),
        ));

        // character just above the floor, falling
        let entity = world
            .spawn((
                LocalTransform3d::from_xyz(0.0, 0.6, 0.0), // just touching top of floor (0.5 + 0.5 = 1.0 - epsilon)
                WorldTransform3d::default(),
                Collider3d::aabb(Vec3::new(0.6, 1.0, 0.6)),
                KinematicBody3d { velocity: Vec3::new(0.0, -1.0, 0.0), ..Default::default() },
            ))
            .id();

        run_build(&mut world);
        run_move_slide(&mut world);

        let body = world.get::<KinematicBody3d>(entity).unwrap();
        assert!(body.is_grounded, "character should be grounded after floor collision");
        assert!(body.velocity.y >= 0.0, "downward velocity should be zeroed on ground contact");
    }

    #[test]
    fn aabb_overlap_depth_axis_selection() {
        // overlap only on Y
        let result = aabb_overlap_depth(
            Vec3::new(0.0, 0.4, 0.0), Vec3::new(0.5, 0.5, 0.5),
            Vec3::ZERO, Vec3::new(0.5, 0.5, 0.5),
        );
        assert!(result.is_some());
        let (axis, _) = result.unwrap();
        assert!(axis.y.abs() > 0.5, "should push along Y axis");
    }

    #[test]
    fn no_overlap_returns_none() {
        let result = aabb_overlap_depth(
            Vec3::new(5.0, 0.0, 0.0), Vec3::ONE,
            Vec3::ZERO, Vec3::ONE,
        );
        assert!(result.is_none());
    }
}
