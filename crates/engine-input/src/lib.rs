//! input subsystem via SDL3
//!
//! handles keyboard, mouse, gamepad input. exposes state through clean interfaces.
//!
//! # input model
//!
//! input state is tracked per-frame with three states for each key/button:
//! - **held**: currently pressed down
//! - **just pressed**: pressed this frame (edge-triggered)
//! - **just released**: released this frame (edge-triggered)
//!
//! the [`InputState`] resource is updated each frame by [`process_events`],
//! which polls SDL3 events and applies them to the state.
//!
//! # example
//!
//! ```ignore
//! use engine_input::{InputState, KeyCode};
//!
//! fn player_movement(input: Res<InputState>, time: Res<Time>) {
//!     if input.is_key_just_pressed(KeyCode::Space) {
//!         // jump!
//!     }
//!     if input.is_key_held(KeyCode::Left) {
//!         // move left
//!     }
//! }
//! ```

use bevy_ecs::prelude::*;
#[cfg(not(target_arch = "wasm32"))]
use engine_core::EngineState;
use engine_core::{App, GamePlugin};

/// number of distinct KeyCode variants (used to size the input arrays)
const KEY_COUNT: usize = 64;
/// number of distinct MouseButton variants
const MOUSE_BUTTON_COUNT: usize = 4;

/// keyboard key codes mapped from SDL3.
///
/// each variant represents a physical key on the keyboard.
/// the discriminant values are used as indices into the input state arrays
/// for O(1) lookup.
///
/// # layout
///
/// keys are grouped: a-z (26), 0-9 (10), f1-f12 (12), special (9), modifiers (6) = 63 total.
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

/// mouse button codes.
///
/// represents the three standard mouse buttons.
/// the discriminant values are used as indices into the input state arrays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    /// left mouse button
    Left,
    /// right mouse button
    Right,
    /// middle mouse button
    Middle,
}

/// number of gamepad buttons tracked (standard gamepad layout)
const GAMEPAD_BUTTON_COUNT: usize = 16;
/// number of gamepad axes tracked (left stick x/y, right stick x/y, triggers)
const GAMEPAD_AXIS_COUNT: usize = 6;

/// gamepad state, tracked per gamepad index.
/// uses fixed-size arrays for O(1) lookup.
#[derive(Debug, Clone)]
pub struct GamepadState {
    buttons_held: [bool; GAMEPAD_BUTTON_COUNT],
    buttons_just_pressed: [bool; GAMEPAD_BUTTON_COUNT],
    buttons_just_released: [bool; GAMEPAD_BUTTON_COUNT],
    axes: [f32; GAMEPAD_AXIS_COUNT],
}

impl GamepadState {
    /// create a new empty gamepad state
    pub fn new() -> Self {
        Self {
            buttons_held: [false; GAMEPAD_BUTTON_COUNT],
            buttons_just_pressed: [false; GAMEPAD_BUTTON_COUNT],
            buttons_just_released: [false; GAMEPAD_BUTTON_COUNT],
            axes: [0.0; GAMEPAD_AXIS_COUNT],
        }
    }

    /// check if a button is currently held
    pub fn is_button_held(&self, button: GamepadButton) -> bool {
        self.buttons_held[button as usize]
    }

    /// check if a button was just pressed this frame
    pub fn is_button_just_pressed(&self, button: GamepadButton) -> bool {
        self.buttons_just_pressed[button as usize]
    }

    /// check if a button was just released this frame
    pub fn is_button_just_released(&self, button: GamepadButton) -> bool {
        self.buttons_just_released[button as usize]
    }

    /// get an axis value (-1.0 to 1.0)
    pub fn axis(&self, axis: GamepadAxis) -> f32 {
        self.axes[axis as usize]
    }

    /// press a button
    pub fn press_button(&mut self, button: GamepadButton) {
        let index = button as usize;
        if !self.buttons_held[index] {
            self.buttons_just_pressed[index] = true;
        }
        self.buttons_held[index] = true;
    }

    /// release a button
    pub fn release_button(&mut self, button: GamepadButton) {
        let index = button as usize;
        if self.buttons_held[index] {
            self.buttons_just_released[index] = true;
        }
        self.buttons_held[index] = false;
    }

    /// set an axis value
    pub fn set_axis(&mut self, axis: GamepadAxis, value: f32) {
        self.axes[axis as usize] = value.clamp(-1.0, 1.0);
    }

    /// begin frame: clear just_pressed/just_released sets
    pub fn begin_frame(&mut self) {
        self.buttons_just_pressed = [false; GAMEPAD_BUTTON_COUNT];
        self.buttons_just_released = [false; GAMEPAD_BUTTON_COUNT];
    }
}

impl Default for GamepadState {
    fn default() -> Self {
        Self::new()
    }
}

/// standard gamepad button layout.
/// maps to a typical xbox-style controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GamepadButton {
    /// face button south (A on xbox, cross on playstation)
    South,
    /// face button east (B on xbox, circle on playstation)
    East,
    /// face button west (X on xbox, square on playstation)
    West,
    /// face button north (Y on xbox, triangle on playstation)
    North,
    /// left bumper
    LeftBumper,
    /// right bumper
    RightBumper,
    /// left stick button
    LeftStick,
    /// right stick button
    RightStick,
    /// back / select / view button
    Back,
    /// start button
    Start,
    /// dpad up
    DpadUp,
    /// dpad down
    DpadDown,
    /// dpad left
    DpadLeft,
    /// dpad right
    DpadRight,
    /// home / guide button
    Home,
    /// share / capture button
    Share,
}

/// standard gamepad axis layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GamepadAxis {
    /// left stick horizontal (-1.0 left, 1.0 right)
    LeftStickX,
    /// left stick vertical (-1.0 up, 1.0 down)
    LeftStickY,
    /// right stick horizontal (-1.0 left, 1.0 right)
    RightStickX,
    /// right stick vertical (-1.0 up, 1.0 down)
    RightStickY,
    /// left trigger (0.0 to 1.0)
    LeftTrigger,
    /// right trigger (0.0 to 1.0)
    RightTrigger,
}

/// input state resource, tracks current and previous frame input.
/// uses fixed-size bool arrays indexed by discriminant value — O(1) lookup, no hashing.
#[derive(Resource, Clone)]
pub struct InputState {
    keys_held: [bool; KEY_COUNT],
    keys_just_pressed: [bool; KEY_COUNT],
    keys_just_released: [bool; KEY_COUNT],
    mouse_position: (f32, f32),
    mouse_delta: (f32, f32),
    mouse_buttons_held: [bool; MOUSE_BUTTON_COUNT],
    mouse_buttons_just_pressed: [bool; MOUSE_BUTTON_COUNT],
    mouse_buttons_just_released: [bool; MOUSE_BUTTON_COUNT],
    gamepads: Vec<GamepadState>,
}

impl InputState {
    /// create a new empty input state
    pub fn new() -> Self {
        Self {
            keys_held: [false; KEY_COUNT],
            keys_just_pressed: [false; KEY_COUNT],
            keys_just_released: [false; KEY_COUNT],
            mouse_position: (0.0, 0.0),
            mouse_delta: (0.0, 0.0),
            mouse_buttons_held: [false; MOUSE_BUTTON_COUNT],
            mouse_buttons_just_pressed: [false; MOUSE_BUTTON_COUNT],
            mouse_buttons_just_released: [false; MOUSE_BUTTON_COUNT],
            gamepads: Vec::new(),
        }
    }

    /// check if a key is currently held down
    pub fn is_key_held(&self, key: KeyCode) -> bool {
        self.keys_held[key as usize]
    }

    /// check if a key was just pressed this frame
    pub fn is_key_just_pressed(&self, key: KeyCode) -> bool {
        self.keys_just_pressed[key as usize]
    }

    /// check if a key was just released this frame
    pub fn is_key_just_released(&self, key: KeyCode) -> bool {
        self.keys_just_released[key as usize]
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
        self.mouse_buttons_held[button as usize]
    }

    /// check if a mouse button was just pressed this frame
    pub fn is_mouse_button_just_pressed(&self, button: MouseButton) -> bool {
        self.mouse_buttons_just_pressed[button as usize]
    }

    /// check if a mouse button was just released this frame
    pub fn is_mouse_button_just_released(&self, button: MouseButton) -> bool {
        self.mouse_buttons_just_released[button as usize]
    }

    /// begin frame: clear just_pressed/just_released sets
    pub fn begin_frame(&mut self) {
        self.keys_just_pressed = [false; KEY_COUNT];
        self.keys_just_released = [false; KEY_COUNT];
        self.mouse_buttons_just_pressed = [false; MOUSE_BUTTON_COUNT];
        self.mouse_buttons_just_released = [false; MOUSE_BUTTON_COUNT];
        self.mouse_delta = (0.0, 0.0);
        for gamepad in &mut self.gamepads {
            gamepad.begin_frame();
        }
    }

    /// get gamepad state by index (0-based).
    /// returns None if the gamepad is not connected.
    pub fn gamepad(&self, index: usize) -> Option<&GamepadState> {
        self.gamepads.get(index)
    }

    /// register a new gamepad, returns its index
    pub fn add_gamepad(&mut self) -> usize {
        let index = self.gamepads.len();
        self.gamepads.push(GamepadState::new());
        index
    }

    /// remove a gamepad by index
    pub fn remove_gamepad(&mut self, index: usize) {
        if index < self.gamepads.len() {
            self.gamepads.remove(index);
        }
    }

    /// press a gamepad button
    pub fn press_gamepad_button(&mut self, gamepad_index: usize, button: GamepadButton) {
        if let Some(gamepad) = self.gamepads.get_mut(gamepad_index) {
            gamepad.press_button(button);
        }
    }

    /// release a gamepad button
    pub fn release_gamepad_button(&mut self, gamepad_index: usize, button: GamepadButton) {
        if let Some(gamepad) = self.gamepads.get_mut(gamepad_index) {
            gamepad.release_button(button);
        }
    }

    /// set a gamepad axis
    pub fn set_gamepad_axis(&mut self, gamepad_index: usize, axis: GamepadAxis, value: f32) {
        if let Some(gamepad) = self.gamepads.get_mut(gamepad_index) {
            gamepad.set_axis(axis, value);
        }
    }

    /// press a key
    pub fn press_key(&mut self, key: KeyCode) {
        let index = key as usize;
        if !self.keys_held[index] {
            self.keys_just_pressed[index] = true;
        }
        self.keys_held[index] = true;
    }

    /// release a key
    pub fn release_key(&mut self, key: KeyCode) {
        let index = key as usize;
        if self.keys_held[index] {
            self.keys_just_released[index] = true;
        }
        self.keys_held[index] = false;
    }

    /// set mouse position
    pub fn set_mouse_position(&mut self, x: f32, y: f32) {
        self.mouse_delta = (x - self.mouse_position.0, y - self.mouse_position.1);
        self.mouse_position = (x, y);
    }

    /// add to the mouse delta (for accumulating motion events)
    pub fn add_mouse_delta(&mut self, delta_x: f32, delta_y: f32) {
        self.mouse_delta = (self.mouse_delta.0 + delta_x, self.mouse_delta.1 + delta_y);
    }

    /// press a mouse button
    pub fn press_mouse_button(&mut self, button: MouseButton) {
        let index = button as usize;
        if !self.mouse_buttons_held[index] {
            self.mouse_buttons_just_pressed[index] = true;
        }
        self.mouse_buttons_held[index] = true;
    }

    /// release a mouse button
    pub fn release_mouse_button(&mut self, button: MouseButton) {
        let index = button as usize;
        if self.mouse_buttons_held[index] {
            self.mouse_buttons_just_released[index] = true;
        }
        self.mouse_buttons_held[index] = false;
    }
}

impl Default for InputState {
    fn default() -> Self {
        Self::new()
    }
}

/// input plugin that initializes the SDL3 input subsystem.
///
/// add this plugin to your [`App`] to enable input handling.
/// it registers the [`InputState`] as an ECS resource.
///
/// # native setup
///
/// on native targets, call [`InputPlugin::init_sdl`] before creating the app,
/// then pass the returned event pump to [`App::run_with_events`].
///
/// # web setup
///
/// on web, call [`setup_web_input`] with the canvas element before running.
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

/// initialize SDL3 and return an event pump.
///
/// call this once at startup on native targets.
/// the returned pump should be used with [`process_events`].
#[cfg(not(target_arch = "wasm32"))]
pub fn init_sdl() -> sdl3::EventPump {
    let sdl = sdl3::init().expect("failed to initialize SDL3");
    sdl.event_pump().expect("failed to get event pump")
}

/// process SDL3 events and update the input state.
///
/// this function should be called once per frame before the ECS tick.
/// pass the event pump returned from [`init_sdl`] to poll events.
///
/// # quit handling
///
/// if a quit event is received, the [`EngineState`] is set to [`EngineState::Stopping`].
///
/// # platform
///
/// this function is only available on non-WASM targets.
/// use [`process_events`] on WASM for the web-compatible version.
#[cfg(not(target_arch = "wasm32"))]
pub fn process_events(event_pump: &mut sdl3::EventPump, world: &mut bevy_ecs::prelude::World) {
    use sdl3::event::Event;

    let events: Vec<Event> = event_pump.poll_iter().collect();
    let mut got_quit = false;

    if let Some(mut input) = world.get_resource_mut::<InputState>() {
        input.begin_frame();
        for event in &events {
            match event {
                Event::KeyDown {
                    keycode: Some(key), ..
                } => {
                    if let Some(code) = keycode_from_sdl(*key) {
                        input.press_key(code);
                    }
                }
                Event::KeyUp {
                    keycode: Some(key), ..
                } => {
                    if let Some(code) = keycode_from_sdl(*key) {
                        input.release_key(code);
                    }
                }
                Event::MouseButtonDown {
                    mouse_btn: button,
                    x,
                    y,
                    ..
                } => {
                    if let Some(mouse_button) = mouse_button_from_sdl(*button) {
                        input.set_mouse_position(*x, *y);
                        input.press_mouse_button(mouse_button);
                    }
                }
                Event::MouseButtonUp {
                    mouse_btn: button,
                    x,
                    y,
                    ..
                } => {
                    if let Some(mouse_button) = mouse_button_from_sdl(*button) {
                        input.set_mouse_position(*x, *y);
                        input.release_mouse_button(mouse_button);
                    }
                }
                Event::MouseMotion {
                    x, y, xrel, yrel, ..
                } => {
                    input.add_mouse_delta(*xrel, *yrel);
                    input.set_mouse_position(*x, *y);
                }
                // gamepad events require hidapi feature — stubbed for now
                Event::Quit { .. } => got_quit = true,
                _ => {}
            }
        }
    }

    if got_quit && let Some(mut state) = world.get_resource_mut::<EngineState>() {
        *state = EngineState::Stopping;
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
fn mouse_button_from_sdl(button: sdl3::mouse::MouseButton) -> Option<MouseButton> {
    use sdl3::mouse::MouseButton as SdlBtn;
    match button {
        SdlBtn::Left => Some(MouseButton::Left),
        SdlBtn::Right => Some(MouseButton::Right),
        SdlBtn::Middle => Some(MouseButton::Middle),
        _ => None,
    }
}

// gamepad event mapping requires hidapi feature — stubbed for now

/// web input event queue (populated by JS callbacks via wasm-bindgen)
#[cfg(target_arch = "wasm32")]
mod web_input {
    use super::{KeyCode, MouseButton};
    use std::cell::RefCell;
    use std::collections::VecDeque;

    #[derive(Clone)]
    enum WebEvent {
        KeyDown(KeyCode),
        KeyUp(KeyCode),
        MouseDown {
            button: MouseButton,
            x: f32,
            y: f32,
        },
        MouseUp {
            button: MouseButton,
            x: f32,
            y: f32,
        },
        MouseMove {
            x: f32,
            y: f32,
            delta_x: f32,
            delta_y: f32,
        },
    }

    thread_local! {
        static EVENT_QUEUE: RefCell<VecDeque<WebEvent>> = RefCell::new(VecDeque::new());
    }

    pub fn push_key_down(key: KeyCode) {
        EVENT_QUEUE.with(|q| q.borrow_mut().push_back(WebEvent::KeyDown(key)));
    }

    pub fn push_key_up(key: KeyCode) {
        EVENT_QUEUE.with(|q| q.borrow_mut().push_back(WebEvent::KeyUp(key)));
    }

    pub fn push_mouse_down(button: MouseButton, x: f32, y: f32) {
        EVENT_QUEUE.with(|q| {
            q.borrow_mut()
                .push_back(WebEvent::MouseDown { button, x, y })
        });
    }

    pub fn push_mouse_up(button: MouseButton, x: f32, y: f32) {
        EVENT_QUEUE.with(|q| q.borrow_mut().push_back(WebEvent::MouseUp { button, x, y }));
    }

    pub fn push_mouse_move(x: f32, y: f32, delta_x: f32, delta_y: f32) {
        EVENT_QUEUE.with(|q| {
            q.borrow_mut().push_back(WebEvent::MouseMove {
                x,
                y,
                delta_x,
                delta_y,
            })
        });
    }

    /// drain all queued events and apply them to the input state
    pub fn drain_to_input(input: &mut super::InputState) {
        EVENT_QUEUE.with(|q| {
            let mut queue = q.borrow_mut();
            while let Some(event) = queue.pop_front() {
                match event {
                    WebEvent::KeyDown(key) => input.press_key(key),
                    WebEvent::KeyUp(key) => input.release_key(key),
                    WebEvent::MouseDown { button, x, y } => {
                        input.set_mouse_position(x, y);
                        input.press_mouse_button(button);
                    }
                    WebEvent::MouseUp { button, x, y } => {
                        input.set_mouse_position(x, y);
                        input.release_mouse_button(button);
                    }
                    WebEvent::MouseMove {
                        x,
                        y,
                        delta_x,
                        delta_y,
                    } => {
                        input.add_mouse_delta(delta_x, delta_y);
                        input.set_mouse_position(x, y);
                    }
                }
            }
        });
    }

    /// map a web keyboard event key string to KeyCode
    pub fn key_from_web(key: &str) -> Option<KeyCode> {
        match key {
            "a" | "A" => Some(KeyCode::A),
            "b" | "B" => Some(KeyCode::B),
            "c" | "C" => Some(KeyCode::C),
            "d" | "D" => Some(KeyCode::D),
            "e" | "E" => Some(KeyCode::E),
            "f" | "F" => Some(KeyCode::F),
            "g" | "G" => Some(KeyCode::G),
            "h" | "H" => Some(KeyCode::H),
            "i" | "I" => Some(KeyCode::I),
            "j" | "J" => Some(KeyCode::J),
            "k" | "K" => Some(KeyCode::K),
            "l" | "L" => Some(KeyCode::L),
            "m" | "M" => Some(KeyCode::M),
            "n" | "N" => Some(KeyCode::N),
            "o" | "O" => Some(KeyCode::O),
            "p" | "P" => Some(KeyCode::P),
            "q" | "Q" => Some(KeyCode::Q),
            "r" | "R" => Some(KeyCode::R),
            "s" | "S" => Some(KeyCode::S),
            "t" | "T" => Some(KeyCode::T),
            "u" | "U" => Some(KeyCode::U),
            "v" | "V" => Some(KeyCode::V),
            "w" | "W" => Some(KeyCode::W),
            "x" | "X" => Some(KeyCode::X),
            "y" | "Y" => Some(KeyCode::Y),
            "z" | "Z" => Some(KeyCode::Z),
            "0" => Some(KeyCode::Num0),
            "1" => Some(KeyCode::Num1),
            "2" => Some(KeyCode::Num2),
            "3" => Some(KeyCode::Num3),
            "4" => Some(KeyCode::Num4),
            "5" => Some(KeyCode::Num5),
            "6" => Some(KeyCode::Num6),
            "7" => Some(KeyCode::Num7),
            "8" => Some(KeyCode::Num8),
            "9" => Some(KeyCode::Num9),
            "F1" => Some(KeyCode::F1),
            "F2" => Some(KeyCode::F2),
            "F3" => Some(KeyCode::F3),
            "F4" => Some(KeyCode::F4),
            "F5" => Some(KeyCode::F5),
            "F6" => Some(KeyCode::F6),
            "F7" => Some(KeyCode::F7),
            "F8" => Some(KeyCode::F8),
            "F9" => Some(KeyCode::F9),
            "F10" => Some(KeyCode::F10),
            "F11" => Some(KeyCode::F11),
            "F12" => Some(KeyCode::F12),
            "Escape" => Some(KeyCode::Escape),
            " " => Some(KeyCode::Space),
            "Enter" => Some(KeyCode::Enter),
            "Tab" => Some(KeyCode::Tab),
            "Backspace" => Some(KeyCode::Backspace),
            "ArrowLeft" => Some(KeyCode::Left),
            "ArrowRight" => Some(KeyCode::Right),
            "ArrowUp" => Some(KeyCode::Up),
            "ArrowDown" => Some(KeyCode::Down),
            "Shift" => Some(KeyCode::LShift),
            "Control" => Some(KeyCode::LCtrl),
            "Alt" => Some(KeyCode::LAlt),
            _ => None,
        }
    }

    /// map a web mouse button index to MouseButton
    /// button 0 = left, 1 = middle, 2 = right (browser API ordering)
    pub fn mouse_button_from_web(button: i16) -> Option<MouseButton> {
        match button {
            0 => Some(MouseButton::Left),
            1 => Some(MouseButton::Middle),
            2 => Some(MouseButton::Right),
            _ => None,
        }
    }
}

/// process web events and update the input state (WASM target)
#[cfg(target_arch = "wasm32")]
pub fn process_events(_event_pump: &mut (), world: &mut bevy_ecs::prelude::World) {
    if let Some(mut input) = world.get_resource_mut::<InputState>() {
        input.begin_frame();
        web_input::drain_to_input(&mut input);
    }
}

/// set up web input event listeners on the given canvas element.
/// call this once during initialization on WASM target.
#[cfg(target_arch = "wasm32")]
pub fn setup_web_input(canvas: &web_sys::HtmlElement) {
    use wasm_bindgen::JsCast;
    use web_input::{key_from_web, mouse_button_from_web};
    use web_sys::EventTarget;

    let canvas_target: &EventTarget = canvas.as_ref();

    // keyboard events on document body (not canvas — canvas doesn't receive keyboard events)
    {
        let window = web_sys::window().expect("no window");
        let document = window.document().expect("no document");
        let body = doc_body(&document);
        let target: &EventTarget = body.as_ref();

        let keydown_closure =
            wasm_bindgen::closure::Closure::wrap(Box::new(move |event: web_sys::KeyboardEvent| {
                event.prevent_default();
                if let Some(code) = key_from_web(&event.key()) {
                    web_input::push_key_down(code);
                }
            }) as Box<dyn FnMut(_)>);
        target
            .add_event_listener_with_callback("keydown", keydown_closure.as_ref().unchecked_ref())
            .expect("failed to add keydown listener");
        keydown_closure.forget();

        let keyup_closure =
            wasm_bindgen::closure::Closure::wrap(Box::new(move |event: web_sys::KeyboardEvent| {
                event.prevent_default();
                if let Some(code) = key_from_web(&event.key()) {
                    web_input::push_key_up(code);
                }
            }) as Box<dyn FnMut(_)>);
        target
            .add_event_listener_with_callback("keyup", keyup_closure.as_ref().unchecked_ref())
            .expect("failed to add keyup listener");
        keyup_closure.forget();
    }

    // mouse events on canvas
    {
        let mousedown_closure =
            wasm_bindgen::closure::Closure::wrap(Box::new(move |event: web_sys::MouseEvent| {
                if let Some(button) = mouse_button_from_web(event.button()) {
                    web_input::push_mouse_down(
                        button,
                        event.offset_x() as f32,
                        event.offset_y() as f32,
                    );
                }
            }) as Box<dyn FnMut(_)>);
        canvas_target
            .add_event_listener_with_callback(
                "mousedown",
                mousedown_closure.as_ref().unchecked_ref(),
            )
            .expect("failed to add mousedown listener");
        mousedown_closure.forget();

        let mouseup_closure =
            wasm_bindgen::closure::Closure::wrap(Box::new(move |event: web_sys::MouseEvent| {
                if let Some(button) = mouse_button_from_web(event.button()) {
                    web_input::push_mouse_up(
                        button,
                        event.offset_x() as f32,
                        event.offset_y() as f32,
                    );
                }
            }) as Box<dyn FnMut(_)>);
        canvas_target
            .add_event_listener_with_callback("mouseup", mouseup_closure.as_ref().unchecked_ref())
            .expect("failed to add mouseup listener");
        mouseup_closure.forget();

        let mousemove_closure =
            wasm_bindgen::closure::Closure::wrap(Box::new(move |event: web_sys::MouseEvent| {
                web_input::push_mouse_move(
                    event.offset_x() as f32,
                    event.offset_y() as f32,
                    event.movement_x() as f32,
                    event.movement_y() as f32,
                );
            }) as Box<dyn FnMut(_)>);
        canvas_target
            .add_event_listener_with_callback(
                "mousemove",
                mousemove_closure.as_ref().unchecked_ref(),
            )
            .expect("failed to add mousemove listener");
        mousemove_closure.forget();
    }
}

#[cfg(target_arch = "wasm32")]
fn doc_body(document: &web_sys::Document) -> web_sys::HtmlElement {
    document.body().expect("no body element")
}
