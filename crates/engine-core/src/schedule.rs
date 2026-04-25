//! system scheduling with stage ordering
//!
//! systems run in registration order by default, but can be grouped into stages
//! for explicit ordering. stages run in a fixed order: Input → Physics → Update → Render.

/// built-in update stages for system ordering
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

/// relative stage ordering for custom stage placement
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StageOrder {
    /// run before the given stage
    Before(UpdateStage),
    /// run after the given stage
    After(UpdateStage),
    /// run between two stages
    Between(UpdateStage, UpdateStage),
}
