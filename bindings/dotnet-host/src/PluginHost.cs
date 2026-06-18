using System.Reflection;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Runtime.Loader;

namespace LunarHost;

/// <summary>
/// manages the <see cref="PluginLoadContext"/> lifecycle so the Rust host can load
/// and hot-reload C# game plugins via CoreCLR without NativeAOT.
///
/// entry points are called by the Rust loader using hostfxr's
/// <c>load_assembly_and_get_function_pointer</c> API.
/// </summary>
public static class PluginHost
{
    static PluginLoadContext? _context;

    /// <summary>load a plugin assembly into a fresh <see cref="AssemblyLoadContext"/> and call its init method.</summary>
    [UnmanagedCallersOnly(EntryPoint = "lunar_host_load", CallConvs = [typeof(CallConvCdecl)])]
    public static unsafe void Load(nint worldPtr, byte* pluginPathUtf8)
    {
        var path = Marshal.PtrToStringUTF8((nint)pluginPathUtf8)
            ?? throw new ArgumentNullException(nameof(pluginPathUtf8));
        LoadInner(worldPtr, path);
    }

    /// <summary>
    /// unload the current plugin context and load a new version — the core of hot reload.
    /// the old <see cref="AssemblyLoadContext"/> is collected so its NativeAOT-style
    /// static state is cleaned up before the new version runs.
    /// </summary>
    [UnmanagedCallersOnly(EntryPoint = "lunar_host_reload", CallConvs = [typeof(CallConvCdecl)])]
    public static unsafe void Reload(nint worldPtr, byte* pluginPathUtf8)
    {
        var path = Marshal.PtrToStringUTF8((nint)pluginPathUtf8)
            ?? throw new ArgumentNullException(nameof(pluginPathUtf8));

        _context?.Unload();
        _context = null;
        // GC needs a few cycles to fully collect a WeakReference-tracked ALC
        for (int i = 0; i < 3; i++)
        {
            GC.Collect();
            GC.WaitForPendingFinalizers();
        }

        LoadInner(worldPtr, path);
    }

    /// <summary>unload the current plugin context without loading a replacement.</summary>
    [UnmanagedCallersOnly(EntryPoint = "lunar_host_unload", CallConvs = [typeof(CallConvCdecl)])]
    public static void Unload()
    {
        _context?.Unload();
        _context = null;
    }

    static void LoadInner(nint worldPtr, string pluginPath)
    {
        _context = new PluginLoadContext(pluginPath);
        var assembly = _context.LoadFromAssemblyPath(pluginPath);

        // the source generator emits LunarGeneratedHost.ManagedInit(nint) in every
        // plugin assembly — this is the managed-safe entry point for the CoreCLR path
        var type = assembly.GetType("LunarGeneratedHost")
            ?? throw new InvalidOperationException(
                $"LunarGeneratedHost not found in {pluginPath}; " +
                "ensure [LunarPlugin] source generator ran and the project was built for CoreCLR (not NativeAOT)");

        var method = type.GetMethod("ManagedInit", BindingFlags.Public | BindingFlags.Static)
            ?? throw new InvalidOperationException("LunarGeneratedHost.ManagedInit(nint) not found");

        method.Invoke(null, [worldPtr]);
    }
}

/// <summary>
/// isolated load context for one version of the plugin assembly.
/// routes <c>lunar_ffi</c> P/Invoke calls to the host process (where Rust exports them)
/// and uses an <see cref="AssemblyDependencyResolver"/> for the plugin's own dependencies.
/// </summary>
sealed class PluginLoadContext : AssemblyLoadContext
{
    readonly AssemblyDependencyResolver _resolver;

    public PluginLoadContext(string pluginPath) : base(isCollectible: true)
    {
        _resolver = new AssemblyDependencyResolver(pluginPath);
        // the host binary exports all lunar_ffi symbols with -export-dynamic; route
        // P/Invoke calls from the plugin back there rather than searching for a
        // separate liblunar_ffi.so that doesn't exist as a standalone file.
        ResolvingUnmanagedDll += (_, name) =>
            name == "lunar_ffi" ? NativeLibrary.GetMainProgramHandle() : IntPtr.Zero;
    }

    protected override Assembly? Load(AssemblyName name)
    {
        var path = _resolver.ResolveAssemblyToPath(name);
        return path != null ? LoadFromAssemblyPath(path) : null;
    }
}
