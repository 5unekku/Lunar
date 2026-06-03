use bevy_ecs::prelude::*;
use lunar_math::Color;

/// marks an entity's mesh as water, rendered with Gerstner wave displacement.
///
/// attach to any `Mesh3dBundle`. the renderer swaps in the water pipeline
/// for this entity on mid+ tier. on low tier the entity renders normally
/// without wave displacement.
///
/// the entity mesh should be a flat XZ-plane centered at the origin — the
/// water shader handles the Y-displacement via Gerstner waves.
#[derive(Component, Clone, Copy)]
pub struct Water {
	/// shallow-water colour and overall alpha.
	pub water_color: Color,
	/// deeper-water colour (blended by apparent depth).
	pub deep_color: Color,
	/// Gerstner wave speed multiplier.
	pub wave_speed: f32,
	/// refraction distortion strength (0 = none, 1 = strong).
	pub refract_strength: f32,
	/// Schlick fresnel exponent (5 is typical for water).
	pub fresnel_power: f32,
}

impl Default for Water {
	fn default() -> Self {
		Self {
			water_color: Color::rgba(0.1, 0.4, 0.6, 0.85),
			deep_color: Color::rgba(0.02, 0.1, 0.25, 1.0),
			wave_speed: 1.2,
			refract_strength: 0.5,
			fresnel_power: 5.0,
		}
	}
}
