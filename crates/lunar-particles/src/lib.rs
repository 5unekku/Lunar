//! particle emitters for 2d games.
//!
//! attach [`ParticleEmitter`] to any entity to emit particles. the
//! [`ParticlePlugin`] drives the simulation: it spawns, ticks, and culls
//! particles each frame, then pushes sprite draws to the `RenderQueue`.
//!
//! # example
//!
//! ```ignore
//! use lunar_particles::{ParticleEmitter, ParticlePlugin};
//! use lunar_math::{Vec2, Color};
//!
//! // register the plugin
//! app.add_plugin(ParticlePlugin);
//!
//! // attach an emitter to an entity
//! commands.spawn((
//!     Transform::from_xy(400.0, 300.0),
//!     ParticleEmitter {
//!         texture: fire_texture,
//!         rate: 30.0,          // particles per second
//!         lifetime: 1.5,
//!         speed: 80.0,
//!         spread: 0.6,         // radians half-angle
//!         direction: Vec2::new(0.0, -1.0), // upward
//!         color_start: Color::WHITE,
//!         color_end: Color::rgba(1.0, 0.5, 0.0, 0.0),
//!         size_start: Vec2::splat(8.0),
//!         size_end: Vec2::splat(2.0),
//!         layer: 10,
//!         active: true,
//!     },
//! ));
//! ```

use bevy_ecs::prelude::*;
use lunar_assets::{Handle, Texture};
use lunar_core::{App, GamePlugin};
use lunar_math::{Color, Transform, Vec2};
use lunar_render::{DrawCommand, DrawKind, RenderQueue};

/// configuration for a particle emitter. attach alongside `Transform`.
#[derive(Component, Clone)]
pub struct ParticleEmitter {
    /// texture drawn for each particle
    pub texture: Handle<Texture>,
    /// particles spawned per second
    pub rate: f32,
    /// particle lifetime in seconds
    pub lifetime: f32,
    /// initial particle speed in world units per second
    pub speed: f32,
    /// emission half-angle in radians (0 = single direction, PI = all directions)
    pub spread: f32,
    /// normalized emission direction
    pub direction: Vec2,
    /// particle color at spawn
    pub color_start: Color,
    /// particle color at end of life
    pub color_end: Color,
    /// particle size at spawn (world units)
    pub size_start: Vec2,
    /// particle size at end of life (world units)
    pub size_end: Vec2,
    /// render layer
    pub layer: i32,
    /// pause emission without removing the component
    pub active: bool,
    /// accumulated fractional spawn counter (managed by the system)
    #[doc(hidden)]
    pub spawn_accumulator: f32,
}

impl Default for ParticleEmitter {
    fn default() -> Self {
        Self {
            texture: Handle::default(),
            rate: 20.0,
            lifetime: 1.0,
            speed: 60.0,
            spread: std::f32::consts::PI,
            direction: Vec2::new(0.0, -1.0),
            color_start: Color::WHITE,
            color_end: Color::rgba(1.0, 1.0, 1.0, 0.0),
            size_start: Vec2::splat(8.0),
            size_end: Vec2::splat(2.0),
            layer: 10,
            active: true,
            spawn_accumulator: 0.0,
        }
    }
}

/// a single live particle.
#[derive(Debug, Clone, Copy)]
pub struct Particle {
    /// world position
    pub position: Vec2,
    /// linear velocity
    pub velocity: Vec2,
    /// time elapsed since spawn
    pub age: f32,
    /// total lifetime
    pub lifetime: f32,
    /// sprite layer
    pub layer: i32,
    /// texture id
    pub texture_id: u64,
    /// color at birth
    pub color_start: Color,
    /// color at end of life
    pub color_end: Color,
    /// size at birth
    pub size_start: Vec2,
    /// size at end of life
    pub size_end: Vec2,
}

impl Particle {
    fn is_dead(&self) -> bool {
        self.age >= self.lifetime
    }

    fn t(&self) -> f32 {
        if self.lifetime > 0.0 { (self.age / self.lifetime).clamp(0.0, 1.0) } else { 1.0 }
    }

    fn current_color(&self) -> Color {
        let t = self.t();
        lerp_color(self.color_start, self.color_end, t)
    }

    fn current_size(&self) -> Vec2 {
        let t = self.t();
        self.size_start * (1.0 - t) + self.size_end * t
    }
}

/// global particle pool. pre-allocated, cleared each frame.
/// insert this resource if you want to customize the capacity.
#[derive(Resource)]
pub struct ParticlePool {
    particles: Vec<Particle>,
    /// maximum particles before spawn stops (prevents runaway emitters)
    pub capacity: usize,
    /// cheap noise offset for deterministic-ish spread angles
    noise_offset: f32,
}

impl Default for ParticlePool {
    fn default() -> Self {
        Self::new(4096)
    }
}

impl ParticlePool {
    /// create a pool with the given max particle capacity
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            particles: Vec::with_capacity(capacity.min(4096)),
            capacity,
            noise_offset: 0.0,
        }
    }

    /// number of currently live particles
    #[must_use]
    pub fn count(&self) -> usize {
        self.particles.len()
    }
}

/// plugin that registers the particle simulation and render systems.
pub struct ParticlePlugin;

impl GamePlugin for ParticlePlugin {
    fn name(&self) -> &'static str {
        "ParticlePlugin"
    }

    fn build(&mut self, app: &mut App) {
        app.insert_resource(ParticlePool::default());
        app.add_system_to_stage(
            lunar_core::UpdateStage::Update,
            (tick_particles, draw_particles).chain(),
        );
    }
}

/// tick all live particles and spawn new ones from active emitters.
pub fn tick_particles(
    time: Res<lunar_core::Time>,
    mut pool: ResMut<ParticlePool>,
    mut emitters: Query<(&Transform, &mut ParticleEmitter)>,
) {
    let delta = time.delta_seconds();
    if delta <= 0.0 { return; }

    // advance all live particles
    for particle in &mut pool.particles {
        particle.age += delta;
        particle.position += particle.velocity * delta;
    }

    // cull dead particles (swap-remove for O(1))
    let mut i = 0;
    while i < pool.particles.len() {
        if pool.particles[i].is_dead() {
            pool.particles.swap_remove(i);
        } else {
            i += 1;
        }
    }

    // spawn from active emitters
    for (transform, mut emitter) in &mut emitters {
        if !emitter.active { continue; }
        emitter.spawn_accumulator += emitter.rate * delta;
        while emitter.spawn_accumulator >= 1.0 && pool.particles.len() < pool.capacity {
            emitter.spawn_accumulator -= 1.0;
            pool.noise_offset += 0.618_034; // golden angle increment

            let angle_offset = pool.noise_offset.sin() * emitter.spread;
            let base_angle = emitter.direction.y.atan2(emitter.direction.x);
            let angle = base_angle + angle_offset;
            let velocity = Vec2::new(angle.cos(), angle.sin()) * emitter.speed;

            pool.particles.push(Particle {
                position: transform.translation,
                velocity,
                age: 0.0,
                lifetime: emitter.lifetime,
                layer: emitter.layer,
                texture_id: u64::from(emitter.texture.id()),
                color_start: emitter.color_start,
                color_end: emitter.color_end,
                size_start: emitter.size_start,
                size_end: emitter.size_end,
            });
        }
    }
}

/// enqueue sprite draw commands for all live particles.
pub fn draw_particles(
    pool: Res<ParticlePool>,
    mut queue: ResMut<RenderQueue>,
) {
    for particle in &pool.particles {
        let size = particle.current_size();
        let color = particle.current_color();
        queue.push(DrawCommand {
            kind: DrawKind::Sprite {
                texture: Some(particle.texture_id),
                position: particle.position,
                rotation: 0.0,
                scale: size,
                tint: color,
                layer: particle.layer,
                uv_rect: None,
                origin: size * 0.5,
                sort_key: None,
            },
        });
    }
}

fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    Color::rgba(
        a.r * (1.0 - t) + b.r * t,
        a.g * (1.0 - t) + b.g * t,
        a.b * (1.0 - t) + b.b * t,
        a.a * (1.0 - t) + b.a * t,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use lunar_core::Time;

    fn make_pool() -> ParticlePool {
        ParticlePool::new(100)
    }

    #[test]
    fn particles_age_and_die() {
        let mut pool = make_pool();
        pool.particles.push(Particle {
            position: Vec2::ZERO,
            velocity: Vec2::ZERO,
            age: 0.9,
            lifetime: 1.0,
            layer: 0,
            texture_id: 0,
            color_start: Color::WHITE,
            color_end: Color::WHITE,
            size_start: Vec2::splat(8.0),
            size_end: Vec2::splat(2.0),
        });

        let mut world = World::new();
        let mut time = Time::default();
        time.set_delta_seconds(0.2);
        world.insert_resource(time);
        world.insert_resource(pool);

        let mut system = IntoSystem::into_system(tick_particles);
        system.initialize(&mut world);
        let _ = system.run((), &mut world);

        let pool = world.resource::<ParticlePool>();
        assert_eq!(pool.count(), 0, "particle should have died");
    }

    #[test]
    fn particles_move_with_velocity() {
        let mut pool = make_pool();
        pool.particles.push(Particle {
            position: Vec2::ZERO,
            velocity: Vec2::new(10.0, 0.0),
            age: 0.0,
            lifetime: 5.0,
            layer: 0,
            texture_id: 0,
            color_start: Color::WHITE,
            color_end: Color::WHITE,
            size_start: Vec2::splat(4.0),
            size_end: Vec2::splat(4.0),
        });

        let mut world = World::new();
        let mut time = Time::default();
        time.set_delta_seconds(1.0);
        world.insert_resource(time);
        world.insert_resource(pool);

        let mut system = IntoSystem::into_system(tick_particles);
        system.initialize(&mut world);
        let _ = system.run((), &mut world);

        let pool = world.resource::<ParticlePool>();
        assert!((pool.particles[0].position.x - 10.0).abs() < 0.001);
    }

    #[test]
    fn particle_color_lerp() {
        let particle = Particle {
            position: Vec2::ZERO,
            velocity: Vec2::ZERO,
            age: 0.5,
            lifetime: 1.0,
            layer: 0,
            texture_id: 0,
            color_start: Color::rgba(1.0, 0.0, 0.0, 1.0),
            color_end: Color::rgba(0.0, 1.0, 0.0, 0.0),
            size_start: Vec2::splat(8.0),
            size_end: Vec2::splat(0.0),
        };
        let color = particle.current_color();
        assert!((color.r - 0.5).abs() < 0.01);
        assert!((color.g - 0.5).abs() < 0.01);
        assert!((color.a - 0.5).abs() < 0.01);
    }
}
