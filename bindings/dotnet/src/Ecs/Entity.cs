namespace Lunar;

/// <summary>entity identifier: index into the engine world's entity table.</summary>
public readonly record struct Entity(uint Id)
{
    /// <summary>entity index as a plain integer (for interop or debug logging).</summary>
    public uint RawIndex => Id;
}
