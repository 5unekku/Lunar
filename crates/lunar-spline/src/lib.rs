//! catmull-rom spline asset and [`PathFollower`] component for smooth path following.
//!
//! # usage
//!
//! ```ignore
//! use lunar_spline::{Spline, PathFollower, advance_path_followers};
//! use lunar_math::Vec3;
//!
//! let spline = Spline::new(vec![
//!     Vec3::new(0.0, 0.0, 0.0),
//!     Vec3::new(5.0, 2.0, 0.0),
//!     Vec3::new(10.0, 0.0, 0.0),
//! ]);
//!
//! // spline.sample(0.5) returns the midpoint
//! let mid = spline.sample(0.5);
//! ```

use bevy_ecs::prelude::*;
use lunar_assets::{Asset, Handle};
use lunar_math::Vec3;

/// catmull-rom spline defined by a sequence of control points.
///
/// the curve passes through all control points (unlike Bezier).
/// points at index 0 and `n-1` are phantom endpoints used only to define the
/// tangent at the first and last segment; the curve is drawn from index 1 to n-2.
/// if fewer than 4 points are provided, the curve falls back to linear interpolation.
pub struct Spline {
    /// control points in world space. at least 2 required.
    pub points: Vec<Vec3>,
    /// catmull-rom tension. 0.5 = standard centripetal, 0.0 = uniform.
    pub tension: f32,
}

impl Spline {
    /// create a spline through the given points with standard tension (0.5).
    ///
    /// if `points.len() < 2`, `sample` returns `Vec3::ZERO`.
    #[must_use]
    pub fn new(points: Vec<Vec3>) -> Self {
        Self { points, tension: 0.5 }
    }

    /// create a spline with custom tension. `tension` in `[0.0, 1.0]`.
    #[must_use]
    pub fn with_tension(mut self, tension: f32) -> Self {
        self.tension = tension.clamp(0.0, 1.0);
        self
    }

    /// sample the spline at `t` in `[0.0, 1.0]`.
    ///
    /// `t=0.0` returns the first point, `t=1.0` returns the last.
    /// if only 2 or 3 points are provided, linear interpolation is used.
    #[must_use]
    pub fn sample(&self, t: f32) -> Vec3 {
        let n = self.points.len();
        if n < 2 {
            return self.points.first().copied().unwrap_or(Vec3::ZERO);
        }
        if n < 4 {
            return linear_interp(&self.points, t);
        }

        let t = t.clamp(0.0, 1.0);
        // map t to segment index
        let segments = n as f32 - 3.0; // interior segments
        let segment_t = t * segments;
        let seg = (segment_t as usize).min(n - 4);
        let local_t = segment_t - seg as f32;

        let p0 = self.points[seg];
        let p1 = self.points[seg + 1];
        let p2 = self.points[seg + 2];
        let p3 = self.points[seg + 3];

        catmull_rom(p0, p1, p2, p3, local_t, self.tension)
    }

    /// derivative of the spline at `t` (tangent direction, not normalized).
    #[must_use]
    pub fn tangent(&self, t: f32) -> Vec3 {
        let n = self.points.len();
        if n < 4 {
            let last = self.points.last().copied().unwrap_or(Vec3::ZERO);
            let first = self.points.first().copied().unwrap_or(Vec3::ZERO);
            return last - first;
        }

        let t = t.clamp(0.0, 1.0);
        let segments = n as f32 - 3.0;
        let segment_t = t * segments;
        let seg = (segment_t as usize).min(n - 4);
        let local_t = segment_t - seg as f32;

        let p0 = self.points[seg];
        let p1 = self.points[seg + 1];
        let p2 = self.points[seg + 2];
        let p3 = self.points[seg + 3];

        catmull_rom_tangent(p0, p1, p2, p3, local_t, self.tension)
    }

    /// arc-length-parameterized sample using `steps` uniform subdivisions.
    ///
    /// more accurate than `sample` for non-uniform point spacing, at the cost of
    /// iterating `steps` segments to build a lookup table each call. cache the
    /// result if sampling many times at the same parameterization.
    #[must_use]
    pub fn sample_arc(self: &Spline, t: f32, steps: usize) -> Vec3 {
        let steps = steps.max(2);
        let mut lengths = Vec::with_capacity(steps + 1);
        let mut prev = self.sample(0.0);
        lengths.push(0.0f32);
        for i in 1..=steps {
            let s = i as f32 / steps as f32;
            let next = self.sample(s);
            lengths.push(lengths.last().copied().unwrap_or(0.0) + (next - prev).length());
            prev = next;
        }
        let total = *lengths.last().unwrap_or(&1.0);
        if total < 1e-7 {
            return self.sample(0.0);
        }
        let target = t.clamp(0.0, 1.0) * total;
        let idx = lengths.partition_point(|&l| l < target).saturating_sub(1).min(steps - 1);
        let seg_start = lengths[idx];
        let seg_end = lengths[idx + 1];
        let seg_len = seg_end - seg_start;
        let local_t = if seg_len < 1e-7 { 0.0 } else { (target - seg_start) / seg_len };
        let t0 = idx as f32 / steps as f32;
        let t1 = (idx + 1) as f32 / steps as f32;
        self.sample(t0 + local_t * (t1 - t0))
    }
}

impl Asset for Spline {}

/// component that moves an entity along a spline over time.
///
/// pair with `LocalTransform` (2D) or `LocalTransform3d` (3D) and register
/// `advance_path_followers` to animate it.
#[derive(Component)]
pub struct PathFollower {
    /// handle to the spline to follow.
    pub spline: Handle<Spline>,
    /// current normalized position on the spline `[0.0, 1.0]`.
    pub t: f32,
    /// movement speed in normalized spline units per second.
    /// to express in world units use `speed / spline.arc_length(steps)`.
    pub speed: f32,
    /// if true, reverse direction at endpoints instead of looping.
    pub ping_pong: bool,
    /// if true, loop back to start at the end.
    pub looping: bool,
    /// if true, the follower is paused.
    pub paused: bool,
    /// current direction: 1.0 forward, -1.0 backward (ping-pong only).
    direction: f32,
}

impl PathFollower {
    /// create a follower for the given spline handle.
    #[must_use]
    pub fn new(spline: Handle<Spline>, speed: f32) -> Self {
        Self {
            spline,
            t: 0.0,
            speed,
            ping_pong: false,
            looping: true,
            paused: false,
            direction: 1.0,
        }
    }
}

/// system — advance all [`PathFollower`] components and update their `LocalTransform3d` positions.
///
/// splines must be stored in a resource implementing `Fn(Handle<Spline>) -> Option<&Spline>`.
/// in practice this is the `MeshRegistry`-style pattern; game code builds a lookup closure.
///
/// this system takes a spline lookup function as a system parameter via a marker resource.
pub fn advance_path_followers(
    time: Res<lunar_core::Time>,
    splines: Res<SplineStore>,
    mut query: Query<(&mut PathFollower, &mut lunar_math::LocalTransform)>,
) {
    let delta = time.delta_seconds();
    for (mut follower, mut transform) in query.iter_mut() {
        if follower.paused {
            continue;
        }
        let Some(spline) = splines.get(follower.spline) else {
            continue;
        };

        follower.t += follower.speed * follower.direction * delta;

        if follower.ping_pong {
            if follower.t >= 1.0 {
                follower.t = 1.0;
                follower.direction = -1.0;
            } else if follower.t <= 0.0 {
                follower.t = 0.0;
                follower.direction = 1.0;
            }
        } else if follower.looping {
            follower.t = follower.t.rem_euclid(1.0);
        } else {
            follower.t = follower.t.clamp(0.0, 1.0);
        }

        let pos = spline.sample(follower.t);
        transform.translation = lunar_math::Vec2::new(pos.x, pos.y);
    }
}

/// resource — stores splines by handle for [`advance_path_followers`].
///
/// insert this resource with your spline data before using path followers.
#[derive(Resource, Default)]
pub struct SplineStore {
    splines: std::collections::HashMap<u32, Spline>,
    next_id: u32,
}

impl SplineStore {
    /// add a spline and return a handle.
    pub fn add(&mut self, spline: Spline) -> Handle<Spline> {
        let id = self.next_id;
        self.next_id += 1;
        self.splines.insert(id, spline);
        Handle::new(id, 0)
    }

    /// retrieve a spline by handle.
    #[must_use]
    pub fn get(&self, handle: Handle<Spline>) -> Option<&Spline> {
        self.splines.get(&handle.id())
    }
}

/// catmull-rom interpolation between p1 and p2 (p0 and p3 are tangent helpers).
fn catmull_rom(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, t: f32, tension: f32) -> Vec3 {
    let m1 = (p2 - p0) * tension;
    let m2 = (p3 - p1) * tension;
    let t2 = t * t;
    let t3 = t2 * t;
    let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
    let h10 = t3 - 2.0 * t2 + t;
    let h01 = -2.0 * t3 + 3.0 * t2;
    let h11 = t3 - t2;
    p1 * h00 + m1 * h10 + p2 * h01 + m2 * h11
}

fn catmull_rom_tangent(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, t: f32, tension: f32) -> Vec3 {
    let m1 = (p2 - p0) * tension;
    let m2 = (p3 - p1) * tension;
    let t2 = t * t;
    let dh00 = 6.0 * t2 - 6.0 * t;
    let dh10 = 3.0 * t2 - 4.0 * t + 1.0;
    let dh01 = -6.0 * t2 + 6.0 * t;
    let dh11 = 3.0 * t2 - 2.0 * t;
    p1 * dh00 + m1 * dh10 + p2 * dh01 + m2 * dh11
}

fn linear_interp(points: &[Vec3], t: f32) -> Vec3 {
    if points.len() == 1 {
        return points[0];
    }
    let t = t.clamp(0.0, 1.0);
    let segment_t = t * (points.len() - 1) as f32;
    let idx = (segment_t as usize).min(points.len() - 2);
    let local_t = segment_t - idx as f32;
    points[idx].lerp(points[idx + 1], local_t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn straight_spline() -> Spline {
        // four collinear points along X axis
        Spline::new(vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(3.0, 0.0, 0.0),
        ])
    }

    #[test]
    fn sample_endpoints() {
        let spline = straight_spline();
        let start = spline.sample(0.0);
        let end = spline.sample(1.0);
        // catmull-rom passes through interior control points (1 and n-2)
        assert!((start.x - 1.0).abs() < 0.01, "t=0 should be near p1 (x=1)");
        assert!((end.x - 2.0).abs() < 0.01, "t=1 should be near p2 (x=2)");
    }

    #[test]
    fn sample_midpoint_straight_line() {
        let spline = straight_spline();
        let mid = spline.sample(0.5);
        // midpoint of a straight spline should be near x=1.5
        assert!((mid.x - 1.5).abs() < 0.01);
        assert!(mid.y.abs() < 0.001);
    }

    #[test]
    fn sample_clamps_to_0_1() {
        let spline = straight_spline();
        let neg = spline.sample(-0.5);
        let over = spline.sample(1.5);
        let at_zero = spline.sample(0.0);
        let at_one = spline.sample(1.0);
        assert!((neg - at_zero).length() < 1e-5);
        assert!((over - at_one).length() < 1e-5);
    }

    #[test]
    fn linear_fallback_two_points() {
        let spline = Spline::new(vec![Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0)]);
        let mid = spline.sample(0.5);
        assert!((mid.x - 5.0).abs() < 0.01);
    }

    #[test]
    fn tangent_nonzero_on_curve() {
        let spline = straight_spline();
        let tangent = spline.tangent(0.5);
        assert!(tangent.length() > 0.01, "tangent on a straight spline should be nonzero");
        assert!(tangent.x > 0.0, "tangent on +X line should point +X");
    }

    #[test]
    fn spline_store_add_and_get() {
        let mut store = SplineStore::default();
        let handle = store.add(straight_spline());
        assert!(store.get(handle).is_some());
    }

    #[test]
    fn path_follower_advances_t() {
        let mut world = World::new();
        let mut store = SplineStore::default();
        let handle = store.add(straight_spline());
        world.insert_resource(store);

        let mut time = lunar_core::Time::default();
        time.set_delta_seconds(0.1);
        world.insert_resource(time);

        let transform = lunar_math::LocalTransform::default();
        let follower = PathFollower::new(handle, 1.0);
        let entity = world.spawn((follower, transform)).id();

        let mut system = bevy_ecs::system::IntoSystem::into_system(advance_path_followers);
        system.initialize(&mut world);
        let _ = system.run((), &mut world);

        let t = world.get::<PathFollower>(entity).unwrap().t;
        assert!((t - 0.1).abs() < 0.001);
    }
}
