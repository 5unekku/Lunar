/// convenience macro to bootstrap a lunar game on native targets.
///
/// expands to a `main` function that initializes SDL3, creates a window,
/// sets up the wgpu render surface, adds all built-in plugins, and runs
/// the game loop.
///
/// fullscreen: F11 or Alt+Enter. game code can rebind F11 via ActionMap.
/// set `target_aspect` in the RenderConfig to lock the window aspect ratio.
///
/// # usage
///
/// ```ignore
/// use lunar::lunar_app;
///
/// #[derive(Default)]
/// struct MyGame;
///
/// impl lunar::GamePlugin for MyGame {
///     fn name(&self) -> &str { "MyGame" }
///     fn build(&mut self, app: &mut lunar::App) {
///         app.add_system(my_system);
///     }
/// }
///
/// lunar_app!(MyGame);
/// ```
///
/// # with a custom render config
///
/// ```ignore
/// lunar_app!(MyGame, lunar::RenderConfig { width: 1920, height: 1080, ..Default::default() });
/// ```
#[macro_export]
macro_rules! lunar_app {
    ($game_plugin:ty) => {
        $crate::lunar_app!($game_plugin, $crate::lunar_render::RenderConfig::default());
    };
    ($game_plugin:ty, $render_config:expr) => {
        fn main() {
            $crate::env_logger::init();
            $crate::log::info!("lunar engine starting...");

            let sdl = $crate::sdl3::init().expect("failed to initialize SDL3");
            let video = sdl.video().expect("failed to get video subsystem");
            let config = $render_config;

            let window = {
                let mut b = video.window("Lunar", config.width, config.height);
                if config.allow_resize { b.resizable(); }
                b.build().expect("failed to create window")
            };

            let instance = $crate::wgpu::Instance::default();
            let surface = unsafe {
                let display_handle = window.display_handle().unwrap();
                let window_handle = window.window_handle().unwrap();
                // SAFETY: the SDL3 `window` is owned by this `main` scope and
                // outlives the wgpu surface (process exit drops both, with the
                // surface dropped first). the handles point into `window`'s
                // internal state and remain valid for that lifetime.
                instance
                    .create_surface_unsafe(
                        $crate::wgpu::SurfaceTargetUnsafe::from_display_and_window(
                            &display_handle,
                            &window_handle,
                        )
                        .unwrap(),
                    )
                    .expect("failed to create wgpu surface")
            };

            let render_engine = $crate::lunar_render::RenderEngine::from_surface(
                &instance, surface, config.clone(),
            );

            let mut app = $crate::lunar_core::App::new();

            let mut initial_settings =
                $crate::lunar_core::WindowSettings::new(config.width, config.height, config.vsync);
            initial_settings.target_aspect = config.target_aspect;
            initial_settings.allow_resize  = config.allow_resize;
            app.insert_resource(initial_settings);
            app.insert_resource(render_engine);

            app.add_plugin($crate::lunar_render::RenderPlugin);
            app.add_plugin($crate::lunar_input::InputPlugin);
            app.add_plugin($crate::lunar_assets::AssetPlugin);

            app.add_startup_system(
                |mut actions: $crate::bevy_ecs::prelude::ResMut<
                    $crate::lunar_input::ActionMap,
                >| {
                    actions.bind(
                        "fullscreen",
                        $crate::lunar_input::InputBinding::Key($crate::lunar_input::KeyCode::F11),
                    );
                },
            );

            app.add_plugin(<$game_plugin>::default());

            let mut event_pump = sdl.event_pump().expect("failed to get event pump");
            let mut window = window;
            let mut actual_fullscreen = false;
            let mut last_window_w = config.width;
            let mut last_window_h = config.height;

            app.run_with_events(config.frame_cap, config.tick_rate, |world| {
                $crate::lunar_input::process_events(&mut event_pump, world);

                let input_snap = world
                    .get_resource::<$crate::lunar_input::InputState>()
                    .map(|i| {
                        let enter = i.is_key_just_pressed($crate::lunar_input::KeyCode::Enter);
                        let alt   = i.is_key_held($crate::lunar_input::KeyCode::LAlt)
                            || i.is_key_held($crate::lunar_input::KeyCode::RAlt);
                        let fs_action = world
                            .get_resource::<$crate::lunar_input::ActionMap>()
                            .is_some_and(|a| a.is_action_just_pressed(i, "fullscreen"));
                        (enter && alt, fs_action)
                    });

                if input_snap.is_some_and(|(alt_enter, _)| alt_enter) {
                    actual_fullscreen = !actual_fullscreen;
                    let _ = window.set_fullscreen(actual_fullscreen);
                    if let Some(mut settings) = world
                        .get_resource_mut::<$crate::lunar_core::WindowSettings>()
                    {
                        settings.is_fullscreen = actual_fullscreen;
                    }
                }
                if input_snap.is_some_and(|(_, fs)| fs) {
                    actual_fullscreen = !actual_fullscreen;
                    let _ = window.set_fullscreen(actual_fullscreen);
                    if let Some(mut settings) = world
                        .get_resource_mut::<$crate::lunar_core::WindowSettings>()
                    {
                        settings.is_fullscreen = actual_fullscreen;
                    }
                }

                if let Some(settings) =
                    world.get_resource::<$crate::lunar_core::WindowSettings>()
                    && settings.is_fullscreen != actual_fullscreen
                {
                    let new_fs = settings.is_fullscreen;
                    let _ = window.set_fullscreen(new_fs);
                    actual_fullscreen = new_fs;
                }

                let (w, h) = window.size();
                if w != last_window_w || h != last_window_h {
                    let target = world
                        .get_resource::<$crate::lunar_core::WindowSettings>()
                        .and_then(|s| if !actual_fullscreen { s.target_aspect } else { None });

                    let (final_w, final_h) = if let Some(aspect) = target {
                        let snapped_h = ((w as f32 / aspect).round() as u32).max(1);
                        if snapped_h != h { let _ = window.set_size(w, snapped_h); }
                        (w, snapped_h)
                    } else {
                        (w, h)
                    };

                    last_window_w = final_w;
                    last_window_h = final_h;
                    if let Some(mut re) = world
                        .get_resource_mut::<$crate::lunar_render::RenderEngine>()
                    {
                        re.resize_surface(final_w as u32, final_h as u32);
                    }
                    if let Some(mut settings) = world
                        .get_resource_mut::<$crate::lunar_core::WindowSettings>()
                    {
                        settings.width  = final_w;
                        settings.height = final_h;
                    }
                }
            });

            $crate::log::info!("lunar engine shutting down...");
        }
    };
}
