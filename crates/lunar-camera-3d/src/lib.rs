//! SpringArm3d orbit camera — follows a target entity and shortens the arm
//! when geometry blocks the line of sight via 3D raycasting.
//!
//! # usage
//!
//! ```ignore
//! use lunar_camera_3d::{SpringArm3d, spring_arm_system};
//! use lunar_3d::LocalTransform3d;
//! use bevy_ecs::prelude::*;
//!
//! fn setup(mut commands: Commands) {
//!     let target = commands.spawn(LocalTransform3d::from_xyz(0.0, 0.0, 0.0)).id();
//!     commands.spawn((
//!         LocalTransform3d::default(),
//!         lunar_3d::WorldTransform3d::default(),
//!         lunar_3d::Camera3d::default(),
//!         SpringArm3d::new(target, 5.0),
//!     ));
//! }
//! ```

use bevy_ecs::prelude::*;
use lunar_math::{Quat, Vec3};

use lunar_3d::{Collider3d, LocalTransform3d, Mesh3d, MeshRegistry, WorldTransform3d};
use lunar_3d::collision::{Ray3d, RayHit3d, raycast_3d};
use lunar_3d::visibility::CullSoa;

/// orbit camera component — mounts the camera on a spring arm behind a target entity.
///
/// each frame `spring_arm_system` positions the camera at `target + arm_offset` rotated
/// by `yaw` and `pitch`. if a raycast from the target hits geometry before reaching the
/// desired arm length, the arm is shortened to keep the camera in front of the obstacle.
/// the actual arm length smoothly recovers toward the desired length when the obstruction clears.
#[derive(Component)]
pub struct SpringArm3d {
    /// entity the arm pivots around. must have a `WorldTransform3d`.
    pub target: Entity,
    /// desired arm length in world units.
    pub desired_length: f32,
    /// horizontal rotation around the target in radians. 0 = +Z behind.
    pub yaw: f32,
    /// vertical rotation in radians. 0 = level, positive = looking up.
    pub pitch: f32,
    /// offset from the target's position to the pivot point (e.g. head height).
    pub pivot_offset: Vec3,
    /// how quickly the arm extends back to `desired_length` after clearing an obstacle.
    /// units: world-units per second. default 5.0.
    pub recover_speed: f32,
    /// collision mask passed to the raycast. default 1.
    pub collision_mask: u32,
    /// current actual arm length (updated by the system).
    current_length: f32,
    /// cached pivot from last raycast — skip raycast when pivot hasn't moved.
    last_cast_pivot: Vec3,
    /// cached desired_length from last raycast — skip raycast when it hasn't changed.
    last_cast_desired: f32,
}

impl SpringArm3d {
    /// create a spring arm targeting `target` with `desired_length`.
    #[must_use]
    pub fn new(target: Entity, desired_length: f32) -> Self {
        Self {
            target,
            desired_length,
            yaw: 0.0,
            pitch: 0.2, // slight downward look
            pivot_offset: Vec3::new(0.0, 1.5, 0.0),
            recover_speed: 5.0,
            collision_mask: 1,
            current_length: desired_length,
            last_cast_pivot: Vec3::splat(f32::NAN),
            last_cast_desired: f32::NAN,
        }
    }

    /// the current actual arm length (may be shorter than `desired_length` due to obstacles).
    #[must_use]
    pub fn current_length(&self) -> f32 {
        self.current_length
    }

    /// compute the arm direction vector (unit length, pointing from target to camera).
    #[must_use]
    pub fn arm_direction(&self) -> Vec3 {
        let rot = Quat::from_rotation_y(self.yaw) * Quat::from_rotation_x(-self.pitch);
        rot * Vec3::Z
    }
}

/// system — updates all [`SpringArm3d`] cameras each frame.
///
/// reads the target's world transform, casts a ray along the arm direction,
/// shortens the arm on hit, then writes the camera's `LocalTransform3d` and looks at the pivot.
///
/// add to your schedule in the PostUpdate stage (after transforms propagate).
pub fn spring_arm_system(
    time: bevy_ecs::system::Res<lunar_core::Time>,
    soa: bevy_ecs::system::Res<CullSoa>,
    registry: bevy_ecs::system::Res<MeshRegistry>,
    target_query: Query<&WorldTransform3d>,
    mesh_query: Query<(&Mesh3d, &WorldTransform3d, Option<&Collider3d>)>,
    mut arm_query: Query<(&mut SpringArm3d, &mut LocalTransform3d)>,
) {
    let delta = time.delta_seconds();

    for (mut arm, mut transform) in arm_query.iter_mut() {
        let Ok(target_world) = target_query.get(arm.target) else {
            continue;
        };

        let pivot = target_world.translation + arm.pivot_offset;
        let arm_dir = arm.arm_direction();

        // only re-cast when pivot or desired_length changed meaningfully
        let pivot_moved = (pivot - arm.last_cast_pivot).length_squared() > 1e-4;
        let length_changed = (arm.desired_length - arm.last_cast_desired).abs() > 1e-4;
        let blocked_length = if pivot_moved || length_changed {
            arm.last_cast_pivot = pivot;
            arm.last_cast_desired = arm.desired_length;
            let ray = Ray3d::new(pivot, arm_dir);
            raycast_3d(
                ray,
                arm.desired_length,
                arm.collision_mask,
                &soa,
                &mesh_query,
                &registry,
            )
            .map(|hit: RayHit3d| (hit.distance - 0.2).max(0.1))
            .unwrap_or(arm.desired_length)
        } else {
            arm.current_length.min(arm.desired_length)
        };

        // snap to shorter, recover smoothly toward desired
        if blocked_length < arm.current_length {
            arm.current_length = blocked_length;
        } else {
            let recovered = arm.current_length + arm.recover_speed * delta;
            arm.current_length = recovered.min(arm.desired_length).min(blocked_length);
        }

        let camera_pos = pivot + arm_dir * arm.current_length;

        // look at the pivot from the camera position
        let forward = (pivot - camera_pos).normalize_or_zero();
        let rotation = if forward.length_squared() > 1e-6 {
            look_rotation(forward, Vec3::Y)
        } else {
            Quat::IDENTITY
        };

        *transform = LocalTransform3d {
            translation: camera_pos,
            rotation,
            scale: Vec3::ONE,
        };
    }
}

/// build a rotation that points +Z at `forward` with +Y approximating `up`.
fn look_rotation(forward: Vec3, up: Vec3) -> Quat {
    let right = up.cross(forward);
    // degenerate: forward parallel to up (looking straight up/down); fall back to world X as up
    let right = if right.length_squared() > 1e-6 {
        right.normalize()
    } else {
        let alt = Vec3::X.cross(forward);
        if alt.length_squared() > 1e-6 { alt.normalize() } else { return Quat::IDENTITY; }
    };
    let actual_up = forward.cross(right);
    Quat::from_mat3(&lunar_math::Mat3::from_cols(right, actual_up, forward))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arm_direction_yaw_zero_points_positive_z() {
        let arm = SpringArm3d::new(Entity::PLACEHOLDER, 5.0);
        let dir = arm.arm_direction();
        // yaw=0, pitch≈0.2 — Z component should dominate and be positive
        assert!(dir.z > 0.0, "arm direction should have +Z component with zero yaw");
    }

    #[test]
    fn arm_direction_yaw_pi_points_negative_z() {
        let mut arm = SpringArm3d::new(Entity::PLACEHOLDER, 5.0);
        arm.yaw = std::f32::consts::PI;
        arm.pitch = 0.0;
        let dir = arm.arm_direction();
        assert!(dir.z < -0.5, "yaw=π should flip Z direction");
    }

    #[test]
    fn look_rotation_forward_z() {
        let rot = look_rotation(Vec3::Z, Vec3::Y);
        let rotated = rot * Vec3::Z;
        assert!((rotated - Vec3::Z).length() < 1e-4);
    }

    #[test]
    fn arm_defaults_current_equals_desired() {
        let arm = SpringArm3d::new(Entity::PLACEHOLDER, 8.0);
        assert_eq!(arm.current_length, arm.desired_length);
    }
}
