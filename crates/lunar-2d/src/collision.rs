//! 2d collision detection — AABB and circle shapes, overlap queries.
//!
//! no physics simulation (no rigid bodies, velocity integration, gravity).
//! this module answers the question "what overlaps what" — game logic decides
//! what to do about it.
//!
//! # usage
//!
//! ```ignore
//! use lunar_2d::collision::{Collider, ColliderShape, CollisionWorld};
//!
//! // spawn a collider
//! commands.spawn((
//!     Transform::from_xy(0.0, 0.0),
//!     Collider::aabb(Vec2::new(16.0, 16.0)),
//! ));
//!
//! // query overlaps in a system
//! fn check_hits(world: Res<CollisionWorld>) {
//!     for (entity, others) in world.all_overlaps() {
//!         // handle collision
//!     }
//! }
//! ```

use bevy_ecs::prelude::*;
use lunar_math::{Transform, Vec2};

/// shape variant for a [`Collider`] component.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColliderShape {
    /// axis-aligned bounding box. `half_extents` is half the width/height.
    Aabb { half_extents: Vec2 },
    /// circle centered on the entity's transform position.
    Circle { radius: f32 },
}

/// component that makes an entity participate in 2d collision detection.
///
/// attach alongside a [`Transform`] component. the `CollisionWorld` resource
/// is rebuilt from all entities that have both every physics tick.
#[derive(Debug, Clone, Component)]
pub struct Collider {
    pub shape: ColliderShape,
    /// bitmask — which collision layers this collider belongs to.
    pub layer: u32,
    /// bitmask — which layers this collider checks against.
    pub mask: u32,
}

impl Collider {
    /// axis-aligned bounding box with the given full size (half_extents = size / 2).
    #[must_use]
    pub fn aabb(size: Vec2) -> Self {
        Self {
            shape: ColliderShape::Aabb {
                half_extents: size * 0.5,
            },
            layer: 1,
            mask: 1,
        }
    }

    /// circle with the given radius.
    #[must_use]
    pub fn circle(radius: f32) -> Self {
        Self {
            shape: ColliderShape::Circle { radius },
            layer: 1,
            mask: 1,
        }
    }

    /// set the collision layer (builder pattern).
    #[must_use]
    pub fn with_layer(mut self, layer: u32) -> Self {
        self.layer = layer;
        self
    }

    /// set the collision mask (builder pattern).
    #[must_use]
    pub fn with_mask(mut self, mask: u32) -> Self {
        self.mask = mask;
        self
    }
}

/// a single entry in the collision world snapshot.
#[derive(Debug, Clone)]
struct ColliderEntry {
    entity: Entity,
    position: Vec2,
    shape: ColliderShape,
    layer: u32,
    mask: u32,
}

impl ColliderEntry {
    fn overlaps(&self, other: &Self) -> bool {
        // layer/mask filter: both must agree
        if self.mask & other.layer == 0 || other.mask & self.layer == 0 {
            return false;
        }
        shapes_overlap(self.position, &self.shape, other.position, &other.shape)
    }
}

fn shapes_overlap(
    pos_a: Vec2,
    shape_a: &ColliderShape,
    pos_b: Vec2,
    shape_b: &ColliderShape,
) -> bool {
    match (shape_a, shape_b) {
        (
            ColliderShape::Aabb {
                half_extents: half_a,
            },
            ColliderShape::Aabb {
                half_extents: half_b,
            },
        ) => {
            (pos_a.x - pos_b.x).abs() < half_a.x + half_b.x
                && (pos_a.y - pos_b.y).abs() < half_a.y + half_b.y
        }
        (ColliderShape::Circle { radius: ra }, ColliderShape::Circle { radius: rb }) => {
            let distance_sq = (pos_a - pos_b).length_squared();
            distance_sq < (ra + rb) * (ra + rb)
        }
        (ColliderShape::Aabb { half_extents }, ColliderShape::Circle { radius })
        | (ColliderShape::Circle { radius }, ColliderShape::Aabb { half_extents }) => {
            let (aabb_pos, circle_pos) = if matches!(shape_a, ColliderShape::Aabb { .. }) {
                (pos_a, pos_b)
            } else {
                (pos_b, pos_a)
            };
            let closest = Vec2::new(
                circle_pos
                    .x
                    .clamp(aabb_pos.x - half_extents.x, aabb_pos.x + half_extents.x),
                circle_pos
                    .y
                    .clamp(aabb_pos.y - half_extents.y, aabb_pos.y + half_extents.y),
            );
            let distance_sq = (circle_pos - closest).length_squared();
            distance_sq < radius * radius
        }
    }
}

/// resource rebuilt every physics tick — holds the current frame's collider snapshot.
///
/// read this from any system in the Update stage or later to query overlaps.
#[derive(Debug, Default, Resource)]
pub struct CollisionWorld {
    entries: Vec<ColliderEntry>,
}

impl CollisionWorld {
    /// all entities that overlap `entity` this frame, filtered by layer/mask.
    #[must_use]
    pub fn overlapping(&self, entity: Entity) -> Vec<Entity> {
        let Some(target) = self.entries.iter().find(|e| e.entity == entity) else {
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
    pub fn query_point(&self, point: Vec2) -> Vec<Entity> {
        self.entries
            .iter()
            .filter(|entry| point_in_shape(point, entry.position, &entry.shape))
            .map(|entry| entry.entity)
            .collect()
    }

    /// all entities whose collider overlaps `rect` (given as center + half_extents).
    #[must_use]
    pub fn query_rect(&self, center: Vec2, half_extents: Vec2) -> Vec<Entity> {
        let rect_shape = ColliderShape::Aabb { half_extents };
        self.entries
            .iter()
            .filter(|entry| shapes_overlap(center, &rect_shape, entry.position, &entry.shape))
            .map(|entry| entry.entity)
            .collect()
    }

    /// iterator over all overlapping pairs this frame. each pair appears once.
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

fn point_in_shape(point: Vec2, position: Vec2, shape: &ColliderShape) -> bool {
    match shape {
        ColliderShape::Aabb { half_extents } => {
            (point.x - position.x).abs() <= half_extents.x
                && (point.y - position.y).abs() <= half_extents.y
        }
        ColliderShape::Circle { radius } => (point - position).length_squared() <= radius * radius,
    }
}

/// system that rebuilds [`CollisionWorld`] from all entities with `Collider + Transform`.
///
/// runs in the Physics stage so `CollisionWorld` is ready for Update systems.
pub fn build_collision_world(
    query: Query<(Entity, &Transform, &Collider)>,
    mut collision_world: ResMut<CollisionWorld>,
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
    use lunar_math::Transform;

    fn make_world_with_aabbs() -> (World, Entity, Entity) {
        let mut world = World::new();
        world.insert_resource(CollisionWorld::default());
        let entity_a = world
            .spawn((
                Transform::from_xy(0.0, 0.0),
                Collider::aabb(Vec2::new(20.0, 20.0)),
            ))
            .id();
        let entity_b = world
            .spawn((
                Transform::from_xy(15.0, 0.0),
                Collider::aabb(Vec2::new(20.0, 20.0)),
            ))
            .id();
        (world, entity_a, entity_b)
    }

    #[test]
    fn aabb_overlap_detected() {
        let (mut world, entity_a, entity_b) = make_world_with_aabbs();
        let mut system = IntoSystem::into_system(build_collision_world);
        system.initialize(&mut world);
        let _ = system.run((), &mut world);

        let collision_world = world.resource::<CollisionWorld>();
        let overlaps = collision_world.overlapping(entity_a);
        assert!(overlaps.contains(&entity_b), "a and b should overlap");
    }

    #[test]
    fn aabb_no_overlap_when_far() {
        let mut world = World::new();
        world.insert_resource(CollisionWorld::default());
        let entity_a = world
            .spawn((
                Transform::from_xy(0.0, 0.0),
                Collider::aabb(Vec2::new(10.0, 10.0)),
            ))
            .id();
        let _entity_b = world
            .spawn((
                Transform::from_xy(100.0, 0.0),
                Collider::aabb(Vec2::new(10.0, 10.0)),
            ))
            .id();

        let mut system = IntoSystem::into_system(build_collision_world);
        system.initialize(&mut world);
        let _ = system.run((), &mut world);

        let collision_world = world.resource::<CollisionWorld>();
        let overlaps = collision_world.overlapping(entity_a);
        assert!(overlaps.is_empty(), "far-apart aabbs should not overlap");
    }

    #[test]
    fn circle_overlap_detected() {
        let mut world = World::new();
        world.insert_resource(CollisionWorld::default());
        let entity_a = world
            .spawn((Transform::from_xy(0.0, 0.0), Collider::circle(10.0)))
            .id();
        let entity_b = world
            .spawn((Transform::from_xy(15.0, 0.0), Collider::circle(10.0)))
            .id();

        let mut system = IntoSystem::into_system(build_collision_world);
        system.initialize(&mut world);
        let _ = system.run((), &mut world);

        let collision_world = world.resource::<CollisionWorld>();
        let overlaps = collision_world.overlapping(entity_a);
        assert!(
            overlaps.contains(&entity_b),
            "circles at distance 15 with radius 10 each should overlap"
        );
    }

    #[test]
    fn layer_mask_filtering() {
        let mut world = World::new();
        world.insert_resource(CollisionWorld::default());
        // entity_a is on layer 1, only checks layer 2
        world.spawn((
            Transform::from_xy(0.0, 0.0),
            Collider::aabb(Vec2::new(20.0, 20.0))
                .with_layer(1)
                .with_mask(2),
        ));
        // entity_b is on layer 1, only checks layer 1 — but a doesn't check layer 1
        let entity_b = world
            .spawn((
                Transform::from_xy(5.0, 0.0),
                Collider::aabb(Vec2::new(20.0, 20.0))
                    .with_layer(1)
                    .with_mask(1),
            ))
            .id();

        let mut system = IntoSystem::into_system(build_collision_world);
        system.initialize(&mut world);
        let _ = system.run((), &mut world);

        let collision_world = world.resource::<CollisionWorld>();
        // b checks layer 1 which a is on, but a's mask=2 doesn't include b's layer=1
        // so the pair is filtered out
        let overlaps = collision_world.overlapping(entity_b);
        assert!(
            overlaps.is_empty(),
            "layer/mask mismatch should suppress overlap"
        );
    }

    #[test]
    fn query_point_hits_aabb() {
        let (mut world, entity_a, _) = make_world_with_aabbs();
        let mut system = IntoSystem::into_system(build_collision_world);
        system.initialize(&mut world);
        let _ = system.run((), &mut world);

        let collision_world = world.resource::<CollisionWorld>();
        let hits = collision_world.query_point(Vec2::new(5.0, 5.0));
        assert!(hits.contains(&entity_a));
    }
}
