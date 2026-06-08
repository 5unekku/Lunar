# input

input is handled through `InputState` (raw key/button/axis state) and `ActionMap`
(named actions mapped to one or more bindings). both are resources available in any system.

## raw input

`InputState` tracks three states per key/button each tick:
- **held** — currently pressed
- **just pressed** — went down this tick (edge-triggered)
- **just released** — went up this tick (edge-triggered)

```rust
fn player_movement(input: Res<InputState>, mut query: Query<&mut Transform, With<Player>>) {
    for mut transform in &mut query {
        if input.is_key_held(KeyCode::Right) {
            transform.translation.x += 3.0;
        }
        if input.is_key_held(KeyCode::Left) {
            transform.translation.x -= 3.0;
        }
        if input.is_key_just_pressed(KeyCode::Space) {
            // jump — fires once on the frame the key is pressed
        }
    }
}
```

### keyboard methods

```rust
input.is_key_held(KeyCode::W)
input.is_key_just_pressed(KeyCode::Space)
input.is_key_just_released(KeyCode::Escape)
```

### mouse methods

```rust
input.is_mouse_button_held(MouseButton::Left)
input.is_mouse_button_just_pressed(MouseButton::Right)
input.mouse_position()          // Vec2 in screen pixels
input.mouse_delta()             // Vec2 movement since last frame (useful for FPS look)
input.mouse_scroll_delta()      // f32 scroll wheel delta
```

### gamepad methods

gamepad methods take an index (0 = first connected gamepad):

```rust
input.is_gamepad_button_held(0, GamepadButton::South)
input.is_gamepad_button_just_pressed(0, GamepadButton::East)
input.gamepad_axis(0, GamepadAxis::LeftX)   // f32 in -1.0..=1.0
input.gamepad_axis(0, GamepadAxis::LeftY)
input.is_gamepad_connected(0)               // bool
```

`GamepadAxis` variants: `LeftX`, `LeftY`, `RightX`, `RightY`, `LeftTrigger`, `RightTrigger`.

`GamepadButton` variants: `South`, `East`, `West`, `North`, `L1`, `R1`, `L2`, `R2`,
`Start`, `Select`, `DPadUp`, `DPadDown`, `DPadLeft`, `DPadRight`, `LeftStick`, `RightStick`.

## action map

`ActionMap` lets game code check named actions instead of hardcoded keys.
multiple bindings can map to the same action — any one triggers it.

### setup

bind actions in a startup system. `ActionMap` is inserted automatically by the
input plugin — you don't need to insert it yourself:

```rust
fn setup_input(mut actions: ResMut<ActionMap>) {
    // fluent builder style
    actions.action("jump")
        .key(KeyCode::Space)
        .button(GamepadButton::South);

    actions.action("fire")
        .mouse(MouseButton::Left)
        .key(KeyCode::Z);

    actions.action("move_right")
        .key(KeyCode::Right)
        .key(KeyCode::D)
        .axis(GamepadAxis::LeftX, 0.3);  // axis with deadzone
}
```

### checking actions

```rust
fn player_logic(input: Res<InputState>, actions: Res<ActionMap>) {
    if actions.is_action_just_pressed(&input, "jump") {
        // jump
    }
    if actions.is_action_held(&input, "move_right") {
        // moving right
    }
    if actions.is_action_just_released(&input, "fire") {
        // released fire
    }
}
```

### programmatic rebinding

```rust
fn apply_settings(mut actions: ResMut<ActionMap>, new_key: KeyCode) {
    actions.unbind("jump");
    actions.bind("jump", InputBinding::Key(new_key));
    actions.bind("jump", InputBinding::GamepadButton(0, GamepadButton::South));
}
```

## cursor lock (FPS-style)

lock the cursor for first-person mouse look via `WindowSettings`:

```rust
fn setup(mut window: ResMut<WindowSettings>) {
    window.cursor_locked = true;
}

fn fps_look(input: Res<InputState>, mut camera: ...) {
    let delta = input.mouse_delta();
    // delta.x = yaw, delta.y = pitch
}
```

the engine applies cursor lock via SDL3 before each frame. set `cursor_locked = false`
to release (e.g. when opening a menu).
