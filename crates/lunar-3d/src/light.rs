use bevy_ecs::prelude::Component;
use lunar_math::Color;

/// directional light — infinite distance, uniform direction across the scene.
///
/// direction is taken from the entity's [`WorldTransform3d`](crate::transform::WorldTransform3d)
/// forward vector. equivalent to a sun or moon — no falloff, no position.
#[derive(Debug, Clone, Copy, Component)]
pub struct DirectionalLight {
    pub color: Color,
    /// light strength in lux. 80_000 ≈ full sun, 1_000 ≈ overcast, 100 ≈ indoor.
    pub illuminance: f32,
    pub casts_shadows: bool,
}

impl Default for DirectionalLight {
    fn default() -> Self {
        Self {
            color: Color::WHITE,
            illuminance: 80_000.0,
            casts_shadows: false,
        }
    }
}

/// point light — emits uniformly in all directions from the entity's world position.
///
/// # attenuation
///
/// `radius` defines both the falloff range and the culling volume — the render
/// system skips any surface whose AABB does not intersect the light sphere. keep
/// it as tight as possible to minimize lit surface count.
///
/// the WGSL shader should use a physically-motivated formula. recommended (Frostbite):
///
/// ```text
/// window     = clamp(1.0 - (distance / radius)^4, 0.0, 1.0)^2
/// attenuation = window / (distance^2 + 1.0)
/// ```
///
/// the `+ 1.0` prevents the singularity at d = 0; `window` provides a smooth
/// hard cutoff at `radius` without an abrupt cliff. this is physically based
/// (inverse-square in the falloff region) and well-behaved at the origin.
#[derive(Debug, Clone, Copy, Component)]
pub struct PointLight {
    pub color: Color,
    /// luminous intensity in candela. combined with attenuation for final contribution.
    pub intensity: f32,
    /// world-space radius. light reaches zero at this distance (hard culling boundary).
    pub radius: f32,
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
/// uses the same radial attenuation formula as [`PointLight`] plus an angular
/// falloff between `inner_angle` and `outer_angle` (analogous to the Doom 3
/// light projection texture approach, computed analytically here).
///
/// angular attenuation in the shader:
/// ```text
/// cos_inner = cos(inner_angle)
/// cos_outer = cos(outer_angle)
/// cos_theta = dot(normalize(fragment_to_light), spot_direction)
/// spot_factor = clamp((cos_theta - cos_outer) / (cos_inner - cos_outer), 0.0, 1.0)
/// ```
#[derive(Debug, Clone, Copy, Component)]
pub struct SpotLight {
    pub color: Color,
    pub intensity: f32,
    pub radius: f32,
    /// inner cone half-angle in radians — fully lit inside this cone.
    pub inner_angle: f32,
    /// outer cone half-angle in radians — no light outside this cone.
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
