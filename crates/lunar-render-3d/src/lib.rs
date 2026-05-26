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

use std::collections::HashMap;

use bevy_ecs::prelude::*;
use lunar_3d::{
    ActiveCamera3d, Camera3d, ComputedVisibility, IndexBuffer, Material3d, Mesh3d,
    MeshData, MeshRegistry, Vertex3d, ViewportAspect, WorldTransform3d,
};
use lunar_3d::primitives::{quad_mesh, sphere_mesh};
use lunar_core::{App, GamePlugin, UpdateStage};
use lunar_math::{Color, Mat4, Vec3};

const SHADER_SRC: &str = include_str!("shader.wgsl");

/// skydome sphere radius — must be less than the camera far plane.
const SKY_RADIUS: f32 = 900.0;

/// y-elevation of the sun quad center (just below the dome top).
const SUN_Y: f32 = 895.0;

/// vertex stride for [`Vertex3d`] in bytes.
const VERTEX_STRIDE: u64 = std::mem::size_of::<Vertex3d>() as u64;

/// size of the draw uniforms buffer: model mat4 (64) + base_color vec4 (16).
const DRAW_UNIFORMS_SIZE: u64 = 80;

// ── gpu types ──────────────────────────────────────────────────────────────

struct GpuMesh {
    vbuf: wgpu::Buffer,
    ibuf: wgpu::Buffer,
    index_count: u32,
    index_fmt: wgpu::IndexFormat,
}

struct EntityDraw {
    buf: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

// ── byte helpers ───────────────────────────────────────────────────────────

/// reinterpret a `#[repr(C)]` slice as a byte slice.
unsafe fn slice_as_bytes<T>(slice: &[T]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(slice.as_ptr() as *const u8, std::mem::size_of_val(slice)) }
}

// ── render config ──────────────────────────────────────────────────────────

/// window and rendering settings for a 3d game.
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

    depth_view: wgpu::TextureView,

    globals_buf: wgpu::Buffer,
    globals_bg: wgpu::BindGroup,
    draw_bgl: wgpu::BindGroupLayout,

    opaque_pipeline: wgpu::RenderPipeline,
    sky_pipeline: wgpu::RenderPipeline,

    mesh_gpu: HashMap<u32, GpuMesh>,
    entity_draws: HashMap<Entity, EntityDraw>,

    dome_mesh: GpuMesh,
    sun_mesh: GpuMesh,
    dome_draw: EntityDraw,
    sun_draw: EntityDraw,
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
            label: Some("3d globals bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let draw_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("3d draw bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        // ── globals buffer ─────────────────────────────────────────────────

        let globals_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("3d globals"),
            size: 64, // mat4x4
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let globals_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("3d globals bg"),
            layout: &globals_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals_buf.as_entire_binding(),
            }],
        });

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

        // sky pipeline — depth write off, no culling so inside of sphere is visible
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

        let dome_data = sphere_mesh(SKY_RADIUS, 32, 16);
        let dome_mesh = Self::upload_mesh_data(&device, &queue, &dome_data);

        // sun: flat XZ quad at SUN_Y, centered on camera each frame
        let sun_data = quad_mesh(40.0, 40.0);
        let sun_mesh = Self::upload_mesh_data(&device, &queue, &sun_data);

        let dome_draw = Self::make_entity_draw(&device, &draw_bgl);
        let sun_draw = Self::make_entity_draw(&device, &draw_bgl);

        log::info!(
            "lunar-render-3d initialized: {}×{}, vsync={}",
            config.width,
            config.height,
            config.vsync,
        );

        Self {
            device,
            queue,
            surface,
            surface_config,
            depth_view,
            globals_buf,
            globals_bg,
            draw_bgl,
            opaque_pipeline,
            sky_pipeline,
            mesh_gpu: HashMap::new(),
            entity_draws: HashMap::new(),
            dome_mesh,
            sun_mesh,
            dome_draw,
            sun_draw,
        }
    }

    // ── helpers ────────────────────────────────────────────────────────────

    fn make_depth_view(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("3d depth"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        tex.create_view(&wgpu::TextureViewDescriptor::default())
    }

    fn make_entity_draw(device: &wgpu::Device, draw_bgl: &wgpu::BindGroupLayout) -> EntityDraw {
        let buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("3d draw uniforms"),
            size: DRAW_UNIFORMS_SIZE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("3d draw bg"),
            layout: draw_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buf.as_entire_binding(),
            }],
        });
        EntityDraw { buf, bind_group }
    }

    fn upload_mesh_data(device: &wgpu::Device, queue: &wgpu::Queue, data: &MeshData) -> GpuMesh {
        let vdata = unsafe { slice_as_bytes(&data.vertices) };
        let vbuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("3d vbuf"),
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
            label: Some("3d ibuf"),
            size: idata.len() as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&ibuf, 0, idata);

        GpuMesh { vbuf, ibuf, index_count, index_fmt }
    }

    fn write_draw_uniforms(queue: &wgpu::Queue, draw: &EntityDraw, model: Mat4, color: Color) {
        let model_cols = model.to_cols_array();
        let color_data = [color.r, color.g, color.b, color.a];
        let mut bytes = [0u8; 80];
        bytes[0..64].copy_from_slice(unsafe { slice_as_bytes(&model_cols) });
        bytes[64..80].copy_from_slice(unsafe { slice_as_bytes(&color_data) });
        queue.write_buffer(&draw.buf, 0, &bytes);
    }

    // ── public surface management ──────────────────────────────────────────

    /// resize the render surface and depth buffer.
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

    // ── render ─────────────────────────────────────────────────────────────

    fn render_frame(&mut self, world: &mut World) -> u32 {
        // ── gather camera ──────────────────────────────────────────────────
        let active = world.resource::<ActiveCamera3d>();
        let Some(cam_entity) = active.entity else {
            return 0;
        };
        let Some(camera) = world.get::<Camera3d>(cam_entity) else {
            return 0;
        };
        let Some(cam_wt) = world.get::<WorldTransform3d>(cam_entity) else {
            return 0;
        };
        let aspect = world.resource::<ViewportAspect>().0;
        let view_proj = camera.view_proj(*cam_wt, aspect);
        let cam_pos = cam_wt.translation;

        // ── gather sky ────────────────────────────────────────────────────
        let sky = world.get_resource::<Sky>().copied();
        let sky_color = sky.map_or(Color::rgb(0.1, 0.1, 0.15), |s| s.sky_color);

        // ── gather entity draws ───────────────────────────────────────────

        // pass 1: query components (mesh_id, mat_id, model) — no registry borrow yet
        let raw_list: Vec<(Entity, u32, u32, Mat4)> = {
            let mut q = world
                .query::<(Entity, &Mesh3d, &Material3d, &WorldTransform3d, &ComputedVisibility)>();
            q.iter(world)
                .filter(|(_, _, _, _, vis)| vis.0)
                .map(|(entity, mesh, mat, wt, _)| (entity, mesh.0.id(), mat.0.id(), wt.to_matrix()))
                .collect()
        };

        // pass 2: resolve colors from registry (world.query borrow is dropped)
        let draw_list: Vec<(Entity, u32, Color, Mat4)> = {
            let registry = world.resource::<MeshRegistry>();
            raw_list
                .into_iter()
                .map(|(entity, mesh_id, mat_id, model)| {
                    let color = registry
                        .get_material(lunar_assets::Handle::new(mat_id, 0))
                        .map(|m| m.base_color)
                        .unwrap_or(Color::WHITE);
                    (entity, mesh_id, color, model)
                })
                .collect()
        };

        // ── upload missing meshes ──────────────────────────────────────────
        for (_, mesh_id, _, _) in &draw_list {
            if !self.mesh_gpu.contains_key(mesh_id) {
                // re-borrow registry for this lookup
                let registry = world.resource::<MeshRegistry>();
                if let Some(data) = registry.get_mesh(lunar_assets::Handle::new(*mesh_id, 0)) {
                    let gpu = Self::upload_mesh_data(&self.device, &self.queue, data);
                    self.mesh_gpu.insert(*mesh_id, gpu);
                }
            }
        }

        // ── ensure per-entity draw buffers ────────────────────────────────
        for (entity, _, _, _) in &draw_list {
            if !self.entity_draws.contains_key(entity) {
                let draw = Self::make_entity_draw(&self.device, &self.draw_bgl);
                self.entity_draws.insert(*entity, draw);
            }
        }

        // ── write uniforms before the render pass begins ──────────────────

        // globals: view_proj
        let vp_cols = view_proj.to_cols_array();
        self.queue.write_buffer(&self.globals_buf, 0, unsafe { slice_as_bytes(&vp_cols) });

        // sky dome: model = translate(camera_pos) so dome follows camera
        let dome_model = Mat4::from_translation(cam_pos);
        Self::write_draw_uniforms(&self.queue, &self.dome_draw, dome_model, sky_color);

        // sun: model = translate(cam_pos + (0, SUN_Y, 0))
        if let Some(sky) = sky {
            let sun_model = Mat4::from_translation(cam_pos + Vec3::new(0.0, SUN_Y, 0.0));
            Self::write_draw_uniforms(&self.queue, &self.sun_draw, sun_model, sky.sun_color);
        }

        // entities
        for (entity, _, color, model) in &draw_list {
            if let Some(draw) = self.entity_draws.get(entity) {
                Self::write_draw_uniforms(&self.queue, draw, *model, *color);
            }
        }

        // ── acquire surface ───────────────────────────────────────────────
        let (wgpu::CurrentSurfaceTexture::Success(frame)
        | wgpu::CurrentSurfaceTexture::Suboptimal(frame)) = self.surface.get_current_texture()
        else {
            return 0;
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("3d frame"),
        });

        // ── render pass ───────────────────────────────────────────────────
        let mut draw_calls: u32 = 0;
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("3d pass"),
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

            // sky pass
            pass.set_pipeline(&self.sky_pipeline);
            pass.set_bind_group(1, &self.dome_draw.bind_group, &[]);
            pass.set_vertex_buffer(0, self.dome_mesh.vbuf.slice(..));
            pass.set_index_buffer(self.dome_mesh.ibuf.slice(..), self.dome_mesh.index_fmt);
            pass.draw_indexed(0..self.dome_mesh.index_count, 0, 0..1);
            draw_calls += 1;

            if sky.is_some_and(|s| s.show_sun) {
                pass.set_bind_group(1, &self.sun_draw.bind_group, &[]);
                pass.set_vertex_buffer(0, self.sun_mesh.vbuf.slice(..));
                pass.set_index_buffer(self.sun_mesh.ibuf.slice(..), self.sun_mesh.index_fmt);
                pass.draw_indexed(0..self.sun_mesh.index_count, 0, 0..1);
                draw_calls += 1;
            }

            // opaque pass
            pass.set_pipeline(&self.opaque_pipeline);
            for (entity, mesh_id, _, _) in &draw_list {
                let (Some(gpu_mesh), Some(entity_draw)) = (
                    self.mesh_gpu.get(mesh_id),
                    self.entity_draws.get(entity),
                ) else {
                    continue;
                };
                pass.set_bind_group(1, &entity_draw.bind_group, &[]);
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
/// [`RenderEngine3d`] and [`RenderInfo3d`] as resources.
///
/// [`RenderEngine3d`] itself must be inserted before `build` is called — do this
/// in `bootstrap_3d` after creating the wgpu surface.
pub struct RenderPlugin3d;

impl GamePlugin for RenderPlugin3d {
    fn name(&self) -> &'static str {
        "render-3d"
    }

    fn build(&mut self, app: &mut App) {
        app.insert_resource(RenderInfo3d::default());
        app.add_system_to_stage(UpdateStage::Render, render_3d_system);
        log::info!("RenderPlugin3d: 3d render system registered");
    }
}
