//! ECS plugin that registers the [`AudioPlayer`] resource and wires it to the active backend.

use crate::backend::{self, AudioBackend, PlatformBackend};
use crate::decoder::{DecodedSource, PlaybackOptions};
use crate::source::AudioSource;
use bevy_ecs::prelude::*;
use lunar_assets::Sound;
use lunar_core::{App, GamePlugin};

// PlatformBackend is Send but not Sync (it holds a Sender<Box<dyn AudioSource>>
// and CubebHandle). bevy_ecs Resources require Send + Sync, so we wrap in a
// newtype that asserts Sync.
//
// SAFETY: AudioPlayer is only accessed from the ECS world where bevy_ecs
// guarantees single-threaded access to a given resource at a time through
// its borrow checker. the inner PlatformBackend only exposes `submit` which
// goes through a channel and is safe to call from any thread.
struct SyncBackend(PlatformBackend);
unsafe impl Sync for SyncBackend {}

/// game-facing audio API — available as a [`Resource`] after [`AudioPlugin`] builds.
///
/// call [`play`] to decode and start a sound, or [`play_source`] to submit a custom
/// [`AudioSource`] (tracker, procedural synth, etc.) directly.
#[derive(Resource)]
pub struct AudioPlayer {
    backend: SyncBackend,
}

impl AudioPlayer {
    fn new(backend: PlatformBackend) -> Self { Self { backend: SyncBackend(backend) } }

    /// decode `sound` and submit it for playback with `options`.
    ///
    /// decodes synchronously on the caller's thread — fine for short SFX.
    /// for streaming music, decode off-thread and use [`play_source`] instead.
    pub fn play(&self, sound: &Sound, options: PlaybackOptions) {
        match DecodedSource::from_sound(sound, options) {
            Ok(source) => self.backend.0.submit(Box::new(source)),
            Err(error) => log::error!("lunar-audio: decode failed — {error}"),
        }
    }

    /// submit any [`AudioSource`] directly, bypassing the built-in decoder.
    ///
    /// use this to plug in a tracker engine, procedural generator, or any
    /// custom audio source without forking the engine.
    pub fn play_source(&self, source: impl AudioSource) {
        self.backend.0.submit(Box::new(source));
    }
}

/// registers the [`AudioPlayer`] resource and initializes the platform backend.
pub struct AudioPlugin;

impl GamePlugin for AudioPlugin {
    fn name(&self) -> &'static str { "AudioPlugin" }

    fn dependencies(&self) -> &[&str] { &[] }

    fn build(&mut self, app: &mut App) {
        match backend::init() {
            Ok(backend) => {
                app.insert_resource(AudioPlayer::new(backend));
                log::info!("AudioPlugin: {} backend initialized", backend_name());
            }
            Err(error) => {
                log::error!("AudioPlugin: backend init failed — {error}; audio disabled");
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn backend_name() -> &'static str { "cubeb" }
#[cfg(target_arch = "wasm32")]
fn backend_name() -> &'static str { "cpal/webaudio" }
