using System.Numerics;
using System.Runtime.InteropServices;

namespace Lunar;

/// <summary>
/// 2D position, rotation (radians), and scale.
/// maps to <c>LocalTransform</c> in the engine via the typed FFI accessors.
/// </summary>
[StructLayout(LayoutKind.Sequential)]
public struct Transform2d
{
    public Vector2 Translation;
    public float   Rotation;
    public Vector2 Scale;

    public Transform2d(Vector2 translation, float rotation, Vector2 scale)
    {
        Translation = translation;
        Rotation    = rotation;
        Scale       = scale;
    }

    public static Transform2d Identity => new(Vector2.Zero, 0f, Vector2.One);
}
