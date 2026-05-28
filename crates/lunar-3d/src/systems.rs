use bevy_ecs::prelude::*;
use lunar_core::Parent;
use lunar_math::Mat4;

use crate::transform::{LocalTransform3d, WorldTransform3d};

/// scratch storage for transform propagation — allocated once, reused every frame.
///
/// uses parallel Vecs keyed by snapshot index rather than HashMaps keyed by Entity.
/// entity → index lookup is a binary search on the sorted `entity_idx` vec.
#[derive(Resource, Default)]
pub struct TransformScratch3d {
    snapshot: Vec<(Entity, LocalTransform3d, Option<Entity>)>,
    // sorted (entity, snapshot_index) pairs — built after snapshot collection
    entity_idx: Vec<(Entity, usize)>,
    // parallel to snapshot: snapshot index of this entity's parent, or None
    parent_idx: Vec<Option<usize>>,
    // parallel to snapshot: computed depth (u32::MAX = not yet computed)
    depths: Vec<u32>,
    // visit order: snapshot indices sorted by depth (parents before children)
    order: Vec<usize>,
    // parallel to snapshot: computed world matrix
    world_mats: Vec<Mat4>,
}

/// propagate [`LocalTransform3d`] through the entity hierarchy to produce [`WorldTransform3d`].
///
/// O(N log N) sort + O(N log N) binary-search parent lookups, then one matrix multiply per entity.
/// all scratch vecs are reused each frame — no per-frame heap allocation in steady state.
pub fn propagate_transforms_3d(world: &mut World) {
    let mut scratch = world
        .remove_resource::<TransformScratch3d>()
        .unwrap_or_default();

    scratch.snapshot.clear();
    world
        .query::<(Entity, &LocalTransform3d, Option<&Parent>)>()
        .iter(world)
        .for_each(|(entity, local, parent)| {
            scratch.snapshot.push((entity, *local, parent.map(|p| p.0)));
        });

    if scratch.snapshot.is_empty() {
        world.insert_resource(scratch);
        return;
    }

    let n = scratch.snapshot.len();

    // build sorted entity → index map
    scratch.entity_idx.clear();
    for (i, &(entity, _, _)) in scratch.snapshot.iter().enumerate() {
        scratch.entity_idx.push((entity, i));
    }
    scratch.entity_idx.sort_unstable_by_key(|&(entity, _)| entity);

    // build parent_idx parallel to snapshot
    scratch.parent_idx.clear();
    scratch.parent_idx.resize(n, None);
    for (i, &(_, _, parent_entity)) in scratch.snapshot.iter().enumerate() {
        if let Some(parent_entity) = parent_entity {
            if let Ok(j) = scratch.entity_idx.binary_search_by_key(&parent_entity, |&(e, _)| e) {
                scratch.parent_idx[i] = Some(scratch.entity_idx[j].1);
            }
        }
    }

    // compute depths via memoized recursion (u32::MAX = not yet computed)
    scratch.depths.clear();
    scratch.depths.resize(n, u32::MAX);
    for i in 0..n {
        depth_of(i, &scratch.parent_idx, &mut scratch.depths);
    }

    // build visit order: sort snapshot indices by depth
    scratch.order.clear();
    scratch.order.extend(0..n);
    scratch.order.sort_unstable_by_key(|&i| scratch.depths[i]);

    // compute world matrices in depth order (parents guaranteed before children)
    scratch.world_mats.clear();
    scratch.world_mats.resize(n, Mat4::IDENTITY);
    for &i in &scratch.order {
        let (entity, local, _) = scratch.snapshot[i];
        let local_mat = local.to_matrix();
        let world_mat = match scratch.parent_idx[i] {
            Some(parent_i) => scratch.world_mats[parent_i] * local_mat,
            None => local_mat,
        };
        scratch.world_mats[i] = world_mat;

        let (scale, rotation, translation) = world_mat.to_scale_rotation_translation();
        let computed = WorldTransform3d { translation, rotation, scale };

        if let Some(mut existing) = world.get_mut::<WorldTransform3d>(entity) {
            *existing = computed;
        } else if let Ok(mut entity_ref) = world.get_entity_mut(entity) {
            entity_ref.insert(computed);
        }
    }

    world.insert_resource(scratch);
}

fn depth_of(idx: usize, parent_idx: &[Option<usize>], depths: &mut [u32]) -> u32 {
    if depths[idx] != u32::MAX {
        return depths[idx];
    }
    let depth = parent_idx[idx]
        .map(|parent| depth_of(parent, parent_idx, depths) + 1)
        .unwrap_or(0);
    depths[idx] = depth;
    depth
}
