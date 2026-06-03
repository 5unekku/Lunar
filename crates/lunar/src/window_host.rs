//! shared per-frame window reconciliation for native game loops.

use bevy_ecs::prelude::World;
use lunar_core::WindowSettings;
use lunar_input::{ActionMap, InputState, KeyCode};

/// drives fullscreen (F11 / Alt+Enter), cursor lock, aspect-ratio snapping, and
/// surface resize in lockstep with [`WindowSettings`], for native game loops.
///
/// both [`bootstrap`](crate::bootstrap) and `bootstrap_3d` use this — and so can a
/// custom loop: own the SDL3 window plus a `WindowHost`, call [`WindowHost::sync`]
/// once per frame after pumping input, and forward the `resize` callback to your
/// render engine. It is render-engine-agnostic (2D or 3D) by design.
///
/// fullscreen shortcuts: F11 (rebindable via the `"fullscreen"` action on
/// [`ActionMap`]) and Alt+Enter (always active). Both are edge-triggered with a
/// key-release re-arm, so a held key can't double-toggle regardless of frame rate.
pub struct WindowHost {
	window: sdl3::video::Window,
	mouse: sdl3::mouse::MouseUtil,
	fullscreen: bool,
	// true while a toggle key is physically held; re-armed only after full release.
	fullscreen_key_down: bool,
	cursor_locked: bool,
	last_w: u32,
	last_h: u32,
}

impl WindowHost {
	/// create a host for the given window. `width`/`height` seed the last-known
	/// size used for resize detection.
	#[must_use]
	pub fn new(
		window: sdl3::video::Window,
		mouse: sdl3::mouse::MouseUtil,
		width: u32,
		height: u32,
	) -> Self {
		Self {
			window,
			mouse,
			fullscreen: false,
			fullscreen_key_down: false,
			cursor_locked: false,
			last_w: width,
			last_h: height,
		}
	}

	/// the underlying SDL3 window, for custom-loop needs (title, icon, etc.).
	pub const fn window_mut(&mut self) -> &mut sdl3::video::Window {
		&mut self.window
	}

	/// reconcile window state with [`WindowSettings`] + input for one frame.
	///
	/// call once per frame after pumping events. `resize` is invoked with the
	/// final `(width, height)` whenever the surface must resize, so the caller can
	/// forward to whichever render engine it owns (`re.resize(w, h)`).
	pub fn sync(&mut self, world: &mut World, mut resize: impl FnMut(&mut World, u32, u32)) {
		// read input once for the frame — capture both the edge (just pressed) and
		// whether any toggle combo is still physically held, for the keyup re-arm.
		let input_snap = world.get_resource::<InputState>().map(|i| {
			let enter_just = i.is_key_just_pressed(KeyCode::Enter);
			let alt = i.is_key_held(KeyCode::LAlt) || i.is_key_held(KeyCode::RAlt);
			let enter_held = i.is_key_held(KeyCode::Enter);
			let fs_just = world
				.get_resource::<ActionMap>()
				.is_some_and(|a| a.is_action_just_pressed(i, "fullscreen"));
			let fs_held = world
				.get_resource::<ActionMap>()
				.is_some_and(|a| a.is_action_held(i, "fullscreen"));
			let any_held = (alt && enter_held) || fs_held;
			(enter_just && alt, fs_just, any_held)
		});

		// re-arm once all toggle keys are physically released
		if !input_snap.is_some_and(|(_, _, held)| held) {
			self.fullscreen_key_down = false;
		}

		// fire only on the keydown edge — gate blocks until full release
		let request_toggle = !self.fullscreen_key_down
			&& (input_snap.is_some_and(|(alt_enter, _, _)| alt_enter)
				|| input_snap.is_some_and(|(_, fs, _)| fs));

		// alt+enter / f11: engine-level fullscreen toggle
		if request_toggle {
			self.fullscreen_key_down = true;
			self.fullscreen = !self.fullscreen;
			let _ = self.window.set_fullscreen(self.fullscreen);
			if let Some(mut settings) = world.get_resource_mut::<WindowSettings>() {
				settings.is_fullscreen = self.fullscreen;
			}
		}

		// game code set is_fullscreen directly (e.g. settings menu) — optimistic update
		if let Some(settings) = world.get_resource::<WindowSettings>()
			&& settings.is_fullscreen != self.fullscreen
		{
			self.fullscreen = settings.is_fullscreen;
			let _ = self.window.set_fullscreen(self.fullscreen);
		}

		// cursor lock (relative mouse mode) — for fps-style mouse-look
		if let Some(settings) = world.get_resource::<WindowSettings>()
			&& settings.cursor_locked != self.cursor_locked
		{
			self.cursor_locked = settings.cursor_locked;
			self.mouse
				.set_relative_mouse_mode(&self.window, self.cursor_locked);
		}

		// window resize — enforce aspect ratio in windowed mode, then notify renderer
		let (w, h) = self.window.size();
		if w != self.last_w || h != self.last_h {
			let target = world.get_resource::<WindowSettings>().and_then(|s| {
				if !self.fullscreen {
					s.target_aspect
				} else {
					None
				}
			});

			let (final_w, final_h) = if let Some(aspect) = target {
				let snapped_h = ((w as f32 / aspect).round() as u32).max(1);
				if snapped_h != h {
					let _ = self.window.set_size(w, snapped_h);
				}
				(w, snapped_h)
			} else {
				(w, h)
			};

			self.last_w = final_w;
			self.last_h = final_h;
			resize(world, final_w, final_h);
			if let Some(mut settings) = world.get_resource_mut::<WindowSettings>() {
				settings.width = final_w;
				settings.height = final_h;
			}
		}
	}
}
