//! cpal webaudio backend for wasm32 targets (Web Audio API via ScriptProcessorNode).
//!
//! cpal's webaudio host wraps the browser's AudioContext and runs the callback
//! on the main thread under a ScriptProcessorNode. latency is higher than native
//! (~50–100 ms typical) but it's the only safe option without AudioWorklet.

use crate::mixer::Mixer;
use crate::source::{AudioSource, SAMPLE_RATE};
use super::AudioBackend;
use crossbeam_channel::{Sender, unbounded};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

// cpal's webaudio Stream holds JS objects (AudioContext, callbacks) and is !Send.
// SAFETY: wasm32-unknown-unknown without the `atomics` target feature is
// single-threaded — no second thread can exist to observe the stream, so Send is
// vacuously satisfied. revisit if wasm threads are ever enabled for this target.
struct StreamHandle(#[allow(dead_code)] cpal::Stream);
unsafe impl Send for StreamHandle {}

pub struct CpalBackend {
    sender: Sender<Box<dyn AudioSource>>,
    /// stream must stay alive to keep audio running.
    _stream: StreamHandle,
}

impl CpalBackend {
    pub fn new() -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or("no output device available")?;

        let config = cpal::StreamConfig {
            channels: 2,
            sample_rate: cpal::SampleRate(SAMPLE_RATE),
            buffer_size: cpal::BufferSize::Default,
        };

        let (sender, receiver) = unbounded::<Box<dyn AudioSource>>();
        let mut mixer = Mixer::new(receiver);

        let stream = device
            .build_output_stream(
                &config,
                move |output: &mut [f32], _info| {
                    mixer.fill(output);
                },
                |error| log::error!("cpal stream error: {error}"),
                None,
            )
            .map_err(|e| format!("build_output_stream failed: {e}"))?;

        stream.play().map_err(|e| format!("stream play failed: {e}"))?;

        Ok(Self { sender, _stream: StreamHandle(stream) })
    }
}

impl AudioBackend for CpalBackend {
    fn submit(&self, source: Box<dyn AudioSource>) {
        let _ = self.sender.send(source);
    }
}
