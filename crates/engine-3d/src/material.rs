use bevy_ecs::prelude::Component;
use engine_assets::{Asset, Handle, Texture};
use engine_math::Color;

/// how a surface is shaded.
///
/// matches what the reference engines use:
/// - `Unlit` — no lighting, full-bright color/texture (HUD elements, debug geometry)
/// - `Phong` — classic diffuse + specular, one texture per channel (Quake 3 / Doom 3 baseline)
/// - `Pbr` — metallic-roughness PBR (Halo CE and later, modern target)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShadingModel {
    Unlit,
    Phong,
    Pbr,
}

impl Default for ShadingModel {
    fn default() -> Self {
        Self::Phong
    }
}

/// face culling mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CullMode {
    /// cull back faces (standard — solids use this).
    Back,
    /// cull front faces (shadow volumes, portals).
    Front,
    /// no culling (foliage, portals, decals).
    None,
}

impl Default for CullMode {
    fn default() -> Self {
        Self::Back
    }
}

/// surface material data.
///
/// defines how a mesh surface responds to light. the render system
/// reads this and selects the appropriate shader pass.
///
/// this is the CPU-side description. the render system creates GPU
/// bind groups from it. handles can be null (no texture, fall back to
/// color alone).
pub struct MaterialData {
    pub shading: ShadingModel,
    pub cull: CullMode,
    /// base color / albedo. multiplied with the diffuse texture.
    pub base_color: Color,
    /// diffuse / albedo texture.
    pub diffuse: Option<Handle<Texture>>,
    /// normal map (tangent space). none = flat normals.
    pub normal_map: Option<Handle<Texture>>,
    /// specular texture (phong) or roughness/metallic (pbr).
    pub specular: Option<Handle<Texture>>,
    /// phong: specular exponent. pbr: metallic factor (0.0–1.0).
    pub specular_intensity: f32,
    /// alpha < 1.0 triggers alpha-blend, otherwise opaque.
    pub alpha: f32,
    /// whether to write to depth buffer (set false for decals / transparent surfaces).
    pub depth_write: bool,
}

impl Default for MaterialData {
    fn default() -> Self {
        Self {
            shading: ShadingModel::Phong,
            cull: CullMode::Back,
            base_color: Color::WHITE,
            diffuse: None,
            normal_map: None,
            specular: None,
            specular_intensity: 32.0,
            alpha: 1.0,
            depth_write: true,
        }
    }
}

impl Asset for MaterialData {}

/// component that references the material used to render this entity's mesh.
#[derive(Debug, Clone, Copy, Component)]
pub struct Material3d(pub Handle<MaterialData>);
