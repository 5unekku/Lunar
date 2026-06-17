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
//!         app.add_plugin(CsPlugin::new("path/to/lunar_scripts.so"));
//!         app.add_startup_system(scene_setup);
//!     }
//! }
//! ```

use std::path::{Path, PathBuf};
use bevy_ecs::world::World;
use lunar_ffi::{LunarWorld, LunarSchedule};
use lunar_core::{App, GamePlugin};

// ── PluginLoader ──────────────────────────────────────────────────────────────

/// error returned by [`PluginLoader::load`].
#[derive(Debug)]
pub enum LoadError {
    DlOpen(libloading::Error),
    MissingSymbol(libloading::Error),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DlOpen(error) => write!(formatter, "failed to open library: {error}"),
            Self::MissingSymbol(error) => write!(formatter, "lunar_plugin_init not found: {error}"),
        }
    }
}

impl std::error::Error for LoadError {}

/// holds loaded plugin libraries for the process lifetime.
///
/// dropping this struct unloads all plugins — typically you want to keep it alive.
#[derive(Default)]
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
}

// ── CsPlugin ──────────────────────────────────────────────────────────────────

/// `GamePlugin` that loads a NativeAOT C# shared library and wires it into the
/// engine's FFI system dispatch.
///
/// handles `init_registry`, plugin loading, startup dispatch, and per-frame
/// update dispatch — use `add_startup_system` for your own scene setup.
pub struct CsPlugin {
    path: PathBuf,
}

impl CsPlugin {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
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

        // keep the library loaded for the process lifetime
        let _ = Box::leak(Box::new(loader));

        app.add_system_to_stage(lunar_core::UpdateStage::Update, dispatch_ffi_update);
    }
}

fn dispatch_ffi_update(world: &mut World) {
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
