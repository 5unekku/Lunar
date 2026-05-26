//! 3d collision detection — AABB and sphere shapes, overlap queries.
//!
//! no physics simulation (no rigid bodies, velocity integration, gravity).
//! this module answers the question "what overlaps what" — game logic decides
//! what to do about it.
//!
//! # usage
//!
//! ```ignore
//! use lunar_3d::collision::{Collider3d, ColliderShape3d, CollisionWorld3d};
//!
//! commands.spawn((
//!     LocalTransform3d::from_xyz(0.0, 1.0, 0.0),
//!     WorldTransform3d::default(),
//!     Collider3d::aabb(Vec3::new(1.0, 2.0, 1.0)),
//! ));
//!
//! fn check_hits(world: Res<CollisionWorld3d>) {
//!     for (entity_a, entity_b) in world.all_overlaps() {
//!         // handle collision
//!     }
//! }
//! ```

use bevy_ecs::prelude::*;
use lunar_math::Vec3;

use crate::transform::WorldTransform3d;

/// shape variant for a [`Collider3d`] component.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColliderShape3d {
    /// axis-aligned bounding box. `half_extents` is half the width/height/depth.
    Aabb { half_extents: Vec3 },
    /// sphere centered on the entity's world position.
    Sphere { radius: f32 },
}

/// component that makes an entity participate in 3d collision detection.
///
/// pair with `WorldTransform3d` — the collision world uses world-space position.
#[derive(Debug, Clone, Component)]
pub struct Collider3d {
    pub shape: ColliderShape3d,
    /// bitmask — which collision layers this collider belongs to.
    pub layer: u32,
    /// bitmask — which layers this collider checks against.
    pub mask: u32,
}

impl Collider3d {
    /// axis-aligned bounding box with the given full size (half_extents = size / 2).
    #[must_use]
    pub fn aabb(size: Vec3) -> Self {
        Self {
            shape: ColliderShape3d::Aabb {
                half_extents: size * 0.5,
            },
            layer: 1,
            mask: 1,
        }
    }

    /// sphere with the given radius.
    #[must_use]
    pub fn sphere(radius: f32) -> Self {
        Self {
            shape: ColliderShape3d::Sphere { radius },
            layer: 1,
            mask: 1,
        }
    }

    /// set the collision layer (fluent).
    #[must_use]
    pub fn with_layer(mut self, layer: u32) -> Self {
        self.layer = layer;
        self
    }

    /// set the collision mask (fluent).
    #[must_use]
    pub fn with_mask(mut self, mask: u32) -> Self {
        self.mask = mask;
        self
    }
}

#[derive(Debug)]
struct ColliderEntry {
    entity: Entity,
    position: Vec3,
    shape: ColliderShape3d,
    layer: u32,
    mask: u32,
}

impl ColliderEntry {
    fn overlaps(&self, other: &Self) -> bool {
        if self.mask & other.layer == 0 || other.mask & self.layer == 0 {
            return false;
        }
        shapes_overlap(self.position, self.shape, other.position, other.shape)
    }
}

fn shapes_overlap(pos_a: Vec3, shape_a: ColliderShape3d, pos_b: Vec3, shape_b: ColliderShape3d) -> bool {
    match (shape_a, shape_b) {
        (
            ColliderShape3d::Aabb { half_extents: half_a },
            ColliderShape3d::Aabb { half_extents: half_b },
        ) => {
            (pos_a.x - pos_b.x).abs() < half_a.x + half_b.x
                && (pos_a.y - pos_b.y).abs() < half_a.y + half_b.y
                && (pos_a.z - pos_b.z).abs() < half_a.z + half_b.z
        }
        (ColliderShape3d::Sphere { radius: ra }, ColliderShape3d::Sphere { radius: rb }) => {
            (pos_a - pos_b).length_squared() < (ra + rb) * (ra + rb)
        }
        (ColliderShape3d::Aabb { half_extents }, ColliderShape3d::Sphere { radius })
        | (ColliderShape3d::Sphere { radius }, ColliderShape3d::Aabb { half_extents }) => {
            let (aabb_pos, sphere_pos) = if matches!(shape_a, ColliderShape3d::Aabb { .. }) {
                (pos_a, pos_b)
            } else {
                (pos_b, pos_a)
            };
            let closest = Vec3::new(
                sphere_pos.x.clamp(aabb_pos.x - half_extents.x, aabb_pos.x + half_extents.x),
                sphere_pos.y.clamp(aabb_pos.y - half_extents.y, aabb_pos.y + half_extents.y),
                sphere_pos.z.clamp(aabb_pos.z - half_extents.z, aabb_pos.z + half_extents.z),
            );
            (sphere_pos - closest).length_squared() < radius * radius
        }
    }
}

/// resource rebuilt every physics tick — holds the current frame's collider snapshot.
///
/// query this from any system in the Update stage or later.
#[derive(Debug, Default, Resource)]
pub struct CollisionWorld3d {
    entries: Vec<ColliderEntry>,
}

impl CollisionWorld3d {
    /// all entities that overlap `entity` this frame, filtered by layer/mask.
    #[must_use]
    pub fn overlapping(&self, entity: Entity) -> Vec<Entity> {
        let Some(target) = self.entries.iter().find(|entry| entry.entity == entity) else {
            return Vec::new();
        };
        self.entries
            .iter()
            .filter(|other| other.entity != entity && target.overlaps(other))
            .map(|other| other.entity)
            .collect()
    }

    /// all entities whose collider contains `point`.
    #[must_use]
    pub fn query_point(&self, point: Vec3) -> Vec<Entity> {
        self.entries
            .iter()
            .filter(|entry| point_in_shape(point, entry.position, entry.shape))
            .map(|entry| entry.entity)
            .collect()
    }

    /// all entities whose AABB or sphere overlaps a sphere centered at `center` with `radius`.
    #[must_use]
    pub fn query_sphere(&self, center: Vec3, radius: f32) -> Vec<Entity> {
        let query_shape = ColliderShape3d::Sphere { radius };
        self.entries
            .iter()
            .filter(|entry| shapes_overlap(center, query_shape, entry.position, entry.shape))
            .map(|entry| entry.entity)
            .collect()
    }

    /// all entities whose collider overlaps a box centered at `center` with given `half_extents`.
    #[must_use]
    pub fn query_aabb(&self, center: Vec3, half_extents: Vec3) -> Vec<Entity> {
        let query_shape = ColliderShape3d::Aabb { half_extents };
        self.entries
            .iter()
            .filter(|entry| shapes_overlap(center, query_shape, entry.position, entry.shape))
            .map(|entry| entry.entity)
            .collect()
    }

    /// iterator over all overlapping pairs this frame. each pair appears exactly once.
    pub fn all_overlaps(&self) -> impl Iterator<Item = (Entity, Entity)> + '_ {
        (0..self.entries.len()).flat_map(move |i| {
            ((i + 1)..self.entries.len()).filter_map(move |j| {
                if self.entries[i].overlaps(&self.entries[j]) {
                    Some((self.entries[i].entity, self.entries[j].entity))
                } else {
                    None
                }
            })
        })
    }
}

fn point_in_shape(point: Vec3, position: Vec3, shape: ColliderShape3d) -> bool {
    match shape {
        ColliderShape3d::Aabb { half_extents } => {
            (point.x - position.x).abs() <= half_extents.x
                && (point.y - position.y).abs() <= half_extents.y
                && (point.z - position.z).abs() <= half_extents.z
        }
        ColliderShape3d::Sphere { radius } => {
            (point - position).length_squared() <= radius * radius
        }
    }
}

/// system that rebuilds [`CollisionWorld3d`] from all entities with `Collider3d + WorldTransform3d`.
///
/// runs in the Physics stage so `CollisionWorld3d` is ready for Update systems.
pub fn build_collision_world_3d(
    query: Query<(Entity, &WorldTransform3d, &Collider3d)>,
    mut collision_world: ResMut<CollisionWorld3d>,
) {
    collision_world.entries.clear();
    for (entity, transform, collider) in &query {
        collision_world.entries.push(ColliderEntry {
            entity,
            position: transform.translation,
            shape: collider.shape,
            layer: collider.layer,
            mask: collider.mask,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_ecs::system::IntoSystem;
    use lunar_math::Vec3;

    fn spawn_aabb(world: &mut World, pos: Vec3, size: Vec3) -> Entity {
        world
            .spawn((
                WorldTransform3d {
                    translation: pos,
                    ..WorldTransform3d::new()
                },
                Collider3d::aabb(size),
            ))
            .id()
    }

    #[test]
    fn aabb_overlap_detected() {
        let mut world = World::new();
        world.insert_resource(CollisionWorld3d::default());
        let entity_a = spawn_aabb(&mut world, Vec3::ZERO, Vec3::new(2.0, 2.0, 2.0));
        let entity_b = spawn_aabb(&mut world, Vec3::new(1.5, 0.0, 0.0), Vec3::new(2.0, 2.0, 2.0));

        let mut system = IntoSystem::into_system(build_collision_world_3d);
        system.initialize(&mut world);
        let _ = system.run((), &mut world);

        let cw = world.resource::<CollisionWorld3d>();
        assert!(cw.overlapping(entity_a).contains(&entity_b));
    }

    #[test]
    fn aabb_no_overlap_when_far() {
        let mut world = World::new();
        world.insert_resource(CollisionWorld3d::default());
        let entity_a = spawn_aabb(&mut world, Vec3::ZERO, Vec3::new(2.0, 2.0, 2.0));
        spawn_aabb(&mut world, Vec3::new(100.0, 0.0, 0.0), Vec3::new(2.0, 2.0, 2.0));

        let mut system = IntoSystem::into_system(build_collision_world_3d);
        system.initialize(&mut world);
        let _ = system.run((), &mut world);

        let cw = world.resource::<CollisionWorld3d>();
        assert!(cw.overlapping(entity_a).is_empty());
    }

    #[test]
    fn sphere_overlap_detected() {
        let mut world = World::new();
        world.insert_resource(CollisionWorld3d::default());
        let entity_a = world
            .spawn((WorldTransform3d::new(), Collider3d::sphere(1.0)))
            .id();
        let entity_b = world
            .spawn((
                WorldTransform3d {
                    translation: Vec3::new(1.5, 0.0, 0.0),
                    ..WorldTransform3d::new()
                },
                Collider3d::sphere(1.0),
            ))
            .id();

        let mut system = IntoSystem::into_system(build_collision_world_3d);
        system.initialize(&mut world);
        let _ = system.run((), &mut world);

        let cw = world.resource::<CollisionWorld3d>();
        assert!(cw.overlapping(entity_a).contains(&entity_b));
    }

    #[test]
    fn aabb_sphere_overlap() {
        let mut world = World::new();
        world.insert_resource(CollisionWorld3d::default());
        let entity_a = spawn_aabb(&mut world, Vec3::ZERO, Vec3::new(2.0, 2.0, 2.0));
        let entity_b = world
            .spawn((
                WorldTransform3d {
                    translation: Vec3::new(1.2, 0.0, 0.0),
                    ..WorldTransform3d::new()
                },
                Collider3d::sphere(0.5),
            ))
            .id();

        let mut system = IntoSystem::into_system(build_collision_world_3d);
        system.initialize(&mut world);
        let _ = system.run((), &mut world);

        let cw = world.resource::<CollisionWorld3d>();
        assert!(cw.overlapping(entity_a).contains(&entity_b));
    }

    #[test]
    fn layer_mask_filtering() {
        let mut world = World::new();
        world.insert_resource(CollisionWorld3d::default());
        // entity_a: layer 1, only checks layer 2
        world.spawn((
            WorldTransform3d::new(),
            Collider3d::aabb(Vec3::ONE).with_layer(1).with_mask(2),
        ));
        // entity_b: layer 1, only checks layer 1 (but a doesn't check layer 1)
        let entity_b = world
            .spawn((
                WorldTransform3d {
                    translation: Vec3::new(0.1, 0.0, 0.0),
                    ..WorldTransform3d::new()
                },
                Collider3d::aabb(Vec3::ONE).with_layer(1).with_mask(1),
            ))
            .id();

        let mut system = IntoSystem::into_system(build_collision_world_3d);
        system.initialize(&mut world);
        let _ = system.run((), &mut world);

        let cw = world.resource::<CollisionWorld3d>();
        assert!(cw.overlapping(entity_b).is_empty());
    }
}
