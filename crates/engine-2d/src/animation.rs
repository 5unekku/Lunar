use bevy_ecs::prelude::Component;
use engine_core::Time;
use engine_math::Vec2;

/// drives frame-by-frame animation for a horizontal sprite sheet.
///
/// the sheet must be a single row of equal-width frames. UV coordinates
/// are computed automatically from the frame index and count.
///
/// the texture handle is stored separately (as a component or resource) because
/// engine-2d does not depend on engine-assets. pair this with your texture handle:
///
/// ```ignore
/// commands.spawn((
///     Transform::from_xy(x, y),
///     SpriteAnimation::looping(4, 8.0),
///     MyTexture(walk_sheet),
/// ));
/// ```
///
/// then in your render system:
/// ```ignore
/// let (uv_min, uv_max) = anim.uv_rect();
/// queue.draw_sprite_atlas_on_layer(&tex, pos, frame_size, (uv_min, uv_max), layer);
/// ```
#[derive(Debug, Clone, Component)]
pub struct SpriteAnimation {
    /// total number of frames in the sheet row.
    pub frame_count: u32,
    /// frames per second.
    pub fps: f32,
    /// current frame index (0 .. frame_count).
    pub current_frame: u32,
    /// accumulated time since last frame advance.
    pub timer: f32,
    /// whether the animation is currently advancing.
    pub playing: bool,
    /// restart from frame 0 when the last frame is reached (false = stop on last frame).
    pub looping: bool,
}

impl SpriteAnimation {
    /// create a new looping animation.
    #[must_use]
    pub fn looping(frame_count: u32, fps: f32) -> Self {
        Self {
            frame_count,
            fps,
            current_frame: 0,
            timer: 0.0,
            playing: true,
            looping: true,
        }
    }

    /// create a one-shot animation that stops on the last frame.
    #[must_use]
    pub fn one_shot(frame_count: u32, fps: f32) -> Self {
        Self {
            looping: false,
            ..Self::looping(frame_count, fps)
        }
    }

    /// UV region `(uv_min, uv_max)` for the current frame within a horizontal strip.
    ///
    /// assumes all frames are equal width and the strip contains exactly `frame_count` frames.
    /// pass the result directly to [`RenderQueue::draw_sprite_atlas_on_layer`].
    #[must_use]
    pub fn uv_rect(&self) -> (Vec2, Vec2) {
        if self.frame_count == 0 {
            return (Vec2::ZERO, Vec2::ONE);
        }
        let frame_w = 1.0 / self.frame_count as f32;
        let u0 = self.current_frame as f32 * frame_w;
        (Vec2::new(u0, 0.0), Vec2::new(u0 + frame_w, 1.0))
    }

    /// jump to a specific frame and reset the inter-frame timer.
    pub fn set_frame(&mut self, frame: u32) {
        self.current_frame = frame.min(self.frame_count.saturating_sub(1));
        self.timer = 0.0;
    }

    /// reset to frame 0 and resume playing.
    pub fn restart(&mut self) {
        self.current_frame = 0;
        self.timer = 0.0;
        self.playing = true;
    }
}

/// advance all [`SpriteAnimation`] components by `delta_seconds`.
///
/// registers via [`Plugin2d`] — no manual setup needed.
pub fn advance_sprite_animations(
    time: bevy_ecs::system::Res<Time>,
    mut query: bevy_ecs::system::Query<&mut SpriteAnimation>,
) {
    let dt = time.delta_seconds();
    for mut anim in &mut query {
        if !anim.playing || anim.frame_count == 0 || anim.fps <= 0.0 {
            continue;
        }
        anim.timer += dt;
        let frame_duration = 1.0 / anim.fps;
        while anim.timer >= frame_duration {
            anim.timer -= frame_duration;
            let next = anim.current_frame + 1;
            if next < anim.frame_count {
                anim.current_frame = next;
            } else if anim.looping {
                anim.current_frame = 0;
            } else {
                anim.playing = false;
                break;
            }
        }
    }
}
