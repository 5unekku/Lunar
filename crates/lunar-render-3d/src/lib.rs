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

pub use sky::Sky;

use std::collections::{HashMap, HashSet};

use bevy_ecs::prelude::*;
use lunar_3d::{
    Aabb3d, ActiveCamera3d, AmbientLight, Camera3d, ComputedVisibility, CullSoa, DirectionalLight,
    Frustum, IndexBuffer, IrradianceSH, Material3d, Mesh3d, MeshData, MeshRegistry, PointLight,
    Projection, ShadowCaster, Vertex3d, ViewportAspect, WorldTransform3d,
};
use lunar_3d::primitives::{quad_mesh, sphere_mesh};
use lunar_core::{App, GamePlugin, UpdateStage};
use lunar_math::{Color, Mat3, Mat4, Vec3};

const SHADER_SRC: &str = include_str!("shader.wgsl");
const SHADOW_SHADER_SRC: &str = include_str!("shadow.wgsl");
const BLOOM_SHADER_SRC: &str = include_str!("bloom.wgsl");
const COMPOSITE_SHADER_SRC: &str = include_str!("composite.wgsl");
const GTAO_SHADER_SRC: &str = include_str!("gtao.wgsl");

const FXAA_SHADER_SRC: &str = include_str!("fxaa.wgsl");

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

/// group 2: model mat4 (64) + normal matrix as 3×vec4 (48) = 112 bytes.
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

    // transparent pass — alpha < 1.0 entities drawn back-to-front after opaques
    transparent_pipeline: wgpu::RenderPipeline,
    // indices into draw_scratch for transparent entities, sorted back-to-front
    transparent_scratch: Vec<usize>,

    // pipeline cache — persists compiled shader binaries across runs (Vulkan/DX12 only)
    pipeline_cache: Option<wgpu::PipelineCache>,

    // staging belt — explicit frame-temporary upload staging for large buffers
    staging_belt: wgpu::util::StagingBelt,

    // per-frame scratch — cleared at frame start, never reallocated in steady state
    frustum_visible: HashSet<Entity>,
    raw_scratch: Vec<(Entity, u32, u32, Mat4)>,
    // (entity, mesh_id, base_color, metallic, roughness, model, alpha)
    draw_scratch: Vec<(Entity, u32, Color, f32, f32, Mat4, f32)>,
    uniform_staging: Vec<u8>,
    point_light_scratch: Vec<(Vec3, Color, f32, f32)>,
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
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(MESH_UNIFORMS_SIZE),
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
        let pipeline_cache = Self::load_pipeline_cache(&device);

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
            cache: pipeline_cache.as_ref(),
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
            cache: pipeline_cache.as_ref(),
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
            cache: pipeline_cache.as_ref(),
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
            cache: pipeline_cache.as_ref(),
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
            cache: pipeline_cache.as_ref(),
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
            cache: pipeline_cache.as_ref(),
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
            cache: pipeline_cache.as_ref(),
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
            cache: pipeline_cache.as_ref(),
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
            cache: pipeline_cache.as_ref(),
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
            cache: pipeline_cache.as_ref(),
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
                label: Some("[gtao] pipeline"),
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
                cache: pipeline_cache.as_ref(),
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

        // rebuild composite_bg now that ao_view_a is available
        let composite_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("[composite] bg"),
            layout: &composite_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: composite_params_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&hdr_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(bloom_mip_views.first().unwrap_or(&hdr_view)) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&gtao_ao_view_a) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&post_sampler) },
            ],
        });

        // ── sky meshes ─────────────────────────────────────────────────────

        let dome_mesh = Self::upload_mesh_data(&device, &queue, &sphere_mesh(SKY_RADIUS, 32, 16));
        let sun_mesh = Self::upload_mesh_data(&device, &queue, &quad_mesh(40.0, 40.0));

        log::info!(
            "lunar-render-3d initialized: {}×{}, vsync={}, tier={:?}",
            config.width, config.height, config.vsync, render_tier,
        );

        // clone before move into struct — wgpu::Device is Arc-backed, clone is cheap
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
            pipeline_cache,
            // 4 MiB chunk — larger than any single write, handles most scene sizes
            staging_belt: wgpu::util::StagingBelt::new(device_for_belt, 4 * 1024 * 1024),
            frame_time_ema_ms: 16.67,
            resolution_scale: 1.0,
            frame_time_budget_ms: 14.0,
            frustum_visible: HashSet::new(),
            raw_scratch: Vec::new(),
            draw_scratch: Vec::new(),
            uniform_staging,
            point_light_scratch: Vec::new(),
        }
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
    #[allow(clippy::too_many_arguments)]
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
        let actual_mips = mip_count.min(MAX_BLOOM_MIPS).max(1);

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
            label: Some("[draw] entity uniform buffer"),
            size: (capacity * UNIFORM_STRIDE as usize) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
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
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: entity_buf,
                    offset: 0,
                    size: wgpu::BufferSize::new(MESH_UNIFORMS_SIZE),
                }),
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

        // rebuild composite bind group with the new views (binding 3 = GTAO ao, binding 4 = sampler)
        let bloom_view = self.bloom_mip_views.first().unwrap_or(&self.hdr_view);
        self.composite_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("[composite] bg"),
            layout: &self.composite_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.composite_params_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&self.hdr_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(bloom_view) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&self.gtao_ao_view_a) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(&self.post_sampler) },
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

        // ── frustum cull via CullSoa ──────────────────────────────────────
        self.frustum_visible.clear();
        {
            let frustum = *world.resource::<Frustum>();
            let soa = world.resource::<CullSoa>();
            for (i, &entity) in soa.entities.iter().enumerate() {
                if frustum.intersects_aabb(soa.centers[i], soa.half_extents[i]) {
                    self.frustum_visible.insert(entity);
                }
            }
        }

        // ── gather draw list ──────────────────────────────────────────────
        self.raw_scratch.clear();
        {
            let mut q = world.query::<(
                Entity, &Mesh3d, &Material3d, &WorldTransform3d, &ComputedVisibility, Option<&Aabb3d>,
            )>();
            q.iter(world)
                .filter(|(entity, _, _, _, vis, aabb)| {
                    vis.0 && (aabb.is_none() || self.frustum_visible.contains(entity))
                })
                .for_each(|(entity, mesh, mat, wt, _, _)| {
                    self.raw_scratch.push((entity, mesh.0.id(), mat.0.id(), wt.to_matrix()));
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
                self.draw_scratch.push((entity, mesh_id, color, metallic, roughness, model, alpha));
            }
        }

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
            let (_, _, color, metallic, roughness, model, _) = self.draw_scratch[i];
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
            if self.draw_scratch[i].6 < 1.0 {
                self.transparent_scratch.push(i);
            }
        }
        self.transparent_scratch.sort_unstable_by(|&a, &b| {
            let wa = self.draw_scratch[a].5.w_axis;
            let wb = self.draw_scratch[b].5.w_axis;
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

        // ── upload mesh + material buffers via StagingBelt ────────────────
        // StagingBelt batches these large per-frame uploads into GPU-side staging memory,
        // issuing a single copy command per buffer instead of multiple queue.write_buffer calls.
        if upload_size > 0 {
            let entity_size = wgpu::BufferSize::new(upload_size).unwrap();
            let material_size = wgpu::BufferSize::new(upload_size).unwrap();
            {
                let mut view = self.staging_belt.write_buffer(
                    &mut encoder, &self.entity_buf, 0, entity_size,
                );
                view.copy_from_slice(&self.uniform_staging[..upload_size as usize]);
            }
            {
                let mut view = self.staging_belt.write_buffer(
                    &mut encoder, &self.material_buf, 0, material_size,
                );
                view.copy_from_slice(&self.material_staging[..upload_size as usize]);
            }
        }

        // ── collect shadow casters ────────────────────────────────────────
        let mut draw_calls: u32 = 0;
        let shadow_list: Vec<(u32, usize)> = {
            let mut q = world.query::<(Entity, &Mesh3d, &ComputedVisibility, Option<&ShadowCaster>)>();
            q.iter(world)
                .filter(|(_, _, vis, caster)| vis.0 && caster.is_some())
                .filter_map(|(_entity, mesh, _, _)| {
                    let mesh_id = mesh.0.id();
                    let slot = self.draw_scratch.iter().position(|(_, mid, _, _, _, _, _)| *mid == mesh_id)?;
                    Some((mesh_id, slot))
                })
                .collect()
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
                for &(mesh_id, slot) in &shadow_list {
                    let Some(gpu_mesh) = self.mesh_gpu.get(&mesh_id) else { continue; };
                    shadow_pass.set_bind_group(1, &self.entity_bg, &[Self::slot_offset(ENTITY_SLOT_START + slot)]);
                    shadow_pass.set_vertex_buffer(0, gpu_mesh.vbuf.slice(..));
                    shadow_pass.set_index_buffer(gpu_mesh.ibuf.slice(..), gpu_mesh.index_fmt);
                    shadow_pass.draw_indexed(0..gpu_mesh.index_count, 0, 0..1);
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
            for i in 0..self.draw_scratch.len() {
                let mesh_id = self.draw_scratch[i].1;
                let Some(gpu_mesh) = self.mesh_gpu.get(&mesh_id) else { continue; };
                zpass.set_bind_group(1, &self.material_bg, &[Self::slot_offset(ENTITY_SLOT_START + i)]);
                zpass.set_bind_group(2, &self.entity_bg, &[Self::slot_offset(ENTITY_SLOT_START + i)]);
                zpass.set_vertex_buffer(0, gpu_mesh.vbuf.slice(..));
                zpass.set_index_buffer(gpu_mesh.ibuf.slice(..), gpu_mesh.index_fmt);
                zpass.draw_indexed(0..gpu_mesh.index_count, 0, 0..1);
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
                for i in 0..self.draw_scratch.len() {
                    let mesh_id = self.draw_scratch[i].1;
                    let Some(gpu_mesh) = self.mesh_gpu.get(&mesh_id) else { continue; };
                    zpass.set_bind_group(1, &self.material_bg, &[Self::slot_offset(ENTITY_SLOT_START + i)]);
                    zpass.set_bind_group(2, &self.entity_bg, &[Self::slot_offset(ENTITY_SLOT_START + i)]);
                    zpass.set_vertex_buffer(0, gpu_mesh.vbuf.slice(..));
                    zpass.set_index_buffer(gpu_mesh.ibuf.slice(..), gpu_mesh.index_fmt);
                    zpass.draw_indexed(0..gpu_mesh.index_count, 0, 0..1);
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

            // sky pass — unlit, dome always drawn; sun only when sky resource present
            pass.set_pipeline(&self.sky_pipeline);
            pass.set_bind_group(1, &self.material_bg, &[Self::slot_offset(SLOT_DOME)]);
            pass.set_bind_group(2, &self.entity_bg, &[Self::slot_offset(SLOT_DOME)]);
            pass.set_vertex_buffer(0, self.dome_mesh.vbuf.slice(..));
            pass.set_index_buffer(self.dome_mesh.ibuf.slice(..), self.dome_mesh.index_fmt);
            pass.draw_indexed(0..self.dome_mesh.index_count, 0, 0..1);
            draw_calls += 1;

            if sky.is_some_and(|s| s.show_sun) {
                pass.set_bind_group(1, &self.material_bg, &[Self::slot_offset(SLOT_SUN)]);
                pass.set_bind_group(2, &self.entity_bg, &[Self::slot_offset(SLOT_SUN)]);
                pass.set_vertex_buffer(0, self.sun_mesh.vbuf.slice(..));
                pass.set_index_buffer(self.sun_mesh.ibuf.slice(..), self.sun_mesh.index_fmt);
                pass.draw_indexed(0..self.sun_mesh.index_count, 0, 0..1);
                draw_calls += 1;
            }

            // opaque PBR pass — only draw entities with alpha >= 1.0
            pass.set_pipeline(&self.opaque_pipeline);
            for i in 0..self.draw_scratch.len() {
                if self.draw_scratch[i].6 < 1.0 { continue; } // skip transparents
                let mesh_id = self.draw_scratch[i].1;
                let Some(gpu_mesh) = self.mesh_gpu.get(&mesh_id) else { continue; };
                pass.set_bind_group(1, &self.material_bg, &[Self::slot_offset(ENTITY_SLOT_START + i)]);
                pass.set_bind_group(2, &self.entity_bg, &[Self::slot_offset(ENTITY_SLOT_START + i)]);
                pass.set_vertex_buffer(0, gpu_mesh.vbuf.slice(..));
                pass.set_index_buffer(gpu_mesh.ibuf.slice(..), gpu_mesh.index_fmt);
                pass.draw_indexed(0..gpu_mesh.index_count, 0, 0..1);
                draw_calls += 1;
            }

            // transparent pass — back-to-front sorted, no depth write, alpha blend
            if !self.transparent_scratch.is_empty() {
                pass.set_pipeline(&self.transparent_pipeline);
                for &i in &self.transparent_scratch {
                    let mesh_id = self.draw_scratch[i].1;
                    let Some(gpu_mesh) = self.mesh_gpu.get(&mesh_id) else { continue; };
                    pass.set_bind_group(1, &self.material_bg, &[Self::slot_offset(ENTITY_SLOT_START + i)]);
                    pass.set_bind_group(2, &self.entity_bg, &[Self::slot_offset(ENTITY_SLOT_START + i)]);
                    pass.set_vertex_buffer(0, gpu_mesh.vbuf.slice(..));
                    pass.set_index_buffer(gpu_mesh.ibuf.slice(..), gpu_mesh.index_fmt);
                    pass.draw_indexed(0..gpu_mesh.index_count, 0, 0..1);
                    draw_calls += 1;
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

        self.staging_belt.finish();
        self.queue.submit(Some(encoder.finish()));
        frame.present();
        // recycle staging belt memory for the next frame
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
