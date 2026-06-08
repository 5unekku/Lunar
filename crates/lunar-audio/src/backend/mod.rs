//! platform audio backends — cubeb on native, cpal/webaudio on wasm32.
//!
//! both expose the same `PlatformBackend` type alias so the rest of the
//! crate never branches on platform. swap in a custom backend by forking
//! this module and implementing [`AudioBackend`].

use crate::source::AudioSource;

/// submit audio sources to the active backend.
pub trait AudioBackend: Send + 'static {
    /// hand off a source to the mixer; returns immediately (non-blocking).
    fn submit(&self, source: Box<dyn AudioSource>);
}

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod web;

/// the backend selected for the current target.
#[cfg(not(target_arch = "wasm32"))]
pub use native::CubebBackend as PlatformBackend;
#[cfg(target_arch = "wasm32")]
pub use web::CpalBackend as PlatformBackend;

/// initialise the platform backend.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn init() -> Result<PlatformBackend, String> {
    native::CubebBackend::new().map_err(|e| e.to_string())
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn init() -> Result<PlatformBackend, String> {
    web::CpalBackend::new()
}
