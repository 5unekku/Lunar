namespace Lunar;

/// <summary>handle to a material in the engine registry. opaque: do not inspect the value.</summary>
public readonly struct MaterialHandle(ulong raw)
{
    internal ulong Raw { get; } = raw;

    /// <summary>null/invalid handle sentinel.</summary>
    public static MaterialHandle Null => new(ulong.MaxValue);

    public bool IsValid => Raw != ulong.MaxValue;
}
