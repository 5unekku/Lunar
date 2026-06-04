//! cubeb backend for native targets (WASAPI / CoreAudio / PulseAudio / ALSA).
//!
//! the cubeb stream runs on a dedicated OS audio thread; game code submits
//! sources via a crossbeam channel and the callback drains it lock-free.

use crate::mixer::Mixer;
use crate::source::{AudioSource, SAMPLE_RATE};
use super::AudioBackend;
use crossbeam_channel::{Sender, unbounded};
use cubeb::{ChannelLayout, SampleFormat, StereoFrame, StreamParamsBuilder};

// libcubeb is internally thread-safe. Context and Stream hold raw C pointers
// that cubeb-rs leaves non-Send to force explicit acknowledgement — we provide it.
struct CubebHandle {
    _context: cubeb::Context,
    _stream: cubeb::Stream<StereoFrame<f32>>,
}
// SAFETY: libcubeb uses a dedicated OS audio thread internally and synchronises
// all access to the context and stream handle itself. moving them across Rust
// threads is safe as long as we never call their methods concurrently, which
// we don't — _handle is permanently idle after construction.
unsafe impl Send for CubebHandle {}
unsafe impl Sync for CubebHandle {}

pub struct CubebBackend {
    sender: Sender<Box<dyn AudioSource>>,
    _handle: CubebHandle,
}

impl CubebBackend {
    pub fn new() -> Result<Self, cubeb::Error> {
        let context = cubeb::init("lunar-audio")?;

        let params = StreamParamsBuilder::new()
            .format(SampleFormat::Float32LE)
            .rate(SAMPLE_RATE)
            .channels(2)
            .layout(ChannelLayout::STEREO)
            .take();

        let (sender, receiver) = unbounded::<Box<dyn AudioSource>>();
        let mut mixer = Mixer::new(receiver);
        // pre-allocate the flat f32 scratch for the callback; resized lazily if cubeb
        // ever changes the buffer size (rare in practice).
        let mut flat: Vec<f32> = Vec::new();

        let mut builder = cubeb::StreamBuilder::<StereoFrame<f32>>::new();
        // 512 frames ≈ 10 ms at 48000 Hz — low latency without underruns
        builder
            .name("lunar")
            .default_output(&params)
            .latency(512)
            .data_callback(move |_input, output: &mut [StereoFrame<f32>]| {
                let needed = output.len() * 2;
                if flat.len() < needed {
                    flat.resize(needed, 0.0);
                }
                mixer.fill(&mut flat[..needed]);
                for (frame, chunk) in output.iter_mut().zip(flat.chunks_exact(2)) {
                    frame.l = chunk[0];
                    frame.r = chunk[1];
                }
                output.len() as isize
            })
            .state_callback(|state| {
                log::debug!("cubeb stream state: {state:?}");
            });
        let stream = builder.init(&context)?;

        stream.start()?;

        Ok(Self {
            sender,
            _handle: CubebHandle { _context: context, _stream: stream },
        })
    }
}

impl AudioBackend for CubebBackend {
    fn submit(&self, source: Box<dyn AudioSource>) {
        // ignore send errors — stream may have closed during shutdown
        let _ = self.sender.send(source);
    }
}
