//! surface shader: q3-style multi-stage fixed-function surface description.
//!
//! a `SurfaceShader` component on a `Mesh3d` entity replaces the standard PBR
//! material with a multi-stage blender. each stage samples one texture and
//! blends its result onto the previous stage's output.
//!
//! only applies when `ShadingModel::Unlit` is set on the entity's material
//! (surface shaders are inherently unlit; no PBR interaction).
//!
//! # example: scrolling lava, 2 stages
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

/// marker: draw this surface entity as a screen-space 2d overlay instead of in
/// the world. overlay surfaces are rendered in a dedicated pass with an
/// orthographic projection (their transform is read in a fixed virtual-screen
/// space, not world space) and an independent depth buffer, so they never
/// interact with world geometry: a true flat hud/menu layer on top of the 3d
/// scene. sort within the overlay by the transform's z (nearer wins).
#[derive(Debug, Clone, Copy, Default, Component)]
pub struct Overlay;

/// marker: draw this `Mesh3d` entity as a sky surface. the renderer shades it with
/// the configured `Sky` panorama (sampled per pixel by view direction) while
/// writing real depth, so it occludes any geometry behind it: this reproduces a
/// software renderer's sky ceilings and sky-to-sky upper walls, which hide
/// everything beyond them instead of leaving a see-through hole. entities with
/// this marker carry no `SurfaceShader` or `Material3d`, so the normal opaque pass
/// ignores them; only the dedicated sky pass draws them.
#[derive(Debug, Clone, Copy, Default, Component)]
pub struct SkySurface;

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
	/// binary cutout (q3 alphaFunc GE128): fragments with sampled alpha below
	/// 0.5 are discarded in both the z-prepass and the surface pass, keeping
	/// depth honest for sprites, grates and fences.
	pub alpha_test: bool,
	/// sample this stage's texture with nearest-neighbor filtering instead of
	/// bilinear. crisp texels for low-res pixel art (retro sprites, hud quads).
	pub nearest: bool,
	/// opt this draw out of the classic depth-cued light path. ui overlays
	/// (automap lines, palette tints) want their authored vertex colors kept
	/// verbatim instead of collapsed to a distance-boosted grey. no effect
	/// unless the global classic_light constant is active.
	pub unlit: bool,
	/// per-stage brightness multiplier (0.0..=1.0), re-read every frame so it
	/// can animate without rebuilding geometry: doom's flickering/strobing/
	/// glowing sector lights drive this. 1.0 = the authored color unchanged.
	pub modulate: f32,
}

impl Default for SurfaceStage {
	fn default() -> Self {
		Self {
			texture: Handle::default(),
			blend: BlendMode::Opaque,
			uv_transform: UvTransform::default(),
			tc_gen: TcGen::Base,
			alpha_gen: AlphaGen::Identity,
			alpha_test: false,
			nearest: false,
			unlit: false,
			modulate: 1.0,
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
