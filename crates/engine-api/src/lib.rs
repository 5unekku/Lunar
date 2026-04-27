//! public API for game logic
//!
//! this crate provides the interfaces that game code uses to interact with the engine.
//! game logic operates on handles, never direct references to engine internals.
//!
//! # architecture
//!
//! the engine follows a handle-based design:
//! - assets (textures, sounds, fonts) are accessed through typed [`Handle`]s
//! - game logic registers systems via the [`App`] builder
//! - all game state lives in the ECS [`World`], never in global singletons
//!
//! # quick start
//!
//! ```ignore
//! use engine_api::prelude::*;
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

pub use bevy_ecs;
pub use engine_math;

/// re-export common types for convenience
pub use engine_math::{Color, Mat2, Mat3, Mat4, Rect, Transform, Vec2, Vec3, Vec4};

/// re-export bevy_ecs prelude for convenience
pub use bevy_ecs::prelude::*;

/// marker trait for components that can be used in game logic.
///
/// any type implementing this trait is guaranteed to be [`Send`], [`Sync`],
/// and have a `'static` lifetime, making it safe to use across threads
/// and store in the ECS world indefinitely.
///
/// # example
///
/// ```ignore
/// use engine_api::GameComponent;
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
/// use engine_api::GameResource;
///
/// struct ScoreTracker {
///     current_score: u32,
/// }
///
/// impl GameResource for ScoreTracker {}
/// ```
pub trait GameResource: Send + Sync + 'static {}
