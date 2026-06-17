namespace Lunar;

/// <summary>
/// key code constants. discriminants match the Rust <c>KeyCode</c> enum layout exactly
/// so they can be passed directly to <c>lunar_input_key_held</c> / <c>lunar_input_key_just_pressed</c>.
/// </summary>
public enum LunarKey : uint
{
    A = 0, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z,

    Num0 = 26, Num1, Num2, Num3, Num4, Num5, Num6, Num7, Num8, Num9,

    F1 = 36, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,

    Escape    = 48,
    Space     = 49,
    Enter     = 50,
    Tab       = 51,
    Backspace = 52,
    Left      = 53,
    Right     = 54,
    Up        = 55,
    Down      = 56,

    LShift = 57, RShift,
    LCtrl  = 59, RCtrl,
    LAlt   = 61, RAlt,

    Minus        = 63,
    Equals       = 64,
    Semicolon    = 65,
    Apostrophe   = 66,
    Comma        = 67,
    Period       = 68,
    Slash        = 69,
    Backslash    = 70,
    LeftBracket  = 71,
    RightBracket = 72,
    Grave        = 73,

    Home     = 74, End, PageUp, PageDown, Insert, Delete,

    Numpad0       = 80, Numpad1, Numpad2, Numpad3, Numpad4,
    Numpad5       = 85, Numpad6, Numpad7, Numpad8, Numpad9,
    NumpadAdd     = 90, NumpadSub, NumpadMul, NumpadDiv, NumpadEnter, NumpadDecimal, NumLock,

    CapsLock   = 97, ScrollLock, Pause, PrintScreen,
    LSuper     = 101, RSuper,

    MediaPlay  = 103, MediaStop, MediaNext, MediaPrev,
    VolumeUp   = 107, VolumeDown, Mute,

    F13 = 128, F14, F15, F16, F17, F18, F19, F20, F21, F22, F23, F24,
}
