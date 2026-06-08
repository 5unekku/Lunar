# assets

all assets (textures, fonts, sounds) go through `AssetServer`. game code gets back
a `Handle<T>` immediately — the data loads in the background.

## loading

call `load_*` from a startup system. the returned handles are cheap to copy and
safe to store in resources or components:

```rust
fn setup(mut commands: Commands, mut assets: ResMut<AssetServer>) {
    let player_texture = assets.load_texture("sprites/player.png");
    let ui_font = assets.load_font("fonts/ui.ttf");
    let jump_sound = assets.load_sound("sfx/jump.ogg");

    // store handles for later use
    commands.insert_resource(GameAssets {
        player: player_texture,
        font: ui_font,
        jump: jump_sound,
    });
}

#[derive(Resource)]
struct GameAssets {
    player: Handle<Texture>,
    font: Handle<Font>,
    jump: Handle<Sound>,
}
```

asset paths are relative to the `assets/` directory at the workspace root.

## loading screen

`LoadingState` is a resource updated each frame with the current load progress:

```rust
fn loading_screen(
    loading: Res<LoadingState>,
    mut next_state: ResMut<GamePhase>,
) {
    let stats = loading.stats;

    if stats.is_done() {
        *next_state = GamePhase::Playing;
        return;
    }

    // stats.fraction() is 0.0..=1.0 — use to draw a progress bar
    let progress = stats.fraction();
    println!("loading: {:.0}%", progress * 100.0);
}
```

`LoadingStats` fields:
- `total: u32` — assets registered (loading + loaded + failed)
- `loaded: u32` — successfully loaded
- `failed: u32` — failed to load
- `.fraction()` — `loaded / total` as f32; returns 1.0 if total == 0
- `.is_done()` — true when `loaded + failed >= total`

## accessing asset data

for direct access to the underlying data (e.g. to play a sound, measure a texture):

```rust
fn play_on_ready(
    loading: Res<LoadingState>,
    assets: Res<AssetServer>,
    handles: Res<GameAssets>,
    audio: Res<AudioPlayer>,
) {
    if loading.stats.is_done() {
        if let Some(sound) = assets.get_sound(&handles.jump) {
            audio.play(sound, PlaybackOptions::default());
        }
        if let Some(texture) = assets.get_texture(&handles.player) {
            println!("{}x{}", texture.width, texture.height);
        }
    }
}
```

## handles

`Handle<T>` is:
- `Copy` and `Clone` — free to duplicate, pass around, store in components
- generational — if an asset is reloaded, the old handle becomes invalid
- typed — `Handle<Texture>` and `Handle<Sound>` are distinct types

handles do not prevent the asset from being unloaded. the engine manages
asset lifetimes internally.

## compile-time embedded textures

for assets that must always be available (e.g. a loading spinner), embed them at
compile time with the `texture!` macro:

```rust
// embeds assets/ui/spinner.png directly in the binary; no runtime loading needed
let spinner = texture!("ui/spinner.png");
```

this is especially useful for WASM targets where assets may not be network-available yet.

## custom asset loaders

implement `AssetLoader` to support new file formats:

```rust
use lunar::lunar_assets::{Asset, AssetLoader};

struct LevelData { /* ... */ }
impl Asset for LevelData {}

struct LevelLoader;

impl AssetLoader for LevelLoader {
    type Asset = LevelData;

    fn load(&self, bytes: Vec<u8>) -> Result<LevelData, String> {
        // parse bytes into LevelData
        todo!()
    }
}
```

register with `AssetServer::register_loader` in a startup system.
