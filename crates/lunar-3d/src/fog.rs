use bevy_ecs::prelude::Resource;
use lunar_math::Color;

/// scene-wide fog settings.
///
/// insert as a resource to enable scene fog. without this resource, no fog is applied.
///
/// # example
///
/// ```ignore
/// app.insert_resource(Fog::linear(
///     Color::new(0.6, 0.7, 0.8, 1.0),
///     50.0,
///     300.0,
/// ));
/// ```
#[derive(Debug, Clone, Resource)]
pub struct Fog {
	/// color blended into fragment output. alpha is ignored — fog always blends at full strength
	/// as determined by the falloff factor.
	pub color: Color,
	pub falloff: FogFalloff,
}

/// controls how fog density increases with distance from the camera.
#[derive(Debug, Clone, Copy)]
pub enum FogFalloff {
	/// linearly increases from 0 at `start` to 1 at `end`.
	/// `factor = clamp((d - start) / (end - start), 0, 1)`
	Linear {
		/// distance at which fog begins (factor = 0).
		start: f32,
		/// distance at which fog reaches full density (factor = 1).
		end: f32,
	},
	/// exponential falloff. less dramatic than squared, suitable for light haze.
	/// `factor = 1 - exp(-density * d)`
	Exponential {
		/// controls how quickly fog reaches full density. 0.01 is light haze, 0.1 is thick.
		density: f32,
	},
	/// exponential-squared falloff. realistic ground fog and thick atmospheric haze.
	/// `factor = 1 - exp(-(density * d)^2)`
	ExponentialSquared {
		/// same interpretation as `Exponential::density` but produces a sharper onset.
		density: f32,
	},
}

impl Fog {
	/// linear fog between two distances.
	#[must_use]
	pub fn linear(color: Color, start: f32, end: f32) -> Self {
		Self {
			color,
			falloff: FogFalloff::Linear { start, end },
		}
	}

	/// exponential fog.
	#[must_use]
	pub fn exponential(color: Color, density: f32) -> Self {
		Self {
			color,
			falloff: FogFalloff::Exponential { density },
		}
	}

	/// exponential-squared fog.
	#[must_use]
	pub fn exponential_squared(color: Color, density: f32) -> Self {
		Self {
			color,
			falloff: FogFalloff::ExponentialSquared { density },
		}
	}

	/// compute the fog factor (0 = no fog, 1 = fully fogged) at a given distance.
	///
	/// the render backend calls this per-fragment (using WGSL equivalents), but
	/// this method is useful for CPU-side debug or LOD calculations.
	#[must_use]
	pub fn factor(&self, distance: f32) -> f32 {
		match self.falloff {
			FogFalloff::Linear { start, end } => {
				if end <= start {
					return if distance >= end { 1.0 } else { 0.0 };
				}
				((distance - start) / (end - start)).clamp(0.0, 1.0)
			}
			FogFalloff::Exponential { density } => 1.0 - (-density * distance).exp(),
			FogFalloff::ExponentialSquared { density } => {
				let t = density * distance;
				1.0 - (-(t * t)).exp()
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn linear_fog_factor() {
		let fog = Fog::linear(Color::WHITE, 10.0, 100.0);
		assert!((fog.factor(10.0) - 0.0).abs() < 0.001);
		assert!((fog.factor(55.0) - 0.5).abs() < 0.001);
		assert!((fog.factor(100.0) - 1.0).abs() < 0.001);
		assert!((fog.factor(200.0) - 1.0).abs() < 0.001);
	}

	#[test]
	fn exponential_fog_zero_distance() {
		let fog = Fog::exponential(Color::WHITE, 0.1);
		assert!((fog.factor(0.0) - 0.0).abs() < 0.001);
	}

	#[test]
	fn exponential_squared_increases_with_distance() {
		let fog = Fog::exponential_squared(Color::WHITE, 0.05);
		assert!(fog.factor(10.0) < fog.factor(20.0));
		assert!(fog.factor(50.0) < fog.factor(100.0));
	}
}
