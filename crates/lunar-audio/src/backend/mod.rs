//! platform audio backends: sdl3 native, sdl3-emscripten-sidecar on wasm32.
//!
//! both expose the same `PlatformBackend` type alias so the rest of the
//! crate never branches on platform. swap in a custom backend by forking
//! this module and implementing [`AudioBackend`].

use crate::source::AudioSource;

/// submit audio sources to the active backend.
pub trait AudioBackend: Send + 'static {
    /// hand off a source to the mixer; returns immediately (non-blocking).
    fn submit(&self, source: Box<dyn AudioSource>);

    /// per-frame upkeep. native: no-op, the OS audio thread already pulls on
    /// demand. wasm32: tops up the sidecar's jitter buffer.
    fn pump(&mut self) {}
}

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod sidecar;

/// the backend selected for the current target.
#[cfg(not(target_arch = "wasm32"))]
pub use native::Sdl3Backend as PlatformBackend;
#[cfg(target_arch = "wasm32")]
pub use sidecar::SidecarBackend as PlatformBackend;

/// initialise the platform backend.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn init() -> Result<PlatformBackend, String> {
    native::Sdl3Backend::new().map_err(|e| e.to_string())
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn init() -> Result<PlatformBackend, String> {
    sidecar::SidecarBackend::new()
}
