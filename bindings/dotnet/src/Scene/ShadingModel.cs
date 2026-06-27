namespace Lunar;

/// <summary>how a material surface responds to lighting.</summary>
public enum ShadingModel : uint
{
    /// <summary>no lighting: full-bright color (HUD, debug geometry).</summary>
    Unlit = 0,
    /// <summary>classic diffuse + specular (default).</summary>
    Phong = 1,
    /// <summary>metallic-roughness PBR.</summary>
    Pbr   = 2,
}
