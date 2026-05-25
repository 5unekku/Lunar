use std::collections::HashMap;

use bevy_ecs::prelude::*;
use engine_core::Parent;
use engine_math::{Mat4, Quat, Vec3};

use crate::transform::{LocalTransform3d, WorldTransform3d};

/// propagate [`LocalTransform3d`] through the entity hierarchy to produce [`WorldTransform3d`].
///
/// mirrors the 2D version: O(N) memoized depth sort, then one matrix multiply per entity.
/// entities without a parent treat their local transform as world space.
pub fn propagate_transforms_3d(world: &mut World) {
    // snapshot all entities with a local transform
    let snapshot: Vec<(Entity, LocalTransform3d, Option<Entity>)> = world
        .query::<(Entity, &LocalTransform3d, Option<&Parent>)>()
        .iter(world)
        .map(|(entity, local, parent)| (entity, *local, parent.map(|p| p.0)))
        .collect();

    if snapshot.is_empty() {
        return;
    }

    // build parent lookup map
    let parent_of: HashMap<Entity, Entity> = snapshot
        .iter()
        .filter_map(|(entity, _, parent)| parent.map(|p| (*entity, p)))
        .collect();

    // memoized depth for stable topological sort
    let mut depths: HashMap<Entity, u32> = HashMap::new();
    for &(entity, _, _) in &snapshot {
        depth_of(entity, &parent_of, &mut depths);
    }

    let mut sorted = snapshot;
    sorted.sort_by_key(|(entity, _, _)| depths.get(entity).copied().unwrap_or(0));

    // compute world transforms top-down; cache matrices for parent lookups
    let mut world_mats: HashMap<Entity, Mat4> = HashMap::with_capacity(sorted.len());

    for (entity, local, parent_entity) in sorted {
        let local_mat = local.to_matrix();
        let world_mat = if let Some(parent) = parent_entity {
            world_mats.get(&parent).copied().unwrap_or(Mat4::IDENTITY) * local_mat
        } else {
            local_mat
        };
        world_mats.insert(entity, world_mat);

        // decompose back to TRS for WorldTransform3d
        let (scale, rotation, translation) = world_mat.to_scale_rotation_translation();
        let computed = WorldTransform3d {
            translation,
            rotation,
            scale,
        };

        if let Some(mut existing) = world.get_mut::<WorldTransform3d>(entity) {
            *existing = computed;
        } else if let Some(mut entity_ref) = world.get_entity_mut(entity).ok() {
            entity_ref.insert(computed);
        }
    }
}

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

// silence unused import warning — Vec3/Quat are used via engine_math re-exports
const _: Vec3 = Vec3::ZERO;
const _: Quat = Quat::IDENTITY;
