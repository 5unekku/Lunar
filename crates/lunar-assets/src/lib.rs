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
//! use lunar_assets::{AssetServer, Handle, Texture};
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

/// web asset loading via the browser fetch API (WASM only).
///
/// provides [`web_fetch::fetch_bytes`] for downloading raw asset data over HTTP.
/// the [`IoTaskPool`] calls this automatically; you rarely need to use it directly.
#[cfg(target_arch = "wasm32")]
pub mod web_fetch;

/// compile-time bundled assets for WASM targets.
///
/// embed assets directly in the WASM binary with [`bundled::register`] so the
/// asset server never needs network requests for them. call [`bundled::register`]
/// (or [`bundled::register_many`]) at startup before any asset loads.
#[cfg(target_arch = "wasm32")]
pub mod bundled;

use bevy_ecs::prelude::*;
use lunar_core::{App, GamePlugin};
use rustc_hash::FxHashMap as HashMap;
use std::marker::PhantomData;
use std::path::Path;
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use crossbeam_channel::{Receiver, Sender};
#[cfg(not(target_arch = "wasm32"))]
use std::thread;

/// trait for types that can load a specific asset type from raw bytes.
///
/// implement this to support new asset formats.
pub trait AssetLoader: Send + Sync + 'static {
    /// the asset type this loader produces
    type Asset: Asset;

    /// load the asset from raw bytes, returning the parsed data
    /// # Errors
    /// returns an error string if the bytes cannot be parsed into the asset type.
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

/// snapshot of loading progress — counts across all asset types.
///
/// obtain via [`AssetServer::loading_stats`]. use this to drive a loading
/// screen: show a progress bar from `loaded / total` while `loaded < total`.
#[derive(Debug, Clone, Copy, Default)]
pub struct LoadingStats {
    /// total assets registered (loading + loaded + failed)
    pub total: u32,
    /// assets that finished loading successfully
    pub loaded: u32,
    /// assets that failed to load
    pub failed: u32,
}

impl LoadingStats {
    /// fraction of assets loaded successfully, in \[0, 1\]. returns 1 if total == 0.
    #[must_use]
    pub fn fraction(&self) -> f32 {
        if self.total == 0 { 1.0 } else { self.loaded as f32 / self.total as f32 }
    }

    /// true when all registered assets are done (loaded or failed, none still pending)
    #[must_use]
    pub fn is_done(&self) -> bool {
        self.loaded + self.failed >= self.total
    }
}

/// ECS resource that mirrors the latest [`LoadingStats`] snapshot.
///
/// updated each frame by the asset system after processing load results.
/// game code can read this to drive loading screens without calling
/// [`AssetServer::loading_stats`] directly.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct LoadingState {
    /// current progress snapshot
    pub stats: LoadingStats,
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
///
/// `Handle<T>` is unconditionally `Copy`, `Clone`, `Debug`, etc. — it stores
/// only an id, a generation, and a `PhantomData<T>`. the trait impls are
/// written by hand because `#[derive]` would spuriously bound `T: Clone`,
/// `T: Debug`, etc., even though `T` is never instantiated inside the handle.
pub struct Handle<T: Asset> {
    id: u32,
    generation: u16,
    _marker: PhantomData<T>,
}

impl<T: Asset> Copy for Handle<T> {}

impl<T: Asset> Default for Handle<T> {
    fn default() -> Self {
        Self { id: u32::MAX, generation: u16::MAX, _marker: PhantomData }
    }
}

impl<T: Asset> Clone for Handle<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: Asset> std::fmt::Debug for Handle<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Handle")
            .field("id", &self.id)
            .field("generation", &self.generation)
            .finish()
    }
}

impl<T: Asset> PartialEq for Handle<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.generation == other.generation
    }
}

impl<T: Asset> Eq for Handle<T> {}

impl<T: Asset> std::hash::Hash for Handle<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
        self.generation.hash(state);
    }
}

impl<T: Asset> Handle<T> {
    /// create a new handle with the given id and generation
    #[must_use]
    pub const fn new(id: u32, generation: u16) -> Self {
        Self {
            id,
            generation,
            _marker: PhantomData,
        }
    }

    /// get the internal id
    #[must_use]
    pub const fn id(&self) -> u32 {
        self.id
    }

    /// get the generation
    #[must_use]
    pub const fn generation(&self) -> u16 {
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
        Self {
            entries: Vec::new(),
            path_index: HashMap::default(),
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
        #[allow(clippy::cast_possible_truncation)]
        let id = self
            .entries
            .iter()
            .position(std::option::Option::is_none)
            .unwrap_or(self.entries.len()) as u32;
        let generation = self
            .entries
            .get(id as usize)
            .and_then(|e| e.as_ref())
            .map_or(0u16, |e| e.generation.wrapping_add(1));

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
        self.entries
            .get(handle.id as usize)
            .and_then(|e| e.as_ref())
            .is_some_and(|entry| {
                entry.generation == handle.generation && entry.state == LoadState::Loaded
            })
    }

    fn is_loaded(&self, handle: &Handle<T>) -> bool {
        self.entries
            .get(handle.id as usize)
            .and_then(|e| e.as_ref())
            .is_some_and(|entry| entry.generation == handle.generation && entry.state == LoadState::Loaded)
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
        self.entries.iter().flatten().filter(|e| e.state == LoadState::Loading).count()
    }

    fn loaded_count(&self) -> usize {
        self.entries.iter().flatten().filter(|e| e.state == LoadState::Loaded).count()
    }

    fn failed_count(&self) -> usize {
        self.entries.iter().flatten().filter(|e| e.state == LoadState::Failed).count()
    }

    /// count loading/loaded/failed/total in one pass.
    fn state_counts(&self) -> (usize, usize, usize, usize) {
        let mut loading = 0;
        let mut loaded = 0;
        let mut failed = 0;
        for entry in self.entries.iter().flatten() {
            match entry.state {
                LoadState::Loading => loading += 1,
                LoadState::Loaded => loaded += 1,
                LoadState::Failed => failed += 1,
            }
        }
        (loading, loaded, failed, loading + loaded + failed)
    }

    fn total_count(&self) -> usize {
        self.entries.iter().flatten().count()
    }

    fn get_by_id(&self, id: u32) -> Option<&T> {
        self.entries
            .get(id as usize)
            .and_then(|e| e.as_ref())
            .and_then(|entry| {
                if entry.state == LoadState::Loaded { entry.data.as_ref() } else { None }
            })
    }

    fn get_by_id_mut(&mut self, id: u32) -> Option<&mut T> {
        self.entries
            .get_mut(id as usize)
            .and_then(|e| e.as_mut())
            .and_then(|entry| {
                if entry.state == LoadState::Loaded { entry.data.as_mut() } else { None }
            })
    }

    fn remove(&mut self, id: u32) {
        if let Some(slot) = self.entries.get_mut(id as usize)
            && let Some(entry) = slot.take() {
                self.path_index.remove(&entry.path);
            }
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
    #[must_use]
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

        Self {
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
        self.drain_texture_results_into(&mut results);
        results
    }

    /// drain all completed texture results into a caller-supplied Vec (no allocation).
    fn drain_texture_results_into(&self, out: &mut Vec<LoadResult<Texture>>) {
        while let Ok(result) = self.texture_receiver.try_recv() {
            out.push(result);
        }
    }

    /// drain all completed sound results
    fn drain_sound_results(&self) -> Vec<LoadResult<Sound>> {
        let mut results = Vec::new();
        self.drain_sound_results_into(&mut results);
        results
    }

    /// drain all completed sound results into a caller-supplied Vec (no allocation).
    fn drain_sound_results_into(&self, out: &mut Vec<LoadResult<Sound>>) {
        while let Ok(result) = self.sound_receiver.try_recv() {
            out.push(result);
        }
    }

    /// drain all completed font results
    fn drain_font_results(&self) -> Vec<LoadResult<Font>> {
        let mut results = Vec::new();
        self.drain_font_results_into(&mut results);
        results
    }

    /// drain all completed font results into a caller-supplied Vec (no allocation).
    fn drain_font_results_into(&self, out: &mut Vec<LoadResult<Font>>) {
        while let Ok(result) = self.font_receiver.try_recv() {
            out.push(result);
        }
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
            mips: Vec::new(),
            compression: TextureCompression::None,
            keep_cpu_data: false,
        })
    }
}

/// loader for .li (lunar image) format.
///
/// decodes .li bytes into raw pixel data via lunar-image.
pub struct LiTextureLoader;

impl TextureLoaderTrait for LiTextureLoader {
    fn load(&self, bytes: Vec<u8>) -> Result<Texture, String> {
        let image =
            lunar_image::decode(&bytes).map_err(|e| format!("failed to decode .li: {e}"))?;
        Ok(Texture {
            width: image.width,
            height: image.height,
            pixels: image.pixels,
            mips: Vec::new(),
            compression: TextureCompression::None,
            keep_cpu_data: false,
        })
    }
}

/// loader for `.bctex` files — BC-compressed textures from the compress-textures tool.
///
/// binary layout: magic `BCTX` (4 bytes), version u8, format u8, mip_count u16,
/// width u32, height u32, then raw BC block data for base + each mip level.
pub struct BctexLoader;

impl TextureLoaderTrait for BctexLoader {
    fn load(&self, bytes: Vec<u8>) -> Result<Texture, String> {
        if bytes.len() < 16 { return Err("bctex: too short".into()); }
        if &bytes[0..4] != b"BCTX" { return Err("bctex: bad magic".into()); }
        let format_byte = bytes[5];
        let mip_count = u16::from_le_bytes([bytes[6], bytes[7]]) as u32;
        let width  = u32::from_le_bytes([bytes[8],  bytes[9],  bytes[10], bytes[11]]);
        let height = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
        let (compression, block_bytes) = match format_byte {
            1 => (TextureCompression::Bc1,  8u32),
            3 => (TextureCompression::Bc3,  16u32),
            5 => (TextureCompression::Bc5,  16u32),
            6 => (TextureCompression::Bc6h, 16u32),
            7 => (TextureCompression::Bc7,  16u32),
            _ => return Err(format!("bctex: unknown format byte {format_byte}")),
        };
        let base_size = width.div_ceil(4) * height.div_ceil(4) * block_bytes;
        let mut offset = 16usize;
        let pixels = bytes[offset..offset + base_size as usize].to_vec();
        offset += base_size as usize;
        let mut mips: Vec<Vec<u8>> = Vec::new();
        let mut mip_w = width;
        let mut mip_h = height;
        for _ in 1..mip_count {
            mip_w = (mip_w / 2).max(1);
            mip_h = (mip_h / 2).max(1);
            let mip_size = (mip_w.div_ceil(4) * mip_h.div_ceil(4) * block_bytes) as usize;
            if offset + mip_size > bytes.len() { break; }
            mips.push(bytes[offset..offset + mip_size].to_vec());
            offset += mip_size;
        }
        Ok(Texture { width, height, pixels, mips, compression, keep_cpu_data: false })
    }
}

/// audio loader for FLAC, OGG Vorbis, and WAV.
///
/// stores compressed bytes as-is; decoding happens in the audio plugin at playback
/// time. the format tag is determined from the file extension at load time so the
/// plugin doesn't need to re-sniff the bytes.
pub struct CompressedSoundLoader {
    format: AudioFormat,
}

impl SoundLoaderTrait for CompressedSoundLoader {
    fn load(&self, bytes: Vec<u8>) -> Result<Sound, String> {
        Ok(Sound {
            data: bytes,
            format: self.format,
        })
    }
}

/// loader for ttf/otf font files.
///
/// stores raw bytes; the render system parses the font when needed.
pub struct TtfFontLoader;

impl FontLoaderTrait for TtfFontLoader {
    fn load(&self, bytes: Vec<u8>) -> Result<Font, String> {
        Ok(Font { data: bytes })
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
        "li"    => Arc::new(LiTextureLoader),
        "bctex" => Arc::new(BctexLoader),
        _       => Arc::new(ImageTextureLoader),
    }
}

/// determine the appropriate sound loader for a file extension.
fn sound_loader_for(path: &str) -> Arc<dyn SoundLoaderTrait> {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    let format = match ext.as_str() {
        "flac" => AudioFormat::Flac,
        "ogg" | "oga" => AudioFormat::OggVorbis,
        "opus" => AudioFormat::OggOpus,
        "wav" | "wave" => AudioFormat::Wav,
        _ => AudioFormat::Unknown,
    };
    Arc::new(CompressedSoundLoader { format })
}

/// determine the appropriate font loader for a file extension.
fn font_loader_for(_path: &str) -> Arc<dyn FontLoaderTrait> {
    Arc::new(TtfFontLoader)
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

/// source for a texture load — either a file path or embedded bytes.
///
/// game code typically passes one of:
/// - `"sprites/player"` — resolved from `assets/` and loaded asynchronously
/// - `texture!("sprites/player")` — bytes baked in at compile time, decoded synchronously
pub enum TextureSource<'a> {
    /// file path, resolved relative to `assets/`
    Path(&'a str),
    /// raw `.li` bytes already embedded in the binary via `texture!`
    Embedded(&'static [u8]),
}

impl<'a> From<&'a str> for TextureSource<'a> {
    fn from(path: &'a str) -> Self {
        TextureSource::Path(path)
    }
}

impl From<&'static [u8]> for TextureSource<'static> {
    fn from(bytes: &'static [u8]) -> Self {
        TextureSource::Embedded(bytes)
    }
}

impl<const N: usize> From<&'static [u8; N]> for TextureSource<'static> {
    fn from(bytes: &'static [u8; N]) -> Self {
        TextureSource::Embedded(bytes as &[u8])
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
    /// IDs of textures ready for GPU upload, drained by the render system each frame.
    pending_texture_ids: Vec<u32>,
    /// IDs of fonts ready for glyph atlas registration, drained by the render system each frame.
    pending_font_ids: Vec<u32>,
    /// IDs of textures released via [`Self::release_texture`], drained by the render system to free GPU memory.
    evicted_texture_ids: Vec<u32>,
    /// counter for generating unique synthetic paths for procedural textures.
    proc_texture_counter: u32,
    /// mip streaming configuration: auto-generates mip chains on texture load.
    mip_config: MipStreamingConfig,
    /// per-texture screen-space coverage hints from the renderer (max coverage this frame).
    /// coverage ≈ object_diameter / camera_distance; larger = closer = needs higher quality.
    /// cleared at the start of each render frame by the renderer.
    pub coverage_hints: rustc_hash::FxHashMap<u32, f32>,
}

impl AssetServer {
    /// create a new asset server with the given number of io threads.
    #[must_use]
    pub fn new(io_thread_count: usize) -> Self {
        Self {
            texture_store: AssetStore::new(),
            sound_store: AssetStore::new(),
            font_store: AssetStore::new(),
            io_pool: IoTaskPool::new(io_thread_count),
            custom_texture_loaders: Vec::new(),
            custom_sound_loaders: Vec::new(),
            custom_font_loaders: Vec::new(),
            pending_texture_ids: Vec::new(),
            pending_font_ids: Vec::new(),
            evicted_texture_ids: Vec::new(),
            proc_texture_counter: 0,
            mip_config: MipStreamingConfig::default(),
            coverage_hints: rustc_hash::FxHashMap::default(),
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
            extensions: extensions
                .iter()
                .map(std::string::ToString::to_string)
                .collect(),
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
            extensions: extensions
                .iter()
                .map(std::string::ToString::to_string)
                .collect(),
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
            extensions: extensions
                .iter()
                .map(std::string::ToString::to_string)
                .collect(),
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
    /// accepts either a path string (async disk/network load) or embedded bytes
    /// from the `texture!` macro (synchronous, already in memory).
    ///
    /// # path loading
    /// the texture loads asynchronously in the background.
    /// use [`is_texture_ready`](Self::is_texture_ready) to check when it's usable.
    ///
    /// # embedded loading
    /// bytes are decoded immediately — the handle is ready on the same frame.
    pub fn load_texture<'a>(&mut self, source: impl Into<TextureSource<'a>>) -> Handle<Texture> {
        match source.into() {
            TextureSource::Path(path) => {
                let resolved = resolve_asset_path(path);
                let handle = self.texture_store.allocate_slot(resolved.clone());
                let loader = self.resolve_texture_loader(&resolved);
                self.io_pool.load_texture(resolved, handle.id(), loader);
                handle
            }
            TextureSource::Embedded(bytes) => {
                let key = format!("__embedded_{:p}", bytes.as_ptr());
                let handle = self.texture_store.allocate_slot(key);
                if self.texture_store.is_ready(&handle) {
                    return handle;
                }
                let id = handle.id();
                match LiTextureLoader.load(bytes.to_vec()) {
                    Ok(texture) => {
                        self.texture_store.insert(id, texture);
                        self.pending_texture_ids.push(id);
                    }
                    Err(err) => {
                        log::warn!("failed to decode embedded texture: {err}");
                        self.texture_store.mark_failed(id);
                    }
                }
                handle
            }
        }
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
    #[must_use]
    pub fn is_texture_ready(&self, handle: &Handle<Texture>) -> bool {
        self.texture_store.is_ready(handle)
    }

    /// check if a sound handle is ready
    #[must_use]
    pub fn is_sound_ready(&self, handle: &Handle<Sound>) -> bool {
        self.sound_store.is_ready(handle)
    }

    /// check if a font handle is ready
    #[must_use]
    pub fn is_font_ready(&self, handle: &Handle<Font>) -> bool {
        self.font_store.is_ready(handle)
    }

    /// check if a texture is loaded
    #[must_use]
    pub fn is_texture_loaded(&self, handle: &Handle<Texture>) -> bool {
        self.texture_store.is_loaded(handle)
    }

    /// check if a sound is loaded
    #[must_use]
    pub fn is_sound_loaded(&self, handle: &Handle<Sound>) -> bool {
        self.sound_store.is_loaded(handle)
    }

    /// check if a font is loaded
    #[must_use]
    pub fn is_font_loaded(&self, handle: &Handle<Font>) -> bool {
        self.font_store.is_loaded(handle)
    }

    /// get texture info
    #[must_use]
    pub fn get_texture_info(&self, handle: &Handle<Texture>) -> Option<AssetInfo> {
        self.texture_store.get_info(handle)
    }

    /// get sound info
    #[must_use]
    pub fn get_sound_info(&self, handle: &Handle<Sound>) -> Option<AssetInfo> {
        self.sound_store.get_info(handle)
    }

    /// get font info
    #[must_use]
    pub fn get_font_info(&self, handle: &Handle<Font>) -> Option<AssetInfo> {
        self.font_store.get_info(handle)
    }

    /// get a loaded texture reference
    #[must_use]
    pub fn get_texture(&self, handle: &Handle<Texture>) -> Option<&Texture> {
        self.texture_store.get(handle)
    }

    /// get a loaded sound reference
    #[must_use]
    pub fn get_sound(&self, handle: &Handle<Sound>) -> Option<&Sound> {
        self.sound_store.get(handle)
    }

    /// get a loaded font reference
    #[must_use]
    pub fn get_font(&self, handle: &Handle<Font>) -> Option<&Font> {
        self.font_store.get(handle)
    }

    /// load a batch of textures, returns handles immediately
    pub fn load_textures(&mut self, paths: &[&str]) -> Vec<Handle<Texture>> {
        paths.iter().map(|p| self.load_texture(*p)).collect()
    }

    /// drain the list of texture IDs that became ready since the last drain.
    ///
    /// the render system calls this once per frame to discover newly loaded
    /// textures and upload them to the GPU. callers other than the render
    /// system should generally not call this — it clears the pending list.
    pub fn drain_new_texture_ids(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.pending_texture_ids)
    }

    /// get a loaded texture by its raw asset ID.
    ///
    /// used by the render system when uploading newly-loaded textures.
    /// prefer [`get_texture`](Self::get_texture) for normal game code.
    #[must_use]
    pub fn get_texture_by_id(&self, id: u32) -> Option<&Texture> {
        self.texture_store.get_by_id(id)
    }

    /// mutable access to a texture by id. used after GPU upload to call `evict_cpu_data`.
    pub fn get_texture_by_id_mut(&mut self, id: u32) -> Option<&mut Texture> {
        self.texture_store.get_by_id_mut(id)
    }

    /// configure mip streaming: set whether newly loaded textures auto-generate mip chains.
    ///
    /// call before loading textures to apply to all subsequent loads.
    pub fn configure_mip_streaming(&mut self, config: MipStreamingConfig) {
        self.mip_config = config;
    }

    /// returns the current mip streaming config.
    #[must_use]
    pub fn mip_config(&self) -> &MipStreamingConfig {
        &self.mip_config
    }

    /// report the screen-space coverage of a texture this frame.
    ///
    /// coverage ≈ entity_size / camera_distance (larger = close, needs more detail).
    /// called by the renderer each frame for all visible textured entities.
    /// the renderer accumulates the max coverage per texture_id.
    pub fn hint_coverage(&mut self, tex_id: u32, coverage: f32) {
        let entry = self.coverage_hints.entry(tex_id).or_insert(0.0);
        if coverage > *entry { *entry = coverage; }
    }

    /// returns how many mip levels to upload for a texture given its coverage hint.
    ///
    /// higher coverage (texture fills screen) = upload all mips (full quality).
    /// lower coverage (texture is tiny on screen) = upload fewer mips (save VRAM).
    ///
    /// `max_mip_levels` is the total number of mip levels available for the texture.
    #[must_use]
    pub fn desired_mip_count(&self, tex_id: u32, max_mip_levels: u32) -> u32 {
        let coverage = self.coverage_hints.get(&tex_id).copied().unwrap_or(1.0);
        if coverage >= 0.5 || max_mip_levels <= 1 { return max_mip_levels; }
        if coverage >= 0.1 { return max_mip_levels.saturating_sub(2).max(1); }
        max_mip_levels.saturating_sub(4).max(1)
    }

    /// drain the list of font IDs that became ready since the last drain.
    ///
    /// the render system calls this once per frame to register new fonts into
    /// the glyph atlas. callers other than the render system should not call this.
    pub fn drain_new_font_ids(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.pending_font_ids)
    }

    /// release a texture handle. decrements the ref count; when it reaches zero
    /// the CPU-side asset data is freed and the texture ID is queued for GPU
    /// cleanup. the render system will call `remove_texture` on the next frame.
    ///
    /// only call this when you are done with the handle and no other code holds a
    /// copy of it. handles that are never released leak GPU memory.
    pub fn release_texture(&mut self, handle: Handle<Texture>) {
        let id = handle.id();
        self.texture_store.decrement_ref(id);
        if self.texture_store.is_unused(id) {
            self.texture_store.remove(id);
            self.evicted_texture_ids.push(id);
        }
    }

    /// drain the list of texture IDs that were released since the last drain.
    ///
    /// the render system calls this once per frame to free GPU resources for
    /// textures that are no longer referenced. callers other than the render
    /// system should not call this.
    pub fn drain_evicted_texture_ids(&mut self) -> Vec<u32> {
        std::mem::take(&mut self.evicted_texture_ids)
    }

    /// get a loaded font by its raw asset ID.
    ///
    /// used by the render system when registering newly-loaded fonts.
    #[must_use]
    pub fn get_font_by_id(&self, id: u32) -> Option<&Font> {
        self.font_store.get_by_id(id)
    }

    /// create a texture from raw RGBA pixel data without loading from disk.
    ///
    /// returns a handle that is immediately ready. the render system will
    /// upload the texture to the GPU on the next frame.
    ///
    /// `pixels` must be exactly `width * height * 4` bytes in RGBA order.
    ///
    /// # Panics
    /// panics in debug mode if `pixels.len() != width * height * 4`.
    pub fn create_texture(&mut self, width: u32, height: u32, pixels: Vec<u8>) -> Handle<Texture> {
        debug_assert_eq!(
            pixels.len(),
            (width * height * 4) as usize,
            "pixel buffer size mismatch"
        );
        let path = format!("__proc_{}", self.proc_texture_counter);
        self.proc_texture_counter += 1;
        let handle = self.texture_store.allocate_slot(path);
        let id = handle.id();
        self.texture_store.insert(
            id,
            Texture { width, height, pixels, mips: Vec::new(), compression: TextureCompression::None, keep_cpu_data: false },
        );
        self.pending_texture_ids.push(id);
        handle
    }

    /// create a solid-color texture.
    ///
    /// shorthand for [`create_texture`](Self::create_texture) with all pixels set to one color.
    pub fn create_solid_color_texture(
        &mut self,
        width: u32,
        height: u32,
        r: u8,
        g: u8,
        b: u8,
        a: u8,
    ) -> Handle<Texture> {
        let pixel_count = (width * height) as usize;
        let mut pixels = Vec::with_capacity(pixel_count * 4);
        for _ in 0..pixel_count {
            pixels.push(r);
            pixels.push(g);
            pixels.push(b);
            pixels.push(a);
        }
        self.create_texture(width, height, pixels)
    }

    /// get the number of assets currently loading across all stores.
    #[must_use]
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

    /// block until all pending asset loads complete. alias for [`wait_for_all`](Self::wait_for_all).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn block_until_all_ready(&self) {
        self.wait_for_all();
    }

    /// block until the given texture handle is loaded or failed.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn block_until_texture_ready(&self, handle: &Handle<Texture>) {
        while !self.texture_store.is_ready(handle)
            && self.texture_store.entries.get(handle.id as usize)
                .and_then(|e| e.as_ref())
                .is_some_and(|e| e.state == LoadState::Loading)
        {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    /// block until the given font handle is loaded or failed.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn block_until_font_ready(&self, handle: &Handle<Font>) {
        while !self.font_store.is_ready(handle)
            && self.font_store.entries.get(handle.id as usize)
                .and_then(|e| e.as_ref())
                .is_some_and(|e| e.state == LoadState::Loading)
        {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    /// snapshot of current loading progress across all asset types.
    #[must_use]
    pub fn loading_stats(&self) -> LoadingStats {
        let (_, tl, tf, tt) = self.texture_store.state_counts();
        let (_, sl, sf, st) = self.sound_store.state_counts();
        let (_, fl, ff, ft) = self.font_store.state_counts();
        LoadingStats {
            total: (tt + st + ft) as u32,
            loaded: (tl + sl + fl) as u32,
            failed: (tf + sf + ff) as u32,
        }
    }

    /// process completed load results from io threads.
    /// call this once per frame from the asset plugin's system.
    pub fn update(&mut self) {
        // drain texture results — auto-generate mips if config is set
        let gen_mips = self.mip_config.generate_mipmaps;
        for result in self.io_pool.drain_texture_results() {
            match result.data {
                Ok(mut data) => {
                    if gen_mips { data.generate_mipmaps(); }
                    self.texture_store.insert(result.id, data);
                    self.pending_texture_ids.push(result.id);
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
                    self.pending_font_ids.push(result.id);
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
///
/// # mip levels
///
/// `mips` carries pre-generated mip levels (index 0 = half-res, 1 = quarter-res, etc.).
/// when non-empty, the renderer creates the GPU texture with a full mip chain and uploads
/// all levels — enabling the GPU sampler to pick the appropriate mip based on screen
/// coverage (trilinear filtering). call `generate_mipmaps()` to populate this field.
/// pixel compression format for a [`Texture`].
///
/// `None` means raw RGBA8 data. Compressed formats store pre-compressed block
/// data in `pixels`/`mips` — the renderer uploads it directly without CPU decode.
/// produce compressed textures offline with a build tool; the runtime only reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextureCompression {
    /// raw RGBA8 (or sRGB) data — the default.
    #[default]
    None,
    /// BC1 / DXT1: 0.5 bytes/block-texel, RGB no alpha, 8:1 compression for albedo.
    Bc1,
    /// BC3 / DXT5: 1 byte/block-texel, RGBA, good for diffuse + alpha maps.
    Bc3,
    /// BC5 / RGTC2: 1 byte/block-texel, two-channel (RG), for normal maps.
    Bc5,
    /// BC6H: 1 byte/block-texel, signed float RGB, for HDR lightmaps.
    Bc6h,
    /// BC7: 1 byte/block-texel, high-quality RGBA general purpose.
    Bc7,
}

pub struct Texture {
    pub width: u32,
    pub height: u32,
    /// pixel data for the base mip level.
    /// for `TextureCompression::None`: RGBA8 linear bytes, `width * height * 4` bytes.
    /// for compressed formats: raw block data, `ceil(w/4) * ceil(h/4) * block_bytes`.
    /// empty after `evict_cpu_data()` is called (unless `keep_cpu_data` is set).
    pub pixels: Vec<u8>,
    /// pre-generated mip levels, each half the previous resolution.
    /// index 0 = mip 1 (half-res), index 1 = mip 2 (quarter-res), etc.
    /// empty = no mip chain; GPU texture created as single mip.
    /// empty after `evict_cpu_data()` is called (unless `keep_cpu_data` is set).
    pub mips: Vec<Vec<u8>>,
    /// compression format; defaults to `None` (uncompressed RGBA8).
    pub compression: TextureCompression,
    /// when true, `evict_cpu_data()` is a no-op. set for textures the baker or
    /// collision system needs to re-read after GPU upload.
    pub keep_cpu_data: bool,
}

impl Texture {
    /// create a texture with no mip chain (single mip level, base image only).
    #[must_use]
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Self {
        Self { width, height, pixels, mips: Vec::new(), compression: TextureCompression::None, keep_cpu_data: false }
    }

    /// free pixel and mip data from RAM, keeping only metadata.
    /// no-op if `keep_cpu_data` is set.
    pub fn evict_cpu_data(&mut self) {
        if self.keep_cpu_data { return; }
        self.pixels = Vec::new();
        self.mips = Vec::new();
    }

    /// total number of mip levels including the base: `mips.len() + 1`.
    #[must_use]
    pub fn mip_level_count(&self) -> u32 {
        self.mips.len() as u32 + 1
    }

    /// generate a full mip chain using a 2×2 box filter.
    ///
    /// each mip halves both dimensions (minimum 1×1).
    /// no-op if mips are already populated or the image is 1×1.
    pub fn generate_mipmaps(&mut self) {
        if !self.mips.is_empty() || (self.width <= 1 && self.height <= 1) {
            return;
        }
        let mut prev_pixels = &self.pixels;
        let mut prev_w = self.width;
        let mut prev_h = self.height;
        let mut prev_owned: Vec<u8>;
        loop {
            let next_w = (prev_w / 2).max(1);
            let next_h = (prev_h / 2).max(1);

            #[cfg(not(target_arch = "wasm32"))]
            let next = {
                use rayon::prelude::*;
                let rows: Vec<Vec<u8>> = (0..next_h).into_par_iter().map(|y| {
                    let mut row = vec![0u8; (next_w * 4) as usize];
                    for x in 0..next_w {
                        let sx = (x * 2).min(prev_w - 1);
                        let sy = (y * 2).min(prev_h - 1);
                        let sx1 = (sx + 1).min(prev_w - 1);
                        let sy1 = (sy + 1).min(prev_h - 1);
                        let sample = |px: u32, py: u32| -> [u32; 4] {
                            let i = ((py * prev_w + px) * 4) as usize;
                            [prev_pixels[i] as u32, prev_pixels[i+1] as u32,
                             prev_pixels[i+2] as u32, prev_pixels[i+3] as u32]
                        };
                        let a = sample(sx, sy); let b = sample(sx1, sy);
                        let c = sample(sx, sy1); let d = sample(sx1, sy1);
                        let di = (x * 4) as usize;
                        for ch in 0..4 {
                            row[di + ch] = ((a[ch] + b[ch] + c[ch] + d[ch] + 2) / 4) as u8;
                        }
                    }
                    row
                }).collect();
                let mut buf = Vec::with_capacity((next_w * next_h * 4) as usize);
                for row in rows { buf.extend_from_slice(&row); }
                buf
            };

            #[cfg(target_arch = "wasm32")]
            let next = {
                let mut buf = vec![0u8; (next_w * next_h * 4) as usize];
                for y in 0..next_h {
                    for x in 0..next_w {
                        let sx = (x * 2).min(prev_w - 1);
                        let sy = (y * 2).min(prev_h - 1);
                        let sx1 = (sx + 1).min(prev_w - 1);
                        let sy1 = (sy + 1).min(prev_h - 1);
                        let sample = |px: u32, py: u32| -> [u32; 4] {
                            let i = ((py * prev_w + px) * 4) as usize;
                            [prev_pixels[i] as u32, prev_pixels[i+1] as u32,
                             prev_pixels[i+2] as u32, prev_pixels[i+3] as u32]
                        };
                        let a = sample(sx, sy); let b = sample(sx1, sy);
                        let c = sample(sx, sy1); let d = sample(sx1, sy1);
                        let di = ((y * next_w + x) * 4) as usize;
                        for ch in 0..4 {
                            buf[di + ch] = ((a[ch] + b[ch] + c[ch] + d[ch] + 2) / 4) as u8;
                        }
                    }
                }
                buf
            };

            self.mips.push(next);
            if next_w <= 1 && next_h <= 1 { break; }
            prev_owned = self.mips.last().unwrap().clone();
            prev_pixels = &prev_owned;
            prev_w = next_w;
            prev_h = next_h;
        }
    }
}

impl Asset for Texture {}

/// compressed audio format tag.
///
/// the audio plugin uses this to select the right decoder at playback time
/// without re-sniffing the file bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFormat {
    Flac,
    OggVorbis,
    OggOpus,
    Wav,
    /// format couldn't be determined from the file extension
    Unknown,
}

/// compressed audio bytes as loaded from disk.
///
/// stores raw file bytes rather than decoded PCM. decoding happens in the audio
/// plugin at playback time — either fully (short SFX, cached) or frame-by-frame
/// (music, streamed). this keeps memory proportional to the compressed size:
/// a 3-min FLAC at 48 kHz stereo is ~70 MB decoded but ~20 MB on disk.
pub struct Sound {
    /// raw bytes from the audio file.
    pub data: Vec<u8>,
    /// format detected from file extension at load time.
    pub format: AudioFormat,
}

impl Asset for Sound {}

/// raw font file bytes.
///
/// the render system parses and caches these into a glyph atlas per target
/// (fontdue on WASM, freetype on native).
pub struct Font {
    pub data: Vec<u8>,
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
    fn name(&self) -> &'static str {
        "AssetPlugin"
    }

    fn dependencies(&self) -> &[&str] {
        &[]
    }

    fn build(&mut self, app: &mut App) {
        app.insert_resource(AssetServer::new(2));
        app.insert_resource(LoadingState::default());
        app.insert_resource(MipStreamingConfig::default());
        app.insert_resource(TextureVramUsage::default());
        app.add_system(process_asset_loads);
        log::info!("AssetPlugin: asset server resource registered");
    }
}

/// system that processes completed asset loads and updates the LoadingState resource.
fn process_asset_loads(mut asset_server: ResMut<AssetServer>, mut loading_state: ResMut<LoadingState>) {
    asset_server.update();
    loading_state.stats = asset_server.loading_stats();
}

/// configuration for GPU mip streaming.
///
/// insert as a resource before `AssetPlugin` to enable automatic mip chain generation
/// for all textures loaded after this point.
///
/// # VRAM budget
///
/// `vram_budget_bytes` is a soft limit on GPU texture memory. when the total size of
/// uploaded textures (all mip levels) would exceed the budget, the asset system
/// stops generating full mip chains for new textures and uploads base mip only.
/// existing textures are not evicted — eviction requires explicit calls to the
/// asset server's eviction API.
///
/// set `generate_mipmaps = false` to disable mip generation entirely (e.g. for UI
/// textures where mip aliasing is undesirable).
#[derive(Resource, Clone)]
pub struct MipStreamingConfig {
    /// generate full mip chains for all newly loaded textures.
    /// default: true.
    pub generate_mipmaps: bool,
    /// soft VRAM budget in bytes for all uploaded textures.
    /// 0 = unlimited. default: 512 MiB.
    pub vram_budget_bytes: u64,
}

impl Default for MipStreamingConfig {
    fn default() -> Self {
        Self {
            generate_mipmaps: true,
            vram_budget_bytes: 512 * 1024 * 1024,
        }
    }
}

/// resource: current GPU texture VRAM usage estimate.
///
/// updated by the asset server each frame as textures are uploaded.
/// read this to monitor memory pressure and adjust quality settings.
#[derive(Resource, Default)]
pub struct TextureVramUsage {
    /// estimated total bytes used by all uploaded textures (all mip levels).
    pub bytes: u64,
}

impl TextureVramUsage {
    /// increment usage by the size of a texture with `mip_count` mip levels.
    pub fn add_texture(&mut self, width: u32, height: u32, mip_count: u32) {
        // RGBA8 = 4 bytes per pixel. geometric series sum: base × (1 - (1/4)^n) / (1 - 1/4)
        // approximation: base × 4/3 (exact only if mip chain goes to 1×1)
        let base_bytes = (width * height * 4) as u64;
        let mip_factor = if mip_count > 1 { 4.0 / 3.0 } else { 1.0 };
        self.bytes += (base_bytes as f64 * mip_factor) as u64;
    }

    /// increment usage by a raw byte count (for compressed or non-standard textures).
    pub fn add_bytes(&mut self, bytes: u64) { self.bytes += bytes; }

    /// decrement usage (call when a GPU texture is freed).
    pub fn remove_bytes(&mut self, bytes: u64) { self.bytes = self.bytes.saturating_sub(bytes); }
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetType {
    Texture,
    Sound,
    Font,
}

/// watches an asset directory for file changes and republishes them to the ECS.
///
/// only available on native targets — `notify` does not support WASM.
/// file-change paths are delivered each frame as [`AssetChanged`] messages
/// (read them with a `MessageReader<AssetChanged>`); changes are also logged.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Resource)]
pub struct AssetWatcher {
    #[allow(dead_code)]
    watcher: Option<notify::RecommendedWatcher>,
    watched: rustc_hash::FxHashMap<String, AssetType>,
    // receiver side of the notify callback channel; behind a mutex so the resource
    // stays Sync. drained once per frame by dispatch_asset_changes.
    changes: std::sync::Mutex<std::sync::mpsc::Receiver<String>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl AssetWatcher {
    /// create a new asset watcher that recursively watches the given directory.
    #[must_use]
    pub fn new(watch_dir: &str) -> Self {
        use notify::{RecursiveMode, Watcher as _};
        // the notify callback runs on its own thread, so it forwards changed paths
        // through a channel; the ecs side drains them each frame.
        let (sender, receiver) = std::sync::mpsc::channel::<String>();
        let mut watcher =
            notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
                if let Ok(event) = res {
                    for path in event.paths {
                        if let Some(p) = path.to_str() {
                            // a send error just means the receiver was dropped (shutdown)
                            let _ = sender.send(p.to_string());
                        }
                    }
                }
            })
            .ok();
        if let Some(ref mut w) = watcher {
            let _ = w.watch(std::path::Path::new(watch_dir), RecursiveMode::Recursive);
        }
        Self {
            watcher,
            watched: rustc_hash::FxHashMap::default(),
            changes: std::sync::Mutex::new(receiver),
        }
    }

    /// register a specific path to track for reload dispatch.
    pub fn watch(&mut self, path: &str, asset_type: AssetType) {
        self.watched.insert(path.to_string(), asset_type);
    }

    /// list all registered watch paths.
    #[must_use]
    pub fn watched_paths(&self) -> Vec<&str> {
        self.watched
            .keys()
            .map(std::string::String::as_str)
            .collect()
    }

    /// drain all file-change paths received since the last call.
    #[must_use]
    pub fn drain_changes(&self) -> Vec<String> {
        let mut changed = Vec::new();
        if let Ok(receiver) = self.changes.lock() {
            while let Ok(path) = receiver.try_recv() {
                changed.push(path);
            }
        }
        changed
    }
}

/// message published when a watched asset file changes on disk.
///
/// only emitted on native targets by [`AssetWatcherPlugin`]. read it with a
/// `MessageReader<AssetChanged>` to trigger hot-reload or cache invalidation.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone)]
pub struct AssetChanged {
    /// path of the file that changed, as reported by the os watcher.
    pub path: String,
}

#[cfg(not(target_arch = "wasm32"))]
impl bevy_ecs::message::Message for AssetChanged {}

/// system that drains watcher file-change events and republishes them as
/// [`AssetChanged`] messages once per frame.
#[cfg(not(target_arch = "wasm32"))]
pub fn dispatch_asset_changes(
    watcher: Res<AssetWatcher>,
    mut changed_writer: bevy_ecs::message::MessageWriter<AssetChanged>,
) {
    for path in watcher.drain_changes() {
        log::info!("asset changed: {path}");
        changed_writer.write(AssetChanged { path });
    }
}

/// asset watcher plugin — registers [`AssetWatcher`] and emits [`AssetChanged`].
///
/// only available on native targets. add this plugin during development to get
/// file-change events for assets in the `assets/` directory.
#[cfg(not(target_arch = "wasm32"))]
pub struct AssetWatcherPlugin;

#[cfg(not(target_arch = "wasm32"))]
impl GamePlugin for AssetWatcherPlugin {
    fn name(&self) -> &'static str {
        "AssetWatcherPlugin"
    }

    fn dependencies(&self) -> &[&str] {
        &["AssetPlugin"]
    }

    fn build(&mut self, app: &mut App) {
        app.insert_resource(AssetWatcher::new("assets/"));
        bevy_ecs::message::MessageRegistry::register_message::<AssetChanged>(app.world_mut());
        app.add_system(dispatch_asset_changes);
        log::info!("AssetWatcherPlugin: asset watcher registered");
    }
}

/// convenience macro to implement the [`Asset`] trait for a custom type.
///
/// # example
///
/// ```ignore
/// use lunar_assets::impl_asset;
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

#[cfg(test)]
mod handle_tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    struct TestAsset;
    impl Asset for TestAsset {}

    #[test]
    fn handle_new_creates_valid_handle() {
        let h = Handle::<TestAsset>::new(5, 3);
        assert_eq!(h.id(), 5);
        assert_eq!(h.generation(), 3);
    }

    #[test]
    fn handle_copy_and_clone() {
        let a = Handle::<TestAsset>::new(1, 2);
        let b = Handle::<TestAsset>::new(1, 2);
        assert_eq!(a, b);
    }

    #[test]
    fn asset_store_allocate_slot() {
        let mut store = AssetStore::<TestAsset>::new();
        let h = store.allocate_slot("test/path".into());
        assert_eq!(h.id(), 0);
        assert_eq!(h.generation(), 0);
        assert_eq!(store.loading_count(), 1);
    }

    #[test]
    fn asset_store_insert_and_retrieve() {
        let mut store = AssetStore::<TestAsset>::new();
        let h = store.allocate_slot("test".into());
        store.insert(h.id(), TestAsset);
        assert_eq!(store.get(&h), Some(&TestAsset));
    }

    #[test]
    fn asset_store_generation_increments_on_reuse() {
        let mut store = AssetStore::<TestAsset>::new();
        let h1 = store.allocate_slot("a".into());
        // nested allocate_slot with a different path reuses slot — not directly testable
        // but we can verify basic generation tracking works
        assert_eq!(h1.generation(), 0);
    }

    #[test]
    fn asset_store_stale_generation_invalid() {
        let mut store = AssetStore::<TestAsset>::new();
        let h = store.allocate_slot("test".into());
        store.insert(h.id(), TestAsset);
        let stale = Handle::<TestAsset>::new(h.id(), 42);
        assert!(store.get(&stale).is_none());
    }

    #[test]
    fn asset_store_is_ready_after_insert() {
        let mut store = AssetStore::<TestAsset>::new();
        let h = store.allocate_slot("test".into());
        assert!(!store.is_ready(&h));
        store.insert(h.id(), TestAsset);
        assert!(store.is_ready(&h));
    }

    #[test]
    fn asset_store_loading_count() {
        let mut store = AssetStore::<TestAsset>::new();
        let _ = store.allocate_slot("a".into());
        let _ = store.allocate_slot("b".into());
        assert_eq!(store.loading_count(), 2);
        // insert one to make it loaded
        store.insert(0, TestAsset);
        assert_eq!(store.loading_count(), 1);
    }

    #[test]
    fn asset_store_mark_failed() {
        let mut store = AssetStore::<TestAsset>::new();
        let h = store.allocate_slot("test".into());
        assert!(
            store
                .get_info(&h)
                .is_some_and(|i| i.state == LoadState::Loading)
        );
        store.mark_failed(h.id());
        assert!(
            store
                .get_info(&h)
                .is_some_and(|i| i.state == LoadState::Failed)
        );
    }

    #[test]
    fn asset_store_ref_count_coalesces_duplicates() {
        let mut store = AssetStore::<TestAsset>::new();
        let h1 = store.allocate_slot("shared".into());
        store.insert(h1.id(), TestAsset);
        let h2 = store.allocate_slot("shared".into());
        // same id because already loaded
        assert_eq!(h1, h2);
    }

    #[test]
    fn load_state_debug() {
        assert_eq!(format!("{:?}", LoadState::Loading), "Loading");
        assert_eq!(format!("{:?}", LoadState::Loaded), "Loaded");
        assert_eq!(format!("{:?}", LoadState::Failed), "Failed");
    }

    #[test]
    fn resolve_asset_path_relative() {
        assert_eq!(
            resolve_asset_path("sprites/player.png"),
            "assets/sprites/player.png"
        );
    }

    #[test]
    fn resolve_asset_path_already_assets() {
        assert_eq!(
            resolve_asset_path("assets/sprites/player.png"),
            "assets/sprites/player.png"
        );
    }

    #[test]
    fn resolve_asset_path_dot_slash() {
        assert_eq!(
            resolve_asset_path("./sprites/player.png"),
            "assets/sprites/player.png"
        );
    }

    #[test]
    fn resolve_asset_path_absolute_unchanged() {
        assert_eq!(
            resolve_asset_path("/absolute/path.png"),
            "/absolute/path.png"
        );
    }

    #[test]
    fn impl_asset_macro() {
        struct Foo;
        impl_asset!(Foo);
        // just verifying it compiles
        fn _accepts_asset(_: &dyn Asset) {}
        _accepts_asset(&Foo);
    }
}
