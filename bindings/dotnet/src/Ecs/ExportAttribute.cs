namespace Lunar;

/// <summary>
/// mark a field or property on an <see cref="IBehavior"/> as editor-tunable (the
/// analog of Godot's @export). the source generator emits the field schema plus
/// get/set wiring so the editor inspector and scene format can read and write it.
/// </summary>
[System.AttributeUsage(System.AttributeTargets.Field | System.AttributeTargets.Property)]
public sealed class ExportAttribute : System.Attribute { }
