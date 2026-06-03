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
	snapshot: Vec<(
		Entity,
		Option<LocalTransform3d>,
		Option<Visibility>,
		Option<Entity>,
	)>,
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
	// parallel to snapshot: computed world transform (identity for entries without a local transform)
	world_ts: Vec<WorldTransform3d>,
	// parallel to snapshot: set when the batched query sweep already wrote the component,
	// so the structural-insert pass only fires for entities genuinely missing it
	wt_written: Vec<bool>,
	cv_written: Vec<bool>,
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
		.query_filtered::<(
			Entity,
			Option<&LocalTransform3d>,
			Option<&Visibility>,
			Option<&Parent>,
		), Or<(With<LocalTransform3d>, With<Visibility>)>>()
		.iter(world)
		.for_each(|(entity, local, vis, parent)| {
			scratch
				.snapshot
				.push((entity, local.copied(), vis.copied(), parent.map(|p| p.0)));
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
	scratch
		.entity_idx
		.sort_unstable_by_key(|&(entity, _)| entity);

	// resolve each entry's parent to a snapshot index; note if the scene has any real link
	scratch.parent_idx.clear();
	scratch.parent_idx.resize(n, None);
	let mut any_parented = false;
	for i in 0..n {
		if let Some(parent_entity) = scratch.snapshot[i].3
			&& let Ok(j) = scratch
				.entity_idx
				.binary_search_by_key(&parent_entity, |&(e, _)| e)
		{
			scratch.parent_idx[i] = Some(scratch.entity_idx[j].1);
			any_parented = true;
		}
	}

	scratch.world_ts.clear();
	scratch.world_ts.resize(n, WorldTransform3d::new());
	scratch.computed_vis.clear();
	scratch.computed_vis.resize(n, true);

	if !any_parented {
		// ── flat-scene fast path ──────────────────────────────────────────
		// no hierarchy: world transform == local transform (T/R/S copied straight
		// through, no matrix build or decompose), and Inherited visibility (no parent)
		// resolves to visible. each entry is independent ⇒ parallelizable.
		#[cfg(not(target_arch = "wasm32"))]
		{
			use rayon::prelude::*;
			let snapshot = &scratch.snapshot;
			scratch
				.world_ts
				.par_iter_mut()
				.zip(scratch.computed_vis.par_iter_mut())
				.enumerate()
				.for_each(|(i, (world_ts, computed_vis))| {
					if let Some(local) = snapshot[i].1 {
						*world_ts = WorldTransform3d {
							translation: local.translation,
							rotation: local.rotation,
							scale: local.scale,
						};
					}
					if let Some(vis) = snapshot[i].2 {
						*computed_vis = !matches!(vis, Visibility::Hidden);
					}
				});
		}
		#[cfg(target_arch = "wasm32")]
		for i in 0..n {
			if let Some(local) = scratch.snapshot[i].1 {
				scratch.world_ts[i] = WorldTransform3d {
					translation: local.translation,
					rotation: local.rotation,
					scale: local.scale,
				};
			}
			if let Some(vis) = scratch.snapshot[i].2 {
				scratch.computed_vis[i] = !matches!(vis, Visibility::Hidden);
			}
		}
	} else {
		// ── hierarchical path ─────────────────────────────────────────────
		// depth-sort so parents are resolved before children, then chain matrices.
		// parentless entries still skip the decompose (world == local).
		scratch.world_mats.clear();
		scratch.world_mats.resize(n, Mat4::IDENTITY);

		scratch.depths.clear();
		scratch.depths.resize(n, u32::MAX);
		for i in 0..n {
			depth_of(i, &scratch.parent_idx, &mut scratch.depths);
		}
		scratch.order.clear();
		scratch.order.extend(0..n);
		scratch.order.sort_unstable_by_key(|&i| scratch.depths[i]);

		for &i in &scratch.order {
			let (_, local, vis, _) = scratch.snapshot[i];

			if let Some(local) = local {
				match scratch.parent_idx[i] {
					None => {
						// parentless: world == local; keep the matrix for any children, skip decompose
						scratch.world_mats[i] = local.to_matrix();
						scratch.world_ts[i] = WorldTransform3d {
							translation: local.translation,
							rotation: local.rotation,
							scale: local.scale,
						};
					}
					Some(parent_i) => {
						let world_mat = scratch.world_mats[parent_i] * local.to_matrix();
						scratch.world_mats[i] = world_mat;
						let (scale, rotation, translation) =
							world_mat.to_scale_rotation_translation();
						scratch.world_ts[i] = WorldTransform3d {
							translation,
							rotation,
							scale,
						};
					}
				}
			} else if let Some(parent_i) = scratch.parent_idx[i] {
				// no local transform — inherit parent matrix for downstream child chains
				scratch.world_mats[i] = scratch.world_mats[parent_i];
			}

			if let Some(vis) = vis {
				let parent_visible = scratch.parent_idx[i]
					.map(|pi| scratch.computed_vis[pi])
					.unwrap_or(true);
				scratch.computed_vis[i] = match vis {
					Visibility::Visible => true,
					Visibility::Hidden => false,
					Visibility::Inherited => parent_visible,
				};
			}
		}
	}

	// ── write back results ────────────────────────────────────────────────
	// one linear column sweep per component (cache-friendly) + a binary search to map
	// entity → snapshot index, replacing N random `get_mut` archetype lookups. entities
	// that don't have the component yet fall to the structural-insert pass below.
	scratch.wt_written.clear();
	scratch.wt_written.resize(n, false);
	scratch.cv_written.clear();
	scratch.cv_written.resize(n, false);
	{
		let entity_idx = &scratch.entity_idx;
		let snapshot = &scratch.snapshot;
		let world_ts = &scratch.world_ts;
		let wt_written = &mut scratch.wt_written;
		let mut query = world.query::<(Entity, &mut WorldTransform3d)>();
		for (entity, mut wt) in query.iter_mut(world) {
			if let Ok(j) = entity_idx.binary_search_by_key(&entity, |&(e, _)| e) {
				let i = entity_idx[j].1;
				if snapshot[i].1.is_some() {
					*wt = world_ts[i];
					wt_written[i] = true;
				}
			}
		}
	}
	{
		let entity_idx = &scratch.entity_idx;
		let snapshot = &scratch.snapshot;
		let computed_vis = &scratch.computed_vis;
		let cv_written = &mut scratch.cv_written;
		let mut query = world.query::<(Entity, &mut ComputedVisibility)>();
		for (entity, mut cv) in query.iter_mut(world) {
			if let Ok(j) = entity_idx.binary_search_by_key(&entity, |&(e, _)| e) {
				let i = entity_idx[j].1;
				if snapshot[i].2.is_some() {
					*cv = ComputedVisibility(computed_vis[i]);
					cv_written[i] = true;
				}
			}
		}
	}
	// structural insert — cold path, only entities missing the component (e.g. first frame)
	for i in 0..n {
		if scratch.snapshot[i].1.is_some()
			&& !scratch.wt_written[i]
			&& let Ok(mut entity_ref) = world.get_entity_mut(scratch.snapshot[i].0)
		{
			entity_ref.insert(scratch.world_ts[i]);
		}
		if scratch.snapshot[i].2.is_some()
			&& !scratch.cv_written[i]
			&& let Ok(mut entity_ref) = world.get_entity_mut(scratch.snapshot[i].0)
		{
			entity_ref.insert(ComputedVisibility(scratch.computed_vis[i]));
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

#[cfg(test)]
mod tests {
	use super::*;
	use lunar_math::Vec3;

	fn close(a: Vec3, b: Vec3) -> bool {
		(a - b).length() < 1e-4
	}

	// flat scene (no parents): world transform == local transform, inserted on first run.
	#[test]
	fn flat_scene_world_equals_local() {
		let mut world = World::new();
		world.init_resource::<TransformScratch3d>();
		let e = world.spawn(LocalTransform3d::from_xyz(3.0, 4.0, 5.0)).id();
		propagate_transforms_3d(&mut world);
		let wt = world.get::<WorldTransform3d>(e).unwrap();
		assert!(close(wt.translation, Vec3::new(3.0, 4.0, 5.0)));
	}

	// child world transform composes with its parent's.
	#[test]
	fn child_composes_with_parent() {
		let mut world = World::new();
		world.init_resource::<TransformScratch3d>();
		let parent = world.spawn(LocalTransform3d::from_xyz(10.0, 0.0, 0.0)).id();
		let child = world
			.spawn((LocalTransform3d::from_xyz(1.0, 0.0, 0.0), Parent(parent)))
			.id();
		propagate_transforms_3d(&mut world);
		let cw = world.get::<WorldTransform3d>(child).unwrap();
		assert!(close(cw.translation, Vec3::new(11.0, 0.0, 0.0)));
	}

	// Inherited visibility resolves through a hidden ancestor.
	#[test]
	fn visibility_inherits_from_parent() {
		let mut world = World::new();
		world.init_resource::<TransformScratch3d>();
		let parent = world
			.spawn((LocalTransform3d::default(), Visibility::Hidden))
			.id();
		let child = world
			.spawn((
				LocalTransform3d::default(),
				Visibility::Inherited,
				Parent(parent),
			))
			.id();
		propagate_transforms_3d(&mut world);
		assert!(!world.get::<ComputedVisibility>(child).unwrap().0);
	}

	// re-running updates the existing component in place (the batched query-sweep path).
	#[test]
	fn updates_existing_world_transform_in_place() {
		let mut world = World::new();
		world.init_resource::<TransformScratch3d>();
		let e = world.spawn(LocalTransform3d::from_xyz(1.0, 2.0, 3.0)).id();
		propagate_transforms_3d(&mut world);
		world.get_mut::<LocalTransform3d>(e).unwrap().translation = Vec3::new(7.0, 8.0, 9.0);
		propagate_transforms_3d(&mut world);
		let wt = world.get::<WorldTransform3d>(e).unwrap();
		assert!(close(wt.translation, Vec3::new(7.0, 8.0, 9.0)));
	}

	// a 3-level chain composes through both ancestors (exercises the depth sort).
	#[test]
	fn grandchild_composes_through_chain() {
		let mut world = World::new();
		world.init_resource::<TransformScratch3d>();
		let a = world.spawn(LocalTransform3d::from_xyz(1.0, 0.0, 0.0)).id();
		let b = world
			.spawn((LocalTransform3d::from_xyz(2.0, 0.0, 0.0), Parent(a)))
			.id();
		let c = world
			.spawn((LocalTransform3d::from_xyz(4.0, 0.0, 0.0), Parent(b)))
			.id();
		propagate_transforms_3d(&mut world);
		assert!(close(
			world.get::<WorldTransform3d>(c).unwrap().translation,
			Vec3::new(7.0, 0.0, 0.0)
		));
	}
}
