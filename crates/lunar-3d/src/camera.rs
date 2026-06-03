use bevy_ecs::prelude::*;
use lunar_math::{Mat4, Vec3};

use crate::transform::WorldTransform3d;

/// projection mode for a 3D camera.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Projection {
	/// standard perspective projection. fov_y in radians.
	Perspective {
		/// vertical field of view in radians. 60° (~1.05 rad) is typical.
		fov_y: f32,
		/// near clipping plane distance. keep as large as feasible (0.1 minimum).
		near: f32,
		/// far clipping plane distance.
		far: f32,
	},
	/// orthographic projection. width defines world units visible across the viewport.
	Orthographic {
		/// world units visible horizontally.
		width: f32,
		/// near clipping plane.
		near: f32,
		/// far clipping plane.
		far: f32,
	},
}

impl Projection {
	/// build the projection matrix for the given viewport aspect ratio (width / height).
	#[must_use]
	pub fn matrix(self, aspect: f32) -> Mat4 {
		match self {
			Self::Perspective { fov_y, near, far } => {
				Mat4::perspective_rh(fov_y, aspect, near, far)
			}
			Self::Orthographic { width, near, far } => {
				let half_w = width * 0.5;
				let half_h = half_w / aspect;
				Mat4::orthographic_rh(-half_w, half_w, -half_h, half_h, near, far)
			}
		}
	}
}

impl Default for Projection {
	fn default() -> Self {
		Self::Perspective {
			fov_y: std::f32::consts::FRAC_PI_3, // 60°
			near: 0.1,
			far: 1000.0,
		}
	}
}

/// 3D camera component.
///
/// place this alongside a [`WorldTransform3d`] on an entity to mark it as the
/// active camera. the render system reads the first entity with both components
/// and builds view + projection matrices from it.
///
/// the view matrix is derived from the entity's world transform — position and
/// orientation live there, not here. this component only stores projection state.
///
/// # example
///
/// ```ignore
/// commands.spawn((
///     LocalTransform3d::from_xyz(0.0, 2.0, 10.0),
///     WorldTransform3d::default(),
///     Camera3d::default(),
/// ));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Component)]
pub struct Camera3d {
	/// how the world projects onto the screen.
	pub projection: Projection,
	/// render order when multiple cameras exist. higher = rendered later (on top).
	pub priority: i32,
	/// whether this camera is active.
	pub active: bool,
}

impl Camera3d {
	/// build the view matrix from this camera's world transform.
	///
	/// the view matrix is the inverse of the camera's world transform matrix —
	/// it transforms world-space positions into camera (eye) space.
	#[must_use]
	pub fn view_matrix(transform: WorldTransform3d) -> Mat4 {
		let eye = transform.translation;
		let target = eye + transform.forward();
		let up = transform.up();
		Mat4::look_at_rh(eye, target, up)
	}

	/// build the view-projection matrix (VP) for the given aspect ratio.
	#[must_use]
	pub fn view_proj(self, transform: WorldTransform3d, aspect: f32) -> Mat4 {
		self.projection.matrix(aspect) * Self::view_matrix(transform)
	}
}

impl Default for Camera3d {
	fn default() -> Self {
		Self {
			projection: Projection::default(),
			priority: 0,
			active: true,
		}
	}
}

/// camera target resource — the entity currently acting as the active 3D camera.
///
/// the render system sets this automatically to the highest-priority active
/// [`Camera3d`] entity each frame.
#[derive(bevy_ecs::prelude::Resource, Debug, Clone, Copy, Default)]
pub struct ActiveCamera3d {
	pub entity: Option<bevy_ecs::entity::Entity>,
}

/// normalized screen-space viewport rectangle for a camera (split-screen / PiP).
///
/// coordinates are in `[0,1]` relative to the window: (0,0) = top-left, (1,1) = bottom-right.
/// when absent from a Camera3d entity, the camera uses the full window.
///
/// # split-screen example
///
/// ```ignore
/// // player 1: left half
/// commands.entity(cam1).insert(ViewportRect { x: 0.0, y: 0.0, width: 0.5, height: 1.0 });
/// // player 2: right half
/// commands.entity(cam2).insert(ViewportRect { x: 0.5, y: 0.0, width: 0.5, height: 1.0 });
/// ```
#[derive(Debug, Clone, Copy, Component)]
pub struct ViewportRect {
	/// left edge in normalized window coordinates [0, 1]
	pub x: f32,
	/// top edge in normalized window coordinates [0, 1]
	pub y: f32,
	/// width in normalized window coordinates [0, 1]
	pub width: f32,
	/// height in normalized window coordinates [0, 1]
	pub height: f32,
}

impl ViewportRect {
	/// full-screen viewport (default when no `ViewportRect` component is present).
	pub const FULL: Self = Self {
		x: 0.0,
		y: 0.0,
		width: 1.0,
		height: 1.0,
	};

	/// convert to pixel coordinates given the window size.
	#[must_use]
	pub fn to_pixels(&self, win_w: u32, win_h: u32) -> (u32, u32, u32, u32) {
		(
			(self.x * win_w as f32) as u32,
			(self.y * win_h as f32) as u32,
			((self.width * win_w as f32) as u32).max(1),
			((self.height * win_h as f32) as u32).max(1),
		)
	}

	/// aspect ratio (width / height) for projection matrix computation.
	#[must_use]
	pub fn aspect(&self) -> f32 {
		if self.height < 1e-6 {
			1.0
		} else {
			self.width / self.height
		}
	}
}

/// resource: ordered list of active cameras and their viewport rects.
///
/// built by `update_active_viewports` each frame from all `Camera3d` entities
/// with `active = true`, sorted by priority (lowest renders first, highest on top).
/// the renderer iterates this list and renders each camera to its viewport rect.
#[derive(Resource, Default)]
pub struct ActiveViewports {
	pub viewports: Vec<(Entity, ViewportRect)>,
}

/// system that builds the `ActiveViewports` list from all active Camera3d entities.
pub fn update_active_viewports(
	cameras: Query<(Entity, &Camera3d, Option<&ViewportRect>)>,
	mut active: ResMut<ActiveViewports>,
) {
	active.viewports.clear();
	let mut sorted: Vec<_> = cameras.iter().filter(|(_, cam, _)| cam.active).collect();
	sorted.sort_unstable_by_key(|(_, cam, _)| cam.priority);
	for (entity, _, rect) in sorted {
		active
			.viewports
			.push((entity, rect.copied().unwrap_or(ViewportRect::FULL)));
	}
}

/// system that resolves the highest-priority active Camera3d each frame.
pub fn update_active_camera(
	cameras: bevy_ecs::prelude::Query<(bevy_ecs::entity::Entity, &Camera3d)>,
	mut active: bevy_ecs::prelude::ResMut<ActiveCamera3d>,
) {
	let best = cameras
		.iter()
		.filter(|(_, cam)| cam.active)
		.max_by_key(|(_, cam)| cam.priority);
	active.entity = best.map(|(entity, _)| entity);
}

/// ambient (scene-wide) light level.
///
/// added as a resource rather than a component — there is only ever one ambient
/// light. defaults to a dim grey so scenes without explicit lights aren't pitch black.
#[derive(bevy_ecs::prelude::Resource, Debug, Clone, Copy)]
pub struct AmbientLight {
	pub color: lunar_math::Color,
	/// multiplier on top of color channels.
	pub intensity: f32,
}

impl Default for AmbientLight {
	fn default() -> Self {
		Self {
			color: lunar_math::Color::WHITE,
			intensity: 0.05,
		}
	}
}

/// unused parameter placeholder — keeps the Vec3 import used.
const _: Vec3 = Vec3::ZERO;
