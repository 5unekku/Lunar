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

#[cfg(test)]
mod tests {
    use super::*;

    /// emits a constant value for a fixed number of stereo frames.
    struct Tone {
        value: f32,
        frames_left: usize,
    }

    impl AudioSource for Tone {
        fn fill(&mut self, output: &mut [f32]) -> usize {
            let frames = (output.len() / 2).min(self.frames_left);
            for sample in &mut output[..frames * 2] {
                *sample = self.value;
            }
            self.frames_left -= frames;
            frames
        }

        fn is_done(&self) -> bool {
            self.frames_left == 0
        }
    }

    fn mixer_with(sources: Vec<Box<dyn AudioSource>>) -> Mixer {
        let (sender, receiver) = crossbeam_channel::unbounded();
        for source in sources {
            sender.send(source).unwrap();
        }
        Mixer::new(receiver)
    }

    #[test]
    fn overlapping_sources_sum() {
        let mut mixer = mixer_with(vec![
            Box::new(Tone { value: 0.25, frames_left: 4 }),
            Box::new(Tone { value: 0.5, frames_left: 4 }),
        ]);
        let mut output = [9.0f32; 8];
        mixer.fill(&mut output);
        assert!(output.iter().all(|&sample| (sample - 0.75).abs() < 1e-6));
    }

    #[test]
    fn output_is_zeroed_even_with_no_sources() {
        let mut mixer = mixer_with(Vec::new());
        let mut output = [9.0f32; 4];
        mixer.fill(&mut output);
        assert_eq!(output, [0.0; 4]);
    }

    #[test]
    fn finished_sources_are_retired_and_stop_contributing() {
        let mut mixer = mixer_with(vec![Box::new(Tone { value: 0.5, frames_left: 2 })]);
        let mut output = [0.0f32; 8];
        mixer.fill(&mut output);
        // the source had 2 frames left: they mix in, the tail stays silent
        assert_eq!(&output[..4], &[0.5; 4]);
        assert_eq!(&output[4..], &[0.0; 4]);
        mixer.fill(&mut output);
        assert_eq!(output, [0.0; 8], "retired source must not play again");
    }

    #[test]
    fn loud_overlap_clamps_to_unit_range() {
        let mut mixer = mixer_with(vec![
            Box::new(Tone { value: 0.8, frames_left: 4 }),
            Box::new(Tone { value: 0.8, frames_left: 4 }),
        ]);
        let mut output = [0.0f32; 8];
        mixer.fill(&mut output);
        assert!(output.iter().all(|&sample| (sample - 1.0).abs() < 1e-6));
    }

    #[test]
    fn sources_submitted_between_fills_are_picked_up() {
        let (sender, receiver) = crossbeam_channel::unbounded();
        let mut mixer = Mixer::new(receiver);
        let mut output = [0.0f32; 4];
        mixer.fill(&mut output);
        assert_eq!(output, [0.0; 4]);
        sender
            .send(Box::new(Tone { value: 0.5, frames_left: 2 }) as Box<dyn AudioSource>)
            .unwrap();
        mixer.fill(&mut output);
        assert_eq!(output, [0.5; 4]);
    }
}
