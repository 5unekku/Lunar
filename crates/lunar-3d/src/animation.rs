use std::collections::HashMap;
use std::sync::Arc;

use bevy_ecs::prelude::*;
use lunar_core::Time;
use lunar_math::{Quat, Vec3};

use crate::transform::LocalTransform3d;

/// a single keyframe value at a given time.
#[derive(Debug, Clone, Copy)]
pub struct Keyframe<T: Copy> {
    pub time: f32,
    pub value: T,
}

/// animation track for one named joint.
///
/// stores translation, rotation, and scale keyframes separately. any channel
/// may be empty — if a channel has no keyframes, that component of the transform
/// is left unchanged.
///
/// keyframes must be sorted by ascending time before use.
#[derive(Debug, Clone)]
pub struct JointTrack {
    /// name matched against [`AnimationTarget::joint_name`] at bind time.
    pub joint_name: String,
    pub translations: Vec<Keyframe<Vec3>>,
    pub rotations: Vec<Keyframe<Quat>>,
    pub scales: Vec<Keyframe<Vec3>>,
}

impl JointTrack {
    pub fn sample_translation(&self, time: f32) -> Option<Vec3> {
        sample_vec3(&self.translations, time)
    }

    pub fn sample_rotation(&self, time: f32) -> Option<Quat> {
        sample_quat(&self.rotations, time)
    }

    pub fn sample_scale(&self, time: f32) -> Option<Vec3> {
        sample_vec3(&self.scales, time)
    }
}

fn sample_vec3(keyframes: &[Keyframe<Vec3>], time: f32) -> Option<Vec3> {
    if keyframes.is_empty() {
        return None;
    }
    if keyframes.len() == 1 {
        return Some(keyframes[0].value);
    }
    if time <= keyframes[0].time {
        return Some(keyframes[0].value);
    }
    let last = keyframes.last().unwrap();
    if time >= last.time {
        return Some(last.value);
    }
    let index = keyframes.partition_point(|k| k.time <= time);
    let before = &keyframes[index - 1];
    let after = &keyframes[index];
    let t = (time - before.time) / (after.time - before.time);
    Some(before.value.lerp(after.value, t))
}

fn sample_quat(keyframes: &[Keyframe<Quat>], time: f32) -> Option<Quat> {
    if keyframes.is_empty() {
        return None;
    }
    if keyframes.len() == 1 {
        return Some(keyframes[0].value);
    }
    if time <= keyframes[0].time {
        return Some(keyframes[0].value);
    }
    let last = keyframes.last().unwrap();
    if time >= last.time {
        return Some(last.value);
    }
    let index = keyframes.partition_point(|k| k.time <= time);
    let before = &keyframes[index - 1];
    let after = &keyframes[index];
    let t = (time - before.time) / (after.time - before.time);
    Some(before.value.slerp(after.value, t))
}

/// collection of joint tracks with a total duration.
///
/// wrap in an `Arc` and share across `AnimationPlayer` components — the clip
/// data is read-only at runtime. build clips at startup and hand them to players.
///
/// # example
///
/// ```ignore
/// let clip = Arc::new(AnimationClip::new(vec![
///     JointTrack { joint_name: "spine".to_string(), translations: vec![], rotations: walk_keys, scales: vec![] },
/// ], 1.2));
/// commands.spawn((mesh_entity, AnimationPlayer::new(clip)));
/// ```
#[derive(Debug)]
pub struct AnimationClip {
    pub tracks: Vec<JointTrack>,
    /// total clip length in seconds.
    pub duration: f32,
}

impl AnimationClip {
    #[must_use]
    pub fn new(tracks: Vec<JointTrack>, duration: f32) -> Self {
        Self { tracks, duration }
    }
}

/// links a joint entity to its animation player and the joint name to bind against.
///
/// place on each joint entity (child of the animated mesh) alongside `LocalTransform3d`.
/// the advance_animations system writes to `LocalTransform3d` based on the clip track
/// whose `joint_name` matches this component.
#[derive(Debug, Clone, Component)]
pub struct AnimationTarget {
    /// the entity that holds the `AnimationPlayer` driving this joint.
    pub player: Entity,
    /// matched against `JointTrack::joint_name` in the clip.
    pub joint_name: String,
}

/// playback state for a skeletal animation. attach to the root entity of a skeleton.
///
/// the clip is shared via `Arc` — multiple players can reference the same clip
/// at no extra cost.
///
/// # example
///
/// ```ignore
/// commands.spawn((
///     LocalTransform3d::from_xyz(0.0, 0.0, 0.0),
///     WorldTransform3d::default(),
///     Mesh3d(mesh_handle),
///     AnimationPlayer::new(clip.clone()),
/// ));
/// ```
#[derive(Debug, Component)]
pub struct AnimationPlayer {
    pub clip: Arc<AnimationClip>,
    /// current playhead position in seconds.
    pub time: f32,
    /// playback speed multiplier. negative values play in reverse.
    pub speed: f32,
    /// restart from the beginning when the clip ends.
    pub looping: bool,
    /// whether the animation is advancing.
    pub playing: bool,
}

impl AnimationPlayer {
    #[must_use]
    pub fn new(clip: Arc<AnimationClip>) -> Self {
        Self {
            clip,
            time: 0.0,
            speed: 1.0,
            looping: true,
            playing: true,
        }
    }

    #[must_use]
    pub fn with_speed(mut self, speed: f32) -> Self {
        self.speed = speed;
        self
    }

    #[must_use]
    pub fn one_shot(mut self) -> Self {
        self.looping = false;
        self
    }

    /// jump to a specific time and resume playing.
    pub fn seek(&mut self, time: f32) {
        self.time = time.clamp(0.0, self.clip.duration);
    }

    /// restart from frame 0 and resume playing.
    pub fn restart(&mut self) {
        self.time = 0.0;
        self.playing = true;
    }
}

/// advance all animation players by delta time, then write sampled transforms to joint entities.
///
/// register in the Update stage after input, before transform propagation, so joints have
/// correct local transforms when the propagation pass runs.
pub fn advance_animations(
    time: Res<Time>,
    mut players: Query<(Entity, &mut AnimationPlayer)>,
    mut targets: Query<(&AnimationTarget, &mut LocalTransform3d)>,
) {
    let mut player_states: HashMap<Entity, (Arc<AnimationClip>, f32)> = HashMap::new();

    for (entity, mut player) in &mut players {
        if !player.playing {
            continue;
        }
        player.time += time.delta_seconds() * player.speed;
        let duration = player.clip.duration.max(f32::EPSILON);
        if player.looping {
            if player.speed >= 0.0 {
                player.time %= duration;
            } else {
                player.time = player.time.rem_euclid(duration);
            }
        } else {
            player.time = player.time.clamp(0.0, duration);
            if player.time >= duration {
                player.playing = false;
            }
        }
        player_states.insert(entity, (player.clip.clone(), player.time));
    }

    for (target, mut transform) in &mut targets {
        let Some((clip, time)) = player_states.get(&target.player) else {
            continue;
        };
        let Some(track) = clip
            .tracks
            .iter()
            .find(|track| track.joint_name == target.joint_name)
        else {
            continue;
        };

        if let Some(translation) = track.sample_translation(*time) {
            transform.translation = translation;
        }
        if let Some(rotation) = track.sample_rotation(*time) {
            transform.rotation = rotation;
        }
        if let Some(scale) = track.sample_scale(*time) {
            transform.scale = scale;
        }
    }
}
