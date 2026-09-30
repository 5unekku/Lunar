# 2d sprite animation

requires `Plugin2d` (see `2d/rendering.md`).

`SpriteAnimation` drives frame-based atlas animation. attach it alongside `Sprite`:

```rust
fn setup(mut commands: Commands, mut assets: ResMut<AssetServer>) {
    let sheet = assets.load_texture("player_sheet.png");

    commands.spawn((
        Transform::from_xy(100.0, 100.0),
        Sprite::new(sheet),
        SpriteAnimation::looping(8, 12.0), // 8 frames at 12 fps
    ));
}
```

`Plugin2d` advances all `SpriteAnimation` components automatically each tick and
writes the correct `source_rect` into the paired `Sprite`. the sheet is assumed
to be a single horizontal strip of `frame_count` equal-width frames; each frame
is `1 / frame_count` of the texture's width.

constructors: `SpriteAnimation::looping(frame_count, fps)` and
`SpriteAnimation::one_shot(frame_count, fps)` (stops on the last frame).

`SpriteAnimation` fields:
- `frame_count: u32`: total number of frames in the strip
- `fps: f32`: playback speed in frames per second
- `looping: bool`: restart from frame 0 when the last frame is reached
- `playing: bool`: set false to pause on the current frame
- `current_frame: u32`: current frame index (writable to jump to a frame)
- `timer: f32`: time accumulated since the last frame advance (writable to reset)

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
