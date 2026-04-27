//! rendering subsystem via wgpu
//!
//! decoupled from game logic. handles 2D rendering with wgpu.
//! architecture allows future 3D expansion without breaking changes.
//!
//! # rendering model
//!
//! the render system uses a command-based approach:
//! 1. game logic pushes [`DrawCommand`]s into the [`RenderQueue`]
//! 2. the [`RenderEngine`] consumes all commands in a single batched draw call
//! 3. all geometry is packed into one vertex buffer for efficiency
//!
//! # example
//!
//! ```ignore
//! use engine_render::{RenderQueue, DrawCommand, DrawKind, Color};
//! use engine_math::Vec2;
//!
//! fn render_system(mut queue: ResMut<RenderQueue>) {
//!     queue.clear(); // clear last frame's commands
//!     queue.push(DrawCommand {
//!         kind: DrawKind::Rect {
//!             position: Vec2::new(100.0, 100.0),
//!             size: Vec2::new(50.0, 50.0),
//!             color: Color::RED,
//!         },
//!     });
//! }
//! ```

use std::collections::HashMap;

use bevy_ecs::prelude::*;
use engine_assets::{Handle, Texture};
use engine_core::{App, GamePlugin};
use engine_math::{Color, Vec2};
use wgpu::util::DeviceExt;

/// rendering configuration.
///
/// controls window size, vsync, and frame rate limiting.
/// used when initializing the [`RenderEngine`].
#[derive(Debug, Clone)]
pub struct RenderConfig {
    /// window width
    pub width: u32,
    /// window height
    pub height: u32,
    /// vsync enabled
    pub vsync: bool,
    /// target frame cap (0 = uncapped)
    pub frame_cap: u32,
}

impl Default for RenderConfig {
    fn default() -> Self {
        RenderConfig {
            width: 1280,
            height: 720,
            vsync: true,
            frame_cap: 0,
        }
    }
}

/// render engine resource, owns all wgpu rendering state.
///
/// manages the GPU device, queue, surface, and render pipelines.
/// the [`Resource`] derive is only applied on native targets — on WASM,
/// WebGPU types are `!Send`, so the engine is stored in a static instead.
#[cfg_attr(not(target_arch = "wasm32"), derive(Resource))]
pub struct RenderEngine {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    render_config: RenderConfig,
    sprite_pipeline: wgpu::RenderPipeline,
    uniform_buf: wgpu::Buffer,
    bind_group_layout: wgpu::BindGroupLayout,
    #[allow(dead_code)]
    bind_group: wgpu::BindGroup,
    sampler: wgpu::Sampler,
    textures: HashMap<u32, GpuTexture>,
}

/// gpu-ready texture: texture + view + sampler
#[allow(dead_code)]
struct GpuTexture {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

impl RenderEngine {
    /// create render engine from a surface (native, blocking)
    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_surface(
        instance: &wgpu::Instance,
        surface: wgpu::Surface<'static>,
        config: RenderConfig,
    ) -> Self {
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
        }))
        .expect("failed to request adapter");

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("lunar render device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::default(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
        }))
        .expect("failed to request device");

        Self::init_inner(adapter, device, queue, surface, config)
    }

    /// create render engine from a surface (WASM, async)
    #[cfg(target_arch = "wasm32")]
    pub async fn from_surface(
        instance: &wgpu::Instance,
        surface: wgpu::Surface<'static>,
        config: RenderConfig,
    ) -> Self {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
            })
            .await
            .expect("no WebGPU adapter found — in Firefox enable dom.webgpu.enabled in about:config, Chrome 113+ required");

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("lunar render device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::default(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
            })
            .await
            .expect("failed to request device");

        Self::init_inner(adapter, device, queue, surface, config)
    }

    fn init_inner(
        adapter: wgpu::Adapter,
        device: wgpu::Device,
        queue: wgpu::Queue,
        surface: wgpu::Surface<'static>,
        config: RenderConfig,
    ) -> Self {
        let caps = surface.get_capabilities(&adapter);
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

        // projection matrix uniform (4x4 f32 = 64 bytes)
        let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uniform buffer"),
            size: 64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sprite sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sprite bind group layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
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

        // placeholder bind group (no texture yet — created per-frame)
        let placeholder_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("placeholder"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let placeholder_view =
            placeholder_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sprite bind group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&placeholder_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sprite shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(SHADER_SOURCE)),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sprite pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        // vertex layout: [pos.x, pos.y, u, v, r, g, b, a] per vertex (stride 32 bytes)
        let sprite_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sprite pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 32,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 0,
                            shader_location: 0,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 8,
                            shader_location: 1,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x4,
                            offset: 16,
                            shader_location: 2,
                        },
                    ],
                }],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_config.format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::SrcAlpha,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            cache: None,
            multiview_mask: None,
        });

        let frame_cap_str = if config.frame_cap == 0 {
            "uncapped".to_string()
        } else {
            config.frame_cap.to_string()
        };
        log::info!(
            "render engine initialized: {}x{}, frame_cap={}",
            config.width,
            config.height,
            frame_cap_str
        );

        RenderEngine {
            surface,
            device,
            queue,
            config: surface_config,
            render_config: config,
            sprite_pipeline,
            uniform_buf,
            bind_group_layout,
            bind_group,
            sampler,
            textures: HashMap::new(),
        }
    }

    /// get the current render config
    pub fn config(&self) -> &RenderConfig {
        &self.render_config
    }

    /// get the wgpu device
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// get the wgpu queue
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// resize the render surface
    pub fn resize(&mut self, width: u32, height: u32) {
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.render_config.width = width;
        self.render_config.height = height;
    }

    /// upload a texture to the GPU, returns its handle id.
    /// if the texture is already uploaded, this is a no-op.
    pub fn upload_texture(&mut self, handle: &Handle<Texture>, texture: &Texture) {
        if self.textures.contains_key(&handle.id()) {
            return;
        }

        let gpu_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sprite texture"),
            size: wgpu::Extent3d {
                width: texture.width,
                height: texture.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &gpu_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &texture.pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * texture.width),
                rows_per_image: Some(texture.height),
            },
            wgpu::Extent3d {
                width: texture.width,
                height: texture.height,
                depth_or_array_layers: 1,
            },
        );

        let view = gpu_texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.textures.insert(
            handle.id(),
            GpuTexture {
                texture: gpu_texture,
                view,
            },
        );
    }

    /// render all draw commands for this frame.
    /// sprites are batched by texture — one draw call per unique texture.
    /// rects (no texture) are drawn in a single additional draw call.
    pub fn render(&mut self, commands: &[DrawCommand]) {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => frame,
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            _ => return,
        };

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        // orthographic projection: y-down, maps [0, width] x [0, height] to NDC
        let surface_width = self.config.width as f32;
        let surface_height = self.config.height as f32;
        let projection: [f32; 16] = [
            2.0 / surface_width,
            0.0,
            0.0,
            0.0,
            0.0,
            -2.0 / surface_height,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
            0.0,
            -1.0,
            1.0,
            0.0,
            1.0,
        ];
        self.queue
            .write_buffer(&self.uniform_buf, 0, bytemuck::cast_slice(&projection));

        // group sprite commands by texture id
        let mut sprites_by_tex: HashMap<u32, Vec<&DrawCommand>> = HashMap::new();
        let mut rect_commands: Vec<&DrawCommand> = Vec::new();

        for command in commands {
            match &command.kind {
                DrawKind::Sprite {
                    texture: Some(tex_id),
                    ..
                } => {
                    sprites_by_tex
                        .entry(*tex_id as u32)
                        .or_default()
                        .push(command);
                }
                DrawKind::Rect { .. } | DrawKind::Text { .. } => {
                    rect_commands.push(command);
                }
                _ => {}
            }
        }

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("render encoder"),
            });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("render pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.07,
                            g: 0.07,
                            b: 0.07,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            pass.set_pipeline(&self.sprite_pipeline);

            // draw sprites batched by texture
            for (tex_id, sprite_cmds) in &sprites_by_tex {
                let Some(gpu_tex) = self.textures.get(tex_id) else {
                    continue;
                };

                let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("frame bind group"),
                    layout: &self.bind_group_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: self.uniform_buf.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(&gpu_tex.view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::Sampler(&self.sampler),
                        },
                    ],
                });

                let mut vertices: Vec<f32> = Vec::with_capacity(sprite_cmds.len() * 48);
                for command in sprite_cmds {
                    if let DrawKind::Sprite {
                        position,
                        rotation,
                        scale,
                        tint,
                        ..
                    } = &command.kind
                    {
                        let hw = scale.x * 0.5;
                        let hh = scale.y * 0.5;
                        let cos = rotation.cos();
                        let sin = rotation.sin();

                        let corners = [
                            [-hw, -hh],
                            [hw, -hh],
                            [-hw, hh],
                            [-hw, hh],
                            [hw, -hh],
                            [hw, hh],
                        ];
                        let uvs = [
                            [0.0, 0.0],
                            [1.0, 0.0],
                            [0.0, 1.0],
                            [0.0, 1.0],
                            [1.0, 0.0],
                            [1.0, 1.0],
                        ];

                        for (i, [lx, ly]) in corners.iter().enumerate() {
                            let rx = lx * cos - ly * sin;
                            let ry = lx * sin + ly * cos;
                            let px = position.x + rx;
                            let py = position.y + ry;
                            let [u, v] = uvs[i];
                            vertices
                                .extend_from_slice(&[px, py, u, v, tint.r, tint.g, tint.b, tint.a]);
                        }
                    }
                }

                if vertices.is_empty() {
                    continue;
                }

                let vertex_buf =
                    self.device
                        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("sprite vertices"),
                            contents: bytemuck::cast_slice(&vertices),
                            usage: wgpu::BufferUsages::VERTEX,
                        });

                pass.set_bind_group(0, &bind_group, &[]);
                pass.set_vertex_buffer(0, vertex_buf.slice(..));
                pass.draw(0..(vertices.len() / 8) as u32, 0..1);
            }

            // draw untextured commands (rects, text) as solid color
            if !rect_commands.is_empty() {
                let mut vertices: Vec<f32> = Vec::with_capacity(rect_commands.len() * 36);
                for command in &rect_commands {
                    match &command.kind {
                        DrawKind::Rect {
                            position,
                            size,
                            color,
                        } => {
                            let (x, y, w, h) = (position.x, position.y, size.x, size.y);
                            for [px, py] in [
                                [x, y],
                                [x + w, y],
                                [x, y + h],
                                [x, y + h],
                                [x + w, y],
                                [x + w, y + h],
                            ] {
                                vertices.extend_from_slice(&[
                                    px, py, 0.0, 0.0, color.r, color.g, color.b, color.a,
                                ]);
                            }
                        }
                        DrawKind::Text {
                            position, color, ..
                        } => {
                            // stub: draw a small rect placeholder for text
                            let (x, y) = (position.x, position.y);
                            for [px, py] in [
                                [x, y],
                                [x + 50.0, y],
                                [x, y + 16.0],
                                [x, y + 16.0],
                                [x + 50.0, y],
                                [x + 50.0, y + 16.0],
                            ] {
                                vertices.extend_from_slice(&[
                                    px, py, 0.0, 0.0, color.r, color.g, color.b, color.a,
                                ]);
                            }
                        }
                        _ => {}
                    }
                }

                if !vertices.is_empty() {
                    let vertex_buf =
                        self.device
                            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                                label: Some("rect vertices"),
                                contents: bytemuck::cast_slice(&vertices),
                                usage: wgpu::BufferUsages::VERTEX,
                            });

                    pass.set_vertex_buffer(0, vertex_buf.slice(..));
                    pass.draw(0..(vertices.len() / 8) as u32, 0..1);
                }
            }
        }

        self.queue.submit(Some(encoder.finish()));
        frame.present();
    }
}

const SHADER_SOURCE: &str = r#"
struct Uniforms { projection: mat4x4<f32> }

struct VertexOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
}

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(0) @binding(1) var sprite_texture: texture_2d<f32>;
@group(0) @binding(2) var sprite_sampler: sampler;

@vertex
fn vs_main(
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
) -> VertexOut {
    var out: VertexOut;
    out.clip_position = uniforms.projection * vec4<f32>(pos, 0.0, 1.0);
    out.uv = uv;
    out.color = color;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let tex_color = textureSample(sprite_texture, sprite_sampler, in.uv);
    return tex_color * in.color;
}
"#;

/// render queue resource, collects draw commands each frame.
///
/// game logic pushes draw commands into the queue during the update phase.
/// the render engine consumes the queue during the render phase.
///
/// # lifecycle
///
/// call [`RenderQueue::clear()`] at the start of each frame to remove
/// last frame's commands before adding new ones.
#[derive(Resource)]
pub struct RenderQueue {
    commands: Vec<DrawCommand>,
    /// optional render target (texture handle id) for off-screen rendering
    target: Option<u32>,
}

/// a single draw command.
///
/// wraps a [`DrawKind`] which specifies what to draw.
#[derive(Debug, Clone)]
pub struct DrawCommand {
    /// draw type
    pub kind: DrawKind,
}

/// type of draw command.
///
/// each variant represents a different primitive that can be rendered.
/// sprite rendering is currently stubbed and will be implemented
/// when the asset pipeline is complete.
#[derive(Debug, Clone)]
pub enum DrawKind {
    /// draw a 2D sprite
    Sprite {
        texture: Option<u64>,
        position: Vec2,
        rotation: f32,
        scale: Vec2,
        tint: Color,
    },
    /// draw a 2D rectangle
    Rect {
        position: Vec2,
        size: Vec2,
        color: Color,
    },
    /// draw text
    Text {
        content: String,
        position: Vec2,
        font_size: f32,
        color: Color,
    },
}

impl RenderQueue {
    /// create a new empty render queue
    pub fn new() -> Self {
        RenderQueue {
            commands: Vec::new(),
            target: None,
        }
    }

    /// clear all pending draw commands
    pub fn clear(&mut self) {
        self.commands.clear();
        self.target = None;
    }

    /// set the render target for subsequent draw commands.
    /// pass None to render to the main surface.
    pub fn set_target(&mut self, target: Option<u32>) {
        self.target = target;
    }

    /// get the current render target
    pub fn target(&self) -> Option<u32> {
        self.target
    }

    /// add a draw command
    pub fn push(&mut self, command: DrawCommand) {
        self.commands.push(command);
    }

    /// get all pending draw commands
    pub fn commands(&self) -> &[DrawCommand] {
        &self.commands
    }

    /// draw a sprite at the given position and size using a texture handle
    pub fn draw_sprite(&mut self, texture: &Handle<Texture>, position: Vec2, size: Vec2) {
        self.push(DrawCommand {
            kind: DrawKind::Sprite {
                texture: Some(texture.id() as u64),
                position,
                rotation: 0.0,
                scale: size,
                tint: Color::WHITE,
            },
        });
    }

    /// draw a sprite with full transform control using a texture handle
    pub fn draw_sprite_transformed(
        &mut self,
        texture: &Handle<Texture>,
        position: Vec2,
        size: Vec2,
        rotation: f32,
        _origin: Vec2,
        tint: Color,
    ) {
        self.push(DrawCommand {
            kind: DrawKind::Sprite {
                texture: Some(texture.id() as u64),
                position,
                rotation,
                scale: size,
                tint,
            },
        });
    }

    /// draw a colored rectangle
    pub fn draw_rect(&mut self, position: Vec2, size: Vec2, color: Color) {
        self.push(DrawCommand {
            kind: DrawKind::Rect {
                position,
                size,
                color,
            },
        });
    }

    /// draw a line between two points
    pub fn draw_line(&mut self, start: Vec2, end: Vec2, color: Color, thickness: f32) {
        let delta = end - start;
        let length = delta.length();
        if length < 0.001 {
            return;
        }
        // draw a thin rect along the line
        let angle = delta.y.atan2(delta.x);
        let cos = angle.cos();
        let sin = angle.sin();
        let half_t = thickness * 0.5;

        // compute the axis-aligned bounding box of the rotated line rect
        let corners = [
            [-sin * half_t, cos * half_t],
            [cos * length - sin * half_t, sin * length + cos * half_t],
            [cos * length + sin * half_t, sin * length - cos * half_t],
            [sin * half_t, -cos * half_t],
        ];

        let min_x = corners.iter().map(|c| c[0]).fold(f32::INFINITY, f32::min);
        let max_x = corners
            .iter()
            .map(|c| c[0])
            .fold(f32::NEG_INFINITY, f32::max);
        let min_y = corners.iter().map(|c| c[1]).fold(f32::INFINITY, f32::min);
        let max_y = corners
            .iter()
            .map(|c| c[1])
            .fold(f32::NEG_INFINITY, f32::max);

        self.push(DrawCommand {
            kind: DrawKind::Rect {
                position: Vec2::new(start.x + min_x, start.y + min_y),
                size: Vec2::new(max_x - min_x, max_y - min_y),
                color,
            },
        });
    }

    /// clear the screen with the given color.
    /// this is a convenience that draws a full-screen rect — the render engine's
    /// default clear color is not affected.
    pub fn clear_color(&mut self, color: Color) {
        self.push(DrawCommand {
            kind: DrawKind::Rect {
                position: Vec2::ZERO,
                size: Vec2::new(10000.0, 10000.0),
                color,
            },
        });
    }

    /// draw text at the given position
    pub fn draw_text(&mut self, content: &str, position: Vec2, font_size: f32, color: Color) {
        self.push(DrawCommand {
            kind: DrawKind::Text {
                content: content.to_string(),
                position,
                font_size,
                color,
            },
        });
    }
}

impl Default for RenderQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// render plugin, registers render systems and resources.
///
/// add this plugin to your [`App`] to enable rendering.
/// it registers the [`RenderQueue`] as an ECS resource.
pub struct RenderPlugin;

impl Default for RenderPlugin {
    fn default() -> Self {
        Self
    }
}

impl GamePlugin for RenderPlugin {
    fn name(&self) -> &str {
        "RenderPlugin"
    }

    fn build(&mut self, app: &mut App) {
        app.insert_resource(RenderQueue::new());
        app.add_system_to_stage(engine_core::UpdateStage::Render, render_system);
    }
}

/// render system that processes the render queue.
/// clears the queue at the start of each frame, then renders all commands.
fn render_system(mut queue: ResMut<RenderQueue>) {
    // clear last frame's commands
    queue.clear();
    // note: actual rendering is handled by the RenderEngine in the game loop.
    // this system exists so game code can push draw commands during the render stage.
    // the RenderEngine will consume the queue when it is available as a resource.
}
