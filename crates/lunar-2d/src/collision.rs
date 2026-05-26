//! 2d collision detection — AABB and circle shapes, overlap queries.
//!
//! no physics simulation (no rigid bodies, velocity integration, gravity).
//! this module answers the question "what overlaps what" — game logic decides
//! what to do about it.
//!
//! # usage
//!
//! ```ignore
//! use lunar_2d::collision::{Collider, Collider2dBundle, ColliderShape, CollisionWorld};
//!
//! // spawn a collider using the bundle
//! commands.spawn(Collider2dBundle {
//!     transform: Transform::from_xy(0.0, 0.0),
//!     collider: Collider::aabb(Vec2::new(16.0, 16.0)),
//! });
//!
//! // query overlaps in a system
//! fn check_hits(world: Res<CollisionWorld>) {
//!     for (a, b) in world.all_overlaps() {
//!         // handle collision
//!     }
//! }
//! ```

use bevy_ecs::bundle::Bundle;
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
/// attach alongside a [`Transform`] component, or use [`Collider2dBundle`].
/// the `CollisionWorld` resource is rebuilt from all entities that have both every physics tick.
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

/// convenience bundle — pairs a [`Transform`] with a [`Collider`].
#[derive(Bundle)]
pub struct Collider2dBundle {
    pub transform: Transform,
    pub collider: Collider,
}

/// a single entry in the collision world snapshot.
///
/// `min_x` / `max_x` are precomputed for the sweep-and-prune broad phase.
#[derive(Debug, Clone)]
struct ColliderEntry {
    entity: Entity,
    position: Vec2,
    shape: ColliderShape,
    layer: u32,
    mask: u32,
    min_x: f32,
    max_x: f32,
}

impl ColliderEntry {
    fn new(entity: Entity, position: Vec2, shape: ColliderShape, layer: u32, mask: u32) -> Self {
        let (min_x, max_x) = match shape {
            ColliderShape::Aabb { half_extents } => {
                (position.x - half_extents.x, position.x + half_extents.x)
            }
            ColliderShape::Circle { radius } => (position.x - radius, position.x + radius),
        };
        Self { entity, position, shape, layer, mask, min_x, max_x }
    }

    fn overlaps(&self, other: &Self) -> bool {
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
            ColliderShape::Aabb { half_extents: half_a },
            ColliderShape::Aabb { half_extents: half_b },
        ) => {
            (pos_a.x - pos_b.x).abs() < half_a.x + half_b.x
                && (pos_a.y - pos_b.y).abs() < half_a.y + half_b.y
        }
        (ColliderShape::Circle { radius: ra }, ColliderShape::Circle { radius: rb }) => {
            (pos_a - pos_b).length_squared() < (ra + rb) * (ra + rb)
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
            (circle_pos - closest).length_squared() < radius * radius
        }
    }
}

/// resource rebuilt every physics tick — holds the current frame's collider snapshot.
///
/// entries are sorted by `min_x` so `all_overlaps` can use a sweep-and-prune
/// early exit and skip pairs that can't possibly overlap along X.
///
/// read this from any system in the Update stage or later to query overlaps.
#[derive(Debug, Default, Resource)]
pub struct CollisionWorld {
    entries: Vec<ColliderEntry>,
}

impl CollisionWorld {
    /// iterator over all entities that overlap `entity` this frame, filtered by layer/mask.
    pub fn overlapping(&self, entity: Entity) -> impl Iterator<Item = Entity> + '_ {
        let target = self.entries.iter().find(|e| e.entity == entity);
        self.entries.iter().filter_map(move |other| {
            if other.entity == entity {
                return None;
            }
            let overlaps = target.is_some_and(|t| t.overlaps(other));
            overlaps.then_some(other.entity)
        })
    }

    /// iterator over all entities whose collider contains `point`.
    pub fn query_point(&self, point: Vec2) -> impl Iterator<Item = Entity> + '_ {
        self.entries.iter().filter_map(move |entry| {
            point_in_shape(point, entry.position, &entry.shape).then_some(entry.entity)
        })
    }

    /// iterator over all entities whose collider overlaps `rect` (center + half_extents).
    pub fn query_rect(
        &self,
        center: Vec2,
        half_extents: Vec2,
    ) -> impl Iterator<Item = Entity> + '_ {
        let rect_shape = ColliderShape::Aabb { half_extents };
        self.entries.iter().filter_map(move |entry| {
            shapes_overlap(center, &rect_shape, entry.position, &entry.shape)
                .then_some(entry.entity)
        })
    }

    /// iterator over all overlapping pairs this frame. each pair appears once.
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
/// entries are sorted by `min_x` after insertion to enable sweep-and-prune in `all_overlaps`.
/// runs in the Physics stage so `CollisionWorld` is ready for Update systems.
pub fn build_collision_world(
    query: Query<(Entity, &Transform, &Collider)>,
    mut collision_world: ResMut<CollisionWorld>,
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

    fn run_build(world: &mut World) {
        let mut system = IntoSystem::into_system(build_collision_world);
        system.initialize(world);
        let _ = system.run((), world);
    }

    #[test]
    fn aabb_overlap_detected() {
        let (mut world, entity_a, entity_b) = make_world_with_aabbs();
        run_build(&mut world);
        let collision_world = world.resource::<CollisionWorld>();
        assert!(collision_world.overlapping(entity_a).any(|e| e == entity_b));
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
        world.spawn((
            Transform::from_xy(100.0, 0.0),
            Collider::aabb(Vec2::new(10.0, 10.0)),
        ));

        run_build(&mut world);

        let collision_world = world.resource::<CollisionWorld>();
        assert!(collision_world.overlapping(entity_a).next().is_none());
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

        run_build(&mut world);

        let collision_world = world.resource::<CollisionWorld>();
        assert!(collision_world.overlapping(entity_a).any(|e| e == entity_b));
    }

    #[test]
    fn layer_mask_filtering() {
        let mut world = World::new();
        world.insert_resource(CollisionWorld::default());
        world.spawn((
            Transform::from_xy(0.0, 0.0),
            Collider::aabb(Vec2::new(20.0, 20.0))
                .with_layer(1)
                .with_mask(2),
        ));
        let entity_b = world
            .spawn((
                Transform::from_xy(5.0, 0.0),
                Collider::aabb(Vec2::new(20.0, 20.0))
                    .with_layer(1)
                    .with_mask(1),
            ))
            .id();

        run_build(&mut world);

        let collision_world = world.resource::<CollisionWorld>();
        assert!(collision_world.overlapping(entity_b).next().is_none());
    }

    #[test]
    fn query_point_hits_aabb() {
        let (mut world, entity_a, _) = make_world_with_aabbs();
        run_build(&mut world);
        let collision_world = world.resource::<CollisionWorld>();
        assert!(collision_world.query_point(Vec2::new(5.0, 5.0)).any(|e| e == entity_a));
    }

    #[test]
    fn sweep_and_prune_skips_far_pairs() {
        // three AABBs: a and c are far apart (no x overlap), b is between them
        let mut world = World::new();
        world.insert_resource(CollisionWorld::default());
        let entity_a = world
            .spawn((Transform::from_xy(0.0, 0.0), Collider::aabb(Vec2::new(10.0, 10.0))))
            .id();
        let entity_b = world
            .spawn((Transform::from_xy(8.0, 0.0), Collider::aabb(Vec2::new(10.0, 10.0))))
            .id();
        let entity_c = world
            .spawn((Transform::from_xy(200.0, 0.0), Collider::aabb(Vec2::new(10.0, 10.0))))
            .id();

        run_build(&mut world);

        let collision_world = world.resource::<CollisionWorld>();
        let pairs: Vec<_> = collision_world.all_overlaps().collect();
        assert!(pairs.contains(&(entity_a, entity_b)) || pairs.contains(&(entity_b, entity_a)));
        assert!(!pairs.iter().any(|&(x, y)| x == entity_c || y == entity_c));
    }

    #[test]
    fn collider2d_bundle_spawns() {
        let mut world = World::new();
        world.insert_resource(CollisionWorld::default());
        let entity = world
            .spawn(Collider2dBundle {
                transform: Transform::from_xy(1.0, 2.0),
                collider: Collider::circle(5.0),
            })
            .id();
        assert!(world.get::<Collider>(entity).is_some());
        assert!(world.get::<Transform>(entity).is_some());
    }
}
