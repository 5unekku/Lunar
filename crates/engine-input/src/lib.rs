//! input subsystem via SDL3
//!
//! handles keyboard, mouse, gamepad input. exposes state through clean interfaces.

use bevy_ecs::prelude::*;
#[cfg(not(target_arch = "wasm32"))]
use engine_core::EngineState;
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
        MouseDown { button: MouseButton, x: f32, y: f32 },
        MouseUp { button: MouseButton, x: f32, y: f32 },
        MouseMove { x: f32, y: f32, dx: f32, dy: f32 },
    }

    thread_local! {
        static EVENT_QUEUE: RefCell<VecDeque<WebEvent>> = RefCell::new(VecDeque::new());
    }

    /// push a key down event into the queue (called from JS)
    pub fn push_key_down(key: KeyCode) {
        EVENT_QUEUE.with(|q| q.borrow_mut().push_back(WebEvent::KeyDown(key)));
    }

    /// push a key up event into the queue (called from JS)
    pub fn push_key_up(key: KeyCode) {
        EVENT_QUEUE.with(|q| q.borrow_mut().push_back(WebEvent::KeyUp(key)));
    }

    /// push a mouse down event (called from JS)
    pub fn push_mouse_down(button: MouseButton, x: f32, y: f32) {
        EVENT_QUEUE.with(|q| {
            q.borrow_mut()
                .push_back(WebEvent::MouseDown { button, x, y });
        });
    }

    /// push a mouse up event (called from JS)
    pub fn push_mouse_up(button: MouseButton, x: f32, y: f32) {
        EVENT_QUEUE.with(|q| {
            q.borrow_mut().push_back(WebEvent::MouseUp { button, x, y });
        });
    }

    /// push a mouse move event (called from JS)
    pub fn push_mouse_move(x: f32, y: f32, dx: f32, dy: f32) {
        EVENT_QUEUE.with(|q| {
            q.borrow_mut()
                .push_back(WebEvent::MouseMove { x, y, dx, dy });
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
                    WebEvent::MouseMove { x, y, dx, dy } => {
                        input.add_mouse_delta(dx, dy);
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

    /// map a web mouse button to MouseButton
    pub fn mouse_button_from_web(button: i16) -> Option<MouseButton> {
        match button {
            0 => Some(MouseButton::Left),
            2 => Some(MouseButton::Right),
            1 => Some(MouseButton::Middle),
            _ => None,
        }
    }
}

/// process web events and update the input state (WASM target)
/// call this each frame from your event callback in App::run_with_events
#[cfg(target_arch = "wasm32")]
pub fn process_events(_event_pump: &mut (), world: &mut bevy_ecs::prelude::World) {
    // begin frame: clear just_pressed/just_released sets
    if let Some(mut input) = world.get_resource_mut::<InputState>() {
        input.begin_frame();
        web_input::drain_to_input(&mut input);
    }
}

/// set up web input event listeners on the given canvas element
/// call this once during initialization on WASM target
#[cfg(target_arch = "wasm32")]
pub fn setup_web_input(canvas: &web_sys::HtmlElement) {
    use wasm_bindgen::JsCast;
    use web_input::{key_from_web, mouse_button_from_web};
    use web_sys::EventTarget;

    let canvas_target: &EventTarget = canvas.as_ref();

    // keyboard events
    {
        let window = web_sys::window().expect("no window");
        let doc = window.document().expect("no document");
        let body = doc.body().expect("no body");
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
