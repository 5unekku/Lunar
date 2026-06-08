# audio

requires the `audio` feature.

## setup

add `AudioPlugin` to your game:

```rust
use lunar::prelude::*;
use lunar::lunar_audio::AudioPlugin;

impl GamePlugin for MyGame {
    fn build(&mut self, app: &mut App) {
        app.add_plugin(AudioPlugin);
    }
}
```

`AudioPlugin` initializes the platform backend (cubeb on native, WebAudio on WASM)
and inserts the `AudioPlayer` resource. if backend initialization fails, it logs an
error and continues without audio — the rest of your game still runs.

## loading sounds

sounds are loaded through `AssetServer` like any other asset:

```rust
fn setup(mut assets: ResMut<AssetServer>) {
    let jump_sound = assets.load_sound("sfx/jump.ogg");
    let music = assets.load_sound("music/theme.ogg");
}
```

supported formats: OGG Vorbis, OGG Opus, WAV, FLAC.

store handles in a resource so they're accessible from other systems:

```rust
#[derive(Resource)]
struct SoundHandles {
    jump: Handle<Sound>,
    music: Handle<Sound>,
}

fn setup(mut commands: Commands, mut assets: ResMut<AssetServer>) {
    commands.insert_resource(SoundHandles {
        jump: assets.load_sound("sfx/jump.ogg"),
        music: assets.load_sound("music/theme.ogg"),
    });
}
```

## playing sounds

call `AudioPlayer::play` with a `Sound` reference and `PlaybackOptions`:

```rust
fn on_jump(
    input: Res<InputState>,
    sounds: Res<SoundHandles>,
    asset_server: Res<AssetServer>,
    audio: Res<AudioPlayer>,
) {
    if input.is_key_just_pressed(KeyCode::Space) {
        if let Some(sound) = asset_server.get_sound(&sounds.jump) {
            audio.play(sound, PlaybackOptions {
                volume: 1.0,
                looping: false,
            });
        }
    }
}
```

`PlaybackOptions` fields:
- `volume: f32` — linear scalar in `0.0..=1.0`
- `looping: bool` — loop from the beginning when playback ends

`PlaybackOptions::default()` is `{ volume: 1.0, looping: false }`.

looping music:

```rust
audio.play(music_sound, PlaybackOptions { volume: 0.7, looping: true });
```

## waiting for assets to load

sounds load asynchronously. playing a sound before it's ready is silently ignored.
use `LoadingState` to gate on asset readiness:

```rust
fn play_when_ready(
    loading: Res<LoadingState>,
    sounds: Res<SoundHandles>,
    asset_server: Res<AssetServer>,
    audio: Res<AudioPlayer>,
    mut played: Local<bool>,
) {
    if !*played && loading.stats.is_done() {
        if let Some(sound) = asset_server.get_sound(&sounds.music) {
            audio.play(sound, PlaybackOptions { volume: 0.7, looping: true });
            *played = true;
        }
    }
}
```

## custom audio sources

`AudioPlayer::play_source` accepts any type implementing `AudioSource` — useful
for procedural audio, tracker engines, or streaming decoders:

```rust
use lunar::lunar_audio::AudioSource;

struct SineWave {
    phase: f32,
    frequency: f32,
}

impl AudioSource for SineWave {
    fn fill(&mut self, output: &mut [f32]) -> usize {
        let sample_rate = 44100.0;
        for (i, sample) in output.iter_mut().enumerate() {
            *sample = (self.phase + i as f32 * self.frequency / sample_rate * std::f32::consts::TAU).sin() * 0.3;
        }
        self.phase += output.len() as f32 * self.frequency / sample_rate * std::f32::consts::TAU;
        output.len() / 2  // return frame count (samples / channels)
    }

    fn is_done(&self) -> bool { false }  // play forever
}

// submit directly without going through the decoder
audio.play_source(SineWave { phase: 0.0, frequency: 440.0 });
```
