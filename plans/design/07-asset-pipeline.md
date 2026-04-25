# Asset Pipeline

## Asset Types

| Type | Extensions | Notes |
|------|-----------|
| Texture | `.png`, `.jpg`, `.bmp`, `.webp`, `.gif` | Loaded as wgpu textures |
| Sprite Sheet | `.json` + image | JSON defines frames |
| Sound | `.wav`, `.ogg`, `.mp3`, `.flac` | Via miniaudio |
| Font | `.ttf`, `.otf` | For text rendering |
| Config | `.json`, `.toml`, `.yaml` | Game data files |
| Scene | `.json` | Serialized scene data (future) |

## Asset Server

```rust
#[derive(Resource)]
pub struct AssetServer {
    // internal state
}

impl AssetServer {
    /// Load an asset, returns handle immediately (data loads async)
    pub fn load<T: Asset>(&self, path: &str) -> Handle<T>;

    /// Load multiple assets
    pub fn load_batch<T: Asset, P: AsRef<str>>(&self, paths: &[P]) -> Vec<Handle<T>>;

    /// Check if an asset's data is ready to use
    pub fn is_ready<T: Asset>(&self, handle: &Handle<T>) -> bool;

    /// Get asset info
    pub fn get_info<T: Asset>(&self, handle: &Handle<T>) -> Option<&AssetInfo>;

    /// Wait for all assets to finish loading (blocking, for startup)
    pub fn wait_for_all(&self);

    /// Get the number of assets still loading
    pub fn loading_count(&self) -> usize;
}
```

## Asset Loading Architecture

```
AssetServer
├── IoTaskPool          # thread pool for file I/O
├── AssetLoaders        # map of extension -> loader
│   ├── TextureLoader
│   ├── SoundLoader
│   ├── FontLoader
│   └── ConfigLoader
└── AssetStores<T>      # one per asset type
    └── entries: Vec<Entry<T>>
```

Loading flow:
1. `assets.load("path")` called
2. AssetServer checks if already loaded (returns existing handle)
3. If not, spawns I/O task to read file
4. I/O task reads bytes, hands to appropriate loader
5. Loader parses bytes into `T`
6. Result stored in asset store, handle becomes "ready"

## Asset Paths

Assets are resolved relative to the game's `assets/` directory:

```
my-game/
├── assets/
│   ├── textures/
│   │   └── player.png
│   └── audio/
│       └── jump.wav
```

```rust
// These are equivalent:
let tex1 = assets.load("textures/player.png");
let tex2 = assets.load("./textures/player.png");
```

## Hot Reloading (Development)

```rust
#[derive(Resource)]
pub struct AssetWatcher {
    enabled: bool,
}

impl AssetWatcher {
    /// Enable file watching for hot reload
    pub fn enable(&mut self);

    /// Disable file watching
    pub fn disable(&mut self);
}

// In dev builds, hot reload is enabled by default
// When a file changes, the asset is reloaded and handles update
```

## Asset Bundles (Future)

For web and distribution, assets can be bundled:

```rust
// Bundle assets into a single file
// lunar bundle create --output game.assets

// Load from bundle
let assets = AssetServer::from_bundle("game.assets");
```

---

[← Back to Dialogue System](06-dialogue-system.md) | [Next: Plugin System →](08-plugin-system.md)
