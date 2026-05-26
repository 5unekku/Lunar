//! rendering subsystem via wgpu
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
//!
//! decoupled from game logic. handles 2D rendering with wgpu.
//! currently 2D; a future `lunar-render-3d` crate will slot in alongside this one.
//!
//! # rendering model
//!
//! game code does not touch the GPU directly. two paths feed the renderer:
//!
//! 1. **components** (preferred) — spawn entities with [`Sprite`] or [`Text`]
//!    alongside a [`Transform`]. built-in systems
//!    enqueue them automatically every frame.
//! 2. **immediate mode** (HUD / debug / one-shots) — call `draw_sprite`,
//!    `draw_rect`, `draw_line`, `draw_text` on [`RenderQueue`] from inside a
//!    system. useful when the thing you're drawing isn't a persistent entity.
//!
//! [`DrawCommand`] / [`DrawKind`] / [`RenderQueue::push`] are internal plumbing
//! and not part of the public contract — they're hidden from rustdoc.
//!
//! # example: component-driven
//!
//! ```ignore
//! use lunar::prelude::*;
//!
//! fn spawn_player(mut commands: Commands, assets: Res<AssetServer>) {
//!     commands.spawn((
//!         Transform::from_xy(100.0, 100.0),
//!         Sprite::new(assets.get_texture_handle("player.png")),
//!     ));
//! }
//! ```
//!
//! # example: immediate mode
//!
//! ```ignore
//! fn draw_hud(mut queue: ResMut<RenderQueue>) {
//!     queue.draw_rect(Vec2::ZERO, Vec2::new(200.0, 40.0), Color::rgba(0.0, 0.0, 0.0, 0.6));
//! }
//! ```

pub mod atlas;
mod text;
pub mod textbox;

use std::collections::HashMap;

use bevy_ecs::prelude::*;
use bevy_ecs::schedule::IntoScheduleConfigs;
use lunar_assets::{AssetServer, Font, Handle, Texture};
use lunar_core::{App, GamePlugin, Time};
use lunar_math::{Color, Transform, Vec2};

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
/// use lunar_render::Camera;
/// use lunar_math::Vec2;
///
/// // camera centered at (400, 300), letterboxed to an 800x600 viewport
/// let cam = Camera {
///     position: Vec2::new(400.0, 300.0),
///     zoom: 1.0,
///     rotation: 0.0,
///     viewport: Some((800, 600)),
///     layer_parallax: Default::default(),
/// };
///
/// // use cam.projection_matrix(window_w, window_h) for the render projection
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
    /// use lunar_render::Camera;
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
            sx * tx,
            sy * ty,
            0.0,
            1.0,
        ]
    }

    /// set a parallax offset for a specific layer.
    /// the offset is a world-space Vec2 subtracted from the camera position
    /// when rendering that layer. to scroll a background at half speed,
    /// pass `camera.position * 0.5` as the offset each frame.
    pub fn set_layer_parallax(&mut self, layer: i32, offset: Vec2) {
        self.layer_parallax.insert(layer, offset);
    }

    /// remove the parallax offset for a layer, reverting to normal camera tracking.
    pub fn clear_layer_parallax(&mut self, layer: i32) {
        self.layer_parallax.remove(&layer);
    }

    /// convert a screen-space pixel position to world-space coordinates.
    ///
    /// accounts for camera position, zoom, rotation, and viewport letterboxing.
    /// screen origin is top-left, y-down. world is top-left, y-down.
    ///
    /// # example
    ///
    /// ```ignore
    /// fn my_system(camera: Res<Camera>, input: Res<InputState>) {
    ///     let (mx, my) = input.mouse_position();
    ///     let world = camera.screen_to_world(Vec2::new(mx, my), 800, 600);
    ///     // spawn something at the mouse position in world space
    /// }
    /// ```
    #[must_use]
    pub fn screen_to_world(&self, screen: Vec2, window_width: u32, window_height: u32) -> Vec2 {
        let (vw, vh) = self.viewport.unwrap_or((window_width, window_height));
        #[allow(clippy::cast_precision_loss)]
        let (vw_f, vh_f) = (vw as f32, vh as f32);

        let zoom = self.zoom.max(0.001);
        let cos = self.rotation.cos();
        let sin = self.rotation.sin();

        // unapply projection transform — input is viewport-space (0..vw, 0..vh)
        let nx = screen.x / vw_f - 0.5;
        let ny = screen.y / vh_f - 0.5;
        let world_dx = nx * vw_f / zoom;
        let world_dy = ny * vh_f / zoom;

        // unrotate
        let unrot_x = world_dx * cos + world_dy * sin;
        let unrot_y = -world_dx * sin + world_dy * cos;

        Vec2::new(self.position.x + unrot_x, self.position.y + unrot_y)
    }

    /// convert a world-space position to screen-space pixel coordinates.
    ///
    /// inverse of [`screen_to_world`](Self::screen_to_world).
    /// the result is in screen pixel coordinates (top-left origin, y-down).
    #[must_use]
    pub fn world_to_screen(&self, world: Vec2, window_width: u32, window_height: u32) -> Vec2 {
        let (vw, vh) = self.viewport.unwrap_or((window_width, window_height));
        #[allow(clippy::cast_precision_loss)]
        let (vw_f, vh_f) = (vw as f32, vh as f32);

        let zoom = self.zoom.max(0.001);
        let cos = self.rotation.cos();
        let sin = self.rotation.sin();

        // rotate world delta and apply zoom/scale
        let dx = world.x - self.position.x;
        let dy = world.y - self.position.y;
        let rx = dx * cos - dy * sin;
        let ry = dx * sin + dy * cos;

        // apply ortho projection — output is viewport-space (0..vw, 0..vh)
        let sx = rx * zoom / vw_f;
        let sy = -ry * zoom / vh_f;

        Vec2::new((sx + 0.5) * vw_f, (0.5 - sy) * vh_f)
    }

    /// enable or disable viewport letterboxing.
    ///
    /// when enabled, the projection auto-computes black-bar offsets when the
    /// window aspect ratio doesn't match the viewport aspect ratio.
    /// this is called internally by [`set_target_aspect`](Self::set_target_aspect).
    ///
    /// returns `&mut Self` for chaining.
    pub fn set_target_aspect(&mut self, width: u32, height: u32) -> &mut Self {
        self.viewport = Some((width, height));
        self
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
/// used when initializing the [`RenderEngine`] and [`lunar_core::WindowSettings`].
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

/// initial vertex capacity per frame (64k vertices = ~10k sprites with packed color).
/// the buffer doubles automatically the frame after an overflow is detected,
/// so this is a tunable starting point — never a ceiling.
/// vertex format: [pos.x, pos.y, u, v] (16 bytes) + [`color_u32`] (4 bytes) = 20 bytes per vertex
const INITIAL_VERTEX_CAPACITY: usize = 65536;

/// bind group key reserved for the glyph atlas texture used by text draws.
/// regular sprite textures use their asset ID; the white placeholder uses `u32::MAX`.
const GLYPH_ATLAS_BIND_ID: u32 = u32::MAX - 1;

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
    sampler: wgpu::Sampler,
    textures: HashMap<u32, GpuTexture>,
    bind_groups: HashMap<u32, wgpu::BindGroup>,
    /// persistent vertex buffers — double-buffered to prevent GPU read/write conflicts
    vertex_bufs: [wgpu::Buffer; VERTEX_BUFFER_COUNT],
    /// current vertex capacity (number of vertices, not bytes). doubles when
    /// a frame overflows. starts at [`INITIAL_VERTEX_CAPACITY`].
    vertex_capacity: usize,
    /// set during render when a draw was dropped due to capacity. on the next
    /// frame, [`Self::grow_vertex_buffers`] doubles capacity before drawing.
    overflow_flag: bool,
    /// current frame index for buffer selection
    frame_index: usize,
    /// current write offset into the active vertex buffer
    vertex_offset: usize,
    glyph_atlas: text::GlyphAtlas,
    #[allow(dead_code)]
    glyph_atlas_texture: Option<GpuTexture>,
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
            power_preference: wgpu::PowerPreference::HighPerformance,
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
                power_preference: wgpu::PowerPreference::HighPerformance,
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

        // 1x1 white texture used for untextured draws (rects, lines, text)
        let placeholder_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("white 1x1"),
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
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &placeholder_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &[255u8, 255, 255, 255],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
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
                size: (INITIAL_VERTEX_CAPACITY * VERTEX_STRIDE) as u64,
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
            sampler,
            textures: HashMap::new(),
            bind_groups: {
                let mut map = HashMap::new();
                map.insert(u32::MAX, bind_group);
                map
            },
            vertex_bufs,
            vertex_capacity: INITIAL_VERTEX_CAPACITY,
            overflow_flag: false,
            frame_index: 0,
            vertex_offset: 0,
            glyph_atlas: text::GlyphAtlas::new(2048, 1024),
            glyph_atlas_texture: None,
            render_passes: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            pipeline_cache: Self::load_pipeline_cache(device),
        }
    }

    /// update the uniform buffer with the projection matrix for a specific layer.
    /// applies per-layer parallax offset from the camera if present.
    fn update_projection_for_layer(&mut self, layer: i32, camera: Option<&Camera>) {
        let (surface_w, surface_h) = (self.config.width as f32, self.config.height as f32);

        // if camera has a viewport, compute a letterboxed projection that fits
        // the viewport into the surface while preserving aspect ratio
        let projection = if let Some(cam) = camera
            && let Some((vp_w, vp_h)) = cam.viewport
        {
            let (vp_w, vp_h) = (vp_w as f32, vp_h as f32);
            // scale viewport to fit surface, maintaining aspect ratio
            let scale = (surface_w / vp_w).min(surface_h / vp_h);
            let vp_w_scaled = vp_w * scale;
            let vp_h_scaled = vp_h * scale;

            // clip-space offset to center the viewport
            let offset_x = (surface_w - vp_w_scaled) / surface_w;
            let offset_y = (surface_h - vp_h_scaled) / surface_h;

            // build a custom orthographic projection:
            // maps world (cam_x - vp_w/2, cam_y - vp_h/2) .. (cam_x + vp_w/2, cam_y + vp_h/2)
            // to a centered letterboxed region of clip space
            let _sx = (vp_w_scaled / surface_w) * 2.0 / vp_w;
            let _sy = -(vp_h_scaled / surface_h) * 2.0 / vp_h;
            let pos = cam.position;
            let tx = pos.x;
            let ty = pos.y;

            // clip_x = sx * (world_x - tx) + (sx * tx - 1 + offset_x)
            //        = sx * world_x + (sx * tx - 1 + offset_x - sx * tx)
            //        = sx * world_x - 1 + offset_x

            // Actually simpler: compute clip directly
            let left = -1.0 + offset_x;
            let right = 1.0 - offset_x;
            let bottom = -1.0 + offset_y;
            let top = 1.0 - offset_y;

            // scale from world to clip:
            // world_x = tx → clip_x = 0 → (left+right)/2
            // world_x = tx - vp_w/2 → clip_x = left
            // world_x = tx + vp_w/2 → clip_x = right

            let sx2 = (right - left) / vp_w;
            let sy2 = (bottom - top) / vp_h;
            let tx2 = (right + left) / 2.0;
            let ty2 = (top + bottom) / 2.0;

            [
                sx2,
                0.0,
                0.0,
                0.0,
                0.0,
                sy2,
                0.0,
                0.0,
                0.0,
                0.0,
                1.0,
                0.0,
                sx2 * (-tx) + tx2,
                sy2 * (-ty) + ty2,
                0.0,
                1.0,
            ]
        } else if let Some(cam) = camera {
            cam.projection_matrix_for_layer(layer, self.config.width, self.config.height)
        } else {
            [
                2.0 / surface_w,
                0.0,
                0.0,
                0.0,
                0.0,
                -2.0 / surface_h,
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
                    // SAFETY: `create_pipeline_cache` is unsafe because malformed
                    // cache data could cause undefined behavior in some drivers.
                    // We pass `fallback: true` so wgpu silently rebuilds a fresh
                    // cache if validation fails; the data on disk was written by
                    // a prior run of this same binary, so format mismatch is the
                    // only realistic risk and is handled by the fallback.
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

    /// register font bytes with the glyph atlas. glyphs are rasterized on demand
    /// during render() and the atlas is uploaded then.
    pub fn upload_font(&mut self, font_id: u32, data: &[u8]) {
        self.glyph_atlas.register_font(font_id, data);
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

    /// reconfigure the surface to a new size (e.g. for fullscreen).
    pub fn resize_surface(&mut self, width: u32, height: u32) {
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    /// current surface size
    pub fn surface_size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    /// recreate the persistent vertex buffers at the current `vertex_capacity`.
    /// called when the previous frame overflowed; doubles the capacity first.
    fn grow_vertex_buffers(&mut self) {
        let new_capacity = self.vertex_capacity.saturating_mul(2);
        log::warn!(
            "render: vertex buffer overflow detected; growing capacity {} → {} vertices",
            self.vertex_capacity,
            new_capacity
        );
        self.vertex_capacity = new_capacity;
        self.vertex_bufs = std::array::from_fn(|i| {
            self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(&format!("persistent vertex buffer {i}")),
                size: (self.vertex_capacity * VERTEX_STRIDE) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });
    }

    /// render all draw commands for this frame.
    /// sprites are batched by texture — one draw call per unique texture.
    /// rects (no texture) are drawn in a single additional draw call.
    #[allow(clippy::too_many_lines)]
    pub fn render(
        &mut self,
        commands: &[DrawCommand],
        camera: Option<&Camera>,
        render_info: &mut RenderInfo,
    ) {
        // if last frame overflowed, double the buffers before rendering this one.
        if self.overflow_flag {
            self.grow_vertex_buffers();
            self.overflow_flag = false;
        }

        // per-frame stats — written to RenderInfo before returning so the
        // debug overlay (and any game HUD that reads RenderInfo) shows real
        // values instead of the zero-defaults.
        let mut sprite_count: u32 = 0;
        let mut draw_calls: u32 = 0;

        let (wgpu::CurrentSurfaceTexture::Success(frame)
        | wgpu::CurrentSurfaceTexture::Suboptimal(frame)) = self.surface.get_current_texture()
        else {
            return;
        };

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        // update atlas rasterization scale from the viewport so glyphs are sharp
        // even when the game viewport is letterboxed into a larger window
        let text_scale = if let Some(cam) = camera
            && let Some((vp_w, vp_h)) = cam.viewport
        {
            let sx = self.config.width as f32 / vp_w as f32;
            let sy = self.config.height as f32 / vp_h as f32;
            sx.min(sy)
        } else {
            1.0
        };
        self.glyph_atlas.set_scale(text_scale);

        // pre-compute text layouts (fills atlas as a side effect)
        let mut text_quads: std::collections::HashMap<usize, Vec<text::TextGlyphQuad>> =
            std::collections::HashMap::new();
        for (i, cmd) in commands.iter().enumerate() {
            let DrawKind::Text {
                font,
                content,
                position,
                font_size,
                wrap_width,
                line_height,
                ..
            } = &cmd.kind
            else {
                continue;
            };
            let font_id = u32::try_from(font.unwrap_or(0)).unwrap_or(u32::MAX);
            let flat: Vec<text::TextGlyphQuad> = if let Some(max_w) = wrap_width {
                text::layout_text_wrapped(
                    &mut self.glyph_atlas,
                    font_id,
                    content,
                    *font_size,
                    *position,
                    *max_w,
                    *line_height,
                )
                .into_iter()
                .flatten()
                .collect()
            } else {
                text::layout_text(
                    &mut self.glyph_atlas,
                    font_id,
                    content,
                    *font_size,
                    *position,
                )
            };
            text_quads.insert(i, flat);
        }
        if std::mem::take(&mut self.glyph_atlas.dirty) {
            self.upload_glyph_atlas();
            if let Some(atlas) = &self.glyph_atlas_texture {
                let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("glyph atlas bind group"),
                    layout: &self.bind_group_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: self.uniform_buf.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(&atlas.view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::Sampler(&self.sampler),
                        },
                    ],
                });
                self.bind_groups.insert(GLYPH_ATLAS_BIND_ID, bind_group);
            }
        }

        // track current layer for parallax — updated as we iterate sorted commands
        let mut current_layer: Option<i32> = None;

        // sort by (layer, texture_id) — same-texture commands are contiguous, no HashMap needed
        let mut sorted_commands: Vec<(usize, &DrawCommand)> = commands.iter().enumerate().collect();
        sorted_commands.sort_by_key(|(_, cmd)| {
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
                DrawKind::Text { .. } => GLYPH_ATLAS_BIND_ID,
                _ => u32::MAX,
            };
            (layer, tex)
        });

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

            // advance frame index for double-buffering
            self.frame_index = (self.frame_index + 1) % VERTEX_BUFFER_COUNT;
            // reset persistent vertex buffer offset for this frame
            self.vertex_offset = 0;

            // single pass in sorted order — sprites and rects interleave correctly by layer.
            // sort gives (layer, tex_id) where rects use u32::MAX, so within a layer sprites
            // always precede rects, and lower layers are fully drawn before higher ones.
            let mut current_tex: Option<u32> = None;
            let mut batch_start = 0;

            for (orig_idx, command) in &sorted_commands {
                let layer = match &command.kind {
                    DrawKind::Sprite { layer, .. }
                    | DrawKind::Rect { layer, .. }
                    | DrawKind::Line { layer, .. }
                    | DrawKind::Text { layer, .. } => *layer,
                };

                let tex_id = match &command.kind {
                    DrawKind::Sprite {
                        texture: Some(id), ..
                    } => u32::try_from(*id).unwrap_or(u32::MAX),
                    DrawKind::Text { .. } => GLYPH_ATLAS_BIND_ID,
                    _ => u32::MAX,
                };

                // update projection if layer changed (parallax)
                if current_layer != Some(layer) {
                    if self.vertex_offset > batch_start
                        && let Some(prev_tex) = current_tex
                    {
                        let vertex_count = (self.vertex_offset - batch_start) / VERTEX_STRIDE;
                        self.draw_vertex_batch(&mut pass, prev_tex, batch_start, vertex_count);
                        draw_calls += 1;
                    }
                    batch_start = self.vertex_offset;
                    self.update_projection_for_layer(layer, camera);
                    current_layer = Some(layer);
                }

                // flush and switch bind group when tex changes
                if current_tex != Some(tex_id) {
                    if self.vertex_offset > batch_start
                        && let Some(prev_tex) = current_tex
                    {
                        let vertex_count = (self.vertex_offset - batch_start) / VERTEX_STRIDE;
                        self.draw_vertex_batch(&mut pass, prev_tex, batch_start, vertex_count);
                        draw_calls += 1;
                    }
                    batch_start = self.vertex_offset;
                    current_tex = Some(tex_id);
                }

                if self.vertex_offset + 6 * VERTEX_STRIDE > self.vertex_capacity * VERTEX_STRIDE {
                    self.overflow_flag = true;
                    continue;
                }

                match &command.kind {
                    DrawKind::Sprite {
                        texture: Some(_),
                        position,
                        rotation,
                        scale,
                        tint,
                        uv_rect,
                        origin,
                        ..
                    } => {
                        self.write_sprite_vertices(&SpriteDrawParams {
                            position: *position,
                            rotation: *rotation,
                            scale: *scale,
                            tint: *tint,
                            uv_rect: *uv_rect,
                            origin: *origin,
                        });
                        sprite_count += 1;
                    }
                    DrawKind::Sprite { texture: None, .. } => {}
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
                    DrawKind::Text { color, .. } => {
                        if let Some(quads) = text_quads.get(orig_idx) {
                            for quad in quads {
                                self.write_text_quad(quad, *color);
                            }
                        }
                    }
                }
            }

            // flush final batch
            if self.vertex_offset > batch_start
                && let Some(tex_id) = current_tex
            {
                let vertex_count = (self.vertex_offset - batch_start) / VERTEX_STRIDE;
                self.draw_vertex_batch(&mut pass, tex_id, batch_start, vertex_count);
                draw_calls += 1;
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

        render_info.window_width = self.config.width;
        render_info.window_height = self.config.height;
        render_info.sprite_count = sprite_count;
        render_info.draw_calls = draw_calls;
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

/// internal — a single draw command produced by the engine's enqueue helpers.
///
/// hidden from the public API: game code uses the [`Sprite`] / [`Text`]
/// components or the immediate-mode helpers on [`RenderQueue`]
/// (`draw_sprite`, `draw_text`, `draw_rect`, `draw_line`). this type and its
/// `kind` field exist only so the engine can pass commands to the renderer.
#[doc(hidden)]
#[derive(Debug, Clone)]
pub struct DrawCommand {
    /// draw type
    pub kind: DrawKind,
}

/// internal — primitive variant for a [`DrawCommand`].
///
/// hidden from the public API; see [`DrawCommand`].
#[doc(hidden)]
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
        /// if set, text wraps at this pixel width using word boundaries
        wrap_width: Option<f32>,
        /// line spacing when wrap_width is set; 0.0 = font_size * 1.25
        line_height: f32,
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

/// renderable 2D sprite component.
///
/// any entity carrying a [`Transform`] and a `Sprite`
/// is drawn automatically each frame. game code spawns the entity and the
/// engine's render system enqueues the draw — no manual `RenderQueue` calls.
///
/// # example
///
/// ```ignore
/// use lunar::prelude::*;
///
/// fn spawn_player(mut commands: Commands, assets: Res<AssetServer>) {
///     let texture = assets.get_texture_handle("player.png");
///     commands.spawn((
///         Transform::from_xy(100.0, 100.0),
///         Sprite::new(texture).with_size(Vec2::new(32.0, 32.0)),
///     ));
/// }
/// ```
///
/// fields can be set directly or via the builder methods. when `size` is
/// `None`, the sprite renders at the texture's native pixel size if the
/// texture is loaded; otherwise a 32×32 placeholder is used.
#[derive(Debug, Clone, Component)]
pub struct Sprite {
    /// texture to draw
    pub texture: Handle<Texture>,
    /// rendered size in pixels. `None` = use the texture's native size.
    /// the entity's `Transform::scale` is applied on top of this.
    pub size: Option<Vec2>,
    /// color tint multiplied with the texture (RGBA). default white = no tint.
    pub color: Color,
    /// optional UV sub-rect for atlas sampling: `(uv_min, uv_max)` in 0..1 space.
    pub source_rect: Option<(Vec2, Vec2)>,
    /// pivot for rotation/scale, in pixels relative to the sprite's top-left.
    /// `None` = sprite center (size / 2).
    pub origin: Option<Vec2>,
    /// render layer (lower = behind, higher = in front). see [`layers`].
    pub layer: i32,
}

impl Sprite {
    /// create a sprite with default settings (white tint, native size, centered, GAME layer)
    #[must_use]
    pub const fn new(texture: Handle<Texture>) -> Self {
        Self {
            texture,
            size: None,
            color: Color::WHITE,
            source_rect: None,
            origin: None,
            layer: layers::GAME,
        }
    }

    /// set explicit pixel size (overrides texture's native size)
    #[must_use]
    pub const fn with_size(mut self, size: Vec2) -> Self {
        self.size = Some(size);
        self
    }

    /// set color tint
    #[must_use]
    pub const fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// set the render layer
    #[must_use]
    pub const fn with_layer(mut self, layer: i32) -> Self {
        self.layer = layer;
        self
    }

    /// set the UV sub-rect for atlas sampling
    #[must_use]
    pub const fn with_source_rect(mut self, uv_min: Vec2, uv_max: Vec2) -> Self {
        self.source_rect = Some((uv_min, uv_max));
        self
    }

    /// set the origin (pivot point) in pixels relative to top-left
    #[must_use]
    pub const fn with_origin(mut self, origin: Vec2) -> Self {
        self.origin = Some(origin);
        self
    }
}

/// renderable text component.
///
/// any entity carrying a [`Transform`] and a `Text`
/// is drawn automatically each frame. position comes from `Transform.translation`.
///
/// # example
///
/// ```ignore
/// use lunar::prelude::*;
///
/// fn spawn_label(mut commands: Commands, assets: Res<AssetServer>) {
///     let font = assets.get_font_handle("ui.ttf");
///     commands.spawn((
///         Transform::from_xy(10.0, 10.0),
///         Text::new("Score: 0", font).with_size(20.0),
///     ));
/// }
/// ```
#[derive(Debug, Clone, Component)]
pub struct Text {
    /// text content
    pub content: String,
    /// font to render with
    pub font: Handle<Font>,
    /// font size in pixels
    pub font_size: f32,
    /// text color (RGBA)
    pub color: Color,
    /// render layer
    pub layer: i32,
}

impl Text {
    /// create a text component with default settings (16px white, UI layer)
    #[must_use]
    pub fn new(content: impl Into<String>, font: Handle<Font>) -> Self {
        Self {
            content: content.into(),
            font,
            font_size: 16.0,
            color: Color::WHITE,
            layer: layers::UI,
        }
    }

    /// set font size in pixels
    #[must_use]
    pub const fn with_size(mut self, font_size: f32) -> Self {
        self.font_size = font_size;
        self
    }

    /// set text color
    #[must_use]
    pub const fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// set the render layer
    #[must_use]
    pub const fn with_layer(mut self, layer: i32) -> Self {
        self.layer = layer;
        self
    }
}

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

    /// internal — enqueue a raw draw command. game code should prefer the
    /// [`Sprite`] / [`Text`] components or the `draw_*` helpers below.
    #[doc(hidden)]
    pub fn push(&mut self, command: DrawCommand) {
        self.commands.push(command);
    }

    /// internal — drain target for the renderer.
    #[doc(hidden)]
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
                wrap_width: None,
                line_height: 0.0,
            },
        });
    }

    /// draw text in screen-space coordinates (for UI).
    /// internally converts through the camera to world-space.
    /// the position is relative to the viewport top-left, y-down.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_ui_text(
        &mut self,
        font: &Handle<Font>,
        text: &str,
        screen_pos: Vec2,
        font_size: f32,
        color: Color,
        camera: &Camera,
        window_width: u32,
        window_height: u32,
    ) {
        let world = camera.screen_to_world(screen_pos, window_width, window_height);
        self.draw_text_on_layer(font, text, world, font_size, color, layers::UI);
    }

    /// draw a colored rectangle in screen-space coordinates (for UI).
    /// internally converts through the camera to world-space.
    pub fn draw_ui_rect(
        &mut self,
        screen_pos: Vec2,
        size: Vec2,
        color: Color,
        camera: &Camera,
        window_width: u32,
        window_height: u32,
    ) {
        let world = camera.screen_to_world(screen_pos, window_width, window_height);
        self.draw_rect_on_layer(world, size, color, layers::UI);
    }

    /// draw word-wrapped text on the given layer.
    /// `max_width` is the pixel width at which lines break.
    /// `line_height` is the vertical spacing per line; 0.0 = font_size * 1.25.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_text_wrapped(
        &mut self,
        font: &Handle<Font>,
        content: &str,
        position: Vec2,
        font_size: f32,
        color: Color,
        max_width: f32,
        line_height: f32,
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
                wrap_width: Some(max_width),
                line_height,
            },
        });
    }

    /// draw a sprite in screen-space coordinates (for UI).
    /// the position is the top-left corner in screen pixels, y-down.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_ui_sprite(
        &mut self,
        texture: &Handle<Texture>,
        screen_pos: Vec2,
        size: Vec2,
        camera: &Camera,
        window_width: u32,
        window_height: u32,
    ) {
        let world = camera.screen_to_world(screen_pos, window_width, window_height);
        self.draw_sprite_on_layer(texture, world, size, layers::UI);
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

    /// draw a filled circle (scanline rects, one pixel tall per row).
    pub fn circle_filled(&mut self, center: Vec2, radius: f32, color: Color) {
        let r = radius.ceil() as i32;
        #[allow(clippy::cast_precision_loss)]
        for dy in -r..=r {
            let dy_f = dy as f32 + 0.5; // sample at center of the scanline row
            let half_w = (radius * radius - dy_f * dy_f).sqrt();
            if half_w <= 0.0 {
                continue;
            }
            self.queue.push(DrawCommand {
                kind: DrawKind::Rect {
                    position: Vec2::new(center.x - half_w, center.y + dy as f32),
                    size: Vec2::new(half_w * 2.0, 1.0),
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
                wrap_width: None,
                line_height: 0.0,
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
        // upload_new_textures_system runs first to ensure any texture that became
        // ready this frame is on the GPU before auto_sprite_system enqueues draws.
        #[cfg(not(target_arch = "wasm32"))]
        app.add_system_to_stage(
            lunar_core::UpdateStage::Render,
            (
                upload_new_textures_system,
                upload_new_fonts_system,
                frame_stats_system,
                auto_sprite_system,
                auto_text_system,
                debug_overlay_system,
                render_system,
            )
                .chain(),
        );
        #[cfg(target_arch = "wasm32")]
        app.add_system_to_stage(
            lunar_core::UpdateStage::Render,
            (
                wasm_upload_new_textures_system,
                wasm_upload_new_fonts_system,
                frame_stats_system,
                auto_sprite_system,
                auto_text_system,
                debug_overlay_system,
                wasm_render_system,
            )
                .chain(),
        );
    }
}

// thread-local storage for the WASM render engine.
// wgpu WebGPU types are !Send, so we cannot store RenderEngine as an ECS Resource
// on WASM. instead, bootstrap stores it here and the render system borrows it.
#[cfg(target_arch = "wasm32")]
thread_local! {
    static WASM_RENDER_ENGINE: std::cell::RefCell<Option<RenderEngine>> =
        std::cell::RefCell::new(None);
}

/// store the render engine for WASM rendering.
/// call this once from bootstrap after async GPU init, before starting the game loop.
#[cfg(target_arch = "wasm32")]
pub fn wasm_set_render_engine(engine: RenderEngine) {
    WASM_RENDER_ENGINE.with(|cell| {
        *cell.borrow_mut() = Some(engine);
    });
}

/// wasm render system: borrows the engine from thread-local storage,
/// renders all queued commands, then clears the queue.
#[cfg(target_arch = "wasm32")]
#[allow(clippy::needless_pass_by_value)]
fn wasm_render_system(
    mut queue: ResMut<RenderQueue>,
    mut render_info: ResMut<RenderInfo>,
    camera: Option<Res<Camera>>,
) {
    WASM_RENDER_ENGINE.with(|cell| {
        if let Some(engine) = cell.borrow_mut().as_mut() {
            engine.render(queue.commands(), camera.as_deref(), &mut render_info);
        }
    });
    queue.clear();
}

/// uploads textures that became ready in the asset server to the GPU.
///
/// drains the pending list from [`AssetServer`] and calls [`RenderEngine::upload_texture`]
/// for each one. runs before the render chain so draws on the same frame a texture
/// loads will succeed.
#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::needless_pass_by_value)]
fn upload_new_textures_system(mut assets: ResMut<AssetServer>, mut render: ResMut<RenderEngine>) {
    for id in assets.drain_new_texture_ids() {
        if let Some(texture) = assets.get_texture_by_id(id) {
            let handle = Handle::<Texture>::new(id, 0);
            render.upload_texture(&handle, texture);
        }
    }
}

/// WASM version — accesses the render engine from thread-local storage.
#[cfg(target_arch = "wasm32")]
#[allow(clippy::needless_pass_by_value)]
fn wasm_upload_new_textures_system(mut assets: ResMut<AssetServer>) {
    let ids = assets.drain_new_texture_ids();
    WASM_RENDER_ENGINE.with(|cell| {
        if let Some(engine) = cell.borrow_mut().as_mut() {
            for id in ids {
                if let Some(texture) = assets.get_texture_by_id(id) {
                    let handle = Handle::<Texture>::new(id, 0);
                    engine.upload_texture(&handle, texture);
                }
            }
        }
    });
}

#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::needless_pass_by_value)]
fn upload_new_fonts_system(mut assets: ResMut<AssetServer>, mut render: ResMut<RenderEngine>) {
    for id in assets.drain_new_font_ids() {
        if let Some(font) = assets.get_font_by_id(id) {
            render.upload_font(id, &font.data);
        }
    }
}

#[cfg(target_arch = "wasm32")]
#[allow(clippy::needless_pass_by_value)]
fn wasm_upload_new_fonts_system(mut assets: ResMut<AssetServer>) {
    let ids = assets.drain_new_font_ids();
    WASM_RENDER_ENGINE.with(|cell| {
        if let Some(engine) = cell.borrow_mut().as_mut() {
            for id in &ids {
                if let Some(font) = assets.get_font_by_id(*id) {
                    engine.upload_font(*id, &font.data);
                }
            }
        }
    });
}

/// populate the [`RenderInfo`] resource each frame from [`Time`] so the debug
/// overlay (and any game-side HUD that reads it) shows real values. without
/// this, `info.fps` and `info.frame_time_ms` stayed at their `0.0` default.
#[allow(clippy::needless_pass_by_value)]
fn frame_stats_system(time: Res<Time>, mut info: ResMut<RenderInfo>) {
    let raw_delta = time.raw_delta_seconds();
    info.frame_time_ms = raw_delta * 1000.0;
    info.fps = if raw_delta > 0.0 {
        1.0 / raw_delta
    } else {
        0.0
    };
}

/// auto-render system: enqueues a sprite draw for every entity with both
/// `Transform` and `Sprite`. resolves native texture size from `AssetServer`
/// when `Sprite::size` is `None`.
#[allow(clippy::needless_pass_by_value)]
fn auto_sprite_system(
    assets: Option<Res<AssetServer>>,
    mut queue: ResMut<RenderQueue>,
    query: Query<(&Transform, &Sprite)>,
) {
    for (transform, sprite) in &query {
        let resolved_size = sprite.size.unwrap_or_else(|| {
            assets
                .as_deref()
                .and_then(|server| server.get_texture(&sprite.texture))
                .map_or(Vec2::splat(32.0), |texture| {
                    Vec2::new(texture.width as f32, texture.height as f32)
                })
        });
        let final_size = resolved_size * transform.scale;
        let origin = sprite
            .origin
            .map_or_else(|| final_size * 0.5, |o| o * transform.scale);
        queue.push(DrawCommand {
            kind: DrawKind::Sprite {
                texture: Some(u64::from(sprite.texture.id())),
                position: transform.translation,
                rotation: transform.rotation,
                scale: final_size,
                tint: sprite.color,
                layer: sprite.layer,
                uv_rect: sprite.source_rect,
                origin,
            },
        });
    }
}

/// auto-render system: enqueues a text draw for every entity with both
/// `Transform` and `Text`.
#[allow(clippy::needless_pass_by_value)]
fn auto_text_system(mut queue: ResMut<RenderQueue>, query: Query<(&Transform, &Text)>) {
    for (transform, text) in &query {
        queue.push(DrawCommand {
            kind: DrawKind::Text {
                font: Some(u64::from(text.font.id())),
                content: text.content.clone(),
                position: transform.translation,
                font_size: text.font_size,
                color: text.color,
                layer: text.layer,
                wrap_width: None,
                line_height: 0.0,
            },
        });
    }
}

/// render system that processes the render queue.
/// clears the queue at the start of each frame, then renders all commands.
/// native-only: takes `ResMut<RenderEngine>` and `RenderEngine` only implements
/// `Resource` on native (WebGPU types are `!Send` on wasm).
#[cfg(not(target_arch = "wasm32"))]
fn render_system(
    mut render_engine: ResMut<RenderEngine>,
    mut queue: ResMut<RenderQueue>,
    mut render_info: ResMut<RenderInfo>,
    camera: Option<Res<Camera>>,
) {
    render_engine.render(queue.commands(), camera.as_deref(), &mut render_info);
    queue.clear();
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
