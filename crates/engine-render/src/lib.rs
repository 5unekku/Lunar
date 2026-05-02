//! rendering subsystem via wgpu
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
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

pub mod atlas;
pub mod mesh;
pub mod render_pass_3d;
mod text;
pub mod textbox;

use std::collections::HashMap;

use bevy_ecs::prelude::*;
use engine_assets::{Font, Handle, Texture};
use engine_core::{App, GamePlugin};
use engine_math::{Color, Vec2};

/// internal parameters for writing sprite vertices.
#[allow(dead_code)]
#[derive(Clone, Copy)]
struct SpriteDrawParams {
    position: Vec2,
    rotation: f32,
    scale: Vec2,
    tint: Color,
    uv_rect: Option<(Vec2, Vec2)>,
    origin: Vec2,
}

/// parameters for drawing a transformed sprite.
/// used with [`RenderQueue::draw_sprite_transformed_on_layer`] to avoid
/// too many function arguments.
#[derive(Debug, Clone, Copy)]
pub struct SpriteParams {
    /// position in world space
    pub position: Vec2,
    /// size (width, height)
    pub scale: Vec2,
    /// rotation in radians
    pub rotation: f32,
    /// origin point for rotation and scaling
    pub origin: Vec2,
    /// color tint (RGBA)
    pub tint: Color,
}

/// camera resource, affects how the render queue is projected.
///
/// when no camera resource exists, rendering uses world-space anchored at origin.
/// when present, the orthographic projection is offset and scaled accordingly.
///
/// # example
///
/// ```ignore
/// use engine_render::Camera;
///
/// // camera centered at (400, 300), 800x600 viewport
/// let cam = Camera {
///     position: Vec2::new(400.0, 300.0),
///     zoom: 1.0,
///     rotation: 0.0,
///     viewport: Some(Vec4::new(0.0, 0.0, 800.0, 600.0)),
/// };
///
/// // use cam.projection_matrix() for the render projection
/// ```
#[derive(Resource, Clone)]
pub struct Camera {
    /// camera position in world space
    pub position: Vec2,
    /// zoom level (1.0 = 1:1, 2.0 = 2x zoom)
    pub zoom: f32,
    /// rotation in radians
    pub rotation: f32,
    /// viewport size in pixels (None = full window)
    pub viewport: Option<(u32, u32)>,
    /// per-layer offset for parallax scrolling (layer id → world offset)
    pub layer_parallax: HashMap<i32, Vec2>,
}

impl Camera {
    /// create a new camera at the origin with default settings
    #[must_use]
    pub fn new() -> Self {
        Self {
            position: Vec2::ZERO,
            zoom: 1.0,
            rotation: 0.0,
            viewport: None,
            layer_parallax: HashMap::default(),
        }
    }

    /// create a camera at the given position
    #[must_use]
    pub fn at_position(x: f32, y: f32) -> Self {
        Self {
            position: Vec2::new(x, y),
            zoom: 1.0,
            rotation: 0.0,
            viewport: None,
            layer_parallax: HashMap::default(),
        }
    }

    /// compute the orthographic projection matrix incorporating camera transforms.
    /// returns a 4x4 column-major matrix as a flat array of 16 f32s.
    /// for per-layer parallax, use [`Camera::projection_matrix_for_layer`] instead.
    ///
    /// # example
    ///
    /// ```ignore
    /// use engine_render::Camera;
    ///
    /// let cam = Camera::default();
    /// let proj = cam.projection_matrix(800, 600);
    /// // proj is a [f32; 16] suitable for wgpu uniform upload
    /// ```
    #[must_use]
    pub fn projection_matrix(&self, window_width: u32, window_height: u32) -> [f32; 16] {
        self.projection_matrix_for_layer(0, window_width, window_height)
    }

    /// compute the orthographic projection matrix with a per-layer parallax offset.
    /// the layer's parallax offset is subtracted from the camera position before
    /// computing the transform, so layers can scroll at different speeds.
    /// set per-layer offsets via [`Camera::set_layer_parallax`].
    #[must_use]
    pub fn projection_matrix_for_layer(
        &self,
        layer: i32,
        window_width: u32,
        window_height: u32,
    ) -> [f32; 16] {
        let parallax_offset = self
            .layer_parallax
            .get(&layer)
            .copied()
            .unwrap_or(Vec2::ZERO);
        let effective_pos = self.position - parallax_offset;
        self.projection_matrix_at(effective_pos, window_width, window_height)
    }

    /// internal: compute projection at a specific camera position.
    fn projection_matrix_at(&self, pos: Vec2, window_width: u32, window_height: u32) -> [f32; 16] {
        #[allow(clippy::cast_precision_loss)]
        let w = window_width as f32;
        #[allow(clippy::cast_precision_loss)]
        let h = window_height as f32;
        let zoom = self.zoom.max(0.001);
        let cos = self.rotation.cos();
        let sin = self.rotation.sin();

        // base orthographic scale
        let sx = 2.0 / w * zoom;
        let sy = -2.0 / h * zoom;

        // camera translation (accounting for rotation)
        let tx = -pos.y.mul_add(-sin, pos.x * cos);
        let ty = -pos.y.mul_add(cos, pos.x * sin);

        // combined matrix: scale then translate, y-down
        [
            sx,
            0.0,
            0.0,
            0.0,
            0.0,
            sy,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
            0.0,
            sx * tx - 1.0,
            sy * ty + 1.0,
            0.0,
            1.0,
        ]
    }

    /// set a parallax offset for a specific layer.
    /// the offset is in world space and is subtracted from the camera position
    /// when rendering that layer, creating a parallax effect.
    /// a factor of 0.0 means no offset (layer moves with camera),
    /// 1.0 means the layer stays fixed in world space,
    /// values between 0 and 1 create slower-scrolling backgrounds.
    pub fn set_layer_parallax(&mut self, layer: i32, offset: Vec2) {
        self.layer_parallax.insert(layer, offset);
    }

    /// remove the parallax offset for a layer, reverting to normal camera tracking.
    pub fn clear_layer_parallax(&mut self, layer: i32) {
        self.layer_parallax.remove(&layer);
    }
}

impl Default for Camera {
    fn default() -> Self {
        Self::new()
    }
}

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
        Self {
            width: 1280,
            height: 720,
            vsync: true,
            frame_cap: 0,
        }
    }
}

/// max vertices per frame before buffer overflow (64k vertices = ~16k sprites with packed color)
/// vertex format: [pos.x, pos.y, u, v] (16 bytes) + [`color_u32`] (4 bytes) = 20 bytes per vertex
const MAX_VERTICES: usize = 65536;

/// number of vertex buffers for double-buffering (prevents GPU read/write conflicts)
const VERTEX_BUFFER_COUNT: usize = 2;

/// bytes per vertex: 2 floats for position + 2 floats for uv + 1 u32 for packed rgba color
const VERTEX_STRIDE: usize = 20;

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
    bind_groups: HashMap<u32, wgpu::BindGroup>,
    /// persistent vertex buffers — double-buffered to prevent GPU read/write conflicts
    vertex_bufs: [wgpu::Buffer; VERTEX_BUFFER_COUNT],
    /// current frame index for buffer selection
    frame_index: usize,
    /// current write offset into the active vertex buffer
    vertex_offset: usize,
    glyph_atlas: text::GlyphAtlas,
    #[allow(dead_code)]
    glyph_atlas_texture: Option<GpuTexture>,
    /// cache of text layout results keyed by (`font_id`, text, `font_size_bits`)
    text_layout_cache: HashMap<(u32, String, u32), Vec<text::TextGlyphQuad>>,
    render_passes: Vec<Box<dyn RenderPass>>,
    /// vulkan pipeline cache for faster startup on subsequent launches
    #[cfg(not(target_arch = "wasm32"))]
    pipeline_cache: Option<wgpu::PipelineCache>,
}

/// gpu-ready texture: texture + view + sampler
#[allow(dead_code)]
struct GpuTexture {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

impl RenderEngine {
    /// create render engine from a surface (native, blocking)
    ///
    /// # Panics
    ///
    /// panics if no adapter or device can be created.
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

        Self::init_inner(&adapter, &device, queue, surface, config)
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

        Self::init_inner(&adapter, &device, queue, surface, config)
    }

    /// create a WebGPU surface from a canvas element (WASM only).
    ///
    /// # Errors
    ///
    /// returns an error if the canvas element is not compatible with the GPU
    /// or if the browser denies GPU access.
    #[cfg(target_arch = "wasm32")]
    pub fn create_canvas_surface(
        instance: &wgpu::Instance,
        canvas: &web_sys::HtmlCanvasElement,
    ) -> Result<wgpu::Surface<'static>, String> {
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
            .map_err(|e| format!("failed to create surface: {e:?}"))?;
        Ok(surface)
    }

    /// find a canvas element by id and return it.
    ///
    /// # Errors
    ///
    /// returns an error if no window, no document, no element with the given id,
    /// or if the element is not an html canvas element.
    #[cfg(target_arch = "wasm32")]
    pub fn find_canvas(id: &str) -> Result<web_sys::HtmlCanvasElement, String> {
        use wasm_bindgen::JsCast;
        let window = web_sys::window().ok_or("no window")?;
        let document = window.document().ok_or("no document")?;
        let element = document
            .get_element_by_id(id)
            .ok_or_else(|| format!("no element with id '{id}'"))?;
        element
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .map_err(|_| format!("element '{id}' is not a canvas"))
    }

    #[allow(clippy::too_many_lines)]
    fn init_inner(
        adapter: &wgpu::Adapter,
        device: &wgpu::Device,
        queue: wgpu::Queue,
        surface: wgpu::Surface<'static>,
        config: RenderConfig,
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

        surface.configure(device, &surface_config);

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

        // vertex layout: [pos.x, pos.y, u, v] (16 bytes) + [packed rgba u32] (4 bytes) = 20 bytes
        let sprite_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sprite pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: VERTEX_STRIDE as u64,
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
                            format: wgpu::VertexFormat::Unorm8x4,
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
            cache: None, // populated below after cache is loaded
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

        // persistent vertex buffers — double-buffered to prevent GPU read/write conflicts
        // uses COPY_DST for queue.write_buffer (no MAP_WRITE needed)
        let vertex_bufs: [wgpu::Buffer; VERTEX_BUFFER_COUNT] = std::array::from_fn(|i| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(&format!("persistent vertex buffer {i}")),
                size: (MAX_VERTICES * VERTEX_STRIDE) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });

        Self {
            surface,
            device: device.clone(),
            queue,
            config: surface_config,
            render_config: config,
            sprite_pipeline,
            uniform_buf,
            bind_group_layout,
            bind_group,
            sampler,
            textures: HashMap::new(),
            bind_groups: HashMap::new(),
            vertex_bufs,
            frame_index: 0,
            vertex_offset: 0,
            glyph_atlas: text::GlyphAtlas::new(1024, 1024),
            glyph_atlas_texture: None,
            text_layout_cache: HashMap::new(),
            render_passes: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            pipeline_cache: Self::load_pipeline_cache(device),
        }
    }

    /// update the uniform buffer with the projection matrix for a specific layer.
    /// applies per-layer parallax offset from the camera if present.
    fn update_projection_for_layer(&mut self, layer: i32, camera: Option<&Camera>) {
        let projection = if let Some(cam) = camera {
            cam.projection_matrix_for_layer(layer, self.config.width, self.config.height)
        } else {
            let w = self.config.width as f32;
            let h = self.config.height as f32;
            [
                2.0 / w,
                0.0,
                0.0,
                0.0,
                0.0,
                -2.0 / h,
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
            ]
        };
        self.queue
            .write_buffer(&self.uniform_buf, 0, bytemuck::cast_slice(&projection));
    }

    /// load the vulkan pipeline cache from disk if it exists.
    #[cfg(not(target_arch = "wasm32"))]
    fn load_pipeline_cache(device: &wgpu::Device) -> Option<wgpu::PipelineCache> {
        let cache_path = std::path::Path::new(".pipeline_cache.bin");
        if cache_path.exists() {
            match std::fs::read(cache_path) {
                Ok(data) => {
                    log::info!("loaded pipeline cache ({} bytes)", data.len());
                    // safety: wgpu validates the cache data internally,
                    // fallback=true ensures a fresh cache is created if data is invalid
                    Some(unsafe {
                        device.create_pipeline_cache(&wgpu::PipelineCacheDescriptor {
                            label: Some("loaded pipeline cache"),
                            data: Some(&data),
                            fallback: true,
                        })
                    })
                }
                Err(e) => {
                    log::warn!("failed to load pipeline cache: {e}");
                    None
                }
            }
        } else {
            None
        }
    }

    /// save the vulkan pipeline cache to disk.
    /// call this before shutting down to speed up future launches.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn save_pipeline_cache(&self) {
        if let Some(ref cache) = self.pipeline_cache
            && let Some(data) = cache.get_data()
        {
            let cache_path = std::path::Path::new(".pipeline_cache.bin");
            if let Err(e) = std::fs::write(cache_path, &data) {
                log::warn!("failed to save pipeline cache: {e}");
            } else {
                log::info!("saved pipeline cache ({} bytes)", data.len());
            }
        }
    }

    /// register a custom render pass.
    /// passes are executed in registration order after the default 2D pass.
    pub fn add_render_pass<P: RenderPass>(&mut self, pass: P) {
        self.render_passes.push(Box::new(pass));
    }

    /// get the current render config
    pub const fn config(&self) -> &RenderConfig {
        &self.render_config
    }

    /// get the wgpu device
    pub const fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// get the wgpu queue
    pub const fn queue(&self) -> &wgpu::Queue {
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

    /// remove a texture and its cached bind group.
    /// call this when a texture is no longer needed to free GPU memory.
    pub fn remove_texture(&mut self, tex_id: u32) {
        self.textures.remove(&tex_id);
        self.bind_groups.remove(&tex_id);
    }

    /// invalidate all cached text layouts.
    /// call this when font data changes or text content changes dynamically.
    pub fn invalidate_text_cache(&mut self) {
        self.text_layout_cache.clear();
    }

    /// invalidate cached text layouts matching a specific font id.
    pub fn invalidate_text_cache_for_font(&mut self, font_id: u32) {
        self.text_layout_cache.retain(|key, _| key.0 != font_id);
    }

    /// get cached text layout for (`font_id`, text, `font_size`).
    /// computes and caches the result on first use.
    fn get_cached_text_layout(
        &mut self,
        font_id: u32,
        text: &str,
        font_size: f32,
    ) -> &[text::TextGlyphQuad] {
        // use f32::to_bits() for hashable key since f32 doesn't implement Hash/Eq
        let key = (font_id, text.to_string(), font_size.to_bits());
        self.text_layout_cache.entry(key).or_insert_with(|| {
            text::layout_text(&self.glyph_atlas, font_id, text, font_size, Vec2::ZERO)
        })
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
        let tex_id = handle.id();

        // create and cache bind group for this texture
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sprite bind group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniform_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        self.bind_groups.insert(tex_id, bind_group);

        self.textures.insert(
            tex_id,
            GpuTexture {
                texture: gpu_texture,
                view,
            },
        );
    }

    /// upload the glyph atlas to the GPU as a texture.
    #[allow(dead_code)]
    fn upload_glyph_atlas(&mut self) {
        let atlas = &self.glyph_atlas;
        let gpu_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("glyph atlas texture"),
            size: wgpu::Extent3d {
                width: atlas.width,
                height: atlas.height,
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
            atlas.pixels(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * atlas.width),
                rows_per_image: Some(atlas.height),
            },
            wgpu::Extent3d {
                width: atlas.width,
                height: atlas.height,
                depth_or_array_layers: 1,
            },
        );

        let view = gpu_texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.glyph_atlas_texture = Some(GpuTexture {
            texture: gpu_texture,
            view,
        });
    }

    /// render all draw commands for this frame.
    /// sprites are batched by texture — one draw call per unique texture.
    /// rects (no texture) are drawn in a single additional draw call.
    #[allow(clippy::too_many_lines)]
    pub fn render(&mut self, commands: &[DrawCommand], camera: Option<&Camera>) {
        let (wgpu::CurrentSurfaceTexture::Success(frame)
        | wgpu::CurrentSurfaceTexture::Suboptimal(frame)) = self.surface.get_current_texture()
        else {
            return;
        };

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        // track current layer for parallax — updated as we iterate sorted commands
        let mut current_layer: Option<i32> = None;

        // sort by (layer, texture_id) — same-texture commands are contiguous, no HashMap needed
        let mut sorted_commands: Vec<&DrawCommand> = commands.iter().collect();
        sorted_commands.sort_by_key(|cmd| {
            let layer = match &cmd.kind {
                DrawKind::Sprite { layer, .. }
                | DrawKind::Rect { layer, .. }
                | DrawKind::Line { layer, .. }
                | DrawKind::Text { layer, .. } => *layer,
            };
            let tex = match &cmd.kind {
                DrawKind::Sprite {
                    texture: Some(id), ..
                } => u32::try_from(*id).unwrap_or(u32::MAX),
                _ => u32::MAX,
            };
            (layer, tex)
        });

        // collect untextured commands (rects, lines, text)
        let mut rect_commands: Vec<&DrawCommand> = Vec::new();
        for command in &sorted_commands {
            if matches!(
                &command.kind,
                DrawKind::Rect { .. } | DrawKind::Line { .. } | DrawKind::Text { .. }
            ) {
                rect_commands.push(command);
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
                        store: wgpu::StoreOp::Discard,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            pass.set_pipeline(&self.sprite_pipeline);

            // advance frame index for double-buffering
            self.frame_index = (self.frame_index + 1) % VERTEX_BUFFER_COUNT;
            // reset persistent vertex buffer offset for this frame
            self.vertex_offset = 0;

            // draw sprites batched by texture (sorted order means same-tex commands are contiguous)
            let mut current_tex: Option<u32> = None;
            let mut batch_start = 0; // vertex index where current batch started

            for command in &sorted_commands {
                let DrawKind::Sprite {
                    texture: Some(tex_id),
                    position,
                    rotation,
                    scale,
                    tint,
                    uv_rect,
                    origin,
                    layer,
                } = &command.kind
                else {
                    continue;
                };

                // update projection if layer changed (parallax)
                if current_layer != Some(*layer) {
                    // flush current batch before projection change
                    if self.vertex_offset > batch_start
                        && let Some(prev_tex) = current_tex
                    {
                        let vertex_count = (self.vertex_offset - batch_start) / VERTEX_STRIDE;
                        self.draw_vertex_batch(&mut pass, prev_tex, batch_start, vertex_count);
                    }
                    batch_start = self.vertex_offset;
                    self.update_projection_for_layer(*layer, camera);
                    current_layer = Some(*layer);
                }

                let tex_id = u32::try_from(*tex_id).unwrap_or(u32::MAX);

                // flush and switch texture when it changes
                if current_tex != Some(tex_id) {
                    if self.vertex_offset > batch_start
                        && let Some(prev_tex) = current_tex
                    {
                        let vertex_count = (self.vertex_offset - batch_start) / VERTEX_STRIDE;
                        self.draw_vertex_batch(&mut pass, prev_tex, batch_start, vertex_count);
                    }
                    batch_start = self.vertex_offset;
                    current_tex = Some(tex_id);
                }

                // drop sprite if buffer is full — 64K vertices = ~10K sprites, adequate for 2D
                if self.vertex_offset + 6 * VERTEX_STRIDE > MAX_VERTICES * VERTEX_STRIDE {
                    log::warn!(
                        "vertex buffer full (limit {} sprites): dropping sprite",
                        MAX_VERTICES / 6
                    );
                    continue;
                }

                // write vertices directly into persistent buffer
                self.write_sprite_vertices(&SpriteDrawParams {
                    position: *position,
                    rotation: *rotation,
                    scale: *scale,
                    tint: *tint,
                    uv_rect: *uv_rect,
                    origin: *origin,
                });
            }

            // flush final sprite batch
            if self.vertex_offset > batch_start
                && let Some(tex_id) = current_tex
            {
                let vertex_count = (self.vertex_offset - batch_start) / VERTEX_STRIDE;
                self.draw_vertex_batch(&mut pass, tex_id, batch_start, vertex_count);
            }

            // draw untextured commands (rects, lines, text) as solid color
            // these are already sorted by layer from sorted_commands
            if !rect_commands.is_empty() {
                let mut rect_batch_start = self.vertex_offset;
                for command in &rect_commands {
                    let layer = match &command.kind {
                        DrawKind::Rect { layer, .. }
                        | DrawKind::Line { layer, .. }
                        | DrawKind::Text { layer, .. } => *layer,
                        DrawKind::Sprite { .. } => continue,
                    };

                    // update projection if layer changed (parallax)
                    if current_layer != Some(layer) {
                        // flush current batch before projection change
                        if self.vertex_offset > rect_batch_start {
                            let vertex_count =
                                (self.vertex_offset - rect_batch_start) / VERTEX_STRIDE;
                            self.draw_vertex_batch(&mut pass, 0, rect_batch_start, vertex_count);
                        }
                        rect_batch_start = self.vertex_offset;
                        self.update_projection_for_layer(layer, camera);
                        current_layer = Some(layer);
                    }

                    // drop this command if the buffer is full
                    if self.vertex_offset + 6 * VERTEX_STRIDE > MAX_VERTICES * VERTEX_STRIDE {
                        log::warn!("vertex buffer full: dropping draw command");
                        continue;
                    }

                    match &command.kind {
                        DrawKind::Rect {
                            position,
                            size,
                            color,
                            ..
                        } => {
                            self.write_rect_vertices(*position, *size, *color);
                        }
                        DrawKind::Line {
                            start,
                            end,
                            color,
                            thickness,
                            ..
                        } => {
                            self.write_line_vertices(*start, *end, *color, *thickness);
                        }
                        DrawKind::Text {
                            font,
                            content,
                            position,
                            font_size,
                            color,
                            ..
                        } => {
                            let font_id = u32::try_from(font.unwrap_or(0)).unwrap_or(u32::MAX);
                            let quads = self
                                .get_cached_text_layout(font_id, content, *font_size)
                                .to_vec();
                            for quad in quads {
                                let offset_quad = text::TextGlyphQuad {
                                    position: quad.position + *position,
                                    ..quad
                                };
                                self.write_text_quad(&offset_quad, *color);
                            }
                        }
                        DrawKind::Sprite { .. } => {}
                    }
                }

                if self.vertex_offset > rect_batch_start {
                    let vertex_count = (self.vertex_offset - rect_batch_start) / VERTEX_STRIDE;
                    self.draw_vertex_batch(&mut pass, 0, rect_batch_start, vertex_count);
                }
            }
        }

        // execute custom render passes
        for pass in &self.render_passes {
            let mut custom_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(&format!("custom pass: {}", pass.name())),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
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
            pass.execute(&self.device, &self.queue, &mut custom_pass);
        }

        self.queue.submit(Some(encoder.finish()));
        frame.present();
    }

    /// draw a batch of vertices from the persistent vertex buffer.
    fn draw_vertex_batch(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        tex_id: u32,
        offset: usize,
        vertex_count: usize,
    ) {
        let Some(bind_group) = self.bind_groups.get(&tex_id) else {
            return;
        };
        pass.set_bind_group(0, bind_group, &[]);
        let buf = &self.vertex_bufs[self.frame_index];
        pass.set_vertex_buffer(
            0,
            buf.slice(offset as u64..(offset + vertex_count * VERTEX_STRIDE) as u64),
        );
        pass.draw(0..u32::try_from(vertex_count).unwrap_or(0), 0..1);
    }

    /// write a sprite's 6 vertices into the persistent vertex buffer.
    /// vertex format: [pos.x, pos.y, u, v] (f32) + [packed rgba] (u32) = 20 bytes
    /// origin is the pivot point for rotation/scaling, relative to the sprite's top-left.
    fn write_sprite_vertices(&mut self, params: &SpriteDrawParams) {
        let &SpriteDrawParams {
            position,
            rotation,
            scale,
            tint,
            uv_rect,
            origin,
        } = params;
        let cos = rotation.cos();
        let sin = rotation.sin();

        // corners relative to origin (not center)
        let corners = [
            [-origin.x, -origin.y],
            [scale.x - origin.x, -origin.y],
            [-origin.x, scale.y - origin.y],
            [-origin.x, scale.y - origin.y],
            [scale.x - origin.x, -origin.y],
            [scale.x - origin.x, scale.y - origin.y],
        ];

        let (uv_min, uv_max) = uv_rect.unwrap_or((Vec2::ZERO, Vec2::new(1.0, 1.0)));
        let uvs = [
            [uv_min.x, uv_min.y],
            [uv_max.x, uv_min.y],
            [uv_min.x, uv_max.y],
            [uv_min.x, uv_max.y],
            [uv_max.x, uv_min.y],
            [uv_max.x, uv_max.y],
        ];

        let packed_color = pack_color(tint);
        // 6 vertices * 5 components (4 f32 + 1 u32) = 30 elements
        let mut verts: [u32; 30] = [0; 30];
        for (i, [lx, ly]) in corners.iter().enumerate() {
            let rx = lx * cos - ly * sin;
            let ry = lx * sin + ly * cos;
            let px = position.x + rx;
            let py = position.y + ry;
            let [u, v] = uvs[i];
            let base = i * 5;
            verts[base] = f32_to_u32(px);
            verts[base + 1] = f32_to_u32(py);
            verts[base + 2] = f32_to_u32(u);
            verts[base + 3] = f32_to_u32(v);
            verts[base + 4] = packed_color;
        }

        let bytes = bytemuck::cast_slice(&verts);
        let buf = &self.vertex_bufs[self.frame_index];
        self.queue
            .write_buffer(buf, self.vertex_offset as u64, bytes);
        self.vertex_offset += 6 * VERTEX_STRIDE;
    }

    /// write a rect's 6 vertices into the persistent vertex buffer.
    /// vertex format: [pos.x, pos.y, u, v] (f32) + [packed rgba] (u32) = 20 bytes
    fn write_rect_vertices(&mut self, position: Vec2, size: Vec2, color: Color) {
        let (x, y, w, h) = (position.x, position.y, size.x, size.y);
        let packed_color = pack_color(color);
        // 6 vertices * 5 components = 30 u32s
        let mut verts: [u32; 30] = [0; 30];
        let positions = [
            (x, y),
            (x + w, y),
            (x, y + h),
            (x, y + h),
            (x + w, y),
            (x + w, y + h),
        ];
        for (i, (px, py)) in positions.iter().enumerate() {
            let base = i * 5;
            verts[base] = f32_to_u32(*px);
            verts[base + 1] = f32_to_u32(*py);
            verts[base + 2] = 0; // u
            verts[base + 3] = 0; // v
            verts[base + 4] = packed_color;
        }
        let bytes = bytemuck::cast_slice(&verts);
        let buf = &self.vertex_bufs[self.frame_index];
        self.queue
            .write_buffer(buf, self.vertex_offset as u64, bytes);
        self.vertex_offset += 6 * VERTEX_STRIDE;
    }

    /// write a line's 6 vertices into the persistent vertex buffer.
    /// renders a rotated rectangle along the line segment.
    fn write_line_vertices(&mut self, start: Vec2, end: Vec2, color: Color, thickness: f32) {
        let dx = end.x - start.x;
        let dy = end.y - start.y;
        let len = dx.hypot(dy);
        if len < 0.001 {
            return;
        }
        // unit direction and perpendicular
        let nx = -dy / len;
        let ny = dx / len;
        let half_t = thickness * 0.5;
        // 4 corners of the line rectangle
        let corners = [
            (nx.mul_add(half_t, start.x), ny.mul_add(half_t, start.y)),
            (nx.mul_add(-half_t, start.x), ny.mul_add(-half_t, start.y)),
            (nx.mul_add(half_t, end.x), ny.mul_add(half_t, end.y)),
            (nx.mul_add(-half_t, end.x), ny.mul_add(-half_t, end.y)),
        ];
        let packed_color = pack_color(color);
        // 6 vertices (2 triangles) * 5 components = 30 u32s
        let mut verts: [u32; 30] = [0; 30];
        let indices = [0, 1, 2, 2, 1, 3];
        for (i, &idx) in indices.iter().enumerate() {
            let base = i * 5;
            let (px, py) = corners[idx];
            verts[base] = f32_to_u32(px);
            verts[base + 1] = f32_to_u32(py);
            verts[base + 2] = 0; // u
            verts[base + 3] = 0; // v
            verts[base + 4] = packed_color;
        }
        let bytes = bytemuck::cast_slice(&verts);
        let buf = &self.vertex_bufs[self.frame_index];
        self.queue
            .write_buffer(buf, self.vertex_offset as u64, bytes);
        self.vertex_offset += 6 * VERTEX_STRIDE;
    }

    /// write a text quad's 6 vertices into the persistent vertex buffer.
    /// vertex format: [pos.x, pos.y, u, v] (f32) + [packed rgba] (u32) = 20 bytes
    fn write_text_quad(&mut self, quad: &text::TextGlyphQuad, color: Color) {
        let x = quad.position.x;
        let y = quad.position.y;
        let w = quad.size.x;
        let h = quad.size.y;
        let u0 = quad.uv_min.x;
        let v0 = quad.uv_min.y;
        let u1 = quad.uv_max.x;
        let v1 = quad.uv_max.y;
        let packed_color = pack_color(color);
        // 6 vertices * 5 components = 30 u32s
        let mut verts: [u32; 30] = [0; 30];
        let positions_uvs = [
            (x, y, u0, v0),
            (x + w, y, u1, v0),
            (x, y + h, u0, v1),
            (x, y + h, u0, v1),
            (x + w, y, u1, v0),
            (x + w, y + h, u1, v1),
        ];
        for (i, (px, py, u, v)) in positions_uvs.iter().enumerate() {
            let base = i * 5;
            verts[base] = f32_to_u32(*px);
            verts[base + 1] = f32_to_u32(*py);
            verts[base + 2] = f32_to_u32(*u);
            verts[base + 3] = f32_to_u32(*v);
            verts[base + 4] = packed_color;
        }
        let bytes = bytemuck::cast_slice(&verts);
        let buf = &self.vertex_bufs[self.frame_index];
        self.queue
            .write_buffer(buf, self.vertex_offset as u64, bytes);
        self.vertex_offset += 6 * VERTEX_STRIDE;
    }
}

/// save the pipeline cache on shutdown so the next launch benefits from it.
#[cfg(not(target_arch = "wasm32"))]
impl Drop for RenderEngine {
    fn drop(&mut self) {
        self.save_pipeline_cache();
    }
}

/// pack an rgba color into a single u32 (r in lowest byte, a in highest).
fn pack_color(color: Color) -> u32 {
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let r = (color.r * 255.0).clamp(0.0, 255.0) as u32;
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let g = (color.g * 255.0).clamp(0.0, 255.0) as u32;
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let b = (color.b * 255.0).clamp(0.0, 255.0) as u32;
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let a = (color.a * 255.0).clamp(0.0, 255.0) as u32;
    (a << 24) | (b << 16) | (g << 8) | r
}

/// reinterpret an f32 as a u32 without conversion (for vertex buffer packing).
fn f32_to_u32(value: f32) -> u32 {
    bytemuck::cast(value)
}

const SHADER_SOURCE: &str = r"
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
";

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
    /// `uv_rect` overrides the default UV range \[0..1, 0..1\] (used for texture atlases).
    /// origin is the pivot point for rotation and scaling, relative to the sprite's top-left.
    Sprite {
        texture: Option<u64>,
        position: Vec2,
        rotation: f32,
        scale: Vec2,
        tint: Color,
        layer: i32,
        uv_rect: Option<(Vec2, Vec2)>,
        origin: Vec2,
    },
    /// draw a 2D rectangle
    Rect {
        position: Vec2,
        size: Vec2,
        color: Color,
        layer: i32,
    },
    /// draw a line between two points
    Line {
        start: Vec2,
        end: Vec2,
        color: Color,
        thickness: f32,
        layer: i32,
    },
    /// draw text
    Text {
        font: Option<u64>,
        content: String,
        position: Vec2,
        font_size: f32,
        color: Color,
        layer: i32,
    },
}

/// built-in layer constants for common rendering needs.
/// lower values are drawn first (behind), higher values are drawn last (in front).
pub mod layers {
    /// background layer — static backgrounds, parallax layers
    pub const BACKGROUND: i32 = 0;
    /// game layer — game objects, characters, projectiles
    pub const GAME: i32 = 100;
    /// foreground layer — effects, overlays, weather
    pub const FOREGROUND: i32 = 200;
    /// UI layer — HUD, menus, dialogue boxes
    pub const UI: i32 = 300;
}

/// ECS component that assigns an entity to a render layer.
///
/// entities with a higher layer value are drawn on top of lower layers.
/// use the [`layers`] constants for common layer assignments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Component)]
pub struct Layer(pub i32);

impl RenderQueue {
    /// create a new empty render queue
    #[must_use]
    pub fn new() -> Self {
        Self {
            commands: Vec::with_capacity(1024),
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
    pub const fn set_target(&mut self, target: Option<u32>) {
        self.target = target;
    }

    /// get the current render target
    #[must_use]
    pub const fn target(&self) -> Option<u32> {
        self.target
    }

    /// add a draw command
    pub fn push(&mut self, command: DrawCommand) {
        self.commands.push(command);
    }

    /// get all pending draw commands
    #[must_use]
    pub fn commands(&self) -> &[DrawCommand] {
        &self.commands
    }

    /// draw a sprite at the given position and size using a texture handle
    pub fn draw_sprite(&mut self, texture: &Handle<Texture>, position: Vec2, size: Vec2) {
        self.draw_sprite_on_layer(texture, position, size, layers::GAME);
    }

    /// draw a sprite on a specific layer
    pub fn draw_sprite_on_layer(
        &mut self,
        texture: &Handle<Texture>,
        position: Vec2,
        size: Vec2,
        layer: i32,
    ) {
        self.push(DrawCommand {
            kind: DrawKind::Sprite {
                texture: Some(u64::from(texture.id())),
                position,
                rotation: 0.0,
                scale: size,
                tint: Color::WHITE,
                layer,
                uv_rect: None,
                origin: Vec2::new(size.x * 0.5, size.y * 0.5),
            },
        });
    }

    /// draw a sprite from a texture atlas by region name.
    /// the `uv_rect` is automatically set from the atlas region's UV coordinates.
    pub fn draw_sprite_atlas(
        &mut self,
        texture: &Handle<Texture>,
        position: Vec2,
        size: Vec2,
        region: (Vec2, Vec2),
    ) {
        self.draw_sprite_atlas_on_layer(texture, position, size, region, layers::GAME);
    }

    /// draw a sprite from a texture atlas on a specific layer.
    pub fn draw_sprite_atlas_on_layer(
        &mut self,
        texture: &Handle<Texture>,
        position: Vec2,
        size: Vec2,
        region: (Vec2, Vec2),
        layer: i32,
    ) {
        self.push(DrawCommand {
            kind: DrawKind::Sprite {
                texture: Some(u64::from(texture.id())),
                position,
                rotation: 0.0,
                scale: size,
                tint: Color::WHITE,
                layer,
                uv_rect: Some(region),
                origin: Vec2::new(size.x * 0.5, size.y * 0.5),
            },
        });
    }

    /// draw a sprite with full transform control using a texture handle
    pub fn draw_sprite_transformed(&mut self, texture: &Handle<Texture>, params: SpriteParams) {
        self.draw_sprite_transformed_on_layer(texture, params, layers::GAME);
    }

    /// draw a sprite with full transform control on a specific layer
    pub fn draw_sprite_transformed_on_layer(
        &mut self,
        texture: &Handle<Texture>,
        params: SpriteParams,
        layer: i32,
    ) {
        self.push(DrawCommand {
            kind: DrawKind::Sprite {
                texture: Some(u64::from(texture.id())),
                position: params.position,
                rotation: params.rotation,
                scale: params.scale,
                tint: params.tint,
                layer,
                uv_rect: None,
                origin: params.origin,
            },
        });
    }

    /// draw a colored rectangle
    pub fn draw_rect(&mut self, position: Vec2, size: Vec2, color: Color) {
        self.draw_rect_on_layer(position, size, color, layers::GAME);
    }

    /// draw a colored rectangle on a specific layer
    pub fn draw_rect_on_layer(&mut self, position: Vec2, size: Vec2, color: Color, layer: i32) {
        self.push(DrawCommand {
            kind: DrawKind::Rect {
                position,
                size,
                color,
                layer,
            },
        });
    }

    /// draw a line between two points with the given thickness.
    /// uses a proper rotated rectangle, not an AABB approximation.
    pub fn draw_line(&mut self, start: Vec2, end: Vec2, color: Color, thickness: f32) {
        self.draw_line_on_layer(start, end, color, thickness, layers::GAME);
    }

    /// draw a line on a specific layer
    pub fn draw_line_on_layer(
        &mut self,
        start: Vec2,
        end: Vec2,
        color: Color,
        thickness: f32,
        layer: i32,
    ) {
        self.push(DrawCommand {
            kind: DrawKind::Line {
                start,
                end,
                color,
                thickness,
                layer,
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
                layer: layers::BACKGROUND,
            },
        });
    }

    /// draw text at the given position using the specified font handle
    pub fn draw_text(
        &mut self,
        font: &Handle<Font>,
        content: &str,
        position: Vec2,
        font_size: f32,
        color: Color,
    ) {
        self.draw_text_on_layer(font, content, position, font_size, color, layers::GAME);
    }

    /// draw text on a specific layer
    pub fn draw_text_on_layer(
        &mut self,
        font: &Handle<Font>,
        content: &str,
        position: Vec2,
        font_size: f32,
        color: Color,
        layer: i32,
    ) {
        self.push(DrawCommand {
            kind: DrawKind::Text {
                font: Some(u64::from(font.id())),
                content: content.to_string(),
                position,
                font_size,
                color,
                layer,
            },
        });
    }

    /// immediate mode drawing API for debug visualization and quick prototyping.
    ///
    /// the closure receives a [`DrawContext`] with convenience methods for
    /// drawing lines, circles, rects, and text without managing draw commands manually.
    ///
    /// # example
    ///
    /// ```ignore
    /// queue.draw_immediate(|draw| {
    ///     draw.line(Vec2::new(0.0, 0.0), Vec2::new(100.0, 100.0), Color::RED, 2.0);
    ///     draw.circle(Vec2::new(50.0, 50.0), 20.0, Color::GREEN, 2.0);
    ///     draw.rect(Vec2::new(10.0, 10.0), Vec2::new(40.0, 40.0), Color::BLUE);
    ///     draw.text("debug info", Vec2::new(0.0, 0.0), 16.0, Color::WHITE);
    /// });
    /// ```
    pub fn draw_immediate(&mut self, f: impl FnOnce(&mut DrawContext<'_>)) {
        let mut ctx = DrawContext { queue: self };
        f(&mut ctx);
    }
}

/// drawing context for immediate mode rendering.
///
/// provides convenience methods for debug drawing without managing
/// draw commands manually. obtained via [`RenderQueue::draw_immediate`].
pub struct DrawContext<'a> {
    queue: &'a mut RenderQueue,
}

impl DrawContext<'_> {
    /// draw a line between two points.
    pub fn line(&mut self, start: Vec2, end: Vec2, color: Color, thickness: f32) {
        self.queue.draw_line(start, end, color, thickness);
    }

    /// draw a filled rectangle.
    pub fn rect(&mut self, position: Vec2, size: Vec2, color: Color) {
        self.queue.draw_rect(position, size, color);
    }

    /// draw a stroked rectangle (outline only).
    pub fn rect_stroke(&mut self, position: Vec2, size: Vec2, color: Color, thickness: f32) {
        let Vec2 { x, y } = position;
        let Vec2 { x: w, y: h } = size;
        // top
        self.line(Vec2::new(x, y), Vec2::new(x + w, y), color, thickness);
        // bottom
        self.line(
            Vec2::new(x, y + h),
            Vec2::new(x + w, y + h),
            color,
            thickness,
        );
        // left
        self.line(Vec2::new(x, y), Vec2::new(x, y + h), color, thickness);
        // right
        self.line(
            Vec2::new(x + w, y),
            Vec2::new(x + w, y + h),
            color,
            thickness,
        );
    }

    /// draw a stroked circle (outline only, approximated with line segments).
    pub fn circle(&mut self, center: Vec2, radius: f32, color: Color, thickness: f32) {
        let segments = 32;
        #[allow(clippy::cast_precision_loss)]
        for i in 0..segments {
            let a1 = (i as f32 / segments as f32) * 2.0 * std::f32::consts::PI;
            let a2 = ((i + 1) as f32 / segments as f32) * 2.0 * std::f32::consts::PI;
            let x1 = center.x + a1.cos() * radius;
            let y1 = center.y + a1.sin() * radius;
            let x2 = center.x + a2.cos() * radius;
            let y2 = center.y + a2.sin() * radius;
            self.line(Vec2::new(x1, y1), Vec2::new(x2, y2), color, thickness);
        }
    }

    /// draw a filled circle (approximated with triangles from center).
    pub fn circle_filled(&mut self, center: Vec2, radius: f32, color: Color) {
        let segments = 32;
        #[allow(clippy::cast_precision_loss)]
        for i in 0..segments {
            let a1 = (i as f32 / segments as f32) * 2.0 * std::f32::consts::PI;
            let a2 = ((i + 1) as f32 / segments as f32) * 2.0 * std::f32::consts::PI;
            let x1 = center.x + a1.cos() * radius;
            let y1 = center.y + a1.sin() * radius;
            let x2 = center.x + a2.cos() * radius;
            let y2 = center.y + a2.sin() * radius;
            // draw triangle as thin rect from center to edge
            self.queue.push(DrawCommand {
                kind: DrawKind::Rect {
                    position: center,
                    size: Vec2::new((x2 - x1).abs() + 1.0, (y2 - y1).abs() + 1.0),
                    color,
                    layer: layers::FOREGROUND,
                },
            });
        }
    }

    /// draw text.
    pub fn text(&mut self, content: &str, position: Vec2, font_size: f32, color: Color) {
        // use a placeholder font id of 0 for immediate mode text
        self.queue.push(DrawCommand {
            kind: DrawKind::Text {
                font: Some(0),
                content: content.to_string(),
                position,
                font_size,
                color,
                layer: layers::FOREGROUND,
            },
        });
    }

    /// draw a point as a small filled circle.
    pub fn point(&mut self, position: Vec2, color: Color) {
        self.circle(position, 3.0, color, 1.0);
    }

    /// draw an AABB collision box.
    pub fn aabb(&mut self, min: Vec2, max: Vec2, color: Color, thickness: f32) {
        self.rect_stroke(min, max - min, color, thickness);
    }
}

/// trait for custom render passes that can be executed by the render engine.
///
/// implement this trait to add custom rendering (e.g. post-processing, 3D passes).
/// passes are executed in registration order after the default 2D pass.
pub trait RenderPass: Send + Sync + 'static {
    /// unique name for this pass.
    fn name(&self) -> &str;

    /// execute this render pass.
    /// the pass receives a reference to the render engine and the current
    /// render pass encoder. use these to issue draw commands.
    fn execute(
        &self,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _pass: &mut wgpu::RenderPass<'_>,
    ) {
    }
}

impl Default for RenderQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// render info resource, tracks rendering statistics.
///
/// updated each frame by the render system. game code can read
/// this to display debug info or make performance decisions.
#[derive(Resource)]
pub struct RenderInfo {
    /// window width in pixels
    pub window_width: u32,
    /// window height in pixels
    pub window_height: u32,
    /// current frames per second
    pub fps: f32,
    /// time to render last frame in milliseconds
    pub frame_time_ms: f32,
    /// number of draw calls issued last frame
    pub draw_calls: u32,
    /// number of sprites rendered last frame
    pub sprite_count: u32,
}

impl RenderInfo {
    /// create a new render info with default values
    #[must_use]
    pub const fn new() -> Self {
        Self {
            window_width: 0,
            window_height: 0,
            fps: 0.0,
            frame_time_ms: 0.0,
            draw_calls: 0,
            sprite_count: 0,
        }
    }
}

impl Default for RenderInfo {
    fn default() -> Self {
        Self::new()
    }
}

/// debug overlay for displaying runtime stats.
///
/// when enabled, draws FPS, frame time, sprite count, and entity count
/// in the top-left corner using immediate mode rendering.
#[derive(Resource)]
pub struct DebugOverlay {
    /// whether the overlay is currently visible
    pub enabled: bool,
    /// position in screen space (top-left corner)
    pub position: Vec2,
    /// font size for text
    pub font_size: f32,
    /// text color
    pub color: Color,
}

impl DebugOverlay {
    /// create a new debug overlay with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self {
            enabled: false,
            position: Vec2::new(10.0, 10.0),
            font_size: 14.0,
            color: Color::WHITE,
        }
    }

    /// draw debug info to the render queue.
    /// call this each frame with current stats.
    pub fn draw(
        &self,
        queue: &mut RenderQueue,
        fps: f32,
        frame_time_ms: f32,
        sprite_count: u32,
        entity_count: u32,
    ) {
        if !self.enabled {
            return;
        }
        let y = self.position.y;
        queue.draw_immediate(|draw| {
            draw.text(
                &format!("FPS: {fps:.1}"),
                Vec2::new(self.position.x, y),
                self.font_size,
                self.color,
            );
            draw.text(
                &format!("Frame: {frame_time_ms:.1}ms"),
                Vec2::new(self.position.x, y + self.font_size + 2.0),
                self.font_size,
                self.color,
            );
            draw.text(
                &format!("Sprites: {sprite_count}"),
                Vec2::new(self.position.x, y + (self.font_size + 2.0) * 2.0),
                self.font_size,
                self.color,
            );
            draw.text(
                &format!("Entities: {entity_count}"),
                Vec2::new(self.position.x, y + (self.font_size + 2.0) * 3.0),
                self.font_size,
                self.color,
            );
        });
    }
}

impl Default for DebugOverlay {
    fn default() -> Self {
        Self::new()
    }
}

/// render plugin, registers render systems and resources.
///
/// add this plugin to your [`App`] to enable rendering.
/// it registers the [`RenderQueue`] and [`RenderInfo`] as ECS resources.
pub struct RenderPlugin;

impl Default for RenderPlugin {
    fn default() -> Self {
        Self
    }
}

impl GamePlugin for RenderPlugin {
    fn name(&self) -> &'static str {
        "RenderPlugin"
    }

    fn build(&mut self, app: &mut App) {
        app.insert_resource(RenderQueue::new());
        app.insert_resource(RenderInfo::new());
        app.insert_resource(DebugOverlay::new());
        app.add_system_to_stage(engine_core::UpdateStage::Render, render_system);
        app.add_system_to_stage(engine_core::UpdateStage::Render, debug_overlay_system);
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

/// debug overlay system — draws FPS, frame time, sprite count, and entity count.
#[allow(clippy::needless_pass_by_value)]
fn debug_overlay_system(
    overlay: Res<DebugOverlay>,
    info: Res<RenderInfo>,
    mut queue: ResMut<RenderQueue>,
    entities: Query<Entity>,
) {
    #[allow(clippy::cast_possible_truncation)]
    let entity_count = entities.iter().count() as u32;
    overlay.draw(
        &mut queue,
        info.fps,
        info.frame_time_ms,
        info.sprite_count,
        entity_count,
    );
}
