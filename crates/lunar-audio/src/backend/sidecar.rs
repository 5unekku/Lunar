//! sdl3-emscripten-sidecar backend for wasm32 targets: pushes batched
//! interleaved f32 PCM into `audio_sidecar.wasm`'s SDL3 audio stream once per
//! frame.
//!
//! no separate audio thread on wasm32: `pump()` runs on the main ECS thread
//! each frame, topping up the sidecar's queue to a target jitter-buffer depth.

use crate::mixer::Mixer;
use crate::source::{AudioSource, SAMPLE_RATE};
use super::AudioBackend;
use crossbeam_channel::{Sender, unbounded};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(inline_js = r#"
export function audio_init(freq, channels) {
    return window.__audioSidecar._audio_init(freq, channels);
}
export function audio_push(data) {
    if (data.length === 0) return;
    const M = window.__audioSidecar;
    const bytes = data.length * 4;
    const sidecar_ptr = M._malloc(bytes);
    // data is a Float32Array view over the caller's wasm memory (passed by
    // wasm-bindgen). copy its bytes into the sidecar's separate heap. this
    // avoids referencing the main module's `wasm` binding, which is not in
    // scope inside a wasm-bindgen inline_js snippet module.
    new Uint8Array(M.HEAPU8.buffer, sidecar_ptr, bytes)
        .set(new Uint8Array(data.buffer, data.byteOffset, bytes));
    M._audio_push(sidecar_ptr, data.length);
    M._free(sidecar_ptr);
}
export function audio_queued_bytes() {
    return window.__audioSidecar._audio_queued_bytes();
}
export function audio_shutdown() {
    window.__audioSidecar._audio_shutdown();
}
"#)]
extern "C" {
    fn audio_init(freq: i32, channels: i32) -> i32;
    fn audio_push(data: &[f32]);
    fn audio_queued_bytes() -> u32;
    fn audio_shutdown();
}

/// wasm32 sidecar backend, calls the `audio_sidecar.wasm` module through JS bridge imports.
pub struct SidecarBackend {
    sender: Sender<Box<dyn AudioSource>>,
    mixer: Mixer,
    scratch: Vec<f32>,
    /// jitter buffer depth in bytes; defaults to ~100ms
    /// (SAMPLE_RATE * channels * 4 bytes/sample * 0.1)
    target_queued_bytes: u32,
}

impl SidecarBackend {
    /// create the sidecar backend. `window.__audioSidecar` must already be
    /// initialized (see `scripts/run_wasm.go`).
    pub fn new() -> Result<Self, String> {
        if audio_init(SAMPLE_RATE as i32, 2) == 0 {
            return Err("audio_sidecar: audio_init failed".to_string());
        }

        let (sender, receiver) = unbounded::<Box<dyn AudioSource>>();
        let mixer = Mixer::new(receiver);
        let target_queued_bytes = (SAMPLE_RATE as f32 * 2.0 * 4.0 * 0.1) as u32;

        Ok(Self { sender, mixer, scratch: Vec::new(), target_queued_bytes })
    }
}

impl Drop for SidecarBackend {
    fn drop(&mut self) {
        audio_shutdown();
    }
}

impl AudioBackend for SidecarBackend {
    fn submit(&self, source: Box<dyn AudioSource>) {
        let _ = self.sender.send(source);
    }

    fn pump(&mut self) {
        let queued = audio_queued_bytes();
        if queued >= self.target_queued_bytes { return; }
        // round down to a whole number of stereo frames (2 floats) so a push
        // never splits a frame across buffers
        let want_floats = ((self.target_queued_bytes - queued) as usize / 4) & !1;
        self.scratch.resize(want_floats, 0.0);
        self.mixer.fill(&mut self.scratch);
        audio_push(&self.scratch);
    }
}
