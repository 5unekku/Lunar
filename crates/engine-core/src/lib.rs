//! engine core: game loop, ECS wiring, engine state
//!
//! this crate owns the main game loop and coordinates all subsystems.
//! game logic operates on handles, never direct references.
//!
//! # architecture
//!
//! the core crate ties together all engine subsystems:
//! - [`App`] provides a fluent builder for configuring the engine
//! - [`Engine`] owns the ECS world and schedule
//! - [`GameLoop`] manages fixed-timestep updates and frame capping
//! - plugins ([`GamePlugin`]) register systems and resources in dependency order
//!
//! # getting started
//!
//! ```ignore
//! use engine_core::App;
//!
//! fn main() {
//!     App::new()
//!         .add_system(my_system)
//!         .run(60);
//! }
//!
//! fn my_system(time: Res<Time>) {
//!     println!("delta: {}s", time.delta_seconds());
//! }
//! ```

/// re-export bevy_ecs for direct ECS access
pub use bevy_ecs;
/// re-export engine-api for game logic interfaces
pub use engine_api;
/// re-export engine-math for math types
pub use engine_math;

mod app;
mod command;
mod dialogue;
mod dialogue_parser;
mod engine;
mod error;
mod game_loop;
mod localization;
mod scene;
mod schedule;
mod state;
mod zone;

/// app builder and time resource
pub use app::{App, GamePlugin, Time};
/// command registry for console commands
pub use command::{Command, CommandRegistry};
/// dialogue system types and manager
pub use dialogue::{
    Dialogue, DialogueBuilder, DialogueChoice, DialogueLine, DialogueManager, DialogueNode,
    DialoguePlugin, DialogueState,
};
/// dialogue yaml parser
pub use dialogue_parser::{parse_dialogue, parse_dialogue_file};
/// engine wrapper around bevy_ecs world and schedule
pub use engine::Engine;
/// error handling types
pub use error::{EngineError, EngineResult, ErrorEvent, ErrorSource};
/// game loop with fixed timestep
pub use game_loop::{GameLoop, TickRate};
/// localization system
pub use localization::{Localization, LocalizationPlugin};
/// scene system for game state management
pub use scene::{Scene, SceneManager};
/// system scheduling with stage ordering
pub use schedule::{StageLabelExt, StageOrder, UpdateStage};
/// engine running state
pub use state::EngineState;
/// world zone management
pub use zone::{FadeConfig, WorldManager, Zone, ZoneTransition};
