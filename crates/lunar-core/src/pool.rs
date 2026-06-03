//! entity pooling — pre-spawned, reusable entities for high-churn objects.
//!
//! insert a [`Pool`] resource to maintain a reservoir of dormant entities.
//! calling [`Pool::acquire`] pops one from the pool and marks it active;
//! [`Pool::release`] returns it. when the pool is empty, `acquire` spawns
//! a fresh entity.
//!
//! # example
//!
//! ```ignore
//! use lunar_core::pool::Pool;
//! use lunar_math::LocalTransform;
//!
//! // during setup: create a bullet pool with 64 pre-spawned entities
//! let pool = Pool::new(world, 64, || LocalTransform::default());
//! world.insert_resource(pool);
//!
//! // fire a bullet
//! fn fire(world: &mut World) {
//!     let mut pool = world.remove_resource::<Pool>().unwrap();
//!     let bullet = pool.acquire(world);
//!     world.entity_mut(bullet).insert(LocalTransform::from_xy(100.0, 200.0));
//!     world.insert_resource(pool);
//! }
//!
//! // on impact: return to pool
//! fn on_hit(world: &mut World, bullet: Entity) {
//!     let mut pool = world.remove_resource::<Pool>().unwrap();
//!     pool.release(bullet);
//!     world.insert_resource(pool);
//! }
//! ```

use bevy_ecs::prelude::*;

/// pre-spawned entity reservoir for high-churn objects like bullets or particles.
///
/// entities in the pool are considered dormant — game code should hide or disable
/// them when released (e.g. move off-screen, remove render components). acquire
/// reactivates one; release puts it back. grows automatically when empty.
#[derive(Resource)]
pub struct Pool {
	available: Vec<Entity>,
}

impl Pool {
	/// create a pool with `capacity` pre-spawned entities. `seed` is called once
	/// per entity to insert initial components (position, state, etc.).
	pub fn new<B: Bundle, F: FnMut() -> B>(
		world: &mut World,
		capacity: usize,
		mut seed: F,
	) -> Self {
		let available = (0..capacity).map(|_| world.spawn(seed()).id()).collect();
		Self { available }
	}

	/// create an empty pool. entities are spawned on-demand when `acquire` is called.
	#[must_use]
	pub fn empty() -> Self {
		Self {
			available: Vec::new(),
		}
	}

	/// take a dormant entity from the pool. if the pool is empty, spawns a new entity.
	pub fn acquire(&mut self, world: &mut World) -> Entity {
		self.available
			.pop()
			.unwrap_or_else(|| world.spawn_empty().id())
	}

	/// return an entity to the pool for future reuse.
	///
	/// game code is responsible for resetting the entity's components before release,
	/// or after the next acquire, depending on the use pattern.
	pub fn release(&mut self, entity: Entity) {
		self.available.push(entity);
	}

	/// number of entities currently waiting in the pool
	#[must_use]
	pub fn available(&self) -> usize {
		self.available.len()
	}

	/// pre-fill the pool with `count` more entities using the provided seed.
	pub fn grow<B: Bundle, F: FnMut() -> B>(
		&mut self,
		world: &mut World,
		count: usize,
		mut seed: F,
	) {
		for _ in 0..count {
			let entity = world.spawn(seed()).id();
			self.available.push(entity);
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn acquire_returns_unique_entities() {
		let mut world = World::new();
		let mut pool = Pool::new(&mut world, 3, || ());

		let a = pool.acquire(&mut world);
		let b = pool.acquire(&mut world);
		let c = pool.acquire(&mut world);
		assert_ne!(a, b);
		assert_ne!(b, c);
		assert_ne!(a, c);
	}

	#[test]
	fn release_returns_entity_to_pool() {
		let mut world = World::new();
		let mut pool = Pool::new(&mut world, 2, || ());

		let entity = pool.acquire(&mut world);
		assert_eq!(pool.available(), 1);
		pool.release(entity);
		assert_eq!(pool.available(), 2);
	}

	#[test]
	fn acquire_from_empty_pool_spawns_new() {
		let mut world = World::new();
		let mut pool = Pool::empty();
		assert_eq!(pool.available(), 0);

		let entity = pool.acquire(&mut world);
		assert!(world.get_entity(entity).is_ok());
		assert_eq!(pool.available(), 0);
	}

	#[test]
	fn acquire_release_acquire_reuses_entity() {
		let mut world = World::new();
		let mut pool = Pool::new(&mut world, 1, || ());

		let first = pool.acquire(&mut world);
		pool.release(first);
		let second = pool.acquire(&mut world);
		assert_eq!(first, second);
	}

	#[test]
	fn grow_increases_available_count() {
		let mut world = World::new();
		let mut pool = Pool::empty();
		pool.grow(&mut world, 5, || ());
		assert_eq!(pool.available(), 5);
	}
}
