use bevy_ecs::prelude::Component;
use lunar_assets::{Asset, Handle, Texture};
use lunar_math::Color;

/// how a surface is shaded.
///
/// matches what the reference engines use:
/// - `Unlit` — no lighting, full-bright color/texture (HUD elements, debug geometry)
/// - `Phong` — classic diffuse + specular, one texture per channel (Quake 3 / Doom 3 baseline)
/// - `Pbr` — metallic-roughness PBR (Halo CE and later, modern target)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShadingModel {
    Unlit,
    #[default]
    Phong,
    Pbr,
}

/// face culling mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CullMode {
    /// cull back faces (standard — solids use this).
    #[default]
    Back,
    /// cull front faces (shadow volumes, portals).
    Front,
    /// no culling (foliage, portals, decals).
    None,
}

/// surface material data.
///
/// defines how a mesh surface responds to light. the render system reads this
/// and selects the appropriate shader pass (one ambient pass + one pass per
/// affecting light, each scissored to the light's screen projection).
///
/// # normal map convention
///
/// normal maps store only the XY tangent-space components in the R and G channels.
/// the Z component is reconstructed in the fragment shader as:
/// `z = sqrt(1.0 - saturate(dot(xy, xy)))`
/// this matches the Doom 3 / id Tech 4 convention and saves one channel for
/// other data (e.g. specular intensity packed into B). do not store Z in textures.
///
/// # lightmap UV
///
/// if `lightmap` is set, the render system samples it using `Vertex3d::uv_lightmap`
/// (the secondary UV channel) and multiplies the result into the diffuse term.
/// this replaces real-time ambient for static geometry in levels.
pub struct MaterialData {
    pub shading: ShadingModel,
    pub cull: CullMode,
    /// base color / albedo. multiplied with the diffuse texture sample.
    pub base_color: Color,
    /// diffuse / albedo texture. none = use base_color alone.
    pub diffuse: Option<Handle<Texture>>,
    /// tangent-space normal map. XY channels only — Z reconstructed in shader.
    /// none = face normals only.
    pub normal_map: Option<Handle<Texture>>,
    /// specular texture:
    /// - phong: intensity/gloss map (greyscale, sampled from B channel)
    /// - pbr: roughness (R) + metallic (G) combined texture
    pub specular: Option<Handle<Texture>>,
    /// phong: specular exponent (shininess). typical range 8–128.
    /// pbr: metallic factor (0.0 = dielectric, 1.0 = full metal).
    pub specular_intensity: f32,
    /// pbr: metallic factor. 0.0 = dielectric (plastic, stone), 1.0 = full metal.
    pub metallic: f32,
    /// pbr: perceptual roughness. 0.04 = mirror-smooth, 1.0 = fully diffuse.
    pub roughness: f32,
    /// baked lightmap for this surface. sampled via uv_lightmap and multiplied
    /// into the ambient term. for dynamic objects, leave as none.
    pub lightmap: Option<Handle<Texture>>,
    /// alpha < 1.0 triggers alpha-blend; 1.0 = opaque.
    pub alpha: f32,
    /// set false for decals, transparent surfaces, and particles.
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
            metallic: 0.0,
            roughness: 0.5,
            lightmap: None,
            alpha: 1.0,
            depth_write: true,
        }
    }
}

impl Asset for MaterialData {}

/// component that references the material used to render this entity's mesh.
#[derive(Debug, Clone, Copy, Component)]
pub struct Material3d(pub Handle<MaterialData>);
