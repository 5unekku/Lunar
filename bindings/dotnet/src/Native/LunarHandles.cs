using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

namespace Lunar.Native;

/// <summary>
/// manages GCHandle lifetimes for ISystem instances registered across the C boundary.
///
/// systems are kept alive via <see cref="GCHandle.Alloc"/> until explicitly unregistered.
/// all registered systems share a single <see cref="SystemTrampoline"/> entry point.
/// </summary>
internal static unsafe class LunarHandles
{
    static readonly Dictionary<uint, GCHandle> s_handles = new();

    internal static uint RegisterSystem(LunarWorld* world, uint schedule, ISystem system)
    {
        var handle = GCHandle.Alloc(system, GCHandleType.Normal);
        var id = LunarNative.LunarSystemRegister(
            world, schedule,
            &SystemTrampoline,
            (void*)GCHandle.ToIntPtr(handle));

        if (id == uint.MaxValue)
        {
            handle.Free();
            return uint.MaxValue;
        }

        s_handles[id] = handle;
        return id;
    }

    internal static void UnregisterSystem(LunarWorld* world, uint id)
    {
        LunarNative.LunarSystemUnregister(world, id);
        if (s_handles.Remove(id, out var handle))
            handle.Free();
    }

    /// <summary>
    /// single static trampoline for all ISystem implementations.
    /// [UnmanagedCallersOnly] gives a compile-time-constant function pointer in NativeAOT.
    /// </summary>
    [UnmanagedCallersOnly(CallConvs = [typeof(CallConvCdecl)])]
    private static void SystemTrampoline(LunarWorld* world, void* userData)
    {
        var handle = GCHandle.FromIntPtr((nint)userData);
        ((ISystem)handle.Target!).Update(new World(world));
    }
}
