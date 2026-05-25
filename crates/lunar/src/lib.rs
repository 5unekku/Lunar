//! public API for game logic
//!
//! this crate re-exports everything a game project needs from the engine.
//! game code should depend only on `lunar` and use its re-exports.
//!
//! # architecture
//!
//! the engine follows a handle-based design:
//! - assets (textures, sounds, fonts) are accessed through typed `Handle`s from `engine_assets`
//! - game logic registers systems via the `App` builder from `engine_core`
//! - all game state lives in the ECS [`World`], never in global singletons
//!
//! # quick start
//!
//! ```ignore
//! use lunar::prelude::*;
//!
//! fn main() {
//!     let mut app = App::new();
//!     app.add_system(my_system);
//!     app.run(60);
//! }
//!
//! fn my_system(time: Res<Time>) {
//!     // game logic here
//! }
//! ```

// `__bevy_ecs` is the internal path the lunar-macros derives target. It
// MUST keep this exact name — the derive macros emit `::lunar::__bevy_ecs::…`
// paths. Hidden from rustdoc; not part of the public API contract.
#[doc(hidden)]
pub use bevy_ecs as __bevy_ecs;

// wrapped ECS derives — game code writes `#[derive(Component)]` etc. without
// ever naming bevy_ecs in its Cargo.toml. The derives expand to impls routed
// through `::lunar::__bevy_ecs::…`.
pub use lunar_macros::{Component, Event, Message, Resource};

#[cfg(feature = "2d")]
pub use engine_2d;
#[cfg(feature = "3d")]
pub use engine_3d;
pub use engine_assets;
pub use engine_core;
pub use engine_input;
pub use engine_math;
pub use engine_render;

pub mod prelude;
pub use prelude::*;

#[cfg(not(target_arch = "wasm32"))]
mod bootstrap;
#[cfg(not(target_arch = "wasm32"))]
pub use bootstrap::bootstrap;

#[cfg(target_arch = "wasm32")]
mod bootstrap_wasm;
#[cfg(target_arch = "wasm32")]
pub use bootstrap_wasm::bootstrap_wasm;

// types re-exported at crate root for direct access (prelude covers glob imports)
pub use engine_assets::{AssetServer, AudioFormat, Font, Handle, Sound, Texture};
pub use engine_core::{App, GamePlugin, Time, WindowSettings};
pub use engine_input::{ActionMap, InputBinding, InputState, KeyCode, MouseButton};
pub use engine_math::{Color, Mat2, Mat3, Mat4, Quat, Rect, Transform, Vec2, Vec3, Vec4};
pub use engine_render::{
    Camera, Layer, RenderConfig, RenderEngine, RenderInfo, RenderQueue, Sprite, Text, layers,
};

/// marker trait for components that can be used in game logic.
///
/// any type implementing this trait is guaranteed to be [`Send`], [`Sync`],
/// and have a `'static` lifetime, making it safe to use across threads
/// and store in the ECS world indefinitely.
///
/// # example
///
/// ```ignore
/// use lunar::GameComponent;
///
/// struct Player {
///     health: u32,
/// }
///
/// impl GameComponent for Player {}
/// ```
pub trait GameComponent: Send + Sync + 'static {}

/// marker trait for resources that can be used in game logic.
///
/// resources are global state accessible from any system.
/// like [`GameComponent`], they must be [`Send`], [`Sync`], and `'static`.
///
/// # example
///
/// ```ignore
/// use lunar::GameResource;
///
/// struct ScoreTracker {
///     current_score: u32,
/// }
///
/// impl GameResource for ScoreTracker {}
/// ```
pub trait GameResource: Send + Sync + 'static {}
