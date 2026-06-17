namespace Lunar;

/// <summary>
/// typed wrapper for a registered component id.
/// prevents accidentally mixing ids from different component types.
/// </summary>
public readonly record struct ComponentId(uint Id);
