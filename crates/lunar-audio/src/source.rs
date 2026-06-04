//! audio source trait — the extension seam for custom platforms and generators.
//!
//! a custom tracker engine, procedural synth, or fork-specific audio system
//! implements [`AudioSource`] and submits instances via [`crate::AudioPlayer::play_source`].
//! the built-in engine only ever calls [`fill`] and [`is_done`] — everything else
//! is encapsulated in the implementor.

/// produces f32 interleaved stereo samples at [`SAMPLE_RATE`] Hz.
///
/// the audio callback invokes [`fill`] on each active source once per buffer.
/// sources are dropped automatically when [`is_done`] returns `true`.
pub trait AudioSource: Send + Sync + 'static {
    /// write interleaved stereo f32 samples into `output` and return the number
    /// of *frames* (sample pairs) written. returning 0 signals exhaustion.
    fn fill(&mut self, output: &mut [f32]) -> usize;

    /// true once all samples have been emitted and the source should be dropped.
    fn is_done(&self) -> bool;
}

/// sample rate all sources must produce. resampling to device rate is handled
/// by the backend if the device disagrees.
pub const SAMPLE_RATE: u32 = 48_000;
