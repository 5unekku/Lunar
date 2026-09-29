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
//! let raw = unsafe {
//!     runtime.get_fn_ptr(Path::new("MyPlugin.dll"), "MyPlugin.Entry, MyPlugin", "Run").unwrap()
//! };
//! let entry: unsafe extern "C" fn() = unsafe { std::mem::transmute(raw) };
//! unsafe { entry() };
//! ```

mod find;

use std::{
    ffi::c_void,
    path::Path,
};

// ── hostfxr raw types ─────────────────────────────────────────────────────────

type Handle = *mut c_void;

/// hostfxr's `char_t`: `wchar_t` (utf-16) on windows, narrow utf-8 elsewhere. every
/// string argument used to be passed as narrow bytes, so on windows hostfxr read
/// them as utf-16 and scanned past the single NUL for a 16-bit one (sec-05).
#[cfg(windows)]
type CharT = u16;
#[cfg(not(windows))]
type CharT = std::ffi::c_char;

/// an owned, NUL-terminated `char_t` string for passing to hostfxr.
struct HostString(Vec<CharT>);

impl HostString {
    fn new(s: &str) -> Result<Self, HostError> {
        if s.contains('\0') {
            return Err(HostError::NulPath);
        }
        #[cfg(windows)]
        let units: Vec<CharT> = s.encode_utf16().chain([0]).collect();
        #[cfg(not(windows))]
        let units: Vec<CharT> = s.bytes().map(|b| b as CharT).chain([0]).collect();
        Ok(Self(units))
    }

    fn from_path(path: &Path) -> Result<Self, HostError> {
        Self::new(path.to_str().ok_or(HostError::NulPath)?)
    }

    fn as_ptr(&self) -> *const CharT {
        self.0.as_ptr()
    }

    #[cfg(test)]
    fn units(&self) -> &[CharT] {
        &self.0
    }
}

// hdt_load_assembly_and_get_function_pointer = 5
const HDT_LOAD_ASSEMBLY_AND_GET_FUNCTION_POINTER: i32 = 5;

// success codes from hostfxr: anything else is an error
const SUCCESS: i32 = 0x00000000;
const SUCCESS_HOST_ALREADY_INITIALIZED: i32 = 0x00000001;

type FnInitForRuntimeConfig = unsafe extern "C" fn(
    runtime_config_path: *const CharT,
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
    assembly_path: *const CharT,
    type_name: *const CharT,
    method_name: *const CharT,
    delegate_type_name: *const CharT, // null = [UnmanagedCallersOnly]
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
// defined by .NET: the runtime itself is thread-safe.
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

        let config_cstr = HostString::from_path(runtimeconfig)?;
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
        // close the init handle, the runtime stays alive
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
    /// # Safety
    ///
    /// the caller must cast the returned pointer to the correct function type
    /// matching the C# method's parameter and return types.
    pub unsafe fn get_fn_ptr(
        &self,
        assembly_path: &Path,
        type_name: &str,
        method_name: &str,
    ) -> Result<*const c_void, HostError> {
        let assembly  = HostString::from_path(assembly_path)?;
        let type_name = HostString::new(type_name)?;
        let method    = HostString::new(method_name)?;

        // UNMANAGEDCALLERSONLY_METHOD sentinel: (const char_t*)-1
        // passing null would mean "use a managed delegate type", which is wrong here
        const UNMANAGEDCALLERSONLY_METHOD: *const CharT = usize::MAX as *const CharT;

        let mut fp: *const c_void = std::ptr::null();
        let rc = unsafe {
            (self.get_fn_ptr)(
                assembly.as_ptr(),
                type_name.as_ptr(),
                method.as_ptr(),
                UNMANAGEDCALLERSONLY_METHOD,
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


#[cfg(test)]
mod tests {
    use super::*;

    /// sec-05: hostfxr's char_t is wchar_t (utf-16) on windows. strings are built
    /// as NUL-terminated char_t units for the target, never narrow bytes there.
    #[test]
    fn host_strings_are_nul_terminated_char_t() {
        let s = HostString::new("Ab").unwrap();
        let expected: Vec<CharT> = "Ab\0"
            .chars()
            .map(|c| c as u32 as CharT)
            .collect();
        assert_eq!(s.units(), expected.as_slice());
        #[cfg(windows)]
        assert_eq!(std::mem::size_of::<CharT>(), 2);
        assert!(HostString::new("a\0b").is_err());
    }
}
