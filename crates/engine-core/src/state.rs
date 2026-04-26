//! engine state
//!
//! holds the current state of the engine, always inspectable from outside.

use bevy_ecs::prelude::Resource;

/// engine running state
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
    pub fn is_running(&self) -> bool {
        matches!(self, EngineState::Running)
    }

    /// check if the engine should stop
    pub fn is_stopping(&self) -> bool {
        matches!(self, EngineState::Stopping)
    }
}
