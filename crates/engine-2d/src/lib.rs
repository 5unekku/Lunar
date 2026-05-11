//! 2d transform propagation systems.
//!
//! this crate is the 2d-specific layer of the engine. 3d games use engine-3d
//! instead — no 2d propagation code compiles into them.
//!
//! register [`Plugin2d`] in your app to enable 2d transform propagation.

use std::collections::HashMap;

use bevy_ecs::prelude::*;
use engine_core::{App, GamePlugin, Parent};
use engine_math::{LocalTransform, Vec2, WorldTransform};

/// plugin that registers the 2d transform propagation system.
///
/// add this alongside [`engine_core::HierarchyPlugin`] — hierarchy manages
/// parent/child relationships, plugin2d propagates 2d transforms through them.
pub struct Plugin2d;

impl GamePlugin for Plugin2d {
    fn name(&self) -> &'static str {
        "2d"
    }

    fn build(&mut self, app: &mut App) {
        app.add_system(propagate_transforms);
    }
}

/// exclusive system that propagates 2d transforms from parents to children.
///
/// runs as an exclusive world system so `WorldTransform` is written immediately
/// (no command deferral) — entities have correct world transforms in the same frame
/// they are spawned.
///
/// uses a topological sort so each entity is processed exactly once, giving
/// O(N) propagation regardless of hierarchy depth.
///
/// entities without a parent get their `WorldTransform` directly from `LocalTransform`.
pub fn propagate_transforms(world: &mut World) {
    let snapshot: Vec<(Entity, LocalTransform, Option<Entity>)> = world
        .query::<(Entity, &LocalTransform, Option<&Parent>)>()
        .iter(world)
        .map(|(entity, local, parent)| (entity, *local, parent.map(|p| p.0)))
        .collect();

    let parent_of: HashMap<Entity, Entity> = snapshot
        .iter()
        .filter_map(|(entity, _, parent)| parent.map(|parent_entity| (*entity, parent_entity)))
        .collect();

    let mut depths: HashMap<Entity, u32> = HashMap::with_capacity(snapshot.len());
    for &(entity, _, _) in &snapshot {
        let mut depth = 0u32;
        let mut current = entity;
        while let Some(&parent_entity) = parent_of.get(&current) {
            depth += 1;
            current = parent_entity;
        }
        depths.insert(entity, depth);
    }

    let mut sorted = snapshot;
    sorted.sort_by_key(|(entity, _, _)| depths.get(entity).copied().unwrap_or(0));

    for (entity, local, parent_entity) in sorted {
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
    use engine_core::Parent;

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
