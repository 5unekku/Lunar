//! system scheduling with stage ordering
//!
//! systems run in registration order by default, but can be grouped into stages
//! for explicit ordering. stages run in a fixed order:
//! Input → Physics → Update → Render → PostUpdate.
//!
//! # stage ordering
//!
//! stages ensure that systems run in a predictable order:
//! 1. [`Input`](UpdateStage::Input) — poll input, update input state
//! 2. [`Physics`](UpdateStage::Physics) — collision detection, physics simulation
//! 3. [`Update`](UpdateStage::Update) — general game logic
//! 4. [`Render`](UpdateStage::Render) — queue render commands
//! 5. [`PostUpdate`](UpdateStage::PostUpdate) — end-of-tick cleanup (e.g. clearing edge-triggered input)

use bevy_ecs::schedule::ScheduleLabel;

/// built-in update stages for system ordering.
///
/// use these to group systems into logical phases of the frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, ScheduleLabel)]
pub enum UpdateStage {
	/// poll input, update input state
	Input,
	/// collision detection, physics simulation
	Physics,
	/// general game logic
	Update,
	/// queue render commands
	Render,
	/// end-of-tick cleanup — runs after Render
	PostUpdate,
}

/// relative stage ordering for custom stage placement.
///
/// reserved for future custom-stage support (needs the `bevy_ecs` schedule graph).
/// not part of the public API yet — kept internal until the feature lands.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StageOrder {
	/// run before the given stage
	Before(UpdateStage),
	/// run after the given stage
	After(UpdateStage),
	/// run between two stages
	Between(UpdateStage, UpdateStage),
}

/// trait for custom stage labels.
///
/// implement this to define custom stages that can be ordered
/// relative to the built-in [`UpdateStage`] variants.
#[allow(dead_code)]
pub trait StageLabelExt: ScheduleLabel {
	/// get the ordering relative to built-in stages
	fn stage_order(&self) -> StageOrder;
}
