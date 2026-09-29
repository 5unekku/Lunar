//! entity hierarchy components: parent-child relationships.
//!
//! entities form trees via [`Parent`] and [`Children`] components.
//! transform propagation is dimension-specific, use `lunar_2d::Plugin2d`
//! (or a future `lunar_3d::Plugin3d`) to register the appropriate system.

use bevy_ecs::prelude::*;
use bevy_ecs::query::Added;
use bevy_ecs::schedule::ScheduleLabel;

use crate::App;

/// component that stores the parent entity reference.
///
/// an entity can only have one parent. adding a [`Parent`] component
/// automatically updates the parent's [`Children`] component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Component)]
pub struct Parent(pub Entity);

/// component that stores the list of child entities.
///
/// this is automatically maintained when [`Parent`] components are added/removed.
/// use the [`Children`] component to iterate over an entity's children.
#[derive(Debug, Clone, Component)]
pub struct Children(pub smallvec::SmallVec<[Entity; 4]>);

impl Children {
	/// create an empty children list.
	#[must_use]
	pub fn new() -> Self {
		Self(smallvec::SmallVec::new())
	}

	/// check if a specific entity is a child.
	#[must_use]
	pub fn contains(&self, entity: Entity) -> bool {
		self.0.contains(&entity)
	}

	/// get the number of children.
	#[must_use]
	pub fn len(&self) -> usize {
		self.0.len()
	}

	/// check if there are no children.
	#[must_use]
	pub fn is_empty(&self) -> bool {
		self.0.is_empty()
	}

	/// iterate over child entities.
	pub fn iter(&self) -> impl Iterator<Item = &Entity> {
		self.0.iter()
	}
}

impl Default for Children {
	fn default() -> Self {
		Self::new()
	}
}

/// exclusive system that syncs [`Parent`] and [`Children`] components.
///
/// runs as an exclusive world system so `Children` is updated immediately
/// (no command deferral); children are visible to other systems in the same frame
/// a `Parent` component is added.
pub fn sync_children(world: &mut World) {
	// only process entities where Parent was just added; fast-path skips stable hierarchies
	let pairs: Vec<(Entity, Entity)> = world
		.query_filtered::<(Entity, &Parent), Added<Parent>>()
		.iter(world)
		.map(|(child, parent)| (child, parent.0))
		.collect();

	if pairs.is_empty() {
		return;
	}

	for (child_entity, parent_entity) in pairs {
		// insert Children component if the parent doesn't have one yet
		if world.get::<Children>(parent_entity).is_none() {
			world.entity_mut(parent_entity).insert(Children::new());
		}

		// add child if not already present, read then mutate to satisfy borrow checker
		let already_present = world
			.get::<Children>(parent_entity)
			.is_some_and(|c| c.contains(child_entity));
		if !already_present && let Some(mut children) = world.get_mut::<Children>(parent_entity) {
			children.0.push(child_entity);
		}
	}
}

/// built-in stage for transform propagation (runs after Update, before Render).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, ScheduleLabel)]
pub struct PostUpdate;

/// plugin that registers hierarchy systems.
pub struct HierarchyPlugin;

impl crate::GamePlugin for HierarchyPlugin {
	fn name(&self) -> &'static str {
		"hierarchy"
	}

	fn build(&mut self, app: &mut App) {
		app.add_system(sync_children);
	}
}

/// depth of node `idx` in a parent forest given as indices (`None` = root), memoized
/// in `depths` (`u32::MAX` = not yet known). used by the 2d and 3d transform
/// propagation to order parents before children.
///
/// iterative, so arbitrarily deep chains cannot overflow the stack, and cycle-safe:
/// `Parent` is plain component data, so game code can build a cycle (a <-> b, or an
/// entity parented to itself). a recursive walk never terminated on one and aborted
/// the process with a stack overflow every frame. nodes on or hanging off a cycle
/// are treated as roots (depth 0) and a warning is logged.
#[doc(hidden)]
pub fn hierarchy_depth(idx: usize, parent_idx: &[Option<usize>], depths: &mut [u32]) -> u32 {
	const UNKNOWN: u32 = u32::MAX;
	if depths[idx] != UNKNOWN {
		return depths[idx];
	}
	// walk up to the first ancestor whose depth is known, or to a root. a walk longer
	// than the node count can only mean a cycle
	let mut steps = 0usize;
	let mut top = idx;
	let top_depth = loop {
		match parent_idx[top] {
			None => break 0,
			Some(parent) if depths[parent] != UNKNOWN => break depths[parent] + 1,
			Some(parent) => {
				steps += 1;
				if steps > parent_idx.len() {
					log::warn!("hierarchy: parent cycle detected; treating its members as roots");
					// give every still-unknown node on the walk depth 0
					let mut node = idx;
					for _ in 0..=parent_idx.len() {
						if depths[node] == UNKNOWN {
							depths[node] = 0;
						}
						match parent_idx[node] {
							Some(parent) => node = parent,
							None => break,
						}
					}
					return depths[idx];
				}
				top = parent;
			}
		}
	};
	// `top` sits `steps` levels above `idx`: fill the path top-down
	let mut node = idx;
	for level in (0..=steps).rev() {
		depths[node] = top_depth + level as u32;
		if level > 0 {
			node = parent_idx[node].expect("walked this edge above");
		}
	}
	depths[idx]
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn children_new_is_empty() {
		let children = Children::new();
		assert!(children.is_empty());
		assert_eq!(children.len(), 0);
	}

	#[test]
	fn children_contains() {
		let entity = Entity::from_bits(1);
		let children = Children(smallvec::SmallVec::from_slice(&[entity]));
		assert!(children.contains(entity));
		assert!(!children.contains(Entity::from_bits(2)));
	}

	#[test]
	fn sync_children_writes_immediately() {
		let mut world = World::new();
		let parent = world.spawn_empty().id();
		let child = world.spawn(Parent(parent)).id();

		// run sync_children directly: no command deferral
		sync_children(&mut world);

		let children = world
			.get::<Children>(parent)
			.expect("parent should have Children");
		assert!(
			children.contains(child),
			"child should be in Children after sync"
		);
	}
}

#[cfg(test)]
mod depth_tests {
	use super::*;

	const UNKNOWN: u32 = u32::MAX;

	fn depths_of(parent_idx: &[Option<usize>]) -> Vec<u32> {
		let mut depths = vec![UNKNOWN; parent_idx.len()];
		for i in 0..parent_idx.len() {
			hierarchy_depth(i, parent_idx, &mut depths);
		}
		depths
	}

	#[test]
	fn chain_and_siblings() {
		// 0 <- 1 <- 2, 0 <- 3, 4 is a separate root
		let parents = [None, Some(0), Some(1), Some(0), None];
		assert_eq!(depths_of(&parents), [0, 1, 2, 1, 0]);
	}

	#[test]
	fn memoized_ancestors_are_reused() {
		let parents = [None, Some(0), Some(1), Some(2)];
		let mut depths = vec![UNKNOWN; 4];
		assert_eq!(hierarchy_depth(3, &parents, &mut depths), 3);
		assert_eq!(depths, [0, 1, 2, 3], "the walk fills every ancestor on the way");
	}

	/// a parent cycle used to recurse until the stack overflowed (process abort).
	#[test]
	fn parent_cycles_terminate() {
		// 0 <-> 1 mutual, 2 parents itself, 3 hangs off the cycle
		let parents = [Some(1), Some(0), Some(2), Some(0)];
		let depths = depths_of(&parents);
		assert!(depths.iter().all(|&d| d != UNKNOWN));
		assert_eq!(depths[2], 0, "a self-parented entity is treated as a root");
	}

	/// deep chains must not recurse: 500k levels would overflow a recursive walk.
	#[test]
	fn very_deep_chain_does_not_overflow() {
		let n = 500_000;
		let parents: Vec<Option<usize>> =
			(0..n).map(|i| if i == 0 { None } else { Some(i - 1) }).collect();
		let mut depths = vec![UNKNOWN; n];
		assert_eq!(hierarchy_depth(n - 1, &parents, &mut depths), (n - 1) as u32);
	}
}
