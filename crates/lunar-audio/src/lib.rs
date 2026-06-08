//! audio system for the Lunar engine.
//!
//! # backends
//!
//! - **native** — cubeb (WASAPI / CoreAudio / PulseAudio / ALSA), low latency
//! - **wasm32** — cpal webaudio (ScriptProcessorNode over the browser's AudioContext)
//!
//! # extension point
//!
//! implement [`AudioSource`] to slot in a tracker engine, procedural synth, or any
//! other custom generator without touching the engine. submit instances via
//! [`AudioPlayer::play_source`]. the engine only calls [`AudioSource::fill`] and
//! [`AudioSource::is_done`] — everything else is up to the implementor.
//!
//! # quick start
//!
//! ```ignore
//! use lunar_audio::{AudioPlugin, AudioPlayer, PlaybackOptions};
//! use lunar_assets::AssetServer;
//!
//! // in App setup:
//! app.add_plugin(AudioPlugin);
//!
//! // in a system:
//! fn play_jump(audio: Res<AudioPlayer>, assets: Res<AssetServer>, handle: Res<Handle<Sound>>) {
//!     if let Some(sound) = assets.get_sound(&handle) {
//!         audio.play(sound, PlaybackOptions::default());
//!     }
//! }
//! ```

mod backend;
mod decoder;
mod mixer;
mod source;
mod plugin;

pub use decoder::PlaybackOptions;
pub use plugin::{AudioPlayer, AudioPlugin};
pub use source::{AudioSource, SAMPLE_RATE};
