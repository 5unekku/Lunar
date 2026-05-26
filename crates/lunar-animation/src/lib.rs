//! frame-by-frame sprite animation.
//!
//! drives the [`lunar_render::Sprite`] component's `source_rect` from a named
//! clip + frame sequence. no skeletal animation — that is post-v1.
//!
//! # usage
//!
//! ```ignore
//! use lunar_animation::{AnimationClip, AnimationFrame, Animator, AnimationPlugin};
//!
//! // define clips at startup
//! fn setup(mut commands: Commands, asset_server: ResMut<AssetServer>) {
//!     let texture = asset_server.load_texture("player.png");
//!     let idle = AnimationClip::new(vec![
//!         AnimationFrame::new(Vec2::new(0.0, 0.0), Vec2::new(16.0, 16.0), 0.15),
//!         AnimationFrame::new(Vec2::new(16.0, 0.0), Vec2::new(16.0, 16.0), 0.15),
//!     ]).looping();
//!     let mut animator = Animator::new();
//!     animator.add_clip("idle", idle);
//!     animator.play("idle");
//!     commands.spawn((
//!         Transform::from_xy(0.0, 0.0),
//!         Sprite::new(texture),
//!         animator,
//!     ));
//! }
//! ```

use std::collections::HashMap;

use bevy_ecs::message::{MessageRegistry, MessageWriter};
use bevy_ecs::prelude::*;
use lunar_core::{App, GamePlugin, Time};
use lunar_math::Vec2;
use lunar_render::Sprite;

/// a single frame in an animation clip.
///
/// `source_pos` and `source_size` are pixel coordinates into the spritesheet.
#[derive(Debug, Clone)]
pub struct AnimationFrame {
    /// top-left pixel position on the spritesheet.
    pub source_pos: Vec2,
    /// pixel size of the frame on the spritesheet.
    pub source_size: Vec2,
    /// how long this frame is shown, in seconds.
    pub duration_secs: f32,
}

impl AnimationFrame {
    #[must_use]
    pub fn new(source_pos: Vec2, source_size: Vec2, duration_secs: f32) -> Self {
        Self {
            source_pos,
            source_size,
            duration_secs,
        }
    }
}

/// a named sequence of frames, optionally looping.
#[derive(Debug, Clone)]
pub struct AnimationClip {
    pub frames: Vec<AnimationFrame>,
    pub looping: bool,
}

impl AnimationClip {
    #[must_use]
    pub fn new(frames: Vec<AnimationFrame>) -> Self {
        Self {
            frames,
            looping: false,
        }
    }

    /// mark this clip as looping (builder pattern).
    #[must_use]
    pub fn looping(mut self) -> Self {
        self.looping = true;
        self
    }

    fn total_duration(&self) -> f32 {
        self.frames.iter().map(|f| f.duration_secs).sum()
    }
}

/// fired when a non-looping animation clip plays through to its last frame.
#[derive(Debug, Clone)]
pub struct AnimationFinished {
    pub entity: Entity,
    pub clip_name: String,
}

impl bevy_ecs::message::Message for AnimationFinished {}

/// component that drives frame-by-frame animation on a [`Sprite`].
///
/// attach to any entity that also has a `Sprite` component.
/// call [`Animator::play`] to switch clips. the `advance_animations` system
/// writes `Sprite::source_rect` each frame based on the current clip and elapsed time.
#[derive(Debug, Clone, Component)]
pub struct Animator {
    clips: HashMap<String, AnimationClip>,
    current_clip: Option<String>,
    elapsed: f32,
    frame_index: usize,
    pub playing: bool,
    /// set by the system when a non-looping clip reaches its last frame.
    pub finished: bool,
}

impl Animator {
    #[must_use]
    pub fn new() -> Self {
        Self {
            clips: HashMap::new(),
            current_clip: None,
            elapsed: 0.0,
            frame_index: 0,
            playing: false,
            finished: false,
        }
    }

    /// register a named clip.
    pub fn add_clip(&mut self, name: impl Into<String>, clip: AnimationClip) {
        self.clips.insert(name.into(), clip);
    }

    /// switch to a named clip and reset playback to the first frame.
    /// does nothing if the clip name is unknown.
    pub fn play(&mut self, name: impl Into<String>) {
        let name = name.into();
        if self.clips.contains_key(&name) {
            self.current_clip = Some(name);
            self.elapsed = 0.0;
            self.frame_index = 0;
            self.playing = true;
            self.finished = false;
        }
    }

    /// pause on the current frame.
    pub fn stop(&mut self) {
        self.playing = false;
    }

    /// resume from where it was paused.
    pub fn resume(&mut self) {
        self.playing = true;
    }

    /// current frame index within the active clip.
    #[must_use]
    pub fn frame_index(&self) -> usize {
        self.frame_index
    }

    /// name of the currently active clip, if any.
    #[must_use]
    pub fn current_clip(&self) -> Option<&str> {
        self.current_clip.as_deref()
    }
}

impl Default for Animator {
    fn default() -> Self {
        Self::new()
    }
}

/// system that advances animators and writes `Sprite::source_rect`.
///
/// registered in the Update stage by [`AnimationPlugin`].
pub fn advance_animations(
    time: Res<Time>,
    mut finished_writer: MessageWriter<AnimationFinished>,
    mut query: Query<(Entity, &mut Animator, &mut Sprite)>,
) {
    let delta = time.delta_seconds();
    for (entity, mut animator, mut sprite) in &mut query {
        if !animator.playing {
            continue;
        }

        // extract what we need as copies so no String/Vec is cloned.
        // the borrow ends at the `;` — after that we can mutate `animator` freely.
        let Some((frame_count, looping, total_duration)) = animator
            .current_clip
            .as_deref()
            .and_then(|name| animator.clips.get(name))
            .map(|clip| (clip.frames.len(), clip.looping, clip.total_duration()))
        else {
            continue;
        };

        if frame_count == 0 {
            continue;
        }

        animator.elapsed += delta;

        // re-borrow clip after mutation — no conflict because the earlier borrow ended
        let clip_name = animator.current_clip.as_deref().unwrap();
        let clip = animator.clips.get(clip_name).unwrap();

        let mut accumulated = 0.0_f32;
        let mut new_index = 0;
        let mut finished = false;

        if looping {
            let looped_elapsed = if total_duration > 0.0 {
                animator.elapsed % total_duration
            } else {
                0.0
            };
            for (index, frame) in clip.frames.iter().enumerate() {
                accumulated += frame.duration_secs;
                if looped_elapsed < accumulated {
                    new_index = index;
                    break;
                }
                new_index = index;
            }
        } else {
            let mut past_end = true;
            for (index, frame) in clip.frames.iter().enumerate() {
                accumulated += frame.duration_secs;
                if animator.elapsed < accumulated {
                    new_index = index;
                    past_end = false;
                    break;
                }
                new_index = index;
            }
            if past_end {
                new_index = frame_count - 1;
                if !animator.finished {
                    finished = true;
                }
            }
        }

        // copy frame rect before borrow ends, then write back
        let frame_rect = (clip.frames[new_index].source_pos, clip.frames[new_index].source_size);
        // clip borrow ends here — animator mutation is now safe

        animator.frame_index = new_index;
        if finished {
            animator.finished = true;
            animator.playing = false;
        }
        sprite.source_rect = Some(frame_rect);

        if finished {
            // to_string only on clip completion (at most once per playthrough, not per frame)
            let name = animator.current_clip.as_deref().unwrap_or("").to_string();
            finished_writer.write(AnimationFinished { entity, clip_name: name });
        }
    }
}

/// plugin that registers the animation system.
///
/// add this alongside [`lunar_render::RenderPlugin`]. the `advance_animations`
/// system runs in Update so `Sprite::source_rect` is set before the render stage.
pub struct AnimationPlugin;

impl GamePlugin for AnimationPlugin {
    fn name(&self) -> &'static str {
        "animation"
    }

    fn build(&mut self, app: &mut App) {
        MessageRegistry::register_message::<AnimationFinished>(app.world_mut());
        app.add_system(advance_animations);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_clip() -> AnimationClip {
        AnimationClip::new(vec![
            AnimationFrame::new(Vec2::new(0.0, 0.0), Vec2::new(16.0, 16.0), 0.1),
            AnimationFrame::new(Vec2::new(16.0, 0.0), Vec2::new(16.0, 16.0), 0.1),
            AnimationFrame::new(Vec2::new(32.0, 0.0), Vec2::new(16.0, 16.0), 0.1),
        ])
    }

    #[test]
    fn animator_play_resets_state() {
        let mut animator = Animator::new();
        animator.add_clip("walk", make_test_clip());
        animator.play("walk");
        assert_eq!(animator.frame_index(), 0);
        assert!(animator.playing);
        assert!(!animator.finished);
        assert_eq!(animator.current_clip(), Some("walk"));
    }

    #[test]
    fn animator_unknown_clip_is_no_op() {
        let mut animator = Animator::new();
        animator.play("missing");
        assert!(!animator.playing);
        assert_eq!(animator.current_clip(), None);
    }

    #[test]
    fn looping_clip_wraps() {
        let mut clip = make_test_clip();
        clip.looping = true;
        let total = clip.total_duration();
        assert!((total - 0.3).abs() < 0.001);
    }
}
