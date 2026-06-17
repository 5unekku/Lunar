using Lunar;
using Lunar.Native;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

namespace PlatformDemoCs;

/// <summary>
/// NativeAOT entry point. the engine's plugin loader dlopen's this library and
/// calls <c>lunar_plugin_init</c>. no reflection — all wiring is explicit.
/// </summary>
static unsafe class EntryPoint
{
    [UnmanagedCallersOnly(EntryPoint = "lunar_plugin_init", CallConvs = [typeof(CallConvCdecl)])]
    public static void Init(LunarWorld* world)
    {
        var w = new World(world);
        w.RegisterSystem(LunarSchedule.Update, new FpsController());
    }
}
