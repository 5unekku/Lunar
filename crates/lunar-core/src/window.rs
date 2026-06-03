//! window state resource and standard resolution list for game code.
//!
//! the engine handles all window lifecycle internally (SDL3, wgpu surface).
//! game code reads [`WindowSettings`] to get the current window dimensions and
//! fullscreen state. to toggle fullscreen, write `is_fullscreen = true`.
//! the engine picks this up and applies it before the next frame.

use bevy_ecs::prelude::*;

/// a standard display resolution (width × height in pixels).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DisplayResolution {
	pub width: u32,
	pub height: u32,
}

impl DisplayResolution {
	#[must_use]
	pub const fn new(width: u32, height: u32) -> Self {
		Self { width, height }
	}

	/// aspect ratio as a float (width / height).
	#[must_use]
	pub fn aspect(self) -> f32 {
		self.width as f32 / self.height as f32
	}
}

/// merged VESA DMT + CTA-861 resolution list, sorted by width then height.
/// use `resolutions_for_aspect` to filter to a specific ratio for a settings menu.
pub static STANDARD_RESOLUTIONS: &[DisplayResolution] = &[
	DisplayResolution::new(640, 480),
	DisplayResolution::new(800, 600),
	DisplayResolution::new(1024, 768),
	DisplayResolution::new(1152, 864),
	DisplayResolution::new(1280, 720),
	DisplayResolution::new(1280, 800),
	DisplayResolution::new(1280, 960),
	DisplayResolution::new(1366, 768),
	DisplayResolution::new(1400, 1050),
	DisplayResolution::new(1440, 900),
	DisplayResolution::new(1600, 900),
	DisplayResolution::new(1600, 1200),
	DisplayResolution::new(1680, 1050),
	DisplayResolution::new(1920, 1080),
	DisplayResolution::new(1920, 1200),
	DisplayResolution::new(2560, 1080),
	DisplayResolution::new(2560, 1440),
	DisplayResolution::new(2560, 1600),
	DisplayResolution::new(3440, 1440),
	DisplayResolution::new(3840, 2160),
	DisplayResolution::new(5120, 2160),
	DisplayResolution::new(5120, 2880),
	DisplayResolution::new(7680, 4320),
];

/// the set of display resolutions available on the current hardware.
///
/// inserted by the bootstrap at startup. on native targets the list comes from
/// SDL3 (`SDL_GetFullscreenDisplayModes`), deduplicated and sorted. on WASM it
/// falls back to [`STANDARD_RESOLUTIONS`] since the browser exposes no display
/// mode API. game code reads this for settings menus.
///
/// # example
///
/// ```ignore
/// fn resolution_menu(resolutions: Res<AvailableResolutions>) {
///     for &res in resolutions.iter() {
///         println!("{}×{}", res.width, res.height);
///     }
/// }
/// ```
#[derive(Resource, Default)]
pub struct AvailableResolutions(pub Vec<DisplayResolution>);

impl AvailableResolutions {
	/// iterate over available resolutions.
	pub fn iter(&self) -> impl Iterator<Item = &DisplayResolution> {
		self.0.iter()
	}

	/// filter to resolutions matching `target_aspect` within `tolerance`.
	/// useful when `target_aspect` is set on [`WindowSettings`].
	#[must_use]
	pub fn for_aspect(&self, target_aspect: f32, tolerance: f32) -> Vec<DisplayResolution> {
		self.0
			.iter()
			.filter(|r| (r.aspect() - target_aspect).abs() <= tolerance)
			.copied()
			.collect()
	}
}

/// returns standard resolutions matching `target_aspect` within `tolerance`.
///
/// for a game locked to 16:9:
/// ```ignore
/// let options = resolutions_for_aspect(16.0 / 9.0, 0.02);
/// // → [1280×720, 1366×768, 1600×900, 1920×1080, 2560×1440, …]
/// ```
#[must_use]
pub fn resolutions_for_aspect(target_aspect: f32, tolerance: f32) -> Vec<DisplayResolution> {
	STANDARD_RESOLUTIONS
		.iter()
		.filter(|r| (r.aspect() - target_aspect).abs() <= tolerance)
		.copied()
		.collect()
}

/// read-only window state resource exposed to game code.
///
/// the engine handles all window lifecycle internally (SDL3, wgpu surface).
/// game code reads this resource to get the current window dimensions and
/// fullscreen state. to toggle fullscreen, write `is_fullscreen = true`
/// (or use Alt+Enter / F11 — both active by default).
///
/// # example
///
/// ```ignore
/// fn my_system(settings: Res<WindowSettings>) {
///     if settings.is_fullscreen {
///         // fullscreen mode
///     }
///     let aspect = settings.width as f32 / settings.height as f32;
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Resource)]
pub struct WindowSettings {
	/// current window width in pixels (reflects the active render area after aspect snap)
	pub width: u32,
	/// current window height in pixels
	pub height: u32,
	/// whether the window is in fullscreen mode
	pub is_fullscreen: bool,
	/// vsync enabled
	pub vsync: bool,
	/// whether the cursor is locked (relative mouse mode, hidden).
	/// set this to true in a setup system to capture the cursor for fps-style input.
	/// the bootstrap loop applies it via SDL3 before the next frame.
	pub cursor_locked: bool,
	/// fixed aspect ratio for window resizing. expressed as width/height (e.g. `16.0/9.0`).
	/// when set, the engine snaps the window height on resize to maintain this ratio.
	/// has no effect in fullscreen. `None` = free aspect ratio.
	pub target_aspect: Option<f32>,
	/// whether the user can resize the window. reflected to the SDL3 window each frame.
	pub allow_resize: bool,
}

impl WindowSettings {
	#[must_use]
	pub const fn new(width: u32, height: u32, vsync: bool) -> Self {
		Self {
			width,
			height,
			is_fullscreen: false,
			vsync,
			cursor_locked: false,
			target_aspect: None,
			allow_resize: true,
		}
	}
}
