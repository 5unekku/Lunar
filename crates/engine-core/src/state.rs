//! engine state
//!
//! holds the current state of the engine, always inspectable from outside.
//! the [`EngineState`] resource is updated by the engine to signal
//! lifecycle transitions like pausing or shutting down.

use bevy_ecs::prelude::Resource;

/// engine running state.
///
/// this resource is checked each frame to determine if the game loop
/// should continue running. set to [`Stopping`](EngineState::Stopping)
/// to trigger a graceful shutdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Resource)]
pub enum EngineState {
    /// engine is initializing
    Initializing,
    /// engine is running the game loop
    Running,
    /// engine is paused
    Paused,
    /// engine is shutting down
    Stopping,
}

impl EngineState {
    /// check if the engine is running
    #[must_use]
    pub const fn is_running(&self) -> bool {
        matches!(self, Self::Running)
    }

    /// check if the engine should stop
    #[must_use]
    pub const fn is_stopping(&self) -> bool {
        matches!(self, Self::Stopping)
    }
}
