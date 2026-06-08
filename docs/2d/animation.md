# 2d sprite animation

requires `Plugin2d` (see `2d/rendering.md`).

`SpriteAnimation` drives frame-based atlas animation. attach it alongside `Sprite`:

```rust
fn setup(mut commands: Commands, mut assets: ResMut<AssetServer>) {
    let sheet = assets.load_texture("player_sheet.png");

    commands.spawn((
        Transform::from_xy(100.0, 100.0),
        Sprite::new(sheet),
        SpriteAnimation {
            frame_count: 8,
            frame_size: Vec2::new(32.0, 32.0),
            fps: 12.0,
            looping: true,
            current_frame: 0,
            timer: 0.0,
        },
    ));
}
```

`Plugin2d` advances all `SpriteAnimation` components automatically each tick and
writes the correct `source_rect` into the paired `Sprite`. the sheet is assumed
to be a horizontal strip — frame 0 is at x=0, frame 1 at x=frame_size.x, etc.

`SpriteAnimation` fields:
- `frame_count: usize` — total number of frames in the strip
- `frame_size: Vec2` — pixel size of one frame
- `fps: f32` — playback speed in frames per second
- `looping: bool` — restart from frame 0 when the last frame is reached
- `current_frame: usize` — current frame index (writable to jump to a frame)
- `timer: f32` — time accumulated since the last frame advance (writable to reset)

to switch animations (e.g. idle → walk), swap the texture and reset the component:

```rust
fn switch_animation(
    mut query: Query<(&mut Sprite, &mut SpriteAnimation), With<Player>>,
    assets: Res<AssetServer>,
    handles: Res<AnimationHandles>,
    input: Res<InputState>,
) {
    for (mut sprite, mut anim) in &mut query {
        if input.is_key_just_pressed(KeyCode::Right) {
            sprite.texture = handles.walk;
            *anim = SpriteAnimation {
                frame_count: 8,
                frame_size: Vec2::new(32.0, 32.0),
                fps: 12.0,
                looping: true,
                current_frame: 0,
                timer: 0.0,
            };
        }
    }
}
```
