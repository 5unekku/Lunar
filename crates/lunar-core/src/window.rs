//! window state resource for game code.
//!
//! the engine handles all window lifecycle internally (SDL3, wgpu surface).
//! game code reads [`WindowSettings`] to get the current window dimensions and
//! fullscreen state. to toggle fullscreen, write `is_fullscreen = true`.
//! the engine picks this up and applies it before the next frame.

use bevy_ecs::prelude::*;

/// read-only window state resource exposed to game code.
///
/// the engine handles all window lifecycle internally (SDL3, wgpu surface).
/// game code reads this resource to get the current window dimensions and
/// fullscreen state. to toggle fullscreen, write `is_fullscreen = true`
/// (or use the default F11/F key binding via ActionMap).
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
    /// current window width in pixels
    pub width: u32,
    /// current window height in pixels
    pub height: u32,
    /// whether the window is in fullscreen mode
    pub is_fullscreen: bool,
    /// vsync enabled
    pub vsync: bool,
}

impl WindowSettings {
    #[must_use]
    pub const fn new(width: u32, height: u32, vsync: bool) -> Self {
        Self {
            width,
            height,
            is_fullscreen: false,
            vsync,
        }
    }
}
