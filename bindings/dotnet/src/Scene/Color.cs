namespace Lunar;

/// <summary>linear RGBA color, each channel in 0..1.</summary>
public readonly struct Color(float r, float g, float b, float a = 1.0f)
{
    public float R { get; } = r;
    public float G { get; } = g;
    public float B { get; } = b;
    public float A { get; } = a;

    public static Color White => new(1, 1, 1);
    public static Color Black => new(0, 0, 0);
    public static Color Red   => new(1, 0, 0);
    public static Color Green => new(0, 1, 0);
    public static Color Blue  => new(0, 0, 1);
}
