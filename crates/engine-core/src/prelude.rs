//! full prelude for game development — re-exports everything from bevy_ecs
//! plus all engine-core types (scenes, zones, dialogue, localization, etc.).
//!
//! for subsystem types (render, input, assets), import those crates directly.
//!
//! # example
//!
//! ```ignore
//! use engine_core::prelude::*;
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

// engine-math types
pub use engine_math::{Color, Mat2, Mat3, Mat4, Rect, Transform, Vec2, Vec3, Vec4};

// engine-core types
pub use crate::app::{App, GamePlugin, Time};
pub use crate::dialogue::{
    Dialogue, DialogueBuilder, DialogueChoice, DialogueLine, DialogueManager,
};
pub use crate::engine::Engine;
pub use crate::error::{EngineError, EngineResult, ErrorEvent, ErrorSource};
pub use crate::game_loop::{GameLoop, TickRate};
pub use crate::hierarchy::{Children, Parent};
pub use crate::localization::Localization;
pub use crate::scene::{Scene, SceneManager};
pub use crate::scene_format::{
    EntityDefinition, SceneEntity, SceneInstance, SceneLayer, SceneLoader, SceneSprite, SceneTags,
    SceneText, SpriteDef, TextDef, TransformDef,
};
pub use crate::schedule::{StageLabelExt, StageOrder, UpdateStage};
pub use crate::state::EngineState;
pub use crate::world_manifest::{AdvancedSceneLoader, LoadMode, LoadedScenes, WorldManifest};
pub use crate::zone::{FadeConfig, WorldManager, Zone, ZoneTransition};
