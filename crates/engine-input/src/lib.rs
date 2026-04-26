//! input subsystem via SDL3
//!
//! handles keyboard, mouse, gamepad input. exposes state through clean interfaces.

use bevy_ecs::prelude::*;
use engine_core::{App, EngineState, GamePlugin};

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

    /// add to the mouse delta (for accumulating motion events)
    pub fn add_mouse_delta(&mut self, dx: f32, dy: f32) {
        self.mouse_delta = (self.mouse_delta.0 + dx, self.mouse_delta.1 + dy);
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

/// process SDL3 events and update the input state
/// call this each frame from your event callback in App::run_with_events
#[cfg(not(target_arch = "wasm32"))]
pub fn process_events(event_pump: &mut sdl3::EventPump, world: &mut bevy_ecs::prelude::World) {
    use sdl3::event::Event;

    // begin frame: clear just_pressed/just_released sets
    if let Some(mut input) = world.get_resource_mut::<InputState>() {
        input.begin_frame();
    }

    for event in event_pump.poll_iter() {
        match event {
            Event::KeyDown {
                keycode: Some(key), ..
            } => {
                if let Some(code) = keycode_from_sdl(key)
                    && let Some(mut input) = world.get_resource_mut::<InputState>()
                {
                    input.press_key(code);
                }
            }
            Event::KeyUp {
                keycode: Some(key), ..
            } => {
                if let Some(code) = keycode_from_sdl(key)
                    && let Some(mut input) = world.get_resource_mut::<InputState>()
                {
                    input.release_key(code);
                }
            }
            Event::MouseButtonDown {
                mouse_btn: btn,
                x,
                y,
                ..
            } => {
                if let Some(button) = mouse_button_from_sdl(btn)
                    && let Some(mut input) = world.get_resource_mut::<InputState>()
                {
                    input.set_mouse_position(x, y);
                    input.press_mouse_button(button);
                }
            }
            Event::MouseButtonUp {
                mouse_btn: btn,
                x,
                y,
                ..
            } => {
                if let Some(button) = mouse_button_from_sdl(btn)
                    && let Some(mut input) = world.get_resource_mut::<InputState>()
                {
                    input.set_mouse_position(x, y);
                    input.release_mouse_button(button);
                }
            }
            Event::MouseMotion {
                x, y, xrel, yrel, ..
            } => {
                if let Some(mut input) = world.get_resource_mut::<InputState>() {
                    input.add_mouse_delta(xrel, yrel);
                    input.set_mouse_position(x, y);
                }
            }
            Event::Quit { .. } => {
                if let Some(mut state) = world.get_resource_mut::<EngineState>() {
                    *state = EngineState::Stopping;
                }
            }
            _ => {}
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn keycode_from_sdl(key: sdl3::keyboard::Keycode) -> Option<KeyCode> {
    use sdl3::keyboard::Keycode;
    match key {
        Keycode::A => Some(KeyCode::A),
        Keycode::B => Some(KeyCode::B),
        Keycode::C => Some(KeyCode::C),
        Keycode::D => Some(KeyCode::D),
        Keycode::E => Some(KeyCode::E),
        Keycode::F => Some(KeyCode::F),
        Keycode::G => Some(KeyCode::G),
        Keycode::H => Some(KeyCode::H),
        Keycode::I => Some(KeyCode::I),
        Keycode::J => Some(KeyCode::J),
        Keycode::K => Some(KeyCode::K),
        Keycode::L => Some(KeyCode::L),
        Keycode::M => Some(KeyCode::M),
        Keycode::N => Some(KeyCode::N),
        Keycode::O => Some(KeyCode::O),
        Keycode::P => Some(KeyCode::P),
        Keycode::Q => Some(KeyCode::Q),
        Keycode::R => Some(KeyCode::R),
        Keycode::S => Some(KeyCode::S),
        Keycode::T => Some(KeyCode::T),
        Keycode::U => Some(KeyCode::U),
        Keycode::V => Some(KeyCode::V),
        Keycode::W => Some(KeyCode::W),
        Keycode::X => Some(KeyCode::X),
        Keycode::Y => Some(KeyCode::Y),
        Keycode::Z => Some(KeyCode::Z),
        Keycode::F1 => Some(KeyCode::F1),
        Keycode::F2 => Some(KeyCode::F2),
        Keycode::F3 => Some(KeyCode::F3),
        Keycode::F4 => Some(KeyCode::F4),
        Keycode::F5 => Some(KeyCode::F5),
        Keycode::F6 => Some(KeyCode::F6),
        Keycode::F7 => Some(KeyCode::F7),
        Keycode::F8 => Some(KeyCode::F8),
        Keycode::F9 => Some(KeyCode::F9),
        Keycode::F10 => Some(KeyCode::F10),
        Keycode::F11 => Some(KeyCode::F11),
        Keycode::F12 => Some(KeyCode::F12),
        Keycode::Escape => Some(KeyCode::Escape),
        Keycode::Space => Some(KeyCode::Space),
        Keycode::Return => Some(KeyCode::Enter),
        Keycode::Tab => Some(KeyCode::Tab),
        Keycode::Backspace => Some(KeyCode::Backspace),
        Keycode::Left => Some(KeyCode::Left),
        Keycode::Right => Some(KeyCode::Right),
        Keycode::Up => Some(KeyCode::Up),
        Keycode::Down => Some(KeyCode::Down),
        Keycode::LShift => Some(KeyCode::LShift),
        Keycode::RShift => Some(KeyCode::RShift),
        Keycode::LCtrl => Some(KeyCode::LCtrl),
        Keycode::RCtrl => Some(KeyCode::RCtrl),
        Keycode::LAlt => Some(KeyCode::LAlt),
        Keycode::RAlt => Some(KeyCode::RAlt),
        _ => None,
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn mouse_button_from_sdl(btn: sdl3::mouse::MouseButton) -> Option<MouseButton> {
    use sdl3::mouse::MouseButton as SdlBtn;
    match btn {
        SdlBtn::Left => Some(MouseButton::Left),
        SdlBtn::Right => Some(MouseButton::Right),
        SdlBtn::Middle => Some(MouseButton::Middle),
        _ => None,
    }
}
