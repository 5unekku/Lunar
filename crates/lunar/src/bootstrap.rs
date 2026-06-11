/// wgpu instance with engine-tuned flags.
///
/// release builds on non-windows targets drop `VALIDATION_INDIRECT_CALL`: it
/// reroutes every `draw_indirect` through a validation compute pass and adds
/// bind groups to every INDIRECT-usage buffer, but the engine's indirect
/// commands come from its own gpu-cull shaders, never from untrusted data.
/// dx12 (windows) keeps it — wgpu relies on that pass for correct
/// `instance_index` / `vertex_index` builtins in indirect draws there.
/// `WGPU_VALIDATION_INDIRECT_CALL=1` still re-enables it for debugging.
pub(crate) fn engine_wgpu_instance() -> wgpu::Instance {
	#[allow(unused_mut)] // mutated only in non-windows release builds
	let mut flags = wgpu::InstanceFlags::default();
	#[cfg(all(not(debug_assertions), not(target_os = "windows")))]
	{
		flags -= wgpu::InstanceFlags::VALIDATION_INDIRECT_CALL;
	}
	wgpu::Instance::new(wgpu::InstanceDescriptor {
		flags: flags.with_env(),
		..wgpu::InstanceDescriptor::new_without_display_handle()
	})
}

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
	use crate::WindowHost;
	use lunar_assets::AssetPlugin;
	use lunar_core::{
		App, AvailableResolutions, DisplayResolution, STANDARD_RESOLUTIONS, WindowSettings,
	};
	use lunar_input::{
		ActionMap, InputBinding, InputPlugin, KeyCode, SdlGamepadProvider, process_events,
	};
	use lunar_render::{RenderEngine, RenderPlugin};
	use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

	env_logger::init();
	log::info!("lunar engine starting...");

	let sdl = sdl3::init().expect("failed to initialize SDL3");
	let video = sdl.video().expect("failed to get video subsystem");

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
		let mut b = video.window(&config.title, config.width, config.height);
		if config.allow_resize {
			b.resizable();
		}
		b.build().expect("failed to create window")
	};

	let instance = engine_wgpu_instance();
	let surface = unsafe {
		let display_handle = window.display_handle().unwrap();
		let window_handle = window.window_handle().unwrap();
		// SAFETY: the handles point into `window`'s internal state and must stay
		// valid for the whole lifetime of the surface. `window` is moved into
		// `host` below; the explicit `drop(app)` after the loop tears the surface
		// down (it lives in the `RenderEngine` app resource) while `host` — and
		// thus `window` — is still alive, so the window outlives the surface.
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
	initial_settings.allow_resize = config.allow_resize;
	app.insert_resource(initial_settings);
	app.insert_resource(AvailableResolutions(available_resolutions));
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
	let mut host = WindowHost::new(window, sdl.mouse(), config.width, config.height);

	app.run_with_events(config.loop_config(), |world| {
		process_events(&mut event_pump, &mut sdl_gamepad, world);
		host.sync(world, |world, w, h| {
			if let Some(mut re) = world.get_resource_mut::<RenderEngine>() {
				re.resize(w, h);
			}
		});
	});

	// tear the surface down (lives in the RenderEngine app resource) before
	// `host`/`window` drop at scope exit — wgpu requires the window to outlive
	// the surface created from its handles.
	drop(app);

	log::info!("lunar engine shutting down...");
}
