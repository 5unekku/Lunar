//! screen shake via trauma²-mapped camera offset.
//!
//! insert a [`ScreenShake`] resource and add trauma to trigger shaking.
//! the system decays trauma each frame and applies a noise-derived offset
//! to the camera position. offset magnitude scales as trauma² so small
//! trauma values produce subtle movement.
//!
//! # example
//!
//! ```ignore
//! use lunar_render::ScreenShake;
//! use lunar_math::Vec2;
//!
//! fn on_explosion(mut shake: ResMut<ScreenShake>) {
//!     shake.add_trauma(0.6); // medium hit
//! }
//!
//! // register once in your setup
//! commands.insert_resource(ScreenShake::new(Vec2::new(20.0, 12.0), 1.5));
//! ```

use bevy_ecs::prelude::*;
use lunar_math::Vec2;

use crate::Camera;

/// camera shake state. insert as a resource to enable shaking.
#[derive(Resource)]
pub struct ScreenShake {
	/// current trauma in [0, 1]. add via [`ScreenShake::add_trauma`]
	pub trauma: f32,
	/// trauma decay per second (e.g. 1.5 clears full trauma in ~0.67s)
	pub decay_rate: f32,
	/// maximum pixel offset at trauma == 1.0
	pub max_offset: Vec2,
	/// accumulated time used to animate the noise function
	elapsed: f32,
}

impl ScreenShake {
	/// create with given max offset and decay rate. trauma starts at zero
	#[must_use]
	pub fn new(max_offset: Vec2, decay_rate: f32) -> Self {
		Self {
			trauma: 0.0,
			decay_rate,
			max_offset,
			elapsed: 0.0,
		}
	}

	/// add trauma, clamped to [0, 1]
	pub fn add_trauma(&mut self, amount: f32) {
		self.trauma = (self.trauma + amount).min(1.0);
	}
}

pub(crate) fn screen_shake_system(
	mut shake: Option<ResMut<ScreenShake>>,
	mut camera: Option<ResMut<Camera>>,
	time: Res<lunar_core::Time>,
) {
	let (Some(shake), Some(camera)) = (shake.as_mut(), camera.as_mut()) else {
		return;
	};
	if shake.trauma <= 0.0 {
		return;
	}

	let delta = time.delta_seconds();
	shake.elapsed += delta;
	shake.trauma = (shake.trauma - shake.decay_rate * delta).max(0.0);

	// trauma² maps [0,1] -> [0,1] with a soft curve: small trauma = subtle shake
	let intensity = shake.trauma * shake.trauma;
	let t = shake.elapsed;

	// two-axis noise from sin harmonics — no extra deps, deterministic
	let noise_x = (t * 13.7).sin() * 0.6 + (t * 29.3).sin() * 0.3 + (t * 53.1).sin() * 0.1;
	let noise_y = (t * 11.3).sin() * 0.6 + (t * 31.7).sin() * 0.3 + (t * 47.9).sin() * 0.1;

	camera.position.x += noise_x * intensity * shake.max_offset.x;
	camera.position.y += noise_y * intensity * shake.max_offset.y;
}

#[cfg(test)]
mod tests {
	use super::*;
	use lunar_core::Time;

	fn run_system(world: &mut World) {
		let mut system = IntoSystem::into_system(screen_shake_system);
		system.initialize(world);
		let _ = system.run((), world);
	}

	#[test]
	fn no_shake_when_trauma_zero() {
		let mut world = World::new();
		world.insert_resource(Camera::new());
		world.insert_resource(Time::default());
		world.insert_resource(ScreenShake::new(Vec2::new(20.0, 20.0), 1.0));

		run_system(&mut world);

		let camera = world.resource::<Camera>();
		assert!((camera.position.x - 0.0).abs() < 0.001);
		assert!((camera.position.y - 0.0).abs() < 0.001);
	}

	#[test]
	fn trauma_decays_each_frame() {
		let mut world = World::new();
		world.insert_resource(Camera::new());

		let mut time = Time::default();
		time.set_delta_seconds(0.1);
		world.insert_resource(time);

		let mut shake = ScreenShake::new(Vec2::new(10.0, 10.0), 2.0);
		shake.add_trauma(1.0);
		world.insert_resource(shake);

		run_system(&mut world);

		let shake = world.resource::<ScreenShake>();
		// 1.0 - 2.0 * 0.1 = 0.8
		assert!((shake.trauma - 0.8).abs() < 0.01);
	}

	#[test]
	fn trauma_clamps_to_one() {
		let mut shake = ScreenShake::new(Vec2::ZERO, 1.0);
		shake.add_trauma(0.7);
		shake.add_trauma(0.7);
		assert!((shake.trauma - 1.0).abs() < 0.001);
	}

	#[test]
	fn trauma_does_not_go_negative() {
		let mut world = World::new();
		world.insert_resource(Camera::new());

		let mut time = Time::default();
		time.set_delta_seconds(10.0); // huge delta
		world.insert_resource(time);

		let mut shake = ScreenShake::new(Vec2::new(10.0, 10.0), 1.0);
		shake.add_trauma(0.1);
		world.insert_resource(shake);

		run_system(&mut world);

		let shake = world.resource::<ScreenShake>();
		assert!(shake.trauma >= 0.0);
	}
}
