using Lunar;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

namespace PlatformDemoCs;

static class EntryPoint
{
    [UnmanagedCallersOnly(EntryPoint = "lunar_plugin_init", CallConvs = [typeof(CallConvCdecl)])]
    public static unsafe void Init(void* world) => Plugin.Run(world, new GamePlugin());
}

class GamePlugin : IPlugin
{
    public void Init(World world)
    {
        world.RegisterSystem(LunarSchedule.Update, new FpsController());
    }
}
