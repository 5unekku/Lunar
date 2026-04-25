//! asset system with typed handles and async loading
//!
//! the engine owns all resources (textures, sounds, fonts, etc.).
//! game code references them through cheap-to-copy typed handles.

use std::collections::HashMap;
use std::marker::PhantomData;

use engine_core::{App, GamePlugin};

/// marker trait for types that can be loaded as assets
pub trait Asset: Send + Sync + 'static {}

/// load state of an asset
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadState {
    /// asset is being loaded
    Loading,
    /// asset is ready to use
    Loaded,
    /// asset failed to load
    Failed,
}

/// a generational handle to a loaded asset
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

/// metadata about a loaded asset
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

/// asset server resource, manages loading and handles
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

/// placeholder asset types — real implementations come later
/// marker type for texture assets
pub struct Texture;
impl Asset for Texture {}

/// marker type for sound assets
pub struct Sound;
impl Asset for Sound {}

/// marker type for font assets
pub struct Font;
impl Asset for Font {}

/// convenient type aliases
pub type TextureHandle = Handle<Texture>;
pub type SoundHandle = Handle<Sound>;
pub type FontHandle = Handle<Font>;

/// asset plugin, registers the asset server resource
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
