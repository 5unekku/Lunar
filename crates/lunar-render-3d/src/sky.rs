use bevy_ecs::prelude::*;
use lunar_math::Color;

/// physically-based atmospheric scattering parameters for the sky.
///
/// when inserted as a resource, the renderer replaces the flat-color skydome with
/// a Nishita-style Rayleigh+Mie single-scattering sky. mid+ tier only — on LowGles
/// the renderer falls back to the flat `Sky` sky_color.
///
/// default values approximate Earth's atmosphere.
#[derive(Resource, Clone, Copy)]
pub struct AtmosphericScattering {
    /// rayleigh scattering coefficients per RGB channel (m^-1).
    /// earth: approximately [5.5e-6, 13.0e-6, 22.4e-6]
    pub rayleigh_scatter: [f32; 3],
    /// mie scattering coefficient (m^-1). earth: ~21e-6
    pub mie_scatter: f32,
    /// rayleigh scale height (m). earth: 8500
    pub rayleigh_scale: f32,
    /// mie scale height (m). earth: 1200
    pub mie_scale: f32,
    /// Henyey-Greenstein g factor for Mie (0.76 typical)
    pub mie_anisotropy: f32,
    /// sun irradiance multiplier
    pub sun_intensity: f32,
    /// tone mapping exposure
    pub exposure: f32,
}

impl Default for AtmosphericScattering {
    fn default() -> Self {
        Self {
            rayleigh_scatter: [5.5e-6, 13.0e-6, 22.4e-6],
            mie_scatter: 21.0e-6,
            rayleigh_scale: 8500.0,
            mie_scale: 1200.0,
            mie_anisotropy: 0.76,
            sun_intensity: 22.0,
            exposure: 1.0,
        }
    }
}

/// controls the sky appearance rendered behind all 3d geometry.
///
/// insert this resource (via [`RenderPlugin3d`](crate::RenderPlugin3d)) and set colors before
/// the first frame. the renderer draws a large unlit skydome mesh + an optional sun disc.
///
/// # example
///
/// ```ignore
/// app.insert_resource(Sky {
///     sky_color: Color::rgb(0.4, 0.6, 1.0),
///     sun_color: Color::rgb(1.0, 0.95, 0.8),
///     show_sun: true,
/// });
/// ```
#[derive(Resource, Clone, Copy)]
pub struct Sky {
    /// color of the skydome sphere interior.
    pub sky_color: Color,
    /// color of the sun disc.
    pub sun_color: Color,
    /// half-width (and half-depth) of the sun quad in world units.
    /// at skydome radius 900 a value of 40 gives roughly a 2.5° apparent radius.
    pub sun_half_size: f32,
    /// whether to draw the sun disc.
    pub show_sun: bool,
}

impl Default for Sky {
    fn default() -> Self {
        Self {
            sky_color: Color::rgb(0.4, 0.65, 1.0),
            sun_color: Color::rgb(1.0, 0.98, 0.85),
            sun_half_size: 40.0,
            show_sun: true,
        }
    }
}
