/// convenience macro to bootstrap a lunar game.
///
/// expands to a `main` function that initializes SDL3, creates a window,
/// sets up the wgpu render surface, adds all built-in plugins, and runs
/// the game loop. the window title defaults to `"Lunar"`.
///
/// # usage
///
/// ```ignore
/// use lunar::lunar_app;
/// use engine_core::GamePlugin;
///
/// #[derive(Default)]
/// struct MyGame;
///
/// impl GamePlugin for MyGame {
///     fn name(&self) -> &str { "MyGame" }
/// }
///
/// lunar_app!(MyGame);
/// ```
///
/// # with a custom render config
///
/// ```ignore
/// lunar_app!(MyGame, engine_render::RenderConfig { width: 1920, height: 1080, ..Default::default() });
/// ```
#[macro_export]
macro_rules! lunar_app {
    ($game_plugin:ty) => {
        $crate::lunar_app!($game_plugin, engine_render::RenderConfig::default());
    };
    ($game_plugin:ty, $render_config:expr) => {
        fn main() {
            env_logger::init();
            log::info!("lunar engine starting...");

            let sdl = sdl3::init().expect("failed to initialize SDL3");
            let video = sdl.video().expect("failed to get video subsystem");
            let config = $render_config;

            let window = video
                .window("Lunar", config.width, config.height)
                .resizable()
                .build()
                .expect("failed to create window");

            let instance = wgpu::Instance::default();
            let surface = unsafe {
                let display_handle = window.display_handle().unwrap();
                let window_handle = window.window_handle().unwrap();
                instance
                    .create_surface_unsafe(
                        wgpu::SurfaceTargetUnsafe::from_display_and_window(
                            &display_handle,
                            &window_handle,
                        )
                        .unwrap(),
                    )
                    .expect("failed to create wgpu surface")
            };

            // from_surface is sync on native (pollster-backed)
            let render_engine =
                engine_render::RenderEngine::from_surface(&instance, surface, config.clone());

            let mut app = engine_core::App::new();
            app.insert_resource(render_engine);

            app.add_plugin(engine_input::InputPlugin);
            app.add_plugin(engine_render::RenderPlugin);
            app.add_plugin(engine_assets::AssetPlugin);
            app.add_plugin(engine_audio::AudioPlugin);
            app.add_plugin(<$game_plugin>::default());

            let mut event_pump = sdl.event_pump().expect("failed to get event pump");
            app.run_with_events(config.frame_cap, |world| {
                engine_input::process_events(&mut event_pump, world);
            });

            log::info!("lunar engine shutting down...");
        }
    };
}
