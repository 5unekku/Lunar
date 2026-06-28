namespace Lunar;

/// <summary>
/// per-entity behavior: attach to an entity and the engine calls these hooks for
/// that entity each frame. default methods mean you implement only what you need.
///
/// register a behavior type once with <see cref="World.RegisterBehavior{T}"/>, then
/// attach instances with <see cref="World.AttachBehavior"/> (or via the scene file).
/// the field methods carry exported (<c>[Export]</c>) fields to the editor; the
/// source generator overrides them for behaviors that declare exported fields.
/// </summary>
public interface IBehavior
{
    void OnReady(Entity self, World world) { }
    void OnUpdate(Entity self, World world) { }
    void OnPhysics(Entity self, World world) { }
    void OnDestroy(Entity self, World world) { }

    /// <summary>number of exported fields. the generator overrides this.</summary>
    int FieldCount => 0;

    /// <summary>schema (name + kind) of the exported field at <paramref name="index"/>.</summary>
    bool GetFieldSchema(int index, out string name, out FieldKind kind)
    {
        name = "";
        kind = FieldKind.Float;
        return false;
    }

    /// <summary>read the exported field named <paramref name="name"/>.</summary>
    bool GetField(string name, out FieldValue value)
    {
        value = FieldValue.OfFloat(0);
        return false;
    }

    /// <summary>write the exported field named <paramref name="name"/>.</summary>
    void SetField(string name, FieldValue value) { }
}
