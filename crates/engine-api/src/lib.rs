//! public API for game logic
//!
//! this crate provides the interfaces that game code uses to interact with the engine.
//! game logic operates on handles, never direct references to engine internals.

pub use bevy_ecs;
pub use engine_math;

/// re-export common types for convenience
pub use engine_math::{Color, Mat2, Mat3, Mat4, Rect, Transform, Vec2, Vec3, Vec4};

/// re-export bevy_ecs prelude for convenience
pub use bevy_ecs::prelude::*;

/// marker trait for components that can be used in game logic
pub trait GameComponent: Send + Sync + 'static {}

/// marker trait for resources that can be used in game logic
pub trait GameResource: Send + Sync + 'static {}
