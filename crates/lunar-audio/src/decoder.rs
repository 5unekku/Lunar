//! symphonia-backed PCM decoder; produces a [`DecodedSource`] that impls [`AudioSource`].

use crate::source::{AudioSource, SAMPLE_RATE};
use lunar_assets::{AudioFormat, Sound};
use symphonia::core::{
    audio::SampleBuffer,
    codecs::{DecoderOptions, CODEC_TYPE_NULL},
    errors::Error as SymphoniaError,
    formats::FormatOptions,
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
};

/// options passed to the decoder at play time.
#[derive(Clone, Copy)]
pub struct PlaybackOptions {
    /// linear volume scalar in 0.0..=1.0
    pub volume: f32,
    /// repeat from the beginning when playback reaches the end
    pub looping: bool,
}

impl Default for PlaybackOptions {
    fn default() -> Self { Self { volume: 1.0, looping: false } }
}

/// pre-decoded stereo f32 PCM emitted sample-by-sample; the buffer is shared
/// with the [`Sound`] asset's cache, so concurrent plays add no memory.
pub struct DecodedSource {
    samples: std::sync::Arc<[f32]>,
    cursor: usize,
    volume: f32,
    looping: bool,
}

impl DecodedSource {
    /// fetch (or decode and cache) `sound`'s PCM and wrap it for playback.
    ///
    /// the first play of a sound decodes synchronously and stores the result in
    /// [`Sound::decoded_pcm`]; later plays reuse it and only pay an Arc clone.
    /// returns an error string on format or decode failure.
    pub fn from_sound(sound: &Sound, options: PlaybackOptions) -> Result<Self, String> {
        let samples = match sound.decoded_pcm.get() {
            Some(pcm) => pcm.clone(),
            None => {
                let pcm: std::sync::Arc<[f32]> = decode(sound)?.into();
                // first writer wins if two plays race; both share one buffer
                sound.decoded_pcm.get_or_init(|| pcm).clone()
            }
        };
        Ok(Self {
            samples,
            cursor: 0,
            volume: options.volume,
            looping: options.looping,
        })
    }

    /// build a source directly from interleaved stereo samples, bypassing the
    /// decode path. test-only: lets fill()/is_done() run against known PCM.
    #[cfg(test)]
    fn from_samples(samples: Vec<f32>, volume: f32, looping: bool) -> Self {
        Self { samples: samples.into(), cursor: 0, volume, looping }
    }
}

impl AudioSource for DecodedSource {
    fn fill(&mut self, output: &mut [f32]) -> usize {
        if self.samples.is_empty() {
            return 0;
        }
        let volume = self.volume;
        let mut written = 0;
        // loop so a looping source restarts mid-buffer instead of padding the
        // tail with silence (which clicks at every loop seam)
        while written < output.len() {
            let remaining = self.samples.len() - self.cursor;
            if remaining == 0 {
                if !self.looping {
                    break;
                }
                self.cursor = 0;
                continue;
            }
            let n = (output.len() - written).min(remaining);
            let source = &self.samples[self.cursor..self.cursor + n];
            for (dst, sample) in output[written..written + n].iter_mut().zip(source) {
                *dst = sample * volume;
            }
            written += n;
            self.cursor += n;
        }
        written / 2 // frames = samples / channels
    }

    fn is_done(&self) -> bool {
        self.samples.is_empty() || (!self.looping && self.cursor >= self.samples.len())
    }
}

/// decode `sound` bytes into interleaved stereo f32 at [`SAMPLE_RATE`] Hz.
fn decode(sound: &Sound) -> Result<Vec<f32>, String> {
    let cursor = std::io::Cursor::new(sound.data.clone());
    let mss = MediaSourceStream::new(Box::new(cursor), Default::default());

    let mut hint = Hint::new();
    match sound.format {
        AudioFormat::OggVorbis | AudioFormat::OggOpus => { hint.with_extension("ogg"); }
        AudioFormat::Wav => { hint.with_extension("wav"); }
        AudioFormat::Flac => { hint.with_extension("flac"); }
        AudioFormat::Unknown => {}
    }

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| format!("probe failed: {e}"))?;

    let mut reader = probed.format;
    let track = reader
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or("no audio track found")?;

    let channels = track
        .codec_params
        .channels
        .map(|c| c.count())
        .unwrap_or(2);
    let source_rate = track.codec_params.sample_rate.unwrap_or(SAMPLE_RATE);
    let track_id = track.id;

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| format!("codec init failed: {e}"))?;

    let mut raw: Vec<f32> = Vec::new();
    // reused across packets; reallocated only if a packet needs more room
    let mut sample_buf: Option<SampleBuffer<f32>> = None;

    loop {
        let packet = match reader.next_packet() {
            Ok(p) => p,
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(SymphoniaError::ResetRequired) => {
                decoder.reset();
                continue;
            }
            Err(e) => return Err(format!("packet read failed: {e}")),
        };

        if packet.track_id() != track_id { continue; }

        let audio = match decoder.decode(&packet) {
            Ok(buf) => buf,
            Err(SymphoniaError::IoError(_) | SymphoniaError::DecodeError(_)) => continue,
            Err(e) => return Err(format!("decode failed: {e}")),
        };

        let spec = *audio.spec();
        let needed = audio.capacity() * spec.channels.count();
        if sample_buf.as_ref().is_none_or(|b| b.capacity() < needed) {
            sample_buf = Some(SampleBuffer::<f32>::new(audio.capacity() as u64, spec));
        }
        let sbuf = sample_buf.as_mut().unwrap();
        sbuf.copy_interleaved_ref(audio);
        raw.extend_from_slice(sbuf.samples());
    }

    // upmix mono to stereo; downmix >2 channels to stereo
    let stereo: Vec<f32> = match channels {
        1 => raw.iter().flat_map(|&s| [s, s]).collect(),
        2 => raw,
        n => raw
            .chunks(n)
            .flat_map(|frame| [frame[0], frame[1]])
            .collect(),
    };

    // decode runs once per sound and the result is cached, so the resample
    // cost is load-time only
    if source_rate != SAMPLE_RATE && source_rate > 0 {
        return Ok(resample_stereo(&stereo, source_rate, SAMPLE_RATE));
    }

    Ok(stereo)
}

/// linearly resample interleaved stereo f32 PCM from `source_rate` to `target_rate`.
///
/// linear interpolation is transparent for game sfx and short music loops; the
/// engine mixes at a fixed [`SAMPLE_RATE`] so this runs only on mismatched files.
fn resample_stereo(input: &[f32], source_rate: u32, target_rate: u32) -> Vec<f32> {
    let frame_count = input.len() / 2;
    if frame_count == 0 {
        return Vec::new();
    }
    let out_frames = (frame_count as u64 * target_rate as u64 / source_rate as u64) as usize;
    let step = source_rate as f64 / target_rate as f64;
    let mut out = Vec::with_capacity(out_frames * 2);
    for i in 0..out_frames {
        let pos = i as f64 * step;
        let idx = pos as usize;
        let frac = (pos - idx as f64) as f32;
        let next = (idx + 1).min(frame_count - 1);
        let left = input[idx * 2] * (1.0 - frac) + input[next * 2] * frac;
        let right = input[idx * 2 + 1] * (1.0 - frac) + input[next * 2 + 1] * frac;
        out.push(left);
        out.push(right);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_scales_frame_count_by_rate_ratio() {
        // 441 frames at 44.1kHz → 480 frames at 48kHz
        let input = vec![0.5f32; 441 * 2];
        let out = resample_stereo(&input, 44_100, 48_000);
        assert_eq!(out.len(), 480 * 2);
    }

    #[test]
    fn resample_preserves_constant_signal() {
        let input = vec![0.25f32; 100 * 2];
        let out = resample_stereo(&input, 22_050, 48_000);
        assert!(out.iter().all(|&s| (s - 0.25).abs() < 1e-6));
    }

    #[test]
    fn resample_interpolates_a_ramp() {
        // left channel ramps 0,1,2,3 — downsampling by 2 should land between samples
        let input: Vec<f32> = (0..8).flat_map(|i| [i as f32, 0.0]).collect();
        let out = resample_stereo(&input, 48_000, 24_000);
        assert_eq!(out.len(), 4 * 2);
        assert_eq!(out[0], 0.0);
        assert_eq!(out[2], 2.0);
        assert_eq!(out[4], 4.0);
    }

    #[test]
    fn resample_handles_empty_input() {
        assert!(resample_stereo(&[], 44_100, 48_000).is_empty());
    }

    #[test]
    fn fill_applies_volume_and_reports_frames() {
        let mut source = DecodedSource::from_samples(vec![1.0, 1.0, 1.0, 1.0], 0.5, false);
        let mut output = [0.0f32; 4];
        let frames = source.fill(&mut output);
        assert_eq!(frames, 2);
        assert!(output.iter().all(|&sample| (sample - 0.5).abs() < 1e-6));
    }

    #[test]
    fn fill_stops_at_end_when_not_looping() {
        let mut source = DecodedSource::from_samples(vec![1.0, 2.0, 3.0, 4.0], 1.0, false);
        let mut output = [9.0f32; 8];
        let frames = source.fill(&mut output);
        assert_eq!(frames, 2);
        assert_eq!(&output[..4], &[1.0, 2.0, 3.0, 4.0]);
        // only the reported prefix is written; the mixer ignores the tail
        assert_eq!(&output[4..], &[9.0; 4]);
        assert!(source.is_done());
    }

    #[test]
    fn fill_looping_restarts_mid_buffer_without_a_gap() {
        let mut source = DecodedSource::from_samples(vec![1.0, 2.0, 3.0, 4.0], 1.0, true);
        let mut output = [0.0f32; 8];
        let frames = source.fill(&mut output);
        // the loop seam lands mid-buffer with no silent padding (silence clicks)
        assert_eq!(frames, 4);
        assert_eq!(output, [1.0, 2.0, 3.0, 4.0, 1.0, 2.0, 3.0, 4.0]);
        assert!(!source.is_done());
    }

    #[test]
    fn fill_resumes_from_cursor_across_calls() {
        let mut source = DecodedSource::from_samples(vec![1.0, 2.0, 3.0, 4.0], 1.0, false);
        let mut output = [0.0f32; 2];
        assert_eq!(source.fill(&mut output), 1);
        assert_eq!(output, [1.0, 2.0]);
        assert!(!source.is_done());
        assert_eq!(source.fill(&mut output), 1);
        assert_eq!(output, [3.0, 4.0]);
        assert!(source.is_done());
    }

    #[test]
    fn fill_empty_source_writes_nothing_and_is_done() {
        let mut source = DecodedSource::from_samples(Vec::new(), 1.0, true);
        let mut output = [0.0f32; 4];
        assert_eq!(source.fill(&mut output), 0);
        assert!(source.is_done());
    }
}
