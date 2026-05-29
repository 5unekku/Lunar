use bevy_ecs::prelude::*;
use lunar_math::Color;

/// configuration for a particle emitter entity.
///
/// attach to any entity that also has a `WorldTransform3d`. the renderer reads this
/// component each frame to spawn and simulate particles. on mid+ tier, simulation runs
/// in a compute shader; on low tier, the CPU simulates and uploads each frame.
#[derive(Component, Clone)]
pub struct ParticleEmitter {
    /// particles to spawn per second.
    pub emission_rate: f32,
    /// how long each particle lives (seconds).
    pub particle_lifetime: f32,
    /// initial speed in the emitter's forward direction.
    pub initial_speed: f32,
    /// half-angle cone spread around the forward axis (radians).
    pub spread_angle: f32,
    /// per-particle colour at birth.
    pub color_start: Color,
    /// per-particle colour at death.
    pub color_end: Color,
    /// billboard size at birth (world units).
    pub size_start: f32,
    /// billboard size at death.
    pub size_end: f32,
    /// downward acceleration (m/s²).
    pub gravity: f32,
    /// maximum simultaneous live particles for this emitter.
    pub max_particles: u32,
    /// whether the emitter is actively spawning new particles.
    pub active: bool,
    /// accumulated fractional particles from the previous frame (internal).
    #[doc(hidden)]
    pub spawn_accumulator: f32,
}

impl Default for ParticleEmitter {
    fn default() -> Self {
        Self {
            emission_rate: 50.0,
            particle_lifetime: 2.0,
            initial_speed: 3.0,
            spread_angle: 0.3,
            color_start: Color::rgba(1.0, 0.8, 0.2, 1.0),
            color_end: Color::rgba(1.0, 0.1, 0.0, 0.0),
            size_start: 0.1,
            size_end: 0.02,
            gravity: 9.8,
            max_particles: 512,
            active: true,
            spawn_accumulator: 0.0,
        }
    }
}
