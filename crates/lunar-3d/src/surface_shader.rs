//! surface shader — q3-style multi-stage fixed-function surface description.
//!
//! a `SurfaceShader` component on a `Mesh3d` entity replaces the standard PBR
//! material with a multi-stage blender. each stage samples one texture and
//! blends its result onto the previous stage's output.
//!
//! only applies when `ShadingModel::Unlit` is set on the entity's material
//! (surface shaders are inherently unlit; no PBR interaction).
//!
//! # example — scrolling lava, 2 stages
//!
//! ```ignore
//! commands.spawn((
//!     Mesh3dBundle { /* ... */ },
//!     SurfaceShader {
//!         stages: vec![
//!             SurfaceStage {
//!                 texture: lava_texture,
//!                 blend: BlendMode::Opaque,
//!                 uv_transform: UvTransform { scroll: Vec2::new(0.02, 0.0), ..Default::default() },
//!                 ..Default::default()
//!             },
//!             SurfaceStage {
//!                 texture: lava_glow,
//!                 blend: BlendMode::Add,
//!                 uv_transform: UvTransform { scroll: Vec2::new(-0.01, 0.01), ..Default::default() },
//!                 alpha_gen: AlphaGen::Const(0.5),
//!                 ..Default::default()
//!             },
//!         ],
//!     },
//! ));
//! ```

use bevy_ecs::component::Component;
use lunar_assets::Handle;
use lunar_math::Vec2;

/// component: multi-stage surface shader for a `Mesh3d` entity.
///
/// at most 4 stages are rendered; extra stages are ignored.
/// entity must also have a `Material3d` with `ShadingModel::Unlit`.
#[derive(Debug, Clone, Component)]
pub struct SurfaceShader {
	pub stages: Vec<SurfaceStage>,
}

/// one rendering stage in a surface shader.
#[derive(Debug, Clone)]
pub struct SurfaceStage {
	/// texture sampled in this stage. use a 1×1 white texture for a solid color stage.
	pub texture: Handle<lunar_assets::Texture>,
	/// how this stage's output blends with the previous result.
	pub blend: BlendMode,
	/// per-frame UV animation applied to this stage's texture coordinates.
	pub uv_transform: UvTransform,
	/// how UV coordinates are generated for this stage.
	pub tc_gen: TcGen,
	/// how the alpha value is determined for this stage.
	pub alpha_gen: AlphaGen,
}

impl Default for SurfaceStage {
	fn default() -> Self {
		Self {
			texture: Handle::default(),
			blend: BlendMode::Opaque,
			uv_transform: UvTransform::default(),
			tc_gen: TcGen::Base,
			alpha_gen: AlphaGen::Identity,
		}
	}
}

/// how a stage's output blends with the surface so far.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlendMode {
	/// stage overwrites previous output (no blending). for the first stage or opaque details.
	Opaque,
	/// stage adds its RGB to the previous output. glow and energy effects.
	Add,
	/// stage multiplies its RGB with the previous output. darkening and detail.
	Multiply,
	/// standard alpha blend (stage_rgb * stage_a + prev * (1 - stage_a)).
	AlphaBlend,
}

/// per-stage UV animation evaluated each frame on the CPU.
#[derive(Debug, Clone, Copy)]
pub struct UvTransform {
	/// constant scroll velocity in UV units per second.
	pub scroll: Vec2,
	/// rotation speed in radians per second (about UV center 0.5, 0.5).
	pub rotate: f32,
	/// uniform scale applied to UV coordinates.
	pub scale: f32,
}

impl Default for UvTransform {
	fn default() -> Self {
		Self {
			scroll: Vec2::ZERO,
			rotate: 0.0,
			scale: 1.0,
		}
	}
}

/// how UV coordinates are generated for a stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TcGen {
	/// use the mesh's primary UV coordinates (uv channel 0).
	Base,
	/// use the mesh's lightmap UV coordinates (uv channel 1).
	Lightmap,
}

/// how the alpha value is determined for a stage.
#[derive(Debug, Clone, Copy)]
pub enum AlphaGen {
	/// use the texture's own alpha channel.
	Identity,
	/// constant alpha value in [0, 1].
	Const(f32),
}
