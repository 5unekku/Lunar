/// math re-exports and custom utilities
pub use glam;

/// 2D vector type alias
pub type Vec2 = glam::Vec2;

/// 3D vector type alias (for future expansion)
pub type Vec3 = glam::Vec3;

/// 4D vector type alias (for future expansion)
pub type Vec4 = glam::Vec4;

/// 2D matrix type alias
pub type Mat2 = glam::Mat2;

/// 3D matrix type alias (for future expansion)
pub type Mat3 = glam::Mat3;

/// 4D matrix type alias (for future expansion)
pub type Mat4 = glam::Mat4;

mod types;

pub use types::{Color, Rect, Transform};
