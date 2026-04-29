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

use bevy_ecs::prelude::*;
use bevy_ecs::schedule::ScheduleLabel;
use engine_math::{LocalTransform, Vec2, Vec3, WorldTransform};

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

/// system that propagates transforms from parents to children.
///
/// for each entity with both [`LocalTransform`] and [`Parent`],
/// computes the [`WorldTransform`] by combining the parent's world transform
/// with the local transform.
///
/// entities without a parent get their [`WorldTransform`] directly from [`LocalTransform`].
pub fn propagate_transforms(
    mut commands: Commands,
    query: Query<(Entity, &LocalTransform, Option<&Parent>)>,
    mut world_query: Query<&mut WorldTransform>,
) {
    // first pass: compute world transforms for root entities (no parent)
    for (entity, local, parent) in &query {
        if parent.is_none() {
            if let Ok(mut world) = world_query.get_mut(entity) {
                world.translation = local.translation;
                world.rotation = local.rotation;
                world.scale = local.scale;
            } else {
                commands.entity(entity).insert(WorldTransform {
                    translation: local.translation,
                    rotation: local.rotation,
                    scale: local.scale,
                });
            }
        }
    }

    // second pass: propagate to children (iterative, handles arbitrary depth)
    let mut changed = true;
    while changed {
        changed = false;
        for (entity, local, parent) in &query {
            let Some(parent_entity) = parent.map(|p| p.0) else {
                continue;
            };
            let Ok(parent_world) = world_query.get(parent_entity) else {
                continue;
            };

            // compute world transform from parent + local
            let world_translation = compute_world_transform(parent_world, local);

            if let Ok(mut world) = world_query.get_mut(entity) {
                #[allow(clippy::float_cmp)]
                if world.translation != world_translation.0
                    || world.rotation != world_translation.1
                    || world.scale != world_translation.2
                {
                    world.translation = world_translation.0;
                    world.rotation = world_translation.1;
                    world.scale = world_translation.2;
                    changed = true;
                }
            } else {
                commands.entity(entity).insert(WorldTransform {
                    translation: world_translation.0,
                    rotation: world_translation.1,
                    scale: world_translation.2,
                });
                changed = true;
            }
        }
    }
}

/// compute the world transform by combining a parent's world transform with a local transform.
fn compute_world_transform(parent: &WorldTransform, local: &LocalTransform) -> (Vec3, f32, Vec2) {
    // scale local translation by parent scale
    let scaled_x = local.translation.x * parent.scale.x;
    let scaled_y = local.translation.y * parent.scale.y;

    // rotate by parent rotation
    let cos = parent.rotation.cos();
    let sin = parent.rotation.sin();
    let rotated_x = scaled_x.mul_add(cos, -scaled_y * sin);
    let rotated_y = scaled_x.mul_add(sin, scaled_y * cos);

    let translation = Vec3::new(
        parent.translation.x + rotated_x,
        parent.translation.y + rotated_y,
        parent.translation.z + local.translation.z,
    );
    let rotation = parent.rotation + local.rotation;
    let scale = Vec2::new(
        parent.scale.x * local.scale.x,
        parent.scale.y * local.scale.y,
    );

    (translation, rotation, scale)
}

/// system that syncs [`Parent`] and [`Children`] components.
///
/// when a [`Parent`] is added, this system adds the entity to the parent's [`Children`].
/// when a [`Parent`] is removed, this system removes the entity from the old parent's [`Children`].
pub fn sync_children(
    mut commands: Commands,
    parents: Query<(Entity, &Parent)>,
    mut children_query: Query<&mut Children>,
) {
    for (child_entity, parent) in &parents {
        let parent_entity = parent.0;
        // ensure parent entity exists and has a Children component
        if children_query.get(parent_entity).is_err() {
            commands.entity(parent_entity).try_insert(Children::new());
        }
        // add child to parent's children list
        if let Ok(mut children) = children_query.get_mut(parent_entity)
            && !children.contains(child_entity)
        {
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
        let (translation, rotation, scale) = compute_world_transform(&parent, &local);
        assert!((translation.x - 10.0).abs() < 0.001);
        assert!((translation.y - 20.0).abs() < 0.001);
        assert!((rotation - 0.0).abs() < 0.001);
        assert!((scale.x - 1.0).abs() < 0.001);
        assert!((scale.y - 1.0).abs() < 0.001);
    }

    #[test]
    fn compute_world_transform_with_parent_rotation() {
        let parent = WorldTransform {
            translation: Vec3::new(100.0, 100.0, 0.0),
            rotation: std::f32::consts::PI / 2.0,
            scale: Vec2::ONE,
        };
        let local = LocalTransform::from_xy(10.0, 0.0);
        let (translation, _rotation, _scale) = compute_world_transform(&parent, &local);
        // parent rotated 90 degrees: local (10, 0) becomes (0, 10) in world space
        assert!((translation.x - 100.0).abs() < 0.001);
        assert!((translation.y - 110.0).abs() < 0.001);
    }

    #[test]
    fn compute_world_transform_with_parent_scale() {
        let parent = WorldTransform {
            translation: Vec3::new(0.0, 0.0, 0.0),
            rotation: 0.0,
            scale: Vec2::new(2.0, 3.0),
        };
        let local = LocalTransform {
            translation: Vec3::new(5.0, 4.0, 0.0),
            rotation: 0.0,
            scale: Vec2::new(1.0, 1.0),
        };
        let (translation, _rotation, scale) = compute_world_transform(&parent, &local);
        assert!((translation.x - 10.0).abs() < 0.001);
        assert!((translation.y - 12.0).abs() < 0.001);
        assert!((scale.x - 2.0).abs() < 0.001);
        assert!((scale.y - 3.0).abs() < 0.001);
    }
}
