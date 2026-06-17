namespace Lunar;

/// <summary>
/// implement this to define your plugin's initialization logic.
/// the engine calls <see cref="Init"/> once when the plugin is loaded,
/// before the first Update tick runs.
///
/// to export your plugin as a native shared library, implement this interface and
/// add an unmanaged entry point in your project:
/// <code>
/// [UnmanagedCallersOnly(EntryPoint = "lunar_plugin_init", CallConvs = [typeof(CallConvCdecl)])]
/// public static unsafe void Init(LunarWorld* world) => new MyPlugin().Init(new World(world));
/// </code>
/// </summary>
public interface IPlugin
{
    void Init(World world);
}
