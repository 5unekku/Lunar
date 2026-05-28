//! behavior tree AI — Selector, Sequence, Condition, and Action nodes.
//!
//! the tree structure and tick loop live here; leaf logic is written by game code.
//! attach a [`BehaviorTree`] component to an AI entity, call [`tick_behavior_trees`]
//! each frame, and implement [`Action`] for your game-specific actions.
//!
//! # usage
//!
//! ```ignore
//! use lunar_ai::{BehaviorTree, BtNode, Status, Action};
//! use bevy_ecs::prelude::*;
//!
//! #[derive(Component)]
//! struct ChasePlayer;
//!
//! impl Action for ChasePlayer {
//!     fn tick(&mut self, entity: Entity, world: &mut World) -> Status {
//!         // move entity toward player position...
//!         Status::Running
//!     }
//! }
//!
//! fn setup(mut commands: Commands) {
//!     commands.spawn((
//!         BehaviorTree::new(BtNode::action(ChasePlayer)),
//!     ));
//! }
//! ```

use bevy_ecs::prelude::*;

/// result of ticking a behavior tree node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// node completed successfully.
    Success,
    /// node failed.
    Failure,
    /// node is still in progress (multi-frame actions).
    Running,
}

/// a leaf action — game code implements this to define AI behavior.
///
/// each call to `tick` should represent one frame's worth of work.
/// return [`Status::Running`] to keep the node active next frame.
pub trait Action: Send + Sync + 'static {
    fn tick(&mut self, entity: Entity, world: &mut World) -> Status;
}

/// a leaf condition — pure read-only predicate evaluated against the world.
pub trait Condition: Send + Sync + 'static {
    fn check(&self, entity: Entity, world: &World) -> bool;
}

/// internal node storage: boxed trait objects so the tree is data-driven.
type BoxAction = Box<dyn Action>;
type BoxCondition = Box<dyn Condition>;

/// a single node in a behavior tree.
pub enum BtNode {
    /// tries children left-to-right, returns Success on first Success, Failure if all fail.
    Selector(Vec<BtNode>),
    /// tries children left-to-right, returns Failure on first Failure, Success if all succeed.
    Sequence(Vec<BtNode>),
    /// inverts the child's result (Success↔Failure; Running stays Running).
    Invert(Box<BtNode>),
    /// evaluates a condition; returns Success if true, Failure if false.
    Condition(BoxCondition),
    /// executes a leaf action.
    Action(BoxAction),
}

impl BtNode {
    /// construct a Selector node.
    #[must_use]
    pub fn selector(children: Vec<BtNode>) -> Self {
        Self::Selector(children)
    }

    /// construct a Sequence node.
    #[must_use]
    pub fn sequence(children: Vec<BtNode>) -> Self {
        Self::Sequence(children)
    }

    /// construct an Invert decorator.
    #[must_use]
    pub fn invert(child: BtNode) -> Self {
        Self::Invert(Box::new(child))
    }

    /// construct a Condition leaf.
    #[must_use]
    pub fn condition(cond: impl Condition) -> Self {
        Self::Condition(Box::new(cond))
    }

    /// construct an Action leaf.
    #[must_use]
    pub fn action(action: impl Action) -> Self {
        Self::Action(Box::new(action))
    }
}

/// component — attach to an entity to give it a behavior tree.
///
/// the tree is ticked once per frame by [`tick_behavior_trees`].
#[derive(Component)]
pub struct BehaviorTree {
    root: BtNode,
    /// last tick's result — readable by game code for debugging.
    pub last_status: Status,
}

impl BehaviorTree {
    /// create a behavior tree with the given root node.
    #[must_use]
    pub fn new(root: BtNode) -> Self {
        Self { root, last_status: Status::Failure }
    }
}

/// tick all [`BehaviorTree`] components in the world.
///
/// runs in Update stage. takes exclusive world access so actions can mutate freely.
pub fn tick_behavior_trees(world: &mut World) {
    let entities: Vec<Entity> = world
        .query_filtered::<Entity, With<BehaviorTree>>()
        .iter(world)
        .collect();

    for entity in entities {
        let Some(mut tree_component) = world.get_mut::<BehaviorTree>(entity) else {
            continue;
        };
        // pull the root out to avoid borrowing world through the component
        let root = std::mem::replace(
            &mut tree_component.root,
            BtNode::Sequence(vec![]),
        );

        let status = tick_node(root, entity, world, &mut |node, entity, world| {
            tick_node_recursive(node, entity, world)
        });

        // re-borrow to put the root back
        if let Some(mut tree_component) = world.get_mut::<BehaviorTree>(entity) {
            tree_component.last_status = status;
        }
    }
}

fn tick_node_recursive(node: BtNode, entity: Entity, world: &mut World) -> (Status, BtNode) {
    match node {
        BtNode::Selector(mut children) => {
            let mut new_children = Vec::with_capacity(children.len());
            let mut result = Status::Failure;
            let mut short_circuited = false;
            for child in children.drain(..) {
                if short_circuited {
                    new_children.push(child);
                    continue;
                }
                let (status, new_child) = tick_node_recursive(child, entity, world);
                new_children.push(new_child);
                match status {
                    Status::Success | Status::Running => {
                        result = status;
                        short_circuited = true;
                    }
                    Status::Failure => {}
                }
            }
            (result, BtNode::Selector(new_children))
        }
        BtNode::Sequence(mut children) => {
            let mut new_children = Vec::with_capacity(children.len());
            let mut result = Status::Success;
            let mut short_circuited = false;
            for child in children.drain(..) {
                if short_circuited {
                    new_children.push(child);
                    continue;
                }
                let (status, new_child) = tick_node_recursive(child, entity, world);
                new_children.push(new_child);
                match status {
                    Status::Failure | Status::Running => {
                        result = status;
                        short_circuited = true;
                    }
                    Status::Success => {}
                }
            }
            (result, BtNode::Sequence(new_children))
        }
        BtNode::Invert(child) => {
            let (status, new_child) = tick_node_recursive(*child, entity, world);
            let inverted = match status {
                Status::Success => Status::Failure,
                Status::Failure => Status::Success,
                Status::Running => Status::Running,
            };
            (inverted, BtNode::Invert(Box::new(new_child)))
        }
        BtNode::Condition(cond) => {
            let result = if cond.check(entity, world) { Status::Success } else { Status::Failure };
            (result, BtNode::Condition(cond))
        }
        BtNode::Action(mut action) => {
            let status = action.tick(entity, world);
            (status, BtNode::Action(action))
        }
    }
}

fn tick_node(
    node: BtNode,
    entity: Entity,
    world: &mut World,
    recurse: &mut impl FnMut(BtNode, Entity, &mut World) -> (Status, BtNode),
) -> Status {
    let (status, new_root) = recurse(node, entity, world);
    if let Some(mut tree) = world.get_mut::<BehaviorTree>(entity) {
        tree.root = new_root;
    }
    status
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- helpers ---

    struct AlwaysSucceed;
    impl Action for AlwaysSucceed {
        fn tick(&mut self, _entity: Entity, _world: &mut World) -> Status { Status::Success }
    }

    struct AlwaysFail;
    impl Action for AlwaysFail {
        fn tick(&mut self, _entity: Entity, _world: &mut World) -> Status { Status::Failure }
    }

    #[derive(Default)]
    struct CountTicks(u32);
    impl Action for CountTicks {
        fn tick(&mut self, _entity: Entity, _world: &mut World) -> Status {
            self.0 += 1;
            if self.0 >= 3 { Status::Success } else { Status::Running }
        }
    }

    struct TrueCondition;
    impl Condition for TrueCondition {
        fn check(&self, _entity: Entity, _world: &World) -> bool { true }
    }

    struct FalseCondition;
    impl Condition for FalseCondition {
        fn check(&self, _entity: Entity, _world: &World) -> bool { false }
    }

    fn tick_once(world: &mut World) {
        tick_behavior_trees(world);
    }

    // --- tests ---

    #[test]
    fn action_success() {
        let mut world = World::new();
        let entity = world.spawn(BehaviorTree::new(BtNode::action(AlwaysSucceed))).id();
        tick_once(&mut world);
        assert_eq!(world.get::<BehaviorTree>(entity).unwrap().last_status, Status::Success);
    }

    #[test]
    fn action_failure() {
        let mut world = World::new();
        let entity = world.spawn(BehaviorTree::new(BtNode::action(AlwaysFail))).id();
        tick_once(&mut world);
        assert_eq!(world.get::<BehaviorTree>(entity).unwrap().last_status, Status::Failure);
    }

    #[test]
    fn sequence_fails_on_first_failure() {
        let mut world = World::new();
        let tree = BtNode::sequence(vec![
            BtNode::action(AlwaysSucceed),
            BtNode::action(AlwaysFail),
            BtNode::action(AlwaysSucceed),
        ]);
        let entity = world.spawn(BehaviorTree::new(tree)).id();
        tick_once(&mut world);
        assert_eq!(world.get::<BehaviorTree>(entity).unwrap().last_status, Status::Failure);
    }

    #[test]
    fn selector_succeeds_on_first_success() {
        let mut world = World::new();
        let tree = BtNode::selector(vec![
            BtNode::action(AlwaysFail),
            BtNode::action(AlwaysSucceed),
            BtNode::action(AlwaysFail),
        ]);
        let entity = world.spawn(BehaviorTree::new(tree)).id();
        tick_once(&mut world);
        assert_eq!(world.get::<BehaviorTree>(entity).unwrap().last_status, Status::Success);
    }

    #[test]
    fn invert_flips_success() {
        let mut world = World::new();
        let tree = BtNode::invert(BtNode::action(AlwaysSucceed));
        let entity = world.spawn(BehaviorTree::new(tree)).id();
        tick_once(&mut world);
        assert_eq!(world.get::<BehaviorTree>(entity).unwrap().last_status, Status::Failure);
    }

    #[test]
    fn condition_true_returns_success() {
        let mut world = World::new();
        let entity = world.spawn(BehaviorTree::new(BtNode::condition(TrueCondition))).id();
        tick_once(&mut world);
        assert_eq!(world.get::<BehaviorTree>(entity).unwrap().last_status, Status::Success);
    }

    #[test]
    fn condition_false_returns_failure() {
        let mut world = World::new();
        let entity = world.spawn(BehaviorTree::new(BtNode::condition(FalseCondition))).id();
        tick_once(&mut world);
        assert_eq!(world.get::<BehaviorTree>(entity).unwrap().last_status, Status::Failure);
    }

    #[test]
    fn running_action_stays_running_multi_tick() {
        let mut world = World::new();
        let entity = world.spawn(BehaviorTree::new(BtNode::action(CountTicks::default()))).id();
        tick_once(&mut world);
        assert_eq!(world.get::<BehaviorTree>(entity).unwrap().last_status, Status::Running);
        tick_once(&mut world);
        assert_eq!(world.get::<BehaviorTree>(entity).unwrap().last_status, Status::Running);
        tick_once(&mut world);
        assert_eq!(world.get::<BehaviorTree>(entity).unwrap().last_status, Status::Success);
    }
}
