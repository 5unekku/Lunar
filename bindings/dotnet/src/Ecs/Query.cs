using Lunar.Native;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

namespace Lunar;

/// <summary>
/// fixed-size inline buffer for up to 32 component ids. stored directly in the
/// <see cref="QueryBuilder"/> struct so no stackalloc or heap allocation is needed.
/// </summary>
[InlineArray(32)]
internal struct ComponentIdBuffer { private uint _first; }

/// <summary>
/// fluent builder for entity queries. component id arrays live inline in the struct
/// (via <see cref="ComponentIdBuffer"/>) — zero heap allocation on the hot path.
///
/// this is a <c>ref struct</c> because it holds a raw world pointer that must not
/// outlive the callback. max 32 include or exclude components per query.
/// </summary>
public unsafe ref struct QueryBuilder
{
    readonly LunarWorld* _world;
    ComponentIdBuffer _include;
    int               _includeCount;
    ComponentIdBuffer _exclude;
    int               _excludeCount;

    internal QueryBuilder(LunarWorld* world) => _world = world;

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
    /// iterate matching entities. <paramref name="callback"/> receives each entity by value.
    /// </summary>
    public void ForEach(Action<Entity> callback)
    {
        var cb = callback;
        QueryCallback wrapper = (entity, _) => cb(new Entity(entity));
        var handle = GCHandle.Alloc(wrapper, GCHandleType.Normal);
        try
        {
            fixed (uint* includePtr = &_include[0])
            fixed (uint* excludePtr = &_exclude[0])
            {
                LunarNative.LunarQueryForeach(
                    _world,
                    includePtr, (nuint)_includeCount,
                    excludePtr, (nuint)_excludeCount,
                    &QueryTrampoline,
                    (void*)GCHandle.ToIntPtr(handle));
            }
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
