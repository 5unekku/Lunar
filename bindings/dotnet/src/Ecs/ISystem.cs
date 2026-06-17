namespace Lunar;

/// <summary>
/// the only scripting extension point. implement this and register via <see cref="World.RegisterSystem"/>.
/// no inheritance hierarchy — the engine calls <see cref="Update"/> through an unmanaged trampoline.
/// </summary>
public interface ISystem
{
    void Update(World world);
}
