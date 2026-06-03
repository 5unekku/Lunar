//! prelude for lunar-core — re-exports bevy_ecs essentials and lunar-core's
//! own types (app/plugin, scenes, hierarchy, world manifest, etc.).
//!
//! domain crates (`lunar-dialogue`, `lunar-localization`, `lunar-zones`)
//! and subsystem crates (render, input, assets) must be imported separately.
//!
//! # example
//!
//! ```ignore
//! use lunar_core::prelude::*;
//!
//! fn setup(mut commands: Commands) {
//!     commands.spawn((Transform::default(), Player));
//! }
//! ```

// bevy_ecs core types
pub use bevy_ecs::event::Event;
pub use bevy_ecs::message::{MessageReader, MessageWriter, Messages};
pub use bevy_ecs::prelude::*;
pub use bevy_ecs::query::{With, Without};
pub use bevy_ecs::system::Commands;

// lunar-math types
pub use lunar_math::{Color, Mat2, Mat3, Mat4, Rect, Transform, Vec2, Vec3, Vec4};

// lunar-core types
pub use crate::app::{App, GamePlugin, LoopConfig, Time};
pub use crate::engine::Engine;
pub use crate::error::{EngineError, EngineResult, ErrorEvent, ErrorSource};
pub use crate::game_loop::{GameLoop, TickRate};
pub use crate::hierarchy::{Children, Parent};
pub use crate::persist::{self, PersistError};
pub use crate::pool::Pool;
pub use crate::scene::{Scene, SceneManager};
pub use crate::scene_format::{
	EntityDefinition, SceneEntity, SceneInstance, SceneLayer, SceneLoader, SceneSprite, SceneTags,
	SceneText, SpriteDef, TextDef, TransformDef,
};
pub use crate::schedule::UpdateStage;
pub use crate::state::EngineState;
pub use crate::world_manifest::{AdvancedSceneLoader, LoadMode, LoadedScenes, WorldManifest};
