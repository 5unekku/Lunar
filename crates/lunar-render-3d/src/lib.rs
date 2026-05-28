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
    Aabb3d, ActiveCamera3d, Camera3d, ComputedVisibility, CullSoa, Frustum, IndexBuffer,
    Material3d, Mesh3d, MeshData, MeshRegistry, Vertex3d, ViewportAspect, WorldTransform3d,
};
use lunar_3d::primitives::{quad_mesh, sphere_mesh};
use lunar_core::{App, GamePlugin, UpdateStage};
use lunar_math::{Color, Mat4, Vec3};

const SHADER_SRC: &str = include_str!("shader.wgsl");

const SKY_RADIUS: f32 = 900.0;
const SUN_Y: f32 = 895.0;
const VERTEX_STRIDE: u64 = std::mem::size_of::<Vertex3d>() as u64;

/// bytes of actual draw data per entity: model mat4 (64) + base_color vec4 (16).
const DRAW_UNIFORMS_SIZE: u64 = 80;

/// stride between entity slots in the dynamic UBO.
/// must be >= min_uniform_buffer_offset_alignment (256 in wgpu default limits).
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

/// per-frame rendering statistics. updated by [`render_3d_system`].
#[derive(Resource, Default)]
pub struct RenderInfo3d {
    pub window_width: u32,
    pub window_height: u32,
    pub draw_calls: u32,
    pub fps: f32,
    pub frame_time_ms: f32,
}

// ── render engine ──────────────────────────────────────────────────────────

/// the 3d rendering engine. owns the wgpu device, queue, and surface.
///
/// inserted as a resource by [`RenderPlugin3d`]. game code should not
/// interact with this directly — use [`MeshRegistry`] and ECS components instead.
#[derive(Resource)]
pub struct RenderEngine3d {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    render_tier: RenderTier,

    depth_view: wgpu::TextureView,

    // group 0: view-global (camera view-proj)
    globals_buf: wgpu::Buffer,
    globals_bg: wgpu::BindGroup,

    // group 1: per-draw (dynamic UBO — one slot per entity + 2 for sky)
    draw_bgl: wgpu::BindGroupLayout,
    entity_buf: wgpu::Buffer,
    entity_bg: wgpu::BindGroup,
    entity_capacity: usize,

    opaque_pipeline: wgpu::RenderPipeline,
    sky_pipeline: wgpu::RenderPipeline,

    mesh_gpu: HashMap<u32, GpuMesh>,
    dome_mesh: GpuMesh,
    sun_mesh: GpuMesh,

    // per-frame scratch — cleared at frame start, never reallocated in steady state
    frustum_visible: HashSet<Entity>,
    raw_scratch: Vec<(Entity, u32, u32, Mat4)>,
    draw_scratch: Vec<(Entity, u32, Color, Mat4)>,
    uniform_staging: Vec<u8>,
}

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

        let depth_view = Self::make_depth_view(&device, config.width, config.height);

        // ── bind group layouts ─────────────────────────────────────────────

        let globals_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("[globals] bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(64),
                },
                count: None,
            }],
        });

        // dynamic offset: one bind group covers the whole entity buffer;
        // each draw call supplies a byte offset to select its slot.
        let draw_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("[draw] bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(DRAW_UNIFORMS_SIZE),
                },
                count: None,
            }],
        });

        // ── globals buffer ─────────────────────────────────────────────────

        let globals_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("[globals] view-proj"),
            size: 64,
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

        // ── entity uniform buffer ──────────────────────────────────────────

        let entity_capacity = INITIAL_ENTITY_CAPACITY;
        let entity_buf = Self::make_entity_buf(&device, entity_capacity);
        let entity_bg = Self::make_entity_bg(&device, &draw_bgl, &entity_buf);
        let uniform_staging = vec![0u8; entity_capacity * UNIFORM_STRIDE as usize];

        // ── pipelines ──────────────────────────────────────────────────────

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("3d unlit shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_SRC.into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("3d pipeline layout"),
            bind_group_layouts: &[Some(&globals_bgl), Some(&draw_bgl)],
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
                    format,
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
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            cache: None,
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
                    format,
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
            multisample: wgpu::MultisampleState::default(),
            cache: None,
            multiview_mask: None,
        });

        // ── sky meshes ─────────────────────────────────────────────────────

        let dome_mesh = Self::upload_mesh_data(&device, &queue, &sphere_mesh(SKY_RADIUS, 32, 16));
        let sun_mesh = Self::upload_mesh_data(&device, &queue, &quad_mesh(40.0, 40.0));

        log::info!(
            "lunar-render-3d initialized: {}×{}, vsync={}, tier={:?}",
            config.width, config.height, config.vsync, render_tier,
        );

        Self {
            device,
            queue,
            surface,
            surface_config,
            render_tier,
            depth_view,
            globals_buf,
            globals_bg,
            draw_bgl,
            entity_buf,
            entity_bg,
            entity_capacity,
            opaque_pipeline,
            sky_pipeline,
            mesh_gpu: HashMap::new(),
            dome_mesh,
            sun_mesh,
            frustum_visible: HashSet::new(),
            raw_scratch: Vec::new(),
            draw_scratch: Vec::new(),
            uniform_staging,
        }
    }

    // ── helpers ────────────────────────────────────────────────────────────

    fn make_depth_view(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
        device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("[depth] attachment"),
                size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor::default())
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
        draw_bgl: &wgpu::BindGroupLayout,
        entity_buf: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("[draw] entity bg"),
            layout: draw_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: entity_buf,
                    offset: 0,
                    size: wgpu::BufferSize::new(DRAW_UNIFORMS_SIZE),
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

    /// write model+color into the staging slice at the given slot.
    fn pack_uniforms(staging: &mut [u8], slot: usize, model: Mat4, color: Color) {
        let offset = slot * UNIFORM_STRIDE as usize;
        let model_cols = model.to_cols_array();
        let color_data = [color.r, color.g, color.b, color.a];
        staging[offset..offset + 64].copy_from_slice(unsafe { slice_as_bytes(&model_cols) });
        staging[offset + 64..offset + 80].copy_from_slice(unsafe { slice_as_bytes(&color_data) });
    }

    // ── public surface management ──────────────────────────────────────────

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.surface_config.width = width;
        self.surface_config.height = height;
        self.surface.configure(&self.device, &self.surface_config);
        self.depth_view = Self::make_depth_view(&self.device, width, height);
    }

    pub fn surface_width(&self) -> u32 { self.surface_config.width }
    pub fn surface_height(&self) -> u32 { self.surface_config.height }
    pub fn render_tier(&self) -> RenderTier { self.render_tier }

    // ── render ─────────────────────────────────────────────────────────────

    fn render_frame(&mut self, world: &mut World) -> u32 {
        // ── gather camera ──────────────────────────────────────────────────
        let active = world.resource::<ActiveCamera3d>();
        let Some(cam_entity) = active.entity else { return 0; };
        let Some(camera) = world.get::<Camera3d>(cam_entity) else { return 0; };
        let Some(cam_wt) = world.get::<WorldTransform3d>(cam_entity) else { return 0; };
        let aspect = world.resource::<ViewportAspect>().0;
        let view_proj = camera.view_proj(*cam_wt, aspect);
        let cam_pos = cam_wt.translation;

        // ── gather sky ────────────────────────────────────────────────────
        let sky = world.get_resource::<Sky>().copied();
        let sky_color = sky.map_or(Color::rgb(0.1, 0.1, 0.15), |s| s.sky_color);

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
                let color = registry
                    .get_material(lunar_assets::Handle::new(mat_id, 0))
                    .map(|m| m.base_color)
                    .unwrap_or(Color::WHITE);
                self.draw_scratch.push((entity, mesh_id, color, model));
            }
        }

        // ── upload missing meshes ──────────────────────────────────────────
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

        // ── grow entity buffer if needed ──────────────────────────────────
        let needed = ENTITY_SLOT_START + self.draw_scratch.len();
        if needed > self.entity_capacity {
            self.entity_capacity = needed.next_power_of_two().max(INITIAL_ENTITY_CAPACITY);
            self.entity_buf = Self::make_entity_buf(&self.device, self.entity_capacity);
            self.entity_bg = Self::make_entity_bg(&self.device, &self.draw_bgl, &self.entity_buf);
            self.uniform_staging.resize(self.entity_capacity * UNIFORM_STRIDE as usize, 0);
            log::debug!("entity uniform buffer grown to {} slots", self.entity_capacity);
        }

        // ── pack all uniforms into staging, upload in one call ────────────
        let dome_model = Mat4::from_translation(cam_pos);
        Self::pack_uniforms(&mut self.uniform_staging, SLOT_DOME, dome_model, sky_color);

        if let Some(sky) = sky {
            let sun_model = Mat4::from_translation(cam_pos + Vec3::new(0.0, SUN_Y, 0.0));
            Self::pack_uniforms(&mut self.uniform_staging, SLOT_SUN, sun_model, sky.sun_color);
        }

        for i in 0..self.draw_scratch.len() {
            let (_, _, color, model) = self.draw_scratch[i];
            Self::pack_uniforms(&mut self.uniform_staging, ENTITY_SLOT_START + i, model, color);
        }

        let upload_size = (needed * UNIFORM_STRIDE as usize) as u64;
        let vp_cols = view_proj.to_cols_array();
        self.queue.write_buffer(&self.globals_buf, 0, unsafe { slice_as_bytes(&vp_cols) });
        self.queue.write_buffer(&self.entity_buf, 0, &self.uniform_staging[..upload_size as usize]);

        // ── acquire surface ───────────────────────────────────────────────
        let (wgpu::CurrentSurfaceTexture::Success(frame)
        | wgpu::CurrentSurfaceTexture::Suboptimal(frame)) = self.surface.get_current_texture()
        else {
            return 0;
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("[frame] encoder"),
        });

        // ── render pass ───────────────────────────────────────────────────
        let mut draw_calls: u32 = 0;
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("[frame] pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: sky_color.r as f64,
                            g: sky_color.g as f64,
                            b: sky_color.b as f64,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            pass.set_bind_group(0, &self.globals_bg, &[]);

            // sky pass — dome always drawn; sun only when sky resource says so
            pass.set_pipeline(&self.sky_pipeline);
            pass.set_bind_group(1, &self.entity_bg, &[Self::slot_offset(SLOT_DOME)]);
            pass.set_vertex_buffer(0, self.dome_mesh.vbuf.slice(..));
            pass.set_index_buffer(self.dome_mesh.ibuf.slice(..), self.dome_mesh.index_fmt);
            pass.draw_indexed(0..self.dome_mesh.index_count, 0, 0..1);
            draw_calls += 1;

            if sky.is_some_and(|s| s.show_sun) {
                pass.set_bind_group(1, &self.entity_bg, &[Self::slot_offset(SLOT_SUN)]);
                pass.set_vertex_buffer(0, self.sun_mesh.vbuf.slice(..));
                pass.set_index_buffer(self.sun_mesh.ibuf.slice(..), self.sun_mesh.index_fmt);
                pass.draw_indexed(0..self.sun_mesh.index_count, 0, 0..1);
                draw_calls += 1;
            }

            // opaque pass
            pass.set_pipeline(&self.opaque_pipeline);
            for i in 0..self.draw_scratch.len() {
                let mesh_id = self.draw_scratch[i].1;
                let Some(gpu_mesh) = self.mesh_gpu.get(&mesh_id) else { continue; };
                pass.set_bind_group(1, &self.entity_bg, &[Self::slot_offset(ENTITY_SLOT_START + i)]);
                pass.set_vertex_buffer(0, gpu_mesh.vbuf.slice(..));
                pass.set_index_buffer(gpu_mesh.ibuf.slice(..), gpu_mesh.index_fmt);
                pass.draw_indexed(0..gpu_mesh.index_count, 0, 0..1);
                draw_calls += 1;
            }
        }

        self.queue.submit(Some(encoder.finish()));
        frame.present();
        draw_calls
    }

    #[inline(always)]
    fn slot_offset(slot: usize) -> u32 {
        (slot * UNIFORM_STRIDE as usize) as u32
    }
}

// ── ecs integration ────────────────────────────────────────────────────────

fn render_3d_system(world: &mut World) {
    let mut engine = world.remove_resource::<RenderEngine3d>().unwrap();
    let draw_calls = engine.render_frame(world);
    world.insert_resource(engine);
    if let Some(mut info) = world.get_resource_mut::<RenderInfo3d>() {
        info.draw_calls = draw_calls;
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

        // pull render tier out of the engine resource (already inserted by bootstrap_3d)
        // and expose it as a standalone resource for game systems to query
        if let Some(engine) = app.world_mut().get_resource::<RenderEngine3d>() {
            let tier = engine.render_tier();
            app.insert_resource(tier);
        }

        app.add_system_to_stage(UpdateStage::Render, render_3d_system);
        log::info!("RenderPlugin3d: 3d render system registered");
    }
}
