//! camera follow system for 2d games.
//!
//! insert a [`CameraFollow2d`] resource to make the camera track a target
//! entity. the system runs in PostUpdate, after transforms propagate, so it
//! always reads the target's position for the current frame.
//!
//! # example
//!
//! ```ignore
//! use lunar_render::{Camera, CameraFollow2d};
//! use lunar_math::{Vec2, Rect};
//!
//! // follow entity `player`, no lead, small deadzone, bounded to the level
//! commands.insert_resource(CameraFollow2d {
//!     target: player,
//!     lead: Vec2::ZERO,
//!     deadzone: Vec2::new(32.0, 24.0),
//!     bounds: Some(Rect::new(0.0, 0.0, 3200.0, 1800.0)),
//!     lerp_speed: 8.0,
//! });
//! ```

use bevy_ecs::prelude::*;
use lunar_math::{Rect, Vec2, WorldTransform};

use crate::Camera;

/// drives the camera to track a target entity each frame.
///
/// all fields are public so game code can tweak them at runtime (e.g. widen
/// deadzone during a cutscene or remove bounds when zooming out to a world map).
#[derive(Resource)]
pub struct CameraFollow2d {
    /// entity to follow — must have a [`WorldTransform`] component
    pub target: Entity,
    /// world-space offset added to the target position before tracking.
    /// positive X leads right, positive Y leads down (matches screen Y)
    pub lead: Vec2,
    /// half-extents of the dead zone in world space.
    /// the camera does not move while the target stays within this box.
    /// set to `Vec2::ZERO` to always track exactly
    pub deadzone: Vec2,
    /// world-space rectangle the camera position is clamped to after tracking.
    /// `None` means unbounded
    pub bounds: Option<Rect>,
    /// lerp speed in units per second (0.0 = snap immediately, higher = smoother)
    pub lerp_speed: f32,
}

pub(crate) fn camera_follow_system(
    follow: Option<Res<CameraFollow2d>>,
    mut camera: Option<ResMut<Camera>>,
    transforms: Query<&WorldTransform>,
    time: Res<lunar_core::Time>,
) {
    let (Some(follow), Some(camera)) = (follow, camera.as_mut()) else {
        return;
    };
    let Ok(target_transform) = transforms.get(follow.target) else {
        return;
    };

    let desired = target_transform.translation + follow.lead;
    let delta = desired - camera.position;

    // skip update if target is inside the deadzone
    if delta.x.abs() <= follow.deadzone.x && delta.y.abs() <= follow.deadzone.y {
        return;
    }

    let new_position = if follow.lerp_speed <= 0.0 {
        desired
    } else {
        let t = (follow.lerp_speed * time.delta_seconds()).min(1.0);
        camera.position + delta * t
    };

    camera.position = match follow.bounds {
        None => new_position,
        Some(bounds) => Vec2::new(
            new_position.x.clamp(bounds.x, bounds.x + bounds.w),
            new_position.y.clamp(bounds.y, bounds.y + bounds.h),
        ),
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use lunar_core::Time;

    fn make_world_with_target(position: Vec2) -> (World, Entity) {
        let mut world = World::new();
        let entity = world.spawn(WorldTransform::from_xy(position.x, position.y)).id();
        world.insert_resource(Time::default());
        (world, entity)
    }

    #[test]
    fn snaps_to_target_with_zero_lerp() {
        let (mut world, target) = make_world_with_target(Vec2::new(200.0, 100.0));
        world.insert_resource(Camera::new());
        world.insert_resource(CameraFollow2d {
            target,
            lead: Vec2::ZERO,
            deadzone: Vec2::ZERO,
            bounds: None,
            lerp_speed: 0.0,
        });

        let mut system = IntoSystem::into_system(camera_follow_system);
        system.initialize(&mut world);
        let _ = system.run((), &mut world);

        let camera = world.resource::<Camera>();
        assert!((camera.position.x - 200.0).abs() < 0.001);
        assert!((camera.position.y - 100.0).abs() < 0.001);
    }

    #[test]
    fn deadzone_prevents_movement() {
        let (mut world, target) = make_world_with_target(Vec2::new(10.0, 5.0));
        world.insert_resource(Camera::new()); // camera at origin
        world.insert_resource(CameraFollow2d {
            target,
            lead: Vec2::ZERO,
            deadzone: Vec2::new(50.0, 50.0),
            bounds: None,
            lerp_speed: 0.0,
        });

        let mut system = IntoSystem::into_system(camera_follow_system);
        system.initialize(&mut world);
        let _ = system.run((), &mut world);

        let camera = world.resource::<Camera>();
        // target (10, 5) is inside deadzone (50, 50) — camera should not move
        assert!((camera.position.x - 0.0).abs() < 0.001);
        assert!((camera.position.y - 0.0).abs() < 0.001);
    }

    #[test]
    fn bounds_clamp_camera_position() {
        let (mut world, target) = make_world_with_target(Vec2::new(9999.0, 9999.0));
        world.insert_resource(Camera::new());
        world.insert_resource(CameraFollow2d {
            target,
            lead: Vec2::ZERO,
            deadzone: Vec2::ZERO,
            bounds: Some(Rect::new(0.0, 0.0, 800.0, 600.0)),
            lerp_speed: 0.0,
        });

        let mut system = IntoSystem::into_system(camera_follow_system);
        system.initialize(&mut world);
        let _ = system.run((), &mut world);

        let camera = world.resource::<Camera>();
        assert!((camera.position.x - 800.0).abs() < 0.001);
        assert!((camera.position.y - 600.0).abs() < 0.001);
    }

    #[test]
    fn lead_offset_applied() {
        let (mut world, target) = make_world_with_target(Vec2::new(100.0, 100.0));
        world.insert_resource(Camera::new());
        world.insert_resource(CameraFollow2d {
            target,
            lead: Vec2::new(50.0, 0.0),
            deadzone: Vec2::ZERO,
            bounds: None,
            lerp_speed: 0.0,
        });

        let mut system = IntoSystem::into_system(camera_follow_system);
        system.initialize(&mut world);
        let _ = system.run((), &mut world);

        let camera = world.resource::<Camera>();
        // desired = (100 + 50, 100 + 0) = (150, 100)
        assert!((camera.position.x - 150.0).abs() < 0.001);
        assert!((camera.position.y - 100.0).abs() < 0.001);
    }
}
