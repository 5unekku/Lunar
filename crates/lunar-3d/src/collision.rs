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

/// a single entry in the collision world snapshot.
///
/// `min_x` / `max_x` are precomputed for the sweep-and-prune broad phase.
#[derive(Debug)]
struct ColliderEntry {
    entity: Entity,
    position: Vec3,
    shape: ColliderShape3d,
    layer: u32,
    mask: u32,
    min_x: f32,
    max_x: f32,
}

impl ColliderEntry {
    fn new(entity: Entity, position: Vec3, shape: ColliderShape3d, layer: u32, mask: u32) -> Self {
        let (min_x, max_x) = match shape {
            ColliderShape3d::Aabb { half_extents } => {
                (position.x - half_extents.x, position.x + half_extents.x)
            }
            ColliderShape3d::Sphere { radius } => (position.x - radius, position.x + radius),
        };
        Self { entity, position, shape, layer, mask, min_x, max_x }
    }

    fn overlaps(&self, other: &Self) -> bool {
        if self.mask & other.layer == 0 || other.mask & self.layer == 0 {
            return false;
        }
        shapes_overlap(self.position, self.shape, other.position, other.shape)
    }
}

fn shapes_overlap(
    pos_a: Vec3,
    shape_a: ColliderShape3d,
    pos_b: Vec3,
    shape_b: ColliderShape3d,
) -> bool {
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
/// entries are sorted by `min_x` so `all_overlaps` can use a sweep-and-prune
/// early exit and skip pairs that can't possibly overlap along X.
///
/// query this from any system in the Update stage or later.
#[derive(Debug, Default, Resource)]
pub struct CollisionWorld3d {
    entries: Vec<ColliderEntry>,
}

impl CollisionWorld3d {
    /// iterator over all entities that overlap `entity` this frame, filtered by layer/mask.
    pub fn overlapping(&self, entity: Entity) -> impl Iterator<Item = Entity> + '_ {
        let target = self.entries.iter().find(|e| e.entity == entity);
        self.entries.iter().filter_map(move |other| {
            if other.entity == entity {
                return None;
            }
            target.is_some_and(|t| t.overlaps(other)).then_some(other.entity)
        })
    }

    /// iterator over all entities whose collider contains `point`.
    pub fn query_point(&self, point: Vec3) -> impl Iterator<Item = Entity> + '_ {
        self.entries.iter().filter_map(move |entry| {
            point_in_shape(point, entry.position, entry.shape).then_some(entry.entity)
        })
    }

    /// iterator over all entities whose collider overlaps a sphere at `center` with `radius`.
    pub fn query_sphere(&self, center: Vec3, radius: f32) -> impl Iterator<Item = Entity> + '_ {
        let query_shape = ColliderShape3d::Sphere { radius };
        self.entries.iter().filter_map(move |entry| {
            shapes_overlap(center, query_shape, entry.position, entry.shape).then_some(entry.entity)
        })
    }

    /// iterator over all entities whose collider overlaps a box at `center` with `half_extents`.
    pub fn query_aabb(
        &self,
        center: Vec3,
        half_extents: Vec3,
    ) -> impl Iterator<Item = Entity> + '_ {
        let query_shape = ColliderShape3d::Aabb { half_extents };
        self.entries.iter().filter_map(move |entry| {
            shapes_overlap(center, query_shape, entry.position, entry.shape).then_some(entry.entity)
        })
    }

    /// iterator over all overlapping pairs this frame. each pair appears exactly once.
    ///
    /// uses a sweep-and-prune broad phase: entries are sorted by `min_x`, so the
    /// inner loop breaks as soon as the next entry's left edge exceeds the current
    /// entry's right edge — skipping all remaining pairs along X.
    pub fn all_overlaps(&self) -> impl Iterator<Item = (Entity, Entity)> + '_ {
        (0..self.entries.len()).flat_map(move |i| {
            let max_x_i = self.entries[i].max_x;
            ((i + 1)..self.entries.len())
                .take_while(move |&j| self.entries[j].min_x < max_x_i)
                .filter_map(move |j| {
                    self.entries[i]
                        .overlaps(&self.entries[j])
                        .then_some((self.entries[i].entity, self.entries[j].entity))
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
/// entries are sorted by `min_x` after insertion to enable sweep-and-prune in `all_overlaps`.
/// runs in the Physics stage so `CollisionWorld3d` is ready for Update systems.
pub fn build_collision_world_3d(
    query: Query<(Entity, &WorldTransform3d, &Collider3d)>,
    mut collision_world: ResMut<CollisionWorld3d>,
) {
    collision_world.entries.clear();
    for (entity, transform, collider) in &query {
        collision_world.entries.push(ColliderEntry::new(
            entity,
            transform.translation,
            collider.shape,
            collider.layer,
            collider.mask,
        ));
    }
    collision_world.entries.sort_unstable_by(|a, b| a.min_x.total_cmp(&b.min_x));
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

    fn run_build(world: &mut World) {
        let mut system = IntoSystem::into_system(build_collision_world_3d);
        system.initialize(world);
        let _ = system.run((), world);
    }

    #[test]
    fn aabb_overlap_detected() {
        let mut world = World::new();
        world.insert_resource(CollisionWorld3d::default());
        let entity_a = spawn_aabb(&mut world, Vec3::ZERO, Vec3::new(2.0, 2.0, 2.0));
        let entity_b = spawn_aabb(&mut world, Vec3::new(1.5, 0.0, 0.0), Vec3::new(2.0, 2.0, 2.0));

        run_build(&mut world);

        let cw = world.resource::<CollisionWorld3d>();
        assert!(cw.overlapping(entity_a).any(|e| e == entity_b));
    }

    #[test]
    fn aabb_no_overlap_when_far() {
        let mut world = World::new();
        world.insert_resource(CollisionWorld3d::default());
        let entity_a = spawn_aabb(&mut world, Vec3::ZERO, Vec3::new(2.0, 2.0, 2.0));
        spawn_aabb(&mut world, Vec3::new(100.0, 0.0, 0.0), Vec3::new(2.0, 2.0, 2.0));

        run_build(&mut world);

        let cw = world.resource::<CollisionWorld3d>();
        assert!(cw.overlapping(entity_a).next().is_none());
    }

    #[test]
    fn sphere_overlap_detected() {
        let mut world = World::new();
        world.insert_resource(CollisionWorld3d::default());
        let entity_a = world.spawn((WorldTransform3d::new(), Collider3d::sphere(1.0))).id();
        let entity_b = world
            .spawn((
                WorldTransform3d {
                    translation: Vec3::new(1.5, 0.0, 0.0),
                    ..WorldTransform3d::new()
                },
                Collider3d::sphere(1.0),
            ))
            .id();

        run_build(&mut world);

        let cw = world.resource::<CollisionWorld3d>();
        assert!(cw.overlapping(entity_a).any(|e| e == entity_b));
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

        run_build(&mut world);

        let cw = world.resource::<CollisionWorld3d>();
        assert!(cw.overlapping(entity_a).any(|e| e == entity_b));
    }

    #[test]
    fn layer_mask_filtering() {
        let mut world = World::new();
        world.insert_resource(CollisionWorld3d::default());
        world.spawn((
            WorldTransform3d::new(),
            Collider3d::aabb(Vec3::ONE).with_layer(1).with_mask(2),
        ));
        let entity_b = world
            .spawn((
                WorldTransform3d {
                    translation: Vec3::new(0.1, 0.0, 0.0),
                    ..WorldTransform3d::new()
                },
                Collider3d::aabb(Vec3::ONE).with_layer(1).with_mask(1),
            ))
            .id();

        run_build(&mut world);

        let cw = world.resource::<CollisionWorld3d>();
        assert!(cw.overlapping(entity_b).next().is_none());
    }

    #[test]
    fn sweep_and_prune_skips_far_pairs() {
        let mut world = World::new();
        world.insert_resource(CollisionWorld3d::default());
        let entity_a = spawn_aabb(&mut world, Vec3::ZERO, Vec3::new(2.0, 2.0, 2.0));
        let entity_b = spawn_aabb(&mut world, Vec3::new(1.5, 0.0, 0.0), Vec3::new(2.0, 2.0, 2.0));
        let entity_c = spawn_aabb(&mut world, Vec3::new(500.0, 0.0, 0.0), Vec3::new(2.0, 2.0, 2.0));

        run_build(&mut world);

        let cw = world.resource::<CollisionWorld3d>();
        let pairs: Vec<_> = cw.all_overlaps().collect();
        assert!(pairs.contains(&(entity_a, entity_b)) || pairs.contains(&(entity_b, entity_a)));
        assert!(!pairs.iter().any(|&(x, y)| x == entity_c || y == entity_c));
    }
}
