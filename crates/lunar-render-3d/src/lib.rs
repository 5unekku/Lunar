//! 3d wgpu renderer for lunar.
//!
//! completely independent of the 2d renderer. owns its own wgpu device, queue,
//! and surface. add [`RenderPlugin3d`] to your app and the renderer handles
//! everything from there.

// on wasm, many native-only items (shadow shaders, mega-buffer constants, etc.)
// have no callers since their use sites are #[cfg(not(wasm32))]. suppress the
// resulting dead_code noise — the items are genuinely used on native.
#![cfg_attr(target_arch = "wasm32", allow(dead_code))]
//!
//! # rendering model
//!
//! - sky pass: skydome + sun disc drawn first, depth write disabled, depth test always passes
//! - opaque pass: all visible `Mesh3d + WorldTransform3d + Material3d` entities, depth write on
//! - shading: unlit only for now (base_color × vertex_color, no lighting calculation)
//!
//! # quick start
//!
//! ```ignore
//! fn setup(mut commands: Commands, mut registry: ResMut<MeshRegistry>) {
//!     let mesh = registry.add_mesh(quad_mesh(2.0, 2.0));
//!     let mat  = registry.add_material(MaterialData { base_color: Color::GREEN, ..default() });
//!     commands.spawn(Mesh3dBundle {
//!         local:    LocalTransform3d::default(),
//!         mesh:     Mesh3d(mesh),
//!         material: Material3d(mat),
//!         ..default()
//!     });
//!     commands.spawn(Camera3dBundle {
//!         local: LocalTransform3d::from_xyz(0.0, 2.0, 8.0),
//!         ..default()
//!     });
//! }
//! ```

pub mod sky;
pub mod render_graph;

pub use sky::{AtmosphericScattering, Sky};

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

use bevy_ecs::prelude::*;
use lunar_3d::{
    Aabb3d, ActiveCamera3d, ActiveViewports, AmbientLight, Camera3d, ComputedVisibility,
    CullSoa, Decal, DetailDensity, DirectionalLight, Frustum, IndexBuffer, IrradianceSH,
    Material3d, Mesh3d, MeshData, MeshImpostor, MeshLod, MeshRegistry, ParticleEmitter,
    PlanarReflector, PointLight, PrevWorldTransform3d, Projection, ShadowCaster, StaticMesh,
    SurfaceShader, Vertex3d, Terrain, ViewportAspect, ViewportRect, Water, WorldTransform3d,
};
use lunar_3d::primitives::{quad_mesh, sphere_mesh};
use lunar_bsp::{Area, BspLevel, VisibleAreas};
use lunar_core::{App, GamePlugin, UpdateStage};
use lunar_lightmap::{DirectionalLightmap, Lightmap};
use lunar_math::{Color, Mat3, Mat4, Vec2, Vec3, Vec3A};

// dev builds and wasm keep wgsl inline; native release uses pre-compiled spirv (build.rs)
#[cfg(any(debug_assertions, target_arch = "wasm32"))]
const SHADER_SRC: &str                 = include_str!("shader.wgsl");
#[cfg(any(debug_assertions, target_arch = "wasm32"))]
const CULL_SHADER_SRC: &str            = include_str!("cull.wgsl");
#[cfg(any(debug_assertions, target_arch = "wasm32"))]
const CULL_INDIRECT_SHADER_SRC: &str   = include_str!("cull_indirect.wgsl");
#[cfg(any(debug_assertions, target_arch = "wasm32"))]
const HZB_SHADER_SRC: &str             = include_str!("hzb.wgsl");
#[cfg(any(debug_assertions, target_arch = "wasm32"))]
const SHADOW_SHADER_SRC: &str          = include_str!("shadow.wgsl");
#[cfg(any(debug_assertions, target_arch = "wasm32"))]
const POINT_SHADOW_SHADER_SRC: &str    = include_str!("point_shadow.wgsl");
#[cfg(any(debug_assertions, target_arch = "wasm32"))]
const CLUSTER_SHADER_SRC: &str         = include_str!("cluster.wgsl");
#[cfg(any(debug_assertions, target_arch = "wasm32"))]
const SURFACE_SHADER_SRC: &str         = include_str!("surface.wgsl");
#[cfg(any(debug_assertions, target_arch = "wasm32"))]
const BLOOM_SHADER_SRC: &str           = include_str!("bloom.wgsl");
#[cfg(any(debug_assertions, target_arch = "wasm32"))]
const COMPOSITE_SHADER_SRC: &str       = include_str!("composite.wgsl");
#[cfg(any(debug_assertions, target_arch = "wasm32"))]
const GTAO_SHADER_SRC: &str            = include_str!("gtao.wgsl");
#[cfg(any(debug_assertions, target_arch = "wasm32"))]
const FXAA_SHADER_SRC: &str            = include_str!("fxaa.wgsl");
#[cfg(any(debug_assertions, target_arch = "wasm32"))]
const SSR_SHADER_SRC: &str             = include_str!("ssr.wgsl");
#[cfg(any(debug_assertions, target_arch = "wasm32"))]
const FOG_SHADER_SRC: &str             = include_str!("volumetric_fog.wgsl");
#[cfg(any(debug_assertions, target_arch = "wasm32"))]
const ATMOS_SHADER_SRC: &str           = include_str!("atmos.wgsl");
#[cfg(any(debug_assertions, target_arch = "wasm32"))]
const UPSCALE_SHADER_SRC: &str         = include_str!("upscale.wgsl");
#[cfg(any(debug_assertions, target_arch = "wasm32"))]
const PARTICLE_SIM_SHADER_SRC: &str    = include_str!("particle_sim.wgsl");
#[cfg(any(debug_assertions, target_arch = "wasm32"))]
const PARTICLE_RENDER_SHADER_SRC: &str = include_str!("particle_render.wgsl");
#[cfg(any(debug_assertions, target_arch = "wasm32"))]
const DECAL_SHADER_SRC: &str           = include_str!("decal.wgsl");
#[cfg(any(debug_assertions, target_arch = "wasm32"))]
const WATER_SHADER_SRC: &str           = include_str!("water.wgsl");
#[cfg(any(debug_assertions, target_arch = "wasm32"))]
const TERRAIN_SHADER_SRC: &str            = include_str!("terrain.wgsl");
#[cfg(any(debug_assertions, target_arch = "wasm32"))]
const CONTACT_SHADOW_SHADER_SRC: &str     = include_str!("contact_shadow.wgsl");
#[cfg(any(debug_assertions, target_arch = "wasm32"))]
const MOTION_VECTOR_SHADER_SRC: &str      = include_str!("motion_vector.wgsl");
#[cfg(any(debug_assertions, target_arch = "wasm32"))]
const DETAIL_SPRITE_SHADER_SRC: &str      = include_str!("detail_sprite.wgsl");
#[cfg(any(debug_assertions, target_arch = "wasm32"))]
const STAA_SHADER_SRC: &str               = include_str!("staa.wgsl");

// staa history is stored at higher precision than the 8-bit swapchain so temporal
// accumulation keeps sub-lsb detail instead of quantizing back to softness each frame.
const STAA_HISTORY_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// create a shader module from pre-compiled spirv (release) or wgsl (debug).
/// in release, `source` is the spirv bytes from OUT_DIR; in debug, the wgsl string.
macro_rules! shader_source {
    ($wgsl_src:ident, $spv_file:literal) => {{
        // wasm/webgpu only accepts wgsl; native release uses pre-compiled spirv
        #[cfg(any(debug_assertions, target_arch = "wasm32"))]
        let src = wgpu::ShaderSource::Wgsl($wgsl_src.into());
        #[cfg(all(not(debug_assertions), not(target_arch = "wasm32")))]
        let src = wgpu::ShaderSource::SpirV(std::borrow::Cow::Borrowed(
            bytemuck::cast_slice::<u8, u32>(include_bytes!(concat!(env!("OUT_DIR"), "/", $spv_file)))
        ));
        src
    }};
}

// method impls split across sibling modules — declared after `shader_source!`
// so the macro is in textual scope for the modules that expand it.
mod init;
mod resources;
mod mesh;
mod cull;
mod passes;
mod post;
mod frame;
mod config;

const SKY_RADIUS: f32 = 900.0;
const SUN_Y: f32 = 895.0;

// quantized gpu vertex: 32 bytes (vs cpu Vertex3d 60 bytes).
// normals/tangents snorm8×4, uvs unorm16×2, position stays f32.
// the upload path converts Vertex3d → GpuVertex3d at upload time.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuVertex3d {
    position:    [f32; 3],  // 12 bytes
    normal:      [i8; 4],   // 4 bytes — snorm8×4, w=0
    tangent:     [i8; 4],   // 4 bytes — snorm8×4, w=handedness (±127)
    uv:          [u16; 2],  // 4 bytes — unorm16×2
    uv_lightmap: [u16; 2],  // 4 bytes — unorm16×2
    color:       [u8; 4],   // 4 bytes
}

const VERTEX_STRIDE: u64 = std::mem::size_of::<GpuVertex3d>() as u64;
/// stride for position-only vertex buffer used by shadow and z-prepass pipelines (f32x3 = 12 bytes).
const POS_VERTEX_STRIDE: u64 = 12;

/// shadow map resolution per cascade.
const SHADOW_MAP_SIZE: u32 = 1024;

/// number of shadow cascades for the directional light.
const NUM_CASCADES: u32 = 3;

/// group 0: view_proj (64) + cam_pos (12) + elapsed (4) + delta (4) + pad (12) = 96 bytes.
const GLOBALS_SIZE: u64 = 96;

/// group 1: base_color (16) + metallic (4) + roughness (4) + flags (4) + has_lightmap (4)
///          + lm_uv_offset (8) + lm_uv_scale (8) = 48 bytes.
const MATERIAL_UNIFORMS_SIZE: u64 = 48;

/// initial size of the mega vertex buffer (16 MB).
const MEGA_VBUF_INIT: u64 = 16 * 1024 * 1024;
/// initial size of the mega index buffer (4 MB, u32 indices).
const MEGA_IBUF_INIT: u64 = 4 * 1024 * 1024;
/// lightmap atlas max side length.
const ATLAS_SIZE: u32 = 4096;

/// per-entity transform data: model mat4 (64) + normal matrix 3×vec4 (48) = 112 bytes,
/// padded to UNIFORM_STRIDE (256) in the staging buffer.
#[allow(dead_code)]
const MESH_UNIFORMS_SIZE: u64 = 112;

/// group 3: ambient(16) + dir(32) + 3×light_space(192) + cascade_splits(16) + sh(160) = 416 bytes.
/// point lights moved to group 5 storage buffer.
const LIGHTS_SIZE: u64 = 416;
/// cluster grid dimensions and per-cluster limits.
const CLUSTER_X: u32 = 16;
const CLUSTER_Y: u32 = 9;
const CLUSTER_Z: u32 = 24;
const NUM_CLUSTERS: usize = (CLUSTER_X * CLUSTER_Y * CLUSTER_Z) as usize;  // 3456
const MAX_LIGHTS_PER_CLUSTER: usize = 32;
/// cluster params uniform size: mat4 (64) + 4×u32 (16) + 4×f32 (16) = 96 bytes.
const CLUSTER_PARAMS_SIZE: u64 = 96;
/// max point lights in the clustered path.
const MAX_CLUSTERED_LIGHTS: usize = 256;
/// max point lights that can cast shadows (cube face layers = MAX_POINT_SHADOW_LIGHTS × 6).
const MAX_POINT_SHADOW_LIGHTS: usize = 4;
/// size of point shadow globals uniform: mat4 (64) + vec3 (12) + f32 (4).
const POINT_SHADOW_GLOBALS_SIZE: u64 = 80;

/// shadow globals: light view-projection mat4 per cascade slot (dynamic offset).
const SHADOW_GLOBALS_SIZE: u64 = 64;

/// maximum point lights uploaded per frame.
const MAX_POINT_LIGHTS: usize = 8;

/// cascade split lambda for logarithmic-linear blending (0=linear, 1=log).
const CASCADE_LAMBDA: f32 = 0.5;

/// near and far planes used for cascade split computation.
const SHADOW_NEAR: f32 = 0.1;
const SHADOW_FAR:  f32 = 200.0;

/// maximum number of bloom mip levels.
const MAX_BLOOM_MIPS: usize = 7;

/// bloom params UBO size: texel_size(8) + filter_radius(4) + threshold(4) = 16 bytes.
const BLOOM_PARAMS_SIZE: u64 = 16;

/// composite params UBO size: 8 × f32 = 32 bytes.
const COMPOSITE_PARAMS_SIZE: u64 = 32;

/// GTAO params UBO size: inv_proj(64) + proj(64) + 8×f32(32) = 160 bytes.
const GTAO_PARAMS_SIZE: u64 = 160;

/// FXAA params UBO: rcp_frame(vec2) + 2 pads = 16 bytes.
const FXAA_PARAMS_SIZE: u64 = 16;

/// TAA params UBO: prev_vp(64) + inv_vp(64) + jitter(8) + rcp_frame(8) + blend_alpha(4)
/// + frame_index(4) + depth_scale(8) + prev_jitter(8) + pad(8) = 176 bytes.
const STAA_PARAMS_SIZE: u64 = 176;

/// SSR params UBO: inv_view_proj(64) + proj(64) + view(64) + misc(32) = 224 bytes.
const SSR_PARAMS_SIZE: u64 = 224;

/// volumetric fog params UBO: inv_view_proj(64) + misc(64) = 128 bytes (std140 aligned).
const FOG_PARAMS_SIZE: u64 = 128;

/// atmospheric scattering params UBO: sun_dir(12)+sun_intensity(4)+rayleigh(12)+mie(4)+scales(16)+radii+exposure+pads = 64 bytes.
const ATMOS_PARAMS_SIZE: u64 = 64;

/// particle sim params UBO: delta_time(4)+gravity(4)+alive_count(4)+pad(4) = 16 bytes.
const PARTICLE_SIM_PARAMS_SIZE: u64 = 16;

/// FSR params UBO: render_w(4)+render_h(4)+display_w(4)+display_h(4)+rcas_sharpness(4)+pad(12) = 32 bytes.
const FSR_PARAMS_SIZE: u64 = 32;
/// contact shadow params UBO: inv_proj(64)+light_dir_vs(12)+step_count(4)+step_size(4)+w(4)+h(4)+pad(4) = 96 bytes.
const CONTACT_SHADOW_PARAMS_SIZE: u64 = 96;

/// motion vector params UBO: inv_view_proj(64)+prev_view_proj(64)+screen_wh(8)+pad(8) = 144 bytes.
const MOTION_VECTOR_PARAMS_SIZE: u64 = 144;

/// one particle in the GPU storage buffer: position(12)+life(4)+vel(12)+maxlife(4)+col_s(16)+col_e(16)+size_s(4)+size_e(4)+pad×2 = 80 bytes.
const PARTICLE_STRIDE: u64 = 80;

/// decal params UBO: decal_inv_world(64)+inv_view_proj(64)+color(16)+decal_world(64)+misc(16) = 224 bytes.
const DECAL_PARAMS_SIZE: u64 = 224;

/// water params UBO: 4×wave(64)+model(64)+water_color(16)+deep_color(16)+misc(32) = 192 bytes.
const WATER_PARAMS_SIZE: u64 = 192;

/// terrain params UBO per ring: ring_origin(16)+terrain_origin(16)+misc(16)+tint(16)+sun_dir(16)+ambient_pad(16) = 96 bytes.
const TERRAIN_PARAMS_SIZE: u64 = 96;

/// stride for dynamic UBO slots — must be ≥ min_uniform_buffer_offset_alignment (256).
const UNIFORM_STRIDE: u64 = 256;

/// initial number of slots (dome + sun + entities) in the entity uniform buffer.
const INITIAL_ENTITY_CAPACITY: usize = 64;

/// fixed slot index for the sky dome.
const SLOT_DOME: usize = 0;
/// fixed slot index for the sun.
const SLOT_SUN: usize = 1;
/// first slot index used for scene entities.
const ENTITY_SLOT_START: usize = 2;

/// surface stage data packed for the GPU (matches StageData in surface.wgsl, 32 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
struct SurfaceStagePacked {
    uv_offset: [f32; 2],
    uv_scale:  f32,
    blend:     u32,
    alpha:     f32,
    use_lm_uv: u32,
    enabled:   u32,
    _pad:      u32,
}

// ── gpu types ──────────────────────────────────────────────────────────────

struct GpuMesh {
    vbuf: wgpu::Buffer,
    /// positions-only (f32x3, 12 bytes/vertex) — bound in shadow and z-prepass pipelines
    pos_buf: wgpu::Buffer,
    ibuf: wgpu::Buffer,
    index_count: u32,
    index_fmt: wgpu::IndexFormat,
}

/// per-particle GPU layout — must match the WGSL Particle struct exactly.
#[repr(C)]
struct GpuParticle {
    position:     [f32; 3],
    lifetime:     f32,
    velocity:     [f32; 3],
    max_lifetime: f32,
    color_start:  [f32; 4],
    color_end:    [f32; 4],
    size_start:   f32,
    size_end:     f32,
    _pad0:        f32,
    _pad1:        f32,
}

/// CPU-side particle tracking for spawn management.
struct CpuParticle {
    position:     Vec3,
    velocity:     Vec3,
    lifetime:     f32,
    max_lifetime: f32,
    color_start:  [f32; 4],
    color_end:    [f32; 4],
    size_start:   f32,
    size_end:     f32,
    alive:        bool,
}

impl CpuParticle {
    fn dead() -> Self {
        Self {
            position: Vec3::ZERO,
            velocity: Vec3::ZERO,
            lifetime: 0.0,
            max_lifetime: 1.0,
            color_start: [0.0; 4],
            color_end: [0.0; 4],
            size_start: 0.0,
            size_end: 0.0,
            alive: false,
        }
    }

    fn as_gpu(&self) -> GpuParticle {
        GpuParticle {
            position: [self.position.x, self.position.y, self.position.z],
            lifetime: self.lifetime,
            velocity: [self.velocity.x, self.velocity.y, self.velocity.z],
            max_lifetime: self.max_lifetime,
            color_start: self.color_start,
            color_end: self.color_end,
            size_start: self.size_start,
            size_end: self.size_end,
            _pad0: 0.0,
            _pad1: 0.0,
        }
    }
}

/// GPU-side terrain resources per terrain entity.
#[allow(dead_code)]
struct TerrainGpu {
    heightmap_tex:  wgpu::Texture,
    heightmap_view: wgpu::TextureView,
    // clipmap ring meshes: index 0 = center patch, 1..N = rings (coarsest last)
    ring_meshes: Vec<GpuMesh>,
    params_buf: wgpu::Buffer,
    params_bg: wgpu::BindGroup,
    hmap_sampler: wgpu::Sampler,
}

// ── render tier ────────────────────────────────────────────────────────────

/// detected rendering capability tier.
///
/// queried from the wgpu adapter at startup. gates features that require
/// compute shaders or indirect drawing. inserted as a `Resource` by
/// [`RenderPlugin3d`] so game systems can query it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Resource)]
pub enum RenderTier {
    /// GLES / Pi 4 floor: no compute shaders, forward only.
    LowGles,
    /// compute available, no multi-draw-indirect (Metal, most Vulkan).
    Mid,
    /// full: compute + indirect execution (Vulkan/DX12 desktop).
    High,
}

impl RenderTier {
    fn from_adapter(adapter: &wgpu::Adapter) -> Self {
        let flags = adapter.get_downlevel_capabilities().flags;
        if !flags.contains(wgpu::DownlevelFlags::COMPUTE_SHADERS) {
            Self::LowGles
        } else if flags.contains(wgpu::DownlevelFlags::INDIRECT_EXECUTION) {
            Self::High
        } else {
            Self::Mid
        }
    }
}

// ── quality settings ──────────────────────────────────────────────────────

/// coarse quality tier. individual toggles in `QualitySettings` can be
/// overridden independently of the preset.
///
/// `Minimum` is the accessibility-first preset: every post-processing pass is off,
/// shadow cost is minimal, no MSAA. targets 60fps on any modern CPU regardless of GPU.
/// this is the floor every game should support before adding quality options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualityPreset { Minimum, Low, Medium, High, Ultra }

/// upscaling algorithm used when `render_scale < 1.0`.
///
/// only active when the surface renders at reduced resolution. at `render_scale = 1.0`
/// the upscale pass is skipped entirely regardless of this setting.
///
/// game devs can force a specific mode via [`DevRenderProfile::forced_upscale_mode`].
/// users control it through [`QualitySettings::upscale_mode`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpscaleMode {
    /// integer-aligned point sampling — zero blur, correct for pixel art at integer scales.
    Nearest,
    /// hardware bilinear — essentially free, acceptable general-purpose quality.
    Linear,
    /// Lanczos-2 — sharper than bilinear, preserves fine detail better.
    Lanczos,
    /// Mitchell-Netravali bicubic — smooth upscaling, good for 2D and UI-heavy content.
    Bicubic,
    /// FSR 3 EASU + RCAS — edge-adaptive spatial upsampling with contrast sharpening.
    /// best quality for rendered 3D content; two-pass algorithm.
    Fsr3,
}

/// per-feature quality knobs. inserted as a resource by [`RenderPlugin3d`]
/// using defaults derived from the detected [`RenderTier`].
///
/// game code can override individual fields after plugin init.
#[derive(Resource, Clone)]
pub struct QualitySettings {
    pub preset: QualityPreset,
    /// shadow map resolution per cascade side (pixels).
    pub shadow_res: u32,
    /// shadow map resolution per point light cube face (pixels). lower than shadow_res is fine.
    /// user-selectable: 256 / 512 / 1024. default 512.
    pub point_shadow_res: u32,
    /// number of shadow cascades (1 on Low, 3 on Mid/High).
    pub shadow_cascades: u32,
    /// msaa sample count: 1 = off, 4 = 4× (applied on Mid/High).
    pub msaa_samples: u32,
    /// enable the bloom post-pass.
    pub bloom: bool,
    /// number of bloom downsample mip levels (3 Low, 5 Mid, 7 High).
    pub bloom_mips: u32,
    /// enable half-res GTAO ambient occlusion (Mid/High only).
    pub ssao: bool,
    /// enable screen-space vignette in the composite pass.
    pub vignette: bool,
    /// enable chromatic aberration in the composite pass.
    pub chromatic_aberration: bool,
    /// enable film grain in the composite pass.
    pub film_grain: bool,
    /// maximum live particles.
    pub particle_cap: u32,
    /// enable FXAA post-process AA. recommended on low tier (no MSAA).
    /// mid/high tier uses MSAA instead and leaves this off by default.
    pub fxaa: bool,
    /// enable selective TAA: stabilizes shimmer and accumulates jitter-based sub-pixel AA.
    /// only modifies edge-adjacent and shimmering pixels — smooth surfaces pass through unchanged.
    /// runs on top of MSAA on mid/high tier. requires compute (non-GLES).
    pub staa: bool,
    /// enable quarter-res screen-space reflections (mid+ tier).
    pub ssr: bool,
    /// enable quarter-res ray-marched volumetric fog (mid+ tier).
    pub volumetric_fog: bool,
    /// render resolution scale factor. 1.0 = native; 0.75 = 75% width/height (56% pixel count).
    /// when < 1.0, the full render chain runs at reduced size and the upscale pass runs.
    pub render_scale: f32,
    /// upscaling algorithm used when render_scale < 1.0.
    /// has no effect at native resolution. see [`UpscaleMode`] for options.
    /// game devs can override this via [`DevRenderProfile::forced_upscale_mode`].
    pub upscale_mode: UpscaleMode,
}

impl QualitySettings {
    /// accessibility-first preset: all post-processing off, single shadow cascade,
    /// no MSAA. every game should support this tier. corresponds to `QualityPreset::Minimum`.
    #[must_use]
    pub fn minimum() -> Self {
        Self {
            preset: QualityPreset::Minimum,
            shadow_res: 512,
            point_shadow_res: 256,
            shadow_cascades: 1,
            msaa_samples: 1,
            bloom: false,
            bloom_mips: 3,
            ssao: false,
            vignette: false,
            chromatic_aberration: false,
            film_grain: false,
            particle_cap: 512,
            fxaa: false,
            staa: false,
            ssr: false,
            volumetric_fog: false,
            render_scale: 1.0,
            upscale_mode: UpscaleMode::Nearest,
        }
    }

    /// everything on, highest fidelity. useful for smoke tests and screenshot tools.
    #[must_use]
    pub fn maximum() -> Self {
        Self::from_tier(RenderTier::High)
    }

    /// build settings for a given tier, overriding the preset.
    /// useful for adaptive quality stepping: tier determines hardware capabilities,
    /// preset determines feature toggles within that capability.
    #[must_use]
    pub fn from_tier_and_preset(tier: RenderTier, preset: QualityPreset) -> Self {
        let mut base = Self::from_tier(tier);
        base.preset = preset;
        // override feature toggles that depend on preset, not tier
        match preset {
            QualityPreset::Minimum => {
                base.msaa_samples = 1;
                base.bloom = false;
                base.ssao = false;
                base.ssr = false;
                base.volumetric_fog = false;
                base.vignette = false;
                base.chromatic_aberration = false;
                base.film_grain = false;
                base.fxaa = false;
                base.staa = false;
                base.shadow_cascades = 1;
                base.render_scale = 1.0;
                base.upscale_mode = UpscaleMode::Nearest;
            }
            QualityPreset::Low => {
                base.msaa_samples = 1;
                base.bloom = false;
                base.ssao = false;
                base.ssr = false;
                base.volumetric_fog = false;
                base.fxaa = true;
                base.staa = false;
                base.shadow_cascades = 1;
            }
            QualityPreset::Medium => {
                base.msaa_samples = if tier == RenderTier::LowGles { 1 } else { 4 };
            }
            QualityPreset::High | QualityPreset::Ultra => {}
        }
        base
    }

    #[must_use]
    pub fn from_tier(tier: RenderTier) -> Self {
        match tier {
            RenderTier::LowGles => Self {
                preset: QualityPreset::Low,
                shadow_res: 512,
                point_shadow_res: 256,
                shadow_cascades: 1,
                msaa_samples: 1,
                bloom: false,
                bloom_mips: 3,
                ssao: false,
                vignette: false,
                chromatic_aberration: false,
                film_grain: false,
                particle_cap: 1024,
                fxaa: true, // no MSAA on low tier — FXAA is the only AA path
                staa: false,
                ssr: false,
                volumetric_fog: false,
                render_scale: 1.0,
                upscale_mode: UpscaleMode::Nearest,
            },
            RenderTier::Mid => Self {
                preset: QualityPreset::Medium,
                shadow_res: 1024,
                point_shadow_res: 512,
                shadow_cascades: 3,
                msaa_samples: 4,
                bloom: true,
                bloom_mips: 5,
                ssao: true,
                vignette: true,
                chromatic_aberration: false,
                film_grain: false,
                particle_cap: 8192,
                fxaa: false, // 4× MSAA is active — FXAA redundant
                staa: true,   // selective temporal AA on top of MSAA: shimmer + sub-pixel jitter
                ssr: true,
                volumetric_fog: true,
                render_scale: 1.0,
                upscale_mode: UpscaleMode::Nearest,
            },
            RenderTier::High => Self {
                preset: QualityPreset::High,
                shadow_res: 2048,
                point_shadow_res: 512,
                shadow_cascades: 3,
                msaa_samples: 4,
                bloom: true,
                bloom_mips: 7,
                ssao: true,
                vignette: true,
                chromatic_aberration: true,
                film_grain: true,
                particle_cap: 32768,
                fxaa: false, // 4× MSAA active
                staa: true,
                ssr: true,
                volumetric_fog: true,
                render_scale: 1.0,
                upscale_mode: UpscaleMode::Nearest,
            },
        }
    }

    /// clamp user quality settings against a dev profile ceiling.
    ///
    /// call this after constructing `QualitySettings` and before inserting it as a resource.
    /// features the dev disabled stay off regardless of tier; msaa and cascade counts are capped.
    #[must_use]
    pub fn apply_dev_profile(mut self, dev: &DevRenderProfile) -> Self {
        if !dev.shadows { self.shadow_cascades = 0; }
        self.shadow_cascades = self.shadow_cascades.min(dev.max_shadow_cascades);
        if !dev.bloom          { self.bloom = false; }
        if !dev.ssao           { self.ssao = false; }
        if !dev.ssr            { self.ssr = false; }
        if !dev.volumetric_fog { self.volumetric_fog = false; }
        if !dev.fxaa           { self.fxaa = false; }
        if !dev.staa            { self.staa = false; }
        if !dev.vignette       { self.vignette = false; }
        if !dev.chromatic_aberration { self.chromatic_aberration = false; }
        if !dev.film_grain     { self.film_grain = false; }
        self.msaa_samples = self.msaa_samples.min(dev.max_msaa);
        self.particle_cap = self.particle_cap.min(dev.max_particles);
        if let Some(mode) = dev.forced_upscale_mode { self.upscale_mode = mode; }
        // soft_shadows and contact_shadows are read directly from DevRenderProfile at render time
        self
    }
}

/// automatic quality stepping based on frame time EMA.
///
/// when enabled, the renderer steps `QualityPreset` down after 3 seconds over
/// budget and up after 10 seconds under budget. respects `min` and `max` bounds.
/// insert into the app alongside `RenderPlugin3d` to enable.
#[derive(Resource, Clone)]
pub struct AutoQuality {
    pub enabled: bool,
    pub min: QualityPreset,
    pub max: QualityPreset,
}

impl Default for AutoQuality {
    fn default() -> Self {
        Self { enabled: false, min: QualityPreset::Minimum, max: QualityPreset::Ultra }
    }
}

// ── dev render profile ────────────────────────────────────────────────────

/// what the game is designed to use — a per-feature ceiling the developer controls.
///
/// `QualitySettings` is the user's slider within this ceiling. `DevRenderProfile`
/// is the developer's decision about the game's visual design. they are orthogonal:
/// a developer building a Quake-style game inserts `DevRenderProfile::classic()` and
/// users scale shadow resolution and MSAA without ever enabling SSAO or bloom.
/// a developer building a photorealistic game uses `DevRenderProfile::default()` (all on)
/// and users can turn features off but the dev's artistic intent is the ceiling.
///
/// the renderer takes `min(user_settings, dev_profile)` each frame. features disabled
/// here are never executed regardless of user or hardware tier.
///
/// insert as a resource before adding `RenderPlugin3d`. if not inserted, `default()` is
/// used — every feature the hardware supports is available to the user.
#[derive(Resource, Clone)]
pub struct DevRenderProfile {
    /// real-time cascaded shadow maps. disable for fully lightmapped games (quake-style).
    pub shadows: bool,
    /// bloom post-pass. disable for games that want a clean raster look.
    pub bloom: bool,
    /// half-res GTAO ambient occlusion.
    pub ssao: bool,
    /// screen-space reflections.
    pub ssr: bool,
    /// ray-marched volumetric fog.
    pub volumetric_fog: bool,
    /// FXAA post-process AA.
    pub fxaa: bool,
    /// selective temporal AA. off by default for classic profiles; on for standard/full.
    pub staa: bool,
    /// screen-space vignette.
    pub vignette: bool,
    /// chromatic aberration.
    pub chromatic_aberration: bool,
    /// film grain overlay.
    pub film_grain: bool,
    /// maximum shadow cascades the game will use (developer ceiling, 1–3).
    /// independently from whether shadows are on, this caps the cascade count
    /// regardless of what the user's quality preset requests.
    pub max_shadow_cascades: u32,
    /// maximum MSAA sample count the game supports (1, 2, 4, or 8).
    pub max_msaa: u32,
    /// maximum particle cap. keep low for simple retro games, high for effects-heavy titles.
    pub max_particles: u32,
    /// point light cube shadow maps. off by default — enable for doom/hl2 style flashlight games.
    /// requires `with_point_light_shadows(true)` since `classic()` leaves this off.
    pub point_light_shadows: bool,
    /// maximum point lights in the scene. classic/standard cap at 8; full allows up to 256
    /// using the clustered forward path (requires high tier with compute shaders).
    pub max_point_lights: u32,
    /// pcss for directional shadows (variable-width penumbra) + pcf for point shadows.
    /// standard/classic default off; full default on.
    pub soft_shadows: bool,
    /// screen-space contact shadow raymarch under objects to fill shadow-map contact gaps.
    /// adds ~0.1ms at 1080p. standard/classic off; full on.
    pub contact_shadows: bool,
    /// force a specific upscaling algorithm regardless of user quality settings.
    /// `None` = let the user's `QualitySettings::upscale_mode` decide.
    /// example: `Some(UpscaleMode::Nearest)` for pixel art games.
    pub forced_upscale_mode: Option<UpscaleMode>,
}

impl Default for DevRenderProfile {
    /// defaults to `classic()` — no runtime lighting, no post-processing.
    /// the cheapest possible starting point. devs opt in to complexity rather than
    /// opting out of it. this matches the accessibility goal.
    fn default() -> Self { Self::classic() }
}

impl DevRenderProfile {
    /// lightmapped game with no post-processing: quake 1 / quake 3 style.
    /// shadows baked into lightmaps, no bloom, no SSAO, no SSR, no fog.
    /// user can still scale resolution and MSAA.
    #[must_use]
    pub fn classic() -> Self {
        Self {
            shadows: false,
            bloom: false,
            ssao: false,
            ssr: false,
            volumetric_fog: false,
            fxaa: true,
            staa: true,
            vignette: false,
            chromatic_aberration: false,
            film_grain: false,
            max_shadow_cascades: 1,
            max_msaa: 8,
            max_particles: 8192,
            point_light_shadows: false,
            max_point_lights: 8,
            soft_shadows: false,
            contact_shadows: false,
            forced_upscale_mode: None,
        }
    }

    /// shadows + bloom, no SSAO/SSR/fog. halo CE / mid-2000s feel.
    /// good baseline for most indie 3d games that want some dynamism without full pbr cost.
    #[must_use]
    pub fn standard() -> Self {
        Self {
            shadows: true,
            bloom: true,
            ssao: false,
            ssr: false,
            volumetric_fog: false,
            fxaa: true,
            staa: true,
            vignette: true,
            chromatic_aberration: false,
            film_grain: false,
            max_shadow_cascades: 3,
            max_msaa: 8,
            max_particles: 32768,
            point_light_shadows: false,
            max_point_lights: 8,
            soft_shadows: false,
            contact_shadows: false,
            forced_upscale_mode: None,
        }
    }

    /// everything on — full modern pipeline. use for photorealistic / high-budget titles.
    /// user can turn individual features off but this is the ceiling.
    #[must_use]
    pub fn full() -> Self {
        Self {
            shadows: true,
            bloom: true,
            ssao: true,
            ssr: true,
            volumetric_fog: true,
            fxaa: true,
            staa: true,
            vignette: true,
            chromatic_aberration: true,
            film_grain: true,
            max_shadow_cascades: 3,
            max_msaa: 8,
            max_particles: u32::MAX,
            point_light_shadows: true,
            max_point_lights: 256,
            soft_shadows: true,
            contact_shadows: true,
            forced_upscale_mode: None,
        }
    }

    // ── builder methods ───────────────────────────────────────────────────
    // each returns Self so they chain: DevRenderProfile::classic().with_bloom().with_shadows()

    #[must_use] pub fn with_point_light_shadows(mut self, enabled: bool) -> Self { self.point_light_shadows = enabled; self }
    #[must_use] pub fn with_max_point_lights(mut self, count: u32) -> Self { self.max_point_lights = count; self }
    #[must_use] pub fn with_soft_shadows(mut self, enabled: bool) -> Self { self.soft_shadows = enabled; self }
    #[must_use] pub fn with_contact_shadows(mut self, enabled: bool) -> Self { self.contact_shadows = enabled; self }
    #[must_use] pub fn with_shadows(mut self, enabled: bool) -> Self { self.shadows = enabled; self }
    #[must_use] pub fn with_bloom(mut self, enabled: bool) -> Self { self.bloom = enabled; self }
    #[must_use] pub fn with_ssao(mut self, enabled: bool) -> Self { self.ssao = enabled; self }
    #[must_use] pub fn with_ssr(mut self, enabled: bool) -> Self { self.ssr = enabled; self }
    #[must_use] pub fn with_volumetric_fog(mut self, enabled: bool) -> Self { self.volumetric_fog = enabled; self }
    #[must_use] pub fn with_fxaa(mut self, enabled: bool) -> Self { self.fxaa = enabled; self }
    #[must_use] pub fn with_staa(mut self, enabled: bool) -> Self { self.staa = enabled; self }
    #[must_use] pub fn with_vignette(mut self, enabled: bool) -> Self { self.vignette = enabled; self }
    #[must_use] pub fn with_chromatic_aberration(mut self, enabled: bool) -> Self { self.chromatic_aberration = enabled; self }
    #[must_use] pub fn with_film_grain(mut self, enabled: bool) -> Self { self.film_grain = enabled; self }
    #[must_use] pub fn with_max_shadow_cascades(mut self, count: u32) -> Self { self.max_shadow_cascades = count; self }
    #[must_use] pub fn with_max_msaa(mut self, samples: u32) -> Self { self.max_msaa = samples; self }
    #[must_use] pub fn with_max_particles(mut self, cap: u32) -> Self { self.max_particles = cap; self }
}

// ── render config ──────────────────────────────────────────────────────────

/// window and loop configuration for a 3D game.
///
/// passed to [`bootstrap_3d`](lunar::bootstrap_3d) at startup. the render engine
/// and game loop are created from these settings before the first frame.
#[derive(Clone)]
pub struct RenderConfig3d {
    /// initial window width in pixels.
    pub width: u32,
    /// initial window height in pixels.
    pub height: u32,
    /// enable vsync. set false to uncap framerate (useful for benchmarking).
    pub vsync: bool,
    /// target render frame cap (0 = uncapped/vsync-limited)
    pub frame_cap: u32,
    /// fixed logic tick rate — independent of render frame rate
    pub tick_rate: lunar_core::TickRate,
    /// window title bar text.
    pub title: String,
    /// fixed aspect ratio. when set, the window snaps on resize to maintain this ratio.
    /// expressed as width/height (e.g. `16.0/9.0`). `None` = free aspect ratio.
    pub target_aspect: Option<f32>,
    /// whether the window is resizable. true by default.
    pub allow_resize: bool,
}

impl Default for RenderConfig3d {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
            vsync: true,
            frame_cap: 0,
            tick_rate: lunar_core::TickRate::Hz60,
            title: "Lunar".to_string(),
            target_aspect: None,
            allow_resize: true,
        }
    }
}

impl RenderConfig3d {
    /// the loop-timing parameters ([`frame_cap`](Self::frame_cap) +
    /// [`tick_rate`](Self::tick_rate)) as a [`LoopConfig`](lunar_core::LoopConfig)
    /// for [`App::run`](lunar_core::App::run).
    #[must_use]
    pub fn loop_config(&self) -> lunar_core::LoopConfig {
        lunar_core::LoopConfig { frame_cap: self.frame_cap, tick_rate: self.tick_rate }
    }
}

// ── render info ────────────────────────────────────────────────────────────

/// per-frame rendering statistics. updated by `render_3d_system`.
#[derive(Resource)]
pub struct RenderInfo3d {
    pub window_width: u32,
    pub window_height: u32,
    pub draw_calls: u32,
    pub fps: f32,
    pub frame_time_ms: f32,
    /// current render resolution scale (1.0 = native, 0.5 = half resolution).
    /// adjusted automatically by the dynamic resolution scaler.
    pub resolution_scale: f32,
}

impl Default for RenderInfo3d {
    fn default() -> Self {
        Self {
            window_width: 0,
            window_height: 0,
            draw_calls: 0,
            fps: 0.0,
            frame_time_ms: 0.0,
            resolution_scale: 1.0,
        }
    }
}

// ── render engine ──────────────────────────────────────────────────────────

/// read-only per-frame state shared across the render passes. built once near the
/// end of render_frame and passed to extracted `record_*` pass methods. all Copy.
struct FrameContext {
    view_proj: Mat4,
    staa_jitter_ndc: Vec2,
    cam_pos: Vec3,
    cam_wt: WorldTransform3d,
    dev_bloom: bool,
    dev_ssao: bool,
    dev_ssr: bool,
    dev_fog: bool,
    dev_fxaa: bool,
    dev_staa: bool,
    dev_vignette: bool,
    dev_chrom_ab: bool,
    dev_film_grain: bool,
    dev_contact_shadows: bool,
    upscale_mode: UpscaleMode,
    dir_color: Color,
    dir_direction: Vec3,
    sky_color: Color,
    sky: Option<Sky>,
    dir_illuminance: f32,
    dir_enabled: u32,
    vp_x: u32,
    vp_y: u32,
    vp_w: u32,
    vp_h: u32,
    aspect: f32,
    camera: Camera3d,
}

/// per-detail-density-entity GPU resources, cached across frames.
///
/// buffers and bind groups are persistent; only `params_buf` is re-written each frame
/// and the bind groups are rebuilt when the resolved density/atlas texture view changes
/// (tracked via `density_key`/`atlas_key` — `(texture id, uploaded yet?)`).
struct DetailSpriteEntry {
    inst_buf:   wgpu::Buffer,
    count_buf:  wgpu::Buffer,
    draw_buf:   wgpu::Buffer,
    params_buf: wgpu::Buffer,
    compute_bg: wgpu::BindGroup,
    render_bg:  wgpu::BindGroup,
    density_key: (u32, bool),
    atlas_key:   (u32, bool),
}

/// the 3d rendering engine. owns the wgpu device, queue, and surface.
///
/// inserted as a resource by [`RenderPlugin3d`]. game code should not
/// interact with this directly — use [`MeshRegistry`] and ECS components instead.
#[derive(Resource)]
#[allow(dead_code)]
pub struct RenderEngine3d {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    render_tier: RenderTier,
    hdr_format: wgpu::TextureFormat,

    // current render resolution (= display * render_scale). equals display when render_scale = 1.0.
    render_w: u32,
    render_h: u32,
    // active render scale; when this changes render-resolution textures are recreated.
    render_scale: f32,

    msaa_samples: u32,
    depth_view: wgpu::TextureView,
    // some when msaa_samples > 1; render target for color pass, resolved to swapchain
    msaa_color_view: Option<wgpu::TextureView>,

    // group 0: view-global (camera view-proj + time)
    globals_buf: wgpu::Buffer,
    globals_bg: wgpu::BindGroup,
    globals_bgl: wgpu::BindGroupLayout,

    // group 1: material (base_color — dynamic UBO, one slot per draw call)
    material_bgl: wgpu::BindGroupLayout,
    material_buf: wgpu::Buffer,
    material_bg: wgpu::BindGroup,
    material_staging: Vec<u8>,

    // group 2: per-mesh (model matrix — dynamic UBO, one slot per draw call)
    mesh_bgl: wgpu::BindGroupLayout,
    entity_buf: wgpu::Buffer,
    entity_bg: wgpu::BindGroup,
    entity_capacity: usize,

    opaque_pipeline: wgpu::RenderPipeline,  // depth_compare=LessEqual when z-prepass active
    sky_pipeline: wgpu::RenderPipeline,
    zprepass_pipeline: wgpu::RenderPipeline, // depth-only pre-pass (mid/high tier)

    // group 3: lights uniform + shadow map
    lights_bgl: wgpu::BindGroupLayout,
    lights_buf: wgpu::Buffer,
    lights_bg: wgpu::BindGroup,
    shadow_map_view: wgpu::TextureView,
    shadow_sampler: wgpu::Sampler,

    // shadow pass — 3 cascades, each a layer of the array depth texture
    shadow_globals_buf: wgpu::Buffer,           // 3 × UNIFORM_STRIDE slots
    shadow_globals_bgl: wgpu::BindGroupLayout,
    shadow_globals_bg: wgpu::BindGroup,         // bound with dynamic offset per cascade
    shadow_pipeline: wgpu::RenderPipeline,
    shadow_cascade_views: [wgpu::TextureView; 3], // per-cascade render attachment views

    // dirty-flag shadow cascade re-rendering.
    // cascade N is only re-rendered when shadow_cascade_dirty[N] is true.
    shadow_cascade_dirty: [bool; 3],
    shadow_last_dir: Vec3,
    shadow_last_draw_count: usize,

    // point light cube shadow maps — 4 lights × 6 faces as 24-layer depth 2D array.
    // layer = shadow_index * 6 + face; face order: 0=+X, 1=-X, 2=+Y, 3=-Y, 4=+Z, 5=-Z.
    point_shadow_tex: wgpu::Texture,
    point_shadow_face_views: Vec<wgpu::TextureView>,
    point_shadow_array_view: wgpu::TextureView,
    point_shadow_globals_bgl: wgpu::BindGroupLayout,
    point_shadow_globals_buf: wgpu::Buffer,
    point_shadow_globals_bg: wgpu::BindGroup,
    point_shadow_pipeline: wgpu::RenderPipeline,
    point_shadow_dirty: [[bool; 6]; MAX_POINT_SHADOW_LIGHTS],
    point_shadow_last_positions: [Vec3; MAX_POINT_SHADOW_LIGHTS],
    point_shadow_last_draw_count: usize,

    // clustered forward lighting (group 5)
    cluster_shader_src_loaded: bool,  // sentinel; real init happens on first use
    cluster_bgl_compute: wgpu::BindGroupLayout,
    cluster_bgl_render: wgpu::BindGroupLayout,
    cluster_pipeline: wgpu::ComputePipeline,
    cluster_params_buf: wgpu::Buffer,         // ClusterParams uniform (96 bytes)
    light_list_buf: wgpu::Buffer,             // PointLightEntry × 256 (storage)
    cluster_counts_buf: wgpu::Buffer,         // u32 × NUM_CLUSTERS (atomic in compute)
    cluster_indices_buf: wgpu::Buffer,        // u32 × NUM_CLUSTERS × MAX_PER_CLUSTER
    cluster_bg_compute: wgpu::BindGroup,      // group 0 for compute pass
    cluster_bg_render: wgpu::BindGroup,       // group 5 for render passes

    // q3-style multi-stage surface shader (Item C)
    surface_bgl: wgpu::BindGroupLayout,
    surface_pipeline: wgpu::RenderPipeline,
    surface_fallback_tex: wgpu::Texture,
    surface_fallback_view: wgpu::TextureView,
    surface_sampler: wgpu::Sampler,
    surface_params_buf: wgpu::Buffer,           // UNIFORM_STRIDE per entity, up to 64 surface entities
    surface_tex_cache: HashMap<u32, (wgpu::Texture, wgpu::TextureView)>,
    surface_bg_cache: HashMap<[u32; 4], wgpu::BindGroup>,
    // (entity, instance_slot, [tex_id; 4], packed stages × 4)
    surface_scratch: Vec<(bevy_ecs::entity::Entity, usize, [u32; 4], [SurfaceStagePacked; 4])>,

    mesh_gpu: HashMap<u32, GpuMesh>,
    dome_mesh: GpuMesh,
    sun_mesh: GpuMesh,

    // HDR render target — color pass writes here; bloom + composite read it
    hdr_texture: wgpu::Texture,
    hdr_view: wgpu::TextureView,

    // bloom pipeline: downsample + upsample mip chain
    bloom_enabled: bool,
    bloom_mip_views: Vec<wgpu::TextureView>,      // one per mip level
    bloom_mip_sizes: Vec<(u32, u32)>,             // (width, height) per mip
    bloom_params_buf: wgpu::Buffer,               // MAX_BLOOM_MIPS × UNIFORM_STRIDE slots
    bloom_downsample_bgl: wgpu::BindGroupLayout,
    bloom_downsample_bgs: Vec<wgpu::BindGroup>,   // one per downsample step
    bloom_upsample_bgs: Vec<wgpu::BindGroup>,     // one per upsample step
    bloom_downsample_pipeline: wgpu::RenderPipeline,
    bloom_upsample_pipeline: wgpu::RenderPipeline,

    // composite pipeline: HDR + bloom → ACES + post → swapchain
    composite_params_buf: wgpu::Buffer,
    composite_bgl: wgpu::BindGroupLayout,
    composite_bg: wgpu::BindGroup,
    composite_pipeline: wgpu::RenderPipeline,
    post_sampler: wgpu::Sampler,

    // GTAO: half-res ambient occlusion
    ssao_enabled: bool,
    // non-MSAA z-prepass depth used as GTAO input (can't sample MSAA depth)
    gtao_depth_view: wgpu::TextureView,
    gtao_ao_a: wgpu::Texture,        // ping-pong target A (AO result)
    gtao_ao_b: wgpu::Texture,        // ping-pong target B (blur intermediate)
    gtao_ao_view_a: wgpu::TextureView,
    gtao_ao_view_b: wgpu::TextureView,
    gtao_params_buf: wgpu::Buffer,
    gtao_bgl: wgpu::BindGroupLayout,
    gtao_point_sampler: wgpu::Sampler,
    gtao_main_bg: wgpu::BindGroup,   // depth → ao_a
    gtao_blur_h_bg: wgpu::BindGroup, // ao_a → ao_b
    gtao_blur_v_bg: wgpu::BindGroup, // ao_b → ao_a (final result in ao_a)
    gtao_pipeline: wgpu::RenderPipeline,
    gtao_blur_h_pipeline: wgpu::RenderPipeline,
    gtao_blur_v_pipeline: wgpu::RenderPipeline,
    // second depth-only pass at sample_count=1 for GTAO depth input
    zprepass_nonmsaa_pipeline: wgpu::RenderPipeline,

    // dynamic resolution scaling — EMA of frame time drives scale adjustments
    frame_time_ema_ms: f32,
    resolution_scale: f32,       // current scale factor [0.5, 1.0]
    frame_time_budget_ms: f32,   // target frame time (e.g. 14 ms for 60 fps)
    // adaptive quality stepping: consecutive frames over/under budget
    auto_quality_over_frames: u32,   // frames consecutively over budget
    auto_quality_under_frames: u32,  // frames consecutively under budget

    // FXAA post-process AA — single pass on LDR composite output (low tier only)
    // upscaling — active when render_scale < 1.0. all modes share one BGL.
    upscale_active: bool,
    // render-resolution LDR target — composite writes here when upscaling
    fsr_ldr_texture: Option<wgpu::Texture>,
    fsr_ldr_view: Option<wgpu::TextureView>,
    // display-resolution intermediate — single-pass modes write output here;
    // FSR EASU writes here and RCAS reads from here
    fsr_mid_texture: Option<wgpu::Texture>,
    fsr_mid_view: Option<wgpu::TextureView>,
    upscale_nearest_pipeline: Option<wgpu::RenderPipeline>,
    upscale_linear_pipeline:  Option<wgpu::RenderPipeline>,
    upscale_lanczos_pipeline: Option<wgpu::RenderPipeline>,
    upscale_bicubic_pipeline: Option<wgpu::RenderPipeline>,
    fsr_easu_pipeline: Option<wgpu::RenderPipeline>,
    fsr_rcas_pipeline: Option<wgpu::RenderPipeline>,
    fsr_bgl: Option<wgpu::BindGroupLayout>,
    fsr_easu_bg: Option<wgpu::BindGroup>,  // binds fsr_ldr as input
    fsr_rcas_bg: Option<wgpu::BindGroup>,  // binds fsr_mid as input
    fsr_params_buf: Option<wgpu::Buffer>,

    fxaa_enabled: bool,
    // intermediate LDR target — composite (or FSR output) writes here when FXAA or TAA active
    fxaa_ldr_texture: wgpu::Texture,
    fxaa_ldr_view: wgpu::TextureView,
    fxaa_bgl: wgpu::BindGroupLayout,
    fxaa_bg: wgpu::BindGroup,
    fxaa_params_buf: wgpu::Buffer,
    fxaa_pipeline: wgpu::RenderPipeline,

    // selective temporal AA (STAA): shimmer suppression + jitter-based sub-pixel AA.
    // uses two ping-pong history textures to avoid read-write aliasing in the same pass.
    // staa_bg_even reads history_a, writes to [swapchain, history_b].
    // staa_bg_odd  reads history_b, writes to [swapchain, history_a].
    staa_enabled: bool,
    staa_frame_index: u32,
    staa_prev_vp_jittered: Mat4,     // jittered view_proj from previous frame (shader history reprojection)
    staa_prev_jitter: Vec2,          // previous frame jitter (uv space) for un-jittering velocity
    staa_history_a_texture: wgpu::Texture,
    staa_history_a_view: wgpu::TextureView,
    staa_history_b_texture: wgpu::Texture,
    staa_history_b_view: wgpu::TextureView,
    staa_bgl: wgpu::BindGroupLayout,
    staa_bg_even: wgpu::BindGroup,   // even frames: reads history_a
    staa_bg_odd: wgpu::BindGroup,    // odd  frames: reads history_b
    staa_params_buf: wgpu::Buffer,
    staa_pipeline: wgpu::RenderPipeline,
    staa_nearest_sampler: wgpu::Sampler,
    staa_ping: bool,

    // SSR — quarter-res screen-space reflections (mid+ tier)
    ssr_enabled: bool,
    ssr_texture: wgpu::Texture,
    ssr_view: wgpu::TextureView,
    ssr_bgl0: wgpu::BindGroupLayout,
    ssr_bgl1: wgpu::BindGroupLayout,
    ssr_bg0: wgpu::BindGroup,
    ssr_bg1: wgpu::BindGroup,
    ssr_params_buf: wgpu::Buffer,
    ssr_pipeline: wgpu::RenderPipeline,

    // atmospheric scattering sky (mid+ tier) — replaces flat dome when AtmosphericScattering is present
    atmos_bgl0: wgpu::BindGroupLayout,
    atmos_bgl1: wgpu::BindGroupLayout,
    atmos_bg0: wgpu::BindGroup,
    atmos_bg1: wgpu::BindGroup,
    atmos_params_buf: wgpu::Buffer,
    atmos_pipeline: wgpu::RenderPipeline,

    // volumetric fog — quarter-res ray-marched sun scattering (mid+ tier)
    fog_enabled: bool,
    fog_texture: wgpu::Texture,
    fog_view: wgpu::TextureView,
    fog_bgl0: wgpu::BindGroupLayout,
    fog_bgl1: wgpu::BindGroupLayout,
    fog_bg0: wgpu::BindGroup,
    fog_bg1: wgpu::BindGroup,
    fog_params_buf: wgpu::Buffer,
    fog_pipeline: wgpu::RenderPipeline,

    // particle system — GPU compute simulation (mid+ tier); CPU fallback on low tier
    particles_enabled: bool,
    particle_cap: u32,
    particle_buf: wgpu::Buffer,
    particle_sim_params_buf: wgpu::Buffer,
    particle_sim_bgl: wgpu::BindGroupLayout,
    particle_sim_bg: wgpu::BindGroup,
    particle_sim_pipeline: wgpu::ComputePipeline,
    particle_render_bgl: wgpu::BindGroupLayout,
    particle_render_bg: wgpu::BindGroup,
    particle_render_pipeline: wgpu::RenderPipeline,
    particle_cpu: Vec<CpuParticle>,

    // water rendering — Gerstner wave displacement + refraction (mid+ tier)
    water_params_buf: wgpu::Buffer,
    water_bgl0: wgpu::BindGroupLayout,
    water_bgl1: wgpu::BindGroupLayout,
    water_bg0: wgpu::BindGroup,
    water_bg1: wgpu::BindGroup,
    water_pipeline: wgpu::RenderPipeline,

    // decal system — box-projected decals rendered after opaques (uses scene depth)
    decal_params_buf: wgpu::Buffer,
    decal_bgl0: wgpu::BindGroupLayout,
    decal_bgl1: wgpu::BindGroupLayout,
    decal_bg0: wgpu::BindGroup,
    decal_bg1: wgpu::BindGroup,
    decal_pipeline: wgpu::RenderPipeline,

    // terrain rendering — geometry clipmap heightmap (all tiers, LOD level varies)
    terrain_pipeline: wgpu::RenderPipeline,
    terrain_globals_bgl: wgpu::BindGroupLayout,
    terrain_globals_bg: wgpu::BindGroup,
    terrain_params_bgl: wgpu::BindGroupLayout,
    terrain_gpu: HashMap<Entity, TerrainGpu>,

    // transparent pass — alpha < 1.0 entities drawn back-to-front after opaques
    transparent_pipeline: wgpu::RenderPipeline,
    // (entity, mesh_id, mat_id, color, metallic, roughness, model, alpha)
    // sorted by (mesh_id, mat_id) before the draw loop for batching
    // indices into draw_scratch for transparent entities, sorted back-to-front
    transparent_scratch: Vec<usize>,
    // sort-skip: quantized depth keys (mm precision) from the previous frame
    transparent_last_depths: Vec<i32>,
    // reused scratch for this frame's depth keys (compared against last_depths)
    transparent_depths_scratch: Vec<i32>,
    transparent_last_cam_fwd: Vec3,

    // stored shader modules for runtime MSAA rebuild
    msaa_main_shader: wgpu::ShaderModule,
    msaa_surface_shader: wgpu::ShaderModule,
    msaa_water_shader: wgpu::ShaderModule,
    msaa_terrain_shader: wgpu::ShaderModule,
    msaa_particle_render_shader: wgpu::ShaderModule,

    // pipeline cache — persists compiled shader binaries across runs (Vulkan/DX12 only)
    #[cfg(not(target_arch = "wasm32"))]
    pipeline_cache: Option<wgpu::PipelineCache>,

    // staging belt — explicit frame-temporary upload staging for large buffers (native only)
    #[cfg(not(target_arch = "wasm32"))]
    staging_belt: wgpu::util::StagingBelt,

    // RenderBundle for static geometry (entities with StaticMesh component).
    // re-recorded when the static entity set changes or when hdr_format/msaa_samples changes.
    // None until the first frame with any static entity.
    static_bundle: Option<wgpu::RenderBundle>,
    // sorted (mesh_id, mat_id, lm_id, entity_slot) list used to detect set changes
    static_draw_list: Vec<(u32, u32, u32, u32, usize)>,
    // reused scratch for building this frame's list before comparing to static_draw_list
    static_list_scratch: Vec<(u32, u32, u32, u32, usize)>,
    // (hdr_format, msaa_samples) the bundle was recorded with
    static_bundle_params: (wgpu::TextureFormat, u32),
    // number of entity slots reserved for static entities (slots 2..2+N)
    static_entity_count: usize,
    // stable entity→slot assignments for static entities
    static_entity_slots: HashMap<Entity, usize>,

    // lightmap bind group (group 4): irradiance tex + dir tex + sampler per entity
    lightmap_bgl: wgpu::BindGroupLayout,
    lightmap_sampler: wgpu::Sampler,
    // fallback textures (1×1): white irradiance, neutral direction (0,0,1) packed
    lightmap_fallback_tex: wgpu::Texture,
    lightmap_fallback_view: wgpu::TextureView,
    dir_lm_fallback_tex: wgpu::Texture,
    dir_lm_fallback_view: wgpu::TextureView,
    lightmap_fallback_bg: wgpu::BindGroup,
    // uploaded irradiance textures (lm_id → (texture, view))
    lm_tex_cache: HashMap<u32, (wgpu::Texture, wgpu::TextureView)>,
    // uploaded direction textures (dir_lm_id → (texture, view))
    dir_lm_tex_cache: HashMap<u32, (wgpu::Texture, wgpu::TextureView)>,
    // combined bind groups keyed by (lm_id, dir_lm_id); u32::MAX = use fallback
    lightmap_bg_cache: HashMap<(u32, u32), wgpu::BindGroup>,
    // lightmap atlas (phase 3) — packs all lightmap textures into one RGBA8 4096×4096 texture.
    // built/rebuilt when has_indirect and lm_tex_cache changes.
    // atlas_lm_uvs maps lm_id → [offset_u, offset_v, scale_u, scale_v]
    atlas_tex: Option<wgpu::Texture>,
    atlas_view: Option<wgpu::TextureView>,
    atlas_bg: Option<wgpu::BindGroup>,
    atlas_lm_uvs: HashMap<u32, [f32; 4]>,
    atlas_lm_ids: Vec<u32>,   // sorted list of lm_ids in current atlas (change detection)

    // mega vertex/index buffers (phase 4) — all meshes packed into one buffer.
    // enables one multi_draw_indexed_indirect_count call for all visible geometry.
    mega_vbuf: Option<wgpu::Buffer>,
    mega_ibuf: Option<wgpu::Buffer>,
    mega_vbuf_bytes: u64,
    mega_ibuf_bytes: u64,
    // draw params per mesh_id: (first_index_u32, index_count, base_vertex_as_i32_bits)
    mega_mesh_entries: HashMap<u32, [u32; 3]>,
    // per-entity GPU draw params buffer for cull shader input (index_count, first_index, base_vertex, entity_slot)
    entity_draw_params_buf: Option<wgpu::Buffer>,

    // per-frame scratch — cleared at frame start, never reallocated in steady state
    frustum_visible: HashSet<Entity>,
    // (entity, mesh_id, mat_id, model, lm_id, dir_lm_id); u32::MAX = none
    raw_scratch: Vec<(Entity, u32, u32, Mat4, u32, u32)>,
    // impostor billboard draw list — entities replaced by impostors this frame.
    // (world_pos, half_w, half_h, texture_id, u_min, u_max)
    impostor_scratch: Vec<(Vec3, f32, f32, u32, f32, f32)>,
    // (entity, mesh_id, mat_id, base_color, metallic, roughness, model, alpha, mat_flags, lm_id, dir_lm_id)
    // sorted by (alpha_bit, mesh_id, mat_id, lm_id, dir_lm_id) for batching
    #[allow(clippy::type_complexity)]
    draw_scratch: Vec<(Entity, u32, u32, Color, f32, f32, Mat4, f32, u32, u32, u32)>,
    uniform_staging: Vec<u8>,
    point_light_scratch: Vec<(Vec3, Color, f32, f32, bool, f32)>,  // (pos, color, intensity, radius, casts_shadows, dist_sq)
    // BSP PVS visible-area set, rebuilt each frame; `active` mirrors the old Option::Some
    bsp_visible_scratch: HashSet<u32>,
    bsp_visible_active: bool,
    // portal visible-area snapshot, rebuilt each frame; `active` mirrors the old Option::Some
    portal_visible_scratch: HashSet<u32>,
    portal_visible_active: bool,
    // static-mesh entity set, refilled each frame to diff against static_entity_slots
    static_entities_scratch: HashSet<Entity>,
    // per-entity AABB upload data (CullSoa order) — built once, fed to both frustum + HZB cull
    cull_aabb_scratch: Vec<f32>,
    // packed point-light list bytes uploaded to light_list_buf
    light_data_scratch: Vec<u8>,
    // CPU clustered-lighting fallback: per-cluster light counts + index table
    cluster_counts_scratch: Vec<u32>,
    cluster_indices_scratch: Vec<u32>,
    // late indirect-cull upload data (draw_scratch order): AABBs + draw params
    late_aabb_scratch: Vec<f32>,
    dp_data_scratch: Vec<u32>,
    // reused per-frame transients (cleared + refilled each frame instead of re-allocating)
    mesh_evict_scratch: Vec<u32>,           // gpu_only mesh ids to evict cpu data for
    coverage_hints_scratch: Vec<(u32, f32)>, // (lm_id, coverage) mip-streaming hints
    shadow_indices_scratch: Vec<u32>,        // per-point-light shadow slot index
    lm_needed_scratch: Vec<(u32, u32)>,      // distinct (lm_id, dir_lm_id) pairs this frame
    lm_evict_scratch: Vec<u32>,              // lightmap texture ids to evict cpu data for
    surface_evict_scratch: Vec<u32>,         // surface texture ids to evict cpu data for

    // render graph DAG — built once at init, drives pass execution order in render_frame.
    // models pass dependencies via declared texture reads/writes and topological sort.
    render_graph: render_graph::RenderGraph,

    // GPU-driven indirect rendering (High tier, MULTI_DRAW_INDIRECT + INDIRECT_FIRST_INSTANCE).
    // phase 2: CPU builds DrawIndexedIndirect args, issues draw_indexed_indirect per batch.
    // phase 4: GPU cull shader writes draw args directly, CPU issues one multi_draw call.
    has_indirect: bool,
    // DrawIndexedIndirect × entity_capacity — CPU-filled in phase 2, GPU-filled in phase 4
    indirect_buf: Option<wgpu::Buffer>,
    indirect_args: Vec<u32>,   // scratch: 5 u32s per entry (index_count, inst_count, first_idx, base_vert, first_inst)

    // GPU-driven frustum culling (high tier only).
    // a compute pass replaces the CPU CullSoa frustum test.
    // 1-frame pipelined: this frame writes to cull_flags_buf, previous frame's
    // staging result is read. first frame falls back to CPU cull (no prior result).
    gpu_cull_enabled: bool,
    cull_aabb_buf: Option<wgpu::Buffer>,
    cull_frustum_buf: Option<wgpu::Buffer>,
    cull_flags_buf: Option<wgpu::Buffer>,
    cull_flags_staging: Option<wgpu::Buffer>,
    cull_count_buf: Option<wgpu::Buffer>,
    cull_bgl: Option<wgpu::BindGroupLayout>,
    cull_pipeline: Option<wgpu::ComputePipeline>,
    // cached cull bind group (aabb+frustum+flags); rebuilt only when those buffers regrow
    cull_bg: Option<wgpu::BindGroup>,
    // cpu-side visible flag result (read back from previous frame's GPU result)
    gpu_cull_flags: Vec<u32>,
    cull_entity_capacity: usize,
    // whether the staging buffer has been written and is ready to map next frame
    cull_staging_pending: bool,
    cull_pending_entity_count: usize,
    // indirect cull pipeline (6 bindings) — created alongside standard pipeline when has_indirect
    cull_indirect_bgl: Option<wgpu::BindGroupLayout>,
    cull_indirect_pipeline: Option<wgpu::ComputePipeline>,
    // per-entity draw params buffer (input to indirect cull shader)
    cull_draw_params_buf: Option<wgpu::Buffer>,
    // indirect output count buffer (u32, zeroed each frame)
    cull_indirect_count_buf: Option<wgpu::Buffer>,
    // separate frustum params buffer for late-frame indirect cull (different entity_count)
    late_cull_frustum_buf: Option<wgpu::Buffer>,

    // hierarchical Z-buffer occlusion culling (high tier only).
    // built after the z-prepass; used next frame to cull occluded entities.
    hzb_enabled: bool,
    hzb_texture: Option<wgpu::Texture>,
    hzb_mip_views: Vec<wgpu::TextureView>,
    hzb_src_view: Option<wgpu::TextureView>,  // view of full HZB (for sampling)
    hzb_width: u32,
    hzb_height: u32,
    hzb_mip_count: u32,
    hzb_downsample_bgl: Option<wgpu::BindGroupLayout>,
    hzb_downsample_pipeline: Option<wgpu::ComputePipeline>,
    hzb_copy_bgl: Option<wgpu::BindGroupLayout>,
    hzb_copy_pipeline: Option<wgpu::ComputePipeline>,
    hzb_cull_bgl: Option<wgpu::BindGroupLayout>,
    hzb_cull_pipeline: Option<wgpu::ComputePipeline>,
    // cached HZB bind groups. copy + per-mip downsample depend only on the (fixed-size)
    // HZB views so they build once; cull also reads the hzb-cull buffers so it regrows.
    hzb_copy_bg: Option<wgpu::BindGroup>,
    hzb_downsample_bgs: Vec<wgpu::BindGroup>,
    hzb_cull_bg: Option<wgpu::BindGroup>,
    // depth-source view for hzb copy (non-msaa, texture_binding)
    hzb_depth_src: Option<wgpu::Texture>,
    hzb_depth_src_view: Option<wgpu::TextureView>,
    // per-entity occlusion flags from hzb cull (combined with gpu_cull_flags)
    hzb_occ_flags: Vec<u32>,
    hzb_occ_buf: Option<wgpu::Buffer>,
    // async staging readback signals — set by map_async callback, checked next frame
    cull_staging_ready: Arc<AtomicBool>,
    hzb_staging_ready: Arc<AtomicBool>,
    hzb_occ_staging: Option<wgpu::Buffer>,
    // hzb cull aabb / camera param buffers
    hzb_cull_aabb_buf: Option<wgpu::Buffer>,
    hzb_cull_params_buf: Option<wgpu::Buffer>,
    // 1-frame pipeline state for hzb occlusion readback
    hzb_staging_pending: bool,
    hzb_pending_entity_count: usize,

    // ── contact shadows ──────────────────────────────────────────────────
    contact_shadow_tex:           Option<wgpu::Texture>,
    contact_shadow_view:          Option<wgpu::TextureView>,
    contact_shadow_bgl:           Option<wgpu::BindGroupLayout>,
    contact_shadow_pipeline:      Option<wgpu::RenderPipeline>,
    contact_shadow_params_buf:    Option<wgpu::Buffer>,
    // 1×1 zero R8Unorm fallback for when contact shadows are disabled
    contact_shadow_fallback_tex:  wgpu::Texture,
    contact_shadow_fallback_view: wgpu::TextureView,
    // set true when contact_shadow_tex is first created to trigger composite_bg rebuild
    composite_bg_dirty: bool,

    // ── motion vectors ────────────────────────────────────────────────────
    motion_vec_tex:      Option<wgpu::Texture>,
    motion_vec_view:     Option<wgpu::TextureView>,
    motion_vec_bgl:      Option<wgpu::BindGroupLayout>,
    motion_vec_pipeline: Option<wgpu::RenderPipeline>,
    motion_vec_params_buf: Option<wgpu::Buffer>,
    /// view_proj from the previous frame for motion vector reprojection
    prev_view_proj: Mat4,

    // ── planar reflections ────────────────────────────────────────────────
    reflection_tex:           Option<wgpu::Texture>,
    reflection_view:          Option<wgpu::TextureView>,
    // depth target for the reflection pass, cached on (rw, rh) like reflection_tex
    reflection_depth_tex:     Option<wgpu::Texture>,
    reflection_depth_view:    Option<wgpu::TextureView>,
    reflection_globals_buf:   Option<wgpu::Buffer>,
    reflection_globals_bg:    Option<wgpu::BindGroup>,
    // 1×1 Rgba16Float fallback for water_bg0 binding 3 when no reflection is active
    reflection_fallback_tex:  wgpu::Texture,
    reflection_fallback_view: wgpu::TextureView,
    // set true when reflection_tex is first created to trigger water_bg0 rebuild
    water_bg_dirty: bool,

    // ── detail sprites ────────────────────────────────────────────────────
    detail_sprite_bgl:              Option<wgpu::BindGroupLayout>,
    detail_sprite_pipeline:         Option<wgpu::RenderPipeline>,
    detail_sprite_compute_bgl:      Option<wgpu::BindGroupLayout>,
    detail_sprite_compute_pipeline: Option<wgpu::ComputePipeline>,
    // per-entity detail sprite GPU resources (buffers + cached bind groups), keyed by entity bits.
    // see [`DetailSpriteEntry`]: instances_buf/count_buf/draw_buf/params_buf + compute & render BGs.
    detail_sprite_cache: HashMap<u64, DetailSpriteEntry>,

    // ── gpu lod selection ─────────────────────────────────────────────────
    lod_select_bgl:           Option<wgpu::BindGroupLayout>,
    lod_select_pipeline:      Option<wgpu::ComputePipeline>,
    // cached LOD-select bind group; rebuilt when cull aabb buf or lod buffers regrow
    lod_select_bg:            Option<wgpu::BindGroup>,
    lod_params_buf:           Option<wgpu::Buffer>,
    lod_indices_buf:          Option<wgpu::Buffer>,
    lod_indices_staging:      Option<wgpu::Buffer>,
    gpu_lod_indices:          HashMap<bevy_ecs::entity::Entity, u32>,
    lod_staging_pending:      bool,
    lod_pending_entity_count: usize,
    lod_staging_ready:        Arc<AtomicBool>,
}

// wasm is single-threaded; wgpu's WebGPU backend uses RefCell instead of Mutex,
// so its types are !Send + !Sync. we never actually run 3d rendering on wasm
// (there's no wasm bootstrap_3d), but the types still need to compile.
#[cfg(target_arch = "wasm32")]
unsafe impl Send for RenderEngine3d {}
#[cfg(target_arch = "wasm32")]
unsafe impl Sync for RenderEngine3d {}


// ── ecs integration ────────────────────────────────────────────────────────

fn render_3d_system(world: &mut World) {
    #[cfg(not(target_arch = "wasm32"))]
    let t0 = std::time::Instant::now();
    #[cfg(target_arch = "wasm32")]
    let t0: f64 = web_sys::window()
        .and_then(|w| w.performance())
        .map_or(0.0, |p| p.now());

    let mut engine = world.remove_resource::<RenderEngine3d>().unwrap();
    let draw_calls = engine.render_frame(world);

    #[cfg(not(target_arch = "wasm32"))]
    let frame_ms = t0.elapsed().as_secs_f32() * 1000.0;
    #[cfg(target_arch = "wasm32")]
    let frame_ms = (web_sys::window()
        .and_then(|w| w.performance())
        .map_or(t0, |p| p.now()) - t0) as f32;
    let scale = engine.tick_dynamic_resolution(frame_ms);

    // adaptive quality: step preset up/down based on sustained over/under budget
    // 3 consecutive seconds over → step down; 10 consecutive seconds under → step up
    // at 60fps: 180 frames over / 600 frames under
    const OVER_THRESHOLD: u32  = 180;
    const UNDER_THRESHOLD: u32 = 600;
    let auto = world.get_resource::<AutoQuality>().cloned();
    if let Some(auto) = auto
        && auto.enabled {
            let (over_f, under_f) = (engine.auto_quality_over_frames, engine.auto_quality_under_frames);
            let current = world.resource::<QualitySettings>().preset;
            let tier = engine.tier();
            if over_f >= OVER_THRESHOLD && RenderEngine3d::preset_ord(current) > RenderEngine3d::preset_ord(auto.min) {
                let next = RenderEngine3d::preset_step_down(current, auto.min);
                if let Some(mut qs) = world.get_resource_mut::<QualitySettings>() {
                    *qs = QualitySettings::from_tier_and_preset(tier, next);
                }
                engine.auto_quality_over_frames = 0;
            } else if under_f >= UNDER_THRESHOLD && RenderEngine3d::preset_ord(current) < RenderEngine3d::preset_ord(auto.max) {
                let next = RenderEngine3d::preset_step_up(current, auto.max);
                if let Some(mut qs) = world.get_resource_mut::<QualitySettings>() {
                    *qs = QualitySettings::from_tier_and_preset(tier, next);
                }
                engine.auto_quality_under_frames = 0;
            }
        }

    world.insert_resource(engine);
    if let Some(mut info) = world.get_resource_mut::<RenderInfo3d>() {
        info.draw_calls = draw_calls;
        info.frame_time_ms = frame_ms;
        info.fps = if frame_ms > 0.0 { 1000.0 / frame_ms } else { 0.0 };
        info.resolution_scale = scale;
    }
}

/// plugin that registers the 3d render system.
///
/// add this after [`Plugin3d`](lunar_3d::Plugin3d) in your app. inserts
/// [`RenderEngine3d`], [`RenderInfo3d`], and [`RenderTier`] as resources.
///
/// [`RenderEngine3d`] must be inserted before `build` is called — do this
/// in `bootstrap_3d` after creating the wgpu surface.
pub struct RenderPlugin3d;

impl GamePlugin for RenderPlugin3d {
    fn name(&self) -> &'static str {
        "render-3d"
    }

    fn build(&mut self, app: &mut App) {
        app.insert_resource(RenderInfo3d::default());

        // pull render tier from the engine resource (inserted by bootstrap) and
        // expose it and quality settings as standalone resources for game systems
        if let Some(engine) = app.world_mut().get_resource::<RenderEngine3d>() {
            let tier = engine.render_tier();
            app.insert_resource(QualitySettings::from_tier(tier));
            app.insert_resource(tier);
        }

        app.add_system_to_stage(UpdateStage::Render, render_3d_system);
        log::info!("RenderPlugin3d: 3d render system registered");
    }
}

/// common, game-facing 3D render types for `use lunar::prelude::*`.
/// dev/profiling internals stay at the crate root (`lunar::lunar_render_3d::X`).
pub mod prelude {
    pub use crate::{
        QualityPreset, QualitySettings, RenderConfig3d, RenderEngine3d, RenderInfo3d,
        RenderPlugin3d, Sky, UpscaleMode,
    };
}
