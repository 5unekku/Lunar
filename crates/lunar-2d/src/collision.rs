//! 2d collision detection — AABB and circle shapes, overlap queries.
//!
//! no physics simulation (no rigid bodies, velocity integration, gravity).
//! this module answers the question "what overlaps what" — game logic decides
//! what to do about it.
//!
//! # usage
//!
//! ```ignore
//! use lunar_2d::collision::{Collider, Collider2dBundle, ColliderShape, CollisionWorld};
//!
//! // spawn a collider using the bundle
//! commands.spawn(Collider2dBundle {
//!     transform: Transform::from_xy(0.0, 0.0),
//!     collider: Collider::aabb(Vec2::new(16.0, 16.0)),
//! });
//!
//! // query overlaps in a system
//! fn check_hits(world: Res<CollisionWorld>) {
//!     for (a, b) in world.all_overlaps() {
//!         // handle collision
//!     }
//! }
//! ```

use bevy_ecs::bundle::Bundle;
use bevy_ecs::prelude::*;
use lunar_math::{Transform, Vec2};

/// shape variant for a [`Collider`] component.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColliderShape {
	/// axis-aligned bounding box. `half_extents` is half the width/height.
	Aabb { half_extents: Vec2 },
	/// circle centered on the entity's transform position.
	Circle { radius: f32 },
}

/// component that makes an entity participate in 2d collision detection.
///
/// attach alongside a [`Transform`] component, or use [`Collider2dBundle`].
/// the `CollisionWorld` resource is rebuilt from all entities that have both every physics tick.
#[derive(Debug, Clone, Component)]
pub struct Collider {
	pub shape: ColliderShape,
	/// bitmask — which collision layers this collider belongs to.
	pub layer: u32,
	/// bitmask — which layers this collider checks against.
	pub mask: u32,
}

impl Collider {
	/// axis-aligned bounding box with the given full size (half_extents = size / 2).
	#[must_use]
	pub fn aabb(size: Vec2) -> Self {
		Self {
			shape: ColliderShape::Aabb {
				half_extents: size * 0.5,
			},
			layer: 1,
			mask: 1,
		}
	}

	/// circle with the given radius.
	#[must_use]
	pub fn circle(radius: f32) -> Self {
		Self {
			shape: ColliderShape::Circle { radius },
			layer: 1,
			mask: 1,
		}
	}

	/// set the collision layer (builder pattern).
	#[must_use]
	pub fn with_layer(mut self, layer: u32) -> Self {
		self.layer = layer;
		self
	}

	/// set the collision mask (builder pattern).
	#[must_use]
	pub fn with_mask(mut self, mask: u32) -> Self {
		self.mask = mask;
		self
	}
}

/// convenience bundle — pairs a [`Transform`] with a [`Collider`].
#[derive(Bundle)]
pub struct Collider2dBundle {
	pub transform: Transform,
	pub collider: Collider,
}

/// a single entry in the collision world snapshot.
///
/// `min_x` / `max_x` are precomputed for the sweep-and-prune broad phase.
#[derive(Debug, Clone)]
struct ColliderEntry {
	entity: Entity,
	position: Vec2,
	shape: ColliderShape,
	layer: u32,
	mask: u32,
	min_x: f32,
	max_x: f32,
}

impl ColliderEntry {
	fn new(entity: Entity, position: Vec2, shape: ColliderShape, layer: u32, mask: u32) -> Self {
		let (min_x, max_x) = match shape {
			ColliderShape::Aabb { half_extents } => {
				(position.x - half_extents.x, position.x + half_extents.x)
			}
			ColliderShape::Circle { radius } => (position.x - radius, position.x + radius),
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
		shapes_overlap(self.position, &self.shape, other.position, &other.shape)
	}
}

fn shapes_overlap(
	pos_a: Vec2,
	shape_a: &ColliderShape,
	pos_b: Vec2,
	shape_b: &ColliderShape,
) -> bool {
	match (shape_a, shape_b) {
		(
			ColliderShape::Aabb {
				half_extents: half_a,
			},
			ColliderShape::Aabb {
				half_extents: half_b,
			},
		) => {
			(pos_a.x - pos_b.x).abs() < half_a.x + half_b.x
				&& (pos_a.y - pos_b.y).abs() < half_a.y + half_b.y
		}
		(ColliderShape::Circle { radius: ra }, ColliderShape::Circle { radius: rb }) => {
			(pos_a - pos_b).length_squared() < (ra + rb) * (ra + rb)
		}
		(ColliderShape::Aabb { half_extents }, ColliderShape::Circle { radius })
		| (ColliderShape::Circle { radius }, ColliderShape::Aabb { half_extents }) => {
			let (aabb_pos, circle_pos) = if matches!(shape_a, ColliderShape::Aabb { .. }) {
				(pos_a, pos_b)
			} else {
				(pos_b, pos_a)
			};
			let closest = Vec2::new(
				circle_pos
					.x
					.clamp(aabb_pos.x - half_extents.x, aabb_pos.x + half_extents.x),
				circle_pos
					.y
					.clamp(aabb_pos.y - half_extents.y, aabb_pos.y + half_extents.y),
			);
			(circle_pos - closest).length_squared() < radius * radius
		}
	}
}

/// resource rebuilt every physics tick — holds the current frame's collider snapshot.
///
/// read this from any system in the Update stage or later to query overlaps.
///
/// # example
///
/// ```ignore
/// fn check_hits(collision_world: Res<CollisionWorld>, query: Query<(Entity, &Collider)>) {
///     for (entity, _collider) in &query {
///         for other in collision_world.overlapping(entity) {
///             // handle collision with `other`
///         }
///     }
/// }
/// ```
#[derive(Debug, Default, Resource)]
pub struct CollisionWorld {
	entries: Vec<ColliderEntry>,
}

impl CollisionWorld {
	/// sweep-and-prune range for a query span `[qmin_x, qmax_x]`.
	/// returns a slice of entries that could overlap along X — use as a pre-filter.
	fn x_candidates(&self, _qmin_x: f32, qmax_x: f32) -> &[ColliderEntry] {
		// entries sorted by min_x; stop at the first entry entirely to the right
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
			let overlaps = target
				.as_ref()
				.is_some_and(|t| other.max_x > t.min_x && t.overlaps(other));
			overlaps.then_some(other.entity)
		})
	}

	/// iterator over all entities whose collider contains `point`.
	pub fn query_point(&self, point: Vec2) -> impl Iterator<Item = Entity> + '_ {
		self.entries.iter().filter_map(move |entry| {
			point_in_shape(point, entry.position, &entry.shape).then_some(entry.entity)
		})
	}

	/// iterator over all entities whose collider overlaps `rect` (center + half_extents).
	///
	/// uses sweep-and-prune on X.
	pub fn query_rect(
		&self,
		center: Vec2,
		half_extents: Vec2,
	) -> impl Iterator<Item = Entity> + '_ {
		let qmin_x = center.x - half_extents.x;
		let qmax_x = center.x + half_extents.x;
		let candidates = self.x_candidates(qmin_x, qmax_x);
		let rect_shape = ColliderShape::Aabb { half_extents };
		candidates.iter().filter_map(move |entry| {
			if entry.max_x <= qmin_x {
				return None;
			}
			shapes_overlap(center, &rect_shape, entry.position, &entry.shape)
				.then_some(entry.entity)
		})
	}

	/// iterator over all overlapping pairs this frame. each pair appears once.
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

fn point_in_shape(point: Vec2, position: Vec2, shape: &ColliderShape) -> bool {
	match shape {
		ColliderShape::Aabb { half_extents } => {
			(point.x - position.x).abs() <= half_extents.x
				&& (point.y - position.y).abs() <= half_extents.y
		}
		ColliderShape::Circle { radius } => (point - position).length_squared() <= radius * radius,
	}
}

/// system that rebuilds [`CollisionWorld`] from all entities with `Collider + Transform`.
///
/// entries are sorted by `min_x` after insertion to enable sweep-and-prune in `all_overlaps`.
/// runs in the Physics stage so `CollisionWorld` is ready for Update systems.
pub fn build_collision_world(
	query: Query<(Entity, &Transform, &Collider)>,
	mut collision_world: ResMut<CollisionWorld>,
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

/// result of a successful ray cast.
#[derive(Debug, Clone, Copy)]
pub struct RayHit2d {
	/// entity that was hit
	pub entity: Entity,
	/// world-space point where the ray intersected the shape surface
	pub point: Vec2,
	/// surface normal at the hit point (normalized)
	pub normal: Vec2,
	/// distance from ray origin to hit point
	pub distance: f32,
}

/// cast a ray against all colliders in `world` and return the nearest hit.
///
/// `direction` should be normalized. `max_dist` caps the search; use `f32::MAX`
/// for unbounded. O(N) against all colliders in the world — suitable for
/// interactive use (player sight, hitscan) but not for mass parallel queries.
///
/// only colliders whose `layer` is matched by `mask` are tested.
pub fn ray_cast_2d(
	origin: Vec2,
	direction: Vec2,
	max_dist: f32,
	mask: u32,
	world: &CollisionWorld,
) -> Option<RayHit2d> {
	let mut nearest: Option<RayHit2d> = None;

	for entry in &world.entries {
		if entry.layer & mask == 0 {
			continue;
		}
		let hit = match entry.shape {
			ColliderShape::Aabb { half_extents } => {
				ray_vs_aabb(origin, direction, entry.position, half_extents, max_dist)
			}
			ColliderShape::Circle { radius } => {
				ray_vs_circle(origin, direction, entry.position, radius, max_dist)
			}
		};
		if let Some((distance, point, normal)) = hit
			&& nearest.as_ref().is_none_or(|n| distance < n.distance)
		{
			nearest = Some(RayHit2d {
				entity: entry.entity,
				point,
				normal,
				distance,
			});
		}
	}

	nearest
}

/// ray vs AABB slab test. returns (distance, point, normal) on hit.
fn ray_vs_aabb(
	origin: Vec2,
	direction: Vec2,
	center: Vec2,
	half_extents: Vec2,
	max_dist: f32,
) -> Option<(f32, Vec2, Vec2)> {
	let inv_direction = Vec2::new(
		if direction.x.abs() > f32::EPSILON {
			1.0 / direction.x
		} else {
			f32::MAX
		},
		if direction.y.abs() > f32::EPSILON {
			1.0 / direction.y
		} else {
			f32::MAX
		},
	);

	let min = center - half_extents;
	let max = center + half_extents;

	let t1 = (min.x - origin.x) * inv_direction.x;
	let t2 = (max.x - origin.x) * inv_direction.x;
	let t3 = (min.y - origin.y) * inv_direction.y;
	let t4 = (max.y - origin.y) * inv_direction.y;

	let tmin = t1.min(t2).max(t3.min(t4));
	let tmax = t1.max(t2).min(t3.max(t4));

	if tmax < 0.0 || tmin > tmax || tmin > max_dist {
		return None;
	}

	let distance = if tmin < 0.0 { tmax } else { tmin };
	if distance > max_dist || distance < 0.0 {
		return None;
	}

	let point = origin + direction * distance;
	let local = point - center;
	let normal = if local.x.abs() / half_extents.x > local.y.abs() / half_extents.y {
		Vec2::new(local.x.signum(), 0.0)
	} else {
		Vec2::new(0.0, local.y.signum())
	};

	Some((distance, point, normal))
}

/// ray vs circle test. returns (distance, point, normal) on hit.
fn ray_vs_circle(
	origin: Vec2,
	direction: Vec2,
	center: Vec2,
	radius: f32,
	max_dist: f32,
) -> Option<(f32, Vec2, Vec2)> {
	let oc = origin - center;
	let a = direction.dot(direction);
	let b = 2.0 * oc.dot(direction);
	let c = oc.dot(oc) - radius * radius;
	let discriminant = b * b - 4.0 * a * c;
	if discriminant < 0.0 {
		return None;
	}
	let sqrt_d = discriminant.sqrt();
	let t = (-b - sqrt_d) / (2.0 * a);
	let distance = if t > 0.0 {
		t
	} else {
		(-b + sqrt_d) / (2.0 * a)
	};
	if distance < 0.0 || distance > max_dist {
		return None;
	}
	let point = origin + direction * distance;
	let normal = (point - center) / radius;
	Some((distance, point, normal))
}

#[cfg(test)]
mod tests {
	use super::*;
	use lunar_math::Transform;

	fn make_world_with_aabbs() -> (World, Entity, Entity) {
		let mut world = World::new();
		world.insert_resource(CollisionWorld::default());
		let entity_a = world
			.spawn((
				Transform::from_xy(0.0, 0.0),
				Collider::aabb(Vec2::new(20.0, 20.0)),
			))
			.id();
		let entity_b = world
			.spawn((
				Transform::from_xy(15.0, 0.0),
				Collider::aabb(Vec2::new(20.0, 20.0)),
			))
			.id();
		(world, entity_a, entity_b)
	}

	fn run_build(world: &mut World) {
		let mut system = IntoSystem::into_system(build_collision_world);
		system.initialize(world);
		let _ = system.run((), world);
	}

	#[test]
	fn aabb_overlap_detected() {
		let (mut world, entity_a, entity_b) = make_world_with_aabbs();
		run_build(&mut world);
		let collision_world = world.resource::<CollisionWorld>();
		assert!(collision_world.overlapping(entity_a).any(|e| e == entity_b));
	}

	#[test]
	fn aabb_no_overlap_when_far() {
		let mut world = World::new();
		world.insert_resource(CollisionWorld::default());
		let entity_a = world
			.spawn((
				Transform::from_xy(0.0, 0.0),
				Collider::aabb(Vec2::new(10.0, 10.0)),
			))
			.id();
		world.spawn((
			Transform::from_xy(100.0, 0.0),
			Collider::aabb(Vec2::new(10.0, 10.0)),
		));

		run_build(&mut world);

		let collision_world = world.resource::<CollisionWorld>();
		assert!(collision_world.overlapping(entity_a).next().is_none());
	}

	#[test]
	fn circle_overlap_detected() {
		let mut world = World::new();
		world.insert_resource(CollisionWorld::default());
		let entity_a = world
			.spawn((Transform::from_xy(0.0, 0.0), Collider::circle(10.0)))
			.id();
		let entity_b = world
			.spawn((Transform::from_xy(15.0, 0.0), Collider::circle(10.0)))
			.id();

		run_build(&mut world);

		let collision_world = world.resource::<CollisionWorld>();
		assert!(collision_world.overlapping(entity_a).any(|e| e == entity_b));
	}

	#[test]
	fn layer_mask_filtering() {
		let mut world = World::new();
		world.insert_resource(CollisionWorld::default());
		world.spawn((
			Transform::from_xy(0.0, 0.0),
			Collider::aabb(Vec2::new(20.0, 20.0))
				.with_layer(1)
				.with_mask(2),
		));
		let entity_b = world
			.spawn((
				Transform::from_xy(5.0, 0.0),
				Collider::aabb(Vec2::new(20.0, 20.0))
					.with_layer(1)
					.with_mask(1),
			))
			.id();

		run_build(&mut world);

		let collision_world = world.resource::<CollisionWorld>();
		assert!(collision_world.overlapping(entity_b).next().is_none());
	}

	#[test]
	fn query_point_hits_aabb() {
		let (mut world, entity_a, _) = make_world_with_aabbs();
		run_build(&mut world);
		let collision_world = world.resource::<CollisionWorld>();
		assert!(
			collision_world
				.query_point(Vec2::new(5.0, 5.0))
				.any(|e| e == entity_a)
		);
	}

	#[test]
	fn sweep_and_prune_skips_far_pairs() {
		// three AABBs: a and c are far apart (no x overlap), b is between them
		let mut world = World::new();
		world.insert_resource(CollisionWorld::default());
		let entity_a = world
			.spawn((
				Transform::from_xy(0.0, 0.0),
				Collider::aabb(Vec2::new(10.0, 10.0)),
			))
			.id();
		let entity_b = world
			.spawn((
				Transform::from_xy(8.0, 0.0),
				Collider::aabb(Vec2::new(10.0, 10.0)),
			))
			.id();
		let entity_c = world
			.spawn((
				Transform::from_xy(200.0, 0.0),
				Collider::aabb(Vec2::new(10.0, 10.0)),
			))
			.id();

		run_build(&mut world);

		let collision_world = world.resource::<CollisionWorld>();
		let pairs: Vec<_> = collision_world.all_overlaps().collect();
		assert!(pairs.contains(&(entity_a, entity_b)) || pairs.contains(&(entity_b, entity_a)));
		assert!(!pairs.iter().any(|&(x, y)| x == entity_c || y == entity_c));
	}

	#[test]
	fn collider2d_bundle_spawns() {
		let mut world = World::new();
		world.insert_resource(CollisionWorld::default());
		let entity = world
			.spawn(Collider2dBundle {
				transform: Transform::from_xy(1.0, 2.0),
				collider: Collider::circle(5.0),
			})
			.id();
		assert!(world.get::<Collider>(entity).is_some());
		assert!(world.get::<Transform>(entity).is_some());
	}

	fn build_ray_world(shapes: &[(Vec2, ColliderShape)]) -> CollisionWorld {
		let mut world = World::new();
		let entries = shapes
			.iter()
			.map(|&(position, shape)| {
				let entity = world.spawn_empty().id();
				ColliderEntry::new(entity, position, shape, 1, 1)
			})
			.collect();
		CollisionWorld { entries }
	}

	#[test]
	fn ray_hits_aabb_from_left() {
		let world = build_ray_world(&[(
			Vec2::new(50.0, 0.0),
			ColliderShape::Aabb {
				half_extents: Vec2::new(10.0, 10.0),
			},
		)]);
		let hit = ray_cast_2d(Vec2::ZERO, Vec2::new(1.0, 0.0), 1000.0, 1, &world);
		let hit = hit.expect("expected a hit");
		assert!(
			(hit.distance - 40.0).abs() < 0.1,
			"expected distance ~40, got {}",
			hit.distance
		);
		assert!(
			(hit.normal.x - (-1.0)).abs() < 0.01,
			"expected left-face normal"
		);
	}

	#[test]
	fn ray_misses_aabb_when_offset() {
		let world = build_ray_world(&[(
			Vec2::new(50.0, 100.0),
			ColliderShape::Aabb {
				half_extents: Vec2::new(10.0, 10.0),
			},
		)]);
		let hit = ray_cast_2d(Vec2::ZERO, Vec2::new(1.0, 0.0), 1000.0, 1, &world);
		assert!(hit.is_none());
	}

	#[test]
	fn ray_hits_nearest_of_two_aabbs() {
		let world = build_ray_world(&[
			(
				Vec2::new(100.0, 0.0),
				ColliderShape::Aabb {
					half_extents: Vec2::new(10.0, 10.0),
				},
			),
			(
				Vec2::new(50.0, 0.0),
				ColliderShape::Aabb {
					half_extents: Vec2::new(10.0, 10.0),
				},
			),
		]);
		let hit = ray_cast_2d(Vec2::ZERO, Vec2::new(1.0, 0.0), 1000.0, 1, &world);
		let hit = hit.expect("expected a hit");
		assert!(
			(hit.distance - 40.0).abs() < 0.1,
			"should hit closer box first"
		);
	}

	#[test]
	fn ray_respects_max_dist() {
		let world = build_ray_world(&[(
			Vec2::new(50.0, 0.0),
			ColliderShape::Aabb {
				half_extents: Vec2::new(10.0, 10.0),
			},
		)]);
		let hit = ray_cast_2d(Vec2::ZERO, Vec2::new(1.0, 0.0), 20.0, 1, &world);
		assert!(
			hit.is_none(),
			"ray should stop before the box at max_dist=20"
		);
	}

	#[test]
	fn ray_hits_circle() {
		let world =
			build_ray_world(&[(Vec2::new(50.0, 0.0), ColliderShape::Circle { radius: 10.0 })]);
		let hit = ray_cast_2d(Vec2::ZERO, Vec2::new(1.0, 0.0), 1000.0, 1, &world);
		let hit = hit.expect("expected a hit");
		assert!((hit.distance - 40.0).abs() < 0.1);
		assert!((hit.normal.x - (-1.0)).abs() < 0.01);
	}
}
