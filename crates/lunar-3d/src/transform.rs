use bevy_ecs::prelude::Component;
use lunar_math::{Mat4, Quat, Vec3};

/// local 3D transform: position, rotation, and scale relative to the parent entity.
///
/// when an entity has no parent, this is equivalent to world space.
/// rotation uses a quaternion — no gimbal lock, clean slerp interpolation.
///
/// # builder pattern
///
/// ```ignore
/// let t = LocalTransform3d::from_xyz(0.0, 1.0, -5.0)
///     .with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2))
///     .with_scale(Vec3::splat(2.0));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Component)]
pub struct LocalTransform3d {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl LocalTransform3d {
    /// create from an xyz translation, identity rotation, and uniform scale 1.
    #[must_use]
    pub const fn from_xyz(x: f32, y: f32, z: f32) -> Self {
        Self {
            translation: Vec3::new(x, y, z),
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }

    /// set rotation, preserving translation and scale.
    #[must_use]
    pub const fn with_rotation(mut self, rotation: Quat) -> Self {
        self.rotation = rotation;
        self
    }

    /// set scale, preserving translation and rotation.
    #[must_use]
    pub const fn with_scale(mut self, scale: Vec3) -> Self {
        self.scale = scale;
        self
    }

    /// compute the TRS matrix (scale → rotate → translate).
    #[must_use]
    pub fn to_matrix(self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }
}

impl Default for LocalTransform3d {
    fn default() -> Self {
        Self {
            translation: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }
}

/// world 3D transform: absolute position, rotation, and scale in world space.
///
/// computed automatically by [`crate::propagate_transforms_3d`] from [`LocalTransform3d`]
/// and the parent hierarchy. do not modify this component directly.
#[derive(Debug, Clone, Copy, PartialEq, Component)]
pub struct WorldTransform3d {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl WorldTransform3d {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            translation: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }

    /// compute the TRS matrix for this world transform.
    #[must_use]
    pub fn to_matrix(self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }

    /// the forward vector (−Z in local space, rotated to world).
    #[must_use]
    pub fn forward(self) -> Vec3 {
        self.rotation * Vec3::NEG_Z
    }

    /// the right vector (+X in local space, rotated to world).
    #[must_use]
    pub fn right(self) -> Vec3 {
        self.rotation * Vec3::X
    }

    /// the up vector (+Y in local space, rotated to world).
    #[must_use]
    pub fn up(self) -> Vec3 {
        self.rotation * Vec3::Y
    }

    /// linearly interpolate toward `other` by `alpha` (0 = self, 1 = other).
    /// rotation is slerped, position and scale are lerped.
    #[must_use]
    pub fn lerp(self, other: &Self, alpha: f32) -> Self {
        Self {
            translation: self.translation.lerp(other.translation, alpha),
            rotation:    self.rotation.slerp(other.rotation, alpha),
            scale:       self.scale.lerp(other.scale, alpha),
        }
    }
}

impl Default for WorldTransform3d {
    fn default() -> Self {
        Self::new()
    }
}

impl From<LocalTransform3d> for WorldTransform3d {
    fn from(local: LocalTransform3d) -> Self {
        Self {
            translation: local.translation,
            rotation: local.rotation,
            scale: local.scale,
        }
    }
}
