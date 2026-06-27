namespace Lunar;

/// <summary>handle to a mesh in the engine registry. opaque: do not inspect the value.</summary>
public readonly struct MeshHandle(ulong raw)
{
    internal ulong Raw { get; } = raw;

    /// <summary>null/invalid handle sentinel.</summary>
    public static MeshHandle Null => new(ulong.MaxValue);

    public bool IsValid => Raw != ulong.MaxValue;
}
