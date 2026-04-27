//! asset system with typed handles and async loading
//!
//! the engine owns all resources (textures, sounds, fonts, etc.).
//! game code references them through cheap-to-copy typed handles.
//!
//! # handle-based design
//!
//! assets are loaded through the [`AssetServer`], which returns [`Handle`]s immediately.
//! the actual asset data loads asynchronously in the background.
//! handles are generational — if an asset is unloaded and reloaded,
//! old handles become invalid, preventing use-after-free bugs.
//!
//! # example
//!
//! ```ignore
//! use engine_assets::{AssetServer, Handle, Texture};
//!
//! fn load_assets(mut asset_server: ResMut<AssetServer>) {
//!     let texture_handle = asset_server.load_texture("sprites/player.png");
//!     let sound_handle = asset_server.load_sound("sfx/jump.wav");
//! }
//!
//! fn use_asset(
//!     asset_server: Res<AssetServer>,
//!     handle: Res<Handle<Texture>>,
//! ) {
//!     if asset_server.is_texture_ready(&handle) {
//!         let texture = asset_server.get_texture(&handle).unwrap();
//!         // use the texture
//!     }
//! }
//! ```

use std::collections::HashMap;
use std::marker::PhantomData;

use engine_core::{App, GamePlugin};

/// marker trait for types that can be loaded as assets.
///
/// implement this trait on your custom types to make them compatible
/// with the [`AssetServer`] and [`Handle`] system.
pub trait Asset: Send + Sync + 'static {}

/// load state of an asset.
///
/// returned by [`AssetInfo::state`] to indicate the current status
/// of an asset load operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadState {
    /// asset is being loaded, check again later
    Loading,
    /// asset is ready to use, data is available
    Loaded,
    /// asset failed to load, the handle is invalid
    Failed,
}

/// a generational handle to a loaded asset.
///
/// handles are cheap to copy and consist of an id and generation number.
/// the generation prevents use-after-free: if an asset is unloaded and
/// a new one takes its slot, the generation increments and old handles
/// become invalid.
///
/// # type parameters
///
/// * `T` - the asset type this handle refers to (e.g. [`Texture`], [`Sound`])
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Handle<T: Asset> {
    id: u32,
    generation: u16,
    _marker: PhantomData<T>,
}

impl<T: Asset> Handle<T> {
    /// create a new handle with the given id and generation
    pub fn new(id: u32, generation: u16) -> Self {
        Handle {
            id,
            generation,
            _marker: PhantomData,
        }
    }

    /// get the internal id
    pub fn id(&self) -> u32 {
        self.id
    }

    /// get the generation
    pub fn generation(&self) -> u16 {
        self.generation
    }
}

/// metadata about a loaded asset.
///
/// contains the source path and current [`LoadState`] of an asset.
/// retrieved via [`AssetServer::get_texture_info`], [`AssetServer::get_sound_info`], etc.
#[derive(Debug, Clone)]
pub struct AssetInfo {
    /// path the asset was loaded from
    pub path: String,
    /// current load state
    pub state: LoadState,
}

/// an entry in the asset store
struct AssetEntry<T: Asset> {
    data: Option<T>,
    generation: u16,
    path: String,
    state: LoadState,
    /// number of active handles referencing this entry
    ref_count: u32,
}

/// a typed asset store that holds loaded resources
struct AssetStore<T: Asset> {
    entries: Vec<Option<AssetEntry<T>>>,
    path_index: HashMap<String, u32>,
}

#[allow(dead_code)]
impl<T: Asset> AssetStore<T> {
    fn new() -> Self {
        AssetStore {
            entries: Vec::new(),
            path_index: HashMap::new(),
        }
    }

    fn allocate_slot(&mut self, path: String) -> Handle<T> {
        // check if already loaded
        if let Some(&id) = self.path_index.get(&path)
            && let Some(entry) = &self.entries[id as usize]
        {
            return Handle::new(id, entry.generation);
        }

        // find a free slot or append
        let id = self
            .entries
            .iter()
            .position(|e| e.is_none())
            .unwrap_or(self.entries.len()) as u32;
        let generation = 0u16;

        if id as usize == self.entries.len() {
            self.entries.push(None);
        }

        self.entries[id as usize] = Some(AssetEntry {
            data: None,
            generation,
            path: path.clone(),
            state: LoadState::Loading,
            ref_count: 1,
        });
        self.path_index.insert(path, id);

        Handle::new(id, generation)
    }

    fn insert(&mut self, id: u32, data: T) {
        if let Some(entry) = &mut self.entries[id as usize] {
            entry.data = Some(data);
            entry.state = LoadState::Loaded;
        }
    }

    fn mark_failed(&mut self, id: u32) {
        if let Some(entry) = &mut self.entries[id as usize] {
            entry.state = LoadState::Failed;
        }
    }

    fn increment_ref(&mut self, id: u32) {
        if let Some(entry) = &mut self.entries[id as usize] {
            entry.ref_count = entry.ref_count.saturating_add(1);
        }
    }

    fn decrement_ref(&mut self, id: u32) {
        if let Some(entry) = &mut self.entries[id as usize] {
            entry.ref_count = entry.ref_count.saturating_sub(1);
        }
    }

    fn is_unused(&self, id: u32) -> bool {
        self.entries
            .get(id as usize)
            .and_then(|e| e.as_ref())
            .is_some_and(|e| e.ref_count == 0)
    }

    fn is_ready(&self, handle: &Handle<T>) -> bool {
        if let Some(entry) = &self
            .entries
            .get(handle.id as usize)
            .and_then(|e| e.as_ref())
        {
            entry.generation == handle.generation && entry.state == LoadState::Loaded
        } else {
            false
        }
    }

    fn is_loaded(&self, handle: &Handle<T>) -> bool {
        self.entries
            .get(handle.id as usize)
            .and_then(|e| e.as_ref())
            .is_some_and(|entry| entry.state == LoadState::Loaded)
    }

    fn get_info(&self, handle: &Handle<T>) -> Option<AssetInfo> {
        self.entries
            .get(handle.id as usize)
            .and_then(|e| e.as_ref())
            .map(|entry| AssetInfo {
                path: entry.path.clone(),
                state: entry.state,
            })
    }

    fn get(&self, handle: &Handle<T>) -> Option<&T> {
        self.entries
            .get(handle.id as usize)
            .and_then(|e| e.as_ref())
            .and_then(|entry| {
                if entry.generation == handle.generation && entry.state == LoadState::Loaded {
                    entry.data.as_ref()
                } else {
                    None
                }
            })
    }
}

/// asset server resource, manages loading and handles.
///
/// the asset server is the primary interface for loading game assets.
/// all load methods return immediately with a [`Handle`]; the actual
/// data loads asynchronously in the background.
///
/// # example
///
/// ```ignore
/// fn load_assets(mut asset_server: ResMut<AssetServer>) {
///     let handle = asset_server.load_texture("player.png");
///     // handle is valid immediately, but the texture data loads in the background
/// }
/// ```
#[derive(bevy_ecs::prelude::Resource)]
pub struct AssetServer {
    texture_store: AssetStore<Texture>,
    sound_store: AssetStore<Sound>,
    font_store: AssetStore<Font>,
}

impl AssetServer {
    /// create a new asset server
    pub fn new() -> Self {
        AssetServer {
            texture_store: AssetStore::new(),
            sound_store: AssetStore::new(),
            font_store: AssetStore::new(),
        }
    }

    /// load a texture, returns immediately with a handle
    pub fn load_texture(&mut self, path: &str) -> Handle<Texture> {
        self.texture_store.allocate_slot(path.to_string())
    }

    /// load a sound, returns immediately with a handle
    pub fn load_sound(&mut self, path: &str) -> Handle<Sound> {
        self.sound_store.allocate_slot(path.to_string())
    }

    /// load a font, returns immediately with a handle
    pub fn load_font(&mut self, path: &str) -> Handle<Font> {
        self.font_store.allocate_slot(path.to_string())
    }

    /// check if a texture handle is ready
    pub fn is_texture_ready(&self, handle: &Handle<Texture>) -> bool {
        self.texture_store.is_ready(handle)
    }

    /// check if a sound handle is ready
    pub fn is_sound_ready(&self, handle: &Handle<Sound>) -> bool {
        self.sound_store.is_ready(handle)
    }

    /// check if a font handle is ready
    pub fn is_font_ready(&self, handle: &Handle<Font>) -> bool {
        self.font_store.is_ready(handle)
    }

    /// check if a texture is loaded
    pub fn is_texture_loaded(&self, handle: &Handle<Texture>) -> bool {
        self.texture_store.is_loaded(handle)
    }

    /// check if a sound is loaded
    pub fn is_sound_loaded(&self, handle: &Handle<Sound>) -> bool {
        self.sound_store.is_loaded(handle)
    }

    /// check if a font is loaded
    pub fn is_font_loaded(&self, handle: &Handle<Font>) -> bool {
        self.font_store.is_loaded(handle)
    }

    /// get texture info
    pub fn get_texture_info(&self, handle: &Handle<Texture>) -> Option<AssetInfo> {
        self.texture_store.get_info(handle)
    }

    /// get sound info
    pub fn get_sound_info(&self, handle: &Handle<Sound>) -> Option<AssetInfo> {
        self.sound_store.get_info(handle)
    }

    /// get font info
    pub fn get_font_info(&self, handle: &Handle<Font>) -> Option<AssetInfo> {
        self.font_store.get_info(handle)
    }

    /// get a loaded texture reference
    pub fn get_texture(&self, handle: &Handle<Texture>) -> Option<&Texture> {
        self.texture_store.get(handle)
    }

    /// get a loaded sound reference
    pub fn get_sound(&self, handle: &Handle<Sound>) -> Option<&Sound> {
        self.sound_store.get(handle)
    }

    /// get a loaded font reference
    pub fn get_font(&self, handle: &Handle<Font>) -> Option<&Font> {
        self.font_store.get(handle)
    }

    /// load a batch of textures, returns handles immediately
    pub fn load_textures(&mut self, paths: &[&str]) -> Vec<Handle<Texture>> {
        paths.iter().map(|p| self.load_texture(p)).collect()
    }
}

impl Default for AssetServer {
    fn default() -> Self {
        Self::new()
    }
}

/// placeholder asset type for texture assets.
///
/// real implementations will wrap actual GPU texture data.
/// currently a stub — replace with real texture loading later.
pub struct Texture;
impl Asset for Texture {}

/// placeholder asset type for sound assets.
///
/// real implementations will wrap decoded audio buffers.
/// currently a stub — replace with real sound loading later.
pub struct Sound;
impl Asset for Sound {}

/// placeholder asset type for font assets.
///
/// real implementations will wrap font glyph data.
/// currently a stub — replace with real font loading later.
pub struct Font;
impl Asset for Font {}

/// convenient type alias for a texture handle.
pub type TextureHandle = Handle<Texture>;
/// convenient type alias for a sound handle.
pub type SoundHandle = Handle<Sound>;
/// convenient type alias for a font handle.
pub type FontHandle = Handle<Font>;

/// asset plugin, registers the asset server resource.
///
/// add this plugin to your [`App`] to enable asset loading.
/// it registers the [`AssetServer`] as an ECS resource.
pub struct AssetPlugin;

impl GamePlugin for AssetPlugin {
    fn name(&self) -> &str {
        "AssetPlugin"
    }

    fn dependencies(&self) -> &[&str] {
        &[]
    }

    fn build(&mut self, app: &mut App) {
        app.insert_resource(AssetServer::new());
        log::info!("AssetPlugin: asset server resource registered");
    }
}

/// raw texture data decoded from a .mi file.
///
/// this is the intermediate format the [`MiLoader`] produces.
/// the render system will consume this and upload to the GPU.
pub struct RawTextureData {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// loader for lunar image format (`.mi`) files.
///
/// decodes .mi bytes into [`RawTextureData`] which the render
/// system can upload to a GPU texture.
///
/// # example
///
/// ```ignore
/// let loader = MiLoader;
/// let data = loader.load(&file_bytes)?;
/// // upload data.pixels to GPU
/// ```
pub struct MiLoader;

impl MiLoader {
    /// decode .mi bytes into raw texture data
    pub fn load(&self, bytes: &[u8]) -> Result<RawTextureData, engine_image::DecodeError> {
        let image = engine_image::decode(bytes)?;
        Ok(RawTextureData {
            width: image.width,
            height: image.height,
            pixels: image.pixels,
        })
    }
}
