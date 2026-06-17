using Lunar.Native;
using System.Runtime.InteropServices;

namespace Lunar;

/// <summary>implement this to define your plugin's initialization logic.</summary>
/// <remarks>
/// the engine calls <see cref="Init"/> once when the plugin is loaded,
/// before the first Update tick runs. pair with <see cref="Plugin.Run"/> in your
/// NativeAOT entry point:
/// <code>
/// [UnmanagedCallersOnly(EntryPoint = "lunar_plugin_init", CallConvs = [typeof(CallConvCdecl)])]
/// public static unsafe void Init(void* world) => Plugin.Run(world, new MyPlugin());
/// </code>
/// </remarks>
public interface IPlugin
{
    void Init(World world);
}

/// <summary>entry-point helpers for NativeAOT plugins.</summary>
public static unsafe class Plugin
{
    /// <summary>
    /// call from the <c>[UnmanagedCallersOnly]</c> entry point to hand control to
    /// a managed <see cref="IPlugin"/> implementation.
    /// hides the raw world pointer so game code stays fully managed.
    /// </summary>
    /// <example>
    /// <code>
    /// [UnmanagedCallersOnly(EntryPoint = "lunar_plugin_init", CallConvs = [typeof(CallConvCdecl)])]
    /// public static unsafe void Init(void* world) => Plugin.Run(world, new MyPlugin());
    /// </code>
    /// </example>
    public static void Run(void* world, IPlugin plugin) =>
        plugin.Init(new World((LunarWorld*)world));
}
