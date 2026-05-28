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
pub use collision::{Collider, Collider2dBundle, ColliderShape, CollisionWorld, RayHit2d, ray_cast_2d};

use bevy_ecs::prelude::*;
use lunar_core::{App, GamePlugin, Parent};
use lunar_math::{LocalTransform, Vec2, WorldTransform};

use collision::build_collision_world;

/// scratch buffers for `propagate_transforms` — allocated once, reused every frame.
#[derive(Default, Resource)]
struct TransformScratch2d {
    snapshot: Vec<(Entity, LocalTransform, Option<Entity>)>,
    // sorted (entity, snapshot_index) pairs for binary-search parent lookup
    entity_idx: Vec<(Entity, usize)>,
    // parallel to snapshot: snapshot index of parent, or None
    parent_idx: Vec<Option<usize>>,
    // parallel to snapshot: computed depth (u32::MAX = not yet computed)
    depths: Vec<u32>,
    // snapshot indices in depth order (parents before children)
    order: Vec<usize>,
    // parallel to snapshot: computed world transform
    world_transforms: Vec<WorldTransform>,
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
/// O(N log N) sort + binary-search parent lookups, then one transform compose per entity.
/// all scratch vecs are reused each frame — no per-frame heap allocation in steady state.
pub fn propagate_transforms(world: &mut World) {
    let mut scratch = world
        .remove_resource::<TransformScratch2d>()
        .unwrap_or_default();

    scratch.snapshot.clear();
    world
        .query::<(Entity, &LocalTransform, Option<&Parent>)>()
        .iter(world)
        .for_each(|(entity, local, parent)| {
            scratch.snapshot.push((entity, *local, parent.map(|p| p.0)));
        });

    if scratch.snapshot.is_empty() {
        world.insert_resource(scratch);
        return;
    }

    let n = scratch.snapshot.len();

    scratch.entity_idx.clear();
    for (i, &(entity, _, _)) in scratch.snapshot.iter().enumerate() {
        scratch.entity_idx.push((entity, i));
    }
    scratch.entity_idx.sort_unstable_by_key(|&(entity, _)| entity);

    scratch.parent_idx.clear();
    scratch.parent_idx.resize(n, None);
    for (i, &(_, _, parent_entity)) in scratch.snapshot.iter().enumerate() {
        if let Some(parent_entity) = parent_entity {
            if let Ok(j) = scratch.entity_idx.binary_search_by_key(&parent_entity, |&(e, _)| e) {
                scratch.parent_idx[i] = Some(scratch.entity_idx[j].1);
            }
        }
    }

    scratch.depths.clear();
    scratch.depths.resize(n, u32::MAX);
    for i in 0..n {
        depth_of_2d(i, &scratch.parent_idx, &mut scratch.depths);
    }

    scratch.order.clear();
    scratch.order.extend(0..n);
    scratch.order.sort_unstable_by_key(|&i| scratch.depths[i]);

    scratch.world_transforms.clear();
    scratch.world_transforms.resize(n, WorldTransform::default());
    for &i in &scratch.order {
        let (entity, local, _) = scratch.snapshot[i];
        let world_transform = match scratch.parent_idx[i] {
            Some(parent_i) => compute_world_transform(&scratch.world_transforms[parent_i], &local),
            None => WorldTransform {
                translation: local.translation,
                rotation: local.rotation,
                scale: local.scale,
            },
        };
        scratch.world_transforms[i] = world_transform;

        if let Some(mut existing) = world.get_mut::<WorldTransform>(entity) {
            *existing = world_transform;
        } else if let Ok(mut entity_ref) = world.get_entity_mut(entity) {
            entity_ref.insert(world_transform);
        }
    }

    world.insert_resource(scratch);
}

fn depth_of_2d(idx: usize, parent_idx: &[Option<usize>], depths: &mut [u32]) -> u32 {
    if depths[idx] != u32::MAX {
        return depths[idx];
    }
    let depth = parent_idx[idx]
        .map(|parent| depth_of_2d(parent, parent_idx, depths) + 1)
        .unwrap_or(0);
    depths[idx] = depth;
    depth
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
