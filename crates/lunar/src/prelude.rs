//! prelude module — re-exports the most common types for game development.
//!
//! users should be able to write `use lunar::prelude::*;` and have
//! everything they need without any further imports.
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

// bevy_ecs core types
pub use bevy_ecs::event::Event;
pub use bevy_ecs::message::{MessageReader, MessageWriter, Messages};
pub use bevy_ecs::prelude::*;
pub use bevy_ecs::query::{With, Without};
pub use bevy_ecs::system::Commands;

// engine-math types
pub use engine_math::{Color, Mat2, Mat3, Mat4, Rect, Transform, Vec2, Vec3, Vec4};

// engine-core types
pub use engine_core::{App, GamePlugin, Time, WindowSettings};

// engine-render types
pub use engine_render::{Camera, RenderConfig, RenderEngine, RenderInfo, RenderQueue};

// engine-input types
pub use engine_input::{ActionMap, InputState, KeyCode, MouseButton};

// engine-assets types
pub use engine_assets::{AssetServer, Handle};

// lunar marker traits
pub use crate::{GameComponent, GameResource};
