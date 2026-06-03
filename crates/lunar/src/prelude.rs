//! prelude module — re-exports the most common types for game development.
//!
//! Game code should be able to write `use lunar::prelude::*;` and have
//! everything it needs without further imports.
//!
//! The prelude is the **public contract**. The underlying ECS backend
//! (currently bevy_ecs) is an internal implementation detail and may be
//! swapped without breaking game code that sticks to the prelude.
//!
//! # example
//!
//! ```ignore
//! use lunar::prelude::*;
//!
//! fn setup(mut commands: Commands) {
//!     commands.spawn(Transform::default());
//! }
//!
//! fn move_player(time: Res<Time>, mut query: Query<&mut Transform, With<Player>>) {
//!     for mut transform in &mut query {
//!         transform.translation.y += time.delta_seconds();
//!     }
//! }
//! ```

// ECS system parameters
pub use bevy_ecs::system::{
	Commands, In, IntoSystem, Local, NonSend, NonSendMut, Query, Res, ResMut, Single, System,
};

// ECS world / entity types
pub use bevy_ecs::entity::Entity;
pub use bevy_ecs::world::{EntityMut, EntityRef, EntityWorldMut, FromWorld, World};

// ECS markers — these names cover BOTH the trait (used as a bound, e.g. `T: Component`)
// AND the derive macro (used in `#[derive(Component)]`). bevy_ecs re-exports its
// derive macro at `bevy_ecs::component::Component` alongside the trait, which
// would shadow ours and emit `::bevy_ecs::…` paths. So we route deliberately:
// the derive comes from `lunar-macros` (emits `::lunar::__bevy_ecs::…`),
// and the trait stays accessible through the same identifier because Rust
// resolves trait bounds through the type namespace separately from macros.
//
// Net effect: `#[derive(Component)]` uses our wrapper; `T: Component` still
// resolves via the bevy_ecs trait (re-exported from a path that doesn't bring
// the macro into scope — see crate root).
pub use crate::{Component, Event, Message, Resource};

// ECS query filters
pub use bevy_ecs::query::{Added, AnyOf, Changed, Has, Or, With, Without};

// ECS change detection
pub use bevy_ecs::change_detection::{DetectChanges, DetectChangesMut, Mut, Ref};

// ECS messaging (bevy_ecs 0.18 renamed events → messages for buffered streams)
pub use bevy_ecs::message::{MessageReader, MessageWriter, Messages};

// lunar-math types
pub use lunar_math::{
	Color, Mat2, Mat3, Mat4, Quat, Rect, ScreenRect, Transform, Vec2, Vec3, Vec4,
};

// lunar-core types
pub use lunar_core::pool::Pool;
pub use lunar_core::{
	App, GamePlugin, LoopConfig, TickRate, TickRateConfig, Time, UpdateStage, WindowSettings,
};

// lunar-2d types — only available when the 2d feature is enabled.
// `propagate_transforms` is a Plugin2d-owned system, not surfaced here — reach it
// via `lunar_2d::propagate_transforms` if you order it by hand.
#[cfg(feature = "2d")]
pub use lunar_2d::{
	Collider, Collider2dBundle, ColliderShape, CollisionWorld, Plugin2d, SpriteAnimation,
};

// 3D — only when the 3d feature is enabled. each crate exposes a curated `prelude`
// of common, game-facing types; advanced types stay reachable at the crate root
// (`lunar::lunar_3d::IrradianceSH`, etc.).
#[cfg(feature = "3d")]
pub use lunar_3d::prelude::*;
#[cfg(feature = "3d")]
pub use lunar_bsp::prelude::*;
#[cfg(feature = "3d")]
pub use lunar_lightmap::prelude::*;
#[cfg(feature = "3d")]
pub use lunar_render_3d::prelude::*;

// Bundle derive — needed for game code that defines its own bundles
#[cfg(feature = "3d")]
pub use bevy_ecs::bundle::Bundle;

// lunar-render types
pub use lunar_gamedata::{DataRecord, DataTable, DataValue, GameData};

pub use lunar_render::{
	Camera, CameraFollow2d, ColorTint, PostEffect, PostProcessStack, RenderConfig, RenderEngine,
	RenderInfo, RenderQueue, RenderTargetId, RenderTargetStore, ScreenFlash, ScreenShake, Sprite,
	Text, YSort, layers,
};

// lunar-input types
pub use lunar_input::{
	ActionMap, GamepadAxis, GamepadButton, InputBinding, InputState, KeyCode, MouseButton,
};

// lunar-assets types — Texture/Font/Sound are needed as type parameters for
// Handle<T> when game code stores asset handles in its own resources.
pub use lunar_assets::{
	AssetServer, AudioFormat, Font, Handle, LoadingState, LoadingStats, Sound, Texture,
	TextureSource,
};

// texture! macro — embeds and converts image assets at compile time
pub use crate::texture;

// optional modules — each pulls its curated prelude in when its feature is enabled,
// so `use lunar::prelude::*` lights up exactly the modules the game opted into.
// the full surface of each stays at its module path (`lunar::ui::X`, etc.).
#[cfg(feature = "pathfinding")]
pub use lunar_pathfinding_rt::prelude::*;
#[cfg(feature = "ai")]
pub use lunar_plugin_ai::prelude::*;
#[cfg(feature = "animation")]
pub use lunar_plugin_animation::prelude::*;
#[cfg(feature = "camera-3d")]
pub use lunar_plugin_camera_3d::prelude::*;
#[cfg(feature = "dialogue")]
pub use lunar_plugin_dialogue::prelude::*;
#[cfg(feature = "localization")]
pub use lunar_plugin_localization::prelude::*;
#[cfg(feature = "particles")]
pub use lunar_plugin_particles::prelude::*;
#[cfg(feature = "physics-2d")]
pub use lunar_plugin_physics_2d::prelude::*;
#[cfg(feature = "physics-3d")]
pub use lunar_plugin_physics_3d::prelude::*;
#[cfg(feature = "spline")]
pub use lunar_plugin_spline::prelude::*;
#[cfg(feature = "tilemap")]
pub use lunar_plugin_tilemap::prelude::*;
#[cfg(feature = "timeline")]
pub use lunar_plugin_timeline::prelude::*;
#[cfg(feature = "ui")]
pub use lunar_plugin_ui::prelude::*;
#[cfg(feature = "zones")]
pub use lunar_plugin_zones::prelude::*;

// lunar marker traits
pub use crate::{GameComponent, GameResource};
