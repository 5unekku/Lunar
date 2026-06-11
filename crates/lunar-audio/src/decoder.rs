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

    // upmix mono to stereo; dowmix >2 channels to stereo
    let stereo = match channels {
        1 => raw.iter().flat_map(|&s| [s, s]).collect(),
        2 => raw,
        n => raw
            .chunks(n)
            .flat_map(|frame| [frame[0], frame[1]])
            .collect(),
    };

    // if source rate differs, log and use as-is for now
    // TODO: resample with rubato when source_rate != SAMPLE_RATE
    if source_rate != SAMPLE_RATE {
        log::warn!(
            "lunar-audio: source is {source_rate} Hz, device expects {SAMPLE_RATE} Hz — pitch will be off until resampling is added"
        );
    }

    Ok(stereo)
}
