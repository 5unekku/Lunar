//! embed the .NET CoreCLR runtime in a Rust process.
//!
//! wraps the `hostfxr` native hosting API. finds hostfxr at runtime (no link-time
//! dependency), initialises a CoreCLR instance from a `.runtimeconfig.json`, and
//! returns native-callable function pointers to `[UnmanagedCallersOnly]` methods
//! in managed assemblies.
//!
//! # example
//!
//! ```no_run
//! use lunar_dotnet_host::DotnetRuntime;
//! use std::path::Path;
//!
//! let runtime = DotnetRuntime::load(Path::new("MyPlugin.runtimeconfig.json")).unwrap();
//! let fp: unsafe extern "C" fn() = unsafe {
//!     runtime.get_fn("MyPlugin.dll", "MyPlugin.Entry, MyPlugin", "Run").unwrap()
//! };
//! unsafe { fp() };
//! ```

mod find;

use std::{
    ffi::{CString, c_char, c_void},
    path::Path,
};

// ── hostfxr raw types ─────────────────────────────────────────────────────────

type Handle = *mut c_void;

// hdt_load_assembly_and_get_function_pointer = 5
const HDT_LOAD_ASSEMBLY_AND_GET_FUNCTION_POINTER: i32 = 5;

// success codes from hostfxr — anything else is an error
const SUCCESS: i32 = 0x00000000;
const SUCCESS_HOST_ALREADY_INITIALIZED: i32 = 0x00000001;

type FnInitForRuntimeConfig = unsafe extern "C" fn(
    runtime_config_path: *const c_char,
    parameters: *const c_void,
    host_context_handle: *mut Handle,
) -> i32;

type FnGetRuntimeDelegate = unsafe extern "C" fn(
    host_context_handle: Handle,
    delegate_type: i32,
    delegate: *mut *const c_void,
) -> i32;

type FnClose = unsafe extern "C" fn(host_context_handle: Handle) -> i32;

type FnLoadAssemblyAndGetFunctionPointer = unsafe extern "C" fn(
    assembly_path: *const c_char,
    type_name: *const c_char,
    method_name: *const c_char,
    delegate_type_name: *const c_char, // null = [UnmanagedCallersOnly]
    reserved: *const c_void,
    delegate: *mut *const c_void,
) -> i32;

// ── public API ────────────────────────────────────────────────────────────────

/// error from the .NET hosting API.
#[derive(Debug)]
pub enum HostError {
    /// hostfxr library could not be found on this machine.
    HostfxrNotFound,
    /// hostfxr could not be loaded as a shared library.
    Load(libloading::Error),
    /// hostfxr returned a non-zero error code during init.
    InitFailed(i32),
    /// hostfxr returned an error fetching the `load_assembly_and_get_function_pointer` delegate.
    GetDelegateFailed(i32),
    /// the runtimeconfig path could not be converted to a C string (embedded NUL byte).
    NulPath,
    /// `load_assembly_and_get_function_pointer` returned a non-zero error code.
    GetFunctionPointerFailed(i32),
}

impl std::fmt::Display for HostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HostfxrNotFound         => write!(f, "hostfxr not found; is the .NET SDK or runtime installed?"),
            Self::Load(error)             => write!(f, "failed to load hostfxr: {error}"),
            Self::InitFailed(code)        => write!(f, "hostfxr_initialize_for_runtime_config failed: 0x{code:08X}"),
            Self::GetDelegateFailed(code) => write!(f, "hostfxr_get_runtime_delegate failed: 0x{code:08X}"),
            Self::NulPath                 => write!(f, "path contains an interior NUL byte"),
            Self::GetFunctionPointerFailed(code) => write!(f, "load_assembly_and_get_function_pointer failed: 0x{code:08X}"),
        }
    }
}

impl std::error::Error for HostError {}

/// a live .NET CoreCLR runtime embedded in this process.
///
/// only one runtime instance can exist per process; the underlying hostfxr
/// API is idempotent so multiple calls to [`DotnetRuntime::load`] with
/// compatible runtimeconfigs are safe.
pub struct DotnetRuntime {
    // keeps hostfxr.so in memory for the lifetime of the runtime
    _lib: libloading::Library,
    get_fn_ptr: FnLoadAssemblyAndGetFunctionPointer,
}

// the runtime is accessed only through fn pointers whose threading contract is
// defined by .NET — the runtime itself is thread-safe.
unsafe impl Send for DotnetRuntime {}
unsafe impl Sync for DotnetRuntime {}

impl DotnetRuntime {
    /// find hostfxr, load it, and initialise a CoreCLR runtime from the given
    /// `.runtimeconfig.json`. blocks until the runtime is ready.
    pub fn load(runtimeconfig: &Path) -> Result<Self, HostError> {
        let hostfxr_path = find::find_hostfxr().ok_or(HostError::HostfxrNotFound)?;
        log::info!("dotnet-host: loading hostfxr from {}", hostfxr_path.display());

        // SAFETY: we are loading a system library whose ABI we match exactly
        let lib = unsafe { libloading::Library::new(&hostfxr_path) }
            .map_err(HostError::Load)?;

        let init: FnInitForRuntimeConfig = unsafe {
            *lib.get::<FnInitForRuntimeConfig>(b"hostfxr_initialize_for_runtime_config\0")
                .map_err(HostError::Load)?
        };
        let get_delegate: FnGetRuntimeDelegate = unsafe {
            *lib.get::<FnGetRuntimeDelegate>(b"hostfxr_get_runtime_delegate\0")
                .map_err(HostError::Load)?
        };
        let close: FnClose = unsafe {
            *lib.get::<FnClose>(b"hostfxr_close\0")
                .map_err(HostError::Load)?
        };

        let config_cstr = path_to_cstring(runtimeconfig)?;
        let mut handle: Handle = std::ptr::null_mut();

        let rc = unsafe { init(config_cstr.as_ptr(), std::ptr::null(), &mut handle) };
        if rc != SUCCESS && rc != SUCCESS_HOST_ALREADY_INITIALIZED {
            return Err(HostError::InitFailed(rc));
        }
        log::info!("dotnet-host: CoreCLR initialised (rc=0x{rc:08X})");

        let mut delegate_ptr: *const c_void = std::ptr::null();
        let rc = unsafe {
            get_delegate(handle, HDT_LOAD_ASSEMBLY_AND_GET_FUNCTION_POINTER, &mut delegate_ptr)
        };
        // close the init handle — the runtime stays alive
        unsafe { close(handle) };

        if rc != SUCCESS {
            return Err(HostError::GetDelegateFailed(rc));
        }

        // SAFETY: hostfxr guarantees this pointer is a valid function of this type
        let get_fn_ptr: FnLoadAssemblyAndGetFunctionPointer =
            unsafe { std::mem::transmute(delegate_ptr) };

        Ok(Self { _lib: lib, get_fn_ptr })
    }

    /// get a native function pointer to an `[UnmanagedCallersOnly]` method in a
    /// managed assembly, loading the assembly if necessary.
    ///
    /// - `assembly_path`: full path to the `.dll`
    /// - `type_name`: `"Namespace.Class, AssemblyName"` (assembly-qualified type name)
    /// - `method_name`: name of the `[UnmanagedCallersOnly]` static method
    ///
    /// # safety
    ///
    /// the caller must cast the returned pointer to the correct function type
    /// matching the C# method's parameter and return types.
    pub unsafe fn get_fn_ptr(
        &self,
        assembly_path: &Path,
        type_name: &str,
        method_name: &str,
    ) -> Result<*const c_void, HostError> {
        let assembly  = path_to_cstring(assembly_path)?;
        let type_name = CString::new(type_name).map_err(|_| HostError::NulPath)?;
        let method    = CString::new(method_name).map_err(|_| HostError::NulPath)?;

        let mut fp: *const c_void = std::ptr::null();
        let rc = unsafe {
            (self.get_fn_ptr)(
                assembly.as_ptr(),
                type_name.as_ptr(),
                method.as_ptr(),
                std::ptr::null(), // null = [UnmanagedCallersOnly] default
                std::ptr::null(),
                &mut fp,
            )
        };

        if rc != SUCCESS {
            return Err(HostError::GetFunctionPointerFailed(rc));
        }
        Ok(fp)
    }
}

fn path_to_cstring(path: &Path) -> Result<CString, HostError> {
    let s = path.to_str().ok_or(HostError::NulPath)?;
    CString::new(s).map_err(|_| HostError::NulPath)
}
