//! sdl3 backend for native targets (WASAPI / CoreAudio / PulseAudio / ALSA via SDL3).
//!
//! the sdl3 stream runs on a dedicated OS audio thread; game code submits
//! sources via a crossbeam channel and the callback drains it lock-free.

use crate::mixer::Mixer;
use crate::source::{AudioSource, SAMPLE_RATE};
use super::AudioBackend;
use crossbeam_channel::{Sender, unbounded};
use sdl3::audio::{AudioCallback, AudioFormat, AudioSpec, AudioStream, AudioStreamWithCallback};

struct MixerCallback {
    mixer: Mixer,
    scratch: Vec<f32>, // resized lazily, mirrors the old cubeb `flat` buffer
}

impl AudioCallback<f32> for MixerCallback {
    fn callback(&mut self, stream: &mut AudioStream, requested: i32) {
        // requested is total interleaved samples (not frames). mixer.fill()
        // already takes a flat &mut [f32], so this drops the old cubeb
        // StereoFrame conversion step entirely.
        let needed = requested.max(0) as usize;
        self.scratch.resize(needed, 0.0);
        self.mixer.fill(&mut self.scratch);
        stream.put_data_f32(&self.scratch).ok();
    }
}

// wraps just the returned stream: sdl3-rs's AudioStreamOwner already holds its
// own AudioSubsystem internally, so the stream alone transitively keeps
// AudioSubsystem (and the underlying SDL_Init) alive. no need to separately
// store Sdl/AudioSubsystem.
struct Sdl3StreamHandle(#[allow(dead_code)] AudioStreamWithCallback<MixerCallback>);
// SAFETY: SDL3 runs the audio callback on its own dedicated OS thread and
// synchronises all access to the stream handle internally. moving the handle
// across Rust threads is safe as long as we never call its methods
// concurrently, which we don't: it's permanently idle after construction,
// aside from the one-time resume() call in `new`.
unsafe impl Send for Sdl3StreamHandle {}
unsafe impl Sync for Sdl3StreamHandle {}

pub struct Sdl3Backend {
    sender: Sender<Box<dyn AudioSource>>,
    _stream: Sdl3StreamHandle,
}

impl Sdl3Backend {
    pub fn new() -> Result<Self, sdl3::Error> {
        // independent sdl3::init() rather than sharing bootstrap.rs's Sdl handle:
        // Sdl::new() only hard-errors on a second call from a *different* thread,
        // and AudioPlugin::build() always runs on the same thread that ran the
        // original init() in bootstrap(), so this just bumps SDL's refcounts.
        let sdl = sdl3::init()?;
        let audio_subsystem = sdl.audio()?;

        let spec = AudioSpec {
            freq: Some(SAMPLE_RATE as i32),
            channels: Some(2),
            format: Some(AudioFormat::F32LE),
        };

        let (sender, receiver) = unbounded::<Box<dyn AudioSource>>();
        let mixer = Mixer::new(receiver);

        let stream = audio_subsystem
            .open_playback_stream(&spec, MixerCallback { mixer, scratch: Vec::new() })?;
        // the device begins paused: skipping this means silent, errorless no sound.
        stream.resume()?;

        Ok(Self { sender, _stream: Sdl3StreamHandle(stream) })
    }
}

impl AudioBackend for Sdl3Backend {
    fn submit(&self, source: Box<dyn AudioSource>) {
        // ignore send errors, stream may have closed during shutdown
        let _ = self.sender.send(source);
    }
}
