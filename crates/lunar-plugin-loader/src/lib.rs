//! dlopen-based plugin loader.
//!
//! loads a shared library that exports `lunar_plugin_init` and calls it with
//! the current engine world. keeps the library alive for the process lifetime.
//!
//! # usage
//!
//! ```ignore
//! use lunar_plugin_loader::PluginLoader;
//! use bevy_ecs::world::World;
//!
//! let mut world = World::new();
//! lunar_ffi::init_registry(&mut world);
//!
//! let mut loader = PluginLoader::new();
//! loader.load(&mut world, "/path/to/libmyplugin.so").expect("plugin load failed");
//!
//! // each frame:
//! lunar_ffi::dispatch_systems(&mut world, lunar_ffi::LunarSchedule::Update);
//! ```

use std::path::Path;
use bevy_ecs::world::World;
use lunar_ffi::{LunarWorld, LunarSchedule};

/// error returned by [`PluginLoader::load`].
#[derive(Debug)]
pub enum LoadError {
    DlOpen(libloading::Error),
    MissingSymbol(libloading::Error),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DlOpen(err) => write!(formatter, "failed to open library: {err}"),
            Self::MissingSymbol(err) => write!(formatter, "lunar_plugin_init not found: {err}"),
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

/// convenience: dispatch startup callbacks, then run `setup`, then dispatch update every frame.
///
/// intended for integration tests or headless runners where you own the loop.
pub fn run_headless<F>(world: &mut World, loader: &mut PluginLoader, plugin_path: &Path, mut frame: F)
where
    F: FnMut(&mut World) -> bool, // return false to stop
{
    loader.load(world, plugin_path).expect("plugin load failed");
    lunar_ffi::dispatch_systems(world, LunarSchedule::Startup);
    while frame(world) {
        lunar_ffi::dispatch_systems(world, LunarSchedule::Update);
    }
    lunar_ffi::dispatch_systems(world, LunarSchedule::Shutdown);
}
