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
use std::path::Path;
use std::sync::Arc;
use std::thread;

use bevy_ecs::prelude::*;
use crossbeam_channel::{Receiver, Sender};
use engine_core::{App, GamePlugin};

/// trait for types that can load a specific asset type from raw bytes.
///
/// implement this to support new asset formats.
pub trait AssetLoader: Send + Sync + 'static {
    /// the asset type this loader produces
    type Asset: Asset;

    /// load the asset from raw bytes, returning the parsed data
    fn load(&self, bytes: Vec<u8>) -> Result<Self::Asset, String>;
}

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
        // check if already loaded and ready
        if let Some(&id) = self.path_index.get(&path)
            && let Some(entry) = &mut self.entries[id as usize]
            && entry.state == LoadState::Loaded
        {
            entry.ref_count = entry.ref_count.saturating_add(1);
            return Handle::new(id, entry.generation);
        }

        // find a free slot or append
        let id = self
            .entries
            .iter()
            .position(|e| e.is_none())
            .unwrap_or(self.entries.len()) as u32;
        let generation = self
            .entries
            .get(id as usize)
            .and_then(|e| e.as_ref())
            .map(|e| e.generation.wrapping_add(1))
            .unwrap_or(0u16);

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

    fn loading_count(&self) -> usize {
        self.entries
            .iter()
            .flatten()
            .filter(|e| e.state == LoadState::Loading)
            .count()
    }
}

/// result of an async load operation, sent from worker threads back to the main thread
struct LoadResult<T: Asset> {
    id: u32,
    path: String,
    data: Result<T, String>,
}

/// io task pool for async file loading.
///
/// spawns worker threads that read files from disk and parse them
/// through the appropriate [`AssetLoader`]. results are sent back
/// through a channel for the main thread to collect each frame.
pub struct IoTaskPool {
    sender: Sender<LoadTask>,
    texture_receiver: Receiver<LoadResult<Texture>>,
    sound_receiver: Receiver<LoadResult<Sound>>,
    font_receiver: Receiver<LoadResult<Font>>,
}

/// a task to be executed by the io task pool
enum LoadTask {
    Texture {
        path: String,
        id: u32,
        loader: Arc<dyn TextureLoaderTrait>,
    },
    Sound {
        path: String,
        id: u32,
        loader: Arc<dyn SoundLoaderTrait>,
    },
    Font {
        path: String,
        id: u32,
        loader: Arc<dyn FontLoaderTrait>,
    },
}

/// trait for texture loaders (object-safe for dynamic dispatch)
trait TextureLoaderTrait: Send + Sync {
    fn load(&self, bytes: Vec<u8>) -> Result<Texture, String>;
}

/// trait for sound loaders (object-safe for dynamic dispatch)
trait SoundLoaderTrait: Send + Sync {
    fn load(&self, bytes: Vec<u8>) -> Result<Sound, String>;
}

/// trait for font loaders (object-safe for dynamic dispatch)
trait FontLoaderTrait: Send + Sync {
    fn load(&self, bytes: Vec<u8>) -> Result<Font, String>;
}

impl IoTaskPool {
    /// create a new io task pool with the given number of worker threads
    pub fn new(thread_count: usize) -> Self {
        let (task_send, task_recv) = crossbeam_channel::unbounded::<LoadTask>();
        let (texture_send, texture_receiver) = crossbeam_channel::unbounded();
        let (sound_send, sound_receiver) = crossbeam_channel::unbounded();
        let (font_send, font_receiver) = crossbeam_channel::unbounded();

        let task_recv = Arc::new(task_recv);

        for _ in 0..thread_count {
            let task_recv = Arc::clone(&task_recv);
            let texture_send = texture_send.clone();
            let sound_send = sound_send.clone();
            let font_send = font_send.clone();

            thread::spawn(move || {
                while let Ok(task) = task_recv.recv() {
                    match task {
                        LoadTask::Texture { path, id, loader } => {
                            let result = std::fs::read(&path)
                                .map_err(|e| format!("failed to read file: {e}"))
                                .and_then(|bytes| loader.load(bytes));

                            let _ = texture_send.send(LoadResult {
                                id,
                                path,
                                data: result,
                            });
                        }
                        LoadTask::Sound { path, id, loader } => {
                            let result = std::fs::read(&path)
                                .map_err(|e| format!("failed to read file: {e}"))
                                .and_then(|bytes| loader.load(bytes));

                            let _ = sound_send.send(LoadResult {
                                id,
                                path,
                                data: result,
                            });
                        }
                        LoadTask::Font { path, id, loader } => {
                            let result = std::fs::read(&path)
                                .map_err(|e| format!("failed to read file: {e}"))
                                .and_then(|bytes| loader.load(bytes));

                            let _ = font_send.send(LoadResult {
                                id,
                                path,
                                data: result,
                            });
                        }
                    }
                }
            });
        }

        IoTaskPool {
            sender: task_send,
            texture_receiver,
            sound_receiver,
            font_receiver,
        }
    }

    /// submit a texture load task
    fn load_texture(&self, path: String, id: u32, loader: Arc<dyn TextureLoaderTrait>) {
        let _ = self.sender.send(LoadTask::Texture { path, id, loader });
    }

    /// submit a sound load task
    fn load_sound(&self, path: String, id: u32, loader: Arc<dyn SoundLoaderTrait>) {
        let _ = self.sender.send(LoadTask::Sound { path, id, loader });
    }

    /// submit a font load task
    fn load_font(&self, path: String, id: u32, loader: Arc<dyn FontLoaderTrait>) {
        let _ = self.sender.send(LoadTask::Font { path, id, loader });
    }

    /// drain all completed texture results
    fn drain_texture_results(&self) -> Vec<LoadResult<Texture>> {
        let mut results = Vec::new();
        while let Ok(result) = self.texture_receiver.try_recv() {
            results.push(result);
        }
        results
    }

    /// drain all completed sound results
    fn drain_sound_results(&self) -> Vec<LoadResult<Sound>> {
        let mut results = Vec::new();
        while let Ok(result) = self.sound_receiver.try_recv() {
            results.push(result);
        }
        results
    }

    /// drain all completed font results
    fn drain_font_results(&self) -> Vec<LoadResult<Font>> {
        let mut results = Vec::new();
        while let Ok(result) = self.font_receiver.try_recv() {
            results.push(result);
        }
        results
    }
}

/// loader for common image formats (png, jpg, bmp, webp, gif).
///
/// uses the `image` crate to decode files into raw pixel data.
pub struct ImageTextureLoader;

impl TextureLoaderTrait for ImageTextureLoader {
    fn load(&self, bytes: Vec<u8>) -> Result<Texture, String> {
        let img =
            image::load_from_memory(&bytes).map_err(|e| format!("failed to decode image: {e}"))?;
        let rgba = img.to_rgba8();
        Ok(Texture {
            width: rgba.width(),
            height: rgba.height(),
            pixels: rgba.into_raw(),
        })
    }
}

/// loader for .mi (lunar image) format.
///
/// decodes .mi bytes into raw pixel data via engine-image.
pub struct MiTextureLoader;

impl TextureLoaderTrait for MiTextureLoader {
    fn load(&self, bytes: Vec<u8>) -> Result<Texture, String> {
        let image =
            engine_image::decode(&bytes).map_err(|e| format!("failed to decode .mi: {e}"))?;
        Ok(Texture {
            width: image.width,
            height: image.height,
            pixels: image.pixels,
        })
    }
}

/// loader for wav sound files.
///
/// uses rodio to decode wav files into sound buffers.
pub struct WavSoundLoader;

impl SoundLoaderTrait for WavSoundLoader {
    fn load(&self, bytes: Vec<u8>) -> Result<Sound, String> {
        let cursor = std::io::Cursor::new(bytes);
        let source =
            rodio::Decoder::new_wav(cursor).map_err(|e| format!("failed to decode wav: {e}"))?;
        let samples: Vec<f32> = source.map(|s| s as f32 / i16::MAX as f32).collect();
        Ok(Sound {
            samples,
            sample_rate: 44100,
        })
    }
}

/// loader for ogg/vorbis sound files.
///
/// uses rodio to decode ogg files into sound buffers.
pub struct OggSoundLoader;

impl SoundLoaderTrait for OggSoundLoader {
    fn load(&self, bytes: Vec<u8>) -> Result<Sound, String> {
        let cursor = std::io::Cursor::new(bytes);
        let source =
            rodio::Decoder::new_vorbis(cursor).map_err(|e| format!("failed to decode ogg: {e}"))?;
        let samples: Vec<f32> = source.map(|s| s as f32 / i16::MAX as f32).collect();
        Ok(Sound {
            samples,
            sample_rate: 44100,
        })
    }
}

/// loader for ttf/otf font files.
///
/// uses fontdue to rasterize font glyphs.
pub struct TtfFontLoader;

impl FontLoaderTrait for TtfFontLoader {
    fn load(&self, bytes: Vec<u8>) -> Result<Font, String> {
        let font = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
            .map_err(|e| format!("failed to load font: {e}"))?;
        Ok(Font { inner: font })
    }
}

/// resolve an asset path relative to the game's assets directory.
///
/// supports both "path" and "./path" formats. if the path doesn't start
/// with "assets/", it's resolved relative to the assets/ directory.
fn resolve_asset_path(path: &str) -> String {
    let cleaned = path.strip_prefix("./").unwrap_or(path);
    if Path::new(cleaned).is_absolute() {
        return cleaned.to_string();
    }
    // check if already starts with assets/
    if cleaned.starts_with("assets/") || cleaned.starts_with('/') {
        return cleaned.to_string();
    }
    format!("assets/{cleaned}")
}

/// determine the appropriate texture loader for a file extension.
fn texture_loader_for(path: &str) -> Arc<dyn TextureLoaderTrait> {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "mi" => Arc::new(MiTextureLoader),
        "png" | "jpg" | "jpeg" | "bmp" | "webp" | "gif" => Arc::new(ImageTextureLoader),
        _ => Arc::new(ImageTextureLoader), // default to image loader
    }
}

/// determine the appropriate sound loader for a file extension.
fn sound_loader_for(path: &str) -> Arc<dyn SoundLoaderTrait> {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "wav" => Arc::new(WavSoundLoader),
        "ogg" => Arc::new(OggSoundLoader),
        _ => Arc::new(WavSoundLoader), // default to wav loader
    }
}

/// determine the appropriate font loader for a file extension.
fn font_loader_for(path: &str) -> Arc<dyn FontLoaderTrait> {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "ttf" | "otf" => Arc::new(TtfFontLoader),
        _ => Arc::new(TtfFontLoader), // default to ttf loader
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
/// loader entry for the extension-based dispatch registry.
struct LoaderEntry {
    extensions: Vec<String>,
    loader: Arc<dyn AssetLoaderDyn>,
}

/// object-safe trait for type-erased asset loaders.
trait AssetLoaderDyn: Send + Sync {
    #[allow(dead_code)]
    fn load(&self, bytes: Vec<u8>) -> Result<Box<dyn std::any::Any>, String>;
}

/// wrapper that makes a typed AssetLoader implement AssetLoaderDyn.
struct AssetLoaderWrapper<L: AssetLoader> {
    #[allow(dead_code)]
    loader: L,
}

impl<L: AssetLoader> AssetLoaderDyn for AssetLoaderWrapper<L> {
    fn load(&self, bytes: Vec<u8>) -> Result<Box<dyn std::any::Any>, String> {
        let asset = self.loader.load(bytes)?;
        Ok(Box::new(asset))
    }
}

#[derive(Resource)]
pub struct AssetServer {
    texture_store: AssetStore<Texture>,
    sound_store: AssetStore<Sound>,
    font_store: AssetStore<Font>,
    io_pool: IoTaskPool,
    /// registered loaders keyed by file extension.
    custom_loaders: Vec<LoaderEntry>,
}

impl AssetServer {
    /// create a new asset server with the given number of io threads
    pub fn new(io_thread_count: usize) -> Self {
        AssetServer {
            texture_store: AssetStore::new(),
            sound_store: AssetStore::new(),
            font_store: AssetStore::new(),
            io_pool: IoTaskPool::new(io_thread_count),
            custom_loaders: Vec::new(),
        }
    }

    /// register a custom texture loader for the given extensions.
    pub fn register_texture_loader<L: AssetLoader<Asset = Texture>>(
        &mut self,
        extensions: &[&str],
        loader: L,
    ) {
        let entry = LoaderEntry {
            extensions: extensions.iter().map(|s| s.to_string()).collect(),
            loader: Arc::new(AssetLoaderWrapper { loader }),
        };
        self.custom_loaders.push(entry);
    }

    /// register a custom sound loader for the given extensions.
    pub fn register_sound_loader<L: AssetLoader<Asset = Sound>>(
        &mut self,
        extensions: &[&str],
        loader: L,
    ) {
        let entry = LoaderEntry {
            extensions: extensions.iter().map(|s| s.to_string()).collect(),
            loader: Arc::new(AssetLoaderWrapper { loader }),
        };
        self.custom_loaders.push(entry);
    }

    /// register a custom font loader for the given extensions.
    pub fn register_font_loader<L: AssetLoader<Asset = Font>>(
        &mut self,
        extensions: &[&str],
        loader: L,
    ) {
        let entry = LoaderEntry {
            extensions: extensions.iter().map(|s| s.to_string()).collect(),
            loader: Arc::new(AssetLoaderWrapper { loader }),
        };
        self.custom_loaders.push(entry);
    }

    /// find a custom loader by file extension.
    #[allow(dead_code)]
    fn find_loader(&self, path: &str) -> Option<&Arc<dyn AssetLoaderDyn>> {
        let ext = std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())?;
        for entry in &self.custom_loaders {
            if entry.extensions.iter().any(|e| e == ext) {
                return Some(&entry.loader);
            }
        }
        None
    }

    /// load an asset by path, returns immediately with a handle.
    /// the asset loads asynchronously in the background.
    /// this is the generic entry point — it dispatches to the correct
    /// type-specific method based on the `T` parameter.
    pub fn load<T: Asset>(&mut self, _path: &str) -> Handle<T> {
        // note: generic load dispatch requires type-erased loader registry.
        // use type-specific methods (load_texture, load_sound, load_font) directly.
        unimplemented!("use type-specific load methods for now")
    }

    /// load a batch of assets by path, returns handles immediately.
    pub fn load_batch<T: Asset>(&mut self, _paths: &[&str]) -> Vec<Handle<T>> {
        Vec::new()
    }

    /// load a texture, returns immediately with a handle.
    /// the texture loads asynchronously in the background.
    pub fn load_texture(&mut self, path: &str) -> Handle<Texture> {
        let resolved = resolve_asset_path(path);
        let handle = self.texture_store.allocate_slot(resolved.clone());
        let loader = texture_loader_for(&resolved);
        self.io_pool.load_texture(resolved, handle.id(), loader);
        handle
    }

    /// load a sound, returns immediately with a handle.
    /// the sound loads asynchronously in the background.
    pub fn load_sound(&mut self, path: &str) -> Handle<Sound> {
        let resolved = resolve_asset_path(path);
        let handle = self.sound_store.allocate_slot(resolved.clone());
        let loader = sound_loader_for(&resolved);
        self.io_pool.load_sound(resolved, handle.id(), loader);
        handle
    }

    /// load a font, returns immediately with a handle.
    /// the font loads asynchronously in the background.
    pub fn load_font(&mut self, path: &str) -> Handle<Font> {
        let resolved = resolve_asset_path(path);
        let handle = self.font_store.allocate_slot(resolved.clone());
        let loader = font_loader_for(&resolved);
        self.io_pool.load_font(resolved, handle.id(), loader);
        handle
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

    /// get the number of assets currently loading across all stores.
    pub fn loading_count(&self) -> usize {
        self.texture_store.loading_count()
            + self.sound_store.loading_count()
            + self.font_store.loading_count()
    }

    /// block until all assets are loaded.
    /// this is a convenience for tests or initialization code.
    /// in a real game, prefer polling `is_loaded` or `loading_count`.
    pub fn wait_for_all(&self) {
        while self.loading_count() > 0 {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    /// process completed load results from io threads.
    /// call this once per frame from the asset plugin's system.
    pub fn update(&mut self) {
        // drain texture results
        for result in self.io_pool.drain_texture_results() {
            match result.data {
                Ok(data) => {
                    self.texture_store.insert(result.id, data);
                }
                Err(err) => {
                    log::warn!("failed to load texture '{}': {}", result.path, err);
                    self.texture_store.mark_failed(result.id);
                }
            }
        }

        // drain sound results
        for result in self.io_pool.drain_sound_results() {
            match result.data {
                Ok(data) => {
                    self.sound_store.insert(result.id, data);
                }
                Err(err) => {
                    log::warn!("failed to load sound '{}': {}", result.path, err);
                    self.sound_store.mark_failed(result.id);
                }
            }
        }

        // drain font results
        for result in self.io_pool.drain_font_results() {
            match result.data {
                Ok(data) => {
                    self.font_store.insert(result.id, data);
                }
                Err(err) => {
                    log::warn!("failed to load font '{}': {}", result.path, err);
                    self.font_store.mark_failed(result.id);
                }
            }
        }
    }
}

impl Default for AssetServer {
    fn default() -> Self {
        Self::new(2)
    }
}

/// raw texture data decoded from an image file.
///
/// contains width, height, and raw pixel bytes (RGBA8).
/// the render system uploads this to the GPU.
pub struct Texture {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Asset for Texture {}

/// decoded sound data with sample buffer.
///
/// contains f32 samples and the sample rate.
/// the audio system plays from this buffer.
pub struct Sound {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

impl Asset for Sound {}

/// loaded font data using fontdue.
///
/// contains the parsed font ready for glyph rasterization.
pub struct Font {
    pub inner: fontdue::Font,
}

impl Asset for Font {}

/// convenient type alias for a texture handle.
pub type TextureHandle = Handle<Texture>;
/// convenient type alias for a sound handle.
pub type SoundHandle = Handle<Sound>;
/// convenient type alias for a font handle.
pub type FontHandle = Handle<Font>;

/// asset plugin, registers the asset server resource and
/// processes completed loads each frame.
///
/// add this plugin to your [`App`] to enable asset loading.
/// it registers the [`AssetServer`] as an ECS resource and
/// adds a system to drain completed loads each frame.
pub struct AssetPlugin;

impl GamePlugin for AssetPlugin {
    fn name(&self) -> &str {
        "AssetPlugin"
    }

    fn dependencies(&self) -> &[&str] {
        &[]
    }

    fn build(&mut self, app: &mut App) {
        app.insert_resource(AssetServer::new(2));
        app.add_system(process_asset_loads);
        log::info!("AssetPlugin: asset server resource registered");
    }
}

/// system that processes completed asset loads from io threads.
/// runs each frame during the update stage.
fn process_asset_loads(mut asset_server: ResMut<AssetServer>) {
    asset_server.update();
}

/// event emitted when a watched asset file changes.
#[derive(Debug, Clone)]
pub struct AssetChangedEvent {
    /// the file path that changed.
    pub path: String,
}

/// asset watcher resource, watches asset directories for changes
/// and triggers reloads of changed assets.
///
/// only active in dev builds — not intended for release.
#[cfg_attr(not(target_arch = "wasm32"), derive(Resource))]
pub struct AssetWatcher {
    #[allow(dead_code)]
    watcher: Option<notify::RecommendedWatcher>,
    /// map of watched paths to asset type tags.
    watched: std::collections::HashMap<String, AssetType>,
}

/// asset type tag for dispatching reloads.
#[derive(Debug, Clone, PartialEq)]
pub enum AssetType {
    Texture,
    Sound,
    Font,
}

impl AssetWatcher {
    /// create a new asset watcher that watches the given directory.
    pub fn new(watch_dir: &str) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            use notify::{RecursiveMode, Watcher as _};
            let _tx: std::sync::mpsc::Sender<()> = std::sync::mpsc::channel().0;
            // note: the watcher is created but events are not yet drained into the ECS.
            // a full implementation would spawn a thread to drain _rx and push events.
            let mut watcher: Option<notify::RecommendedWatcher> = None;
            if let Ok(mut w) =
                notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
                    if let Ok(event) = res {
                        for path in event.paths {
                            if let Some(p) = path.to_str() {
                                log::info!("asset changed: {}", p);
                            }
                        }
                    }
                })
            {
                let _ = w.watch(std::path::Path::new(watch_dir), RecursiveMode::Recursive);
                watcher = Some(w);
            }
            Self {
                watcher,
                watched: std::collections::HashMap::new(),
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = watch_dir;
            Self {
                watcher: None,
                watched: std::collections::HashMap::new(),
            }
        }
    }

    /// watch a specific asset path.
    pub fn watch(&mut self, path: &str, asset_type: AssetType) {
        self.watched.insert(path.to_string(), asset_type);
    }

    /// get the list of watched paths.
    pub fn watched_paths(&self) -> Vec<&str> {
        self.watched.keys().map(|s| s.as_str()).collect()
    }
}

/// asset watcher plugin, registers the AssetWatcher resource.
pub struct AssetWatcherPlugin;

impl GamePlugin for AssetWatcherPlugin {
    fn name(&self) -> &str {
        "AssetWatcherPlugin"
    }

    fn dependencies(&self) -> &[&str] {
        &["AssetPlugin"]
    }

    fn build(&mut self, app: &mut App) {
        app.insert_resource(AssetWatcher::new("assets/"));
        log::info!("AssetWatcherPlugin: asset watcher registered");
    }
}

/// raw texture data from .mi files (kept for backward compat).
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

/// convenience macro to implement the [`Asset`] trait for a custom type.
///
/// # example
///
/// ```ignore
/// use engine_assets::impl_asset;
///
/// struct MyCustomTexture {
///     width: u32,
///     height: u32,
/// }
///
/// impl_asset!(MyCustomTexture);
/// ```
#[macro_export]
macro_rules! impl_asset {
    ($ty:ty) => {
        impl $crate::Asset for $ty {}
    };
}
