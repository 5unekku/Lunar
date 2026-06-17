using System.Numerics;
using System.Runtime.InteropServices;

namespace Lunar;

/// <summary>
/// 3D position, orientation, and scale.
/// this maps to <c>LocalTransform3d</c> in the engine via the typed FFI accessors.
/// </summary>
[StructLayout(LayoutKind.Sequential)]
public struct Transform3d
{
    public Vector3    Translation;
    public Quaternion Rotation;
    public Vector3    Scale;

    public Transform3d(Vector3 translation, Quaternion rotation, Vector3 scale)
    {
        Translation = translation;
        Rotation    = rotation;
        Scale       = scale;
    }

    public static Transform3d Identity => new(Vector3.Zero, Quaternion.Identity, Vector3.One);
}
