//! lunar game engine
//!
//! main entry point that wires up all subsystems and runs the game loop.

mod app_macro;

use engine_core::{CommandRegistry, EngineState, GameLoop};
use engine_render::RenderConfig;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use sdl3::event::{Event, WindowEvent};
use sdl3::keyboard::Keycode;

#[allow(clippy::too_many_lines, clippy::cast_sign_loss)]
#[tokio::main]
async fn main() {
    env_logger::init();

    log::info!("lunar engine starting...");

    let sdl = sdl3::init().expect("failed to initialize SDL3");
    let video = sdl.video().expect("failed to get video subsystem");

    let window = video
        .window("Lunar", 1280, 720)
        .resizable()
        .build()
        .expect("failed to create window");

    log::info!("window created");

    let instance = wgpu::Instance::default();
    // SAFETY: the window and display handles are valid for the lifetime of the surface
    let surface = unsafe {
        let display_handle = window.display_handle().unwrap();
        let window_handle = window.window_handle().unwrap();
        instance
            .create_surface_unsafe(
                wgpu::SurfaceTargetUnsafe::from_display_and_window(&display_handle, &window_handle)
                    .unwrap(),
            )
            .expect("failed to create wgpu surface")
    };

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
        .await
        .expect("failed to request device");

    let caps = surface.get_capabilities(&adapter);
    let format = caps
        .formats
        .first()
        .copied()
        .unwrap_or(wgpu::TextureFormat::Bgra8UnormSrgb);

    let config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format,
        width: 1280,
        height: 720,
        present_mode: wgpu::PresentMode::AutoVsync,
        alpha_mode: caps.alpha_modes.first().copied().unwrap_or_default(),
        view_formats: vec![],
        desired_maximum_frame_latency: 2,
    };

    surface.configure(&device, &config);

    log::info!("wgpu device and surface configured");

    let mut event_pump = sdl.event_pump().expect("failed to get event pump");
    let mut running = true;

    let mut game_loop = GameLoop::new(60);
    #[allow(clippy::no_effect_underscore_binding)]
    let mut _state = EngineState::Running;
    let _command_registry = CommandRegistry::new();
    let _render_config = RenderConfig {
        frame_cap: 60,
        ..Default::default()
    };

    while running {
        for event in event_pump.poll_iter() {
            match event {
                Event::Quit { .. }
                | Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => {
                    running = false;
                }
                Event::Window {
                    win_event: WindowEvent::Resized(width, height),
                    ..
                } => {
                    let new_config = wgpu::SurfaceConfiguration {
                        width: width as u32,
                        height: height as u32,
                        ..config.clone()
                    };
                    surface.configure(&device, &new_config);
                }
                _ => {}
            }
        }

        let ticks = game_loop.tick();

        if ticks > 0 {
            // game logic tick
            for _ in 0..ticks {
                // process game logic
            }
        }

        // render frame
        match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                let view = frame
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());

                let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("render encoder"),
                });

                {
                    let _render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("main render pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &view,
                            resolve_target: None,
                            depth_slice: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color {
                                    r: 0.1,
                                    g: 0.2,
                                    b: 0.3,
                                    a: 1.0,
                                }),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                }

                queue.submit(std::iter::once(encoder.finish()));
                frame.present();
            }
            wgpu::CurrentSurfaceTexture::Timeout => {
                log::debug!("surface texture timeout");
            }
            wgpu::CurrentSurfaceTexture::Occluded => {
                log::debug!("surface occluded");
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                log::warn!("surface outdated");
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                log::error!("surface lost");
                running = false;
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                log::error!("validation error");
            }
        }

        game_loop.apply_frame_cap();
    }

    log::info!("lunar engine shutting down...");
}
