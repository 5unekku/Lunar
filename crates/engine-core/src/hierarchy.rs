//! entity hierarchy components: parent-child relationships and transform propagation.
//!
//! # architecture
//!
//! entities can form trees via [`Parent`] and [`Children`] components.
//! the [`LocalTransform`] of each child is combined with its parent's [`WorldTransform`]
//! to produce the child's own [`WorldTransform`].
//!
//! # example
//!
//! ```ignore
//! // parent entity
//! let parent = commands.spawn((
//!     LocalTransform::from_xy(100.0, 100.0),
//! )).id();
//!
//! // child entity
//! commands.spawn((
//!     LocalTransform::from_xy(10.0, 0.0),
//!     Parent(parent),
//! ));
//! ```

use std::collections::HashMap;

use bevy_ecs::prelude::*;
use bevy_ecs::schedule::ScheduleLabel;
use engine_math::{LocalTransform, Vec2, WorldTransform};

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

/// exclusive system that propagates transforms from parents to children.
///
/// runs as an exclusive world system so `WorldTransform` is written immediately
/// (no command deferral) — entities have correct world transforms in the same frame
/// they are spawned.
///
/// uses a topological sort (depth-first from roots) so each entity is processed
/// exactly once, giving O(N) propagation regardless of hierarchy depth.
///
/// entities without a parent get their `WorldTransform` directly from `LocalTransform`.
pub fn propagate_transforms(world: &mut World) {
    // collect snapshot — copy values so we can freely mutate world afterward
    let snapshot: Vec<(Entity, LocalTransform, Option<Entity>)> = world
        .query::<(Entity, &LocalTransform, Option<&Parent>)>()
        .iter(world)
        .map(|(entity, local, parent)| (entity, *local, parent.map(|p| p.0)))
        .collect();

    // build parent map for depth computation
    let parent_of: HashMap<Entity, Entity> = snapshot
        .iter()
        .filter_map(|(entity, _, parent)| parent.map(|p| (*entity, p)))
        .collect();

    // compute depth of each entity by walking up toward the root
    let mut depths: HashMap<Entity, u32> = HashMap::with_capacity(snapshot.len());
    for &(entity, _, _) in &snapshot {
        let mut depth = 0u32;
        let mut current = entity;
        while let Some(&parent_entity) = parent_of.get(&current) {
            depth += 1;
            current = parent_entity;
        }
        depths.insert(entity, depth);
    }

    // sort by depth so roots are processed before children (topological order)
    let mut sorted = snapshot;
    sorted.sort_by_key(|(entity, _, _)| depths.get(entity).copied().unwrap_or(0));

    // propagate in topological order — parent WorldTransform is always written first
    for (entity, local, parent_entity) in sorted {
        let world_transform = if let Some(parent) = parent_entity {
            // copy the parent's WorldTransform (already written in this pass)
            if let Some(parent_wt) = world.get::<WorldTransform>(parent).copied() {
                compute_world_transform(&parent_wt, &local)
            } else {
                // parent missing WorldTransform (cycle or missing LocalTransform) — use local
                WorldTransform {
                    translation: local.translation,
                    rotation: local.rotation,
                    scale: local.scale,
                }
            }
        } else {
            WorldTransform {
                translation: local.translation,
                rotation: local.rotation,
                scale: local.scale,
            }
        };

        // write directly — no deferral, visible to all systems in the same frame
        if let Some(mut wt) = world.get_mut::<WorldTransform>(entity) {
            *wt = world_transform;
        } else {
            world.entity_mut(entity).insert(world_transform);
        }
    }
}

/// compute the world transform by combining a parent's world transform with a local transform.
fn compute_world_transform(parent: &WorldTransform, local: &LocalTransform) -> WorldTransform {
    // scale local translation by parent scale
    let scaled_x = local.translation.x * parent.scale.x;
    let scaled_y = local.translation.y * parent.scale.y;

    // rotate by parent rotation
    let cos = parent.rotation.cos();
    let sin = parent.rotation.sin();
    let rotated_x = scaled_x.mul_add(cos, -scaled_y * sin);
    let rotated_y = scaled_x.mul_add(sin, scaled_y * cos);

    WorldTransform {
        translation: Vec2::new(
            parent.translation.x + rotated_x,
            parent.translation.y + rotated_y,
        ),
        rotation: parent.rotation + local.rotation,
        scale: Vec2::new(
            parent.scale.x * local.scale.x,
            parent.scale.y * local.scale.y,
        ),
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
        app.add_system(propagate_transforms);
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
    fn compute_world_transform_no_parent() {
        let parent = WorldTransform::from_xy(0.0, 0.0);
        let local = LocalTransform::from_xy(10.0, 20.0);
        let world_transform = compute_world_transform(&parent, &local);
        assert!((world_transform.translation.x - 10.0).abs() < 0.001);
        assert!((world_transform.translation.y - 20.0).abs() < 0.001);
        assert!((world_transform.rotation - 0.0).abs() < 0.001);
        assert!((world_transform.scale.x - 1.0).abs() < 0.001);
        assert!((world_transform.scale.y - 1.0).abs() < 0.001);
    }

    #[test]
    fn compute_world_transform_with_parent_rotation() {
        let parent = WorldTransform {
            translation: Vec2::new(100.0, 100.0),
            rotation: std::f32::consts::PI / 2.0,
            scale: Vec2::ONE,
        };
        let local = LocalTransform::from_xy(10.0, 0.0);
        let world_transform = compute_world_transform(&parent, &local);
        // parent rotated 90 degrees: local (10, 0) becomes (0, 10) in world space
        assert!((world_transform.translation.x - 100.0).abs() < 0.001);
        assert!((world_transform.translation.y - 110.0).abs() < 0.001);
    }

    #[test]
    fn compute_world_transform_with_parent_scale() {
        let parent = WorldTransform {
            translation: Vec2::ZERO,
            rotation: 0.0,
            scale: Vec2::new(2.0, 3.0),
        };
        let local = LocalTransform {
            translation: Vec2::new(5.0, 4.0),
            rotation: 0.0,
            scale: Vec2::new(1.0, 1.0),
        };
        let world_transform = compute_world_transform(&parent, &local);
        assert!((world_transform.translation.x - 10.0).abs() < 0.001);
        assert!((world_transform.translation.y - 12.0).abs() < 0.001);
        assert!((world_transform.scale.x - 2.0).abs() < 0.001);
        assert!((world_transform.scale.y - 3.0).abs() < 0.001);
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

    #[test]
    fn propagate_transforms_writes_immediately() {
        let mut world = World::new();
        let parent = world.spawn(LocalTransform::from_xy(100.0, 0.0)).id();
        let child = world
            .spawn((LocalTransform::from_xy(10.0, 0.0), Parent(parent)))
            .id();

        // run propagate_transforms directly — should insert WorldTransform in the same call
        propagate_transforms(&mut world);

        let parent_wt = world
            .get::<WorldTransform>(parent)
            .expect("parent WorldTransform");
        assert!((parent_wt.translation.x - 100.0).abs() < 0.001);

        let child_wt = world
            .get::<WorldTransform>(child)
            .expect("child WorldTransform");
        assert!((child_wt.translation.x - 110.0).abs() < 0.001);
    }
}
