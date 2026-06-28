using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Text;

namespace Lunar.Native;

/// <summary>
/// managed bridge for per-entity behaviors. mirrors <see cref="LunarHandles"/> but
/// keyed per instance: a behavior id maps to a managed factory, and a single set of
/// <see cref="UnmanagedCallersOnlyAttribute"/> trampolines forwards every
/// lifecycle/field call from the Rust dispatcher to the right managed instance.
///
/// each live behavior is held by a <see cref="GCHandle"/>; the handle's IntPtr is the
/// opaque u64 token the Rust side stores in its <c>CsBehavior</c>.
/// </summary>
internal static unsafe class BehaviorRuntime
{
    static readonly Dictionary<string, Func<IBehavior>> s_factories = new();

    // parks a text payload long enough for the Rust side to copy it after a get_field call
    [ThreadStatic] static nint s_parkedText;

    /// <summary>register a behavior id with a managed factory and wire the trampolines.</summary>
    internal static void Register(LunarWorld* world, string id, Func<IBehavior> factory)
    {
        s_factories[id] = factory;
        var idBytes = Utf8(id);
        fixed (byte* idPtr = idBytes)
        {
            LunarNative.LunarBehaviorRegister(
                world, idPtr,
                &Factory, &Lifecycle, &FieldCount, &FieldSchema, &GetField, &SetField, &Drop);
        }
    }

    static byte[] Utf8(string value)
    {
        var bytes = Encoding.UTF8.GetBytes(value);
        var terminated = new byte[bytes.Length + 1];
        Array.Copy(bytes, terminated, bytes.Length);
        return terminated;
    }

    [UnmanagedCallersOnly(CallConvs = [typeof(CallConvCdecl)])]
    static ulong Factory(byte* idPtr)
    {
        var id = Marshal.PtrToStringUTF8((nint)idPtr) ?? "";
        if (!s_factories.TryGetValue(id, out var make))
            return 0;
        var handle = GCHandle.Alloc(make(), GCHandleType.Normal);
        return (ulong)GCHandle.ToIntPtr(handle);
    }

    static IBehavior Resolve(ulong handle) =>
        (IBehavior)GCHandle.FromIntPtr((nint)handle).Target!;

    [UnmanagedCallersOnly(CallConvs = [typeof(CallConvCdecl)])]
    static void Lifecycle(ulong handle, LunarWorld* world, uint entity, uint stage)
    {
        var instance = Resolve(handle);
        var self = new Entity(entity);
        var w = new World(world);
        switch (stage)
        {
            case 0: instance.OnReady(self, w); break;
            case 1: instance.OnUpdate(self, w); break;
            case 2: instance.OnPhysics(self, w); break;
            case 3: instance.OnDestroy(self, w); break;
        }
    }

    [UnmanagedCallersOnly(CallConvs = [typeof(CallConvCdecl)])]
    static uint FieldCount(ulong handle) => (uint)Resolve(handle).FieldCount;

    [UnmanagedCallersOnly(CallConvs = [typeof(CallConvCdecl)])]
    static byte FieldSchema(ulong handle, uint index, LunarFieldSchema* outSchema)
    {
        if (!Resolve(handle).GetFieldSchema((int)index, out var name, out var kind))
            return 0;
        WriteName(name, outSchema->Name);
        outSchema->Kind = (uint)kind;
        return 1;
    }

    [UnmanagedCallersOnly(CallConvs = [typeof(CallConvCdecl)])]
    static byte GetField(ulong handle, byte* namePtr, LunarFieldValue* outValue)
    {
        var name = Marshal.PtrToStringUTF8((nint)namePtr) ?? "";
        if (!Resolve(handle).GetField(name, out var value))
            return 0;
        ToNative(value, outValue);
        return 1;
    }

    [UnmanagedCallersOnly(CallConvs = [typeof(CallConvCdecl)])]
    static byte SetField(ulong handle, byte* namePtr, LunarFieldValue* valuePtr)
    {
        var name = Marshal.PtrToStringUTF8((nint)namePtr) ?? "";
        Resolve(handle).SetField(name, FromNative(valuePtr));
        return 1;
    }

    [UnmanagedCallersOnly(CallConvs = [typeof(CallConvCdecl)])]
    static void Drop(ulong handle) => GCHandle.FromIntPtr((nint)handle).Free();

    // ── native <-> managed field value conversion (also used by World) ──────────

    internal static void ToNative(FieldValue value, LunarFieldValue* outValue)
    {
        *outValue = default;
        outValue->Kind = (uint)value.Kind;
        switch (value.Kind)
        {
            case FieldKind.Float:
                outValue->FloatValue = value.Float;
                break;
            case FieldKind.Int:
                outValue->IntValue = value.Int;
                break;
            case FieldKind.Bool:
                outValue->BoolValue = value.Bool ? (byte)1 : (byte)0;
                break;
            case FieldKind.Vec3:
                outValue->Vec[0] = value.Vec3.X;
                outValue->Vec[1] = value.Vec3.Y;
                outValue->Vec[2] = value.Vec3.Z;
                break;
            case FieldKind.Color:
                outValue->Vec[0] = value.Color.R;
                outValue->Vec[1] = value.Color.G;
                outValue->Vec[2] = value.Color.B;
                outValue->Vec[3] = value.Color.A;
                break;
            case FieldKind.Text:
                // free the previous parked text, park the new one for the duration of the call
                if (s_parkedText != 0)
                    Marshal.FreeCoTaskMem(s_parkedText);
                s_parkedText = Marshal.StringToCoTaskMemUTF8(value.Text);
                outValue->Text = (byte*)s_parkedText;
                break;
        }
    }

    internal static FieldValue FromNative(LunarFieldValue* value) =>
        (FieldKind)value->Kind switch
        {
            FieldKind.Float => FieldValue.OfFloat(value->FloatValue),
            FieldKind.Int => FieldValue.OfInt(value->IntValue),
            FieldKind.Bool => FieldValue.OfBool(value->BoolValue != 0),
            FieldKind.Vec3 => FieldValue.OfVec3(value->Vec[0], value->Vec[1], value->Vec[2]),
            FieldKind.Color => FieldValue.OfColor(value->Vec[0], value->Vec[1], value->Vec[2], value->Vec[3]),
            FieldKind.Text => FieldValue.OfText(
                value->Text == null ? "" : Marshal.PtrToStringUTF8((nint)value->Text) ?? ""),
            _ => FieldValue.OfFloat(0),
        };

    static void WriteName(string name, byte* buffer)
    {
        var bytes = Encoding.UTF8.GetBytes(name);
        int count = Math.Min(bytes.Length, 63);
        for (int i = 0; i < count; i++)
            buffer[i] = bytes[i];
        buffer[count] = 0;
    }
}
