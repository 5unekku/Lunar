//! full prelude for game development — re-exports everything from engine-api
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

// everything from engine-api prelude
pub use engine_api::prelude::*;

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
