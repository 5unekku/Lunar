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
//! use lunar_input::{InputState, KeyCode};
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
use lunar_core::EngineState;
use lunar_core::{App, GamePlugin};
use rustc_hash::FxHashMap as HashMap;

/// size of the fast-path key array (covers common keys 0-127)
const KEY_ARRAY_SIZE: usize = 128;
/// highest gamepad index [`InputState::ensure_gamepad`] will register (exclusive)
pub const MAX_GAMEPADS: usize = 16;
/// number of distinct `MouseButton` variants
const MOUSE_BUTTON_COUNT: usize = 4;

/// a single input binding that can be a key, mouse button, gamepad button, or gamepad axis.
///
/// used by [`ActionMap`] to map named actions to physical inputs.
#[derive(Debug, Clone, PartialEq)]
pub enum InputBinding {
	/// a keyboard key
	Key(KeyCode),
	/// a mouse button
	Mouse(MouseButton),
	/// a gamepad button (gamepad index, button)
	GamepadButton(usize, GamepadButton),
	/// a gamepad axis with a signed threshold (gamepad index, axis, threshold).
	/// the threshold's sign picks the direction: `0.5` is active once the axis is at or
	/// past `0.5`, `-0.5` once it is at or past `-0.5` the other way. bind both signs
	/// for "either direction".
	GamepadAxis(usize, GamepadAxis, f32),
}

/// maps named action names to one or more [`InputBinding`]s.
///
/// this lets game code check actions like "jump" or "fire" instead of
/// hardcoding specific keys. multiple bindings can map to the same action
/// (e.g. both spacebar and a gamepad button can trigger "jump").
///
/// # example
///
/// ```ignore
/// fn setup(mut action_map: ResMut<ActionMap>) {
///     action_map.bind("jump", InputBinding::Key(KeyCode::Space));
///     action_map.bind("jump", InputBinding::GamepadButton(0, GamepadButton::South));
///     action_map.bind("fire", InputBinding::Mouse(MouseButton::Left));
/// }
///
/// fn player_logic(input: Res<InputState>, actions: Res<ActionMap>) {
///     if actions.is_action_just_pressed(&input, "jump") {
///         // jump!
///     }
/// }
/// ```
#[derive(Resource)]
pub struct ActionMap {
	bindings: rustc_hash::FxHashMap<String, Vec<InputBinding>>,
}

impl ActionMap {
	/// create a new empty action map
	#[must_use]
	pub fn new() -> Self {
		Self {
			bindings: rustc_hash::FxHashMap::default(),
		}
	}

	/// bind an input to an action name.
	///
	/// multiple bindings can be added to the same action, any one of them
	/// triggering will make the action active.
	pub fn bind(&mut self, action: &str, binding: InputBinding) {
		self.bindings
			.entry(action.to_string())
			.or_default()
			.push(binding);
	}

	/// unbind all bindings for an action name.
	pub fn unbind(&mut self, action: &str) {
		self.bindings.remove(action);
	}

	/// check if an action is currently held (any of its bindings are active).
	#[must_use]
	pub fn is_action_held(&self, input: &InputState, action: &str) -> bool {
		let Some(bindings) = self.bindings.get(action) else {
			return false;
		};
		bindings.iter().any(|b| b.is_held(input))
	}

	/// check if an action was just pressed this frame.
	#[must_use]
	pub fn is_action_just_pressed(&self, input: &InputState, action: &str) -> bool {
		let Some(bindings) = self.bindings.get(action) else {
			return false;
		};
		bindings.iter().any(|b| b.is_just_pressed(input))
	}

	/// check if an action was just released this frame.
	#[must_use]
	pub fn is_action_just_released(&self, input: &InputState, action: &str) -> bool {
		let Some(bindings) = self.bindings.get(action) else {
			return false;
		};
		bindings.iter().any(|b| b.is_just_released(input))
	}

	/// check if an action has any bindings registered.
	#[must_use]
	pub fn has_action(&self, action: &str) -> bool {
		self.bindings.contains_key(action)
	}

	/// list all registered action names.
	pub fn actions(&self) -> impl Iterator<Item = &str> {
		self.bindings.keys().map(std::string::String::as_str)
	}

	/// begin a fluent binding definition for an action.
	///
	/// returns an [`ActionBuilder`] that lets you chain bindings without repeating
	/// the action name. bindings are committed when the builder is dropped.
	///
	/// gamepad methods default to gamepad index 0 (player one). use the `_for`
	/// variants to target a specific gamepad index in multiplayer games.
	///
	/// # example
	///
	/// ```ignore
	/// actions.action("jump")
	///     .key(KeyCode::Space)
	///     .button(GamepadButton::South);
	/// ```
	pub fn action(&mut self, name: &str) -> ActionBuilder<'_> {
		ActionBuilder {
			action_map: self,
			name: name.to_string(),
			bindings: Vec::new(),
		}
	}
}

impl Default for ActionMap {
	fn default() -> Self {
		Self::new()
	}
}

/// fluent builder returned by [`ActionMap::action`].
///
/// chain binding methods then let the builder drop; bindings are committed on drop.
pub struct ActionBuilder<'a> {
	action_map: &'a mut ActionMap,
	name: String,
	bindings: Vec<InputBinding>,
}

impl<'a> ActionBuilder<'a> {
	/// bind a keyboard key
	pub fn key(mut self, key: KeyCode) -> Self {
		self.bindings.push(InputBinding::Key(key));
		self
	}

	/// bind a mouse button
	pub fn mouse(mut self, button: MouseButton) -> Self {
		self.bindings.push(InputBinding::Mouse(button));
		self
	}

	/// bind a gamepad button on gamepad 0
	pub fn button(mut self, button: GamepadButton) -> Self {
		self.bindings.push(InputBinding::GamepadButton(0, button));
		self
	}

	/// bind a gamepad button on a specific gamepad
	pub fn button_for(mut self, gamepad: usize, button: GamepadButton) -> Self {
		self.bindings
			.push(InputBinding::GamepadButton(gamepad, button));
		self
	}

	/// bind a gamepad axis on gamepad 0.
	/// positive threshold triggers when the axis exceeds the value in that direction
	/// (e.g. `0.5` = pushed right/down, `-0.5` = pushed left/up).
	pub fn axis(mut self, axis: GamepadAxis, threshold: f32) -> Self {
		self.bindings
			.push(InputBinding::GamepadAxis(0, axis, threshold));
		self
	}

	/// bind a gamepad axis on a specific gamepad
	pub fn axis_for(mut self, gamepad: usize, axis: GamepadAxis, threshold: f32) -> Self {
		self.bindings
			.push(InputBinding::GamepadAxis(gamepad, axis, threshold));
		self
	}
}

impl Drop for ActionBuilder<'_> {
	fn drop(&mut self) {
		for binding in self.bindings.drain(..) {
			self.action_map.bind(&self.name, binding);
		}
	}
}

impl InputBinding {
	/// signed: a negative threshold watches the negative direction. this compared
	/// `value.abs() >= threshold.abs()`, so `-0.5` and `0.5` bindings on one stick
	/// both fired whichever way it was pushed (move_left and move_right at once)
	fn axis_active(value: f32, threshold: f32) -> bool {
		if threshold >= 0.0 {
			value >= threshold
		} else {
			value <= threshold
		}
	}

	fn is_held(&self, input: &InputState) -> bool {
		match self {
			Self::Key(key) => input.is_key_held(*key),
			Self::Mouse(button) => input.is_mouse_button_held(*button),
			Self::GamepadButton(index, button) => input
				.gamepad(*index)
				.is_some_and(|gp| gp.is_button_held(*button)),
			Self::GamepadAxis(index, axis, threshold) => input
				.gamepad(*index)
				.is_some_and(|gp| Self::axis_active(gp.axis(*axis), *threshold)),
		}
	}

	fn is_just_pressed(&self, input: &InputState) -> bool {
		match self {
			Self::Key(key) => input.is_key_just_pressed(*key),
			Self::Mouse(button) => input.is_mouse_button_just_pressed(*button),
			Self::GamepadButton(index, button) => input
				.gamepad(*index)
				.is_some_and(|gp| gp.is_button_just_pressed(*button)),
			// edge: crossed the threshold since the start of the frame (corr-38)
			Self::GamepadAxis(index, axis, threshold) => input.gamepad(*index).is_some_and(|gp| {
				Self::axis_active(gp.axis(*axis), *threshold)
					&& !Self::axis_active(gp.previous_axis(*axis), *threshold)
			}),
		}
	}

	fn is_just_released(&self, input: &InputState) -> bool {
		match self {
			Self::Key(key) => input.is_key_just_released(*key),
			Self::Mouse(button) => input.is_mouse_button_just_released(*button),
			Self::GamepadButton(index, button) => input
				.gamepad(*index)
				.is_some_and(|gp| gp.is_button_just_released(*button)),
			Self::GamepadAxis(index, axis, threshold) => input.gamepad(*index).is_some_and(|gp| {
				!Self::axis_active(gp.axis(*axis), *threshold)
					&& Self::axis_active(gp.previous_axis(*axis), *threshold)
			}),
		}
	}
}

/// keyboard key codes mapped from SDL3.
///
/// each variant represents a physical key on the keyboard.
/// the discriminant values are used as indices into the input state arrays
/// for O(1) lookup.
///
/// # layout
///
/// keys are grouped: a-z (26), 0-9 (10), f1-f12 (12), special (9), modifiers (6), punctuation (2) = 65 total.
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
	/// punctuation and symbols (0..KEY_ARRAY_SIZE range)
	Minus,
	Equals,
	Semicolon,
	Apostrophe,
	Comma,
	Period,
	Slash,
	Backslash,
	LeftBracket,
	RightBracket,
	Grave,
	/// navigation cluster
	Home,
	End,
	PageUp,
	PageDown,
	Insert,
	Delete,
	/// numpad
	Numpad0,
	Numpad1,
	Numpad2,
	Numpad3,
	Numpad4,
	Numpad5,
	Numpad6,
	Numpad7,
	Numpad8,
	Numpad9,
	NumpadAdd,
	NumpadSub,
	NumpadMul,
	NumpadDiv,
	NumpadEnter,
	NumpadDecimal,
	NumLock,
	/// lock and control keys
	CapsLock,
	ScrollLock,
	Pause,
	PrintScreen,
	/// super / meta keys
	LSuper,
	RSuper,
	/// media keys
	MediaPlay,
	MediaStop,
	MediaNext,
	MediaPrev,
	VolumeUp,
	VolumeDown,
	Mute,
	/// extended function keys (discriminants >= 128, use HashMap fallback)
	F13 = 128,
	F14,
	F15,
	F16,
	F17,
	F18,
	F19,
	F20,
	F21,
	F22,
	F23,
	F24,
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
	/// axis values at the start of the frame, for axis-binding edges
	prev_axes: [f32; GAMEPAD_AXIS_COUNT],
}

impl GamepadState {
	/// create a new empty gamepad state
	#[must_use]
	pub const fn new() -> Self {
		Self {
			buttons_held: [false; GAMEPAD_BUTTON_COUNT],
			buttons_just_pressed: [false; GAMEPAD_BUTTON_COUNT],
			buttons_just_released: [false; GAMEPAD_BUTTON_COUNT],
			axes: [0.0; GAMEPAD_AXIS_COUNT],
			prev_axes: [0.0; GAMEPAD_AXIS_COUNT],
		}
	}

	/// check if a button is currently held
	#[must_use]
	pub const fn is_button_held(&self, button: GamepadButton) -> bool {
		self.buttons_held[button as usize]
	}

	/// check if a button was just pressed this frame
	#[must_use]
	pub const fn is_button_just_pressed(&self, button: GamepadButton) -> bool {
		self.buttons_just_pressed[button as usize]
	}

	/// check if a button was just released this frame
	#[must_use]
	pub const fn is_button_just_released(&self, button: GamepadButton) -> bool {
		self.buttons_just_released[button as usize]
	}

	/// get an axis value (-1.0 to 1.0)
	#[must_use]
	pub const fn axis(&self, axis: GamepadAxis) -> f32 {
		self.axes[axis as usize]
	}

	/// the axis value at the start of this frame (before this frame's events)
	#[must_use]
	pub const fn previous_axis(&self, axis: GamepadAxis) -> f32 {
		self.prev_axes[axis as usize]
	}

	/// press a button
	pub const fn press_button(&mut self, button: GamepadButton) {
		let index = button as usize;
		if !self.buttons_held[index] {
			self.buttons_just_pressed[index] = true;
		}
		self.buttons_held[index] = true;
	}

	/// release a button
	pub const fn release_button(&mut self, button: GamepadButton) {
		let index = button as usize;
		if self.buttons_held[index] {
			self.buttons_just_released[index] = true;
		}
		self.buttons_held[index] = false;
	}

	/// set an axis value
	pub const fn set_axis(&mut self, axis: GamepadAxis, value: f32) {
		self.axes[axis as usize] = value.clamp(-1.0, 1.0);
	}

	/// begin frame: clear `just_pressed/just_released` sets
	pub const fn begin_frame(&mut self) {
		self.buttons_just_pressed = [false; GAMEPAD_BUTTON_COUNT];
		self.buttons_just_released = [false; GAMEPAD_BUTTON_COUNT];
		self.prev_axes = self.axes;
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
	/// left shoulder button (L1 / LB)
	LeftShoulder,
	/// right shoulder button (R1 / RB)
	RightShoulder,
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
/// uses fixed-size bool arrays indexed by discriminant value: O(1) lookup, no hashing.
#[derive(Resource, Clone)]
pub struct InputState {
	/// fast-path array for common keys (indices `0..KEY_ARRAY_SIZE`)
	keys_held: [bool; KEY_ARRAY_SIZE],
	keys_just_pressed: [bool; KEY_ARRAY_SIZE],
	keys_just_released: [bool; KEY_ARRAY_SIZE],
	/// fallback for rare/international keys outside the fast-path range
	keys_held_extra: HashMap<KeyCode, bool>,
	keys_just_pressed_extra: HashMap<KeyCode, bool>,
	keys_just_released_extra: HashMap<KeyCode, bool>,
	mouse_position: (f32, f32),
	mouse_delta: (f32, f32),
	mouse_buttons_held: [bool; MOUSE_BUTTON_COUNT],
	mouse_buttons_just_pressed: [bool; MOUSE_BUTTON_COUNT],
	mouse_buttons_just_released: [bool; MOUSE_BUTTON_COUNT],
	// pending edge accumulator (keyboard + mouse). the event pump writes here once
	// per display frame; `promote_pending` moves it into the visible sets above at
	// the start of each logic tick. this decouples edge input from the render rate:
	// every press is consumed by exactly one tick (never doubled when several ticks
	// share a frame, never dropped when a frame runs zero ticks).
	pending_keys_just_pressed: [bool; KEY_ARRAY_SIZE],
	pending_keys_just_released: [bool; KEY_ARRAY_SIZE],
	pending_keys_just_pressed_extra: HashMap<KeyCode, bool>,
	pending_keys_just_released_extra: HashMap<KeyCode, bool>,
	pending_mouse_delta: (f32, f32),
	pending_mouse_buttons_just_pressed: [bool; MOUSE_BUTTON_COUNT],
	pending_mouse_buttons_just_released: [bool; MOUSE_BUTTON_COUNT],
	/// one slot per gamepad index; a disconnect empties its slot instead of shifting
	/// later pads down, so indices cached by the platform layer stay valid
	gamepads: Vec<Option<GamepadState>>,
}

impl InputState {
	/// create a new empty input state
	#[must_use]
	pub fn new() -> Self {
		Self {
			keys_held: [false; KEY_ARRAY_SIZE],
			keys_just_pressed: [false; KEY_ARRAY_SIZE],
			keys_just_released: [false; KEY_ARRAY_SIZE],
			keys_held_extra: HashMap::default(),
			keys_just_pressed_extra: HashMap::default(),
			keys_just_released_extra: HashMap::default(),
			mouse_position: (0.0, 0.0),
			mouse_delta: (0.0, 0.0),
			mouse_buttons_held: [false; MOUSE_BUTTON_COUNT],
			mouse_buttons_just_pressed: [false; MOUSE_BUTTON_COUNT],
			mouse_buttons_just_released: [false; MOUSE_BUTTON_COUNT],
			pending_keys_just_pressed: [false; KEY_ARRAY_SIZE],
			pending_keys_just_released: [false; KEY_ARRAY_SIZE],
			pending_keys_just_pressed_extra: HashMap::default(),
			pending_keys_just_released_extra: HashMap::default(),
			pending_mouse_delta: (0.0, 0.0),
			pending_mouse_buttons_just_pressed: [false; MOUSE_BUTTON_COUNT],
			pending_mouse_buttons_just_released: [false; MOUSE_BUTTON_COUNT],
			gamepads: Vec::new(),
		}
	}

	/// move one display frame's accumulated key/mouse edges into the visible sets
	/// and reset the accumulator. called once at the start of every logic tick (the
	/// Input stage) so edge input is consumed per tick, not per render frame.
	pub fn promote_pending(&mut self) {
		self.keys_just_pressed =
			std::mem::replace(&mut self.pending_keys_just_pressed, [false; KEY_ARRAY_SIZE]);
		self.keys_just_released =
			std::mem::replace(&mut self.pending_keys_just_released, [false; KEY_ARRAY_SIZE]);
		self.keys_just_pressed_extra = std::mem::take(&mut self.pending_keys_just_pressed_extra);
		self.keys_just_released_extra = std::mem::take(&mut self.pending_keys_just_released_extra);
		self.mouse_buttons_just_pressed = std::mem::replace(
			&mut self.pending_mouse_buttons_just_pressed,
			[false; MOUSE_BUTTON_COUNT],
		);
		self.mouse_buttons_just_released = std::mem::replace(
			&mut self.pending_mouse_buttons_just_released,
			[false; MOUSE_BUTTON_COUNT],
		);
		self.mouse_delta = std::mem::replace(&mut self.pending_mouse_delta, (0.0, 0.0));
	}

	/// check if a key is currently held down
	#[must_use]
	pub fn is_key_held(&self, key: KeyCode) -> bool {
		let idx = key as usize;
		if idx < KEY_ARRAY_SIZE {
			self.keys_held[idx]
		} else {
			self.keys_held_extra.get(&key).copied().unwrap_or(false)
		}
	}

	/// check if a key was just pressed this frame
	#[must_use]
	pub fn is_key_just_pressed(&self, key: KeyCode) -> bool {
		let idx = key as usize;
		if idx < KEY_ARRAY_SIZE {
			self.keys_just_pressed[idx]
		} else {
			self.keys_just_pressed_extra
				.get(&key)
				.copied()
				.unwrap_or(false)
		}
	}

	/// check if a key was just released this frame
	#[must_use]
	pub fn is_key_just_released(&self, key: KeyCode) -> bool {
		let idx = key as usize;
		if idx < KEY_ARRAY_SIZE {
			self.keys_just_released[idx]
		} else {
			self.keys_just_released_extra
				.get(&key)
				.copied()
				.unwrap_or(false)
		}
	}

	/// get the current mouse position
	#[must_use]
	pub const fn mouse_position(&self) -> (f32, f32) {
		self.mouse_position
	}

	/// get the mouse movement delta this frame
	#[must_use]
	pub const fn mouse_delta(&self) -> (f32, f32) {
		self.mouse_delta
	}

	/// check if a mouse button is currently held down
	#[must_use]
	pub const fn is_mouse_button_held(&self, button: MouseButton) -> bool {
		self.mouse_buttons_held[button as usize]
	}

	/// check if a mouse button was just pressed this frame
	#[must_use]
	pub const fn is_mouse_button_just_pressed(&self, button: MouseButton) -> bool {
		self.mouse_buttons_just_pressed[button as usize]
	}

	/// check if a mouse button was just released this frame
	#[must_use]
	pub const fn is_mouse_button_just_released(&self, button: MouseButton) -> bool {
		self.mouse_buttons_just_released[button as usize]
	}

	/// begin frame: clear the per-frame gamepad edge sets. keyboard and mouse edges
	/// are no longer cleared here; they ride the pending accumulator and are cycled
	/// per logic tick by `promote_pending`, so they stay decoupled from the render
	/// rate. (gamepad edges are still per-frame; a ponytail upgrade path if a pad
	/// game needs tick-exact buttons.)
	pub fn begin_frame(&mut self) {
		for gamepad in self.gamepads.iter_mut().flatten() {
			gamepad.begin_frame();
		}
	}

	/// get gamepad state by index (0-based).
	/// returns None if the gamepad is not connected.
	#[must_use]
	pub fn gamepad(&self, index: usize) -> Option<&GamepadState> {
		self.gamepads.get(index).and_then(Option::as_ref)
	}

	fn gamepad_mut(&mut self, index: usize) -> Option<&mut GamepadState> {
		self.gamepads.get_mut(index).and_then(Option::as_mut)
	}

	/// whether gamepad `index` is connected.
	#[must_use]
	pub fn is_gamepad_connected(&self, index: usize) -> bool {
		self.gamepad(index).is_some()
	}

	/// whether `button` is held on gamepad `index` (false if it is not connected).
	#[must_use]
	pub fn is_gamepad_button_held(&self, index: usize, button: GamepadButton) -> bool {
		self.gamepad(index).is_some_and(|pad| pad.is_button_held(button))
	}

	/// whether `button` went down on gamepad `index` this tick.
	#[must_use]
	pub fn is_gamepad_button_just_pressed(&self, index: usize, button: GamepadButton) -> bool {
		self.gamepad(index).is_some_and(|pad| pad.is_button_just_pressed(button))
	}

	/// whether `button` came up on gamepad `index` this tick.
	#[must_use]
	pub fn is_gamepad_button_just_released(&self, index: usize, button: GamepadButton) -> bool {
		self.gamepad(index).is_some_and(|pad| pad.is_button_just_released(button))
	}

	/// `axis` on gamepad `index` in -1.0..=1.0 (0.0 if it is not connected).
	#[must_use]
	pub fn gamepad_axis(&self, index: usize, axis: GamepadAxis) -> f32 {
		self.gamepad(index).map_or(0.0, |pad| pad.axis(axis))
	}

	/// register a new gamepad, returns its index
	pub fn add_gamepad(&mut self) -> usize {
		// reuse the first free slot (a reconnecting pad usually gets its old index)
		if let Some(index) = self.gamepads.iter().position(Option::is_none) {
			self.gamepads[index] = Some(GamepadState::new());
			return index;
		}
		self.gamepads.push(Some(GamepadState::new()));
		self.gamepads.len() - 1
	}

	/// make sure a gamepad is registered at `index` (lower empty slots stay disconnected).
	/// platforms that report pads by a fixed index (the browser gamepad api) call this
	/// before applying events; indices past [`MAX_GAMEPADS`] are ignored.
	pub fn ensure_gamepad(&mut self, index: usize) {
		if index < MAX_GAMEPADS {
			if self.gamepads.len() <= index {
				self.gamepads.resize_with(index + 1, || None);
			}
			self.gamepads[index].get_or_insert_with(GamepadState::new);
		}
	}

	/// remove a gamepad by index
	pub fn remove_gamepad(&mut self, index: usize) {
		// empty the slot rather than Vec::remove: shifting later pads down desynced the
		// indices the sdl provider cached for every pad above the removed one
		if let Some(slot) = self.gamepads.get_mut(index) {
			*slot = None;
		}
	}

	/// press a gamepad button
	pub fn press_gamepad_button(&mut self, gamepad_index: usize, button: GamepadButton) {
		if let Some(gamepad) = self.gamepad_mut(gamepad_index) {
			gamepad.press_button(button);
		}
	}

	/// release a gamepad button
	pub fn release_gamepad_button(&mut self, gamepad_index: usize, button: GamepadButton) {
		if let Some(gamepad) = self.gamepad_mut(gamepad_index) {
			gamepad.release_button(button);
		}
	}

	/// set a gamepad axis
	pub fn set_gamepad_axis(&mut self, gamepad_index: usize, axis: GamepadAxis, value: f32) {
		if let Some(gamepad) = self.gamepad_mut(gamepad_index) {
			gamepad.set_axis(axis, value);
		}
	}

	/// press a key. the edge lands in the pending accumulator and becomes visible
	/// on the next logic tick's `promote_pending`; `held` updates immediately.
	pub fn press_key(&mut self, key: KeyCode) {
		let index = key as usize;
		if index < KEY_ARRAY_SIZE {
			if !self.keys_held[index] {
				self.pending_keys_just_pressed[index] = true;
			}
			self.keys_held[index] = true;
		} else {
			let was_held = self.keys_held_extra.get(&key).copied().unwrap_or(false);
			if !was_held {
				self.pending_keys_just_pressed_extra.insert(key, true);
			}
			self.keys_held_extra.insert(key, true);
		}
	}

	/// release a key. edge pends for the next tick; `held` updates immediately.
	pub fn release_key(&mut self, key: KeyCode) {
		let index = key as usize;
		if index < KEY_ARRAY_SIZE {
			if self.keys_held[index] {
				self.pending_keys_just_released[index] = true;
			}
			self.keys_held[index] = false;
		} else {
			let was_held = self.keys_held_extra.get(&key).copied().unwrap_or(false);
			if was_held {
				self.pending_keys_just_released_extra.insert(key, true);
			}
			self.keys_held_extra.insert(key, false);
		}
	}

	/// update stored mouse position without affecting delta.
	/// delta comes exclusively from `add_mouse_delta` so that relative mouse
	/// mode (xrel/yrel from SDL) is never clobbered by absolute position diffs.
	pub fn set_mouse_position(&mut self, x: f32, y: f32) {
		self.mouse_position = (x, y);
	}

	/// add to the mouse delta (for accumulating motion events). accumulates into
	/// the pending buffer so a tick sees the whole frame's motion exactly once.
	pub fn add_mouse_delta(&mut self, delta_x: f32, delta_y: f32) {
		self.pending_mouse_delta = (
			self.pending_mouse_delta.0 + delta_x,
			self.pending_mouse_delta.1 + delta_y,
		);
	}

	/// press a mouse button. edge pends for the next tick; `held` is immediate.
	pub const fn press_mouse_button(&mut self, button: MouseButton) {
		let index = button as usize;
		if !self.mouse_buttons_held[index] {
			self.pending_mouse_buttons_just_pressed[index] = true;
		}
		self.mouse_buttons_held[index] = true;
	}

	/// release a mouse button. edge pends for the next tick; `held` is immediate.
	pub const fn release_mouse_button(&mut self, button: MouseButton) {
		let index = button as usize;
		if self.mouse_buttons_held[index] {
			self.pending_mouse_buttons_just_released[index] = true;
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
/// on native targets, call `InputPlugin::init_sdl` before creating the app,
/// then pass the returned event pump to [`App::run_with_events`].
///
/// # web setup
///
/// on web, call the platform input setup function before running.
pub struct InputPlugin;

#[cfg(target_arch = "wasm32")]
fn drain_web_input_system(mut input: ResMut<InputState>) {
	web_input::drain_to_input(&mut input);
	poll_gamepads(&mut input);
}

/// cycle the pending key/mouse edges into the visible sets, once per logic tick.
/// runs in the Input stage so every later stage this tick sees the freshly
/// promoted edges, and the next tick starts from an empty accumulator.
fn promote_input_edges_system(mut input: ResMut<InputState>) {
	input.promote_pending();
}

impl GamePlugin for InputPlugin {
	fn name(&self) -> &'static str {
		"InputPlugin"
	}

	fn build(&mut self, app: &mut App) {
		app.insert_resource(InputState::new());
		app.insert_resource(ActionMap::new());
		// promote must land after the web drain so a wasm frame's events are visible
		// the same tick; on native the event pump fills the accumulator before the
		// tick loop, so promote just needs to be in the Input stage.
		#[cfg(target_arch = "wasm32")]
		{
			app.add_system_to_stage(lunar_core::UpdateStage::Input, drain_web_input_system);
			app.add_system_to_stage(
				lunar_core::UpdateStage::Input,
				promote_input_edges_system.after(drain_web_input_system),
			);
		}
		#[cfg(not(target_arch = "wasm32"))]
		app.add_system_to_stage(lunar_core::UpdateStage::Input, promote_input_edges_system);
		log::info!("InputPlugin: input state and action map resources registered");
	}
}

/// abstraction over a source of gamepad events.
///
/// implement this to provide gamepad input from any backend (gilrs, custom HID, etc.).
/// the SDL3 implementation is [`SdlGamepadProvider`].
///
/// # swapping backends
///
/// to use a different gamepad library:
/// 1. implement this trait for your provider type
/// 2. call `provider.poll(input)` in your game loop, after [`process_events`]
/// 3. pass `&mut NoGamepad` to [`process_events`] so it ignores SDL3 controller events
pub trait GamepadProvider {
	/// poll for new gamepad events and apply them to `input`
	fn poll(&mut self, input: &mut InputState);
}

/// no-op gamepad provider. use this when a separate backend handles gamepads.
pub struct NoGamepad;

impl GamepadProvider for NoGamepad {
	fn poll(&mut self, _input: &mut InputState) {}
}

/// SDL3-backed gamepad provider.
///
/// receives gamepad events routed from [`process_events`] via the SDL3 event pump.
/// event routing happens inside [`process_events`] because SDL3 delivers all input
/// (keyboard, mouse, controller) through a single event pump; they cannot be split.
///
/// to swap to a different backend (e.g. gilrs):
/// - pass `&mut NoGamepad` to [`process_events`]
/// - call `your_provider.poll(input)` after `process_events` returns
#[cfg(not(target_arch = "wasm32"))]
pub struct SdlGamepadProvider {
	gamepad_subsystem: sdl3::GamepadSubsystem,
	/// maps SDL joystick id → (open gamepad handle, engine gamepad index)
	open_gamepads: HashMap<u32, (sdl3::gamepad::Gamepad, usize)>,
}

#[cfg(not(target_arch = "wasm32"))]
impl SdlGamepadProvider {
	/// create a new provider from an already-initialized SDL3 gamepad subsystem.
	#[must_use]
	pub fn new(gamepad_subsystem: sdl3::GamepadSubsystem) -> Self {
		Self {
			gamepad_subsystem,
			open_gamepads: HashMap::default(),
		}
	}

	fn handle_event(&mut self, event: &sdl3::event::Event, input: &mut InputState) {
		use sdl3::event::Event;
		match event {
			Event::ControllerDeviceAdded { which, .. } => {
				let joystick_id = sdl3::sys::joystick::SDL_JoystickID(*which);
				match self.gamepad_subsystem.open(joystick_id) {
					Ok(gamepad) => {
						let engine_index = input.add_gamepad();
						self.open_gamepads.insert(*which, (gamepad, engine_index));
						log::info!(
							"gamepad {} connected (engine index {})",
							which,
							engine_index
						);
					}
					Err(error) => log::warn!("failed to open gamepad {}: {}", which, error),
				}
			}
			Event::ControllerDeviceRemoved { which, .. } => {
				if let Some((_, engine_index)) = self.open_gamepads.remove(which) {
					input.remove_gamepad(engine_index);
					log::info!("gamepad {} disconnected", which);
				}
			}
			Event::ControllerButtonDown { which, button, .. } => {
				if let Some((_, engine_index)) = self.open_gamepads.get(which)
					&& let Some(mapped) = gamepad_button_from_sdl(*button)
				{
					input.press_gamepad_button(*engine_index, mapped);
				}
			}
			Event::ControllerButtonUp { which, button, .. } => {
				if let Some((_, engine_index)) = self.open_gamepads.get(which)
					&& let Some(mapped) = gamepad_button_from_sdl(*button)
				{
					input.release_gamepad_button(*engine_index, mapped);
				}
			}
			Event::ControllerAxisMotion {
				which, axis, value, ..
			} => {
				if let Some((_, engine_index)) = self.open_gamepads.get(which)
					&& let Some(mapped) = gamepad_axis_from_sdl(*axis)
				{
					let normalized = (*value as f32 / 32767.0).clamp(-1.0, 1.0);
					input.set_gamepad_axis(*engine_index, mapped, normalized);
				}
			}
			_ => {}
		}
	}
}

/// process SDL3 events and update the input state.
///
/// call once per frame before the ECS tick. collects all SDL3 events and routes
/// keyboard/mouse events internally, controller events to `gamepad`.
///
/// pass `&mut NoGamepad` when a separate library handles controller input.
///
/// # quit handling
///
/// if a quit event is received, the [`EngineState`] is set to [`EngineState::Stopping`].
#[cfg(not(target_arch = "wasm32"))]
pub fn process_events(
	event_pump: &mut sdl3::EventPump,
	gamepad: &mut SdlGamepadProvider,
	world: &mut bevy_ecs::prelude::World,
) {
	use sdl3::event::Event;

	let mut got_quit = false;

	// process events directly from the iterator, no intermediate Vec allocation.
	// the InputState borrow must be released before we can access EngineState below,
	// so it lives in its own block.
	{
		if let Some(mut input) = world.get_resource_mut::<InputState>() {
			// clear just_pressed/just_released here, not in PostUpdate; this keeps
			// edge events alive until the next logic tick even when ticks == 0 for a
			// display frame (e.g. 360fps render with 60hz tick rate).
			input.begin_frame();
			for event in event_pump.poll_iter() {
				match &event {
					Event::KeyDown {
						keycode: Some(key),
						repeat: false,
						..
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
					Event::Quit { .. } => got_quit = true,
					_ => gamepad.handle_event(&event, &mut input),
				}
			}
		}
	}

	if got_quit && let Some(mut state) = world.get_resource_mut::<EngineState>() {
		*state = EngineState::Stopping;
	}
}

#[cfg(not(target_arch = "wasm32"))]
const fn keycode_from_sdl(key: sdl3::keyboard::Keycode) -> Option<KeyCode> {
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
		Keycode::Minus => Some(KeyCode::Minus),
		Keycode::Equals => Some(KeyCode::Equals),
		Keycode::LeftBracket => Some(KeyCode::LeftBracket),
		Keycode::RightBracket => Some(KeyCode::RightBracket),
		Keycode::Grave => Some(KeyCode::Grave),
		Keycode::_0 => Some(KeyCode::Num0),
		Keycode::_1 => Some(KeyCode::Num1),
		Keycode::_2 => Some(KeyCode::Num2),
		Keycode::_3 => Some(KeyCode::Num3),
		Keycode::_4 => Some(KeyCode::Num4),
		Keycode::_5 => Some(KeyCode::Num5),
		Keycode::_6 => Some(KeyCode::Num6),
		Keycode::_7 => Some(KeyCode::Num7),
		Keycode::_8 => Some(KeyCode::Num8),
		Keycode::_9 => Some(KeyCode::Num9),
		Keycode::Semicolon => Some(KeyCode::Semicolon),
		Keycode::Apostrophe => Some(KeyCode::Apostrophe),
		Keycode::Comma => Some(KeyCode::Comma),
		Keycode::Period => Some(KeyCode::Period),
		Keycode::Slash => Some(KeyCode::Slash),
		Keycode::Backslash => Some(KeyCode::Backslash),
		Keycode::Home => Some(KeyCode::Home),
		Keycode::End => Some(KeyCode::End),
		Keycode::PageUp => Some(KeyCode::PageUp),
		Keycode::PageDown => Some(KeyCode::PageDown),
		Keycode::Insert => Some(KeyCode::Insert),
		Keycode::Delete => Some(KeyCode::Delete),
		Keycode::Kp0 => Some(KeyCode::Numpad0),
		Keycode::Kp1 => Some(KeyCode::Numpad1),
		Keycode::Kp2 => Some(KeyCode::Numpad2),
		Keycode::Kp3 => Some(KeyCode::Numpad3),
		Keycode::Kp4 => Some(KeyCode::Numpad4),
		Keycode::Kp5 => Some(KeyCode::Numpad5),
		Keycode::Kp6 => Some(KeyCode::Numpad6),
		Keycode::Kp7 => Some(KeyCode::Numpad7),
		Keycode::Kp8 => Some(KeyCode::Numpad8),
		Keycode::Kp9 => Some(KeyCode::Numpad9),
		Keycode::KpPlus => Some(KeyCode::NumpadAdd),
		Keycode::KpMinus => Some(KeyCode::NumpadSub),
		Keycode::KpMultiply => Some(KeyCode::NumpadMul),
		Keycode::KpDivide => Some(KeyCode::NumpadDiv),
		Keycode::KpEnter => Some(KeyCode::NumpadEnter),
		Keycode::KpPeriod => Some(KeyCode::NumpadDecimal),
		Keycode::NumLockClear => Some(KeyCode::NumLock),
		Keycode::CapsLock => Some(KeyCode::CapsLock),
		Keycode::ScrollLock => Some(KeyCode::ScrollLock),
		Keycode::Pause => Some(KeyCode::Pause),
		Keycode::PrintScreen => Some(KeyCode::PrintScreen),
		Keycode::LGui => Some(KeyCode::LSuper),
		Keycode::RGui => Some(KeyCode::RSuper),
		Keycode::MediaPlay | Keycode::MediaPlayPause => Some(KeyCode::MediaPlay),
		Keycode::MediaStop => Some(KeyCode::MediaStop),
		Keycode::MediaNextTrack => Some(KeyCode::MediaNext),
		Keycode::MediaPreviousTrack => Some(KeyCode::MediaPrev),
		Keycode::VolumeUp => Some(KeyCode::VolumeUp),
		Keycode::VolumeDown => Some(KeyCode::VolumeDown),
		Keycode::Mute => Some(KeyCode::Mute),
		Keycode::F13 => Some(KeyCode::F13),
		Keycode::F14 => Some(KeyCode::F14),
		Keycode::F15 => Some(KeyCode::F15),
		Keycode::F16 => Some(KeyCode::F16),
		Keycode::F17 => Some(KeyCode::F17),
		Keycode::F18 => Some(KeyCode::F18),
		Keycode::F19 => Some(KeyCode::F19),
		Keycode::F20 => Some(KeyCode::F20),
		Keycode::F21 => Some(KeyCode::F21),
		Keycode::F22 => Some(KeyCode::F22),
		Keycode::F23 => Some(KeyCode::F23),
		Keycode::F24 => Some(KeyCode::F24),
		_ => None,
	}
}

#[cfg(not(target_arch = "wasm32"))]
const fn mouse_button_from_sdl(button: sdl3::mouse::MouseButton) -> Option<MouseButton> {
	use sdl3::mouse::MouseButton as SdlBtn;
	match button {
		SdlBtn::Left => Some(MouseButton::Left),
		SdlBtn::Right => Some(MouseButton::Right),
		SdlBtn::Middle => Some(MouseButton::Middle),
		_ => None,
	}
}

#[cfg(not(target_arch = "wasm32"))]
const fn gamepad_button_from_sdl(button: sdl3::gamepad::Button) -> Option<GamepadButton> {
	use sdl3::gamepad::Button as SdlBtn;
	match button {
		SdlBtn::South => Some(GamepadButton::South),
		SdlBtn::East => Some(GamepadButton::East),
		SdlBtn::West => Some(GamepadButton::West),
		SdlBtn::North => Some(GamepadButton::North),
		SdlBtn::LeftShoulder => Some(GamepadButton::LeftShoulder),
		SdlBtn::RightShoulder => Some(GamepadButton::RightShoulder),
		SdlBtn::LeftStick => Some(GamepadButton::LeftStick),
		SdlBtn::RightStick => Some(GamepadButton::RightStick),
		SdlBtn::Back => Some(GamepadButton::Back),
		SdlBtn::Start => Some(GamepadButton::Start),
		SdlBtn::Guide => Some(GamepadButton::Home),
		SdlBtn::DPadUp => Some(GamepadButton::DpadUp),
		SdlBtn::DPadDown => Some(GamepadButton::DpadDown),
		SdlBtn::DPadLeft => Some(GamepadButton::DpadLeft),
		SdlBtn::DPadRight => Some(GamepadButton::DpadRight),
		SdlBtn::Misc1 => Some(GamepadButton::Share),
		_ => None,
	}
}

#[cfg(not(target_arch = "wasm32"))]
const fn gamepad_axis_from_sdl(axis: sdl3::gamepad::Axis) -> Option<GamepadAxis> {
	use sdl3::gamepad::Axis as SdlAxis;
	match axis {
		SdlAxis::LeftX => Some(GamepadAxis::LeftStickX),
		SdlAxis::LeftY => Some(GamepadAxis::LeftStickY),
		SdlAxis::RightX => Some(GamepadAxis::RightStickX),
		SdlAxis::RightY => Some(GamepadAxis::RightStickY),
		SdlAxis::TriggerLeft => Some(GamepadAxis::LeftTrigger),
		SdlAxis::TriggerRight => Some(GamepadAxis::RightTrigger),
	}
}

/// map a browser keyboard event to a KeyCode (corr-37). letters follow `key`,
/// so they respect the keyboard layout like SDL keycodes do on native, falling
/// back to the physical `code` when the layout's character isn't a latin letter.
/// everything else follows `code`: it is shift-invariant (a key pressed as "1"
/// and released as "!" is still one key) and tells numpad and left/right
/// modifiers apart, which `key` can't.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn key_from_web(key: &str, code: &str) -> Option<KeyCode> {
	const LETTERS: [KeyCode; 26] = [
		KeyCode::A,
		KeyCode::B,
		KeyCode::C,
		KeyCode::D,
		KeyCode::E,
		KeyCode::F,
		KeyCode::G,
		KeyCode::H,
		KeyCode::I,
		KeyCode::J,
		KeyCode::K,
		KeyCode::L,
		KeyCode::M,
		KeyCode::N,
		KeyCode::O,
		KeyCode::P,
		KeyCode::Q,
		KeyCode::R,
		KeyCode::S,
		KeyCode::T,
		KeyCode::U,
		KeyCode::V,
		KeyCode::W,
		KeyCode::X,
		KeyCode::Y,
		KeyCode::Z,
	];
	let letter = |c: u8| LETTERS[usize::from(c.to_ascii_lowercase() - b'a')];
	if let [c] = key.as_bytes()
		&& c.is_ascii_alphabetic()
	{
		return Some(letter(*c));
	}
	if let Some(rest) = code.strip_prefix("Key")
		&& let [c] = rest.as_bytes()
		&& c.is_ascii_alphabetic()
	{
		return Some(letter(*c));
	}
	Some(match code {
		"Digit0" => KeyCode::Num0,
		"Digit1" => KeyCode::Num1,
		"Digit2" => KeyCode::Num2,
		"Digit3" => KeyCode::Num3,
		"Digit4" => KeyCode::Num4,
		"Digit5" => KeyCode::Num5,
		"Digit6" => KeyCode::Num6,
		"Digit7" => KeyCode::Num7,
		"Digit8" => KeyCode::Num8,
		"Digit9" => KeyCode::Num9,
		"F1" => KeyCode::F1,
		"F2" => KeyCode::F2,
		"F3" => KeyCode::F3,
		"F4" => KeyCode::F4,
		"F5" => KeyCode::F5,
		"F6" => KeyCode::F6,
		"F7" => KeyCode::F7,
		"F8" => KeyCode::F8,
		"F9" => KeyCode::F9,
		"F10" => KeyCode::F10,
		"F11" => KeyCode::F11,
		"F12" => KeyCode::F12,
		"F13" => KeyCode::F13,
		"F14" => KeyCode::F14,
		"F15" => KeyCode::F15,
		"F16" => KeyCode::F16,
		"F17" => KeyCode::F17,
		"F18" => KeyCode::F18,
		"F19" => KeyCode::F19,
		"F20" => KeyCode::F20,
		"F21" => KeyCode::F21,
		"F22" => KeyCode::F22,
		"F23" => KeyCode::F23,
		"F24" => KeyCode::F24,
		"Escape" => KeyCode::Escape,
		"Space" => KeyCode::Space,
		"Enter" => KeyCode::Enter,
		"Tab" => KeyCode::Tab,
		"Backspace" => KeyCode::Backspace,
		"ArrowLeft" => KeyCode::Left,
		"ArrowRight" => KeyCode::Right,
		"ArrowUp" => KeyCode::Up,
		"ArrowDown" => KeyCode::Down,
		"ShiftLeft" => KeyCode::LShift,
		"ShiftRight" => KeyCode::RShift,
		"ControlLeft" => KeyCode::LCtrl,
		"ControlRight" => KeyCode::RCtrl,
		"AltLeft" => KeyCode::LAlt,
		"AltRight" => KeyCode::RAlt,
		// "OS*" is firefox < 118's name for the meta keys
		"MetaLeft" | "OSLeft" => KeyCode::LSuper,
		"MetaRight" | "OSRight" => KeyCode::RSuper,
		"Minus" => KeyCode::Minus,
		"Equal" => KeyCode::Equals,
		"Semicolon" => KeyCode::Semicolon,
		"Quote" => KeyCode::Apostrophe,
		"Comma" => KeyCode::Comma,
		"Period" => KeyCode::Period,
		"Slash" => KeyCode::Slash,
		"Backslash" => KeyCode::Backslash,
		"BracketLeft" => KeyCode::LeftBracket,
		"BracketRight" => KeyCode::RightBracket,
		"Backquote" => KeyCode::Grave,
		"Home" => KeyCode::Home,
		"End" => KeyCode::End,
		"PageUp" => KeyCode::PageUp,
		"PageDown" => KeyCode::PageDown,
		"Insert" => KeyCode::Insert,
		"Delete" => KeyCode::Delete,
		"Numpad0" => KeyCode::Numpad0,
		"Numpad1" => KeyCode::Numpad1,
		"Numpad2" => KeyCode::Numpad2,
		"Numpad3" => KeyCode::Numpad3,
		"Numpad4" => KeyCode::Numpad4,
		"Numpad5" => KeyCode::Numpad5,
		"Numpad6" => KeyCode::Numpad6,
		"Numpad7" => KeyCode::Numpad7,
		"Numpad8" => KeyCode::Numpad8,
		"Numpad9" => KeyCode::Numpad9,
		"NumpadAdd" => KeyCode::NumpadAdd,
		"NumpadSubtract" => KeyCode::NumpadSub,
		"NumpadMultiply" => KeyCode::NumpadMul,
		"NumpadDivide" => KeyCode::NumpadDiv,
		"NumpadEnter" => KeyCode::NumpadEnter,
		"NumpadDecimal" => KeyCode::NumpadDecimal,
		"NumLock" => KeyCode::NumLock,
		"CapsLock" => KeyCode::CapsLock,
		"ScrollLock" => KeyCode::ScrollLock,
		"Pause" => KeyCode::Pause,
		"PrintScreen" => KeyCode::PrintScreen,
		"MediaPlayPause" => KeyCode::MediaPlay,
		"MediaStop" => KeyCode::MediaStop,
		"MediaTrackNext" => KeyCode::MediaNext,
		"MediaTrackPrevious" => KeyCode::MediaPrev,
		"AudioVolumeUp" | "VolumeUp" => KeyCode::VolumeUp,
		"AudioVolumeDown" | "VolumeDown" => KeyCode::VolumeDown,
		"AudioVolumeMute" | "VolumeMute" => KeyCode::Mute,
		_ => return None,
	})
}

/// what a browser standard-mapping gamepad button index drives. LT/RT (6, 7)
/// are analog buttons, so they feed the trigger axes from `GamepadButton.value`
/// rather than a digital button (corr-36).
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq)]
enum WebPadInput {
	Button(GamepadButton),
	Axis(GamepadAxis),
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
const fn web_pad_input(index: usize) -> Option<WebPadInput> {
	Some(match index {
		0 => WebPadInput::Button(GamepadButton::South),
		1 => WebPadInput::Button(GamepadButton::East),
		2 => WebPadInput::Button(GamepadButton::West),
		3 => WebPadInput::Button(GamepadButton::North),
		4 => WebPadInput::Button(GamepadButton::LeftShoulder),
		5 => WebPadInput::Button(GamepadButton::RightShoulder),
		6 => WebPadInput::Axis(GamepadAxis::LeftTrigger),
		7 => WebPadInput::Axis(GamepadAxis::RightTrigger),
		8 => WebPadInput::Button(GamepadButton::Back),
		9 => WebPadInput::Button(GamepadButton::Start),
		10 => WebPadInput::Button(GamepadButton::LeftStick),
		11 => WebPadInput::Button(GamepadButton::RightStick),
		12 => WebPadInput::Button(GamepadButton::DpadUp),
		13 => WebPadInput::Button(GamepadButton::DpadDown),
		14 => WebPadInput::Button(GamepadButton::DpadLeft),
		15 => WebPadInput::Button(GamepadButton::DpadRight),
		16 => WebPadInput::Button(GamepadButton::Home),
		_ => return None,
	})
}

/// web input event queue (populated by JS callbacks via wasm-bindgen)
#[cfg(target_arch = "wasm32")]
mod web_input {
	use super::{GamepadAxis, GamepadButton, KeyCode, MouseButton};
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
		GamepadButtonPress {
			gamepad_index: usize,
			button: GamepadButton,
		},
		GamepadButtonRelease {
			gamepad_index: usize,
			button: GamepadButton,
		},
		GamepadAxisMove {
			gamepad_index: usize,
			axis: GamepadAxis,
			value: f32,
		},
	}

	thread_local! {
		static EVENT_QUEUE: RefCell<VecDeque<WebEvent>> = const { RefCell::new(VecDeque::new()) };
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
					// the browser api reports pads by index and nothing registers them
					// (sdl's device-added event is native-only), so without this every
					// web gamepad event hit an empty gamepad list and was dropped
					WebEvent::GamepadButtonPress {
						gamepad_index,
						button,
					} => {
						input.ensure_gamepad(gamepad_index);
						input.press_gamepad_button(gamepad_index, button);
					}
					WebEvent::GamepadButtonRelease {
						gamepad_index,
						button,
					} => {
						input.ensure_gamepad(gamepad_index);
						input.release_gamepad_button(gamepad_index, button);
					}
					WebEvent::GamepadAxisMove {
						gamepad_index,
						axis,
						value,
					} => {
						input.ensure_gamepad(gamepad_index);
						input.set_gamepad_axis(gamepad_index, axis, value);
					}
				}
			}
		});
	}

	/// push a gamepad button down event
	pub fn push_gamepad_button(gamepad_index: usize, button: GamepadButton) {
		EVENT_QUEUE.with(|q| {
			q.borrow_mut().push_back(WebEvent::GamepadButtonPress {
				gamepad_index,
				button,
			})
		});
	}

	/// push a gamepad button up event
	pub fn release_gamepad_button(gamepad_index: usize, button: GamepadButton) {
		EVENT_QUEUE.with(|q| {
			q.borrow_mut().push_back(WebEvent::GamepadButtonRelease {
				gamepad_index,
				button,
			})
		});
	}

	/// push a gamepad axis move event
	pub fn push_gamepad_axis(gamepad_index: usize, axis: GamepadAxis, value: f32) {
		EVENT_QUEUE.with(|q| {
			q.borrow_mut().push_back(WebEvent::GamepadAxisMove {
				gamepad_index,
				axis,
				value,
			})
		});
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
		web_input::drain_to_input(&mut input);
		poll_gamepads(&mut input);
	}
}

/// poll the gamepad API for connected gamepads (WASM target)
#[cfg(target_arch = "wasm32")]
fn poll_gamepads(_input: &mut InputState) {
	use web_input::{push_gamepad_axis, push_gamepad_button, release_gamepad_button};
	use web_sys::Gamepad;

	let window = match web_sys::window() {
		Some(w) => w,
		None => return,
	};
	let navigator = window.navigator();
	let gamepads = match navigator.get_gamepads() {
		Ok(g) => g,
		Err(_) => return,
	};

	for (index, gamepad_opt) in gamepads.iter().enumerate() {
		use wasm_bindgen::JsCast;
		let gamepad: Gamepad = match gamepad_opt.dyn_into() {
			Ok(g) => g,
			Err(_) => continue,
		};

		// poll buttons
		let buttons = gamepad.buttons();
		for (btn_index, btn) in buttons.iter().enumerate() {
			let web_btn: web_sys::GamepadButton = match btn.dyn_into() {
				Ok(b) => b,
				Err(_) => continue,
			};
			match web_pad_input(btn_index) {
				Some(WebPadInput::Button(button)) => {
					if web_btn.pressed() {
						push_gamepad_button(index, button);
					} else {
						release_gamepad_button(index, button);
					}
				}
				Some(WebPadInput::Axis(axis)) => {
					push_gamepad_axis(index, axis, web_btn.value() as f32);
				}
				None => {}
			}
		}

		// poll axes
		let axes = gamepad.axes();
		if axes.length() > 0 {
			push_gamepad_axis(
				index,
				GamepadAxis::LeftStickX,
				axes.get(0).as_f64().unwrap_or(0.0) as f32,
			);
		}
		if axes.length() > 1 {
			push_gamepad_axis(
				index,
				GamepadAxis::LeftStickY,
				axes.get(1).as_f64().unwrap_or(0.0) as f32,
			);
		}
		if axes.length() > 2 {
			push_gamepad_axis(
				index,
				GamepadAxis::RightStickX,
				axes.get(2).as_f64().unwrap_or(0.0) as f32,
			);
		}
		if axes.length() > 3 {
			push_gamepad_axis(
				index,
				GamepadAxis::RightStickY,
				axes.get(3).as_f64().unwrap_or(0.0) as f32,
			);
		}
	}
}

/// set up web input event listeners on the given canvas element.
/// call this once during initialization on WASM target.
#[cfg(target_arch = "wasm32")]
pub fn setup_web_input(canvas: &web_sys::HtmlElement) {
	use wasm_bindgen::JsCast;
	use web_input::mouse_button_from_web;
	use web_sys::EventTarget;

	let canvas_target: &EventTarget = canvas.as_ref();

	// keyboard events on document body (not canvas: canvas doesn't receive keyboard events)
	{
		let window = web_sys::window().expect("no window");
		let document = window.document().expect("no document");
		let body = doc_body(&document);
		let target: &EventTarget = body.as_ref();

		let keydown_closure =
			wasm_bindgen::closure::Closure::wrap(Box::new(move |event: web_sys::KeyboardEvent| {
				event.prevent_default();
				if let Some(code) = key_from_web(&event.key(), &event.code()) {
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
				if let Some(code) = key_from_web(&event.key(), &event.code()) {
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

		// request pointer lock on click so mouse movement stays captured inside the page
		let canvas_for_lock = canvas.clone();
		let click_closure = wasm_bindgen::closure::Closure::wrap(Box::new(move || {
			canvas_for_lock
				.unchecked_ref::<web_sys::Element>()
				.request_pointer_lock();
		}) as Box<dyn FnMut()>);
		canvas_target
			.add_event_listener_with_callback("click", click_closure.as_ref().unchecked_ref())
			.expect("failed to add click listener");
		click_closure.forget();
	}
}

#[cfg(target_arch = "wasm32")]
fn doc_body(document: &web_sys::Document) -> web_sys::HtmlElement {
	document.body().expect("no body element")
}

#[cfg(test)]
mod tests {
	use super::*;

	fn make_input() -> InputState {
		InputState::new()
	}

	/// corr-37: every KeyCode past the original core set was declared but never
	/// translated, so binding it compiled and never fired.
	#[cfg(not(target_arch = "wasm32"))]
	#[test]
	fn sdl_translates_extended_keys() {
		use sdl3::keyboard::Keycode as K;
		let cases = [
			(K::Semicolon, KeyCode::Semicolon),
			(K::Apostrophe, KeyCode::Apostrophe),
			(K::Comma, KeyCode::Comma),
			(K::Period, KeyCode::Period),
			(K::Slash, KeyCode::Slash),
			(K::Backslash, KeyCode::Backslash),
			(K::Home, KeyCode::Home),
			(K::End, KeyCode::End),
			(K::PageUp, KeyCode::PageUp),
			(K::PageDown, KeyCode::PageDown),
			(K::Insert, KeyCode::Insert),
			(K::Delete, KeyCode::Delete),
			(K::Kp0, KeyCode::Numpad0),
			(K::Kp9, KeyCode::Numpad9),
			(K::KpPlus, KeyCode::NumpadAdd),
			(K::KpMinus, KeyCode::NumpadSub),
			(K::KpMultiply, KeyCode::NumpadMul),
			(K::KpDivide, KeyCode::NumpadDiv),
			(K::KpEnter, KeyCode::NumpadEnter),
			(K::KpPeriod, KeyCode::NumpadDecimal),
			(K::NumLockClear, KeyCode::NumLock),
			(K::CapsLock, KeyCode::CapsLock),
			(K::ScrollLock, KeyCode::ScrollLock),
			(K::Pause, KeyCode::Pause),
			(K::PrintScreen, KeyCode::PrintScreen),
			(K::LGui, KeyCode::LSuper),
			(K::RGui, KeyCode::RSuper),
			(K::MediaPlay, KeyCode::MediaPlay),
			(K::MediaStop, KeyCode::MediaStop),
			(K::MediaNextTrack, KeyCode::MediaNext),
			(K::MediaPreviousTrack, KeyCode::MediaPrev),
			(K::VolumeUp, KeyCode::VolumeUp),
			(K::VolumeDown, KeyCode::VolumeDown),
			(K::Mute, KeyCode::Mute),
			(K::F13, KeyCode::F13),
			(K::F24, KeyCode::F24),
		];
		for (sdl, expected) in cases {
			assert_eq!(keycode_from_sdl(sdl), Some(expected), "{sdl:?}");
		}
		assert_eq!(
			gamepad_button_from_sdl(sdl3::gamepad::Button::Misc1),
			Some(GamepadButton::Share)
		);
	}

	/// corr-37: the web mapper covers the extended keys, keeps letters
	/// layout-aware, and resolves the rest by physical code.
	#[test]
	fn web_translates_keys() {
		let cases = [
			("a", "KeyQ", KeyCode::A), // azerty: layout decides letters
			("Q", "KeyQ", KeyCode::Q),
			("ф", "KeyA", KeyCode::A), // non-latin layout falls back to position
			("!", "Digit1", KeyCode::Num1), // shift-invariant
			("1", "Numpad1", KeyCode::Numpad1),
			("Shift", "ShiftRight", KeyCode::RShift),
			("Control", "ControlRight", KeyCode::RCtrl),
			("Meta", "MetaLeft", KeyCode::LSuper),
			(";", "Semicolon", KeyCode::Semicolon),
			("'", "Quote", KeyCode::Apostrophe),
			("-", "Minus", KeyCode::Minus),
			("=", "Equal", KeyCode::Equals),
			("[", "BracketLeft", KeyCode::LeftBracket),
			("Home", "Home", KeyCode::Home),
			("PageDown", "PageDown", KeyCode::PageDown),
			("+", "NumpadAdd", KeyCode::NumpadAdd),
			("Enter", "NumpadEnter", KeyCode::NumpadEnter),
			("CapsLock", "CapsLock", KeyCode::CapsLock),
			("F24", "F24", KeyCode::F24),
			("AudioVolumeMute", "AudioVolumeMute", KeyCode::Mute),
			(" ", "Space", KeyCode::Space),
		];
		for (key, code, expected) in cases {
			assert_eq!(key_from_web(key, code), Some(expected), "{key:?}/{code:?}");
		}
		assert_eq!(key_from_web("Unidentified", "Lang1"), None);
	}

	/// corr-36: the browser's analog LT/RT buttons drive the trigger axes.
	#[test]
	fn web_triggers_map_to_axes() {
		assert_eq!(
			web_pad_input(6),
			Some(WebPadInput::Axis(GamepadAxis::LeftTrigger))
		);
		assert_eq!(
			web_pad_input(7),
			Some(WebPadInput::Axis(GamepadAxis::RightTrigger))
		);
		assert_eq!(
			web_pad_input(16),
			Some(WebPadInput::Button(GamepadButton::Home))
		);
	}

	/// corr-38: an axis binding's just-pressed/just-released must be edges, not
	/// "held" every tick and "never" respectively.
	#[test]
	fn axis_binding_edges_fire_once() {
		let mut input = make_input();
		let mut actions = ActionMap::new();
		actions.bind(
			"fire",
			InputBinding::GamepadAxis(0, GamepadAxis::RightTrigger, 0.5),
		);
		input.add_gamepad();

		input.begin_frame();
		input.set_gamepad_axis(0, GamepadAxis::RightTrigger, 0.9);
		assert!(actions.is_action_just_pressed(&input, "fire"));

		input.begin_frame();
		assert!(actions.is_action_held(&input, "fire"));
		assert!(!actions.is_action_just_pressed(&input, "fire"));

		input.begin_frame();
		input.set_gamepad_axis(0, GamepadAxis::RightTrigger, 0.1);
		assert!(actions.is_action_just_released(&input, "fire"));

		input.begin_frame();
		assert!(!actions.is_action_just_released(&input, "fire"));
	}

	#[test]
	fn action_map_bind_and_check_held() {
		let mut input = make_input();
		let mut actions = ActionMap::new();

		actions.bind("jump", InputBinding::Key(KeyCode::Space));
		input.press_key(KeyCode::Space);
		input.promote_pending(); // edges become visible on the tick

		assert!(actions.is_action_held(&input, "jump"));
		assert!(actions.is_action_just_pressed(&input, "jump"));
	}

	#[test]
	fn action_map_multiple_bindings() {
		let mut input = make_input();
		let mut actions = ActionMap::new();

		actions.bind("fire", InputBinding::Mouse(MouseButton::Left));
		actions.bind("fire", InputBinding::Key(KeyCode::F));

		input.press_mouse_button(MouseButton::Left);
		input.promote_pending(); // edges become visible on the tick
		assert!(actions.is_action_held(&input, "fire"));
		assert!(actions.is_action_just_pressed(&input, "fire"));
	}

	#[test]
	fn action_map_no_bindings_returns_false() {
		let input = make_input();
		let actions = ActionMap::new();

		assert!(!actions.is_action_held(&input, "nonexistent"));
		assert!(!actions.is_action_just_pressed(&input, "nonexistent"));
		assert!(!actions.is_action_just_released(&input, "nonexistent"));
	}

	#[test]
	fn action_map_unbind() {
		let mut input = make_input();
		let mut actions = ActionMap::new();

		actions.bind("jump", InputBinding::Key(KeyCode::Space));
		actions.unbind("jump");

		input.press_key(KeyCode::Space);
		assert!(!actions.is_action_held(&input, "jump"));
	}

	#[test]
	fn action_map_has_action() {
		let mut actions = ActionMap::new();
		assert!(!actions.has_action("jump"));

		actions.bind("jump", InputBinding::Key(KeyCode::Space));
		assert!(actions.has_action("jump"));

		actions.unbind("jump");
		assert!(!actions.has_action("jump"));
	}

	#[test]
	fn action_map_list_actions() {
		let mut actions = ActionMap::new();
		actions.bind("jump", InputBinding::Key(KeyCode::Space));
		actions.bind("fire", InputBinding::Mouse(MouseButton::Left));

		let mut names: Vec<&str> = actions.actions().collect();
		names.sort();
		assert_eq!(names, vec!["fire", "jump"]);
	}

	#[test]
	fn action_map_gamepad_button() {
		let mut input = make_input();
		let mut actions = ActionMap::new();

		let gp_index = input.add_gamepad();
		actions.bind(
			"jump",
			InputBinding::GamepadButton(gp_index, GamepadButton::South),
		);

		input.press_gamepad_button(gp_index, GamepadButton::South);
		assert!(actions.is_action_held(&input, "jump"));
		assert!(actions.is_action_just_pressed(&input, "jump"));
	}

	#[test]
	fn action_map_gamepad_axis() {
		let mut input = make_input();
		let mut actions = ActionMap::new();

		let gp_index = input.add_gamepad();
		actions.bind(
			"move_left",
			InputBinding::GamepadAxis(gp_index, GamepadAxis::LeftStickX, -0.5),
		);

		input.set_gamepad_axis(gp_index, GamepadAxis::LeftStickX, -0.8);
		assert!(actions.is_action_held(&input, "move_left"));

		input.set_gamepad_axis(gp_index, GamepadAxis::LeftStickX, -0.3);
		assert!(!actions.is_action_held(&input, "move_left"));
	}

	/// corr-16: opposite-sign bindings on one axis must not both fire.
	#[test]
	fn action_map_gamepad_axis_is_directional() {
		let mut input = make_input();
		let mut actions = ActionMap::new();
		let gp = input.add_gamepad();
		actions.bind("left", InputBinding::GamepadAxis(gp, GamepadAxis::LeftStickX, -0.5));
		actions.bind("right", InputBinding::GamepadAxis(gp, GamepadAxis::LeftStickX, 0.5));

		input.set_gamepad_axis(gp, GamepadAxis::LeftStickX, -1.0);
		assert!(actions.is_action_held(&input, "left"));
		assert!(!actions.is_action_held(&input, "right"));

		input.set_gamepad_axis(gp, GamepadAxis::LeftStickX, 1.0);
		assert!(!actions.is_action_held(&input, "left"));
		assert!(actions.is_action_held(&input, "right"));
	}

	#[test]
	fn action_map_key_release() {
		let mut input = make_input();
		let mut actions = ActionMap::new();

		actions.bind("jump", InputBinding::Key(KeyCode::Space));
		input.press_key(KeyCode::Space);
		input.promote_pending(); // tick 1: the press is visible, key held
		assert!(actions.is_action_held(&input, "jump"));
		assert!(actions.is_action_just_pressed(&input, "jump"));

		input.release_key(KeyCode::Space);
		input.promote_pending(); // tick 2: the release is visible, no longer held

		assert!(!actions.is_action_held(&input, "jump"));
		assert!(actions.is_action_just_released(&input, "jump"));
		// the press edge did not survive into the second tick
		assert!(!actions.is_action_just_pressed(&input, "jump"));
	}

	/// corr-04: web gamepads are reported by index and never registered, so every
	/// event was dropped. ensure_gamepad registers the slot the event names.
	#[test]
	fn ensure_gamepad_registers_slots_up_to_the_index() {
		let mut input = InputState::default();
		input.ensure_gamepad(1);
		input.press_gamepad_button(1, GamepadButton::South);
		assert!(input.gamepad(1).is_some_and(|g| g.is_button_held(GamepadButton::South)));
		assert!(input.gamepad(0).is_none(), "lower slots stay disconnected");
		input.ensure_gamepad(MAX_GAMEPADS);
		assert!(input.gamepad(MAX_GAMEPADS).is_none(), "out-of-range indices are ignored");
	}

	/// the index-based gamepad accessors docs/input.md documents
	#[test]
	fn gamepad_convenience_accessors() {
		let mut input = make_input();
		assert!(!input.is_gamepad_connected(0));
		assert_eq!(input.gamepad_axis(0, GamepadAxis::LeftStickX), 0.0);
		let gp = input.add_gamepad();
		input.press_gamepad_button(gp, GamepadButton::South);
		input.set_gamepad_axis(gp, GamepadAxis::LeftStickY, -0.75);
		assert!(input.is_gamepad_connected(gp));
		assert!(input.is_gamepad_button_held(gp, GamepadButton::South));
		assert_eq!(input.gamepad_axis(gp, GamepadAxis::LeftStickY), -0.75);
		assert!(!input.is_gamepad_button_held(gp + 1, GamepadButton::South));
	}

	/// corr-17: removing a pad shifted every later pad down, so the sdl provider's
	/// cached indices sent their input to the wrong pad (or nowhere).
	#[test]
	fn removing_a_gamepad_keeps_other_indices() {
		let mut input = make_input();
		let a = input.add_gamepad();
		let b = input.add_gamepad();
		let c = input.add_gamepad();
		input.remove_gamepad(a);
		input.press_gamepad_button(c, GamepadButton::South);
		assert!(input.is_gamepad_button_held(c, GamepadButton::South));
		assert!(!input.is_gamepad_button_held(b, GamepadButton::South));
		assert!(!input.is_gamepad_connected(a));
		assert_eq!(input.add_gamepad(), a, "a reconnect reuses the freed slot");
	}
}
