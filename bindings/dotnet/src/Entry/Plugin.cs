using Lunar.Native;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

namespace Lunar;

/// <summary>
/// NativeAOT entry point. every C# plugin must contain exactly one class
/// that implements <see cref="IPlugin"/> — the runtime discovers it via
/// <see cref="InitPlugin"/>, which is called by the engine's plugin loader.
///
/// example:
/// <code>
/// public sealed class MyPlugin : IPlugin
/// {
///     public void Init(World world)
///     {
///         world.RegisterSystem(LunarSchedule.Update, new MySystem());
///     }
/// }
/// </code>
/// </summary>
public interface IPlugin
{
    void Init(World world);
}

/// <summary>
/// static class that provides the unmanaged entry point the engine calls after dlopen.
/// you do not call this directly — the engine plugin loader does.
/// </summary>
public static unsafe class Plugin
{
    static IPlugin? s_instance;

    /// <summary>
    /// find and activate the single <see cref="IPlugin"/> implementation in this assembly.
    /// called by the engine immediately after loading the plugin shared library.
    /// </summary>
    [UnmanagedCallersOnly(EntryPoint = "lunar_plugin_init", CallConvs = [typeof(CallConvCdecl)])]
    public static void InitPlugin(LunarWorld* world)
    {
        // scan loaded assemblies for a concrete IPlugin
        foreach (var assembly in AppDomain.CurrentDomain.GetAssemblies())
        {
            foreach (var type in assembly.GetTypes())
            {
                if (type.IsAbstract || type.IsInterface) continue;
                if (!typeof(IPlugin).IsAssignableFrom(type)) continue;
                s_instance = (IPlugin)Activator.CreateInstance(type)!;
                break;
            }
            if (s_instance is not null) break;
        }

        if (s_instance is null)
        {
            System.Diagnostics.Debug.Fail("no IPlugin implementation found in this assembly");
            return;
        }

        s_instance.Init(new World(world));
    }
}
