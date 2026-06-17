//! dlopen-based plugin loader for Lunar.
//!
//! # low-level usage
//!
//! ```ignore
//! use lunar_plugin_loader::PluginLoader;
//!
//! let mut world = World::new();
//! lunar_ffi::init_registry(&mut world);
//!
//! let mut loader = PluginLoader::new();
//! loader.load(&mut world, "/path/to/libmyplugin.so").expect("plugin load failed");
//! ```
//!
//! # high-level usage (recommended)
//!
//! ```ignore
//! use lunar_plugin_loader::CsPlugin;
//!
//! impl GamePlugin for MyGame {
//!     fn build(&mut self, app: &mut App) {
//!         app.add_plugin(CsPlugin::new("path/to/lunar_scripts.so").with_hot_reload());
//!         app.add_startup_system(scene_setup);
//!     }
//! }
//! ```

use std::{
    io,
    path::{Path, PathBuf},
    sync::{Mutex, mpsc},
    time::SystemTime,
};

use bevy_ecs::{prelude::Resource, world::World};
use lunar_ffi::{LunarWorld, LunarSchedule};
use lunar_core::{App, GamePlugin};

// ── PluginLoader ──────────────────────────────────────────────────────────────

/// error returned by [`PluginLoader::load`] and [`PluginLoader::reload`].
#[derive(Debug)]
pub enum LoadError {
    DlOpen(libloading::Error),
    MissingSymbol(libloading::Error),
    Copy(io::Error),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DlOpen(error) => write!(formatter, "failed to open library: {error}"),
            Self::MissingSymbol(error) => write!(formatter, "lunar_plugin_init not found: {error}"),
            Self::Copy(error) => write!(formatter, "failed to copy plugin for reload: {error}"),
        }
    }
}

impl std::error::Error for LoadError {}

/// holds loaded plugin libraries for the process lifetime.
///
/// stored as a world resource when hot reload is enabled so the reload system
/// can drop old libraries and load new ones between frames.
#[derive(Default, Resource)]
pub struct PluginLoader {
    libs: Vec<libloading::Library>,
}

impl PluginLoader {
    pub fn new() -> Self { Self::default() }

    /// load a plugin dylib and call its `lunar_plugin_init(world)` entry point.
    ///
    /// # safety
    ///
    /// the plugin's `lunar_plugin_init` must correctly use the C API. the engine
    /// cannot verify this at load time.
    pub fn load(&mut self, world: &mut World, path: &Path) -> Result<(), LoadError> {
        log::info!("plugin-loader: loading {}", path.display());

        // SAFETY: loading arbitrary code is inherently unsafe
        let lib = unsafe { libloading::Library::new(path) }
            .map_err(LoadError::DlOpen)?;

        {
            // SAFETY: the symbol is a function pointer matching our C ABI contract
            let init: libloading::Symbol<unsafe extern "C" fn(*mut LunarWorld)> =
                unsafe { lib.get(b"lunar_plugin_init\0") }
                    .map_err(LoadError::MissingSymbol)?;

            let world_ptr = world as *mut World as *mut LunarWorld;
            unsafe { init(world_ptr) };
        }

        self.libs.push(lib);
        log::info!("plugin-loader: {} loaded ok", path.display());
        Ok(())
    }

    /// reload a new version of the plugin, preserving ECS world state.
    ///
    /// clears update/fixedupdate/shutdown systems, then loads the new library
    /// and calls `lunar_plugin_init` to re-register systems.
    ///
    /// old libraries are intentionally kept alive rather than dlclosed: NativeAOT
    /// embeds a GC + signal handler per .so that cannot safely be torn down
    /// mid-process. each reload gets a versioned copy of the file so dlopen
    /// treats it as a distinct shared object with its own runtime instance.
    pub fn reload(&mut self, world: &mut World, path: &Path) -> Result<(), LoadError> {
        log::info!("hot reload: loading version {}", self.libs.len());
        lunar_ffi::clear_schedule(world, LunarSchedule::Update);
        lunar_ffi::clear_schedule(world, LunarSchedule::FixedUpdate);
        lunar_ffi::clear_schedule(world, LunarSchedule::Shutdown);
        let versioned = versioned_plugin_copy(path, self.libs.len())?;
        lunar_ffi::set_is_reload(world, true);
        let result = self.load(world, &versioned);
        lunar_ffi::set_is_reload(world, false);
        if result.is_ok() {
            log::info!("hot reload: done");
        }
        result
    }
}

// ── hot reload receiver resource ─────────────────────────────────────────────

/// world resource that carries incoming reload path requests from the file watcher.
#[derive(Resource)]
pub struct ReloadReceiver(pub Mutex<mpsc::Receiver<PathBuf>>);

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok().and_then(|m| m.modified().ok())
}

/// copy `src` to a per-version temp path so each reload is a distinct .so file.
///
/// linux dlopen returns the same handle for the same path; a distinct path forces
/// a fresh mapping and a separate NativeAOT runtime instance.
fn versioned_plugin_copy(src: &Path, version: usize) -> Result<PathBuf, LoadError> {
    let stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or("plugin");
    let ext  = src.extension().and_then(|s| s.to_str()).unwrap_or("so");
    let dir  = std::env::temp_dir().join("lunar_cs_hot");
    std::fs::create_dir_all(&dir).map_err(LoadError::Copy)?;
    let dest = dir.join(format!("{stem}_v{version}.{ext}"));
    std::fs::copy(src, &dest).map_err(LoadError::Copy)?;
    Ok(dest)
}

fn watch_for_changes(path: PathBuf, sender: mpsc::Sender<PathBuf>) {
    std::thread::spawn(move || {
        let mut last = mtime(&path);
        loop {
            std::thread::sleep(std::time::Duration::from_secs(2));
            let current = mtime(&path);
            if current != last && current.is_some() {
                last = current;
                // brief wait to ensure the file write is fully flushed
                std::thread::sleep(std::time::Duration::from_millis(250));
                if sender.send(path.clone()).is_err() { break; }
            }
        }
    });
}

// ── CsPlugin ──────────────────────────────────────────────────────────────────

/// `GamePlugin` that loads a NativeAOT C# shared library and wires it into the
/// engine's FFI system dispatch.
///
/// handles `init_registry`, plugin loading, startup dispatch, and per-frame
/// update dispatch. use [`CsPlugin::with_hot_reload`] to enable file watching.
pub struct CsPlugin {
    path: PathBuf,
    hot_reload: bool,
}

impl CsPlugin {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into(), hot_reload: false }
    }

    /// watch the plugin `.so` for changes and reload automatically.
    ///
    /// when the file changes on disk, all FFI systems are unregistered and the
    /// new library is loaded, calling `lunar_plugin_init` again. the ECS world
    /// state (entities, components, resources) is fully preserved across reload.
    pub fn with_hot_reload(mut self) -> Self {
        self.hot_reload = true;
        self
    }
}

impl GamePlugin for CsPlugin {
    fn name(&self) -> &str { "lunar-cs-plugin" }

    fn build(&mut self, app: &mut App) {
        let world = app.world_mut();
        lunar_ffi::init_registry(world);

        let mut loader = PluginLoader::new();
        loader
            .load(world, &self.path)
            .unwrap_or_else(|error| panic!("failed to load C# plugin '{}': {error}", self.path.display()));

        lunar_ffi::dispatch_systems(world, LunarSchedule::Startup);

        if self.hot_reload {
            let (tx, rx) = mpsc::channel::<PathBuf>();
            watch_for_changes(self.path.clone(), tx);
            world.insert_resource(ReloadReceiver(Mutex::new(rx)));
            world.insert_resource(loader);
            app.add_system_to_stage(lunar_core::UpdateStage::Update, dispatch_ffi_update_hot);
        } else {
            let _ = Box::leak(Box::new(loader));
            app.add_system_to_stage(lunar_core::UpdateStage::Update, dispatch_ffi_update);
        }
    }
}

fn dispatch_ffi_update(world: &mut World) {
    lunar_ffi::dispatch_systems(world, LunarSchedule::Update);
}

fn dispatch_ffi_update_hot(world: &mut World) {
    // drain all queued paths and use the last one — guards against double-trigger
    // when dotnet publish touches the .so more than once during a single write
    let pending = world
        .get_resource::<ReloadReceiver>()
        .and_then(|receiver| {
            let guard = receiver.0.lock().ok()?;
            let mut latest = None;
            while let Ok(path) = guard.try_recv() {
                latest = Some(path);
            }
            latest
        });

    if let Some(new_path) = pending {
        world.resource_scope(|world, mut loader: bevy_ecs::world::Mut<PluginLoader>| {
            if let Err(error) = loader.reload(world, &new_path) {
                log::error!("hot reload failed: {error}");
            }
        });
    }

    lunar_ffi::dispatch_systems(world, LunarSchedule::Update);
}

// ── headless helper ───────────────────────────────────────────────────────────

/// dispatch startup, run the frame loop, then dispatch shutdown.
/// intended for integration tests or headless runners where you own the loop.
pub fn run_headless<F>(world: &mut World, loader: &mut PluginLoader, plugin_path: &Path, mut frame: F)
where
    F: FnMut(&mut World) -> bool,
{
    loader.load(world, plugin_path).expect("plugin load failed");
    lunar_ffi::dispatch_systems(world, LunarSchedule::Startup);
    while frame(world) {
        lunar_ffi::dispatch_systems(world, LunarSchedule::Update);
    }
    lunar_ffi::dispatch_systems(world, LunarSchedule::Shutdown);
}
