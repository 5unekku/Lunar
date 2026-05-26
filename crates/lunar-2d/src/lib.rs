//! 2d-specific systems: transform propagation, sprite animation, and collision detection.
//!
//! this crate is the 2d-specific layer of the engine. 3d games use lunar-3d
//! instead — no 2d code compiles into them.
//!
//! register [`Plugin2d`] in your app to enable 2d transform propagation,
//! sprite animation, and the [`collision::CollisionWorld`] resource.

pub mod animation;
pub mod collision;

pub use animation::{SpriteAnimation, advance_sprite_animations};
pub use collision::{Collider, Collider2dBundle, ColliderShape, CollisionWorld};

use std::collections::HashMap;

use bevy_ecs::prelude::*;
use lunar_core::{App, GamePlugin, Parent};
use lunar_math::{LocalTransform, Vec2, WorldTransform};

use collision::build_collision_world;

/// scratch buffers for `propagate_transforms` — allocated once, reused every frame.
#[derive(Default, Resource)]
struct TransformScratch2d {
    snapshot: Vec<(Entity, LocalTransform, Option<Entity>)>,
    parent_of: HashMap<Entity, Entity>,
    depths: HashMap<Entity, u32>,
}

/// plugin that registers the 2d transform propagation system.
///
/// add this alongside [`lunar_core::HierarchyPlugin`] — hierarchy manages
/// parent/child relationships, plugin2d propagates 2d transforms through them.
pub struct Plugin2d;

impl GamePlugin for Plugin2d {
    fn name(&self) -> &'static str {
        "2d"
    }

    fn build(&mut self, app: &mut App) {
        app.insert_resource(CollisionWorld::default());
        app.insert_resource(TransformScratch2d::default());
        app.add_system_to_stage(lunar_core::UpdateStage::Physics, build_collision_world);
        app.add_system_to_stage(lunar_core::UpdateStage::Update, propagate_transforms);
        app.add_system_to_stage(
            lunar_core::UpdateStage::Update,
            animation::advance_sprite_animations,
        );
    }
}

/// exclusive system that propagates 2d transforms from parents to children.
///
/// uses a persistent scratch resource to avoid per-frame heap allocations.
/// topological sort gives O(N) propagation regardless of hierarchy depth.
pub fn propagate_transforms(world: &mut World) {
    let mut scratch = world
        .remove_resource::<TransformScratch2d>()
        .unwrap_or_default();

    scratch.snapshot.clear();
    scratch.parent_of.clear();
    scratch.depths.clear();

    scratch.snapshot.extend(
        world
            .query::<(Entity, &LocalTransform, Option<&Parent>)>()
            .iter(world)
            .map(|(entity, local, parent)| (entity, *local, parent.map(|p| p.0))),
    );

    for &(entity, _, parent) in &scratch.snapshot {
        if let Some(parent_entity) = parent {
            scratch.parent_of.insert(entity, parent_entity);
        }
    }

    // compute depths — iterate snapshot by index to avoid borrow conflict
    for i in 0..scratch.snapshot.len() {
        let entity = scratch.snapshot[i].0;
        depth_of(entity, &scratch.parent_of, &mut scratch.depths);
    }

    scratch
        .snapshot
        .sort_by_key(|(entity, _, _)| scratch.depths.get(entity).copied().unwrap_or(0));

    for i in 0..scratch.snapshot.len() {
        let (entity, local, parent_entity) = scratch.snapshot[i];
        let world_transform = if let Some(parent) = parent_entity {
            if let Some(parent_wt) = world.get::<WorldTransform>(parent).copied() {
                compute_world_transform(&parent_wt, &local)
            } else {
                WorldTransform {
                    translation: local.translation,
                    rotation: local.rotation,
                    scale: local.scale,
                }
            }
        } else {
            WorldTransform {
                translation: local.translation,
                rotation: local.rotation,
                scale: local.scale,
            }
        };

        if let Some(mut wt) = world.get_mut::<WorldTransform>(entity) {
            *wt = world_transform;
        } else {
            world.entity_mut(entity).insert(world_transform);
        }
    }

    world.insert_resource(scratch);
}

/// memoized depth lookup — O(N) total across all entities (each computed at most once)
fn depth_of(
    entity: Entity,
    parent_of: &HashMap<Entity, Entity>,
    cache: &mut HashMap<Entity, u32>,
) -> u32 {
    if let Some(&d) = cache.get(&entity) {
        return d;
    }
    let d = parent_of
        .get(&entity)
        .map(|&parent| depth_of(parent, parent_of, cache) + 1)
        .unwrap_or(0);
    cache.insert(entity, d);
    d
}

fn compute_world_transform(parent: &WorldTransform, local: &LocalTransform) -> WorldTransform {
    let scaled_x = local.translation.x * parent.scale.x;
    let scaled_y = local.translation.y * parent.scale.y;

    let cos = parent.rotation.cos();
    let sin = parent.rotation.sin();
    let rotated_x = scaled_x.mul_add(cos, -scaled_y * sin);
    let rotated_y = scaled_x.mul_add(sin, scaled_y * cos);

    WorldTransform {
        translation: Vec2::new(
            parent.translation.x + rotated_x,
            parent.translation.y + rotated_y,
        ),
        rotation: parent.rotation + local.rotation,
        scale: Vec2::new(
            parent.scale.x * local.scale.x,
            parent.scale.y * local.scale.y,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lunar_core::Parent;

    #[test]
    fn compute_world_transform_no_parent() {
        let parent = WorldTransform::from_xy(0.0, 0.0);
        let local = LocalTransform::from_xy(10.0, 20.0);
        let world_transform = compute_world_transform(&parent, &local);
        assert!((world_transform.translation.x - 10.0).abs() < 0.001);
        assert!((world_transform.translation.y - 20.0).abs() < 0.001);
        assert!((world_transform.rotation - 0.0).abs() < 0.001);
        assert!((world_transform.scale.x - 1.0).abs() < 0.001);
        assert!((world_transform.scale.y - 1.0).abs() < 0.001);
    }

    #[test]
    fn compute_world_transform_with_parent_rotation() {
        let parent = WorldTransform {
            translation: Vec2::new(100.0, 100.0),
            rotation: std::f32::consts::PI / 2.0,
            scale: Vec2::ONE,
        };
        let local = LocalTransform::from_xy(10.0, 0.0);
        let world_transform = compute_world_transform(&parent, &local);
        assert!((world_transform.translation.x - 100.0).abs() < 0.001);
        assert!((world_transform.translation.y - 110.0).abs() < 0.001);
    }

    #[test]
    fn compute_world_transform_with_parent_scale() {
        let parent = WorldTransform {
            translation: Vec2::ZERO,
            rotation: 0.0,
            scale: Vec2::new(2.0, 3.0),
        };
        let local = LocalTransform {
            translation: Vec2::new(5.0, 4.0),
            rotation: 0.0,
            scale: Vec2::new(1.0, 1.0),
        };
        let world_transform = compute_world_transform(&parent, &local);
        assert!((world_transform.translation.x - 10.0).abs() < 0.001);
        assert!((world_transform.translation.y - 12.0).abs() < 0.001);
        assert!((world_transform.scale.x - 2.0).abs() < 0.001);
        assert!((world_transform.scale.y - 3.0).abs() < 0.001);
    }

    #[test]
    fn propagate_transforms_writes_immediately() {
        let mut world = World::new();
        world.insert_resource(TransformScratch2d::default());
        let parent = world.spawn(LocalTransform::from_xy(100.0, 0.0)).id();
        let child = world
            .spawn((LocalTransform::from_xy(10.0, 0.0), Parent(parent)))
            .id();

        propagate_transforms(&mut world);

        let parent_wt = world
            .get::<WorldTransform>(parent)
            .expect("parent WorldTransform");
        assert!((parent_wt.translation.x - 100.0).abs() < 0.001);

        let child_wt = world
            .get::<WorldTransform>(child)
            .expect("child WorldTransform");
        assert!((child_wt.translation.x - 110.0).abs() < 0.001);
    }
}
