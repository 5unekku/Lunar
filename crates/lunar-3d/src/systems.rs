use std::collections::HashMap;

use bevy_ecs::prelude::*;
use lunar_core::Parent;
use lunar_math::Mat4;

use crate::transform::{LocalTransform3d, WorldTransform3d};

/// scratch storage for transform propagation — allocated once, reused every frame.
#[derive(Resource, Default)]
pub struct TransformScratch3d {
    snapshot: Vec<(Entity, LocalTransform3d, Option<Entity>)>,
    parent_of: HashMap<Entity, Entity>,
    depths: HashMap<Entity, u32>,
    world_mats: HashMap<Entity, Mat4>,
}

/// propagate [`LocalTransform3d`] through the entity hierarchy to produce [`WorldTransform3d`].
///
/// O(N) memoized depth sort, then one matrix multiply per entity.
/// entities without a parent treat their local transform as world space.
///
/// uses a persistent scratch resource to avoid per-frame heap allocations.
pub fn propagate_transforms_3d(world: &mut World) {
    let mut scratch = world
        .remove_resource::<TransformScratch3d>()
        .unwrap_or_default();

    scratch.snapshot.clear();
    scratch.parent_of.clear();
    scratch.depths.clear();
    scratch.world_mats.clear();

    world
        .query::<(Entity, &LocalTransform3d, Option<&Parent>)>()
        .iter(world)
        .for_each(|(entity, local, parent)| {
            scratch
                .snapshot
                .push((entity, *local, parent.map(|p| p.0)));
        });

    if scratch.snapshot.is_empty() {
        world.insert_resource(scratch);
        return;
    }

    for &(entity, _, parent) in &scratch.snapshot {
        if let Some(parent_entity) = parent {
            scratch.parent_of.insert(entity, parent_entity);
        }
    }

    for i in 0..scratch.snapshot.len() {
        let entity = scratch.snapshot[i].0;
        depth_of(entity, &scratch.parent_of, &mut scratch.depths);
    }

    scratch
        .snapshot
        .sort_by_key(|(entity, _, _)| scratch.depths.get(entity).copied().unwrap_or(0));

    for (entity, local, parent_entity) in scratch.snapshot.iter().copied() {
        let local_mat = local.to_matrix();
        let world_mat = match parent_entity {
            Some(parent) => {
                scratch.world_mats.get(&parent).copied().unwrap_or(Mat4::IDENTITY) * local_mat
            }
            None => local_mat,
        };
        scratch.world_mats.insert(entity, world_mat);

        let (scale, rotation, translation) = world_mat.to_scale_rotation_translation();
        let computed = WorldTransform3d {
            translation,
            rotation,
            scale,
        };

        if let Some(mut existing) = world.get_mut::<WorldTransform3d>(entity) {
            *existing = computed;
        } else if let Ok(mut entity_ref) = world.get_entity_mut(entity) {
            entity_ref.insert(computed);
        }
    }

    world.insert_resource(scratch);
}

fn depth_of(
    entity: Entity,
    parent_of: &HashMap<Entity, Entity>,
    cache: &mut HashMap<Entity, u32>,
) -> u32 {
    if let Some(&depth) = cache.get(&entity) {
        return depth;
    }
    let depth = parent_of
        .get(&entity)
        .map(|&parent| depth_of(parent, parent_of, cache) + 1)
        .unwrap_or(0);
    cache.insert(entity, depth);
    depth
}
