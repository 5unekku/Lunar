/// bootstrap a native 3d lunar game.
///
/// initializes SDL3, creates a window, sets up the wgpu surface, adds all
/// built-in plugins, and runs the game loop. use [`lunar_core::WindowSettings`]
/// to read window state and toggle fullscreen or cursor lock from game code.
///
/// # cursor lock
///
/// set `WindowSettings::cursor_locked = true` in a setup system to capture
/// the cursor (relative mouse mode). mouse delta is then available via
/// [`lunar_input::InputState::mouse_delta`].
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
///     lunar::bootstrap_3d::<MyGame>(Default::default());
/// }
/// ```
#[cfg(not(target_arch = "wasm32"))]
pub fn bootstrap_3d<Plugin: lunar_core::GamePlugin + Default + 'static>(
    config: lunar_render_3d::RenderConfig3d,
) {
    use lunar_3d::Plugin3d;
    use lunar_assets::AssetPlugin;
    use lunar_core::{App, WindowSettings};
    use lunar_input::{ActionMap, InputBinding, InputPlugin, InputState, KeyCode, SdlGamepadProvider, process_events};
    use lunar_render_3d::{RenderEngine3d, RenderPlugin3d};
    use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

    env_logger::init();
    log::info!("lunar 3d engine starting...");

    let sdl = sdl3::init().expect("failed to initialize SDL3");
    let video = sdl.video().expect("failed to get video subsystem");
    let mouse = sdl.mouse();

    let window = video
        .window(&config.title, config.width, config.height)
        .resizable()
        .build()
        .expect("failed to create window");

    let instance = wgpu::Instance::default();
    let surface = unsafe {
        let display_handle = window.display_handle().unwrap();
        let window_handle = window.window_handle().unwrap();
        // SAFETY: the SDL3 window is owned by this function's stack frame and
        // outlives the wgpu surface (the surface is dropped when this function
        // returns or panics, before `window` is dropped). the display and window
        // handles point into `window`'s internal state and remain valid for
        // that lifetime, satisfying wgpu's `create_surface_unsafe` contract.
        instance
            .create_surface_unsafe(
                wgpu::SurfaceTargetUnsafe::from_display_and_window(&display_handle, &window_handle)
                    .unwrap(),
            )
            .expect("failed to create wgpu surface")
    };

    let render_engine = RenderEngine3d::from_surface(&instance, surface, &config);

    let mut app = App::new();

    app.insert_resource(WindowSettings::new(config.width, config.height, config.vsync));
    app.insert_resource(render_engine);

    app.add_plugin(Plugin3d);
    app.add_plugin(RenderPlugin3d);
    app.add_plugin(InputPlugin);
    app.add_plugin(AssetPlugin);

    // default fullscreen toggle bindings
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
    let mut actual_cursor_locked = false;
    let mut last_window_w = config.width;
    let mut last_window_h = config.height;

    app.run_with_events(config.frame_cap, |world| {
        process_events(&mut event_pump, &mut sdl_gamepad, world);

        // fullscreen toggle via action map
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

        // fullscreen set directly via WindowSettings
        if let Some(settings) = world.get_resource::<WindowSettings>()
            && settings.is_fullscreen != actual_fullscreen
        {
            actual_fullscreen = settings.is_fullscreen;
            let _ = window.set_fullscreen(actual_fullscreen);
        }

        // cursor lock set via WindowSettings
        if let Some(settings) = world.get_resource::<WindowSettings>()
            && settings.cursor_locked != actual_cursor_locked
        {
            actual_cursor_locked = settings.cursor_locked;
            mouse.set_relative_mouse_mode(&window, actual_cursor_locked);
        }

        // window resize
        let (w, h) = window.size();
        if w != last_window_w || h != last_window_h {
            last_window_w = w;
            last_window_h = h;
            if let Some(mut re) = world.get_resource_mut::<RenderEngine3d>() {
                re.resize(w, h);
            }
            if let Some(mut settings) = world.get_resource_mut::<WindowSettings>() {
                settings.width = w;
                settings.height = h;
            }
        }
    });

    log::info!("lunar 3d engine shutting down...");
}
