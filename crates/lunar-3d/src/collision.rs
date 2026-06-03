//! 3d collision detection — AABB and sphere shapes, overlap queries.
//!
//! no physics simulation (no rigid bodies, velocity integration, gravity).
//! this module answers the question "what overlaps what" — game logic decides
//! what to do about it.
//!
//! # usage
//!
//! ```ignore
//! use lunar_3d::collision::{Collider3d, ColliderShape3d, CollisionWorld3d};
//!
//! commands.spawn((
//!     LocalTransform3d::from_xyz(0.0, 1.0, 0.0),
//!     WorldTransform3d::default(),
//!     Collider3d::aabb(Vec3::new(1.0, 2.0, 1.0)),
//! ));
//!
//! fn check_hits(world: Res<CollisionWorld3d>) {
//!     for (entity_a, entity_b) in world.all_overlaps() {
//!         // handle collision
//!     }
//! }
//! ```

use bevy_ecs::prelude::*;
use lunar_math::{Quat, Vec3, Vec3A};

use crate::mesh::Mesh3d;
use crate::mesh_registry::MeshRegistry;
use crate::transform::WorldTransform3d;
use crate::visibility::CullSoa;

/// shape variant for a [`Collider3d`] component.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColliderShape3d {
	/// axis-aligned bounding box. `half_extents` is half the width/height/depth.
	Aabb { half_extents: Vec3 },
	/// sphere centered on the entity's world position.
	Sphere { radius: f32 },
}

/// component that makes an entity participate in 3d collision detection.
///
/// pair with `WorldTransform3d` — the collision world uses world-space position.
#[derive(Debug, Clone, Component)]
pub struct Collider3d {
	pub shape: ColliderShape3d,
	/// bitmask — which collision layers this collider belongs to.
	pub layer: u32,
	/// bitmask — which layers this collider checks against.
	pub mask: u32,
}

impl Collider3d {
	/// axis-aligned bounding box with the given full size (half_extents = size / 2).
	#[must_use]
	pub fn aabb(size: Vec3) -> Self {
		Self {
			shape: ColliderShape3d::Aabb {
				half_extents: size * 0.5,
			},
			layer: 1,
			mask: 1,
		}
	}

	/// sphere with the given radius.
	#[must_use]
	pub fn sphere(radius: f32) -> Self {
		Self {
			shape: ColliderShape3d::Sphere { radius },
			layer: 1,
			mask: 1,
		}
	}

	/// set the collision layer (fluent).
	#[must_use]
	pub fn with_layer(mut self, layer: u32) -> Self {
		self.layer = layer;
		self
	}

	/// set the collision mask (fluent).
	#[must_use]
	pub fn with_mask(mut self, mask: u32) -> Self {
		self.mask = mask;
		self
	}
}

/// a single entry in the collision world snapshot.
///
/// `min_x` / `max_x` are precomputed for the sweep-and-prune broad phase.
#[derive(Debug, Clone)]
struct ColliderEntry {
	entity: Entity,
	position: Vec3,
	shape: ColliderShape3d,
	layer: u32,
	mask: u32,
	min_x: f32,
	max_x: f32,
}

impl ColliderEntry {
	fn new(entity: Entity, position: Vec3, shape: ColliderShape3d, layer: u32, mask: u32) -> Self {
		let (min_x, max_x) = match shape {
			ColliderShape3d::Aabb { half_extents } => {
				(position.x - half_extents.x, position.x + half_extents.x)
			}
			ColliderShape3d::Sphere { radius } => (position.x - radius, position.x + radius),
		};
		Self {
			entity,
			position,
			shape,
			layer,
			mask,
			min_x,
			max_x,
		}
	}

	fn overlaps(&self, other: &Self) -> bool {
		if self.mask & other.layer == 0 || other.mask & self.layer == 0 {
			return false;
		}
		shapes_overlap(self.position, self.shape, other.position, other.shape)
	}
}

fn shapes_overlap(
	pos_a: Vec3,
	shape_a: ColliderShape3d,
	pos_b: Vec3,
	shape_b: ColliderShape3d,
) -> bool {
	match (shape_a, shape_b) {
		(
			ColliderShape3d::Aabb {
				half_extents: half_a,
			},
			ColliderShape3d::Aabb {
				half_extents: half_b,
			},
		) => {
			(pos_a.x - pos_b.x).abs() < half_a.x + half_b.x
				&& (pos_a.y - pos_b.y).abs() < half_a.y + half_b.y
				&& (pos_a.z - pos_b.z).abs() < half_a.z + half_b.z
		}
		(ColliderShape3d::Sphere { radius: ra }, ColliderShape3d::Sphere { radius: rb }) => {
			(pos_a - pos_b).length_squared() < (ra + rb) * (ra + rb)
		}
		(ColliderShape3d::Aabb { half_extents }, ColliderShape3d::Sphere { radius })
		| (ColliderShape3d::Sphere { radius }, ColliderShape3d::Aabb { half_extents }) => {
			let (aabb_pos, sphere_pos) = if matches!(shape_a, ColliderShape3d::Aabb { .. }) {
				(pos_a, pos_b)
			} else {
				(pos_b, pos_a)
			};
			let closest = Vec3::new(
				sphere_pos
					.x
					.clamp(aabb_pos.x - half_extents.x, aabb_pos.x + half_extents.x),
				sphere_pos
					.y
					.clamp(aabb_pos.y - half_extents.y, aabb_pos.y + half_extents.y),
				sphere_pos
					.z
					.clamp(aabb_pos.z - half_extents.z, aabb_pos.z + half_extents.z),
			);
			(sphere_pos - closest).length_squared() < radius * radius
		}
	}
}

/// read-only view of a single entry in the collision world.
///
/// used by external physics systems (e.g. `lunar-physics-3d`) to iterate all colliders.
pub struct ColliderEntryRef<'a> {
	pub entity: Entity,
	pub position: Vec3,
	pub shape: ColliderShape3d,
	pub layer: u32,
	pub mask: u32,
	_phantom: std::marker::PhantomData<&'a ()>,
}

/// resource rebuilt every physics tick — holds the current frame's collider snapshot.
///
/// query this from any system in the Update stage or later.
///
/// # example
///
/// ```ignore
/// fn check_hits(world: Res<CollisionWorld3d>, query: Query<(Entity, &Collider3d)>) {
///     for (entity, _) in &query {
///         for other in world.overlapping(entity) {
///             // handle collision with `other`
///         }
///     }
/// }
/// ```
#[derive(Debug, Default, Resource)]
pub struct CollisionWorld3d {
	entries: Vec<ColliderEntry>,
}

impl CollisionWorld3d {
	/// sweep-and-prune range for X span `[qmin_x, qmax_x]`.
	fn x_candidates(&self, _qmin_x: f32, qmax_x: f32) -> &[ColliderEntry] {
		let end = self.entries.partition_point(|e| e.min_x <= qmax_x);
		&self.entries[..end]
	}

	/// iterator over all entities that overlap `entity` this frame, filtered by layer/mask.
	///
	/// uses sweep-and-prune on X to skip entries that can't possibly overlap.
	pub fn overlapping(&self, entity: Entity) -> impl Iterator<Item = Entity> + '_ {
		let target = self.entries.iter().find(|e| e.entity == entity).cloned();
		let candidates = target
			.as_ref()
			.map_or(&[] as &[_], |t| self.x_candidates(t.min_x, t.max_x));
		candidates.iter().filter_map(move |other| {
			if other.entity == entity {
				return None;
			}
			target
				.as_ref()
				.is_some_and(|t| other.max_x > t.min_x && t.overlaps(other))
				.then_some(other.entity)
		})
	}

	/// iterator over all entities whose collider contains `point`.
	pub fn query_point(&self, point: Vec3) -> impl Iterator<Item = Entity> + '_ {
		self.entries.iter().filter_map(move |entry| {
			point_in_shape(point, entry.position, entry.shape).then_some(entry.entity)
		})
	}

	/// iterator over all entities whose collider overlaps a sphere at `center` with `radius`.
	///
	/// uses sweep-and-prune on X.
	pub fn query_sphere(&self, center: Vec3, radius: f32) -> impl Iterator<Item = Entity> + '_ {
		let qmin_x = center.x - radius;
		let qmax_x = center.x + radius;
		let candidates = self.x_candidates(qmin_x, qmax_x);
		let query_shape = ColliderShape3d::Sphere { radius };
		candidates.iter().filter_map(move |entry| {
			if entry.max_x <= qmin_x {
				return None;
			}
			shapes_overlap(center, query_shape, entry.position, entry.shape).then_some(entry.entity)
		})
	}

	/// iterator over all entities whose collider overlaps a box at `center` with `half_extents`.
	///
	/// uses sweep-and-prune on X.
	pub fn query_aabb(
		&self,
		center: Vec3,
		half_extents: Vec3,
	) -> impl Iterator<Item = Entity> + '_ {
		let qmin_x = center.x - half_extents.x;
		let qmax_x = center.x + half_extents.x;
		let candidates = self.x_candidates(qmin_x, qmax_x);
		let query_shape = ColliderShape3d::Aabb { half_extents };
		candidates.iter().filter_map(move |entry| {
			if entry.max_x <= qmin_x {
				return None;
			}
			shapes_overlap(center, query_shape, entry.position, entry.shape).then_some(entry.entity)
		})
	}

	/// iterator over all entries in the collision world this frame.
	///
	/// used by physics systems that need to test a swept position against all colliders.
	pub fn all_entries(&self) -> impl Iterator<Item = ColliderEntryRef<'_>> {
		self.entries.iter().map(|entry| ColliderEntryRef {
			entity: entry.entity,
			position: entry.position,
			shape: entry.shape,
			layer: entry.layer,
			mask: entry.mask,
			_phantom: std::marker::PhantomData,
		})
	}

	/// entries in the X range `[qmin_x, qmax_x]` — sweep-and-prune pre-filter for physics queries.
	pub fn query_aabb_entries(
		&self,
		qmin_x: f32,
		qmax_x: f32,
	) -> impl Iterator<Item = ColliderEntryRef<'_>> {
		let candidates = self.x_candidates(qmin_x, qmax_x);
		candidates
			.iter()
			.filter(move |e| e.max_x > qmin_x)
			.map(|entry| ColliderEntryRef {
				entity: entry.entity,
				position: entry.position,
				shape: entry.shape,
				layer: entry.layer,
				mask: entry.mask,
				_phantom: std::marker::PhantomData,
			})
	}

	/// iterator over all overlapping pairs this frame. each pair appears exactly once.
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

fn point_in_shape(point: Vec3, position: Vec3, shape: ColliderShape3d) -> bool {
	match shape {
		ColliderShape3d::Aabb { half_extents } => {
			(point.x - position.x).abs() <= half_extents.x
				&& (point.y - position.y).abs() <= half_extents.y
				&& (point.z - position.z).abs() <= half_extents.z
		}
		ColliderShape3d::Sphere { radius } => {
			(point - position).length_squared() <= radius * radius
		}
	}
}

/// a ray in 3D world space.
#[derive(Debug, Clone, Copy)]
pub struct Ray3d {
	/// world-space origin.
	pub origin: Vec3,
	/// unit-length direction vector.
	pub direction: Vec3,
}

impl Ray3d {
	/// construct from origin and direction; normalizes the direction.
	#[must_use]
	pub fn new(origin: Vec3, direction: Vec3) -> Self {
		Self {
			origin,
			direction: direction.normalize_or_zero(),
		}
	}

	/// world-space point at distance `t` along the ray.
	#[must_use]
	pub fn at(self, t: f32) -> Vec3 {
		self.origin + self.direction * t
	}
}

/// result of a successful [`raycast_3d`] query.
#[derive(Debug, Clone, Copy)]
pub struct RayHit3d {
	/// entity whose geometry was hit.
	pub entity: Entity,
	/// world-space hit point.
	pub point: Vec3,
	/// world-space surface normal at the hit point (unit length).
	pub normal: Vec3,
	/// world-space distance along the ray from origin to hit point.
	pub distance: f32,
}

/// cast a ray and return the nearest mesh hit within `max_dist`, or `None`.
///
/// `mask` filters by [`Collider3d`] layer bitmask — pass `u32::MAX` to hit everything.
///
/// # usage
///
/// ```ignore
/// fn fire_hitscan(
///     soa: Res<CullSoa>,
///     registry: Res<MeshRegistry>,
///     query: Query<(&Mesh3d, &WorldTransform3d, Option<&Collider3d>)>,
/// ) {
///     let ray = Ray3d::new(origin, direction);
///     if let Some(hit) = raycast_3d(ray, 100.0, u32::MAX, &soa, &query, &registry) {
///         println!("hit entity {:?} at {:?}", hit.entity, hit.point);
///     }
/// }
/// ```
pub fn raycast_3d(
	ray: Ray3d,
	max_dist: f32,
	mask: u32,
	soa: &CullSoa,
	entity_query: &Query<(&Mesh3d, &WorldTransform3d, Option<&Collider3d>)>,
	registry: &MeshRegistry,
) -> Option<RayHit3d> {
	let mut nearest: Option<RayHit3d> = None;

	for (idx, &entity) in soa.entities.iter().enumerate() {
		let center = soa.centers[idx];
		let half_extents = soa.half_extents[idx];

		let Some(aabb_t) = ray_vs_aabb_3d(ray, center, half_extents, max_dist) else {
			continue;
		};

		let Ok((mesh_handle, world, collider)) = entity_query.get(entity) else {
			continue;
		};

		let entity_layer = collider.map(|c| c.layer).unwrap_or(1);
		if mask & entity_layer == 0 {
			continue;
		}

		let current_max = nearest.as_ref().map(|h| h.distance).unwrap_or(max_dist);

		let hit = registry
			.get_mesh(mesh_handle.0)
			.and_then(|mesh_data| {
				raycast_mesh(
					ray,
					mesh_data.vertices.iter().map(|v| v.position),
					&mesh_data.indices,
					world,
					current_max,
					entity,
				)
			})
			.unwrap_or_else(|| {
				// no mesh data — use AABB hit point
				let point = ray.at(aabb_t);
				let normal = aabb_normal(point, Vec3::from(center), Vec3::from(half_extents));
				RayHit3d {
					entity,
					point,
					normal,
					distance: aabb_t,
				}
			});

		if hit.distance < nearest.as_ref().map(|h| h.distance).unwrap_or(f32::MAX) {
			nearest = Some(hit);
		}
	}

	nearest
}

/// ray vs world-space axis-aligned bounding box (slab test).
/// returns the entry distance, or `None` if no intersection within `max_dist`.
fn ray_vs_aabb_3d(ray: Ray3d, center: Vec3A, half_extents: Vec3A, max_dist: f32) -> Option<f32> {
	let c = Vec3::from(center);
	let h = Vec3::from(half_extents);
	let inv_dir = Vec3::new(
		if ray.direction.x == 0.0 {
			f32::INFINITY
		} else {
			1.0 / ray.direction.x
		},
		if ray.direction.y == 0.0 {
			f32::INFINITY
		} else {
			1.0 / ray.direction.y
		},
		if ray.direction.z == 0.0 {
			f32::INFINITY
		} else {
			1.0 / ray.direction.z
		},
	);
	let t_min_v = (c - h - ray.origin) * inv_dir;
	let t_max_v = (c + h - ray.origin) * inv_dir;
	let t_near = t_min_v.min(t_max_v);
	let t_far = t_min_v.max(t_max_v);
	let t_entry = t_near.max_element();
	let t_exit = t_far.min_element();
	if t_entry <= t_exit && t_exit >= 0.0 && t_entry <= max_dist {
		Some(t_entry.max(0.0))
	} else {
		None
	}
}

/// approximate outward-facing AABB normal for a surface point.
fn aabb_normal(point: Vec3, center: Vec3, half_extents: Vec3) -> Vec3 {
	let local = point - center;
	let d = local.abs() - half_extents;
	// largest component of d is the hit face
	if d.x >= d.y && d.x >= d.z {
		Vec3::new(local.x.signum(), 0.0, 0.0)
	} else if d.y >= d.x && d.y >= d.z {
		Vec3::new(0.0, local.y.signum(), 0.0)
	} else {
		Vec3::new(0.0, 0.0, local.z.signum())
	}
}

/// Möller–Trumbore intersection.
/// returns `(t, model-space normal)` or `None` if no hit.
fn moller_trumbore(
	origin: Vec3,
	direction: Vec3,
	v0: Vec3,
	v1: Vec3,
	v2: Vec3,
) -> Option<(f32, Vec3)> {
	const EPSILON: f32 = 1e-7;
	let edge1 = v1 - v0;
	let edge2 = v2 - v0;
	let h = direction.cross(edge2);
	let a = edge1.dot(h);
	if a.abs() < EPSILON {
		return None;
	}
	let f = 1.0 / a;
	let s = origin - v0;
	let u = f * s.dot(h);
	if !(0.0..=1.0).contains(&u) {
		return None;
	}
	let q = s.cross(edge1);
	let v = f * direction.dot(q);
	if v < 0.0 || u + v > 1.0 {
		return None;
	}
	let t = f * edge2.dot(q);
	if t < EPSILON {
		return None;
	}
	let normal = edge1.cross(edge2).normalize_or_zero();
	Some((t, normal))
}

/// iterate mesh triangles, transform ray into model space, return nearest world-space hit.
fn raycast_mesh(
	ray: Ray3d,
	vertices: impl Iterator<Item = Vec3>,
	indices: &crate::mesh::IndexBuffer,
	world: &WorldTransform3d,
	max_dist: f32,
	entity: Entity,
) -> Option<RayHit3d> {
	let inv_rot = Quat::from_xyzw(
		-world.rotation.x,
		-world.rotation.y,
		-world.rotation.z,
		world.rotation.w,
	);
	let inv_scale = Vec3::new(
		if world.scale.x == 0.0 {
			0.0
		} else {
			1.0 / world.scale.x
		},
		if world.scale.y == 0.0 {
			0.0
		} else {
			1.0 / world.scale.y
		},
		if world.scale.z == 0.0 {
			0.0
		} else {
			1.0 / world.scale.z
		},
	);

	// transform ray into model space
	let model_origin = inv_scale * inv_rot.mul_vec3(ray.origin - world.translation);
	let model_dir = inv_scale * inv_rot.mul_vec3(ray.direction);

	let verts: Vec<Vec3> = vertices.collect();

	let mut nearest_t = f32::MAX;
	let mut nearest_model_normal = Vec3::Y;

	macro_rules! test_tris {
		($chunks:expr, $cast:ty) => {
			for tri in $chunks {
				let v0 = verts[tri[0] as $cast];
				let v1 = verts[tri[1] as $cast];
				let v2 = verts[tri[2] as $cast];
				if let Some((t, model_normal)) =
					moller_trumbore(model_origin, model_dir, v0, v1, v2)
					&& t < nearest_t
				{
					nearest_t = t;
					nearest_model_normal = model_normal;
				}
			}
		};
	}

	match indices {
		crate::mesh::IndexBuffer::U16(idx) => test_tris!(idx.chunks_exact(3), usize),
		crate::mesh::IndexBuffer::U32(idx) => test_tris!(idx.chunks_exact(3), usize),
	}

	if nearest_t == f32::MAX {
		return None;
	}

	// hit point back to world space
	let model_hit = model_origin + nearest_t * model_dir;
	let world_hit = world.translation + world.rotation.mul_vec3(world.scale * model_hit);
	let world_dist = (world_hit - ray.origin).length();

	if world_dist > max_dist {
		return None;
	}

	// transform normal: inverse-transpose = R * (1/S) for non-uniform scale
	let world_normal =
		(world.rotation.mul_vec3(inv_scale * nearest_model_normal)).normalize_or_zero();
	// make sure normal points away from ray origin
	let world_normal = if world_normal.dot(ray.direction) > 0.0 {
		-world_normal
	} else {
		world_normal
	};

	Some(RayHit3d {
		entity,
		point: world_hit,
		normal: world_normal,
		distance: world_dist,
	})
}

/// system that rebuilds [`CollisionWorld3d`] from all entities with `Collider3d + WorldTransform3d`.
///
/// entries are sorted by `min_x` after insertion to enable sweep-and-prune in `all_overlaps`.
/// runs in the Physics stage so `CollisionWorld3d` is ready for Update systems.
pub fn build_collision_world_3d(
	query: Query<(Entity, &WorldTransform3d, &Collider3d)>,
	mut collision_world: ResMut<CollisionWorld3d>,
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
	collision_world
		.entries
		.sort_unstable_by(|a, b| a.min_x.total_cmp(&b.min_x));
}

#[cfg(test)]
mod tests {
	use super::*;
	use bevy_ecs::system::IntoSystem;
	use lunar_math::{Quat, Vec3, Vec3A};

	// helpers for raycast tests -----------------------------------------------

	fn identity_world() -> WorldTransform3d {
		WorldTransform3d {
			translation: Vec3::ZERO,
			rotation: Quat::IDENTITY,
			scale: Vec3::ONE,
		}
	}

	fn unit_quad_mesh() -> (Vec<Vec3>, crate::mesh::IndexBuffer) {
		// two triangles forming a unit quad in the XY plane at Z = 0
		let verts = vec![
			Vec3::new(-0.5, -0.5, 0.0),
			Vec3::new(0.5, -0.5, 0.0),
			Vec3::new(0.5, 0.5, 0.0),
			Vec3::new(-0.5, 0.5, 0.0),
		];
		let indices = crate::mesh::IndexBuffer::U16(vec![0, 1, 2, 0, 2, 3]);
		(verts, indices)
	}

	#[test]
	fn moller_trumbore_direct_hit() {
		let origin = Vec3::new(0.0, 0.0, 1.0);
		let direction = Vec3::new(0.0, 0.0, -1.0);
		let v0 = Vec3::new(-1.0, -1.0, 0.0);
		let v1 = Vec3::new(1.0, -1.0, 0.0);
		let v2 = Vec3::new(0.0, 1.0, 0.0);
		let result = moller_trumbore(origin, direction, v0, v1, v2);
		assert!(result.is_some());
		let (t, _) = result.unwrap();
		assert!((t - 1.0).abs() < 1e-5);
	}

	#[test]
	fn moller_trumbore_miss() {
		let origin = Vec3::new(5.0, 5.0, 1.0);
		let direction = Vec3::new(0.0, 0.0, -1.0);
		let v0 = Vec3::new(-1.0, -1.0, 0.0);
		let v1 = Vec3::new(1.0, -1.0, 0.0);
		let v2 = Vec3::new(0.0, 1.0, 0.0);
		assert!(moller_trumbore(origin, direction, v0, v1, v2).is_none());
	}

	#[test]
	fn ray_vs_aabb_3d_hit() {
		let ray = Ray3d::new(Vec3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 0.0, -1.0));
		let center = Vec3A::ZERO;
		let half = Vec3A::ONE;
		let result = ray_vs_aabb_3d(ray, center, half, 100.0);
		assert!(result.is_some());
		let t = result.unwrap();
		assert!((t - 4.0).abs() < 1e-4);
	}

	#[test]
	fn ray_vs_aabb_3d_miss() {
		let ray = Ray3d::new(Vec3::new(5.0, 0.0, 5.0), Vec3::new(0.0, 0.0, -1.0));
		let result = ray_vs_aabb_3d(ray, Vec3A::ZERO, Vec3A::ONE, 100.0);
		assert!(result.is_none());
	}

	#[test]
	fn raycast_mesh_hits_quad() {
		let (verts, indices) = unit_quad_mesh();
		let mesh_verts: Vec<crate::mesh::Vertex3d> = verts
			.iter()
			.map(|&p| {
				crate::mesh::Vertex3d::new(p, Vec3::Z, [1.0, 0.0, 0.0, 1.0], lunar_math::Vec2::ZERO)
			})
			.collect();

		let ray = Ray3d::new(Vec3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 0.0, -1.0));
		let world = identity_world();

		let result = raycast_mesh(
			ray,
			mesh_verts.iter().map(|v| v.position),
			&indices,
			&world,
			100.0,
			Entity::PLACEHOLDER,
		);
		assert!(result.is_some());
		let hit = result.unwrap();
		assert!((hit.distance - 5.0).abs() < 1e-4);
		assert!((hit.point.z).abs() < 1e-4);
	}

	#[test]
	fn raycast_mesh_misses_quad() {
		let (verts, indices) = unit_quad_mesh();
		let mesh_verts: Vec<crate::mesh::Vertex3d> = verts
			.iter()
			.map(|&p| {
				crate::mesh::Vertex3d::new(p, Vec3::Z, [1.0, 0.0, 0.0, 1.0], lunar_math::Vec2::ZERO)
			})
			.collect();

		// ray aimed off to the side
		let ray = Ray3d::new(Vec3::new(5.0, 5.0, 5.0), Vec3::new(0.0, 0.0, -1.0));
		let world = identity_world();

		let result = raycast_mesh(
			ray,
			mesh_verts.iter().map(|v| v.position),
			&indices,
			&world,
			100.0,
			Entity::PLACEHOLDER,
		);
		assert!(result.is_none());
	}

	fn spawn_aabb(world: &mut World, pos: Vec3, size: Vec3) -> Entity {
		world
			.spawn((
				WorldTransform3d {
					translation: pos,
					..WorldTransform3d::new()
				},
				Collider3d::aabb(size),
			))
			.id()
	}

	fn run_build(world: &mut World) {
		let mut system = IntoSystem::into_system(build_collision_world_3d);
		system.initialize(world);
		let _ = system.run((), world);
	}

	#[test]
	fn aabb_overlap_detected() {
		let mut world = World::new();
		world.insert_resource(CollisionWorld3d::default());
		let entity_a = spawn_aabb(&mut world, Vec3::ZERO, Vec3::new(2.0, 2.0, 2.0));
		let entity_b = spawn_aabb(
			&mut world,
			Vec3::new(1.5, 0.0, 0.0),
			Vec3::new(2.0, 2.0, 2.0),
		);

		run_build(&mut world);

		let cw = world.resource::<CollisionWorld3d>();
		assert!(cw.overlapping(entity_a).any(|e| e == entity_b));
	}

	#[test]
	fn aabb_no_overlap_when_far() {
		let mut world = World::new();
		world.insert_resource(CollisionWorld3d::default());
		let entity_a = spawn_aabb(&mut world, Vec3::ZERO, Vec3::new(2.0, 2.0, 2.0));
		spawn_aabb(
			&mut world,
			Vec3::new(100.0, 0.0, 0.0),
			Vec3::new(2.0, 2.0, 2.0),
		);

		run_build(&mut world);

		let cw = world.resource::<CollisionWorld3d>();
		assert!(cw.overlapping(entity_a).next().is_none());
	}

	#[test]
	fn sphere_overlap_detected() {
		let mut world = World::new();
		world.insert_resource(CollisionWorld3d::default());
		let entity_a = world
			.spawn((WorldTransform3d::new(), Collider3d::sphere(1.0)))
			.id();
		let entity_b = world
			.spawn((
				WorldTransform3d {
					translation: Vec3::new(1.5, 0.0, 0.0),
					..WorldTransform3d::new()
				},
				Collider3d::sphere(1.0),
			))
			.id();

		run_build(&mut world);

		let cw = world.resource::<CollisionWorld3d>();
		assert!(cw.overlapping(entity_a).any(|e| e == entity_b));
	}

	#[test]
	fn aabb_sphere_overlap() {
		let mut world = World::new();
		world.insert_resource(CollisionWorld3d::default());
		let entity_a = spawn_aabb(&mut world, Vec3::ZERO, Vec3::new(2.0, 2.0, 2.0));
		let entity_b = world
			.spawn((
				WorldTransform3d {
					translation: Vec3::new(1.2, 0.0, 0.0),
					..WorldTransform3d::new()
				},
				Collider3d::sphere(0.5),
			))
			.id();

		run_build(&mut world);

		let cw = world.resource::<CollisionWorld3d>();
		assert!(cw.overlapping(entity_a).any(|e| e == entity_b));
	}

	#[test]
	fn layer_mask_filtering() {
		let mut world = World::new();
		world.insert_resource(CollisionWorld3d::default());
		world.spawn((
			WorldTransform3d::new(),
			Collider3d::aabb(Vec3::ONE).with_layer(1).with_mask(2),
		));
		let entity_b = world
			.spawn((
				WorldTransform3d {
					translation: Vec3::new(0.1, 0.0, 0.0),
					..WorldTransform3d::new()
				},
				Collider3d::aabb(Vec3::ONE).with_layer(1).with_mask(1),
			))
			.id();

		run_build(&mut world);

		let cw = world.resource::<CollisionWorld3d>();
		assert!(cw.overlapping(entity_b).next().is_none());
	}

	#[test]
	fn sweep_and_prune_skips_far_pairs() {
		let mut world = World::new();
		world.insert_resource(CollisionWorld3d::default());
		let entity_a = spawn_aabb(&mut world, Vec3::ZERO, Vec3::new(2.0, 2.0, 2.0));
		let entity_b = spawn_aabb(
			&mut world,
			Vec3::new(1.5, 0.0, 0.0),
			Vec3::new(2.0, 2.0, 2.0),
		);
		let entity_c = spawn_aabb(
			&mut world,
			Vec3::new(500.0, 0.0, 0.0),
			Vec3::new(2.0, 2.0, 2.0),
		);

		run_build(&mut world);

		let cw = world.resource::<CollisionWorld3d>();
		let pairs: Vec<_> = cw.all_overlaps().collect();
		assert!(pairs.contains(&(entity_a, entity_b)) || pairs.contains(&(entity_b, entity_a)));
		assert!(!pairs.iter().any(|&(x, y)| x == entity_c || y == entity_c));
	}
}
