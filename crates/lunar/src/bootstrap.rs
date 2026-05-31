/// bootstrap a native lunar game.
///
/// initializes SDL3, creates a window, sets up the wgpu render surface,
/// adds all built-in plugins, and runs the game loop.
///
/// fullscreen: F11 or Alt+Enter by default. game code can rebind F11 via
/// [`lunar_input::ActionMap`]. Alt+Enter is always active.
///
/// set `config.target_aspect` to lock the window to a fixed aspect ratio —
/// the height snaps to maintain the ratio when the user resizes.
#[cfg(not(target_arch = "wasm32"))]
pub fn bootstrap<Plugin: lunar_core::GamePlugin + Default + 'static>(
    config: lunar_render::RenderConfig,
) {
    use lunar_assets::AssetPlugin;
    use lunar_core::{App, WindowSettings};
    use lunar_input::{ActionMap, InputBinding, InputPlugin, InputState, KeyCode, SdlGamepadProvider, process_events};
    use lunar_render::{RenderEngine, RenderPlugin};
    use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

    env_logger::init();
    log::info!("lunar engine starting...");

    let sdl = sdl3::init().expect("failed to initialize SDL3");
    let video = sdl.video().expect("failed to get video subsystem");

    let window = {
        let mut b = video.window("Lunar", config.width, config.height);
        if config.allow_resize { b.resizable(); }
        b.build().expect("failed to create window")
    };

    let instance = wgpu::Instance::default();
    let surface = unsafe {
        let display_handle = window.display_handle().unwrap();
        let window_handle = window.window_handle().unwrap();
        // SAFETY: the SDL3 window is owned by this function's stack frame and
        // outlives the wgpu surface. handles point into `window`'s internal state
        // and remain valid for that lifetime.
        instance
            .create_surface_unsafe(
                wgpu::SurfaceTargetUnsafe::from_display_and_window(&display_handle, &window_handle)
                    .unwrap(),
            )
            .expect("failed to create wgpu surface")
    };

    let render_engine = RenderEngine::from_surface(&instance, surface, config.clone());

    let mut app = App::new();

    let mut initial_settings = WindowSettings::new(config.width, config.height, config.vsync);
    initial_settings.target_aspect = config.target_aspect;
    initial_settings.allow_resize  = config.allow_resize;
    app.insert_resource(initial_settings);
    app.insert_resource(render_engine);

    app.add_plugin(RenderPlugin);
    app.add_plugin(InputPlugin);
    app.add_plugin(AssetPlugin);

    app.add_startup_system(|mut actions: bevy_ecs::prelude::ResMut<ActionMap>| {
        actions.bind("fullscreen", InputBinding::Key(KeyCode::F11));
    });

    app.add_plugin(Plugin::default());

    let gamepad_subsystem = sdl.gamepad().expect("failed to get gamepad subsystem");
    let mut event_pump = sdl.event_pump().expect("failed to get event pump");
    let mut sdl_gamepad = SdlGamepadProvider::new(gamepad_subsystem);
    let mut window = window;
    let mut actual_fullscreen = false;
    let mut last_window_w = config.width;
    let mut last_window_h = config.height;

    app.run_with_events(config.frame_cap, config.tick_rate, |world| {
        process_events(&mut event_pump, &mut sdl_gamepad, world);

        let input_snap = world.get_resource::<InputState>().map(|i| {
            let enter     = i.is_key_just_pressed(KeyCode::Enter);
            let alt       = i.is_key_held(KeyCode::LAlt) || i.is_key_held(KeyCode::RAlt);
            let fs_action = world.get_resource::<ActionMap>()
                .is_some_and(|a| a.is_action_just_pressed(i, "fullscreen"));
            (enter && alt, fs_action)
        });

        if input_snap.is_some_and(|(alt_enter, _)| alt_enter) {
            actual_fullscreen = !actual_fullscreen;
            let _ = window.set_fullscreen(actual_fullscreen);
            if let Some(mut settings) = world.get_resource_mut::<WindowSettings>() {
                settings.is_fullscreen = actual_fullscreen;
            }
        }
        if input_snap.is_some_and(|(_, fs)| fs) {
            actual_fullscreen = !actual_fullscreen;
            let _ = window.set_fullscreen(actual_fullscreen);
            if let Some(mut settings) = world.get_resource_mut::<WindowSettings>() {
                settings.is_fullscreen = actual_fullscreen;
            }
        }

        if let Some(settings) = world.get_resource::<WindowSettings>()
            && settings.is_fullscreen != actual_fullscreen
        {
            actual_fullscreen = settings.is_fullscreen;
            let _ = window.set_fullscreen(actual_fullscreen);
        }

        let (w, h) = window.size();
        if w != last_window_w || h != last_window_h {
            let target = world.get_resource::<WindowSettings>()
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
            if let Some(mut re) = world.get_resource_mut::<RenderEngine>() {
                re.resize_surface(final_w, final_h);
            }
            if let Some(mut settings) = world.get_resource_mut::<WindowSettings>() {
                settings.width  = final_w;
                settings.height = final_h;
            }
        }
    });

    log::info!("lunar engine shutting down...");
}
