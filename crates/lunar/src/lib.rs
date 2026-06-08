//! public API for game logic
//!
//! this crate re-exports everything a game project needs from the engine.
//! game code should depend only on `lunar` and use its re-exports.
//!
//! # quick start
//!
//! implement [`GamePlugin`] to define your game, then hand it to a bootstrap function:
//!
//! ```ignore
//! use lunar::prelude::*;
//!
//! #[derive(Default)]
//! struct MyGame;
//!
//! impl GamePlugin for MyGame {
//!     fn name(&self) -> &str { "MyGame" }
//!     fn build(&mut self, app: &mut App) {
//!         app.add_startup_system(setup);
//!         app.add_system(update);
//!     }
//! }
//!
//! fn setup(mut commands: Commands) {
//!     commands.spawn(Transform::from_xy(0.0, 0.0));
//! }
//!
//! fn update(time: Res<Time>) {
//!     // runs every tick
//! }
//!
//! // generates a native main() that calls bootstrap::<MyGame>()
//! lunar_app!(MyGame);
//! ```
//!
//! for a 3d game, use [`bootstrap_3d`] instead:
//!
//! ```ignore
//! fn main() {
//!     lunar::bootstrap_3d::<MyGame>(lunar::lunar_render_3d::RenderConfig3d::default());
//! }
//! ```
//!
//! # features
//!
//! | feature | what it enables |
//! |---------|----------------|
//! | `2d` *(default)* | 2d sprite/text rendering, 2d collision |
//! | `3d` | clustered-forward PBR renderer, BSP/PVS culling, lightmaps |
//! | `full` | `2d` + `3d` |
//!
//! audio, navigation, pathfinding, physics, particles, UI, and other opt-in
//! functionality live in the
//! [`lunar-plugins`](https://gitlab.com/5unekku/lunar-plugins) workspace.
//!
//! # architecture
//!
//! the engine follows a handle-based design:
//! - assets (textures, sounds, fonts) are accessed through typed `Handle`s from `lunar_assets`
//! - game logic registers systems via the `App` builder from `lunar_core`
//! - all game state lives in the ECS [`World`], never in global singletons
//! - the ECS backend (bevy_ecs) is sealed behind this crate's prelude — game code never names it

// `__bevy_ecs` is the internal path the lunar-macros derives target. It
// MUST keep this exact name — the derive macros emit `::lunar::__bevy_ecs::…`
// paths. Hidden from rustdoc; not part of the public API contract.
#[doc(hidden)]
pub use bevy_ecs as __bevy_ecs;

// wrapped ECS derives — game code writes `#[derive(Component)]` etc. without
// ever naming bevy_ecs in its Cargo.toml. The derives expand to impls routed
// through `::lunar::__bevy_ecs::…`.
pub use lunar_macros::{Component, Event, Message, Resource, texture};

#[cfg(feature = "2d")]
pub use lunar_2d;
#[cfg(feature = "3d")]
pub use lunar_3d;
pub use lunar_assets;
pub use lunar_core;
pub use lunar_input;
pub use lunar_math;
pub use lunar_render;
#[cfg(feature = "3d")]
pub use lunar_render_3d;

pub mod prelude;
pub use prelude::*;

#[cfg(not(target_arch = "wasm32"))]
mod bootstrap;
#[cfg(not(target_arch = "wasm32"))]
pub use bootstrap::bootstrap;

// `lunar_app!` lives here; it expands to a native `main` that calls `bootstrap`.
// `#[macro_export]` hoists the macro to the crate root once the module compiles.
#[cfg(not(target_arch = "wasm32"))]
mod app_macro;

// shared per-frame window reconciliation, reusable by custom native loops.
#[cfg(not(target_arch = "wasm32"))]
mod window_host;
#[cfg(not(target_arch = "wasm32"))]
pub use window_host::WindowHost;

#[cfg(all(not(target_arch = "wasm32"), feature = "3d"))]
mod bootstrap_3d;
#[cfg(all(not(target_arch = "wasm32"), feature = "3d"))]
pub use bootstrap_3d::bootstrap_3d;

#[cfg(target_arch = "wasm32")]
mod bootstrap_wasm;
#[cfg(target_arch = "wasm32")]
pub use bootstrap_wasm::bootstrap_wasm;

#[cfg(all(target_arch = "wasm32", feature = "3d"))]
mod bootstrap_wasm_3d;
#[cfg(all(target_arch = "wasm32", feature = "3d"))]
pub use bootstrap_wasm_3d::bootstrap_wasm_3d;

// types re-exported at crate root for direct access (prelude covers glob imports)
pub use lunar_assets::{AssetServer, AudioFormat, Font, Handle, Sound, Texture};
pub use lunar_core::{App, GamePlugin, Time, WindowSettings};
pub use lunar_input::{ActionMap, InputBinding, InputState, KeyCode, MouseButton};
pub use lunar_math::{
	Color, Mat2, Mat3, Mat4, Quat, Rect, ScreenRect, Transform, Vec2, Vec3, Vec4,
};
pub use lunar_render::{
	Camera, RenderConfig, RenderEngine, RenderInfo, RenderQueue, Sprite, Text, layers,
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
