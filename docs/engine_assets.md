# engine_assets

asset system with typed handles and async loading

the engine owns all resources (textures, sounds, fonts, etc.).
game code references them through cheap-to-copy typed handles.

# example

```ignore
use engine_assets::{AssetServer, Handle, Texture};

fn load_assets(mut asset_server: ResMut<AssetServer>) {
    let texture_handle = asset_server.load_texture("sprites/player.png");
    let sound_handle = asset_server.load_sound("sfx/jump.wav");
}

fn use_asset(
    asset_server: Res<AssetServer>,
    handle: Res<Handle<Texture>>,
) {
    if asset_server.is_texture_ready(&handle) {
        let texture = asset_server.get_texture(&handle).unwrap();
        // use the texture
    }
}
```

## Structs

### AssetChangedEvent

event emitted when a watched asset file changes.

only emitted on native targets — not available on WASM.

### AssetInfo

metadata about a loaded asset.

contains the source path and current [`LoadState`] of an asset.
retrieved via [`AssetServer::get_texture_info`], [`AssetServer::get_sound_info`], etc.

### AssetPlugin

asset plugin, registers the asset server resource and
processes completed loads each frame.

add this plugin to your [`App`] to enable asset loading.
it registers the [`AssetServer`] as an ECS resource and
adds a system to drain completed loads each frame.

### AssetServer

asset server resource, manages loading and handles.

the asset server is the primary interface for loading game assets.
all load methods return immediately with a [`Handle`]; the actual
data loads asynchronously in the background.

# example

```ignore
fn load_assets(mut asset_server: ResMut<AssetServer>) {
    let handle = asset_server.load_texture("player.png");
    // handle is valid immediately, but the texture data loads in the background
}
```

### AssetWatcher

watches an asset directory for file changes and logs them.

only available on native targets — `notify` does not support WASM.
event routing into the ECS is a planned feature; currently changes
are logged via [`log::info`] for debugging.

### AssetWatcherPlugin

asset watcher plugin — registers [`AssetWatcher`] as a resource.

only available on native targets. add this plugin during development
to get file-change logs for assets in the `assets/` directory.

### Font

loaded font data using fontdue.

contains the parsed font ready for glyph rasterization.

### Handle

a generational handle to a loaded asset.

handles are cheap to copy and consist of an id and generation number.
the generation prevents use-after-free: if an asset is unloaded and
a new one takes its slot, the generation increments and old handles
become invalid.


### ImageTextureLoader

loader for common image formats (png, jpg, bmp, webp, gif).

uses the `image` crate to decode files into raw pixel data.

### IoTaskPool

io task pool for async file loading.

spawns worker threads that read files from disk and parse them
through the appropriate [`AssetLoader`]. results are sent back
through a channel for the main thread to collect each frame.

### MiLoader

loader for lunar image format (`.mi`) files.

decodes .mi bytes into [`RawTextureData`] which the render
system can upload to a GPU texture.

# example

```ignore
let loader = MiLoader;
let data = loader.load(&file_bytes)?;
// upload data.pixels to GPU
```

### MiTextureLoader

loader for .mi (lunar image) format.

decodes .mi bytes into raw pixel data via engine-image.

### OggSoundLoader

loader for ogg/vorbis sound files.

uses rodio to decode ogg files into sound buffers.

### RawTextureData

raw texture data from .mi files (kept for backward compat).

### Sound

decoded sound data with sample buffer.

contains f32 samples and the sample rate.
the audio system plays from this buffer.

### Texture

raw texture data decoded from an image file.

contains width, height, and raw pixel bytes (RGBA8).
the render system uploads this to the GPU.

### TtfFontLoader

loader for ttf/otf font files.

uses fontdue to rasterize font glyphs.

### WavSoundLoader

loader for wav sound files.

uses rodio to decode wav files into sound buffers.

## Enums

### AssetType

asset type tag for dispatching hot-reload events.

### LoadState

load state of an asset.

returned by [`AssetInfo::state`] to indicate the current status
of an asset load operation.

## Traits

### Asset

marker trait for types that can be loaded as assets.

implement this trait on your custom types to make them compatible
with the [`AssetServer`] and [`Handle`] system.

### AssetLoader

trait for types that can load a specific asset type from raw bytes.

implement this to support new asset formats.

## Type Aliases

### FontHandle

convenient type alias for a font handle.

### SoundHandle

convenient type alias for a sound handle.

### TextureHandle

convenient type alias for a texture handle.

## Macros

### impl_asset

convenience macro to implement the [`Asset`] trait for a custom type.

# example

```ignore
use engine_assets::impl_asset;

struct MyCustomTexture {
    width: u32,
    height: u32,
}

impl_asset!(MyCustomTexture);
```
