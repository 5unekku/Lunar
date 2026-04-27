//! system scheduling with stage ordering
//!
//! systems run in registration order by default, but can be grouped into stages
//! for explicit ordering. stages run in a fixed order: Input → Physics → Update → Render.
//!
//! # stage ordering
//!
//! stages ensure that systems run in a predictable order:
//! 1. [`Input`](UpdateStage::Input) — poll input, update input state
//! 2. [`Physics`](UpdateStage::Physics) — collision detection, physics simulation
//! 3. [`Update`](UpdateStage::Update) — general game logic
//! 4. [`Render`](UpdateStage::Render) — queue render commands

/// built-in update stages for system ordering.
///
/// use these to group systems into logical phases of the frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UpdateStage {
    /// poll input, update input state
    Input,
    /// collision detection, physics simulation
    Physics,
    /// general game logic
    Update,
    /// queue render commands
    Render,
}

/// relative stage ordering for custom stage placement.
///
/// allows inserting custom stages before, after, or between built-in stages.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StageOrder {
    /// run before the given stage
    Before(UpdateStage),
    /// run after the given stage
    After(UpdateStage),
    /// run between two stages
    Between(UpdateStage, UpdateStage),
}
