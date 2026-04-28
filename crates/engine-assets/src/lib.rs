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

#[cfg(target_arch = "wasm32")]
pub mod web_fetch;

#[cfg(target_arch = "wasm32")]
pub mod bundled;

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
#[cfg(not(target_arch = "wasm32"))]
pub struct IoTaskPool {
    sender: Sender<LoadTask>,
    texture_receiver: Receiver<LoadResult<Texture>>,
    sound_receiver: Receiver<LoadResult<Sound>>,
    font_receiver: Receiver<LoadResult<Font>>,
}

/// a task to be executed by the io task pool
#[cfg(not(target_arch = "wasm32"))]
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

#[cfg(not(target_arch = "wasm32"))]
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

/// web-compatible io task pool using async fetch instead of threads.
///
/// results are sent back through channels identical to the native pool,
/// so [`AssetServer::update`] works the same on both targets.
#[cfg(target_arch = "wasm32")]
pub struct IoTaskPool {
    texture_results: crossbeam_channel::Receiver<LoadResult<Texture>>,
    sound_results: crossbeam_channel::Receiver<LoadResult<Sound>>,
    font_results: crossbeam_channel::Receiver<LoadResult<Font>>,
    texture_send: crossbeam_channel::Sender<LoadResult<Texture>>,
    sound_send: crossbeam_channel::Sender<LoadResult<Sound>>,
    font_send: crossbeam_channel::Sender<LoadResult<Font>>,
}

#[cfg(target_arch = "wasm32")]
impl IoTaskPool {
    /// create a new io task pool for web (`thread_count` is unused on WASM).
    pub fn new(_thread_count: usize) -> Self {
        let (texture_send, texture_results) = crossbeam_channel::unbounded();
        let (sound_send, sound_results) = crossbeam_channel::unbounded();
        let (font_send, font_results) = crossbeam_channel::unbounded();
        IoTaskPool {
            texture_results,
            sound_results,
            font_results,
            texture_send,
            sound_send,
            font_send,
        }
    }

    /// submit a texture load task — checks bundled assets first, then falls back to fetch.
    fn load_texture(&self, path: String, id: u32, loader: Arc<dyn TextureLoaderTrait>) {
        let send = self.texture_send.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let bytes_result = if crate::bundled::contains(&path) {
                crate::bundled::get(&path).ok_or_else(|| "bundled asset disappeared".to_string())
            } else {
                crate::web_fetch::fetch_bytes(&path).await
            };
            let data = bytes_result.and_then(|bytes| loader.load(bytes));
            let _ = send.send(LoadResult { id, path, data });
        });
    }

    /// submit a sound load task — checks bundled assets first, then falls back to fetch.
    fn load_sound(&self, path: String, id: u32, loader: Arc<dyn SoundLoaderTrait>) {
        let send = self.sound_send.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let bytes_result = if crate::bundled::contains(&path) {
                crate::bundled::get(&path).ok_or_else(|| "bundled asset disappeared".to_string())
            } else {
                crate::web_fetch::fetch_bytes(&path).await
            };
            let data = bytes_result.and_then(|bytes| loader.load(bytes));
            let _ = send.send(LoadResult { id, path, data });
        });
    }

    /// submit a font load task — checks bundled assets first, then falls back to fetch.
    fn load_font(&self, path: String, id: u32, loader: Arc<dyn FontLoaderTrait>) {
        let send = self.font_send.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let bytes_result = if crate::bundled::contains(&path) {
                crate::bundled::get(&path).ok_or_else(|| "bundled asset disappeared".to_string())
            } else {
                crate::web_fetch::fetch_bytes(&path).await
            };
            let data = bytes_result.and_then(|bytes| loader.load(bytes));
            let _ = send.send(LoadResult { id, path, data });
        });
    }

    /// drain all completed texture results.
    fn drain_texture_results(&self) -> Vec<LoadResult<Texture>> {
        let mut results = Vec::new();
        while let Ok(result) = self.texture_results.try_recv() {
            results.push(result);
        }
        results
    }

    /// drain all completed sound results.
    fn drain_sound_results(&self) -> Vec<LoadResult<Sound>> {
        let mut results = Vec::new();
        while let Ok(result) = self.sound_results.try_recv() {
            results.push(result);
        }
        results
    }

    /// drain all completed font results.
    fn drain_font_results(&self) -> Vec<LoadResult<Font>> {
        let mut results = Vec::new();
        while let Ok(result) = self.font_results.try_recv() {
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
        use rodio::Source as _;
        let cursor = std::io::Cursor::new(bytes);
        let source =
            rodio::Decoder::new_wav(cursor).map_err(|e| format!("failed to decode wav: {e}"))?;
        let sample_rate = source.sample_rate();
        let samples: Vec<f32> = source.map(|s| s as f32 / i16::MAX as f32).collect();
        Ok(Sound { samples, sample_rate })
    }
}

/// loader for ogg/vorbis sound files.
///
/// uses rodio to decode ogg files into sound buffers.
pub struct OggSoundLoader;

impl SoundLoaderTrait for OggSoundLoader {
    fn load(&self, bytes: Vec<u8>) -> Result<Sound, String> {
        use rodio::Source as _;
        let cursor = std::io::Cursor::new(bytes);
        let source =
            rodio::Decoder::new_vorbis(cursor).map_err(|e| format!("failed to decode ogg: {e}"))?;
        let sample_rate = source.sample_rate();
        let samples: Vec<f32> = source.map(|s| s as f32 / i16::MAX as f32).collect();
        Ok(Sound { samples, sample_rate })
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

/// per-type entry in the custom loader registry.
struct CustomLoaderEntry<T: ?Sized> {
    extensions: Vec<String>,
    loader: Arc<T>,
}

/// bridges a public [`AssetLoader<Asset=Texture>`] into the internal [`TextureLoaderTrait`].
struct TextureLoaderAdapter<L: AssetLoader<Asset = Texture>>(L);
impl<L: AssetLoader<Asset = Texture> + Send + Sync> TextureLoaderTrait for TextureLoaderAdapter<L> {
    fn load(&self, bytes: Vec<u8>) -> Result<Texture, String> {
        self.0.load(bytes)
    }
}

/// bridges a public [`AssetLoader<Asset=Sound>`] into the internal [`SoundLoaderTrait`].
struct SoundLoaderAdapter<L: AssetLoader<Asset = Sound>>(L);
impl<L: AssetLoader<Asset = Sound> + Send + Sync> SoundLoaderTrait for SoundLoaderAdapter<L> {
    fn load(&self, bytes: Vec<u8>) -> Result<Sound, String> {
        self.0.load(bytes)
    }
}

/// bridges a public [`AssetLoader<Asset=Font>`] into the internal [`FontLoaderTrait`].
struct FontLoaderAdapter<L: AssetLoader<Asset = Font>>(L);
impl<L: AssetLoader<Asset = Font> + Send + Sync> FontLoaderTrait for FontLoaderAdapter<L> {
    fn load(&self, bytes: Vec<u8>) -> Result<Font, String> {
        self.0.load(bytes)
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
#[derive(Resource)]
pub struct AssetServer {
    texture_store: AssetStore<Texture>,
    sound_store: AssetStore<Sound>,
    font_store: AssetStore<Font>,
    io_pool: IoTaskPool,
    custom_texture_loaders: Vec<CustomLoaderEntry<dyn TextureLoaderTrait>>,
    custom_sound_loaders: Vec<CustomLoaderEntry<dyn SoundLoaderTrait>>,
    custom_font_loaders: Vec<CustomLoaderEntry<dyn FontLoaderTrait>>,
}

impl AssetServer {
    /// create a new asset server with the given number of io threads.
    pub fn new(io_thread_count: usize) -> Self {
        AssetServer {
            texture_store: AssetStore::new(),
            sound_store: AssetStore::new(),
            font_store: AssetStore::new(),
            io_pool: IoTaskPool::new(io_thread_count),
            custom_texture_loaders: Vec::new(),
            custom_sound_loaders: Vec::new(),
            custom_font_loaders: Vec::new(),
        }
    }

    /// register a custom texture loader for the given file extensions.
    ///
    /// custom loaders take priority over the built-in ones. call this before
    /// any [`load_texture`](Self::load_texture) calls for those extensions.
    pub fn register_texture_loader<L: AssetLoader<Asset = Texture> + 'static>(
        &mut self,
        extensions: &[&str],
        loader: L,
    ) {
        self.custom_texture_loaders.push(CustomLoaderEntry {
            extensions: extensions.iter().map(|s| s.to_string()).collect(),
            loader: Arc::new(TextureLoaderAdapter(loader)),
        });
    }

    /// register a custom sound loader for the given file extensions.
    ///
    /// custom loaders take priority over the built-in ones.
    pub fn register_sound_loader<L: AssetLoader<Asset = Sound> + 'static>(
        &mut self,
        extensions: &[&str],
        loader: L,
    ) {
        self.custom_sound_loaders.push(CustomLoaderEntry {
            extensions: extensions.iter().map(|s| s.to_string()).collect(),
            loader: Arc::new(SoundLoaderAdapter(loader)),
        });
    }

    /// register a custom font loader for the given file extensions.
    ///
    /// custom loaders take priority over the built-in ones.
    pub fn register_font_loader<L: AssetLoader<Asset = Font> + 'static>(
        &mut self,
        extensions: &[&str],
        loader: L,
    ) {
        self.custom_font_loaders.push(CustomLoaderEntry {
            extensions: extensions.iter().map(|s| s.to_string()).collect(),
            loader: Arc::new(FontLoaderAdapter(loader)),
        });
    }

    /// resolve the texture loader for a path — custom loaders take priority over built-ins.
    fn resolve_texture_loader(&self, path: &str) -> Arc<dyn TextureLoaderTrait> {
        let ext = Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        for entry in &self.custom_texture_loaders {
            if entry.extensions.iter().any(|e| e.as_str() == ext) {
                return Arc::clone(&entry.loader);
            }
        }
        texture_loader_for(path)
    }

    /// resolve the sound loader for a path — custom loaders take priority over built-ins.
    fn resolve_sound_loader(&self, path: &str) -> Arc<dyn SoundLoaderTrait> {
        let ext = Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        for entry in &self.custom_sound_loaders {
            if entry.extensions.iter().any(|e| e.as_str() == ext) {
                return Arc::clone(&entry.loader);
            }
        }
        sound_loader_for(path)
    }

    /// resolve the font loader for a path — custom loaders take priority over built-ins.
    fn resolve_font_loader(&self, path: &str) -> Arc<dyn FontLoaderTrait> {
        let ext = Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        for entry in &self.custom_font_loaders {
            if entry.extensions.iter().any(|e| e.as_str() == ext) {
                return Arc::clone(&entry.loader);
            }
        }
        font_loader_for(path)
    }

    /// load a texture, returns immediately with a handle.
    ///
    /// the texture loads asynchronously in the background.
    /// use [`is_texture_ready`](Self::is_texture_ready) to check when it's usable.
    pub fn load_texture(&mut self, path: &str) -> Handle<Texture> {
        let resolved = resolve_asset_path(path);
        let handle = self.texture_store.allocate_slot(resolved.clone());
        let loader = self.resolve_texture_loader(&resolved);
        self.io_pool.load_texture(resolved, handle.id(), loader);
        handle
    }

    /// load a sound, returns immediately with a handle.
    ///
    /// the sound loads asynchronously in the background.
    /// use [`is_sound_ready`](Self::is_sound_ready) to check when it's usable.
    pub fn load_sound(&mut self, path: &str) -> Handle<Sound> {
        let resolved = resolve_asset_path(path);
        let handle = self.sound_store.allocate_slot(resolved.clone());
        let loader = self.resolve_sound_loader(&resolved);
        self.io_pool.load_sound(resolved, handle.id(), loader);
        handle
    }

    /// load a font, returns immediately with a handle.
    ///
    /// the font loads asynchronously in the background.
    /// use [`is_font_ready`](Self::is_font_ready) to check when it's usable.
    pub fn load_font(&mut self, path: &str) -> Handle<Font> {
        let resolved = resolve_asset_path(path);
        let handle = self.font_store.allocate_slot(resolved.clone());
        let loader = self.resolve_font_loader(&resolved);
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

    /// block the calling thread until all pending asset loads complete.
    ///
    /// intended for tests and one-shot init code only — not available on WASM
    /// (no threads to block). in a running game, prefer polling [`loading_count`](Self::loading_count).
    #[cfg(not(target_arch = "wasm32"))]
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
///
/// only emitted on native targets — not available on WASM.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone)]
pub struct AssetChangedEvent {
    /// the file path that changed.
    pub path: String,
}

/// asset type tag for dispatching hot-reload events.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone, PartialEq)]
pub enum AssetType {
    Texture,
    Sound,
    Font,
}

/// watches an asset directory for file changes and logs them.
///
/// only available on native targets — `notify` does not support WASM.
/// event routing into the ECS is a planned feature; currently changes
/// are logged via [`log::info`] for debugging.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Resource)]
pub struct AssetWatcher {
    #[allow(dead_code)]
    watcher: Option<notify::RecommendedWatcher>,
    watched: std::collections::HashMap<String, AssetType>,
}

#[cfg(not(target_arch = "wasm32"))]
impl AssetWatcher {
    /// create a new asset watcher that recursively watches the given directory.
    pub fn new(watch_dir: &str) -> Self {
        use notify::{RecursiveMode, Watcher as _};
        // TODO: route events into ECS via a channel instead of just logging.
        let mut watcher: Option<notify::RecommendedWatcher> = None;
        if let Ok(mut w) =
            notify::recommended_watcher(|res: Result<notify::Event, notify::Error>| {
                if let Ok(event) = res {
                    for path in event.paths {
                        if let Some(p) = path.to_str() {
                            log::info!("asset changed: {p}");
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

    /// register a specific path to track for reload dispatch.
    pub fn watch(&mut self, path: &str, asset_type: AssetType) {
        self.watched.insert(path.to_string(), asset_type);
    }

    /// list all registered watch paths.
    pub fn watched_paths(&self) -> Vec<&str> {
        self.watched.keys().map(|s| s.as_str()).collect()
    }
}

/// asset watcher plugin — registers [`AssetWatcher`] as a resource.
///
/// only available on native targets. add this plugin during development
/// to get file-change logs for assets in the `assets/` directory.
#[cfg(not(target_arch = "wasm32"))]
pub struct AssetWatcherPlugin;

#[cfg(not(target_arch = "wasm32"))]
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
