use bevy_ecs::prelude::*;
use lunar_math::Color;

/// box-projected decal. attach to an entity with `WorldTransform3d`.
///
/// the entity's transform defines the decal box in world space:
/// scale controls the coverage area, rotation controls projection direction.
/// the decal is projected down the local -Y axis onto any geometry inside the box.
#[derive(Component, Clone, Copy)]
pub struct Decal {
    /// tint colour and alpha. alpha controls decal opacity.
    pub color: Color,
}

impl Default for Decal {
    fn default() -> Self {
        Self { color: Color::rgba(0.2, 0.1, 0.05, 0.8) }
    }
}
