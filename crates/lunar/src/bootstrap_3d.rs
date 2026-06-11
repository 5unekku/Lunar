/// bootstrap a native 3d lunar game.
///
/// initializes SDL3, creates a window, sets up the wgpu surface, adds all
/// built-in plugins, and runs the game loop. use [`lunar_core::WindowSettings`]
/// to read window state and toggle fullscreen or cursor lock from game code.
///
/// # fullscreen
///
/// F11 or Alt+Enter toggle fullscreen by default. game code can rebind F11 via
/// [`lunar_input::ActionMap`] or disable it entirely. Alt+Enter is always active
/// and cannot be rebound (it is an engine-level shortcut).
///
/// to enter fullscreen programmatically: `settings.is_fullscreen = true`.
///
/// # aspect ratio
///
/// set `config.target_aspect` (e.g. `Some(16.0 / 9.0)`) to lock the window to a
/// fixed aspect ratio. on resize the engine snaps the height to the nearest
/// correct value. has no effect in fullscreen mode.
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
	use crate::WindowHost;
	use lunar_3d::Plugin3d;
	use lunar_assets::AssetPlugin;
	use lunar_core::{
		App, AvailableResolutions, DisplayResolution, STANDARD_RESOLUTIONS, WindowSettings,
	};
	use lunar_input::{
		ActionMap, InputBinding, InputPlugin, KeyCode, SdlGamepadProvider, process_events,
	};
	use lunar_render_3d::{RenderEngine3d, RenderPlugin3d};
	use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

	env_logger::init();
	log::info!("lunar 3d engine starting...");

	#[cfg(debug_assertions)]
	let smoke = std::env::args().any(|a| a == "--smoke");
	#[cfg(not(debug_assertions))]
	let smoke = false;

	let (win_w, win_h) = if smoke {
		(1, 1)
	} else {
		(config.width, config.height)
	};

	let sdl = sdl3::init().expect("failed to initialize SDL3");
	let video = sdl.video().expect("failed to get video subsystem");
	let mouse = sdl.mouse();

	// query the primary display for available fullscreen modes, deduplicated by (w, h)
	let available_resolutions = {
		use std::collections::BTreeSet;
		let modes = video
			.displays()
			.ok()
			.and_then(|displays| displays.into_iter().next())
			.and_then(|display| display.get_fullscreen_modes().ok())
			.unwrap_or_default();
		let unique: BTreeSet<(u32, u32)> = modes
			.iter()
			.filter(|m| m.w > 0 && m.h > 0)
			.map(|m| (m.w as u32, m.h as u32))
			.collect();
		if unique.is_empty() {
			STANDARD_RESOLUTIONS.to_vec()
		} else {
			unique
				.into_iter()
				.map(|(w, h)| DisplayResolution::new(w, h))
				.collect()
		}
	};

	let window = {
		let mut b = video.window(&config.title, win_w, win_h);
		if config.allow_resize {
			b.resizable();
		}
		b.build().expect("failed to create window")
	};

	let instance = crate::bootstrap_impl::engine_wgpu_instance();
	let surface = unsafe {
		let display_handle = window.display_handle().unwrap();
		let window_handle = window.window_handle().unwrap();
		// SAFETY: the display and window handles point into `window`'s internal
		// state and must stay valid for the whole lifetime of the surface.
		// `window` is moved into `host` below; the explicit `drop(app)` after the
		// loop tears the surface down (it lives in the `RenderEngine3d` app
		// resource) while `host` — and thus `window` — is still alive, so the
		// window outlives the surface, satisfying `create_surface_unsafe`.
		instance
			.create_surface_unsafe(
				wgpu::SurfaceTargetUnsafe::from_display_and_window(&display_handle, &window_handle)
					.unwrap(),
			)
			.expect("failed to create wgpu surface")
	};

	let render_engine = RenderEngine3d::from_surface(&instance, surface, &config);

	let mut app = App::new();

	let mut initial_settings = WindowSettings::new(win_w, win_h, config.vsync);
	initial_settings.target_aspect = config.target_aspect;
	initial_settings.allow_resize = config.allow_resize;
	app.insert_resource(initial_settings);
	app.insert_resource(AvailableResolutions(available_resolutions));
	app.insert_resource(render_engine);

	app.add_plugin(Plugin3d);
	app.add_plugin(RenderPlugin3d);
	app.add_plugin(InputPlugin);
	app.add_plugin(AssetPlugin);

	// F11 toggles fullscreen (rebindable). alt+enter is handled directly in the loop below.
	app.add_startup_system(|mut actions: bevy_ecs::prelude::ResMut<ActionMap>| {
		actions.bind("fullscreen", InputBinding::Key(KeyCode::F11));
	});

	app.add_plugin(Plugin::default());

	// stop after the first rendered frame — only active when --smoke is passed in debug builds
	#[cfg(debug_assertions)]
	if smoke {
		use lunar_core::{EngineState, Time};
		app.add_system(
			|time: bevy_ecs::prelude::Res<Time>,
			 mut state: bevy_ecs::prelude::ResMut<EngineState>| {
				if time.frame_count() >= 1 {
					*state = EngineState::Stopping;
				}
			},
		);
	}

	let gamepad_subsystem = sdl.gamepad().expect("failed to get gamepad subsystem");
	let mut event_pump = sdl.event_pump().expect("failed to get event pump");
	let mut sdl_gamepad = SdlGamepadProvider::new(gamepad_subsystem);
	let mut host = WindowHost::new(window, mouse, win_w, win_h);

	app.run_with_events(config.loop_config(), |world| {
		process_events(&mut event_pump, &mut sdl_gamepad, world);
		host.sync(world, |world, w, h| {
			if let Some(mut re) = world.get_resource_mut::<RenderEngine3d>() {
				re.resize(w, h);
			}
		});
	});

	// tear the surface down (lives in the RenderEngine3d app resource) before
	// `host`/`window` drop at scope exit — wgpu requires the window to outlive
	// the surface created from its handles.
	drop(app);

	log::info!("lunar 3d engine shutting down...");
}
