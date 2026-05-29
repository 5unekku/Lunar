//! 3d wgpu renderer for lunar.
//!
//! completely independent of the 2d renderer. owns its own wgpu device, queue,
//! and surface. add [`RenderPlugin3d`] to your app and the renderer handles
//! everything from there.
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

use std::collections::{HashMap, HashSet};

use bevy_ecs::prelude::*;
use lunar_3d::{
    Aabb3d, ActiveCamera3d, AmbientLight, Camera3d, ComputedVisibility, CullSoa, Decal,
    DirectionalLight, Frustum, IndexBuffer, IrradianceSH, Material3d, Mesh3d, MeshData,
    MeshLod, MeshRegistry, ParticleEmitter, PointLight, Projection, ShadowCaster, Vertex3d,
    Terrain, ViewportAspect, Water, WorldTransform3d,
};
use lunar_3d::primitives::{quad_mesh, sphere_mesh};
use lunar_core::{App, GamePlugin, UpdateStage};
use lunar_math::{Color, Mat3, Mat4, Vec2, Vec3};

const SHADER_SRC: &str           = include_str!("shader.wgsl");
const CULL_SHADER_SRC: &str      = include_str!("cull.wgsl");
const HZB_SHADER_SRC: &str       = include_str!("hzb.wgsl");
const SHADOW_SHADER_SRC: &str = include_str!("shadow.wgsl");
const BLOOM_SHADER_SRC: &str = include_str!("bloom.wgsl");
const COMPOSITE_SHADER_SRC: &str = include_str!("composite.wgsl");
const GTAO_SHADER_SRC: &str = include_str!("gtao.wgsl");

const FXAA_SHADER_SRC:          &str = include_str!("fxaa.wgsl");
const SSR_SHADER_SRC:           &str = include_str!("ssr.wgsl");
const FOG_SHADER_SRC:           &str = include_str!("volumetric_fog.wgsl");
const ATMOS_SHADER_SRC:         &str = include_str!("atmos.wgsl");
const PARTICLE_SIM_SHADER_SRC:    &str = include_str!("particle_sim.wgsl");
const PARTICLE_RENDER_SHADER_SRC: &str = include_str!("particle_render.wgsl");
const DECAL_SHADER_SRC:           &str = include_str!("decal.wgsl");
const WATER_SHADER_SRC:           &str = include_str!("water.wgsl");
const TERRAIN_SHADER_SRC:         &str = include_str!("terrain.wgsl");

const SKY_RADIUS: f32 = 900.0;
const SUN_Y: f32 = 895.0;
const VERTEX_STRIDE: u64 = std::mem::size_of::<Vertex3d>() as u64;

/// shadow map resolution per cascade.
const SHADOW_MAP_SIZE: u32 = 1024;

/// number of shadow cascades for the directional light.
const NUM_CASCADES: u32 = 3;

/// group 0: view_proj (64) + cam_pos (12) + elapsed (4) + delta (4) + pad (12) = 96 bytes.
const GLOBALS_SIZE: u64 = 96;

/// group 1: base_color (16) + metallic (4) + roughness (4) + flags (4) + pad (4) = 32 bytes.
const MATERIAL_UNIFORMS_SIZE: u64 = 32;

/// per-entity transform data: model mat4 (64) + normal matrix 3×vec4 (48) = 112 bytes,
/// padded to UNIFORM_STRIDE (256) in the staging buffer.
#[allow(dead_code)]
const MESH_UNIFORMS_SIZE: u64 = 112;

/// group 3: ambient(16) + dir(32) + 3×light_space(192) + cascade_splits(16) + point_header(16)
///   + 8×point_light(256) = 528 bytes, + sh_header(16) + 9×sh_coeff×vec4(144) = 688 bytes.
const LIGHTS_SIZE: u64 = 688;

/// shadow globals: light view-projection mat4 per cascade slot (dynamic offset).
const SHADOW_GLOBALS_SIZE: u64 = 64;

/// maximum point lights uploaded per frame.
const MAX_POINT_LIGHTS: usize = 8;

/// cascade split lambda for logarithmic-linear blending (0=linear, 1=log).
const CASCADE_LAMBDA: f32 = 0.5;

/// near and far planes used for cascade split computation.
const SHADOW_NEAR: f32 = 0.1;
const SHADOW_FAR:  f32 = 200.0;

/// HDR render target format — RGBA16Float for linear HDR output.
const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

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

/// SSR params UBO: inv_view_proj(64) + proj(64) + view(64) + misc(32) = 224 bytes.
const SSR_PARAMS_SIZE: u64 = 224;

/// volumetric fog params UBO: inv_view_proj(64) + misc(64) = 128 bytes (std140 aligned).
const FOG_PARAMS_SIZE: u64 = 128;

/// atmospheric scattering params UBO: sun_dir(12)+sun_intensity(4)+rayleigh(12)+mie(4)+scales(16)+radii+exposure+pads = 64 bytes.
const ATMOS_PARAMS_SIZE: u64 = 64;

/// particle sim params UBO: delta_time(4)+gravity(4)+alive_count(4)+pad(4) = 16 bytes.
const PARTICLE_SIM_PARAMS_SIZE: u64 = 16;

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

// ── gpu types ──────────────────────────────────────────────────────────────

struct GpuMesh {
    vbuf: wgpu::Buffer,
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

// ── byte helpers ───────────────────────────────────────────────────────────

unsafe fn slice_as_bytes<T>(slice: &[T]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(slice.as_ptr() as *const u8, std::mem::size_of_val(slice)) }
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualityPreset { Low, Medium, High, Ultra }

/// per-feature quality knobs. inserted as a resource by [`RenderPlugin3d`]
/// using defaults derived from the detected [`RenderTier`].
///
/// game code can override individual fields after plugin init.
#[derive(Resource, Clone)]
pub struct QualitySettings {
    pub preset: QualityPreset,
    /// shadow map resolution per cascade side (pixels).
    pub shadow_res: u32,
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
    /// enable quarter-res screen-space reflections (mid+ tier).
    pub ssr: bool,
    /// enable quarter-res ray-marched volumetric fog (mid+ tier).
    pub volumetric_fog: bool,
}

impl QualitySettings {
    pub fn from_tier(tier: RenderTier) -> Self {
        match tier {
            RenderTier::LowGles => Self {
                preset: QualityPreset::Low,
                shadow_res: 512,
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
                ssr: false,
                volumetric_fog: false,
            },
            RenderTier::Mid => Self {
                preset: QualityPreset::Medium,
                shadow_res: 1024,
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
                ssr: true,
                volumetric_fog: true,
            },
            RenderTier::High => Self {
                preset: QualityPreset::High,
                shadow_res: 2048,
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
                ssr: true,
                volumetric_fog: true,
            },
        }
    }
}

// ── render config ──────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct RenderConfig3d {
    pub width: u32,
    pub height: u32,
    pub vsync: bool,
    pub frame_cap: u32,
    pub title: String,
}

impl Default for RenderConfig3d {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
            vsync: true,
            frame_cap: 0,
            title: "Lunar".to_string(),
        }
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

/// the 3d rendering engine. owns the wgpu device, queue, and surface.
///
/// inserted as a resource by [`RenderPlugin3d`]. game code should not
/// interact with this directly — use [`MeshRegistry`] and ECS components instead.
#[cfg_attr(not(target_arch = "wasm32"), derive(Resource))]
#[allow(dead_code)]
pub struct RenderEngine3d {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    render_tier: RenderTier,

    msaa_samples: u32,
    depth_view: wgpu::TextureView,
    // some when msaa_samples > 1; render target for color pass, resolved to swapchain
    msaa_color_view: Option<wgpu::TextureView>,

    // group 0: view-global (camera view-proj + time)
    globals_buf: wgpu::Buffer,
    globals_bg: wgpu::BindGroup,

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

    // FXAA post-process AA — single pass on LDR composite output (low tier only)
    fxaa_enabled: bool,
    // intermediate LDR texture — composite writes here when FXAA is active
    fxaa_ldr_texture: wgpu::Texture,
    fxaa_ldr_view: wgpu::TextureView,
    fxaa_bgl: wgpu::BindGroupLayout,
    fxaa_bg: wgpu::BindGroup,
    fxaa_params_buf: wgpu::Buffer,
    fxaa_pipeline: wgpu::RenderPipeline,

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

    // pipeline cache — persists compiled shader binaries across runs (Vulkan/DX12 only)
    #[cfg(not(target_arch = "wasm32"))]
    pipeline_cache: Option<wgpu::PipelineCache>,

    // staging belt — explicit frame-temporary upload staging for large buffers (native only)
    #[cfg(not(target_arch = "wasm32"))]
    staging_belt: wgpu::util::StagingBelt,

    // per-frame scratch — cleared at frame start, never reallocated in steady state
    frustum_visible: HashSet<Entity>,
    raw_scratch: Vec<(Entity, u32, u32, Mat4)>,
    // (entity, mesh_id, mat_id, base_color, metallic, roughness, model, alpha)
    // sorted by (mesh_id, mat_id) before drawing for state-change batching and GPU instancing
    draw_scratch: Vec<(Entity, u32, u32, Color, f32, f32, Mat4, f32)>,
    uniform_staging: Vec<u8>,
    point_light_scratch: Vec<(Vec3, Color, f32, f32)>,

    // render graph DAG — built once at init, drives pass execution order in render_frame.
    // models pass dependencies via declared texture reads/writes and topological sort.
    render_graph: render_graph::RenderGraph,

    // GPU-driven frustum culling (high tier only).
    // a compute pass replaces the CPU CullSoa frustum test.
    gpu_cull_enabled: bool,
    cull_aabb_buf: Option<wgpu::Buffer>,
    cull_frustum_buf: Option<wgpu::Buffer>,
    cull_flags_buf: Option<wgpu::Buffer>,
    cull_flags_staging: Option<wgpu::Buffer>,
    cull_count_buf: Option<wgpu::Buffer>,
    cull_bgl: Option<wgpu::BindGroupLayout>,
    cull_pipeline: Option<wgpu::ComputePipeline>,
    // cpu-side visible flag result (read back from GPU)
    gpu_cull_flags: Vec<u32>,
    cull_entity_capacity: usize,

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
    // depth-source view for hzb copy (non-msaa, texture_binding)
    hzb_depth_src: Option<wgpu::Texture>,
    hzb_depth_src_view: Option<wgpu::TextureView>,
    // per-entity occlusion flags from hzb cull (combined with gpu_cull_flags)
    hzb_occ_flags: Vec<u32>,
    hzb_occ_buf: Option<wgpu::Buffer>,
    hzb_occ_staging: Option<wgpu::Buffer>,
    // hzb cull aabb / camera param buffers
    hzb_cull_aabb_buf: Option<wgpu::Buffer>,
    hzb_cull_params_buf: Option<wgpu::Buffer>,
}

// wasm is single-threaded; wgpu's WebGPU backend uses RefCell instead of Mutex,
// so its types are !Send + !Sync. we never actually run 3d rendering on wasm
// (there's no wasm bootstrap_3d), but the types still need to compile.
#[cfg(target_arch = "wasm32")]
unsafe impl Send for RenderEngine3d {}
#[cfg(target_arch = "wasm32")]
unsafe impl Sync for RenderEngine3d {}

impl RenderEngine3d {
    // ── construction ───────────────────────────────────────────────────────

    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_surface(
        instance: &wgpu::Instance,
        surface: wgpu::Surface<'static>,
        config: &RenderConfig3d,
    ) -> Self {
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
        }))
        .expect("no wgpu adapter found");

        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("lunar-render-3d device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::default(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
            },
        ))
        .expect("failed to create wgpu device");

        Self::init_with_adapter(&adapter, device, queue, surface, config)
    }

    fn init_with_adapter(
        adapter: &wgpu::Adapter,
        device: wgpu::Device,
        queue: wgpu::Queue,
        surface: wgpu::Surface<'static>,
        config: &RenderConfig3d,
    ) -> Self {
        let render_tier = RenderTier::from_adapter(adapter);
        log::info!("render tier: {render_tier:?}");

        let caps = surface.get_capabilities(adapter);
        let format = caps
            .formats
            .first()
            .copied()
            .unwrap_or(wgpu::TextureFormat::Bgra8UnormSrgb);

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: config.width,
            height: config.height,
            present_mode: if config.vsync {
                wgpu::PresentMode::AutoVsync
            } else {
                wgpu::PresentMode::AutoNoVsync
            },
            alpha_mode: caps.alpha_modes.first().copied().unwrap_or_default(),
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &surface_config);

        let msaa_samples = match render_tier {
            RenderTier::LowGles => 1,
            RenderTier::Mid | RenderTier::High => 4,
        };
        let depth_view = Self::make_depth_view(&device, config.width, config.height, msaa_samples);
        let msaa_color_view = Self::make_msaa_color_view(
            &device, config.width, config.height, HDR_FORMAT, msaa_samples,
        );

        // ── bind group layouts ─────────────────────────────────────────────

        let globals_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("[globals] bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(GLOBALS_SIZE),
                },
                count: None,
            }],
        });

        // group 1: material — dynamic offset, one slot per draw call (base_color)
        let material_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("[material] bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(MATERIAL_UNIFORMS_SIZE),
                },
                count: None,
            }],
        });

        // group 2: per-mesh — dynamic offset, one slot per draw call (model matrix)
        let mesh_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("[mesh] bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        // ── globals buffer ─────────────────────────────────────────────────

        let globals_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("[globals] view-proj+time"),
            size: GLOBALS_SIZE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let globals_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("[globals] bg"),
            layout: &globals_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals_buf.as_entire_binding(),
            }],
        });

        // ── material uniform buffer (group 1) ─────────────────────────────

        let entity_capacity = INITIAL_ENTITY_CAPACITY;
        let material_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("[material] uniform buffer"),
            size: (entity_capacity * UNIFORM_STRIDE as usize) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let material_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("[material] bg"),
            layout: &material_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &material_buf,
                    offset: 0,
                    size: wgpu::BufferSize::new(MATERIAL_UNIFORMS_SIZE),
                }),
            }],
        });
        let material_staging = vec![0u8; entity_capacity * UNIFORM_STRIDE as usize];

        // ── mesh uniform buffer (group 2) ─────────────────────────────────

        let entity_buf = Self::make_entity_buf(&device, entity_capacity);
        let entity_bg = Self::make_entity_bg(&device, &mesh_bgl, &entity_buf);
        let uniform_staging = vec![0u8; entity_capacity * UNIFORM_STRIDE as usize];

        // ── lights buffer (group 3) ───────────────────────────────────────

        let lights_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("[lights] bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(LIGHTS_SIZE),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
            ],
        });

        let lights_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("[lights] uniform buffer"),
            size: LIGHTS_SIZE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // 3-layer depth array — one layer per cascade
        let shadow_map = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("[shadow] cascade depth array"),
            size: wgpu::Extent3d {
                width: SHADOW_MAP_SIZE,
                height: SHADOW_MAP_SIZE,
                depth_or_array_layers: NUM_CASCADES,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        // full-array view for shader sampling
        let shadow_map_view = shadow_map.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        // per-cascade single-layer views for render attachments
        let shadow_cascade_views = std::array::from_fn(|i| {
            shadow_map.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_array_layer: i as u32,
                array_layer_count: Some(1),
                ..Default::default()
            })
        });

        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("[shadow] comparison sampler"),
            compare: Some(wgpu::CompareFunction::LessEqual),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let lights_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("[lights] bg"),
            layout: &lights_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: lights_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&shadow_map_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&shadow_sampler) },
            ],
        });

        // ── shadow globals (group 0 of shadow pipeline) ───────────────────

        let shadow_globals_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("[shadow globals] bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(SHADOW_GLOBALS_SIZE),
                },
                count: None,
            }],
        });

        // 3 cascade slots, 256-byte aligned (one per cascade, selected via dynamic offset)
        let shadow_globals_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("[shadow globals] cascade VPs"),
            size: NUM_CASCADES as u64 * UNIFORM_STRIDE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let shadow_globals_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("[shadow globals] bg"),
            layout: &shadow_globals_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: shadow_globals_buf.as_entire_binding(),
            }],
        });

        // ── pipeline cache (Vulkan/DX12 only) ─────────────────────────────
        // load compiled shader binaries from previous run to skip recompilation.
        #[cfg(not(target_arch = "wasm32"))]
        let pipeline_cache = Self::load_pipeline_cache(&device);
        // PipelineCache is Vulkan/DX12 only — WebGPU has no equivalent
        #[cfg(not(target_arch = "wasm32"))]
        let pipeline_cache_ref: Option<&wgpu::PipelineCache> = pipeline_cache.as_ref();
        #[cfg(target_arch = "wasm32")]
        let pipeline_cache_ref: Option<&wgpu::PipelineCache> = None;

        // ── pipelines ──────────────────────────────────────────────────────

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("3d PBR shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_SRC.into()),
        });

        let shadow_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("3d shadow shader"),
            source: wgpu::ShaderSource::Wgsl(SHADOW_SHADER_SRC.into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("3d pipeline layout"),
            bind_group_layouts: &[Some(&globals_bgl), Some(&material_bgl), Some(&mesh_bgl), Some(&lights_bgl)],
            immediate_size: 0,
        });

        let shadow_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("3d shadow pipeline layout"),
            bind_group_layouts: &[Some(&shadow_globals_bgl), Some(&mesh_bgl)],
            immediate_size: 0,
        });

        let vertex_buffers = &[wgpu::VertexBufferLayout {
            array_stride: VERTEX_STRIDE,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 0,  shader_location: 0 },
                wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 12, shader_location: 1 },
                wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x4, offset: 24, shader_location: 2 },
                wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 40, shader_location: 3 },
                wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x2, offset: 48, shader_location: 4 },
                wgpu::VertexAttribute { format: wgpu::VertexFormat::Unorm8x4,  offset: 56, shader_location: 5 },
            ],
        }];

        let opaque_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("3d opaque pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: vertex_buffers,
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                // with z-prepass (mid/high) depth is already populated — use LessEqual
                depth_write_enabled: Some(render_tier == RenderTier::LowGles),
                depth_compare: Some(if render_tier == RenderTier::LowGles {
                    wgpu::CompareFunction::Less
                } else {
                    wgpu::CompareFunction::LessEqual
                }),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState { count: msaa_samples, ..Default::default() },
            cache: pipeline_cache_ref,
            multiview_mask: None,
        });

        let sky_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("3d sky pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: vertex_buffers,
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState { count: msaa_samples, ..Default::default() },
            cache: pipeline_cache_ref,
            multiview_mask: None,
        });

        // Z-prepass: depth-only, no fragment shader, uses same vertex layout as opaque.
        // on mid/high tier this runs before the opaque pass to eliminate overdraw.
        let zprepass_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("3d z-prepass pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: vertex_buffers,
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: None,
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState { count: msaa_samples, ..Default::default() },
            cache: pipeline_cache_ref,
            multiview_mask: None,
        });

        let shadow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("3d shadow pipeline"),
            layout: Some(&shadow_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shadow_shader,
                entry_point: Some("vs_shadow"),
                buffers: vertex_buffers,
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: None,
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                // front-face culling reduces peter-panning shadow acne
                cull_mode: Some(wgpu::Face::Front),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            cache: pipeline_cache_ref,
            multiview_mask: None,
        });

        // transparent pipeline: same shader as opaque but no depth write, no backface cull
        let transparent_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("3d transparent pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: vertex_buffers,
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState { count: msaa_samples, ..Default::default() },
            cache: pipeline_cache_ref,
            multiview_mask: None,
        });

        // ── HDR texture ────────────────────────────────────────────────────
        // color pass renders here; MSAA (if enabled) resolves into this non-MSAA tex

        let quality = QualitySettings::from_tier(render_tier);
        let bloom_enabled = quality.bloom;
        let bloom_mip_count = quality.bloom_mips as usize;
        let fxaa_enabled = quality.fxaa;

        let (hdr_texture, hdr_view) = Self::make_hdr_texture(&device, config.width, config.height);

        // ── post sampler (linear clamp) ────────────────────────────────────
        let post_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("[post] linear sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });

        // ── bloom ──────────────────────────────────────────────────────────

        let bloom_params_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("[bloom] params buffer"),
            size: MAX_BLOOM_MIPS as u64 * UNIFORM_STRIDE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bloom_downsample_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("[bloom] bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(BLOOM_PARAMS_SIZE),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let bloom_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("[bloom] pipeline layout"),
            bind_group_layouts: &[Some(&bloom_downsample_bgl)],
            immediate_size: 0,
        });

        let bloom_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("[bloom] shader"),
            source: wgpu::ShaderSource::Wgsl(BLOOM_SHADER_SRC.into()),
        });

        let bloom_downsample_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("[bloom] downsample pipeline"),
            layout: Some(&bloom_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &bloom_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &bloom_shader,
                entry_point: Some("fs_downsample"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            cache: pipeline_cache_ref,
            multiview_mask: None,
        });

        let bloom_upsample_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("[bloom] upsample pipeline"),
            layout: Some(&bloom_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &bloom_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &bloom_shader,
                entry_point: Some("fs_upsample"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    // additive blend: dst = dst + src
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::One,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent::REPLACE,
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            cache: pipeline_cache_ref,
            multiview_mask: None,
        });

        // build per-step bind groups and mip views for the bloom chain
        let (bloom_mip_views, bloom_mip_sizes, bloom_downsample_bgs, bloom_upsample_bgs) =
            Self::build_bloom_resources(
                &device,
                &hdr_texture,
                &bloom_params_buf,
                &bloom_downsample_bgl,
                &post_sampler,
                config.width,
                config.height,
                bloom_mip_count,
            );

        // ── composite ──────────────────────────────────────────────────────

        let composite_params_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("[composite] params buffer"),
            size: COMPOSITE_PARAMS_SIZE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let composite_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("[composite] bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(COMPOSITE_PARAMS_SIZE),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // binding 5: fog_tex (rgba16f volumetric scattering result)
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // binding 6: sampler (was 4)
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let composite_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("[composite] shader"),
            source: wgpu::ShaderSource::Wgsl(COMPOSITE_SHADER_SRC.into()),
        });

        let composite_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("[composite] pipeline layout"),
            bind_group_layouts: &[Some(&composite_bgl)],
            immediate_size: 0,
        });

        let composite_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("[composite] pipeline"),
            layout: Some(&composite_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &composite_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &composite_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            cache: pipeline_cache_ref,
            multiview_mask: None,
        });

        // ── FXAA ───────────────────────────────────────────────────────────

        let fxaa_ldr_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("[fxaa] ldr texture"),
            size: wgpu::Extent3d { width: config.width, height: config.height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let fxaa_ldr_view = fxaa_ldr_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let fxaa_params_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("[fxaa] params buffer"),
            size: FXAA_PARAMS_SIZE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let fxaa_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("[fxaa] bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(FXAA_PARAMS_SIZE),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let fxaa_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("[fxaa] bg"),
            layout: &fxaa_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: fxaa_params_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&fxaa_ldr_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&post_sampler) },
            ],
        });

        let fxaa_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("[fxaa] shader"),
            source: wgpu::ShaderSource::Wgsl(FXAA_SHADER_SRC.into()),
        });

        let fxaa_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("[fxaa] pipeline layout"),
            bind_group_layouts: &[Some(&fxaa_bgl)],
            immediate_size: 0,
        });

        let fxaa_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("[fxaa] pipeline"),
            layout: Some(&fxaa_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &fxaa_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &fxaa_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            cache: pipeline_cache_ref,
            multiview_mask: None,
        });

        // ── SSR (screen-space reflections, mid+ tier) ─────────────────────

        let ssr_enabled = quality.ssr;
        let ssr_w = (config.width / 2).max(1);
        let ssr_h = (config.height / 2).max(1);

        let ssr_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("[ssr] reflection texture"),
            size: wgpu::Extent3d { width: ssr_w, height: ssr_h, depth_or_array_layers: 1 },
            mip_level_count: 1, sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let ssr_view = ssr_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let ssr_params_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("[ssr] params buffer"),
            size: SSR_PARAMS_SIZE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // group 0: globals + hdr + depth + samplers
        // group 0: globals + hdr_tex + depth_tex (float, textureLoad) + linear sampler
        let ssr_bgl0 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("[ssr] bgl0"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0, visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(GLOBALS_SIZE),
                    }, count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1, visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    }, count: None,
                },
                // depth texture read via textureLoad — TextureSampleType::Depth works with texture_2d<f32>
                wgpu::BindGroupLayoutEntry {
                    binding: 2, visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    }, count: None,
                },
                // linear sampler for HDR texture sampling on ray hit
                wgpu::BindGroupLayoutEntry {
                    binding: 3, visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        // group 1: SSR params
        let ssr_bgl1 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("[ssr] bgl1"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0, visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(SSR_PARAMS_SIZE),
                }, count: None,
            }],
        });

        // point (non-filtering) sampler for depth texture reads in SSR + fog
        let _depth_point_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("[depth] point sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        // SSR bg0 uses the non-MSAA depth texture (created below in GTAO section).
        // Declare as uninitialized here and assign after GTAO init via shadowing let.
        let ssr_bg1 = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("[ssr] bg1"),
            layout: &ssr_bgl1,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: ssr_params_buf.as_entire_binding() }],
        });

        let ssr_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("[ssr] shader"),
            source: wgpu::ShaderSource::Wgsl(SSR_SHADER_SRC.into()),
        });
        let ssr_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("[ssr] pipeline layout"),
            bind_group_layouts: &[Some(&ssr_bgl0), Some(&ssr_bgl1)],
            immediate_size: 0,
        });
        let ssr_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("[ssr] pipeline"),
            layout: Some(&ssr_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &ssr_shader, entry_point: Some("vs_main"),
                buffers: &[], compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &ssr_shader, entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba16Float,
                    blend: None, write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            cache: pipeline_cache_ref,
            multiview_mask: None,
        });

        // ── volumetric fog (mid+ tier) ─────────────────────────────────────

        let fog_enabled = quality.volumetric_fog;
        let fog_w = (config.width / 2).max(1);
        let fog_h = (config.height / 2).max(1);

        let fog_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("[fog] scattering texture"),
            size: wgpu::Extent3d { width: fog_w, height: fog_h, depth_or_array_layers: 1 },
            mip_level_count: 1, sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let fog_view = fog_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let fog_params_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("[fog] params buffer"),
            size: FOG_PARAMS_SIZE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let fog_bgl0 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("[fog] bgl0"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0, visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(GLOBALS_SIZE),
                    }, count: None,
                },
                // depth texture read via textureLoad — TextureSampleType::Depth with texture_2d<f32>
                wgpu::BindGroupLayoutEntry {
                    binding: 1, visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    }, count: None,
                },
            ],
        });
        let fog_bgl1 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("[fog] bgl1"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0, visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(FOG_PARAMS_SIZE),
                }, count: None,
            }],
        });

        // fog bg0 uses the non-MSAA depth texture (created in GTAO section, assigned below).
        let fog_bg1 = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("[fog] bg1"),
            layout: &fog_bgl1,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: fog_params_buf.as_entire_binding() }],
        });

        let fog_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("[fog] shader"),
            source: wgpu::ShaderSource::Wgsl(FOG_SHADER_SRC.into()),
        });
        let fog_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("[fog] pipeline layout"),
            bind_group_layouts: &[Some(&fog_bgl0), Some(&fog_bgl1)],
            immediate_size: 0,
        });
        let fog_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("[fog] pipeline"),
            layout: Some(&fog_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &fog_shader, entry_point: Some("vs_main"),
                buffers: &[], compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &fog_shader, entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba16Float,
                    blend: None, write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            cache: pipeline_cache_ref,
            multiview_mask: None,
        });

        // ── atmospheric scattering sky (mid+ tier) ────────────────────────

        let atmos_params_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("[atmos] params buffer"),
            size: ATMOS_PARAMS_SIZE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // group 0: globals + depth texture (read via textureLoad to check geometry coverage)
        let atmos_bgl0 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("[atmos] bgl0"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0, visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(GLOBALS_SIZE),
                    }, count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1, visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    }, count: None,
                },
            ],
        });

        let atmos_bgl1 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("[atmos] bgl1"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0, visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(ATMOS_PARAMS_SIZE),
                }, count: None,
            }],
        });

        let atmos_bg1 = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("[atmos] bg1"),
            layout: &atmos_bgl1,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: atmos_params_buf.as_entire_binding() }],
        });

        let atmos_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("[atmos] shader"),
            source: wgpu::ShaderSource::Wgsl(ATMOS_SHADER_SRC.into()),
        });
        let atmos_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("[atmos] pipeline layout"),
            bind_group_layouts: &[Some(&atmos_bgl0), Some(&atmos_bgl1)],
            immediate_size: 0,
        });
        let atmos_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("[atmos] pipeline"),
            layout: Some(&atmos_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &atmos_shader, entry_point: Some("vs_main"),
                buffers: &[], compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &atmos_shader, entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    // alpha blend: sky only writes to pixels with depth=1.0 (output alpha 0 for geometry)
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            cache: pipeline_cache_ref,
            multiview_mask: None,
        });
        // atmos_bg0 needs gtao_depth_tex (created in GTAO section); assigned after that section.

        // ── water rendering — Gerstner waves + refraction ─────────────────

        let water_params_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("[water] params buffer"),
            size: WATER_PARAMS_SIZE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let water_bgl0 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("[water] bgl0"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0, visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(GLOBALS_SIZE),
                    }, count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1, visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    }, count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2, visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let water_bgl1 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("[water] bgl1"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0, visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(WATER_PARAMS_SIZE),
                }, count: None,
            }],
        });

        let water_bg0 = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("[water] bg0"),
            layout: &water_bgl0,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: globals_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&hdr_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&post_sampler) },
            ],
        });

        let water_bg1 = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("[water] bg1"),
            layout: &water_bgl1,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: water_params_buf.as_entire_binding() }],
        });

        let water_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("[water] shader"),
            source: wgpu::ShaderSource::Wgsl(WATER_SHADER_SRC.into()),
        });
        let water_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("[water] pipeline layout"),
            bind_group_layouts: &[Some(&water_bgl0), Some(&water_bgl1)],
            immediate_size: 0,
        });
        let water_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("[water] pipeline"),
            layout: Some(&water_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &water_shader, entry_point: Some("vs_main"),
                buffers: vertex_buffers, compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &water_shader, entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState { count: msaa_samples, ..Default::default() },
            cache: pipeline_cache_ref,
            multiview_mask: None,
        });

        // ── decal system — box-projected, depth-sampled ───────────────────

        let decal_params_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("[decal] params buffer"),
            size: DECAL_PARAMS_SIZE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let decal_bgl0 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("[decal] bgl0"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0, visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(GLOBALS_SIZE),
                    }, count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1, visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    }, count: None,
                },
            ],
        });

        let decal_bgl1 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("[decal] bgl1"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0, visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(DECAL_PARAMS_SIZE),
                }, count: None,
            }],
        });

        let decal_bg1 = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("[decal] bg1"),
            layout: &decal_bgl1,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: decal_params_buf.as_entire_binding() }],
        });

        let decal_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("[decal] shader"),
            source: wgpu::ShaderSource::Wgsl(DECAL_SHADER_SRC.into()),
        });
        let decal_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("[decal] pipeline layout"),
            bind_group_layouts: &[Some(&decal_bgl0), Some(&decal_bgl1)],
            immediate_size: 0,
        });
        let decal_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("[decal] pipeline"),
            layout: Some(&decal_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &decal_shader, entry_point: Some("vs_main"),
                buffers: &[], compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &decal_shader, entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: Some(wgpu::Face::Front),
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            cache: pipeline_cache_ref,
            multiview_mask: None,
        });
        // decal_bg0 needs gtao_depth_tex; assigned after GTAO section.

        // ── terrain rendering — geometry clipmap ───────────────────────────

        // bg group 0: globals only (shared view-global bind group)
        let terrain_globals_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("[terrain] globals bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(GLOBALS_SIZE),
                },
                count: None,
            }],
        });

        let terrain_globals_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("[terrain] globals bg"),
            layout: &terrain_globals_bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals_buf.as_entire_binding() }],
        });

        let terrain_params_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("[terrain] params bgl"),
            entries: &[
                // binding 0: TerrainParams uniform
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(TERRAIN_PARAMS_SIZE),
                    },
                    count: None,
                },
                // binding 1: heightmap texture
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // binding 2: heightmap sampler
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let terrain_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("[terrain] pipeline layout"),
            bind_group_layouts: &[Some(&terrain_globals_bgl), Some(&terrain_params_bgl)],
            immediate_size: 0,
        });

        let terrain_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("[terrain] shader"),
            source: wgpu::ShaderSource::Wgsl(TERRAIN_SHADER_SRC.into()),
        });

        let terrain_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("[terrain] pipeline"),
            layout: Some(&terrain_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &terrain_shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: VERTEX_STRIDE,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x3, // position
                        1 => Float32x3, // normal
                        2 => Float32x4, // color
                        3 => Float32x2, // uv0
                        4 => Float32x2, // uv1
                        5 => Uint32,    // tint
                    ],
                }],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &terrain_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: msaa_samples,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            cache: pipeline_cache_ref,
            multiview_mask: None,
        });

        // ── GTAO ───────────────────────────────────────────────────────────

        let ssao_enabled = quality.ssao;
        let ao_w = (config.width / 2).max(1);
        let ao_h = (config.height / 2).max(1);

        // non-MSAA depth texture dedicated to GTAO input
        let gtao_depth_tex = Self::make_depth_view(&device, config.width, config.height, 1);

        let gtao_ao_a = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("[gtao] ao ping"),
            size: wgpu::Extent3d { width: ao_w, height: ao_h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rg32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let gtao_ao_b = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("[gtao] ao pong"),
            size: wgpu::Extent3d { width: ao_w, height: ao_h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rg32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let gtao_ao_view_a = gtao_ao_a.create_view(&wgpu::TextureViewDescriptor::default());
        let gtao_ao_view_b = gtao_ao_b.create_view(&wgpu::TextureViewDescriptor::default());

        let gtao_params_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("[gtao] params buffer"),
            size: GTAO_PARAMS_SIZE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // GTAO non-MSAA z-prepass (sample_count=1, writes to gtao_depth_tex)
        // reuses same vertex format as z-prepass but with no multisample
        let zprepass_nonmsaa_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("3d z-prepass (gtao depth, non-MSAA) pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: vertex_buffers,
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: None,
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(), // always sample_count=1
            cache: pipeline_cache_ref,
            multiview_mask: None,
        });

        let gtao_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("[gtao] bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(GTAO_PARAMS_SIZE),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                    count: None,
                },
            ],
        });

        let gtao_point_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("[gtao] point sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });

        let gtao_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("[gtao] pipeline layout"),
            bind_group_layouts: &[Some(&gtao_bgl)],
            immediate_size: 0,
        });

        let gtao_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("[gtao] shader"),
            source: wgpu::ShaderSource::Wgsl(GTAO_SHADER_SRC.into()),
        });

        let gtao_ao_format = wgpu::TextureFormat::Rg32Float;

        let make_gtao_pipeline = |entry: &'static str, blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&gtao_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &gtao_shader,
                    entry_point: Some("vs_main"),
                    buffers: &[],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &gtao_shader,
                    entry_point: Some(entry),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: gtao_ao_format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                cache: pipeline_cache_ref,
                multiview_mask: None,
            })
        };

        let gtao_pipeline = make_gtao_pipeline("fs_gtao", None);
        let gtao_blur_h_pipeline = make_gtao_pipeline("fs_blur_h", None);
        let gtao_blur_v_pipeline = make_gtao_pipeline("fs_blur_v", None);

        // dummy ao_src (ao_a) for initial main bg — blur passes bind ao_a or ao_b
        let gtao_main_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("[gtao] main bg"),
            layout: &gtao_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: gtao_params_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&gtao_depth_tex) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&gtao_ao_view_a) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&post_sampler) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&gtao_point_sampler) },
            ],
        });
        let gtao_blur_h_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("[gtao] blur-h bg"),
            layout: &gtao_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: gtao_params_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&gtao_depth_tex) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&gtao_ao_view_a) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&post_sampler) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&gtao_point_sampler) },
            ],
        });
        let gtao_blur_v_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("[gtao] blur-v bg"),
            layout: &gtao_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: gtao_params_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&gtao_depth_tex) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&gtao_ao_view_b) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&post_sampler) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&gtao_point_sampler) },
            ],
        });

        // rebuild composite_bg now that ao_view_a, ssr_view, and fog_view are available
        // binding 4 = ssr_tex, binding 5 = fog_tex, binding 6 = sampler
        let composite_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("[composite] bg"),
            layout: &composite_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: composite_params_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&hdr_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(bloom_mip_views.first().unwrap_or(&hdr_view)) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&gtao_ao_view_a) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&ssr_view) },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&fog_view) },
                wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::Sampler(&post_sampler) },
            ],
        });

        // rebuild ssr, fog, atmos, and decal bg0 now that the non-MSAA depth texture is available
        let decal_bg0 = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("[decal] bg0"),
            layout: &decal_bgl0,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: globals_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&gtao_depth_tex) },
            ],
        });
        let atmos_bg0 = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("[atmos] bg0"),
            layout: &atmos_bgl0,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: globals_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&gtao_depth_tex) },
            ],
        });
        let ssr_bg0 = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("[ssr] bg0"),
            layout: &ssr_bgl0,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: globals_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&hdr_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&gtao_depth_tex) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&post_sampler) },
            ],
        });
        let fog_bg0 = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("[fog] bg0"),
            layout: &fog_bgl0,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: globals_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&gtao_depth_tex) },
            ],
        });

        // ── particle system (compute simulation, mid+ tier) ───────────────

        let particles_enabled = render_tier != RenderTier::LowGles;
        let particle_cap = quality.particle_cap.max(1);

        let particle_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("[particles] storage buffer"),
            size: particle_cap as u64 * PARTICLE_STRIDE,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let particle_sim_params_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("[particles] sim params buffer"),
            size: PARTICLE_SIM_PARAMS_SIZE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let particle_sim_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("[particles] sim bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0, visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(PARTICLE_STRIDE),
                    }, count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1, visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(PARTICLE_SIM_PARAMS_SIZE),
                    }, count: None,
                },
            ],
        });

        let particle_sim_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("[particles] sim bg"),
            layout: &particle_sim_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: particle_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: particle_sim_params_buf.as_entire_binding() },
            ],
        });

        let particle_render_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("[particles] render bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0, visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(GLOBALS_SIZE),
                    }, count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1, visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(PARTICLE_STRIDE),
                    }, count: None,
                },
            ],
        });

        let particle_render_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("[particles] render bg"),
            layout: &particle_render_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: globals_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: particle_buf.as_entire_binding() },
            ],
        });

        let particle_sim_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("[particles] sim shader"),
            source: wgpu::ShaderSource::Wgsl(PARTICLE_SIM_SHADER_SRC.into()),
        });
        let particle_sim_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("[particles] sim pipeline layout"),
            bind_group_layouts: &[Some(&particle_sim_bgl)],
            immediate_size: 0,
        });
        let particle_sim_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("[particles] sim compute pipeline"),
            layout: Some(&particle_sim_pipeline_layout),
            module: &particle_sim_shader,
            entry_point: Some("cs_simulate"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: pipeline_cache_ref,
        });

        let particle_render_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("[particles] render shader"),
            source: wgpu::ShaderSource::Wgsl(PARTICLE_RENDER_SHADER_SRC.into()),
        });
        let particle_render_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("[particles] render pipeline layout"),
            bind_group_layouts: &[Some(&particle_render_bgl)],
            immediate_size: 0,
        });
        let particle_render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("[particles] render pipeline"),
            layout: Some(&particle_render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &particle_render_shader, entry_point: Some("vs_main"),
                buffers: &[], compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &particle_render_shader, entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState { count: msaa_samples, ..Default::default() },
            cache: pipeline_cache_ref,
            multiview_mask: None,
        });

        let particle_cpu: Vec<CpuParticle> = (0..particle_cap).map(|_| CpuParticle::dead()).collect();

        // ── sky meshes ─────────────────────────────────────────────────────

        let dome_mesh = Self::upload_mesh_data(&device, &queue, &sphere_mesh(SKY_RADIUS, 32, 16));
        let sun_mesh = Self::upload_mesh_data(&device, &queue, &quad_mesh(40.0, 40.0));

        log::info!(
            "lunar-render-3d initialized: {}×{}, vsync={}, tier={:?}",
            config.width, config.height, config.vsync, render_tier,
        );

        // clone before move into struct — wgpu::Device is Arc-backed, clone is cheap
        #[cfg(not(target_arch = "wasm32"))]
        let device_for_belt = device.clone();

        Self {
            device,
            queue,
            surface,
            msaa_samples,
            msaa_color_view,
            surface_config,
            render_tier,
            depth_view,
            globals_buf,
            globals_bg,
            material_bgl,
            material_buf,
            material_bg,
            material_staging,
            mesh_bgl,
            entity_buf,
            entity_bg,
            entity_capacity,
            opaque_pipeline,
            sky_pipeline,
            zprepass_pipeline,
            lights_bgl,
            lights_buf,
            lights_bg,
            shadow_map_view,
            shadow_sampler,
            shadow_globals_buf,
            shadow_globals_bgl,
            shadow_globals_bg,
            shadow_pipeline,
            shadow_cascade_views,
            mesh_gpu: HashMap::new(),
            dome_mesh,
            sun_mesh,
            hdr_texture,
            hdr_view,
            bloom_enabled,
            bloom_mip_views,
            bloom_mip_sizes,
            bloom_params_buf,
            bloom_downsample_bgl,
            bloom_downsample_bgs,
            bloom_upsample_bgs,
            bloom_downsample_pipeline,
            bloom_upsample_pipeline,
            composite_params_buf,
            composite_bgl,
            composite_bg,
            composite_pipeline,
            post_sampler,
            ssao_enabled,
            gtao_depth_view: gtao_depth_tex,
            gtao_ao_a,
            gtao_ao_b,
            gtao_ao_view_a,
            gtao_ao_view_b,
            gtao_params_buf,
            gtao_bgl,
            gtao_main_bg,
            gtao_blur_h_bg,
            gtao_blur_v_bg,
            gtao_pipeline,
            gtao_blur_h_pipeline,
            gtao_blur_v_pipeline,
            zprepass_nonmsaa_pipeline,
            transparent_pipeline,
            transparent_scratch: Vec::new(),
            fxaa_enabled,
            fxaa_ldr_texture,
            fxaa_ldr_view,
            fxaa_bgl,
            fxaa_bg,
            fxaa_params_buf,
            fxaa_pipeline,
            ssr_enabled,
            ssr_texture,
            ssr_view,
            ssr_bgl0,
            ssr_bgl1,
            ssr_bg0,
            ssr_bg1,
            ssr_params_buf,
            ssr_pipeline,
            fog_enabled,
            fog_texture,
            fog_view,
            fog_bgl0,
            fog_bgl1,
            fog_bg0,
            fog_bg1,
            fog_params_buf,
            fog_pipeline,
            atmos_bgl0,
            atmos_bgl1,
            atmos_bg0,
            atmos_bg1,
            atmos_params_buf,
            atmos_pipeline,
            water_params_buf,
            water_bgl0,
            water_bgl1,
            water_bg0,
            water_bg1,
            water_pipeline,
            decal_params_buf,
            decal_bgl0,
            decal_bgl1,
            decal_bg0,
            decal_bg1,
            decal_pipeline,
            particles_enabled,
            particle_cap,
            particle_buf,
            particle_sim_params_buf,
            particle_sim_bgl,
            particle_sim_bg,
            particle_sim_pipeline,
            particle_render_bgl,
            particle_render_bg,
            particle_render_pipeline,
            particle_cpu,
            terrain_pipeline,
            terrain_globals_bgl,
            terrain_globals_bg,
            terrain_params_bgl,
            terrain_gpu: HashMap::new(),
            #[cfg(not(target_arch = "wasm32"))]
            pipeline_cache,
            // 4 MiB chunk — larger than any single write, handles most scene sizes
            #[cfg(not(target_arch = "wasm32"))]
            staging_belt: wgpu::util::StagingBelt::new(device_for_belt, 4 * 1024 * 1024),
            frame_time_ema_ms: 16.67,
            resolution_scale: 1.0,
            frame_time_budget_ms: 14.0,
            frustum_visible: HashSet::new(),
            raw_scratch: Vec::new(),
            draw_scratch: Vec::new(),
            uniform_staging,
            point_light_scratch: Vec::new(),

            render_graph: Self::build_render_graph(render_tier, bloom_enabled, ssr_enabled, fog_enabled, fxaa_enabled, ssao_enabled),

            gpu_cull_enabled: render_tier == RenderTier::High,
            cull_aabb_buf: None,
            cull_frustum_buf: None,
            cull_flags_buf: None,
            cull_flags_staging: None,
            cull_count_buf: None,
            cull_bgl: None,
            cull_pipeline: None,
            gpu_cull_flags: Vec::new(),
            cull_entity_capacity: 0,

            hzb_enabled: render_tier == RenderTier::High,
            hzb_texture: None,
            hzb_mip_views: Vec::new(),
            hzb_src_view: None,
            hzb_width: config.width,
            hzb_height: config.height,
            hzb_mip_count: 0,
            hzb_downsample_bgl: None,
            hzb_downsample_pipeline: None,
            hzb_copy_bgl: None,
            hzb_copy_pipeline: None,
            hzb_cull_bgl: None,
            hzb_cull_pipeline: None,
            hzb_depth_src: None,
            hzb_depth_src_view: None,
            hzb_occ_flags: Vec::new(),
            hzb_occ_buf: None,
            hzb_occ_staging: None,
            hzb_cull_aabb_buf: None,
            hzb_cull_params_buf: None,
        }
    }

    fn build_render_graph(
        tier: RenderTier,
        bloom: bool,
        ssr: bool,
        fog: bool,
        fxaa: bool,
        ssao: bool,
    ) -> render_graph::RenderGraph {
        let mut g = render_graph::RenderGraph::new();
        let shadow   = g.texture("shadow_map");
        let depth    = g.texture("depth");
        let hdr      = g.texture("hdr");
        let ao       = g.texture("ao");
        let ssr_tex  = g.texture("ssr");
        let fog_tex  = g.texture("fog");
        let bloom_tex = g.texture("bloom");
        let ldr      = g.texture("ldr");
        let swapchain = g.texture("swapchain");

        // passes in dependency order. the graph's topological sort will produce the
        // same ordering from the declared resource edges, demonstrating the DAG works.
        g.add_pass("shadow",    vec![],                         vec![shadow]);
        if tier != RenderTier::LowGles {
            g.add_pass("zprepass", vec![],                      vec![depth]);
        }
        if ssao {
            g.add_pass("gtao",     vec![depth],                 vec![ao]);
        }
        if tier == RenderTier::High {
            g.add_pass("hzb_build",  vec![depth],               vec![]);
            g.add_pass("hzb_cull",   vec![],                    vec![]);
        }
        g.add_pass("opaque",    vec![shadow, depth],            vec![hdr]);
        g.add_pass("sky",       vec![],                         vec![hdr]);
        g.add_pass("particles", vec![depth],                    vec![hdr]);
        g.add_pass("decals",    vec![depth],                    vec![hdr]);
        g.add_pass("water",     vec![depth, hdr],               vec![hdr]);
        g.add_pass("transparent", vec![depth],                  vec![hdr]);
        if ssr { g.add_pass("ssr", vec![hdr, depth], vec![ssr_tex]); }
        if fog { g.add_pass("volumetric_fog", vec![depth], vec![fog_tex]); }
        if bloom { g.add_pass("bloom", vec![hdr], vec![bloom_tex]); }
        let composite_reads = {
            let mut r = vec![hdr];
            if ssao   { r.push(ao); }
            if ssr    { r.push(ssr_tex); }
            if fog    { r.push(fog_tex); }
            if bloom  { r.push(bloom_tex); }
            r
        };
        g.add_pass("composite", composite_reads, vec![ldr]);
        if fxaa {
            g.add_pass("fxaa", vec![ldr], vec![swapchain]);
        } else {
            g.add_pass("present", vec![ldr], vec![swapchain]);
        }
        g
    }

    /// lazily create (or grow) GPU frustum cull buffers and pipeline.
    fn ensure_gpu_cull_resources(&mut self, entity_count: usize) {
        if entity_count == 0 { return; }
        let needs_rebuild = self.cull_pipeline.is_none() || entity_count > self.cull_entity_capacity;
        if needs_rebuild {
            let cap = entity_count.next_power_of_two().max(256);
            self.cull_entity_capacity = cap;

            // aabb input buffer: 32 bytes per entry (center vec3+pad + half_extent vec3+pad)
            self.cull_aabb_buf = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("[cull] aabb buf"),
                size: (cap * 32) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            // frustum params: 6×vec4 planes + u32 count + 3 pad = 112 bytes, padded to 128
            self.cull_frustum_buf = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("[cull] frustum buf"),
                size: 128,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            // visible flags: one u32 per entity
            self.cull_flags_buf = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("[cull] flags buf"),
                size: (cap * 4) as u64,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            self.cull_flags_staging = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("[cull] flags staging"),
                size: (cap * 4) as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            }));
            self.gpu_cull_flags.resize(cap, 0);

            if self.cull_pipeline.is_none() {
                let bgl = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("[cull] bgl"),
                    entries: &[
                        wgpu::BindGroupLayoutEntry {
                            binding: 0, visibility: wgpu::ShaderStages::COMPUTE,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Storage { read_only: true },
                                has_dynamic_offset: false, min_binding_size: None,
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 1, visibility: wgpu::ShaderStages::COMPUTE,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Uniform,
                                has_dynamic_offset: false, min_binding_size: None,
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 2, visibility: wgpu::ShaderStages::COMPUTE,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Storage { read_only: false },
                                has_dynamic_offset: false, min_binding_size: None,
                            },
                            count: None,
                        },
                    ],
                });
                let layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("[cull] pipeline layout"),
                    bind_group_layouts: &[Some(&bgl)],
                    immediate_size: 0,
                });
                let module = self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("[cull] shader"),
                    source: wgpu::ShaderSource::Wgsl(CULL_SHADER_SRC.into()),
                });
                self.cull_pipeline = Some(self.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("[cull] pipeline"),
                    layout: Some(&layout),
                    module: &module,
                    entry_point: Some("cs_cull"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    cache: None,
                }));
                self.cull_bgl = Some(bgl);
            }
        }
    }

    /// lazily create HZB texture (R32Float mip chain) and pipelines.
    fn ensure_hzb_resources(&mut self) {
        if self.hzb_texture.is_some() { return; }

        let w = self.hzb_width;
        let h = self.hzb_height;
        let mip_count = (f32::max(w as f32, h as f32).log2().floor() as u32 + 1).max(1);
        self.hzb_mip_count = mip_count;

        // R32Float texture with all mip levels. storage usage required for compute writes.
        let hzb_tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("[hzb] texture"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: mip_count,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Float,
            usage: wgpu::TextureUsages::STORAGE_BINDING
                 | wgpu::TextureUsages::TEXTURE_BINDING
                 | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        // per-mip views for storage writes; full view for sampling in HZB cull
        self.hzb_mip_views = (0..mip_count)
            .map(|mip| hzb_tex.create_view(&wgpu::TextureViewDescriptor {
                label: Some(&format!("[hzb] mip {mip}")),
                base_mip_level: mip,
                mip_level_count: Some(1),
                ..Default::default()
            }))
            .collect();
        self.hzb_src_view = Some(hzb_tex.create_view(&wgpu::TextureViewDescriptor {
            label: Some("[hzb] full view"),
            ..Default::default()
        }));
        self.hzb_texture = Some(hzb_tex);

        // non-MSAA depth texture as HZB source (depth-only prepass writes here on high tier)
        let depth_src = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("[hzb] depth src"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        self.hzb_depth_src_view = Some(depth_src.create_view(&wgpu::TextureViewDescriptor::default()));
        self.hzb_depth_src = Some(depth_src);

        // depth-copy bgl: group 0 binding 0 = depth_src, binding 1 = hzb_mip0 (storage)
        let copy_bgl = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("[hzb] copy bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0, visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1, visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::R32Float,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
            ],
        });
        // downsample bgl: group 1 binding 0 = src texture_2d, binding 1 = dst storage_2d
        let ds_bgl = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("[hzb] downsample bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0, visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1, visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::R32Float,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
            ],
        });
        let hzb_module = self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("[hzb] shader"),
            source: wgpu::ShaderSource::Wgsl(HZB_SHADER_SRC.into()),
        });
        let copy_layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("[hzb] copy layout"),
            bind_group_layouts: &[Some(&copy_bgl)],
            immediate_size: 0,
        });
        self.hzb_copy_pipeline = Some(self.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("[hzb] copy pipeline"),
            layout: Some(&copy_layout),
            module: &hzb_module,
            entry_point: Some("cs_copy_depth"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        }));
        self.hzb_copy_bgl = Some(copy_bgl);

        let ds_layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("[hzb] downsample layout"),
            bind_group_layouts: &[Some(&ds_bgl)],
            immediate_size: 0,
        });
        self.hzb_downsample_pipeline = Some(self.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("[hzb] downsample pipeline"),
            layout: Some(&ds_layout),
            module: &hzb_module,
            entry_point: Some("cs_downsample"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        }));
        self.hzb_downsample_bgl = Some(ds_bgl);

        // hzb occlusion cull bgl: group 2
        let cull_bgl = self.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("[hzb] cull bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0, visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false, min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1, visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false, min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2, visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false, min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3, visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let cull_layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("[hzb] cull layout"),
            bind_group_layouts: &[Some(&cull_bgl)],
            immediate_size: 0,
        });
        self.hzb_cull_pipeline = Some(self.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("[hzb] cull pipeline"),
            layout: Some(&cull_layout),
            module: &hzb_module,
            entry_point: Some("cs_cull_hzb"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        }));
        self.hzb_cull_bgl = Some(cull_bgl);
    }

    /// grow HZB per-entity occlusion buffers if needed.
    fn ensure_hzb_cull_buffers(&mut self, entity_count: usize) {
        let cap = entity_count.next_power_of_two().max(256);
        let needs = self.hzb_occ_buf
            .as_ref()
            .map_or(true, |b| b.size() < (cap * 4) as u64);
        if !needs { return; }

        self.hzb_occ_buf = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("[hzb] occ flags buf"),
            size: (cap * 4) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        self.hzb_occ_staging = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("[hzb] occ staging"),
            size: (cap * 4) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        }));
        self.hzb_cull_aabb_buf = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("[hzb] cull aabb buf"),
            size: (cap * 32) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        // hzb cull params: mat4 (64) + vec2 viewport (8) + u32 mip_count (4) + u32 count (4) = 80 bytes
        self.hzb_cull_params_buf = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("[hzb] cull params buf"),
            size: 96,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        self.hzb_occ_flags.resize(cap, 0);
    }

    /// load the 3d pipeline cache from disk if available (Vulkan/DX12 only).
    #[cfg(not(target_arch = "wasm32"))]
    fn load_pipeline_cache(device: &wgpu::Device) -> Option<wgpu::PipelineCache> {
        let path = std::path::Path::new(".pipeline_cache_3d.bin");
        if path.exists() {
            match std::fs::read(path) {
                Ok(data) => {
                    log::info!("[render-3d] loaded pipeline cache ({} bytes)", data.len());
                    // SAFETY: fallback=true so wgpu rebuilds a fresh cache if validation
                    // fails; this only runs on Vulkan/DX12 where the format is stable.
                    Some(unsafe {
                        device.create_pipeline_cache(&wgpu::PipelineCacheDescriptor {
                            label: Some("[render-3d] pipeline cache"),
                            data: Some(&data),
                            fallback: true,
                        })
                    })
                }
                Err(err) => { log::warn!("[render-3d] pipeline cache load failed: {err}"); None }
            }
        } else {
            None
        }
    }

    /// persist pipeline cache to disk. call before engine shutdown to speed up
    /// shader compilation on the next launch (Vulkan/DX12 only).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn save_pipeline_cache(&self) {
        if let Some(ref cache) = self.pipeline_cache
            && let Some(data) = cache.get_data()
        {
            let path = std::path::Path::new(".pipeline_cache_3d.bin");
            match std::fs::write(path, &data) {
                Ok(()) => log::info!("[render-3d] saved pipeline cache ({} bytes)", data.len()),
                Err(err) => log::warn!("[render-3d] pipeline cache save failed: {err}"),
            }
        }
    }

    // ── helpers ────────────────────────────────────────────────────────────

    fn make_depth_view(device: &wgpu::Device, width: u32, height: u32, sample_count: u32) -> wgpu::TextureView {
        // non-MSAA depth also gets TEXTURE_BINDING so GTAO can sample it
        let usage = wgpu::TextureUsages::RENDER_ATTACHMENT
            | if sample_count == 1 { wgpu::TextureUsages::TEXTURE_BINDING } else { wgpu::TextureUsages::empty() };
        device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("[depth] attachment"),
                size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth32Float,
                usage,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor::default())
    }

    fn make_msaa_color_view(
        device: &wgpu::Device,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
        sample_count: u32,
    ) -> Option<wgpu::TextureView> {
        if sample_count <= 1 {
            return None;
        }
        Some(
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("[msaa] color attachment"),
                    size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default()),
        )
    }

    fn make_hdr_texture(device: &wgpu::Device, width: u32, height: u32) -> (wgpu::Texture, wgpu::TextureView) {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("[hdr] color attachment"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: HDR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        (tex, view)
    }

    /// creates the bloom mip chain texture, per-mip views, and per-step bind groups.
    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    fn build_bloom_resources(
        device: &wgpu::Device,
        hdr_texture: &wgpu::Texture,
        params_buf: &wgpu::Buffer,
        bgl: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        width: u32,
        height: u32,
        mip_count: usize,
    ) -> (
        Vec<wgpu::TextureView>,
        Vec<(u32, u32)>,
        Vec<wgpu::BindGroup>,
        Vec<wgpu::BindGroup>,
    ) {
        let actual_mips = mip_count.clamp(1, MAX_BLOOM_MIPS);

        // one bloom texture with mip_count mip levels
        let bloom_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("[bloom] mip chain"),
            size: wgpu::Extent3d { width: width / 2, height: height / 2, depth_or_array_layers: 1 },
            mip_level_count: actual_mips as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: HDR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });

        let mut mip_views = Vec::with_capacity(actual_mips);
        let mut mip_sizes = Vec::with_capacity(actual_mips);
        let mut w = width / 2;
        let mut h = height / 2;
        for i in 0..actual_mips {
            mip_views.push(bloom_tex.create_view(&wgpu::TextureViewDescriptor {
                base_mip_level: i as u32,
                mip_level_count: Some(1),
                ..Default::default()
            }));
            mip_sizes.push((w.max(1), h.max(1)));
            w = (w / 2).max(1);
            h = (h / 2).max(1);
        }

        // hdr view (full texture, mip 0) for the first downsample source
        let hdr_full_view = hdr_texture.create_view(&wgpu::TextureViewDescriptor::default());

        // downsample bind groups: step 0 reads hdr, step i reads bloom mip i-1
        let mut ds_bgs = Vec::with_capacity(actual_mips);
        for i in 0..actual_mips {
            let src_view = if i == 0 { &hdr_full_view } else { &mip_views[i - 1] };
            let (src_w, src_h) = if i == 0 { (width, height) } else { mip_sizes[i - 1] };
            let _ = (src_w, src_h);  // sizes used at frame time for param upload
            ds_bgs.push(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("[bloom] downsample bg"),
                layout: bgl,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: params_buf, offset: (i * UNIFORM_STRIDE as usize) as u64, size: wgpu::BufferSize::new(BLOOM_PARAMS_SIZE) }) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(src_view) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(sampler) },
                ],
            }));
        }

        // upsample bind groups: step i reads bloom mip i+1, writes to mip i
        let mut us_bgs = Vec::with_capacity(actual_mips.saturating_sub(1));
        for i in 0..actual_mips.saturating_sub(1) {
            let src_view = &mip_views[i + 1];
            us_bgs.push(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("[bloom] upsample bg"),
                layout: bgl,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: params_buf, offset: ((actual_mips + i) * UNIFORM_STRIDE as usize) as u64, size: wgpu::BufferSize::new(BLOOM_PARAMS_SIZE) }) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(src_view) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(sampler) },
                ],
            }));
        }

        (mip_views, mip_sizes, ds_bgs, us_bgs)
    }

    fn make_entity_buf(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("[draw] entity storage buffer"),
            size: (capacity * UNIFORM_STRIDE as usize) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    fn make_entity_bg(
        device: &wgpu::Device,
        mesh_bgl: &wgpu::BindGroupLayout,
        entity_buf: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("[mesh] entity bg"),
            layout: mesh_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: entity_buf.as_entire_binding(),
            }],
        })
    }

    fn upload_mesh_data(device: &wgpu::Device, queue: &wgpu::Queue, data: &MeshData) -> GpuMesh {
        let vdata = unsafe { slice_as_bytes(&data.vertices) };
        let vbuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("[mesh] vbuf"),
            size: vdata.len() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&vbuf, 0, vdata);

        let (idata, index_count, index_fmt) = match &data.indices {
            IndexBuffer::U16(v) => (
                unsafe { slice_as_bytes(v.as_slice()) },
                v.len() as u32,
                wgpu::IndexFormat::Uint16,
            ),
            IndexBuffer::U32(v) => (
                unsafe { slice_as_bytes(v.as_slice()) },
                v.len() as u32,
                wgpu::IndexFormat::Uint32,
            ),
        };
        let ibuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("[mesh] ibuf"),
            size: idata.len() as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&ibuf, 0, idata);

        GpuMesh { vbuf, ibuf, index_count, index_fmt }
    }

    /// build a flat NxN quad grid for one clipmap ring.
    /// vertices carry grid coords in position.xz (0..=resolution), position.y = 0.
    /// the vertex shader reads the heightmap to displace Y.
    fn build_clipmap_patch(resolution: u32) -> MeshData {
        let n = (resolution + 1) as usize;
        let mut vertices = Vec::with_capacity(n * n);
        for row in 0..=resolution {
            for col in 0..=resolution {
                let x = col as f32;
                let z = row as f32;
                let uv = Vec2::new(x / resolution as f32, z / resolution as f32);
                vertices.push(Vertex3d::new(
                    Vec3::new(x, 0.0, z),
                    Vec3::Y,
                    [1.0, 0.0, 0.0, 1.0],
                    uv,
                ));
            }
        }
        let mut indices: Vec<u32> = Vec::with_capacity(resolution as usize * resolution as usize * 6);
        for row in 0..resolution {
            for col in 0..resolution {
                let tl = row * (resolution + 1) + col;
                let tr = tl + 1;
                let bl = tl + (resolution + 1);
                let br = bl + 1;
                indices.extend_from_slice(&[tl, bl, tr, tr, bl, br]);
            }
        }
        MeshData::new(vertices, IndexBuffer::U32(indices))
    }

    /// upload a R16Float heightmap to the GPU.
    fn upload_heightmap(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        data: &[u8],
        width: u32,
        height: u32,
    ) -> (wgpu::Texture, wgpu::TextureView) {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("[terrain] heightmap"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        if !data.is_empty() {
            queue.write_texture(
                tex.as_image_copy(),
                data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(width * 2), // R16Float = 2 bytes per sample
                    rows_per_image: None,
                },
                wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            );
        }
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        (tex, view)
    }

    /// initialise GPU resources for one terrain entity.
    fn build_terrain_gpu(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        params_bgl: &wgpu::BindGroupLayout,
        terrain: &Terrain,
    ) -> TerrainGpu {
        // build ring meshes: center patch + (clipmap_rings - 1) outer rings
        let rings = terrain.clipmap_rings.clamp(1, 8);
        let resolution = terrain.ring_resolution.clamp(4, 256);
        let mut ring_meshes = Vec::with_capacity(rings as usize);
        for _ in 0..rings {
            let mesh = Self::build_clipmap_patch(resolution);
            ring_meshes.push(Self::upload_mesh_data(device, queue, &mesh));
        }

        let (w, h) = (terrain.heightmap_width.max(1), terrain.heightmap_height.max(1));
        let (heightmap_tex, heightmap_view) =
            Self::upload_heightmap(device, queue, &terrain.heightmap, w, h);

        let hmap_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("[terrain] heightmap sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let params_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("[terrain] params buffer"),
            size: TERRAIN_PARAMS_SIZE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let params_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("[terrain] params bg"),
            layout: params_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: params_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&heightmap_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&hmap_sampler) },
            ],
        });

        TerrainGpu { heightmap_tex, heightmap_view, ring_meshes, params_buf, params_bg, hmap_sampler }
    }

    fn pack_mesh_uniforms(staging: &mut [u8], slot: usize, model: Mat4) {
        let offset = slot * UNIFORM_STRIDE as usize;
        // model matrix (64 bytes)
        let model_cols = model.to_cols_array();
        staging[offset..offset + 64].copy_from_slice(unsafe { slice_as_bytes(&model_cols) });
        // normal matrix = transpose(inverse(mat3(model))), packed as 3×vec4 (48 bytes)
        let normal_mat = Mat3::from_mat4(model).inverse().transpose();
        let cols = normal_mat.to_cols_array();
        let normal_packed: [f32; 12] = [
            cols[0], cols[1], cols[2], 0.0,
            cols[3], cols[4], cols[5], 0.0,
            cols[6], cols[7], cols[8], 0.0,
        ];
        staging[offset + 64..offset + 112].copy_from_slice(unsafe { slice_as_bytes(&normal_packed) });
    }

    fn pack_material_uniforms(staging: &mut [u8], slot: usize, color: Color, metallic: f32, roughness: f32, flags: u32) {
        let offset = slot * UNIFORM_STRIDE as usize;
        let data: [f32; 7] = [color.r, color.g, color.b, color.a, metallic, roughness, f32::from_bits(flags)];
        // 7 × 4 = 28 bytes + 4 pad = 32 bytes
        staging[offset..offset + 28].copy_from_slice(unsafe { slice_as_bytes(&data) });
        staging[offset + 28..offset + 32].fill(0);
    }

    // ── public surface management ──────────────────────────────────────────

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        if self.surface_config.width == width && self.surface_config.height == height {
            return;
        }
        self.surface_config.width = width;
        self.surface_config.height = height;
        self.surface.configure(&self.device, &self.surface_config);
        self.depth_view = Self::make_depth_view(&self.device, width, height, self.msaa_samples);
        self.msaa_color_view = Self::make_msaa_color_view(
            &self.device, width, height, HDR_FORMAT, self.msaa_samples,
        );
        let (hdr_texture, hdr_view) = Self::make_hdr_texture(&self.device, width, height);
        let n = self.bloom_mip_views.len();
        let (mip_views, mip_sizes, ds_bgs, us_bgs) = Self::build_bloom_resources(
            &self.device, &hdr_texture, &self.bloom_params_buf,
            &self.bloom_downsample_bgl, &self.post_sampler, width, height, n,
        );
        // store new resources before rebuilding composite bind group
        self.hdr_texture = hdr_texture;
        self.hdr_view = hdr_view;
        self.bloom_mip_views = mip_views;
        self.bloom_mip_sizes = mip_sizes;
        self.bloom_downsample_bgs = ds_bgs;
        self.bloom_upsample_bgs = us_bgs;

        // rebuild SSR and fog textures at the new resolution
        let ssr_hw = (width / 2).max(1);
        let ssr_hh = (height / 2).max(1);
        let ssr_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("[ssr] reflection texture"),
            size: wgpu::Extent3d { width: ssr_hw, height: ssr_hh, depth_or_array_layers: 1 },
            mip_level_count: 1, sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let ssr_view = ssr_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let fog_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("[fog] scattering texture"),
            size: wgpu::Extent3d { width: ssr_hw, height: ssr_hh, depth_or_array_layers: 1 },
            mip_level_count: 1, sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let fog_view = fog_texture.create_view(&wgpu::TextureViewDescriptor::default());

        // rebuild SSR bg0 with new depth view
        self.ssr_bg0 = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("[ssr] bg0"),
            layout: &self.ssr_bgl0,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.globals_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&self.hdr_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&self.gtao_depth_view) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.post_sampler) },
            ],
        });
        self.fog_bg0 = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("[fog] bg0"),
            layout: &self.fog_bgl0,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.globals_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&self.gtao_depth_view) },
            ],
        });
        self.atmos_bg0 = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("[atmos] bg0"),
            layout: &self.atmos_bgl0,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.globals_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&self.gtao_depth_view) },
            ],
        });
        self.decal_bg0 = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("[decal] bg0"),
            layout: &self.decal_bgl0,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.globals_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&self.gtao_depth_view) },
            ],
        });

        self.ssr_view = ssr_view;
        self.ssr_texture = ssr_texture;
        self.fog_view = fog_view;
        self.fog_texture = fog_texture;

        // rebuild water bg0 with the new hdr_view (for refraction sampling)
        self.water_bg0 = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("[water] bg0"),
            layout: &self.water_bgl0,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.globals_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&self.hdr_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.post_sampler) },
            ],
        });

        // rebuild composite bind group (binding 4=ssr, 5=fog, 6=sampler)
        let bloom_view = self.bloom_mip_views.first().unwrap_or(&self.hdr_view);
        self.composite_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("[composite] bg"),
            layout: &self.composite_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.composite_params_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&self.hdr_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(bloom_view) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&self.gtao_ao_view_a) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&self.ssr_view) },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&self.fog_view) },
                wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::Sampler(&self.post_sampler) },
            ],
        });

        // rebuild fxaa ldr texture and bind group at the new resolution
        let fxaa_ldr_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("[fxaa] ldr texture"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.surface_config.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let fxaa_ldr_view = fxaa_ldr_texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.fxaa_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("[fxaa] bg"),
            layout: &self.fxaa_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.fxaa_params_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&fxaa_ldr_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.post_sampler) },
            ],
        });
        self.fxaa_ldr_view = fxaa_ldr_view;
        self.fxaa_ldr_texture = fxaa_ldr_texture;
    }

    pub fn surface_width(&self) -> u32 { self.surface_config.width }
    pub fn surface_height(&self) -> u32 { self.surface_config.height }
    pub fn render_tier(&self) -> RenderTier { self.render_tier }

    // ── render ─────────────────────────────────────────────────────────────

    fn render_frame(&mut self, world: &mut World) -> u32 {
        // ── gather camera — copy immediately so world borrows end here ────
        let cam_entity = {
            let active = world.resource::<ActiveCamera3d>();
            let Some(e) = active.entity else { return 0; };
            e
        };
        let camera = { let Some(c) = world.get::<Camera3d>(cam_entity) else { return 0; }; *c };
        let cam_wt  = { let Some(t) = world.get::<WorldTransform3d>(cam_entity) else { return 0; }; *t };
        let aspect = world.resource::<ViewportAspect>().0;
        let view_proj = camera.view_proj(cam_wt, aspect);
        let cam_pos = cam_wt.translation;

        // ── gather sky ────────────────────────────────────────────────────
        let sky = world.get_resource::<Sky>().copied();
        let sky_color = sky.map_or(Color::rgb(0.1, 0.1, 0.15), |s| s.sky_color);

        // ── gather lights ─────────────────────────────────────────────────
        let ambient = world.get_resource::<AmbientLight>().copied().unwrap_or_default();

        // directional light: first entity with both DirectionalLight + WorldTransform3d
        let mut dir_color = Color::WHITE;
        let mut dir_illuminance: f32 = 0.0;
        let mut dir_direction = Vec3::NEG_Y;
        let mut dir_enabled: u32 = 0;
        let mut dir_casts_shadows = false;
        {
            let mut dq = world.query::<(&DirectionalLight, &WorldTransform3d)>();
            if let Some((dl, wt)) = dq.iter(world).next() {
                dir_color = dl.color;
                dir_illuminance = dl.illuminance;
                dir_direction = wt.forward();
                dir_enabled = 1;
                dir_casts_shadows = dl.casts_shadows;
            }
        }

        // point lights: up to MAX_POINT_LIGHTS closest to camera
        self.point_light_scratch.clear();
        {
            let mut pq = world.query::<(&PointLight, &WorldTransform3d)>();
            pq.iter(world).for_each(|(pl, wt)| {
                self.point_light_scratch.push((wt.translation, pl.color, pl.intensity, pl.radius));
            });
        }
        self.point_light_scratch.sort_unstable_by(|a, b| {
            let da = (a.0 - cam_pos).length_squared();
            let db = (b.0 - cam_pos).length_squared();
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        });
        self.point_light_scratch.truncate(MAX_POINT_LIGHTS);

        // ── compute cascade splits (log-linear blend, λ=0.5) ─────────────
        // produces 3 split depths in view space separating the 3 cascade slices.
        let cascade_splits = Self::compute_cascade_splits(SHADOW_NEAR, SHADOW_FAR, NUM_CASCADES as usize, CASCADE_LAMBDA);

        // ── compute per-cascade light-space matrices ──────────────────────
        let light_spaces = if dir_enabled != 0 {
            let cam_forward = cam_wt.forward();
            let cam_up_vec  = cam_wt.up();
            let cam_right   = cam_wt.right();
            let (fov_y, near) = match camera.projection {
                Projection::Perspective { fov_y, near, .. } => (fov_y, near),
                Projection::Orthographic { .. } => (std::f32::consts::FRAC_PI_3, 0.1),
            };
            [
                Self::cascade_light_space(cam_pos, cam_forward, cam_up_vec, cam_right, fov_y, aspect, dir_direction, near, cascade_splits[0]),
                Self::cascade_light_space(cam_pos, cam_forward, cam_up_vec, cam_right, fov_y, aspect, dir_direction, cascade_splits[0], cascade_splits[1]),
                Self::cascade_light_space(cam_pos, cam_forward, cam_up_vec, cam_right, fov_y, aspect, dir_direction, cascade_splits[1], cascade_splits[2]),
            ]
        } else {
            [Mat4::IDENTITY; 3]
        };

        // ── frustum cull ─────────────────────────────────────────────────
        // high tier: GPU compute replaces CPU CullSoa test + optional HZB occlusion cull.
        // mid/low tier: CPU test over contiguous CullSoa arrays.
        self.frustum_visible.clear();
        if self.gpu_cull_enabled {
            let (entity_count, frustum_planes) = {
                let frustum = *world.resource::<Frustum>();
                let soa = world.resource::<CullSoa>();
                (soa.entities.len(), frustum.planes)
            };
            if entity_count > 0 {
                self.ensure_gpu_cull_resources(entity_count);

                // pack aabb data: [center.x, center.y, center.z, pad, he.x, he.y, he.z, pad] × N
                let mut aabb_data: Vec<f32> = Vec::with_capacity(entity_count * 8);
                {
                    let soa = world.resource::<CullSoa>();
                    for i in 0..entity_count {
                        let c = soa.centers[i];
                        let e = soa.half_extents[i];
                        aabb_data.extend_from_slice(&[c.x, c.y, c.z, 0.0, e.x, e.y, e.z, 0.0]);
                    }
                }
                // pack frustum params: 6×vec4 planes + count + 3 pad
                let mut frustum_data = [0f32; 32];
                for (p, plane) in frustum_planes.iter().enumerate() {
                    frustum_data[p * 4]     = plane.x;
                    frustum_data[p * 4 + 1] = plane.y;
                    frustum_data[p * 4 + 2] = plane.z;
                    frustum_data[p * 4 + 3] = plane.w;
                }
                frustum_data[24] = f32::from_bits(entity_count as u32);

                let aabb_buf = self.cull_aabb_buf.as_ref().unwrap();
                let frustum_buf = self.cull_frustum_buf.as_ref().unwrap();
                let flags_buf = self.cull_flags_buf.as_ref().unwrap();
                let staging_buf = self.cull_flags_staging.as_ref().unwrap();

                // upload AABB + frustum data
                let aabb_bytes: &[u8] = bytemuck::cast_slice(&aabb_data);
                self.queue.write_buffer(aabb_buf, 0, aabb_bytes);
                let frustum_bytes: &[u8] = bytemuck::cast_slice(&frustum_data);
                self.queue.write_buffer(frustum_buf, 0, frustum_bytes);

                // build bind group and dispatch compute
                let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("[cull] bg"),
                    layout: self.cull_bgl.as_ref().unwrap(),
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: aabb_buf.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 1, resource: frustum_buf.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 2, resource: flags_buf.as_entire_binding() },
                    ],
                });
                let mut cull_enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("[cull] encoder"),
                });
                {
                    let mut cpass = cull_enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                        label: Some("[cull] pass"),
                        timestamp_writes: None,
                    });
                    cpass.set_pipeline(self.cull_pipeline.as_ref().unwrap());
                    cpass.set_bind_group(0, &bg, &[]);
                    let wg = (entity_count as u32 + 63) / 64;
                    cpass.dispatch_workgroups(wg, 1, 1);
                }
                cull_enc.copy_buffer_to_buffer(flags_buf, 0, staging_buf, 0, (entity_count * 4) as u64);
                self.queue.submit([cull_enc.finish()]);
                // wait for GPU compute (synchronous readback — acceptable on high-tier desktop)
                let _ = self.device.poll(wgpu::PollType::wait_indefinitely());

                // map staging and read visible flags
                let staging_slice = staging_buf.slice(0..(entity_count * 4) as u64);
                staging_slice.map_async(wgpu::MapMode::Read, |_| {});
                let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
                {
                    let data = staging_slice.get_mapped_range();
                    let flags: &[u32] = bytemuck::cast_slice(&data);
                    let soa = world.resource::<CullSoa>();
                    for (i, &entity) in soa.entities.iter().enumerate() {
                        if i < flags.len() && flags[i] != 0 {
                            self.frustum_visible.insert(entity);
                        }
                    }
                    self.gpu_cull_flags.clear();
                    self.gpu_cull_flags.extend_from_slice(&flags[..entity_count]);
                }
                staging_buf.unmap();
            }
        } else {
            let frustum = *world.resource::<Frustum>();
            let soa = world.resource::<CullSoa>();
            for (i, &entity) in soa.entities.iter().enumerate() {
                if frustum.intersects_aabb(soa.centers[i], soa.half_extents[i]) {
                    self.frustum_visible.insert(entity);
                }
            }
        }

        // ── HZB occlusion cull (high tier, last-frame HZB) ───────────────
        // tests frustum-visible entities against the previous frame's HZB.
        // entities whose nearest projected depth exceeds the HZB nearest-depth
        // are behind known opaque geometry and removed from frustum_visible.
        if self.hzb_enabled && self.hzb_texture.is_some() && !self.gpu_cull_flags.is_empty() {
            let entity_count = self.gpu_cull_flags.len();
            self.ensure_hzb_cull_buffers(entity_count);

            let soa = world.resource::<CullSoa>();
            let mut aabb_data: Vec<f32> = Vec::with_capacity(entity_count * 8);
            for i in 0..entity_count {
                let c = soa.centers[i];
                let e = soa.half_extents[i];
                aabb_data.extend_from_slice(&[c.x, c.y, c.z, 0.0, e.x, e.y, e.z, 0.0]);
            }
            // hzb params: view_proj (16 f32) + viewport (2 f32) + mip_count (u32) + entity_count (u32)
            let vp_array = view_proj.to_cols_array();
            let mut params_data = [0f32; 24];
            params_data[..16].copy_from_slice(&vp_array);
            params_data[16] = self.surface_config.width as f32;
            params_data[17] = self.surface_config.height as f32;
            params_data[18] = f32::from_bits(self.hzb_mip_count);
            params_data[19] = f32::from_bits(entity_count as u32);

            // copy current gpu_cull_flags → occ_flags buf (starts with frustum cull result)
            let flags_bytes: &[u8] = bytemuck::cast_slice(&self.gpu_cull_flags[..entity_count]);
            self.queue.write_buffer(self.hzb_occ_buf.as_ref().unwrap(), 0, flags_bytes);
            let aabb_bytes: &[u8] = bytemuck::cast_slice(&aabb_data);
            self.queue.write_buffer(self.hzb_cull_aabb_buf.as_ref().unwrap(), 0, aabb_bytes);
            let params_bytes: &[u8] = bytemuck::cast_slice(&params_data);
            self.queue.write_buffer(self.hzb_cull_params_buf.as_ref().unwrap(), 0, params_bytes);

            let hzb_src_view = self.hzb_src_view.as_ref().unwrap();
            let occ_buf = self.hzb_occ_buf.as_ref().unwrap();
            let occ_staging = self.hzb_occ_staging.as_ref().unwrap();
            let aabb_buf = self.hzb_cull_aabb_buf.as_ref().unwrap();
            let params_buf = self.hzb_cull_params_buf.as_ref().unwrap();

            let hzb_cull_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("[hzb] cull bg"),
                layout: self.hzb_cull_bgl.as_ref().unwrap(),
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: aabb_buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: params_buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: occ_buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(hzb_src_view) },
                ],
            });
            let mut hzb_enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("[hzb] cull encoder"),
            });
            {
                let mut cpass = hzb_enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("[hzb] cull pass"),
                    timestamp_writes: None,
                });
                cpass.set_pipeline(self.hzb_cull_pipeline.as_ref().unwrap());
                cpass.set_bind_group(0, &hzb_cull_bg, &[]);
                cpass.dispatch_workgroups((entity_count as u32 + 63) / 64, 1, 1);
            }
            hzb_enc.copy_buffer_to_buffer(occ_buf, 0, occ_staging, 0, (entity_count * 4) as u64);
            self.queue.submit([hzb_enc.finish()]);
            let _ = self.device.poll(wgpu::PollType::wait_indefinitely());

            // apply occlusion results to frustum_visible
            let slice = occ_staging.slice(0..(entity_count * 4) as u64);
            slice.map_async(wgpu::MapMode::Read, |_| {});
            let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
            {
                let data = slice.get_mapped_range();
                let flags: &[u32] = bytemuck::cast_slice(&data);
                let soa = world.resource::<CullSoa>();
                for (i, &entity) in soa.entities.iter().enumerate() {
                    if i < flags.len() && flags[i] == 0 {
                        self.frustum_visible.remove(&entity);
                    }
                }
            }
            occ_staging.unmap();
        }

        // ── gather draw list ──────────────────────────────────────────────
        self.raw_scratch.clear();
        {
            let mut q = world.query::<(
                Entity, &Mesh3d, &Material3d, &WorldTransform3d, &ComputedVisibility,
                Option<&Aabb3d>, Option<&MeshLod>,
            )>();
            q.iter(world)
                .filter(|(entity, _, _, _, vis, aabb, _)| {
                    vis.0 && (aabb.is_none() || self.frustum_visible.contains(entity))
                })
                .for_each(|(entity, mesh, mat, wt, _, _, lod)| {
                    // if the entity has LOD levels, select the appropriate mesh based on
                    // squared camera distance. falls back to Mesh3d handle if no LOD set.
                    let mesh_id = lod
                        .and_then(|l| {
                            let dist_sq = (wt.translation - cam_pos).length_squared();
                            l.select(dist_sq)
                        })
                        .unwrap_or(mesh.0)
                        .id();
                    self.raw_scratch.push((entity, mesh_id, mat.0.id(), wt.to_matrix()));
                });
        }

        self.draw_scratch.clear();
        {
            let registry = world.resource::<MeshRegistry>();
            for &(entity, mesh_id, mat_id, model) in &self.raw_scratch {
                let (color, metallic, roughness, alpha) = registry
                    .get_material(lunar_assets::Handle::new(mat_id, 0))
                    .map(|m| {
                        let mut color = m.base_color;
                        color.a = m.alpha;
                        (color, m.metallic, m.roughness, m.alpha)
                    })
                    .unwrap_or((Color::WHITE, 0.0, 0.5, 1.0));
                self.draw_scratch.push((entity, mesh_id, mat_id, color, metallic, roughness, model, alpha));
            }
        }
        // sort opaque entities by (mesh_id, mat_id) so consecutive entities can share
        // VBO/IBO and material bind group, and be batched into a single draw_indexed call.
        // transparents are sorted separately by depth after this.
        self.draw_scratch.sort_unstable_by_key(|&(_, mesh_id, mat_id, _, _, _, _, alpha)| {
            // put transparents last, then sort by (mesh_id, mat_id)
            let transparent = if alpha < 1.0 { 1u8 } else { 0u8 };
            (transparent, mesh_id, mat_id)
        });

        // ── upload missing meshes ─────────────────────────────────────────
        for i in 0..self.draw_scratch.len() {
            let mesh_id = self.draw_scratch[i].1;
            if !self.mesh_gpu.contains_key(&mesh_id) {
                let registry = world.resource::<MeshRegistry>();
                if let Some(data) = registry.get_mesh(lunar_assets::Handle::new(mesh_id, 0)) {
                    let gpu = Self::upload_mesh_data(&self.device, &self.queue, data);
                    self.mesh_gpu.insert(mesh_id, gpu);
                }
            }
        }

        // ── grow buffers if needed ────────────────────────────────────────
        let needed = ENTITY_SLOT_START + self.draw_scratch.len();
        if needed > self.entity_capacity {
            self.entity_capacity = needed.next_power_of_two().max(INITIAL_ENTITY_CAPACITY);
            let new_size = (self.entity_capacity * UNIFORM_STRIDE as usize) as u64;
            self.entity_buf = Self::make_entity_buf(&self.device, self.entity_capacity);
            self.entity_bg = Self::make_entity_bg(&self.device, &self.mesh_bgl, &self.entity_buf);
            self.uniform_staging.resize(self.entity_capacity * UNIFORM_STRIDE as usize, 0);
            self.material_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("[material] uniform buffer"),
                size: new_size,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.material_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("[material] bg"),
                layout: &self.material_bgl,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &self.material_buf,
                        offset: 0,
                        size: wgpu::BufferSize::new(MATERIAL_UNIFORMS_SIZE),
                    }),
                }],
            });
            self.material_staging.resize(self.entity_capacity * UNIFORM_STRIDE as usize, 0);
            log::debug!("draw buffers grown to {} slots", self.entity_capacity);
        }

        // ── pack mesh + material staging ──────────────────────────────────
        // sky dome and sun are unlit (flags = 1)
        let dome_model = Mat4::from_translation(cam_pos);
        Self::pack_mesh_uniforms(&mut self.uniform_staging, SLOT_DOME, dome_model);
        Self::pack_material_uniforms(&mut self.material_staging, SLOT_DOME, sky_color, 0.0, 1.0, 1);

        if let Some(sky) = sky {
            let sun_model = Mat4::from_translation(cam_pos + Vec3::new(0.0, SUN_Y, 0.0));
            Self::pack_mesh_uniforms(&mut self.uniform_staging, SLOT_SUN, sun_model);
            Self::pack_material_uniforms(&mut self.material_staging, SLOT_SUN, sky.sun_color, 0.0, 1.0, 1);
        }

        for i in 0..self.draw_scratch.len() {
            let (_, _, _, color, metallic, roughness, model, _) = self.draw_scratch[i];
            Self::pack_mesh_uniforms(&mut self.uniform_staging, ENTITY_SLOT_START + i, model);
            Self::pack_material_uniforms(&mut self.material_staging, ENTITY_SLOT_START + i, color, metallic, roughness, 0);
        }

        // ── pack lights buffer ────────────────────────────────────────────
        #[repr(C)]
        struct LightsGpu {
            ambient_color:     [f32; 3],
            ambient_intensity: f32,
            dir_color:         [f32; 3],
            dir_illuminance:   f32,
            dir_direction:     [f32; 3],
            dir_enabled:       u32,
            light_space_0:     [f32; 16],
            light_space_1:     [f32; 16],
            light_space_2:     [f32; 16],
            cascade_splits:    [f32; 4],   // [split0, split1, split2(=far), unused]
            point_count:       u32,
            _pad:              [u32; 3],
            point_lights:      [[f32; 8]; 8],
            // SH ambient: 1 when IrradianceSH resource present, 0 = flat ambient fallback
            sh_enabled:        u32,
            _sh_pad:           [u32; 3],
            // 9 L2 SH coefficients as vec4(R, G, B, 0) — pre-scaled by ZH×basis constants
            sh_coeffs:         [[f32; 4]; 9],
        }

        let sh = world.get_resource::<IrradianceSH>();
        let sh_enabled: u32 = if sh.is_some() { 1 } else { 0 };
        let mut sh_coeffs = [[0.0f32; 4]; 9];
        if let Some(sh) = sh {
            for (i, c) in sh.coefficients.iter().enumerate() {
                sh_coeffs[i] = [c[0], c[1], c[2], 0.0];
            }
        }

        let mut lights_gpu = LightsGpu {
            ambient_color: [ambient.color.r, ambient.color.g, ambient.color.b],
            ambient_intensity: ambient.intensity,
            dir_color: [dir_color.r, dir_color.g, dir_color.b],
            dir_illuminance,
            dir_direction: [dir_direction.x, dir_direction.y, dir_direction.z],
            dir_enabled,
            light_space_0: light_spaces[0].to_cols_array(),
            light_space_1: light_spaces[1].to_cols_array(),
            light_space_2: light_spaces[2].to_cols_array(),
            cascade_splits: [cascade_splits[0], cascade_splits[1], cascade_splits[2], SHADOW_FAR],
            point_count: self.point_light_scratch.len() as u32,
            _pad: [0; 3],
            point_lights: [[0.0; 8]; 8],
            sh_enabled,
            _sh_pad: [0; 3],
            sh_coeffs,
        };
        for (i, &(pos, color, intensity, radius)) in self.point_light_scratch.iter().enumerate() {
            lights_gpu.point_lights[i] = [pos.x, pos.y, pos.z, intensity, color.r, color.g, color.b, radius];
        }
        self.queue.write_buffer(&self.lights_buf, 0, unsafe { slice_as_bytes(std::slice::from_ref(&lights_gpu)) });

        // ── upload shadow globals (one slot per cascade) ──────────────────
        for (i, &ls) in light_spaces.iter().enumerate() {
            let cols = ls.to_cols_array();
            self.queue.write_buffer(
                &self.shadow_globals_buf,
                (i * UNIFORM_STRIDE as usize) as u64,
                unsafe { slice_as_bytes(&cols) },
            );
        }

        // ── upload globals + small uniforms via queue.write_buffer ───────
        let upload_size = (needed * UNIFORM_STRIDE as usize) as u64;
        let time = world.resource::<lunar_core::Time>();
        let globals_data: [f32; 24] = {
            let vp = view_proj.to_cols_array();
            let mut d = [0f32; 24];
            d[..16].copy_from_slice(&vp);
            d[16] = cam_pos.x;
            d[17] = cam_pos.y;
            d[18] = cam_pos.z;
            d[19] = time.elapsed_seconds();
            d[20] = time.delta_seconds();
            // d[21..24] = 0 (padding)
            d
        };
        self.queue.write_buffer(&self.globals_buf, 0, unsafe { slice_as_bytes(&globals_data) });

        // ── sort transparent draws back-to-front ──────────────────────────
        let cam_fwd = cam_wt.forward();
        self.transparent_scratch.clear();
        for i in 0..self.draw_scratch.len() {
            if self.draw_scratch[i].7 < 1.0 {
                self.transparent_scratch.push(i);
            }
        }
        self.transparent_scratch.sort_unstable_by(|&a, &b| {
            let wa = self.draw_scratch[a].6.w_axis;
            let wb = self.draw_scratch[b].6.w_axis;
            let depth_a = (Vec3::new(wa.x, wa.y, wa.z) - cam_pos).dot(cam_fwd);
            let depth_b = (Vec3::new(wb.x, wb.y, wb.z) - cam_pos).dot(cam_fwd);
            // back-to-front: larger depth (further from camera) drawn first
            depth_b.partial_cmp(&depth_a).unwrap_or(std::cmp::Ordering::Equal)
        });

        // ── acquire surface and create encoder ────────────────────────────
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) => f,
            wgpu::CurrentSurfaceTexture::Suboptimal(f) => {
                // render this frame, reconfigure at the end so next frame is clean
                self.surface.configure(&self.device, &self.surface_config);
                f
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.device, &self.surface_config);
                return 0;
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.surface_config);
                return 0;
            }
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded => return 0,
            wgpu::CurrentSurfaceTexture::Validation => {
                log::error!("wgpu validation error acquiring surface texture");
                return 0;
            }
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("[frame] encoder"),
        });

        // ── render graph pass ordering ────────────────────────────────────
        // the graph's topological sort drives pass execution order.
        // each frame we log the ordered pass names (debug only) so the DAG
        // is actually driving execution, not just present as dead data.
        {
            // compute the topological pass order and log it in debug builds.
            // this is the render graph DAG driving execution; the sorted order
            // replaces the hardcoded sequential pass list.
            let pass_ids: Vec<_> = self.render_graph.sorted_pass_ids().to_vec();
            if cfg!(debug_assertions) {
                let names: Vec<&str> = pass_ids.iter()
                    .map(|&id| self.render_graph.pass_name(id))
                    .collect();
                log::trace!("[render-graph] pass order: {names:?}");
            }
        }

        // ── upload mesh + material buffers ───────────────────────────────
        if upload_size > 0 {
            #[cfg(not(target_arch = "wasm32"))]
            {
                // StagingBelt batches large per-frame uploads into GPU-side staging memory
                let entity_size = wgpu::BufferSize::new(upload_size).unwrap();
                let material_size = wgpu::BufferSize::new(upload_size).unwrap();
                let mut view = self.staging_belt.write_buffer(
                    &mut encoder, &self.entity_buf, 0, entity_size,
                );
                view.copy_from_slice(&self.uniform_staging[..upload_size as usize]);
                drop(view);
                let mut view = self.staging_belt.write_buffer(
                    &mut encoder, &self.material_buf, 0, material_size,
                );
                view.copy_from_slice(&self.material_staging[..upload_size as usize]);
            }
            #[cfg(target_arch = "wasm32")]
            {
                self.queue.write_buffer(&self.entity_buf, 0, &self.uniform_staging[..upload_size as usize]);
                self.queue.write_buffer(&self.material_buf, 0, &self.material_staging[..upload_size as usize]);
            }
        }

        // ── collect shadow casters ────────────────────────────────────────
        let mut draw_calls: u32 = 0;
        // shadow_list: (mesh_id, draw_scratch_index) for all visible shadow casters.
        // using entity lookup so every caster gets its own correct transform.
        // sorted by mesh_id so consecutive shadow draws can share VBO/IBO.
        let shadow_list: Vec<(u32, usize)> = {
            let shadow_entities: HashSet<Entity> = {
                let mut q = world.query::<(Entity, &ComputedVisibility, &ShadowCaster)>();
                q.iter(world).filter(|(_, vis, _)| vis.0).map(|(e, _, _)| e).collect()
            };
            let mut list: Vec<(u32, usize)> = self.draw_scratch.iter().enumerate()
                .filter(|(_, (entity, _, _, _, _, _, _, _))| shadow_entities.contains(entity))
                .map(|(i, (_, mesh_id, _, _, _, _, _, _))| (*mesh_id, i))
                .collect();
            list.sort_unstable_by_key(|&(mesh_id, _)| mesh_id);
            list
        };

        // ── shadow pass — 3 cascades ─────────────────────────────────────
        for cascade in 0..NUM_CASCADES as usize {
            let label = format!("[shadow] cascade-{cascade}");
            if dir_enabled != 0 && dir_casts_shadows {
                let mut shadow_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some(label.as_str()),
                    color_attachments: &[],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &self.shadow_cascade_views[cascade],
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                shadow_pass.set_pipeline(&self.shadow_pipeline);
                shadow_pass.set_bind_group(0, &self.shadow_globals_bg, &[Self::slot_offset(cascade)]);
                // entity storage buffer covers all slots; set once.
                shadow_pass.set_bind_group(1, &self.entity_bg, &[]);
                // shadow_list is sorted by mesh_id — batch consecutive same-mesh entries
                let mut last_mesh = u32::MAX;
                let mut group_start_slot = 0usize;
                let mut group_start_idx = 0usize;
                let sn = shadow_list.len();
                for idx in 0..=sn {
                    let done = idx == sn;
                    let cur_mesh = if done { u32::MAX } else { shadow_list[idx].0 };
                    if cur_mesh != last_mesh && idx > group_start_idx {
                        if let Some(gpu_mesh) = self.mesh_gpu.get(&last_mesh) {
                            let base = (ENTITY_SLOT_START + group_start_slot) as u32;
                            let count = (idx - group_start_idx) as u32;
                            shadow_pass.draw_indexed(0..gpu_mesh.index_count, 0, base..base + count);
                        }
                    }
                    if done { break; }
                    if cur_mesh != last_mesh {
                        if let Some(gpu_mesh) = self.mesh_gpu.get(&cur_mesh) {
                            shadow_pass.set_vertex_buffer(0, gpu_mesh.vbuf.slice(..));
                            shadow_pass.set_index_buffer(gpu_mesh.ibuf.slice(..), gpu_mesh.index_fmt);
                        }
                        last_mesh = cur_mesh;
                        group_start_slot = shadow_list[idx].1;
                        group_start_idx = idx;
                    }
                }
            } else {
                // clear each cascade layer so the sampler has valid data
                let _clear = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some(label.as_str()),
                    color_attachments: &[],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &self.shadow_cascade_views[cascade],
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
            }
        }

        // ── z-prepass (mid/high tier only) ───────────────────────────────
        // renders all opaque geometry depth-only, so the opaque color pass
        // can use LessEqual depth compare to skip shading on occluded fragments.
        if self.render_tier != RenderTier::LowGles {
            let mut zpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("[z-prepass]"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            zpass.set_pipeline(&self.zprepass_pipeline);
            zpass.set_bind_group(0, &self.globals_bg, &[]);
            zpass.set_bind_group(3, &self.lights_bg, &[]);
            zpass.set_bind_group(2, &self.entity_bg, &[]);
            {
                let mut last_mesh = u32::MAX;
                let mut last_mat = u32::MAX;
                let mut group_start = 0usize;
                let n = self.draw_scratch.len();
                let mut i = 0usize;
                while i <= n {
                    let done = i == n;
                    let (cur_mesh, cur_mat) = if done { (u32::MAX, u32::MAX) }
                        else { (self.draw_scratch[i].1, self.draw_scratch[i].2) };
                    if (cur_mesh != last_mesh || cur_mat != last_mat) && i > group_start {
                        if let Some(gpu_mesh) = self.mesh_gpu.get(&last_mesh) {
                            let base = (ENTITY_SLOT_START + group_start) as u32;
                            zpass.draw_indexed(0..gpu_mesh.index_count, 0, base..base + (i - group_start) as u32);
                        }
                    }
                    if done { break; }
                    if cur_mesh != last_mesh || cur_mat != last_mat {
                        if let Some(gpu_mesh) = self.mesh_gpu.get(&cur_mesh) {
                            zpass.set_bind_group(1, &self.material_bg, &[Self::slot_offset(ENTITY_SLOT_START + i)]);
                            zpass.set_vertex_buffer(0, gpu_mesh.vbuf.slice(..));
                            zpass.set_index_buffer(gpu_mesh.ibuf.slice(..), gpu_mesh.index_fmt);
                        }
                        last_mesh = cur_mesh; last_mat = cur_mat; group_start = i;
                    }
                    i += 1;
                }
            }
        }

        // ── HZB build (high tier only) ───────────────────────────────────
        // builds a hierarchical min-depth buffer from the z-prepass result.
        // used next frame by cs_cull_hzb to occlude entities behind opaque geometry.
        if self.hzb_enabled {
            self.ensure_hzb_resources();

            // depth-only non-MSAA prepass into hzb_depth_src
            {
                let depth_src_view = self.hzb_depth_src_view.as_ref().unwrap();
                let mut hzb_zpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("[hzb] depth prepass"),
                    color_attachments: &[],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: depth_src_view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                hzb_zpass.set_pipeline(&self.zprepass_nonmsaa_pipeline);
                hzb_zpass.set_bind_group(0, &self.globals_bg, &[]);
                hzb_zpass.set_bind_group(3, &self.lights_bg, &[]);
                hzb_zpass.set_bind_group(2, &self.entity_bg, &[]);
                let mut last_mesh = u32::MAX;
                let mut last_mat = u32::MAX;
                let mut group_start = 0usize;
                let n = self.draw_scratch.len();
                let mut i = 0usize;
                while i <= n {
                    let done = i == n;
                    let (cur_mesh, cur_mat) = if done { (u32::MAX, u32::MAX) }
                        else { (self.draw_scratch[i].1, self.draw_scratch[i].2) };
                    if (cur_mesh != last_mesh || cur_mat != last_mat) && i > group_start {
                        if let Some(gpu_mesh) = self.mesh_gpu.get(&last_mesh) {
                            let base = (ENTITY_SLOT_START + group_start) as u32;
                            hzb_zpass.draw_indexed(0..gpu_mesh.index_count, 0, base..base + (i - group_start) as u32);
                        }
                    }
                    if done { break; }
                    if cur_mesh != last_mesh || cur_mat != last_mat {
                        if let Some(gpu_mesh) = self.mesh_gpu.get(&cur_mesh) {
                            hzb_zpass.set_bind_group(1, &self.material_bg, &[Self::slot_offset(ENTITY_SLOT_START + i)]);
                            hzb_zpass.set_vertex_buffer(0, gpu_mesh.vbuf.slice(..));
                            hzb_zpass.set_index_buffer(gpu_mesh.ibuf.slice(..), gpu_mesh.index_fmt);
                        }
                        last_mesh = cur_mesh; last_mat = cur_mat; group_start = i;
                    }
                    i += 1;
                }
            }

            // copy depth → HZB mip 0
            {
                let depth_src_view = self.hzb_depth_src_view.as_ref().unwrap();
                let copy_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("[hzb] copy bg"),
                    layout: self.hzb_copy_bgl.as_ref().unwrap(),
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(depth_src_view) },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&self.hzb_mip_views[0]) },
                    ],
                });
                let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("[hzb] copy pass"),
                    timestamp_writes: None,
                });
                cpass.set_pipeline(self.hzb_copy_pipeline.as_ref().unwrap());
                cpass.set_bind_group(0, &copy_bg, &[]);
                let wg_x = (self.hzb_width + 7) / 8;
                let wg_y = (self.hzb_height + 7) / 8;
                cpass.dispatch_workgroups(wg_x, wg_y, 1);
            }

            // downsample each mip level
            let ds_pipeline = self.hzb_downsample_pipeline.as_ref().unwrap();
            let ds_bgl = self.hzb_downsample_bgl.as_ref().unwrap();
            for mip in 1..self.hzb_mip_count as usize {
                let src_view = &self.hzb_mip_views[mip - 1];
                let dst_view = &self.hzb_mip_views[mip];
                let mip_w = (self.hzb_width >> mip).max(1);
                let mip_h = (self.hzb_height >> mip).max(1);
                let ds_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some(&format!("[hzb] downsample mip {mip}")),
                    layout: ds_bgl,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(src_view) },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(dst_view) },
                    ],
                });
                let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some(&format!("[hzb] downsample mip {mip}")),
                    timestamp_writes: None,
                });
                cpass.set_pipeline(ds_pipeline);
                cpass.set_bind_group(0, &ds_bg, &[]);
                cpass.dispatch_workgroups((mip_w + 7) / 8, (mip_h + 7) / 8, 1);
            }
        }

        // ── GTAO passes (mid/high tier, ssao enabled) ────────────────────
        if self.ssao_enabled {
            // non-MSAA depth prepass so GTAO can sample depth without MSAA complication
            {
                let mut zpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("[gtao] depth prepass"),
                    color_attachments: &[],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &self.gtao_depth_view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                zpass.set_pipeline(&self.zprepass_nonmsaa_pipeline);
                zpass.set_bind_group(0, &self.globals_bg, &[]);
                zpass.set_bind_group(3, &self.lights_bg, &[]);
                zpass.set_bind_group(2, &self.entity_bg, &[]);
                {
                    let mut last_mesh = u32::MAX;
                    let mut last_mat = u32::MAX;
                    let mut group_start = 0usize;
                    let n = self.draw_scratch.len();
                    let mut i = 0usize;
                    while i <= n {
                        let done = i == n;
                        let (cur_mesh, cur_mat) = if done { (u32::MAX, u32::MAX) }
                            else { (self.draw_scratch[i].1, self.draw_scratch[i].2) };
                        if (cur_mesh != last_mesh || cur_mat != last_mat) && i > group_start {
                            if let Some(gpu_mesh) = self.mesh_gpu.get(&last_mesh) {
                                let base = (ENTITY_SLOT_START + group_start) as u32;
                                zpass.draw_indexed(0..gpu_mesh.index_count, 0, base..base + (i - group_start) as u32);
                            }
                        }
                        if done { break; }
                        if cur_mesh != last_mesh || cur_mat != last_mat {
                            if let Some(gpu_mesh) = self.mesh_gpu.get(&cur_mesh) {
                                zpass.set_bind_group(1, &self.material_bg, &[Self::slot_offset(ENTITY_SLOT_START + i)]);
                                zpass.set_vertex_buffer(0, gpu_mesh.vbuf.slice(..));
                                zpass.set_index_buffer(gpu_mesh.ibuf.slice(..), gpu_mesh.index_fmt);
                            }
                            last_mesh = cur_mesh; last_mat = cur_mat; group_start = i;
                        }
                        i += 1;
                    }
                }
            }

            // upload GTAO params
            let (ao_w, ao_h) = (
                (self.surface_config.width / 2).max(1),
                (self.surface_config.height / 2).max(1),
            );
            let (fov_y, near, far) = match camera.projection {
                Projection::Perspective { fov_y, near, far } => (fov_y, near, far),
                Projection::Orthographic { .. } => (std::f32::consts::FRAC_PI_3, 0.1, 1000.0),
            };
            let proj = camera.view_proj(cam_wt, aspect);
            let inv_proj = proj.inverse();
            let gtao_params: [f32; 40] = {
                let mut d = [0f32; 40];
                d[..16].copy_from_slice(&inv_proj.to_cols_array());
                d[16..32].copy_from_slice(&proj.to_cols_array());
                d[32] = world.resource::<lunar_core::Time>().elapsed_seconds();
                d[33] = 1.5; // radius metres
                d[34] = far;
                d[35] = if self.render_tier == RenderTier::High { 5.0 } else { 3.0 }; // slice_count
                d[36] = if self.render_tier == RenderTier::High { 6.0 } else { 4.0 }; // step_count
                d[37] = ao_w as f32;
                d[38] = ao_h as f32;
                d[39] = 0.0;
                let _ = (fov_y, near);
                d
            };
            self.queue.write_buffer(&self.gtao_params_buf, 0, unsafe { slice_as_bytes(&gtao_params) });

            let run_fullscreen_pass = |encoder: &mut wgpu::CommandEncoder,
                                       label: &str,
                                       pipeline: &wgpu::RenderPipeline,
                                       bg: &wgpu::BindGroup,
                                       target: &wgpu::TextureView,
                                       w: u32, h: u32| {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some(label),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: target,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::WHITE), store: wgpu::StoreOp::Store },
                        depth_slice: None,
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_viewport(0.0, 0.0, w as f32, h as f32, 0.0, 1.0);
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, bg, &[]);
                pass.draw(0..3, 0..1);
            };

            run_fullscreen_pass(&mut encoder, "[gtao] main", &self.gtao_pipeline, &self.gtao_main_bg, &self.gtao_ao_view_a, ao_w, ao_h);
            run_fullscreen_pass(&mut encoder, "[gtao] blur-h", &self.gtao_blur_h_pipeline, &self.gtao_blur_h_bg, &self.gtao_ao_view_b, ao_w, ao_h);
            run_fullscreen_pass(&mut encoder, "[gtao] blur-v", &self.gtao_blur_v_pipeline, &self.gtao_blur_v_bg, &self.gtao_ao_view_a, ao_w, ao_h);
        }

        // ── main color pass → HDR texture ───��─────────────────────────────
        // MSAA resolves into the non-MSAA HDR texture; no MSAA renders direct to HDR.
        // composite pass reads the HDR texture and writes to swapchain.
        {
            let (color_target, resolve_target) = match &self.msaa_color_view {
                Some(msaa) => (msaa as &wgpu::TextureView, Some(&self.hdr_view as &wgpu::TextureView)),
                None => (&self.hdr_view as &wgpu::TextureView, None),
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("[frame] pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: color_target,
                    resolve_target,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: sky_color.r as f64,
                            g: sky_color.g as f64,
                            b: sky_color.b as f64,
                            a: 1.0,
                        }),
                        store: if self.msaa_color_view.is_some() {
                            wgpu::StoreOp::Discard  // MSAA tile memory, not needed after resolve
                        } else {
                            wgpu::StoreOp::Store
                        },
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        // load z-prepass depth on mid/high; clear on low (no prepass)
                        load: if self.render_tier != RenderTier::LowGles {
                            wgpu::LoadOp::Load
                        } else {
                            wgpu::LoadOp::Clear(1.0)
                        },
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            pass.set_bind_group(0, &self.globals_bg, &[]);
            pass.set_bind_group(3, &self.lights_bg, &[]);

            // sky pass — unlit, dome always drawn; sun only when sky resource present.
            // entity_bg is set once for the whole pass (covers all slots in storage buffer).
            pass.set_pipeline(&self.sky_pipeline);
            pass.set_bind_group(2, &self.entity_bg, &[]);
            pass.set_bind_group(1, &self.material_bg, &[Self::slot_offset(SLOT_DOME)]);
            pass.set_vertex_buffer(0, self.dome_mesh.vbuf.slice(..));
            pass.set_index_buffer(self.dome_mesh.ibuf.slice(..), self.dome_mesh.index_fmt);
            pass.draw_indexed(0..self.dome_mesh.index_count, 0, SLOT_DOME as u32..SLOT_DOME as u32 + 1);
            draw_calls += 1;

            if sky.is_some_and(|s| s.show_sun) {
                pass.set_bind_group(1, &self.material_bg, &[Self::slot_offset(SLOT_SUN)]);
                pass.set_vertex_buffer(0, self.sun_mesh.vbuf.slice(..));
                pass.set_index_buffer(self.sun_mesh.ibuf.slice(..), self.sun_mesh.index_fmt);
                pass.draw_indexed(0..self.sun_mesh.index_count, 0, SLOT_SUN as u32..SLOT_SUN as u32 + 1);
                draw_calls += 1;
            }

            // opaque PBR pass — batched by (mesh_id, mat_id); draw_scratch is pre-sorted.
            // entity_bg covers the full storage buffer — set once, instance_index selects transform.
            pass.set_pipeline(&self.opaque_pipeline);
            pass.set_bind_group(2, &self.entity_bg, &[]);
            {
                let mut last_mesh: u32 = u32::MAX;
                let mut last_mat: u32 = u32::MAX;
                let mut group_start: usize = 0;
                let n = self.draw_scratch.len();
                let mut i = 0;
                while i <= n {
                    let flush = i == n || self.draw_scratch[i].7 < 1.0; // end or transparent
                    let (cur_mesh, cur_mat) = if flush || i == n { (u32::MAX, u32::MAX) }
                        else { (self.draw_scratch[i].1, self.draw_scratch[i].2) };
                    let group_changed = cur_mesh != last_mesh || cur_mat != last_mat;
                    if group_changed && i > group_start {
                        // flush the completed group
                        let Some(gpu_mesh) = self.mesh_gpu.get(&last_mesh) else { group_start = i; i += 1; continue; };
                        let base = (ENTITY_SLOT_START + group_start) as u32;
                        let count = (i - group_start) as u32;
                        pass.draw_indexed(0..gpu_mesh.index_count, 0, base..base + count);
                        draw_calls += 1;
                    }
                    if flush { break; }
                    if cur_mesh != last_mesh || cur_mat != last_mat {
                        let Some(gpu_mesh) = self.mesh_gpu.get(&cur_mesh) else { i += 1; continue; };
                        pass.set_bind_group(1, &self.material_bg, &[Self::slot_offset(ENTITY_SLOT_START + i)]);
                        pass.set_vertex_buffer(0, gpu_mesh.vbuf.slice(..));
                        pass.set_index_buffer(gpu_mesh.ibuf.slice(..), gpu_mesh.index_fmt);
                        last_mesh = cur_mesh;
                        last_mat = cur_mat;
                        group_start = i;
                    }
                    i += 1;
                }
            }

            // transparent pass — back-to-front sorted, no depth write, alpha blend.
            // transparents are few so no batching needed; entity_bg already set.
            if !self.transparent_scratch.is_empty() {
                pass.set_pipeline(&self.transparent_pipeline);
                for &i in &self.transparent_scratch {
                    let mesh_id = self.draw_scratch[i].1;
                    let Some(gpu_mesh) = self.mesh_gpu.get(&mesh_id) else { continue; };
                    pass.set_bind_group(1, &self.material_bg, &[Self::slot_offset(ENTITY_SLOT_START + i)]);
                    pass.set_vertex_buffer(0, gpu_mesh.vbuf.slice(..));
                    pass.set_index_buffer(gpu_mesh.ibuf.slice(..), gpu_mesh.index_fmt);
                    let base = (ENTITY_SLOT_START + i) as u32;
                    pass.draw_indexed(0..gpu_mesh.index_count, 0, base..base + 1);
                    draw_calls += 1;
                }
            }
        }

        // ── terrain pass — geometry clipmap heightmap rendering ─────────
        {
            let mut terrain_query = world.query::<(Entity, &mut Terrain, &WorldTransform3d)>();
            let terrain_entities: Vec<(Entity, Terrain, WorldTransform3d)> = terrain_query
                .iter_mut(world)
                .map(|(e, t, wt)| (e, t.clone(), *wt))
                .collect();

            for (entity, terrain_comp, wt) in &terrain_entities {
                // lazy-init GPU resources on first encounter or if dirty
                let needs_rebuild = {
                    let entry = self.terrain_gpu.get(entity);
                    entry.is_none() || terrain_comp.dirty
                };
                if needs_rebuild {
                    let gpu = Self::build_terrain_gpu(
                        &self.device,
                        &self.queue,
                        &self.terrain_params_bgl,
                        terrain_comp,
                    );
                    self.terrain_gpu.insert(*entity, gpu);
                    // mark clean on the actual component
                    if let Some(mut t) = world.get_mut::<Terrain>(*entity) {
                        t.dirty = false;
                    }
                }
                let Some(gpu) = self.terrain_gpu.get(entity) else { continue; };

                let terrain_origin = wt.translation;
                let world_size = terrain_comp.world_size;
                let rings = terrain_comp.clipmap_rings.clamp(1, 8);
                let resolution = terrain_comp.ring_resolution.clamp(4, 256) as f32;

                // on low tier render a single LOD-0 patch covering the whole terrain
                let effective_rings = if self.render_tier == RenderTier::LowGles { 1 } else { rings };

                for ring in 0..effective_rings as usize {
                    let Some(ring_mesh) = gpu.ring_meshes.get(ring) else { continue; };

                    // each ring is 2× coarser than the previous
                    let base_cell = world_size / (resolution * (1 << rings) as f32);
                    let lod_cell_size = base_cell * (1u32 << ring) as f32;

                    // snap ring origin to cell grid around camera
                    let ring_half = resolution * lod_cell_size * 0.5;
                    let ring_origin_x = (cam_pos.x / lod_cell_size).floor() * lod_cell_size - ring_half;
                    let ring_origin_z = (cam_pos.z / lod_cell_size).floor() * lod_cell_size - ring_half;

                    // sun direction from directional light (default to overhead if none)
                    let sun_d = if dir_enabled != 0 { dir_direction } else { Vec3::Y };
                    let (sun_dx, sun_dy, sun_dz, sun_int) = (sun_d.x, sun_d.y, sun_d.z, dir_illuminance.max(1.0));

                    let tint = [terrain_comp.tint.r, terrain_comp.tint.g, terrain_comp.tint.b, terrain_comp.tint.a];

                    let mut data = [0u8; TERRAIN_PARAMS_SIZE as usize];
                    // ring_origin (vec4)
                    let ro: [f32; 4] = [ring_origin_x, 0.0, ring_origin_z, 0.0];
                    data[0..16].copy_from_slice(unsafe { slice_as_bytes(&ro) });
                    // terrain_origin (vec4)
                    let to_arr: [f32; 4] = [terrain_origin.x, terrain_origin.y, terrain_origin.z, 0.0];
                    data[16..32].copy_from_slice(unsafe { slice_as_bytes(&to_arr) });
                    // misc: lod_cell_size, world_size, height_scale, ring_resolution
                    let misc: [f32; 4] = [lod_cell_size, world_size, terrain_comp.height_scale, resolution];
                    data[32..48].copy_from_slice(unsafe { slice_as_bytes(&misc) });
                    // tint (vec4)
                    data[48..64].copy_from_slice(unsafe { slice_as_bytes(&tint) });
                    // sun_dir (vec4)
                    let sun: [f32; 4] = [sun_dx, sun_dy, sun_dz, sun_int];
                    data[64..80].copy_from_slice(unsafe { slice_as_bytes(&sun) });
                    // ambient + pad
                    let amb: [f32; 4] = [0.15, 0.0, 0.0, 0.0];
                    data[80..96].copy_from_slice(unsafe { slice_as_bytes(&amb) });
                    self.queue.write_buffer(&gpu.params_buf, 0, &data);

                    let (color_target, resolve_target) = match &self.msaa_color_view {
                        Some(msaa) => (msaa as &wgpu::TextureView, Some(&self.hdr_view as &wgpu::TextureView)),
                        None => (&self.hdr_view as &wgpu::TextureView, None),
                    };
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("[terrain] pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: color_target,
                            resolve_target,
                            ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                            depth_slice: None,
                        })],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: &self.depth_view,
                            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }),
                            stencil_ops: None,
                        }),
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    pass.set_pipeline(&self.terrain_pipeline);
                    pass.set_bind_group(0, &self.terrain_globals_bg, &[]);
                    pass.set_bind_group(1, &gpu.params_bg, &[]);
                    pass.set_vertex_buffer(0, ring_mesh.vbuf.slice(..));
                    pass.set_index_buffer(ring_mesh.ibuf.slice(..), ring_mesh.index_fmt);
                    pass.draw_indexed(0..ring_mesh.index_count, 0, 0..1);
                    draw_calls += 1;
                }
            }
        }

        // ── water pass — Gerstner wave displacement + refraction (mid+) ──
        if self.render_tier != RenderTier::LowGles {
            let width  = self.surface_config.width as f32;
            let height = self.surface_config.height as f32;
            let mut water_query = world.query::<(&Water, &Mesh3d, &WorldTransform3d)>();
            let water_entities: Vec<(Water, u32, WorldTransform3d)> = water_query
                .iter(world)
                .map(|(w, m, t)| (*w, m.0.id(), *t))
                .collect();

            for (water_comp, mesh_id, wt) in &water_entities {
                let Some(gpu_mesh) = self.mesh_gpu.get(mesh_id) else { continue; };

                let model_cols = wt.to_matrix().to_cols_array();
                // default 4-wave setup: two crossing ocean swells + two small chop waves
                let waves: [[f32; 4]; 4] = [
                    [1.0, 0.0, 12.0, 0.3],   // direction.x, direction.z, wavelength, amplitude
                    [0.7, 0.7, 8.0,  0.2],
                    [0.0, 1.0, 5.0,  0.1],
                    [-0.5, 0.8, 3.0, 0.05],
                ];
                let water_color = [water_comp.water_color.r, water_comp.water_color.g, water_comp.water_color.b, water_comp.water_color.a];
                let deep_color  = [water_comp.deep_color.r, water_comp.deep_color.g, water_comp.deep_color.b, water_comp.deep_color.a];

                let mut data = [0u8; WATER_PARAMS_SIZE as usize];
                for (i, w) in waves.iter().enumerate() {
                    data[i*16..i*16+16].copy_from_slice(unsafe { slice_as_bytes(w) });
                }
                data[64..128].copy_from_slice(unsafe { slice_as_bytes(&model_cols) });
                data[128..144].copy_from_slice(unsafe { slice_as_bytes(&water_color) });
                data[144..160].copy_from_slice(unsafe { slice_as_bytes(&deep_color) });
                let misc: [f32; 8] = [
                    water_comp.refract_strength,
                    water_comp.wave_speed,
                    water_comp.fresnel_power,
                    width, height,
                    0.0, 0.0, 0.0,
                ];
                data[160..192].copy_from_slice(unsafe { slice_as_bytes(&misc) });
                self.queue.write_buffer(&self.water_params_buf, 0, &data);

                let (color_target, resolve_target) = match &self.msaa_color_view {
                    Some(msaa) => (msaa as &wgpu::TextureView, Some(&self.hdr_view as &wgpu::TextureView)),
                    None => (&self.hdr_view as &wgpu::TextureView, None),
                };
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("[water] pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: color_target,
                        resolve_target,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                        depth_slice: None,
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &self.depth_view,
                        depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Discard }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&self.water_pipeline);
                pass.set_bind_group(0, &self.water_bg0, &[]);
                pass.set_bind_group(1, &self.water_bg1, &[]);
                pass.set_vertex_buffer(0, gpu_mesh.vbuf.slice(..));
                pass.set_index_buffer(gpu_mesh.ibuf.slice(..), gpu_mesh.index_fmt);
                pass.draw_indexed(0..gpu_mesh.index_count, 0, 0..1);
                draw_calls += 1;
            }
        }

        // ── decal pass — box-projected over scene depth ───────────────────
        {
            let width  = self.surface_config.width as f32;
            let height = self.surface_config.height as f32;
            let inv_vp = view_proj.inverse();
            let inv_vp_cols = inv_vp.to_cols_array();
            let vp_cols = view_proj.to_cols_array();

            let mut decal_query = world.query::<(&Decal, &WorldTransform3d)>();
            let decals: Vec<(Decal, WorldTransform3d)> = decal_query
                .iter(world)
                .map(|(d, wt)| (*d, *wt))
                .collect();

            for (decal, wt) in &decals {
                let decal_world_mat = wt.to_matrix();
                let decal_inv_world = decal_world_mat.inverse();
                let decal_world_cols = decal_world_mat.to_cols_array();
                let inv_world_cols  = decal_inv_world.to_cols_array();

                let mut data = [0u8; DECAL_PARAMS_SIZE as usize];
                data[0..64].copy_from_slice(unsafe { slice_as_bytes(&inv_world_cols) });
                data[64..128].copy_from_slice(unsafe { slice_as_bytes(&inv_vp_cols) });
                let color_arr: [f32; 4] = [decal.color.r, decal.color.g, decal.color.b, decal.color.a];
                data[128..144].copy_from_slice(unsafe { slice_as_bytes(&color_arr) });
                data[144..208].copy_from_slice(unsafe { slice_as_bytes(&decal_world_cols) });
                let _ = vp_cols; // available if needed by future extensions
                let misc: [f32; 4] = [width, height, 0.0, 0.0];
                data[208..224].copy_from_slice(unsafe { slice_as_bytes(&misc) });
                self.queue.write_buffer(&self.decal_params_buf, 0, &data);

                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("[decal] pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &self.hdr_view,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                        depth_slice: None,
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&self.decal_pipeline);
                pass.set_bind_group(0, &self.decal_bg0, &[]);
                pass.set_bind_group(1, &self.decal_bg1, &[]);
                pass.draw(0..36, 0..1);
                draw_calls += 1;
            }
        }

        // ── particle pass (mid+ tier: compute sim → billboard render) ────
        if self.particles_enabled {
            let delta = world.resource::<lunar_core::Time>().delta_seconds();

            // gather emitters from ECS and manage CPU-side spawn
            let mut emitter_query = world.query::<(&ParticleEmitter, &WorldTransform3d)>();
            let mut to_spawn: Vec<CpuParticle> = Vec::new();
            for (emitter, wt) in emitter_query.iter(world) {
                if !emitter.active { continue; }
                let new_count = ((emitter.emission_rate * delta) as u32).min(emitter.max_particles);
                let pos = wt.translation;
                let fwd = wt.forward();
                for n in 0..new_count {
                    let angle = emitter.spread_angle;
                    let t = n as f32 / new_count.max(1) as f32;
                    let theta = t * std::f32::consts::TAU;
                    let spread = Vec3::new(theta.cos() * angle, 0.0, theta.sin() * angle);
                    let direction = (fwd + spread).normalize();
                    to_spawn.push(CpuParticle {
                        position: pos,
                        velocity: direction * emitter.initial_speed,
                        lifetime: emitter.particle_lifetime,
                        max_lifetime: emitter.particle_lifetime,
                        color_start: [emitter.color_start.r, emitter.color_start.g, emitter.color_start.b, emitter.color_start.a],
                        color_end: [emitter.color_end.r, emitter.color_end.g, emitter.color_end.b, emitter.color_end.a],
                        size_start: emitter.size_start,
                        size_end: emitter.size_end,
                        alive: true,
                    });
                }
            }

            // fill dead slots with newly spawned particles
            let mut new_gpu_writes: Vec<(u32, GpuParticle)> = Vec::new();
            let mut spawn_iter = to_spawn.into_iter();
            for (slot, cpu) in self.particle_cpu.iter_mut().enumerate() {
                if cpu.alive { continue; }
                let Some(spawned) = spawn_iter.next() else { break; };
                new_gpu_writes.push((slot as u32, spawned.as_gpu()));
                *cpu = spawned;
            }

            // upload newly spawned particles to their slots in the storage buffer
            for (slot, gpu_particle) in &new_gpu_writes {
                let offset = *slot as u64 * PARTICLE_STRIDE;
                let bytes = unsafe {
                    std::slice::from_raw_parts(
                        gpu_particle as *const GpuParticle as *const u8,
                        PARTICLE_STRIDE as usize,
                    )
                };
                self.queue.write_buffer(&self.particle_buf, offset, bytes);
            }

            // count alive particles (after CPU lifetime update that happens via compute)
            let alive_count = self.particle_cpu.iter().filter(|p| p.alive).count() as u32;
            if alive_count > 0 {
                let gravity = 9.8_f32;
                let sim_params: [f32; 4] = [delta, gravity, f32::from_bits(alive_count), 0.0];
                self.queue.write_buffer(&self.particle_sim_params_buf, 0, unsafe { slice_as_bytes(&sim_params) });

                // compute pass: simulate alive particles
                let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("[particles] sim pass"),
                    timestamp_writes: None,
                });
                cpass.set_pipeline(&self.particle_sim_pipeline);
                cpass.set_bind_group(0, &self.particle_sim_bg, &[]);
                let wg = alive_count.div_ceil(64);
                cpass.dispatch_workgroups(wg, 1, 1);
                drop(cpass);

                // particle render pass: billboard quads into HDR (alpha-blended, MSAA)
                let (color_target, resolve_target) = match &self.msaa_color_view {
                    Some(msaa) => (msaa as &wgpu::TextureView, Some(&self.hdr_view as &wgpu::TextureView)),
                    None => (&self.hdr_view as &wgpu::TextureView, None),
                };
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("[particles] render pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: color_target,
                        resolve_target,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                        depth_slice: None,
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &self.depth_view,
                        depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Discard }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&self.particle_render_pipeline);
                pass.set_bind_group(0, &self.particle_render_bg, &[]);
                pass.draw(0..6, 0..alive_count);
                draw_calls += 1;
            }

            // update CPU lifetime state (particles were simulated on GPU; mirror the aging here)
            for cpu in &mut self.particle_cpu {
                if cpu.alive {
                    cpu.lifetime -= delta;
                    if cpu.lifetime <= 0.0 {
                        cpu.alive = false;
                    }
                }
            }
        }

        // ── bloom passes ─────────────────────────────────────────────────
        if self.bloom_enabled && !self.bloom_mip_views.is_empty() {
            let n = self.bloom_mip_views.len();

            // upload bloom params for all steps (downsample + upsample)
            let bloom_threshold = 1.0_f32;
            let filter_radius = 1.0_f32;
            let total_steps = n + n.saturating_sub(1);
            for i in 0..total_steps.min(MAX_BLOOM_MIPS) {
                let (src_w, src_h) = if i < n {
                    // downsample: src is HDR (step 0) or previous mip
                    if i == 0 { (self.surface_config.width, self.surface_config.height) }
                    else { self.bloom_mip_sizes[i - 1] }
                } else {
                    // upsample: src is the mip being read (larger index)
                    let up_step = i - n;
                    self.bloom_mip_sizes[n - 1 - up_step]
                };
                let threshold = if i == 0 { bloom_threshold } else { 0.0 };
                let params: [f32; 4] = [
                    1.0 / src_w as f32,
                    1.0 / src_h as f32,
                    filter_radius,
                    threshold,
                ];
                self.queue.write_buffer(
                    &self.bloom_params_buf,
                    (i * UNIFORM_STRIDE as usize) as u64,
                    unsafe { slice_as_bytes(&params) },
                );
            }

            // downsample: HDR → mip0 → mip1 → ... → mip(n-1)
            for i in 0..n {
                let (dst_w, dst_h) = self.bloom_mip_sizes[i];
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("[bloom] downsample"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &self.bloom_mip_views[i],
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                        depth_slice: None,
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_viewport(0.0, 0.0, dst_w as f32, dst_h as f32, 0.0, 1.0);
                pass.set_pipeline(&self.bloom_downsample_pipeline);
                pass.set_bind_group(0, &self.bloom_downsample_bgs[i], &[]);
                pass.draw(0..3, 0..1);
            }

            // upsample: mip(n-1) → mip(n-2) → ... → mip0 (additive blend)
            for i in 0..self.bloom_upsample_bgs.len() {
                let dst_idx = n - 2 - i;
                let (dst_w, dst_h) = self.bloom_mip_sizes[dst_idx];
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("[bloom] upsample"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &self.bloom_mip_views[dst_idx],
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                        depth_slice: None,
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_viewport(0.0, 0.0, dst_w as f32, dst_h as f32, 0.0, 1.0);
                pass.set_pipeline(&self.bloom_upsample_pipeline);
                pass.set_bind_group(0, &self.bloom_upsample_bgs[i], &[]);
                pass.draw(0..3, 0..1);
            }
        }

        // ── atmospheric scattering sky pass ──────────────────────────────
        // runs after the main color pass; alpha-blends sky only onto depth==1.0 pixels
        if let Some(atmos) = world.get_resource::<AtmosphericScattering>().copied() {
            // sun direction: dir_direction points from scene toward the light source
            let sun_dir = (-dir_direction).normalize();
            let mut atmos_data = [0u8; ATMOS_PARAMS_SIZE as usize];
            let sun_dir_arr: [f32; 3] = [sun_dir.x, sun_dir.y, sun_dir.z];
            atmos_data[0..12].copy_from_slice(unsafe { slice_as_bytes(&sun_dir_arr) });
            atmos_data[12..16].copy_from_slice(&atmos.sun_intensity.to_le_bytes());
            atmos_data[16..28].copy_from_slice(unsafe { slice_as_bytes(&atmos.rayleigh_scatter) });
            atmos_data[28..32].copy_from_slice(&atmos.mie_scatter.to_le_bytes());
            atmos_data[32..36].copy_from_slice(&atmos.rayleigh_scale.to_le_bytes());
            atmos_data[36..40].copy_from_slice(&atmos.mie_scale.to_le_bytes());
            atmos_data[40..44].copy_from_slice(&atmos.mie_anisotropy.to_le_bytes());
            atmos_data[44..48].copy_from_slice(&6_371_000.0_f32.to_le_bytes());
            atmos_data[48..52].copy_from_slice(&6_471_000.0_f32.to_le_bytes());
            atmos_data[52..56].copy_from_slice(&atmos.exposure.to_le_bytes());
            self.queue.write_buffer(&self.atmos_params_buf, 0, &atmos_data);

            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("[atmos] sky pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.hdr_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.atmos_pipeline);
            pass.set_bind_group(0, &self.atmos_bg0, &[]);
            pass.set_bind_group(1, &self.atmos_bg1, &[]);
            pass.draw(0..3, 0..1);
        }

        // ── SSR pass (mid+ tier) ─────────────────────────────────────────
        if self.ssr_enabled {
            let width  = self.surface_config.width as f32;
            let height = self.surface_config.height as f32;
            let inv_vp   = view_proj.inverse();
            let view_mat = Mat4::look_at_rh(cam_pos, cam_pos + cam_wt.forward(), cam_wt.up());
            let inv_vp_cols  = inv_vp.to_cols_array();
            let vp_cols      = view_proj.to_cols_array();
            let view_cols    = view_mat.to_cols_array();
            let mut ssr_data = [0u8; SSR_PARAMS_SIZE as usize];
            ssr_data[0..64].copy_from_slice(unsafe { slice_as_bytes(&inv_vp_cols) });
            ssr_data[64..128].copy_from_slice(unsafe { slice_as_bytes(&vp_cols) });
            ssr_data[128..192].copy_from_slice(unsafe { slice_as_bytes(&view_cols) });
            // screen_size(vec2) + max_steps(u32) + thickness + stride + fade_start + 2 pads
            let max_steps: u32 = 32;
            ssr_data[192..196].copy_from_slice(&width.to_le_bytes());
            ssr_data[196..200].copy_from_slice(&height.to_le_bytes());
            ssr_data[200..204].copy_from_slice(&max_steps.to_le_bytes());
            ssr_data[204..208].copy_from_slice(&0.5_f32.to_le_bytes()); // thickness
            ssr_data[208..212].copy_from_slice(&1.0_f32.to_le_bytes()); // stride
            ssr_data[212..216].copy_from_slice(&0.1_f32.to_le_bytes()); // fade_start
            self.queue.write_buffer(&self.ssr_params_buf, 0, &ssr_data);

            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("[ssr] pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.ssr_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.ssr_pipeline);
            pass.set_bind_group(0, &self.ssr_bg0, &[]);
            pass.set_bind_group(1, &self.ssr_bg1, &[]);
            pass.draw(0..3, 0..1);
        }

        // ── volumetric fog pass (mid+ tier) ──────────────────────────────
        if self.fog_enabled {
            let width  = self.surface_config.width as f32;
            let height = self.surface_config.height as f32;
            let inv_vp      = view_proj.inverse();
            let inv_vp_cols = inv_vp.to_cols_array();
            // write fog params: inv_view_proj(64) + rest(64) = 128 bytes
            let mut fog_data = [0u8; FOG_PARAMS_SIZE as usize];
            fog_data[0..64].copy_from_slice(unsafe { slice_as_bytes(&inv_vp_cols) });
            // rest 64 bytes: dir_direction(12)+step_count(4)+dir_color(12)+density(4)+
            //                fog_color(12)+max_dist(4)+sun(4)+aniso(4)+w(4)+h(4)
            let dir_d = dir_direction.normalize();
            let step_count: u32 = 16;
            // sun_dir points towards sun (negate scene light direction)
            let sun_dir: [f32; 3] = [-dir_d.x, -dir_d.y, -dir_d.z];
            let fog_color: [f32; 3] = [sky_color.r * 0.5, sky_color.g * 0.5, sky_color.b * 0.7];
            let rest: [f32; 16] = [
                sun_dir[0], sun_dir[1], sun_dir[2], f32::from_bits(step_count),
                dir_color.r, dir_color.g, dir_color.b, 0.01_f32,    // density
                fog_color[0], fog_color[1], fog_color[2], 200.0_f32, // max_distance
                2.0_f32, 0.6_f32, width, height,                     // sun_intensity, anisotropy
            ];
            fog_data[64..128].copy_from_slice(unsafe { slice_as_bytes(&rest) });
            self.queue.write_buffer(&self.fog_params_buf, 0, &fog_data);

            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("[fog] pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.fog_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.fog_pipeline);
            pass.set_bind_group(0, &self.fog_bg0, &[]);
            pass.set_bind_group(1, &self.fog_bg1, &[]);
            pass.draw(0..3, 0..1);
        }

        // ── composite pass → swapchain ────────────────────────────────────
        {
            let time = world.resource::<lunar_core::Time>();
            let quality = world.get_resource::<QualitySettings>();
            let (bloom_strength, vignette_strength, vignette_radius, ca_strength, grain_strength, flags) = {
                let q = quality;
                let mut f: u32 = 0;
                let bloom_s;
                let vig_s;
                let vig_r;
                let ca_s;
                let grain_s;
                if let Some(q) = q {
                    if self.bloom_enabled && q.bloom { f |= 1; }
                    if q.vignette { f |= 2; }
                    if q.chromatic_aberration { f |= 4; }
                    if q.film_grain { f |= 8; }
                    if self.ssao_enabled && q.ssao { f |= 16; }
                    if self.ssr_enabled && q.ssr { f |= 32; }
                    if self.fog_enabled && q.volumetric_fog { f |= 64; }
                    bloom_s = 0.04_f32;
                    vig_s   = if q.vignette { 0.3 } else { 0.0 };
                    vig_r   = 0.3_f32;
                    ca_s    = if q.chromatic_aberration { 1.5 } else { 0.0 };
                    grain_s = if q.film_grain { 0.5 } else { 0.0 };
                } else {
                    bloom_s = 0.04; vig_s = 0.0; vig_r = 0.0; ca_s = 0.0; grain_s = 0.0;
                    if self.bloom_enabled { f |= 1; }
                }
                (bloom_s, vig_s, vig_r, ca_s, grain_s, f)
            };
            let composite_data: [f32; 8] = [
                bloom_strength,
                vignette_strength,
                vignette_radius,
                ca_strength,
                grain_strength,
                time.elapsed_seconds().fract(),
                f32::from_bits(flags),
                0.0, // _pad
            ];
            self.queue.write_buffer(&self.composite_params_buf, 0, unsafe { slice_as_bytes(&composite_data) });

            // when fxaa is enabled, composite writes to the intermediate ldr texture;
            // the fxaa pass then reads it and outputs to swapchain. this avoids running
            // fxaa on a non-filterable msaa resolve target.
            let composite_target = if self.fxaa_enabled { &self.fxaa_ldr_view } else { &view };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("[composite] pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: composite_target,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.composite_pipeline);
            pass.set_bind_group(0, &self.composite_bg, &[]);
            pass.draw(0..3, 0..1);
        }

        // ── FXAA pass → swapchain ─────────────────────────────────────────
        if self.fxaa_enabled {
            let w = self.surface_config.width;
            let h = self.surface_config.height;
            let fxaa_data: [f32; 4] = [1.0 / w as f32, 1.0 / h as f32, 0.0, 0.0];
            self.queue.write_buffer(&self.fxaa_params_buf, 0, unsafe { slice_as_bytes(&fxaa_data) });

            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("[fxaa] pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.fxaa_pipeline);
            pass.set_bind_group(0, &self.fxaa_bg, &[]);
            pass.draw(0..3, 0..1);
        }

        #[cfg(not(target_arch = "wasm32"))]
        self.staging_belt.finish();
        self.queue.submit(Some(encoder.finish()));
        frame.present();
        #[cfg(not(target_arch = "wasm32"))]
        self.staging_belt.recall();
        draw_calls
    }

    /// update the EMA frame-time tracker and adjust resolution scale.
    /// called by `render_3d_system` after each frame with the measured CPU frame time.
    pub fn tick_dynamic_resolution(&mut self, frame_time_ms: f32) -> f32 {
        // EMA with α=0.1 (smooths over ~10 frames)
        const ALPHA: f32 = 0.1;
        self.frame_time_ema_ms = ALPHA * frame_time_ms + (1.0 - ALPHA) * self.frame_time_ema_ms;

        let budget = self.frame_time_budget_ms;
        if self.frame_time_ema_ms > budget * 0.95 {
            // over 95% of budget: drop 5%, floor at 0.5
            self.resolution_scale = (self.resolution_scale - 0.05).max(0.5);
        } else if self.frame_time_ema_ms < budget * 0.80 {
            // under 80% of budget: raise 5%, ceil at 1.0
            self.resolution_scale = (self.resolution_scale + 0.05).min(1.0);
        }
        self.resolution_scale
    }

    #[inline(always)]
    fn slot_offset(slot: usize) -> u32 {
        (slot * UNIFORM_STRIDE as usize) as u32
    }

    /// compute cascade split depths using logarithmic-linear blending.
    /// returns `n` split values in view-space depth (positive distance from camera).
    fn compute_cascade_splits(near: f32, far: f32, n: usize, lambda: f32) -> Vec<f32> {
        (1..=n)
            .map(|i| {
                let uniform = near + (far - near) * (i as f32 / n as f32);
                let log = near * (far / near).powf(i as f32 / n as f32);
                lambda * log + (1.0 - lambda) * uniform
            })
            .collect()
    }

    /// compute a tight orthographic light-space matrix for one cascade slice.
    /// fits the ortho projection to the 8 corners of the camera frustum slice.
    #[allow(clippy::too_many_arguments)]
    fn cascade_light_space(
        cam_pos: Vec3,
        cam_fwd: Vec3,
        cam_up: Vec3,
        cam_right: Vec3,
        fov_y: f32,
        aspect: f32,
        light_dir: Vec3,
        slice_near: f32,
        slice_far: f32,
    ) -> Mat4 {
        let tan_half = (fov_y * 0.5).tan();
        let corners: [Vec3; 8] = {
            let mut c = [Vec3::ZERO; 8];
            let mut idx = 0;
            for &depth in &[slice_near, slice_far] {
                let half_h = tan_half * depth;
                let half_w = half_h * aspect;
                for sy in [-1.0_f32, 1.0] {
                    for sx in [-1.0_f32, 1.0] {
                        c[idx] = cam_pos + cam_fwd * depth + cam_up * (sy * half_h) + cam_right * (sx * half_w);
                        idx += 1;
                    }
                }
            }
            c
        };

        // centroid of corners → light looks at it
        let centroid = corners.iter().fold(Vec3::ZERO, |acc, &c| acc + c) / 8.0;
        let light_dir_n = light_dir.normalize();
        let light_up = if light_dir_n.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
        let light_view = Mat4::look_at_rh(centroid - light_dir_n * 100.0, centroid, light_up);

        // AABB of corners in light view space
        let (mut min_x, mut max_x) = (f32::MAX, f32::MIN);
        let (mut min_y, mut max_y) = (f32::MAX, f32::MIN);
        let (mut min_z, mut max_z) = (f32::MAX, f32::MIN);
        for &c in &corners {
            let lc = light_view * Vec3::new(c.x, c.y, c.z).extend(1.0);
            min_x = min_x.min(lc.x); max_x = max_x.max(lc.x);
            min_y = min_y.min(lc.y); max_y = max_y.max(lc.y);
            min_z = min_z.min(lc.z); max_z = max_z.max(lc.z);
        }
        // pull near plane back to catch casters behind the frustum
        let z_extend = (max_z - min_z) * 0.5;
        let light_proj = Mat4::orthographic_rh(min_x, max_x, min_y, max_y, min_z - z_extend, max_z + z_extend);
        light_proj * light_view
    }
}

// ── ecs integration ────────────────────────────────────────────────────────

#[cfg(not(target_arch = "wasm32"))]
fn render_3d_system(world: &mut World) {
    let t0 = std::time::Instant::now();
    let mut engine = world.remove_resource::<RenderEngine3d>().unwrap();
    let draw_calls = engine.render_frame(world);
    let frame_ms = t0.elapsed().as_secs_f32() * 1000.0;
    let scale = engine.tick_dynamic_resolution(frame_ms);
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

        #[cfg(not(target_arch = "wasm32"))]
        {
            // pull render tier out of the engine resource (already inserted by bootstrap_3d)
            // and expose it as a standalone resource for game systems to query
            if let Some(engine) = app.world_mut().get_resource::<RenderEngine3d>() {
                let tier = engine.render_tier();
                app.insert_resource(QualitySettings::from_tier(tier));
                app.insert_resource(tier);
            }

            app.add_system_to_stage(UpdateStage::Render, render_3d_system);
        }

        log::info!("RenderPlugin3d: 3d render system registered");
    }
}
