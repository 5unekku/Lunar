//! engine core: game loop, ECS wiring, engine state
//!
//! this crate owns the main game loop and coordinates all subsystems.
//! game logic operates on handles, never direct references.

pub use bevy_ecs;
pub use engine_api;
pub use engine_math;

mod app;
mod command;
mod engine;
mod game_loop;
mod schedule;
mod state;

pub use app::{App, GamePlugin, Time};
pub use command::{Command, CommandRegistry};
pub use engine::Engine;
pub use game_loop::{GameLoop, TickRate};
pub use schedule::{StageOrder, UpdateStage};
pub use state::EngineState;
