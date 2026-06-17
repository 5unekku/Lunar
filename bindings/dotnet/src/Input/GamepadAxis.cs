namespace Lunar;

/// <summary>
/// gamepad axis constants. discriminants match the Rust <c>GamepadAxis</c> enum layout.
/// </summary>
public enum LunarGamepadAxis : uint
{
    LeftStickX  = 0,
    LeftStickY  = 1,
    RightStickX = 2,
    RightStickY = 3,
    LeftTrigger = 4,
    RightTrigger = 5,
}
