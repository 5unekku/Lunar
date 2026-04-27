//! math re-exports and custom utilities
//!
//! this crate wraps [`glam`] for vector/matrix math and provides engine-specific types
//! like [`Transform`], [`Color`], and [`Rect`].
//!
//! # re-exports
//!
//! the full `glam` crate is re-exported for convenience, so you can access
//! any glam types directly via `engine_math::glam`.

pub use glam;

/// 2D vector type alias.
///
/// backed by [`glam::Vec2`], provides x, y components with SIMD support.
pub type Vec2 = glam::Vec2;

/// 3D vector type alias.
///
/// backed by [`glam::Vec3`], provided for future 3D expansion.
pub type Vec3 = glam::Vec3;

/// 4D vector type alias.
///
/// backed by [`glam::Vec4`], useful for colors and quaternions.
pub type Vec4 = glam::Vec4;

/// 2x2 matrix type alias.
///
/// backed by [`glam::Mat2`], used for 2D rotations.
pub type Mat2 = glam::Mat2;

/// 3x3 matrix type alias.
///
/// backed by [`glam::Mat3`], provided for future 3D expansion.
pub type Mat3 = glam::Mat3;

/// 4x4 matrix type alias.
///
/// backed by [`glam::Mat4`], used for 3D transformations and projections.
pub type Mat4 = glam::Mat4;

mod macros;
mod types;

pub use types::{Color, Rect, Transform};
