namespace Lunar;

/// <summary>
/// marks a class as the lunar plugin entry point.
/// the source generator produces the <c>[UnmanagedCallersOnly]</c> boilerplate automatically.
/// exactly one class per assembly may carry this attribute.
/// </summary>
[AttributeUsage(AttributeTargets.Class, AllowMultiple = false, Inherited = false)]
public sealed class LunarPluginAttribute : Attribute { }
