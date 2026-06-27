//! engine patch traits: replace built-in render passes without forking.
//!
//! each trait covers one hookable seam in the render pipeline. register an
//! implementation on [`RenderEngine3d`] from a startup system and the engine
//! calls it in place of the built-in logic for that pass.
//!
//! # example: custom shadow technique
//!
//! ```ignore
//! struct FlatShadow;
//!
//! impl ShadowProvider for FlatShadow {
//!     fn render_shadows(&mut self, _context: ShadowCtx<'_>) {
//!         // writes nothing: disables all shadows
//!     }
//! }
//!
//! fn register(world: &mut World) {
//!     world.resource_mut::<RenderEngine3d>()
//!          .set_shadow_provider(FlatShadow);
//! }
//! app.add_startup_system(register);
//! ```

use bevy_ecs::world::World;

// ── shadow ────────────────────────────────────────────────────────────────────

/// input to a custom shadow provider, available each frame.
pub struct ShadowCtx<'a> {
    /// read-only world: query lights, transforms, shadow-caster flags, etc.
    pub world:             &'a World,
    pub device:            &'a wgpu::Device,
    pub queue:             &'a wgpu::Queue,
    /// the shadow atlas that the shading pass samples.
    /// must be filled by the custom provider before returning.
    /// format: `Depth32Float`, `2DArray`, slices = 3 cascades + MAX_POINT_SHADOW_LIGHTS * 6.
    pub shadow_atlas:      &'a wgpu::Texture,
    /// full-array default view of [`shadow_atlas`].
    pub shadow_atlas_view: &'a wgpu::TextureView,
}

/// implement this to replace the built-in cascade + point-light shadow pass.
///
/// the engine calls [`render_shadows`] in place of its own shadow recording.
/// any wgpu work must be submitted before returning (use `context.queue.submit`).
pub trait ShadowProvider: Send + Sync + 'static {
    fn render_shadows(&mut self, context: ShadowCtx<'_>);
}

// ── type-erased storage ───────────────────────────────────────────────────────

pub(crate) struct ShadowHook(pub Box<dyn ShadowProvider>);
