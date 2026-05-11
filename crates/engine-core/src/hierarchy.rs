//! entity hierarchy components: parent-child relationships.
//!
//! entities form trees via [`Parent`] and [`Children`] components.
//! transform propagation is dimension-specific — use `engine_2d::Plugin2d`
//! (or a future `engine_3d::Plugin3d`) to register the appropriate system.

use bevy_ecs::prelude::*;
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
/// (no command deferral) — children are visible to other systems in the same frame
/// a `Parent` component is added.
pub fn sync_children(world: &mut World) {
    // collect all (child, parent_entity) pairs first to avoid borrow conflicts
    let pairs: Vec<(Entity, Entity)> = world
        .query::<(Entity, &Parent)>()
        .iter(world)
        .map(|(child, parent)| (child, parent.0))
        .collect();

    for (child_entity, parent_entity) in pairs {
        // insert Children component if the parent doesn't have one yet
        if world.get::<Children>(parent_entity).is_none() {
            world.entity_mut(parent_entity).insert(Children::new());
        }

        // add child if not already present — read then mutate to satisfy borrow checker
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

        // run sync_children directly — no command deferral
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
