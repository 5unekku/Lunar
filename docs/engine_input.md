# engine_input

input subsystem via SDL3

handles keyboard, mouse, gamepad input. exposes state through clean interfaces.

# example

```ignore
use engine_input::{InputState, KeyCode};

fn player_movement(input: Res<InputState>, time: Res<Time>) {
    if input.is_key_just_pressed(KeyCode::Space) {
        // jump!
    }
    if input.is_key_held(KeyCode::Left) {
        // move left
    }
}
```

## Structs

### ActionMap

maps named action names to one or more [`InputBinding`]s.

this lets game code check actions like "jump" or "fire" instead of
hardcoding specific keys. multiple bindings can map to the same action
(e.g. both spacebar and a gamepad button can trigger "jump").

# example

```ignore
fn setup(mut action_map: ResMut<ActionMap>) {
    action_map.bind("jump", InputBinding::Key(KeyCode::Space));
    action_map.bind("jump", InputBinding::GamepadButton(0, GamepadButton::South));
    action_map.bind("fire", InputBinding::Mouse(MouseButton::Left));
}

fn player_logic(input: Res<InputState>, actions: Res<ActionMap>) {
    if actions.is_action_just_pressed(&input, "jump") {
        // jump!
    }
}
```

### GamepadState

gamepad state, tracked per gamepad index.
uses fixed-size arrays for O(1) lookup.

### InputPlugin

input plugin that initializes the SDL3 input subsystem.

add this plugin to your [`App`] to enable input handling.
it registers the [`InputState`] as an ECS resource.


### InputState

input state resource, tracks current and previous frame input.
uses fixed-size bool arrays indexed by discriminant value — O(1) lookup, no hashing.

## Enums

### GamepadAxis

standard gamepad axis layout.

### GamepadButton

standard gamepad button layout.
maps to a typical xbox-style controller.

### InputBinding

a single input binding that can be a key, mouse button, gamepad button, or gamepad axis.

used by [`ActionMap`] to map named actions to physical inputs.

### KeyCode

keyboard key codes mapped from SDL3.

each variant represents a physical key on the keyboard.
the discriminant values are used as indices into the input state arrays
for O(1) lookup.


### MouseButton

mouse button codes.

represents the three standard mouse buttons.
the discriminant values are used as indices into the input state arrays.

## Functions

### init_sdl

initialize SDL3 and return an event pump.

call this once at startup on native targets.
the returned pump should be used with [`process_events`].

# Panics
panics if SDL3 cannot be initialized or if the event pump cannot be created.

### process_events

process SDL3 events and update the input state.

this function should be called once per frame before the ECS tick.
pass the event pump returned from [`init_sdl`] to poll events.
