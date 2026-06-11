//! mixes multiple [`AudioSource`] streams into a single interleaved stereo f32 buffer.

use crate::source::AudioSource;
use crossbeam_channel::Receiver;

/// mixes active sources and fills output buffers on the audio callback thread.
///
/// the mixer lives entirely inside the backend callback — game code submits
/// new sources via a channel and the mixer drains it each callback.
pub struct Mixer {
    sources: Vec<Box<dyn AudioSource>>,
    /// pre-allocated per-source scratch buffer; resized lazily
    scratch: Vec<f32>,
    receiver: Receiver<Box<dyn AudioSource>>,
}

impl Mixer {
    pub fn new(receiver: Receiver<Box<dyn AudioSource>>) -> Self {
        Self { sources: Vec::new(), scratch: Vec::new(), receiver }
    }

    /// drain any pending sources from the submission channel, then mix all active
    /// sources into `output` (interleaved stereo f32; zeroed here, callers don't
    /// need to clear it).
    pub fn fill(&mut self, output: &mut [f32]) {
        while let Ok(source) = self.receiver.try_recv() {
            self.sources.push(source);
        }

        output.fill(0.0);
        self.scratch.resize(output.len(), 0.0);

        // field-split borrow so the borrow checker sees sources and scratch as disjoint
        let (sources, scratch) = (&mut self.sources, &mut self.scratch);

        let mut i = 0;
        while i < sources.len() {
            // sources fully write the prefix they report, so only that prefix is
            // mixed and scratch never needs re-zeroing
            let frames = sources[i].fill(&mut scratch[..output.len()]);
            let written = (frames * 2).min(output.len());

            for (out, sample) in output[..written].iter_mut().zip(&scratch[..written]) {
                *out += sample;
            }

            if sources[i].is_done() {
                sources.swap_remove(i);
            } else {
                i += 1;
            }
        }

        // soft clamp — avoids hard distortion on loud overlapping sources
        for sample in output.iter_mut() {
            *sample = sample.clamp(-1.0, 1.0);
        }
    }
}
