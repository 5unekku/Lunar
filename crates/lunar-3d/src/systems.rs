use bevy_ecs::prelude::*;
use lunar_core::Parent;
use lunar_math::Mat4;

use crate::mesh::PrevWorldTransform3d;
use crate::transform::{LocalTransform3d, WorldTransform3d};
use crate::visibility::{ComputedVisibility, Visibility};

/// scratch storage for the combined transform + visibility propagation pass.
///
/// allocated once as a resource, cleared and refilled every frame.
/// uses parallel Vecs keyed by snapshot index; entity→index lookup is a binary search.
#[derive(Resource, Default)]
#[allow(clippy::type_complexity)]
pub struct TransformScratch3d {
    // (entity, local_transform_if_any, visibility_if_any, parent_entity)
    snapshot: Vec<(Entity, Option<LocalTransform3d>, Option<Visibility>, Option<Entity>)>,
    // sorted (entity, snapshot_index) pairs for O(log n) lookup
    entity_idx: Vec<(Entity, usize)>,
    // parallel to snapshot: snapshot index of this entity's parent, or None
    parent_idx: Vec<Option<usize>>,
    // parallel to snapshot: computed depth (u32::MAX = not yet computed)
    depths: Vec<u32>,
    // visit order: snapshot indices sorted by depth (parents before children)
    order: Vec<usize>,
    // parallel to snapshot: computed world matrix (Mat4::IDENTITY for entities without LocalTransform3d)
    world_mats: Vec<Mat4>,
    // parallel to snapshot: computed visibility (true for entities without Visibility)
    computed_vis: Vec<bool>,
}

/// propagate [`LocalTransform3d`] and [`Visibility`] through the entity hierarchy in one pass.
///
/// replaces the separate `propagate_transforms_3d` + `propagate_visibility` systems.
/// both share the same hierarchy sort (O(N log N)) — doing them together halves that cost.
///
/// produces [`WorldTransform3d`] and [`ComputedVisibility`] for all relevant entities.
pub fn propagate_transforms_3d(world: &mut World) {
    let mut scratch = world
        .remove_resource::<TransformScratch3d>()
        .unwrap_or_default();

    scratch.snapshot.clear();
    // collect all entities that have a transform or a visibility component (or both)
    world
        .query_filtered::<
            (Entity, Option<&LocalTransform3d>, Option<&Visibility>, Option<&Parent>),
            Or<(With<LocalTransform3d>, With<Visibility>)>,
        >()
        .iter(world)
        .for_each(|(entity, local, vis, parent)| {
            scratch.snapshot.push((entity, local.copied(), vis.copied(), parent.map(|p| p.0)));
        });

    if scratch.snapshot.is_empty() {
        world.insert_resource(scratch);
        return;
    }

    let n = scratch.snapshot.len();

    scratch.entity_idx.clear();
    for (i, &(entity, _, _, _)) in scratch.snapshot.iter().enumerate() {
        scratch.entity_idx.push((entity, i));
    }
    scratch.entity_idx.sort_unstable_by_key(|&(entity, _)| entity);

    scratch.parent_idx.clear();
    scratch.parent_idx.resize(n, None);
    for (i, &(_, _, _, parent_entity)) in scratch.snapshot.iter().enumerate() {
        if let Some(parent_entity) = parent_entity
            && let Ok(j) = scratch.entity_idx.binary_search_by_key(&parent_entity, |&(e, _)| e) {
                scratch.parent_idx[i] = Some(scratch.entity_idx[j].1);
            }
    }

    scratch.depths.clear();
    scratch.depths.resize(n, u32::MAX);
    for i in 0..n {
        depth_of(i, &scratch.parent_idx, &mut scratch.depths);
    }

    scratch.order.clear();
    scratch.order.extend(0..n);
    scratch.order.sort_unstable_by_key(|&i| scratch.depths[i]);

    scratch.world_mats.clear();
    scratch.world_mats.resize(n, Mat4::IDENTITY);
    scratch.computed_vis.clear();
    scratch.computed_vis.resize(n, true);

    for &i in &scratch.order {
        let (entity, local, vis, _) = scratch.snapshot[i];

        // transform propagation
        if let Some(local) = local {
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
        } else if let Some(parent_i) = scratch.parent_idx[i] {
            // entity has no local transform — inherit parent matrix for child chains
            scratch.world_mats[i] = scratch.world_mats[parent_i];
        }

        // visibility propagation
        if let Some(vis) = vis {
            let parent_visible = scratch.parent_idx[i]
                .map(|pi| scratch.computed_vis[pi])
                .unwrap_or(true);
            let visible = match vis {
                Visibility::Visible => true,
                Visibility::Hidden => false,
                Visibility::Inherited => parent_visible,
            };
            scratch.computed_vis[i] = visible;

            let cv = ComputedVisibility(visible);
            if let Some(mut existing) = world.get_mut::<ComputedVisibility>(entity) {
                *existing = cv;
            } else if let Ok(mut entity_ref) = world.get_entity_mut(entity) {
                entity_ref.insert(cv);
            }
        }
    }

    world.insert_resource(scratch);
}

/// copy current `WorldTransform3d` into `PrevWorldTransform3d` at end of each tick.
///
/// run this at `PostUpdate` after all transform propagation so every tick snapshot
/// is committed before the next tick begins. the renderer uses the prev/cur pair
/// to lerp by `Time::interp_alpha()` for smooth sub-tick motion.
pub fn copy_prev_transforms(mut query: Query<(&WorldTransform3d, &mut PrevWorldTransform3d)>) {
    for (current, mut previous) in &mut query {
        previous.0 = *current;
    }
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
