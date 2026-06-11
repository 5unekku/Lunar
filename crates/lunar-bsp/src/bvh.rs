//! dynamic AABB BVH (bounding volume hierarchy) for O(log n) frustum culling.
//!
//! built each frame from entities with [`Aabb3d`] and [`WorldTransform3d`].
//! the renderer can query [`BvhVisible`] instead of scanning `CullSoa` linearly.
//!
//! the tree is a top-down split BVH using SAH (surface area heuristic) axis selection:
//! at each node, the split axis is the longest extent of the node's AABB. entities are
//! sorted along that axis and split at the median, producing a balanced tree.
//!
//! nodes are stored in a flat `Vec` (cache-friendly traversal). leaf nodes store a
//! range of entity indices in a separate sorted entity list.

use bevy_ecs::prelude::*;
use lunar_3d::{Aabb3d, ComputedVisibility, Frustum, WorldTransform3d};
use lunar_core::{App, GamePlugin, UpdateStage};
use lunar_math::{Vec3, Vec3A};

/// resource: entities visible according to BVH frustum query this frame.
///
/// populated by `build_bvh_visible` each render frame. the renderer reads this
/// set instead of iterating all entities with AABBs.
///
/// entities without [`Aabb3d`] are not in the BVH and should always be drawn.
#[derive(Resource, Default)]
pub struct BvhVisible {
	pub entities: Vec<Entity>,
}

/// axis-aligned AABB node in the BVH.
#[derive(Clone, Copy)]
pub struct BvhNode {
	pub min: Vec3,
	pub max: Vec3,
	/// if >= 0: index of left child (right child = left + 1).
	/// if negative: leaf, entity range is [-(left+1) .. -(right+1)] in entity list.
	pub left_or_start: i32,
	pub right_or_end: i32,
}

/// resource: the BVH tree built from all visible AABB entities.
///
/// rebuilt every frame by `build_bvh` before frustum culling.
#[derive(Resource, Default)]
pub struct Bvh {
	pub nodes: Vec<BvhNode>,
	pub entities: Vec<(Entity, Vec3, Vec3)>, // entity + world min/max
	/// permutation of entity indices, partitioned during the build so every
	/// leaf owns a contiguous range of it
	order: Vec<u32>,
}

impl Bvh {
	pub fn clear(&mut self) {
		self.nodes.clear();
		self.entities.clear();
		self.order.clear();
	}

	/// build the BVH from entity AABBs. returns the root node index (always 0 after build).
	pub fn build(
		&mut self,
		query: &Query<(Entity, &Aabb3d, &WorldTransform3d, &ComputedVisibility)>,
	) {
		self.clear();
		for (entity, aabb, wt, vis) in query.iter() {
			if !vis.0 {
				continue;
			}
			let center = Vec3::from(wt.translation) + Vec3::from(aabb.center) * wt.scale;
			let he = Vec3::from(aabb.half_extents) * wt.scale;
			self.entities.push((entity, center - he, center + he));
		}
		if self.entities.is_empty() {
			return;
		}
		let n = self.entities.len();
		self.nodes.reserve(n * 2);
		self.order.extend(0..n as u32);
		// take the order array out so build_node can partition its sub-slices
		// while nodes is borrowed mutably
		let mut order = std::mem::take(&mut self.order);
		Self::build_node(&mut self.nodes, &self.entities, &mut order, 0);
		self.order = order;
	}

	/// build the node covering `order` (a sub-slice starting at `base` within
	/// the full order array), partitioning it in place
	fn build_node(
		nodes: &mut Vec<BvhNode>,
		entities: &[(Entity, Vec3, Vec3)],
		order: &mut [u32],
		base: usize,
	) -> i32 {
		// compute AABB covering all entities in this node
		let mut min = Vec3::splat(f32::INFINITY);
		let mut max = Vec3::splat(f32::NEG_INFINITY);
		for &i in order.iter() {
			min = min.min(entities[i as usize].1);
			max = max.max(entities[i as usize].2);
		}

		let node_idx = nodes.len() as i32;
		nodes.push(BvhNode {
			min,
			max,
			left_or_start: 0,
			right_or_end: 0,
		});

		if order.len() <= 4 {
			// leaf: contiguous range in the order array, negative-encoded
			nodes[node_idx as usize].left_or_start = -(base as i32 + 1);
			nodes[node_idx as usize].right_or_end = -((base + order.len() - 1) as i32 + 1);
			return node_idx;
		}

		// split along longest axis at the median; a full sort is unnecessary —
		// partitioning around the median element is enough for a balanced tree
		let extent = max - min;
		let axis = if extent.x >= extent.y && extent.x >= extent.z {
			0
		} else if extent.y >= extent.z {
			1
		} else {
			2
		};
		let centroid = |i: u32| (entities[i as usize].1[axis] + entities[i as usize].2[axis]) * 0.5;
		let mid = order.len() / 2;
		order.select_nth_unstable_by(mid, |&a, &b| {
			centroid(a)
				.partial_cmp(&centroid(b))
				.unwrap_or(std::cmp::Ordering::Equal)
		});
		let (left, right) = order.split_at_mut(mid);

		let left_node = Self::build_node(nodes, entities, left, base);
		let right_node = Self::build_node(nodes, entities, right, base + mid);

		nodes[node_idx as usize].left_or_start = left_node;
		nodes[node_idx as usize].right_or_end = right_node;

		node_idx
	}

	/// query all entities whose AABB overlaps the frustum.
	pub fn query_frustum(&self, frustum: &Frustum, out: &mut Vec<Entity>) {
		if self.nodes.is_empty() {
			for &(entity, _, _) in &self.entities {
				out.push(entity);
			}
			return;
		}
		self.visit_node(0, frustum, out);
	}

	fn visit_node(&self, node_idx: i32, frustum: &Frustum, out: &mut Vec<Entity>) {
		let node = &self.nodes[node_idx as usize];
		let center = Vec3A::from((node.min + node.max) * 0.5);
		let half_extent = Vec3A::from((node.max - node.min) * 0.5);

		if !frustum.intersects_aabb(center, half_extent) {
			return;
		}

		if node.left_or_start < 0 {
			// leaf: test each entity so the result is exact, not just node-level
			// conservative (leaves hold at most 4 entities)
			let start_idx = (-(node.left_or_start + 1)) as usize;
			let end_idx = (-(node.right_or_end + 1)) as usize;
			for &entity_idx in &self.order[start_idx..=end_idx] {
				let (entity, entity_min, entity_max) = self.entities[entity_idx as usize];
				let entity_center = Vec3A::from((entity_min + entity_max) * 0.5);
				let entity_half = Vec3A::from((entity_max - entity_min) * 0.5);
				if frustum.intersects_aabb(entity_center, entity_half) {
					out.push(entity);
				}
			}
		} else {
			self.visit_node(node.left_or_start, frustum, out);
			self.visit_node(node.right_or_end, frustum, out);
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use bevy_ecs::system::RunSystemOnce;
	use lunar_math::{Mat4, Quat};

	fn spawn_row(world: &mut World, count: usize) -> Vec<Entity> {
		// unit boxes spaced 3 apart along x, with some y/z scatter
		(0..count)
			.map(|i| {
				world
					.spawn((
						Aabb3d {
							center: Vec3A::ZERO,
							half_extents: Vec3A::splat(0.5),
						},
						WorldTransform3d {
							translation: Vec3::new(i as f32 * 3.0, (i % 5) as f32, (i % 7) as f32),
							rotation: Quat::IDENTITY,
							scale: Vec3::ONE,
						},
						ComputedVisibility(true),
					))
					.id()
			})
			.collect()
	}

	fn build_and_query(world: &mut World, frustum: Frustum) -> Vec<Entity> {
		world
			.run_system_once(
				move |query: Query<(Entity, &Aabb3d, &WorldTransform3d, &ComputedVisibility)>| {
					let mut bvh = Bvh::default();
					bvh.build(&query);
					let mut out = Vec::new();
					bvh.query_frustum(&frustum, &mut out);
					out
				},
			)
			.unwrap()
	}

	#[test]
	fn query_returns_each_contained_entity_exactly_once() {
		let mut world = World::new();
		let mut spawned = spawn_row(&mut world, 37);
		let all = Frustum::from_view_proj(Mat4::orthographic_rh(
			-1000.0, 1000.0, -1000.0, 1000.0, -1000.0, 1000.0,
		));
		let mut visible = build_and_query(&mut world, all);
		spawned.sort();
		visible.sort();
		// sorted equality also rejects duplicates and drops
		assert_eq!(visible, spawned);
	}

	#[test]
	fn query_culls_entities_outside_the_frustum() {
		let mut world = World::new();
		let spawned = spawn_row(&mut world, 37);
		// boxes sit at x = 0, 3, 6, 9, 12, …; this frustum ends at x = 10, so
		// exactly the first four (max x = 9.5) are inside
		let narrow = Frustum::from_view_proj(Mat4::orthographic_rh(
			-10.0, 10.0, -1000.0, 1000.0, -1000.0, 1000.0,
		));
		let mut visible = build_and_query(&mut world, narrow);
		let mut expected = spawned[..4].to_vec();
		visible.sort();
		expected.sort();
		assert_eq!(visible, expected);
	}
}

/// system that builds the BVH from current entity AABBs and frustum-culls them.
pub fn build_bvh_visible(
	query: Query<(Entity, &Aabb3d, &WorldTransform3d, &ComputedVisibility)>,
	frustum: Res<Frustum>,
	mut bvh: ResMut<Bvh>,
	mut visible: ResMut<BvhVisible>,
) {
	bvh.build(&query);
	visible.entities.clear();
	bvh.query_frustum(&frustum, &mut visible.entities);
}

/// plugin that inserts BVH resources and registers the build system.
pub struct BvhPlugin;

impl GamePlugin for BvhPlugin {
	fn name(&self) -> &str {
		"BvhPlugin"
	}
	fn build(&mut self, app: &mut App) {
		app.insert_resource(Bvh::default())
			.insert_resource(BvhVisible::default());
		// run after transform propagation so WorldTransform3d is current
		app.add_system_to_stage(UpdateStage::Render, build_bvh_visible);
	}
}
