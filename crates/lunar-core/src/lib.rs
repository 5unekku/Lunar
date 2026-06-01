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
//! use lunar_core::App;
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

/// re-export `bevy_ecs` for direct ECS access
pub use bevy_ecs;
/// re-export lunar-math for math types
pub use lunar_math;

mod app;
mod command;
mod engine;
mod error;
mod game_loop;
mod hierarchy;
pub mod persist;
pub mod pool;
mod scene;
mod scene_format;
mod schedule;
mod state;
mod window;
mod world_manifest;

/// app builder and time resource
pub use app::{App, GamePlugin, LoopConfig, Time, TickRateConfig};
/// command registry for console commands
pub use command::{Command, CommandRegistry};
/// engine wrapper around `bevy_ecs` world and schedule
pub use engine::Engine;
/// error handling types
pub use error::{EngineError, EngineResult, ErrorEvent, ErrorSource};
/// game loop with fixed timestep
pub use game_loop::{GameLoop, TickRate};
/// entity hierarchy components and parent/child sync
pub use hierarchy::{Children, HierarchyPlugin, Parent, PostUpdate, sync_children};
/// scene system for game state management
pub use scene::{Scene, SceneManager};
/// scene definition format: RON authoring and binary runtime
pub use scene_format::{
    EntityDefinition, SceneData, SceneDefinition, SceneEntity, SceneInstance, SceneLayer,
    SceneLoader, SceneSprite, SceneTags, SceneText, SpriteDef, TextDef, TransformDef,
};
/// system scheduling: the built-in update stages
pub use schedule::UpdateStage;
/// engine running state
pub use state::EngineState;
/// world manifest: XML-based world definition with scenes and spatial chunks.
/// authoring + runtime types a game uses directly.
pub use world_manifest::{
    AdvancedSceneLoader, ChunkEntry, ComponentScene, EntityData, LoadMode, LoadedScenes,
    SceneEntry, StreamingConfig, StreamingState, WorldManifest,
};
/// compiled/interned manifest internals — reachable for tooling, but not part of
/// the game-facing contract (the manifest pipeline produces these; games don't author them).
#[doc(hidden)]
pub use world_manifest::{
    CompiledChunkEntry, CompiledSceneEntry, CompiledWorld, StringInterner, builtin_components,
};

/// window state resource, display resolution helpers, and available-resolutions resource
pub use window::{
    WindowSettings, DisplayResolution, AvailableResolutions,
    STANDARD_RESOLUTIONS, resolutions_for_aspect,
};

/// full prelude for game development
pub mod prelude;
