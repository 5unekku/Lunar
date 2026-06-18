//! C# plugin loader for Lunar.
//!
//! two backends, selected by cargo feature:
//!
//! - **default (NativeAOT)**: dlopen the compiled `.so`, call `lunar_plugin_init`.
//!   hot reload works but requires versioned-copy workaround because NativeAOT
//!   cannot safely handle dlclose mid-process.
//!
//! - **`coreclr`**: load via hostfxr + `LunarHost.dll` bootstrapper, which
//!   manages an `AssemblyLoadContext` per plugin version. hot reload is clean:
//!   the old context is GC-collected before the new one loads.
//!
//! # recommended usage
//!
//! ```ignore
//! use lunar_plugin_loader::CsPlugin;
//!
//! impl GamePlugin for MyGame {
//!     fn build(&mut self, app: &mut App) {
//!         app.add_plugin(CsPlugin::new("plugin.dll").with_hot_reload());
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

// ── error type ────────────────────────────────────────────────────────────────

/// error returned by [`PluginLoader`] operations.
#[derive(Debug)]
pub enum LoadError {
    DlOpen(libloading::Error),
    MissingSymbol(libloading::Error),
    Copy(io::Error),
    #[cfg(feature = "coreclr")]
    Host(lunar_dotnet_host::HostError),
    #[cfg(feature = "coreclr")]
    NulPath,
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DlOpen(e)       => write!(formatter, "failed to open library: {e}"),
            Self::MissingSymbol(e)=> write!(formatter, "lunar_plugin_init not found: {e}"),
            Self::Copy(e)         => write!(formatter, "failed to copy plugin for reload: {e}"),
            #[cfg(feature = "coreclr")]
            Self::Host(e)         => write!(formatter, "dotnet host error: {e}"),
            #[cfg(feature = "coreclr")]
            Self::NulPath         => write!(formatter, "plugin path contains an interior NUL byte"),
        }
    }
}

impl std::error::Error for LoadError {}

// ── loader backend ────────────────────────────────────────────────────────────

// function pointer types matching the C# bootstrapper's [UnmanagedCallersOnly] methods.
// nint in C# maps to isize; used to pass the *mut World opaquely.
#[cfg(feature = "coreclr")]
type HostFn = unsafe extern "C" fn(world_ptr: isize, path: *const std::ffi::c_char);

enum LoaderBackend {
    NativeAot {
        libs: Vec<libloading::Library>,
    },
    #[cfg(feature = "coreclr")]
    CoreClr {
        // keeps hostfxr.so alive for the process lifetime
        _runtime:    lunar_dotnet_host::DotnetRuntime,
        host_reload: HostFn,
    },
}

impl Default for LoaderBackend {
    fn default() -> Self {
        Self::NativeAot { libs: vec![] }
    }
}

// all fields are either function pointers (Send+Sync) or DotnetRuntime (Send+Sync)
unsafe impl Send for LoaderBackend {}
unsafe impl Sync for LoaderBackend {}

// ── PluginLoader ──────────────────────────────────────────────────────────────

/// holds the active plugin backend. stored as an ECS [`Resource`] when hot
/// reload is enabled so [`dispatch_ffi_update_hot`] can reload between frames.
#[derive(Resource, Default)]
pub struct PluginLoader {
    backend: LoaderBackend,
}

impl PluginLoader {
    pub fn new() -> Self { Self::default() }

    /// (NativeAOT) load a plugin `.so` and call its `lunar_plugin_init`.
    ///
    /// old libraries are accumulated rather than dropped — NativeAOT embeds a GC
    /// per `.so` that cannot safely be torn down mid-process via dlclose.
    #[allow(irrefutable_let_patterns)]
    pub fn load_nativeaot(&mut self, world: &mut World, path: &Path) -> Result<(), LoadError> {
        let LoaderBackend::NativeAot { libs } = &mut self.backend else {
            panic!("load_nativeaot called on a CoreCLR loader");
        };

        log::info!("plugin-loader: loading {}", path.display());
        // SAFETY: loading arbitrary code; plugin must implement the C ABI contract
        let lib = unsafe { libloading::Library::new(path) }.map_err(LoadError::DlOpen)?;
        {
            let init: libloading::Symbol<unsafe extern "C" fn(*mut LunarWorld)> =
                unsafe { lib.get(b"lunar_plugin_init\0") }.map_err(LoadError::MissingSymbol)?;
            unsafe { init(world as *mut World as *mut LunarWorld) };
        }
        libs.push(lib);
        log::info!("plugin-loader: loaded ok");
        Ok(())
    }

    /// (NativeAOT) reload: copy the `.so` to a versioned temp path and load it
    /// without dropping old libraries, preserving the old NativeAOT runtime.
    #[allow(irrefutable_let_patterns)]
    pub fn reload_nativeaot(&mut self, world: &mut World, path: &Path) -> Result<(), LoadError> {
        let LoaderBackend::NativeAot { libs } = &mut self.backend else {
            panic!("reload_nativeaot called on a CoreCLR loader");
        };
        let version = libs.len();

        log::info!("hot reload: loading nativeaot version {version}");
        lunar_ffi::clear_schedule(world, LunarSchedule::Update);
        lunar_ffi::clear_schedule(world, LunarSchedule::FixedUpdate);
        lunar_ffi::clear_schedule(world, LunarSchedule::Shutdown);

        let versioned = versioned_plugin_copy(path, version)?;
        lunar_ffi::set_is_reload(world, true);

        // borrow checker: load_nativeaot also borrows self, so inline it here
        log::info!("plugin-loader: loading {}", versioned.display());
        let lib = unsafe { libloading::Library::new(&versioned) }.map_err(LoadError::DlOpen)?;
        {
            let init: libloading::Symbol<unsafe extern "C" fn(*mut LunarWorld)> =
                unsafe { lib.get(b"lunar_plugin_init\0") }.map_err(LoadError::MissingSymbol)?;
            unsafe { init(world as *mut World as *mut LunarWorld) };
        }
        libs.push(lib);

        lunar_ffi::set_is_reload(world, false);
        log::info!("hot reload: done (nativeaot version {version})");
        Ok(())
    }

    /// (CoreCLR) reload: call `lunar_host_reload` on the bootstrapper, which
    /// collects the old `AssemblyLoadContext` and loads the new version cleanly.
    #[cfg(feature = "coreclr")]
    pub fn reload_coreclr(&mut self, world: &mut World, path: &Path) -> Result<(), LoadError> {
        let LoaderBackend::CoreClr { host_reload, .. } = &self.backend else {
            panic!("reload_coreclr called on a NativeAOT loader");
        };
        let host_reload = *host_reload;

        log::info!("hot reload: reloading via CoreCLR");
        lunar_ffi::clear_schedule(world, LunarSchedule::Update);
        lunar_ffi::clear_schedule(world, LunarSchedule::FixedUpdate);
        lunar_ffi::clear_schedule(world, LunarSchedule::Shutdown);
        lunar_ffi::set_is_reload(world, true);

        let path_cstr = path_to_cstr(path)?;
        let world_ptr = world as *mut World as isize;
        unsafe { host_reload(world_ptr, path_cstr.as_ptr()) };

        lunar_ffi::set_is_reload(world, false);
        log::info!("hot reload: done (coreclr)");
        Ok(())
    }

    /// dispatch to the right reload implementation for the active backend.
    pub fn reload(&mut self, world: &mut World, path: &Path) -> Result<(), LoadError> {
        match &self.backend {
            LoaderBackend::NativeAot { .. } => self.reload_nativeaot(world, path),
            #[cfg(feature = "coreclr")]
            LoaderBackend::CoreClr { .. }   => self.reload_coreclr(world, path),
        }
    }
}

// ── hot reload helpers ────────────────────────────────────────────────────────

/// world resource that carries incoming reload path requests from the file watcher.
#[derive(Resource)]
pub struct ReloadReceiver(pub Mutex<mpsc::Receiver<PathBuf>>);

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok().and_then(|m| m.modified().ok())
}

/// copy `src` to a per-version temp path so each reload is a distinct shared object.
/// linux dlopen returns the same handle for the same path; a distinct path forces a
/// fresh mapping and a separate NativeAOT runtime instance, avoiding the dlclose bug.
fn versioned_plugin_copy(src: &Path, version: usize) -> Result<PathBuf, LoadError> {
    let stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or("plugin");
    let ext  = src.extension().and_then(|s| s.to_str()).unwrap_or("so");
    let dir  = std::env::temp_dir().join("lunar_cs_hot");
    std::fs::create_dir_all(&dir).map_err(LoadError::Copy)?;
    let dest = dir.join(format!("{stem}_v{version}.{ext}"));
    std::fs::copy(src, &dest).map_err(LoadError::Copy)?;
    Ok(dest)
}

#[cfg(feature = "coreclr")]
fn path_to_cstr(path: &Path) -> Result<std::ffi::CString, LoadError> {
    std::ffi::CString::new(path.to_str().ok_or(LoadError::NulPath)?).map_err(|_| LoadError::NulPath)
}

fn watch_for_changes(path: PathBuf, sender: mpsc::Sender<PathBuf>) {
    std::thread::spawn(move || {
        let mut last = mtime(&path);
        loop {
            std::thread::sleep(std::time::Duration::from_secs(2));
            let current = mtime(&path);
            if current != last && current.is_some() {
                last = current;
                std::thread::sleep(std::time::Duration::from_millis(250));
                if sender.send(path.clone()).is_err() { break; }
            }
        }
    });
}

// ── CsPlugin ──────────────────────────────────────────────────────────────────

/// `GamePlugin` that loads a C# plugin and wires it into the engine's FFI dispatch.
///
/// in the default (NativeAOT) configuration, the plugin is a compiled `.so`.
/// with the `coreclr` cargo feature, the plugin is a managed `.dll` loaded via
/// the .NET CoreCLR hosting API with proper `AssemblyLoadContext` isolation.
///
/// use [`CsPlugin::with_hot_reload`] to enable file-watch-driven reload.
pub struct CsPlugin {
    path: PathBuf,
    hot_reload: bool,
    /// coreclr only: path to LunarHost.dll (the ALC bootstrapper)
    #[cfg(feature = "coreclr")]
    host_dll: PathBuf,
}

impl CsPlugin {
    /// create a plugin loader for the given path.
    ///
    /// - NativeAOT build: `path` is the `.so` / `.dll` / `.dylib`
    /// - CoreCLR build: `path` is the managed `.dll`; the host bootstrapper
    ///   (`LunarHost.dll`) must be in the same directory or set via [`CsPlugin::with_host_dll`]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        #[cfg(feature = "coreclr")]
        let host_dll = path.parent()
            .unwrap_or(Path::new("."))
            .join("LunarHost.dll");
        Self {
            path,
            hot_reload: false,
            #[cfg(feature = "coreclr")]
            host_dll,
        }
    }

    /// override the path to `LunarHost.dll` (CoreCLR only).
    #[cfg(feature = "coreclr")]
    pub fn with_host_dll(mut self, host_dll: impl Into<PathBuf>) -> Self {
        self.host_dll = host_dll.into();
        self
    }

    /// watch the plugin file for changes and reload in-place between frames.
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

        #[cfg(feature = "coreclr")]
        let loader = self.build_coreclr(world);
        #[cfg(not(feature = "coreclr"))]
        let loader = self.build_nativeaot(world);

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

impl CsPlugin {
    #[cfg(not(feature = "coreclr"))]
    fn build_nativeaot(&self, world: &mut World) -> PluginLoader {
        let mut loader = PluginLoader::new();
        loader
            .load_nativeaot(world, &self.path)
            .unwrap_or_else(|e| panic!("failed to load C# plugin '{}': {e}", self.path.display()));
        loader
    }

    #[cfg(feature = "coreclr")]
    fn build_coreclr(&self, world: &mut World) -> PluginLoader {
        use lunar_dotnet_host::DotnetRuntime;

        // derive runtimeconfig.json path from the host DLL path
        let runtimeconfig = self.host_dll.with_extension("runtimeconfig.json");

        log::info!("plugin-loader: initialising CoreCLR from {}", runtimeconfig.display());
        let runtime = DotnetRuntime::load(&runtimeconfig)
            .unwrap_or_else(|e| panic!("failed to initialise .NET runtime: {e}"));

        let host_load: HostFn = unsafe {
            let ptr = runtime.get_fn_ptr(
                &self.host_dll,
                "LunarHost.PluginHost, LunarHost",
                "Load",
            ).unwrap_or_else(|e| panic!("failed to get lunar_host_load: {e}"));
            std::mem::transmute(ptr)
        };
        let host_reload: HostFn = unsafe {
            let ptr = runtime.get_fn_ptr(
                &self.host_dll,
                "LunarHost.PluginHost, LunarHost",
                "Reload",
            ).unwrap_or_else(|e| panic!("failed to get lunar_host_reload: {e}"));
            std::mem::transmute(ptr)
        };

        log::info!("plugin-loader: loading {} via CoreCLR", self.path.display());
        let path_cstr = path_to_cstr(&self.path)
            .expect("plugin path contains NUL bytes");
        let world_ptr = world as *mut World as isize;
        unsafe { host_load(world_ptr, path_cstr.as_ptr()) };

        PluginLoader {
            backend: LoaderBackend::CoreClr {
                _runtime: runtime,
                host_reload,
            },
        }
    }
}

// ── per-frame dispatch ────────────────────────────────────────────────────────

fn dispatch_ffi_update(world: &mut World) {
    lunar_ffi::dispatch_systems(world, LunarSchedule::Update);
}

fn dispatch_ffi_update_hot(world: &mut World) {
    // drain all queued paths and use the last — guards against double-trigger
    // when the build tool touches the file more than once per write
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

/// run a NativeAOT plugin headlessly: load, startup, frame loop, shutdown.
/// intended for integration tests where you own the loop.
pub fn run_headless<F>(world: &mut World, loader: &mut PluginLoader, plugin_path: &Path, mut frame: F)
where
    F: FnMut(&mut World) -> bool,
{
    loader
        .load_nativeaot(world, plugin_path)
        .expect("plugin load failed");
    lunar_ffi::dispatch_systems(world, LunarSchedule::Startup);
    while frame(world) {
        lunar_ffi::dispatch_systems(world, LunarSchedule::Update);
    }
    lunar_ffi::dispatch_systems(world, LunarSchedule::Shutdown);
}
