//! rendering subsystem via wgpu
//!
//! completely decoupled from game logic. handles 2D rendering with wgpu.
//! architecture allows for future 3D expansion without breaking changes.

//! rendering configuration
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

/// render engine handle, owns all rendering state
pub struct RenderEngine {
    /// wgpu surface
    surface: wgpu::Surface<'static>,
    /// wgpu device
    device: wgpu::Device,
    /// wgpu queue
    queue: wgpu::Queue,
    /// surface configuration
    config: wgpu::SurfaceConfiguration,
    /// current render config
    render_config: RenderConfig,
}

impl RenderEngine {
    /// create a new render engine instance
    pub async fn new<
        W: raw_window_handle::HasWindowHandle
            + raw_window_handle::HasDisplayHandle
            + Send
            + Sync
            + 'static,
    >(
        instance: &wgpu::Instance,
        window: W,
        config: RenderConfig,
    ) -> Result<Self, wgpu::RequestDeviceError> {
        let surface = instance
            .create_surface(window)
            .expect("failed to create surface");

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
            })
            .await
            .expect("failed to request adapter");

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("lunar render device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::default(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
            })
            .await?;

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

        log::info!(
            "render engine initialized: {}x{}, frame_cap={}",
            config.width,
            config.height,
            if config.frame_cap == 0 {
                "uncapped".to_string()
            } else {
                config.frame_cap.to_string()
            }
        );

        Ok(RenderEngine {
            surface,
            device,
            queue,
            config: surface_config,
            render_config: config,
        })
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

    /// begin a new frame, returns a view to render into
    pub fn begin_frame(&self) -> Option<wgpu::SurfaceTexture> {
        match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => Some(frame),
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => Some(frame),
            wgpu::CurrentSurfaceTexture::Timeout => {
                log::warn!("surface texture timed out");
                None
            }
            wgpu::CurrentSurfaceTexture::Occluded => {
                log::warn!("surface texture occluded");
                None
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                log::warn!("surface texture outdated, resizing may be needed");
                None
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                log::error!("surface texture lost");
                None
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                log::error!("surface texture validation error");
                None
            }
        }
    }

    /// present the current frame
    pub fn present(&self, frame: wgpu::SurfaceTexture) {
        frame.present();
    }

    /// resize the render surface
    pub fn resize(&mut self, width: u32, height: u32) {
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.render_config.width = width;
        self.render_config.height = height;
    }
}

/// render queue resource, collects draw commands each frame
#[derive(bevy_ecs::prelude::Resource)]
pub struct RenderQueue {
    /// pending draw commands
    commands: Vec<DrawCommand>,
}

/// a single draw command
#[derive(Debug, Clone)]
pub struct DrawCommand {
    /// entity id
    pub entity: u64,
    /// draw type
    pub kind: DrawKind,
}

/// type of draw command
#[derive(Debug, Clone)]
pub enum DrawKind {
    /// draw a 2D sprite
    Sprite {
        /// texture handle
        texture: Option<u64>,
        /// position
        position: (f32, f32),
        /// rotation in radians
        rotation: f32,
        /// scale
        scale: (f32, f32),
        /// tint color
        tint: (f32, f32, f32, f32),
    },
    /// draw a 2D rectangle
    Rect {
        /// position
        position: (f32, f32),
        /// size
        size: (f32, f32),
        /// fill color
        color: (f32, f32, f32, f32),
    },
    /// draw text
    Text {
        /// text content
        content: String,
        /// position
        position: (f32, f32),
        /// font size
        font_size: f32,
        /// color
        color: (f32, f32, f32, f32),
    },
}

impl RenderQueue {
    /// create a new empty render queue
    pub fn new() -> Self {
        RenderQueue {
            commands: Vec::new(),
        }
    }

    /// clear all pending draw commands
    pub fn clear(&mut self) {
        self.commands.clear();
    }

    /// add a draw command
    pub fn push(&mut self, command: DrawCommand) {
        self.commands.push(command);
    }

    /// get all pending draw commands
    pub fn commands(&self) -> &[DrawCommand] {
        &self.commands
    }
}

impl Default for RenderQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// render plugin, registers render systems and resources
pub struct RenderPlugin;

impl engine_core::GamePlugin for RenderPlugin {
    fn name(&self) -> &str {
        "RenderPlugin"
    }

    fn build(&mut self, app: &mut engine_core::App) {
        app.insert_resource(RenderQueue::new());
    }
}
