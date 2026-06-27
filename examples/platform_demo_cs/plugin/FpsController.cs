using Lunar;
using System.Numerics;

namespace PlatformDemoCs;

/// <summary>
/// first-person movement controller. mirrors the Rust fps_controller system from platform_demo.
/// yaw/pitch state is kept as instance fields: the GCHandle keeps this object alive across frames.
/// </summary>
sealed class FpsController : ISystem
{
    float _yaw;
    float _pitch = MathF.PI / 2f; // start level (zenith 90° = π/2)

    const float HalfPlatform  = 2.0f;
    const float EyeHeight     = 1.7f;
    const float WalkSpeed     = 4.0f;
    const float Sensitivity   = 0.002f;
    const float Deadzone      = 0.15f;
    const float StickLookSpeed = 2.5f;

    static float ApplyDeadzone(float value)
    {
        float sign = MathF.Sign(value);
        float abs  = MathF.Abs(value);
        return abs < Deadzone ? 0f : sign * (abs - Deadzone) / (1f - Deadzone);
    }

    public void Update(World world)
    {
        Entity camera = world.MainCamera;
        if (camera.Id == uint.MaxValue) return;

        float dt = world.DeltaSeconds;

        // quit
        if (world.IsKeyJustPressed(LunarKey.Escape))
        {
            Environment.Exit(0);
            return;
        }

        // look input: mouse + right stick
        world.MouseDelta(out float dx, out float dy);
        float stickRx = ApplyDeadzone(world.GamepadAxis(0, LunarGamepadAxis.RightStickX));
        float stickRy = ApplyDeadzone(world.GamepadAxis(0, LunarGamepadAxis.RightStickY));

        _yaw  -= dx * Sensitivity + stickRx * StickLookSpeed * dt;
        _pitch = float.Clamp(
            _pitch + dy * Sensitivity + stickRy * StickLookSpeed * dt,
            0.001f, MathF.PI - 0.001f);

        if (!world.GetTransform3d(camera, out Transform3d transform)) return;

        // move input: WASD + left stick
        // forward = direction camera faces (ignores pitch: ground-plane movement only)
        var forward = new Vector3(-MathF.Sin(_yaw), 0f, -MathF.Cos(_yaw));
        var right   = new Vector3(-forward.Z, 0f, forward.X);

        float stickMx = ApplyDeadzone(world.GamepadAxis(0, LunarGamepadAxis.LeftStickX));
        float stickMy = ApplyDeadzone(world.GamepadAxis(0, LunarGamepadAxis.LeftStickY));

        float moveX = (world.IsKeyHeld(LunarKey.D) ? 1f : 0f)
                    - (world.IsKeyHeld(LunarKey.A) ? 1f : 0f)
                    + stickMx;
        float moveZ = (world.IsKeyHeld(LunarKey.S) ? 1f : 0f)
                    - (world.IsKeyHeld(LunarKey.W) ? 1f : 0f)
                    + stickMy;

        float inputLen = MathF.Sqrt(moveX * moveX + moveZ * moveZ);
        if (inputLen > 1f) { moveX /= inputLen; moveZ /= inputLen; }

        Vector3 pos   = transform.Translation;
        float   speed = WalkSpeed * dt;
        pos += forward * (-moveZ * speed) + right * (moveX * speed);

        // keep inside the platform + lock Y to eye height
        float limit = HalfPlatform - 0.1f;
        pos = new(float.Clamp(pos.X, -limit, limit), EyeHeight, float.Clamp(pos.Z, -limit, limit));

        transform.Translation = pos;
        // yaw around world Y, then pitch around local X.
        // zenith pitch: 0=up, π/2=level, π=down → rotation_x(π/2 - pitch)
        transform.Rotation =
            Quaternion.CreateFromAxisAngle(Vector3.UnitY, _yaw) *
            Quaternion.CreateFromAxisAngle(Vector3.UnitX, MathF.PI / 2f - _pitch);

        world.SetTransform3d(camera, in transform);
    }
}
