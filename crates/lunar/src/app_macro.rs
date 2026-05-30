/// convenience macro to bootstrap a lunar game on native targets.
///
/// expands to a `main` function that initializes SDL3, creates a window,
/// sets up the wgpu render surface, adds all built-in plugins, and runs
/// the game loop. the window title defaults to `"Lunar"`.
///
/// this is the ONLY entry point game code needs — no `unsafe`, no SDL3,
/// no wgpu surface touching required. read window state through the
/// [`WindowSettings`] resource.
/// fullscreen can be toggled via F11/F (default) or by writing to
/// `WindowSettings::is_fullscreen`.
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

            let window = video
                .window("Lunar", config.width, config.height)
                .resizable()
                .build()
                .expect("failed to create window");

            let instance = $crate::wgpu::Instance::default();
            let surface = unsafe {
                let display_handle = window.display_handle().unwrap();
                let window_handle = window.window_handle().unwrap();
                // SAFETY: the SDL3 `window` is owned by this `main` scope and
                // outlives the wgpu surface (process exit drops both, with the
                // surface dropped first). The handles point into `window`'s
                // internal state and remain valid for that lifetime, satisfying
                // wgpu's `create_surface_unsafe` contract.
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

            app.insert_resource($crate::lunar_core::WindowSettings::new(
                config.width, config.height, config.vsync,
            ));
            app.insert_resource(render_engine);

            app.add_plugin($crate::lunar_render::RenderPlugin);
            app.add_plugin($crate::lunar_input::InputPlugin);
            app.add_plugin($crate::lunar_assets::AssetPlugin);
            // audio plugin slot — wire up here when the audio crate is ready

            // register default fullscreen toggle (F11 or F)
            app.add_startup_system(
                |mut actions: $crate::bevy_ecs::prelude::ResMut<
                    $crate::lunar_input::ActionMap,
                >| {
                    actions.bind(
                        "fullscreen",
                        $crate::lunar_input::InputBinding::Key(
                            $crate::lunar_input::KeyCode::F11,
                        ),
                    );
                    actions.bind(
                        "fullscreen",
                        $crate::lunar_input::InputBinding::Key(
                            $crate::lunar_input::KeyCode::F,
                        ),
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

                // handle fullscreen toggle via action
                if let Some(actions) =
                    world.get_resource::<$crate::lunar_input::ActionMap>()
                    && let Some(input) =
                        world.get_resource::<$crate::lunar_input::InputState>()
                    && actions.is_action_just_pressed(input, "fullscreen")
                {
                    actual_fullscreen = !actual_fullscreen;
                    let _ = window.set_fullscreen(actual_fullscreen);
                    if let Some(mut settings) =
                        world.get_resource_mut::<$crate::lunar_core::WindowSettings>()
                    {
                        settings.is_fullscreen = actual_fullscreen;
                    }
                }

                // check if game code set is_fullscreen directly
                if let Some(settings) =
                    world.get_resource::<$crate::lunar_core::WindowSettings>()
                {
                    if settings.is_fullscreen != actual_fullscreen {
                        let new_fs = settings.is_fullscreen;
                        let _ = window.set_fullscreen(new_fs);
                        actual_fullscreen = new_fs;
                    }
                }

                // handle window resize
                if let (Some(w), Some(h)) = (window.size().0, window.size().1) {
                    if w != last_window_w || h != last_window_h {
                        last_window_w = w;
                        last_window_h = h;
                        if let Some(mut re) = world
                            .get_resource_mut::<$crate::lunar_render::RenderEngine>()
                        {
                            re.resize_surface(w as u32, h as u32);
                        }
                        if let Some(mut settings) = world
                            .get_resource_mut::<$crate::lunar_core::WindowSettings>()
                        {
                            settings.width = w as u32;
                            settings.height = h as u32;
                        }
                    }
                }
            });

            $crate::log::info!("lunar engine shutting down...");
        }
    };
}
