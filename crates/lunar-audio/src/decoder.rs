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
    pub looping: bool,
}

impl Default for PlaybackOptions {
    fn default() -> Self { Self { volume: 1.0, looping: false } }
}

/// pre-decoded stereo f32 PCM held in memory; emitted sample-by-sample.
pub struct DecodedSource {
    samples: Vec<f32>,
    cursor: usize,
    volume: f32,
    looping: bool,
}

impl DecodedSource {
    /// decode `sound` synchronously into interleaved stereo f32 PCM.
    ///
    /// returns an error string on format or decode failure.
    pub fn from_sound(sound: &Sound, options: PlaybackOptions) -> Result<Self, String> {
        let samples = decode(sound)?;
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
        let remaining = self.samples.len() - self.cursor;
        if remaining == 0 {
            if self.looping {
                self.cursor = 0;
            } else {
                return 0;
            }
        }

        let n = output.len().min(self.samples.len() - self.cursor);
        let v = self.volume;
        for (dst, src) in output[..n].iter_mut().zip(&self.samples[self.cursor..]) {
            *dst = src * v;
        }
        self.cursor += n;

        if self.looping && self.cursor == self.samples.len() {
            self.cursor = 0;
        }

        n / 2 // frames = samples / channels
    }

    fn is_done(&self) -> bool { !self.looping && self.cursor >= self.samples.len() }
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
        let mut sbuf = SampleBuffer::<f32>::new(audio.capacity() as u64, spec);
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
