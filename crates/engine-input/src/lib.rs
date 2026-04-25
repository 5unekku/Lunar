//! input subsystem via SDL3
//!
//! handles keyboard, mouse, gamepad input. exposes state through clean interfaces.

use bevy_ecs::prelude::*;
use engine_core::{App, GamePlugin};

/// keyboard key codes mapped from SDL3
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyCode {
    /// a-z keys
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    /// 0-9 keys
    Num0,
    Num1,
    Num2,
    Num3,
    Num4,
    Num5,
    Num6,
    Num7,
    Num8,
    Num9,
    /// function keys
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    /// special keys
    Escape,
    Space,
    Enter,
    Tab,
    Backspace,
    Left,
    Right,
    Up,
    Down,
    /// modifier keys
    LShift,
    RShift,
    LCtrl,
    RCtrl,
    LAlt,
    RAlt,
}

/// mouse button codes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    /// left mouse button
    Left,
    /// right mouse button
    Right,
    /// middle mouse button
    Middle,
}

/// input state resource, tracks current and previous frame input
#[derive(Resource, Clone)]
pub struct InputState {
    /// keys currently held down
    keys_held: std::collections::HashSet<KeyCode>,
    /// keys pressed this frame
    keys_just_pressed: std::collections::HashSet<KeyCode>,
    /// keys released this frame
    keys_just_released: std::collections::HashSet<KeyCode>,
    /// current mouse position
    mouse_position: (f32, f32),
    /// mouse delta this frame
    mouse_delta: (f32, f32),
    /// mouse buttons currently held down
    mouse_buttons_held: std::collections::HashSet<MouseButton>,
    /// mouse buttons just pressed this frame
    mouse_buttons_just_pressed: std::collections::HashSet<MouseButton>,
    /// mouse buttons just released this frame
    mouse_buttons_just_released: std::collections::HashSet<MouseButton>,
}

impl InputState {
    /// create a new empty input state
    pub fn new() -> Self {
        Self {
            keys_held: std::collections::HashSet::new(),
            keys_just_pressed: std::collections::HashSet::new(),
            keys_just_released: std::collections::HashSet::new(),
            mouse_position: (0.0, 0.0),
            mouse_delta: (0.0, 0.0),
            mouse_buttons_held: std::collections::HashSet::new(),
            mouse_buttons_just_pressed: std::collections::HashSet::new(),
            mouse_buttons_just_released: std::collections::HashSet::new(),
        }
    }

    /// check if a key is currently held down
    pub fn is_key_held(&self, key: KeyCode) -> bool {
        self.keys_held.contains(&key)
    }

    /// check if a key was just pressed this frame
    pub fn is_key_just_pressed(&self, key: KeyCode) -> bool {
        self.keys_just_pressed.contains(&key)
    }

    /// check if a key was just released this frame
    pub fn is_key_just_released(&self, key: KeyCode) -> bool {
        self.keys_just_released.contains(&key)
    }

    /// get the current mouse position
    pub fn mouse_position(&self) -> (f32, f32) {
        self.mouse_position
    }

    /// get the mouse movement delta this frame
    pub fn mouse_delta(&self) -> (f32, f32) {
        self.mouse_delta
    }

    /// check if a mouse button is currently held down
    pub fn is_mouse_button_held(&self, button: MouseButton) -> bool {
        self.mouse_buttons_held.contains(&button)
    }

    /// check if a mouse button was just pressed this frame
    pub fn is_mouse_button_just_pressed(&self, button: MouseButton) -> bool {
        self.mouse_buttons_just_pressed.contains(&button)
    }

    /// check if a mouse button was just released this frame
    pub fn is_mouse_button_just_released(&self, button: MouseButton) -> bool {
        self.mouse_buttons_just_released.contains(&button)
    }

    /// begin frame: clear just_pressed/just_released sets
    pub fn begin_frame(&mut self) {
        self.keys_just_pressed.clear();
        self.keys_just_released.clear();
        self.mouse_buttons_just_pressed.clear();
        self.mouse_buttons_just_released.clear();
        self.mouse_delta = (0.0, 0.0);
    }

    /// press a key
    pub fn press_key(&mut self, key: KeyCode) {
        if !self.keys_held.contains(&key) {
            self.keys_just_pressed.insert(key);
        }
        self.keys_held.insert(key);
    }

    /// release a key
    pub fn release_key(&mut self, key: KeyCode) {
        if self.keys_held.contains(&key) {
            self.keys_just_released.insert(key);
        }
        self.keys_held.remove(&key);
    }

    /// set mouse position
    pub fn set_mouse_position(&mut self, x: f32, y: f32) {
        self.mouse_delta = (x - self.mouse_position.0, y - self.mouse_position.1);
        self.mouse_position = (x, y);
    }

    /// press a mouse button
    pub fn press_mouse_button(&mut self, button: MouseButton) {
        if !self.mouse_buttons_held.contains(&button) {
            self.mouse_buttons_just_pressed.insert(button);
        }
        self.mouse_buttons_held.insert(button);
    }

    /// release a mouse button
    pub fn release_mouse_button(&mut self, button: MouseButton) {
        if self.mouse_buttons_held.contains(&button) {
            self.mouse_buttons_just_released.insert(button);
        }
        self.mouse_buttons_held.remove(&button);
    }
}

impl Default for InputState {
    fn default() -> Self {
        Self::new()
    }
}

/// input plugin that initializes the SDL3 input subsystem
pub struct InputPlugin;

impl GamePlugin for InputPlugin {
    fn name(&self) -> &str {
        "InputPlugin"
    }

    fn build(&mut self, app: &mut App) {
        app.insert_resource(InputState::new());
        log::info!("InputPlugin: input state resource registered");
    }
}
