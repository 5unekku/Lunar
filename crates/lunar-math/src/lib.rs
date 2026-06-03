//! math re-exports and custom utilities
//!
//! this crate wraps [`glam`] for vector/matrix math and provides engine-specific types
//! like [`Transform`], [`Color`], and [`Rect`].
//!
//! # re-exports
//!
//! the full `glam` crate is re-exported for convenience, so you can access
//! any glam types directly via `lunar_math::glam`.

pub use glam;

/// 2D vector type alias.
///
/// backed by [`glam::Vec2`], provides x, y components with SIMD support.
pub type Vec2 = glam::Vec2;

/// 3D vector type alias.
///
/// backed by [`glam::Vec3`]. use for general-purpose 3D math and component storage.
pub type Vec3 = glam::Vec3;

/// 16-byte aligned 3D vector type alias.
///
/// backed by [`glam::Vec3A`]. use in hot-loop math (culling, physics, SoA buffers)
/// where SIMD register fit matters — same cost as Vec3 on most paths but aligns
/// to 16 bytes so glam's SSE2/NEON paths can load it in one instruction.
pub type Vec3A = glam::Vec3A;

/// 4D vector type alias.
///
/// backed by [`glam::Vec4`], useful for packed colors and shader uniforms.
pub type Vec4 = glam::Vec4;

/// 2x2 matrix type alias.
///
/// backed by [`glam::Mat2`], used for 2D rotations.
pub type Mat2 = glam::Mat2;

/// 3x3 matrix type alias.
///
/// backed by [`glam::Mat3`]. useful for 2D affine transforms and normal matrix computation.
pub type Mat3 = glam::Mat3;

/// 4x4 matrix type alias.
///
/// backed by [`glam::Mat4`]. useful for custom projection matrices and 3D transforms.
pub type Mat4 = glam::Mat4;

/// quaternion type alias.
///
/// backed by [`glam::Quat`]. used for 3D rotation in `LocalTransform3d`.
/// quaternions avoid gimbal lock and interpolate cleanly via slerp.
pub type Quat = glam::Quat;

mod macros;
mod screen_rect;
mod types;

pub use screen_rect::ScreenRect;
pub use types::{Color, LocalTransform, Rect, Transform, WorldTransform};
