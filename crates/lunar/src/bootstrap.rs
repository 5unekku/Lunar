/// bootstrap a native lunar game.
///
/// initializes SDL3, creates a window, sets up the wgpu render surface,
/// adds all built-in plugins including default fullscreen bindings (F11/F),
/// and runs the game loop. the window title defaults to `"Lunar"`.
///
/// game code never touches SDL3, wgpu, or unsafe — read window state
/// through [`crate::WindowSettings`].
///
/// # example
///
/// ```ignore
/// use lunar::prelude::*;
///
/// struct MyGame;
/// impl GamePlugin for MyGame {
///     fn name(&self) -> &str { "MyGame" }
/// }
///
/// fn main() {
///     lunar::bootstrap::<MyGame>(Default::default());
/// }
/// ```
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

    let window = video
        .window("Lunar", config.width, config.height)
        .resizable()
        .build()
        .expect("failed to create window");

    let instance = wgpu::Instance::default();
    let surface = unsafe {
        let display_handle = window.display_handle().unwrap();
        let window_handle = window.window_handle().unwrap();
        // SAFETY: the SDL3 window is owned by this function's stack frame and
        // outlives the wgpu surface (the surface is dropped when this function
        // returns or panics, before `window` is dropped). The display and window
        // handles point into `window`'s internal state and remain valid for
        // that lifetime, satisfying wgpu's `create_surface_unsafe` contract.
        instance
            .create_surface_unsafe(
                wgpu::SurfaceTargetUnsafe::from_display_and_window(&display_handle, &window_handle)
                    .unwrap(),
            )
            .expect("failed to create wgpu surface")
    };

    let render_engine = RenderEngine::from_surface(&instance, surface, config.clone());

    let mut app = App::new();

    app.insert_resource(WindowSettings::new(
        config.width,
        config.height,
        config.vsync,
    ));
    app.insert_resource(render_engine);

    app.add_plugin(RenderPlugin);
    app.add_plugin(InputPlugin);
    app.add_plugin(AssetPlugin);
    // audio plugin slot — wire up here when the audio crate is ready

    // register default fullscreen toggle bindings (F11 or F)
    app.add_startup_system(|mut actions: bevy_ecs::prelude::ResMut<ActionMap>| {
        actions.bind("fullscreen", InputBinding::Key(KeyCode::F11));
        actions.bind("fullscreen", InputBinding::Key(KeyCode::F));
    });

    app.add_plugin(Plugin::default());

    let gamepad_subsystem = sdl.gamepad().expect("failed to get gamepad subsystem");
    let mut event_pump = sdl.event_pump().expect("failed to get event pump");
    let mut sdl_gamepad = SdlGamepadProvider::new(gamepad_subsystem);
    let mut window = window;
    let mut actual_fullscreen = false;
    let mut last_window_w = config.width;
    let mut last_window_h = config.height;

    app.run_with_events(config.frame_cap, |world| {
        process_events(&mut event_pump, &mut sdl_gamepad, world);

        // handle fullscreen toggle via action map
        if let Some(actions) = world.get_resource::<ActionMap>()
            && let Some(input) = world.get_resource::<InputState>()
            && actions.is_action_just_pressed(input, "fullscreen")
        {
            actual_fullscreen = !actual_fullscreen;
            let _ = window.set_fullscreen(actual_fullscreen);
            if let Some(mut settings) = world.get_resource_mut::<WindowSettings>() {
                settings.is_fullscreen = actual_fullscreen;
            }
        }

        // check if game code set is_fullscreen directly
        if let Some(settings) = world.get_resource::<WindowSettings>()
            && settings.is_fullscreen != actual_fullscreen
        {
            let new_fs = settings.is_fullscreen;
            let _ = window.set_fullscreen(new_fs);
            actual_fullscreen = new_fs;
        }

        // handle window resize by polling actual window size
        let (w, h) = window.size();
        if w != last_window_w || h != last_window_h {
            last_window_w = w;
            last_window_h = h;
            if let Some(mut re) = world.get_resource_mut::<RenderEngine>() {
                re.resize_surface(w, h);
            }
            if let Some(mut settings) = world.get_resource_mut::<WindowSettings>() {
                settings.width = w;
                settings.height = h;
            }
        }
    });

    log::info!("lunar engine shutting down...");
}
