using Lunar.Native;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

namespace Lunar;

/// <summary>
/// access to the engine world during a system callback.
///
/// this is a <c>ref struct</c> so the compiler prevents it from being heap-allocated
/// or stored in a field — enforcing the "only valid during callback" rule at the type level.
/// </summary>
public unsafe ref struct World
{
    readonly LunarWorld* _handle;

    internal World(LunarWorld* handle) => _handle = handle;

    // ── entities ──────────────────────────────────────────────────────────────

    public Entity Spawn()               => new(LunarNative.LunarSpawn(_handle));
    public void   Despawn(Entity entity) => LunarNative.LunarDespawn(_handle, entity.Id);
    public bool   IsAlive(Entity entity) => LunarNative.LunarAlive(_handle, entity.Id);

    // ── time ──────────────────────────────────────────────────────────────────

    public float DeltaSeconds   => LunarNative.LunarDeltaSeconds(_handle);
    public float ElapsedSeconds => LunarNative.LunarElapsedSeconds(_handle);

    // ── component registration ────────────────────────────────────────────────

    public ComponentId RegisterComponent<T>(ReadOnlySpan<byte> name) where T : unmanaged
    {
        fixed (byte* namePtr = name)
        {
            var id = LunarNative.LunarComponentRegister(
                _handle, namePtr,
                (nuint)sizeof(T),
                (nuint)Unsafe.AlignOf<T>());
            return new ComponentId(id);
        }
    }

    public ComponentId GetComponentId(ReadOnlySpan<byte> name)
    {
        fixed (byte* namePtr = name)
            return new ComponentId(LunarNative.LunarComponentId(_handle, namePtr));
    }

    // ── component access ──────────────────────────────────────────────────────

    public void Insert<T>(Entity entity, ComponentId id, in T value) where T : unmanaged
    {
        fixed (T* ptr = &value)
            LunarNative.LunarComponentInsert(_handle, entity.Id, id.Id, ptr, (nuint)sizeof(T));
    }

    public void Remove(Entity entity, ComponentId id) =>
        LunarNative.LunarComponentRemove(_handle, entity.Id, id.Id);

    public bool Has(Entity entity, ComponentId id) =>
        LunarNative.LunarComponentHas(_handle, entity.Id, id.Id);

    public ref readonly T Get<T>(Entity entity, ComponentId id) where T : unmanaged
    {
        var ptr = (T*)LunarNative.LunarComponentGet(_handle, entity.Id, id.Id);
        return ref System.Runtime.CompilerServices.Unsafe.AsRef<T>(ptr);
    }

    public ref T GetMut<T>(Entity entity, ComponentId id) where T : unmanaged
    {
        var ptr = (T*)LunarNative.LunarComponentGetMut(_handle, entity.Id, id.Id);
        return ref System.Runtime.CompilerServices.Unsafe.AsRef<T>(ptr);
    }

    // ── transforms ────────────────────────────────────────────────────────────

    public bool GetTransform3d(Entity entity, out Transform3d transform)
    {
        fixed (Transform3d* ptr = &transform)
        {
            var native = default(LunarTransform3d);
            if (!LunarNative.LunarGetTransform3d(_handle, entity.Id, &native))
            {
                transform = default;
                return false;
            }
            ptr->Translation = new(native.Translation.X, native.Translation.Y, native.Translation.Z);
            ptr->Rotation    = new(native.Rotation.X, native.Rotation.Y, native.Rotation.Z, native.Rotation.W);
            ptr->Scale       = new(native.Scale.X, native.Scale.Y, native.Scale.Z);
            return true;
        }
    }

    public bool SetTransform3d(Entity entity, in Transform3d transform)
    {
        var native = new LunarTransform3d
        {
            Translation = new() { X = transform.Translation.X, Y = transform.Translation.Y, Z = transform.Translation.Z },
            Rotation    = new() { X = transform.Rotation.X,    Y = transform.Rotation.Y,    Z = transform.Rotation.Z,   W = transform.Rotation.W },
            Scale       = new() { X = transform.Scale.X,       Y = transform.Scale.Y,       Z = transform.Scale.Z },
        };
        return LunarNative.LunarSetTransform3d(_handle, entity.Id, &native);
    }

    public bool GetTransform2d(Entity entity, out Transform2d transform)
    {
        var native = default(LunarTransform2d);
        if (!LunarNative.LunarGetTransform2d(_handle, entity.Id, &native))
        {
            transform = default;
            return false;
        }
        transform = new Transform2d(
            new(native.Translation.X, native.Translation.Y),
            native.Rotation,
            new(native.Scale.X, native.Scale.Y));
        return true;
    }

    public bool SetTransform2d(Entity entity, in Transform2d transform)
    {
        var native = new LunarTransform2d
        {
            Translation = new() { X = transform.Translation.X, Y = transform.Translation.Y },
            Rotation    = transform.Rotation,
            Scale       = new() { X = transform.Scale.X, Y = transform.Scale.Y },
        };
        return LunarNative.LunarSetTransform2d(_handle, entity.Id, &native);
    }

    // ── query ─────────────────────────────────────────────────────────────────

    public QueryBuilder Query() => new(_handle);

    // ── input ─────────────────────────────────────────────────────────────────

    public bool IsKeyHeld(LunarKey key) =>
        LunarNative.LunarInputKeyHeld(_handle, (uint)key);

    public bool IsKeyJustPressed(LunarKey key) =>
        LunarNative.LunarInputKeyJustPressed(_handle, (uint)key);

    public void MouseDelta(out float dx, out float dy)
    {
        float x, y;
        LunarNative.LunarInputMouseDelta(_handle, &x, &y);
        dx = x; dy = y;
    }

    public float GamepadAxis(uint index, LunarGamepadAxis axis) =>
        LunarNative.LunarInputGamepadAxis(_handle, index, (uint)axis);

    // ── main camera ───────────────────────────────────────────────────────────

    /// <summary>entity set by the host via <c>set_main_camera_entity</c>, or an invalid entity if unset.</summary>
    public Entity MainCamera => new(LunarNative.LunarGetMainCamera(_handle));

    // ── system registration ───────────────────────────────────────────────────

    public uint RegisterSystem(LunarSchedule schedule, ISystem system) =>
        LunarHandles.RegisterSystem(_handle, (uint)schedule, system);

    public void UnregisterSystem(uint id) =>
        LunarHandles.UnregisterSystem(_handle, id);
}

/// <summary>schedule constants matching LUNAR_SCHEDULE_* in lunar.h.</summary>
public enum LunarSchedule : uint
{
    Startup     = 0,
    Update      = 1,
    FixedUpdate = 2,
    Shutdown    = 3,
}
