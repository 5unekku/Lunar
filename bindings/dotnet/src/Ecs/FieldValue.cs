namespace Lunar;

/// <summary>kind tags for an exported behavior field, mirroring LUNAR_FIELD_KIND_* in lunar.h.</summary>
public enum FieldKind : uint
{
    Float = 0,
    Int = 1,
    Bool = 2,
    Vec3 = 3,
    Color = 4,
    Text = 5,
}

/// <summary>
/// a tunable behavior field value (the C# mirror of lunar-core's FieldValue and the
/// FFI LunarFieldValue union). use the static factory methods to construct one.
/// </summary>
public readonly struct FieldValue
{
    public FieldKind Kind { get; }
    public float Float { get; }
    public long Int { get; }
    public bool Bool { get; }
    public (float X, float Y, float Z) Vec3 { get; }
    public (float R, float G, float B, float A) Color { get; }
    public string Text { get; }

    private FieldValue(
        FieldKind kind,
        float floatValue,
        long intValue,
        bool boolValue,
        (float, float, float) vec3,
        (float, float, float, float) color,
        string text)
    {
        Kind = kind;
        Float = floatValue;
        Int = intValue;
        Bool = boolValue;
        Vec3 = vec3;
        Color = color;
        Text = text ?? "";
    }

    public static FieldValue OfFloat(float value) =>
        new(FieldKind.Float, value, 0, false, default, default, "");
    public static FieldValue OfInt(long value) =>
        new(FieldKind.Int, 0, value, false, default, default, "");
    public static FieldValue OfBool(bool value) =>
        new(FieldKind.Bool, 0, 0, value, default, default, "");
    public static FieldValue OfVec3(float x, float y, float z) =>
        new(FieldKind.Vec3, 0, 0, false, (x, y, z), default, "");
    public static FieldValue OfColor(float r, float g, float b, float a) =>
        new(FieldKind.Color, 0, 0, false, default, (r, g, b, a), "");
    public static FieldValue OfText(string value) =>
        new(FieldKind.Text, 0, 0, false, default, default, value);
}
