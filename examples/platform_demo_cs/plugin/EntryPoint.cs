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
        // lunar_ffi symbols live in the host executable (statically linked, exported via
        // -export-dynamic). redirect all [LibraryImport("lunar_ffi")] calls to the main
        // process handle so the NativeAOT runtime finds them without a separate .so.
        // World is in the same assembly as LunarNative so typeof(World).Assembly
        // is the right handle without needing to expose LunarNative as public.
        NativeLibrary.SetDllImportResolver(
            typeof(World).Assembly,
            static (name, _, _) => name == "lunar_ffi"
                ? NativeLibrary.GetMainProgramHandle()
                : IntPtr.Zero);

        var w = new World(world);
        w.RegisterSystem(LunarSchedule.Update, new FpsController());
    }
}
