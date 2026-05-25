use bevy_ecs::prelude::Component;
use engine_math::Color;

/// directional light — infinite distance, uniform direction across the scene.
///
/// direction is taken from the entity's [`WorldTransform3d`](crate::transform::WorldTransform3d)
/// forward vector, so rotate the entity to aim the light. equivalent to a sun.
///
/// # example
///
/// ```ignore
/// commands.spawn((
///     LocalTransform3d::default()
///         .with_rotation(Quat::from_euler(EulerRot::XYZ, -0.8, 0.4, 0.0)),
///     WorldTransform3d::default(),
///     DirectionalLight {
///         color: Color::WHITE,
///         illuminance: 80_000.0, // roughly outdoor sunlight in lux
///     },
/// ));
/// ```
#[derive(Debug, Clone, Copy, Component)]
pub struct DirectionalLight {
    pub color: Color,
    /// light strength in lux. 80_000 ≈ full sun, 1_000 ≈ overcast.
    pub illuminance: f32,
}

impl Default for DirectionalLight {
    fn default() -> Self {
        Self {
            color: Color::WHITE,
            illuminance: 80_000.0,
        }
    }
}

/// point light — emits uniformly in all directions from the entity's position.
///
/// position is taken from the entity's [`WorldTransform3d`](crate::transform::WorldTransform3d)
/// translation. attenuation follows inverse-square law up to `radius`.
#[derive(Debug, Clone, Copy, Component)]
pub struct PointLight {
    pub color: Color,
    /// luminous intensity in candela.
    pub intensity: f32,
    /// world-space radius at which intensity falls to zero (hard cutoff for culling).
    pub radius: f32,
    /// whether this light casts shadows.
    pub casts_shadows: bool,
}

impl Default for PointLight {
    fn default() -> Self {
        Self {
            color: Color::WHITE,
            intensity: 800.0,
            radius: 20.0,
            casts_shadows: false,
        }
    }
}

/// spot light — cone of light from the entity's position in its forward direction.
///
/// the Doom 3 / id Tech 4 lighting model uses these extensively for environment
/// lighting. inner_angle is fully lit; from inner to outer it falls off smoothly.
#[derive(Debug, Clone, Copy, Component)]
pub struct SpotLight {
    pub color: Color,
    /// luminous intensity in candela.
    pub intensity: f32,
    /// world-space range.
    pub radius: f32,
    /// inner (full brightness) cone half-angle in radians.
    pub inner_angle: f32,
    /// outer (zero brightness) cone half-angle in radians.
    pub outer_angle: f32,
    pub casts_shadows: bool,
}

impl Default for SpotLight {
    fn default() -> Self {
        Self {
            color: Color::WHITE,
            intensity: 800.0,
            radius: 20.0,
            inner_angle: std::f32::consts::FRAC_PI_8,
            outer_angle: std::f32::consts::FRAC_PI_4,
            casts_shadows: false,
        }
    }
}
