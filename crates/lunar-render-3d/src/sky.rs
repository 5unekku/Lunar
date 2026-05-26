use bevy_ecs::prelude::*;
use lunar_math::Color;

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
