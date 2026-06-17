using Lunar.Native;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

namespace Lunar;

/// <summary>
/// fluent builder for entity queries. allocates id arrays on the stack (zero heap allocation on hot path).
///
/// this is a <c>ref struct</c> because it contains the world pointer, which must not outlive the callback.
/// </summary>
public unsafe ref struct QueryBuilder
{
    readonly LunarWorld* _world;
    // stack-allocated id buffers — 32 slots is sufficient for almost all queries
    uint* _include;
    int   _includeCount;
    uint* _exclude;
    int   _excludeCount;

    internal QueryBuilder(LunarWorld* world)
    {
        _world = world;
        _include = stackalloc uint[32];
        _exclude = stackalloc uint[32];
    }

    public QueryBuilder With(ComponentId id)
    {
        _include[_includeCount++] = id.Id;
        return this;
    }

    public QueryBuilder Without(ComponentId id)
    {
        _exclude[_excludeCount++] = id.Id;
        return this;
    }

    /// <summary>
    /// iterate matching entities. `callback` receives the entity by value.
    /// world access is safe because World is a ref struct that cannot outlive the frame.
    /// </summary>
    public void ForEach(Action<Entity> callback)
    {
        // capture for the unmanaged delegate
        var cb = callback;
        QueryCallback wrapper = (entity, _) => cb(new Entity(entity));
        var handle = GCHandle.Alloc(wrapper, GCHandleType.Normal);
        try
        {
            LunarNative.LunarQueryForeach(
                _world,
                _include, (nuint)_includeCount,
                _exclude, (nuint)_excludeCount,
                &QueryTrampoline,
                (void*)GCHandle.ToIntPtr(handle));
        }
        finally { handle.Free(); }
    }

    delegate void QueryCallback(uint entity, void* userData);

    [UnmanagedCallersOnly(CallConvs = [typeof(CallConvCdecl)])]
    static void QueryTrampoline(uint entity, void* userData)
    {
        var handle = GCHandle.FromIntPtr((nint)userData);
        ((QueryCallback)handle.Target!)(entity, null);
    }
}
