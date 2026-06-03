use rustc_hash::FxHashMap as HashMap;
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
/// may be empty — if empty, that component of the transform is left unchanged.
///
/// keyframes must be sorted by ascending time before the clip is used.
#[derive(Debug, Clone)]
pub struct JointTrack {
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
	if keyframes.len() == 1 || time <= keyframes[0].time {
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
	if keyframes.len() == 1 || time <= keyframes[0].time {
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
/// tracks are keyed by joint name for O(1) lookup at sample time.
/// wrap in an `Arc` and share across `AnimationPlayer` components.
///
/// # example
///
/// ```ignore
/// let clip = Arc::new(AnimationClip::new(
///     HashMap::from([
///         ("spine".to_string(), JointTrack { translations: vec![], rotations: walk_keys, scales: vec![] }),
///     ]),
///     1.2,
/// ));
/// commands.spawn(AnimationPlayer::new(clip));
/// ```
#[derive(Debug)]
pub struct AnimationClip {
	/// tracks stored by index; resolve a joint name to its index once via
	/// [`Self::track_index`], then sample with [`Self::track`]. interning the
	/// name → index lookup keeps `advance_animations` off the per-frame string hash.
	tracks: Vec<JointTrack>,
	/// joint name → index into `tracks`.
	names: HashMap<String, usize>,
	/// total clip length in seconds.
	pub duration: f32,
}

impl AnimationClip {
	#[must_use]
	pub fn new(tracks: HashMap<String, JointTrack>, duration: f32) -> Self {
		let mut names = HashMap::default();
		let mut indexed = Vec::with_capacity(tracks.len());
		for (name, track) in tracks {
			names.insert(name, indexed.len());
			indexed.push(track);
		}
		Self {
			tracks: indexed,
			names,
			duration,
		}
	}

	/// convenience: build from an iterator of `(name, track)` pairs.
	#[must_use]
	pub fn from_tracks(
		tracks: impl IntoIterator<Item = (impl Into<String>, JointTrack)>,
		duration: f32,
	) -> Self {
		let mut names = HashMap::default();
		let mut indexed = Vec::new();
		for (name, track) in tracks {
			names.insert(name.into(), indexed.len());
			indexed.push(track);
		}
		Self {
			tracks: indexed,
			names,
			duration,
		}
	}

	/// resolve a joint name to its stable track index, or `None` if absent.
	/// callers cache this once and sample via [`Self::track`] thereafter.
	#[must_use]
	pub fn track_index(&self, joint_name: &str) -> Option<usize> {
		self.names.get(joint_name).copied()
	}

	/// fetch a track by the index returned from [`Self::track_index`].
	#[must_use]
	pub fn track(&self, index: usize) -> Option<&JointTrack> {
		self.tracks.get(index)
	}
}

/// links a joint entity to its animation player and joint name.
///
/// place on each joint entity (child of the animated mesh) alongside `LocalTransform3d`.
/// the advance_animations system writes to `LocalTransform3d` based on the matching
/// track in the clip.
#[derive(Debug, Clone, Component)]
pub struct AnimationTarget {
	/// the entity that holds the `AnimationPlayer` driving this joint.
	pub player: Entity,
	/// matched against a joint name in the clip; resolved to a track index once and cached.
	pub joint_name: String,
	/// cached `(clip pointer, resolved track index)`. `usize::MAX` index = no matching track.
	/// re-resolved only when the player swaps to a different clip. interior to the type so the
	/// per-frame sampler skips the name hash. construct via [`Self::new`].
	cache: Option<(usize, usize)>,
}

impl AnimationTarget {
	/// link a joint entity to the `player` entity driving it and the `joint_name` to follow.
	#[must_use]
	pub fn new(player: Entity, joint_name: impl Into<String>) -> Self {
		Self {
			player,
			joint_name: joint_name.into(),
			cache: None,
		}
	}
}

/// playback state for a skeletal animation. attach to the root entity of a skeleton.
///
/// the clip is shared via `Arc` — multiple players can reference the same clip at no
/// extra memory cost.
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

	/// jump to a specific time.
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
/// scratch is a sorted `Vec` (by entity) reused each frame — O(N log N) sort + O(log N) binary
/// search per target. better cache behavior than a HashMap for typical animation counts.
pub fn advance_animations(
	time: Res<Time>,
	mut players: Query<(Entity, &mut AnimationPlayer)>,
	mut targets: Query<(&mut AnimationTarget, &mut LocalTransform3d)>,
	mut scratch: Local<Vec<(Entity, Arc<AnimationClip>, f32)>>,
) {
	scratch.clear();

	for (entity, mut player) in &mut players {
		if !player.playing {
			continue;
		}
		player.time += time.delta_seconds() * player.speed;
		let duration = player.clip.duration.max(f32::EPSILON);
		if player.looping {
			player.time = player.time.rem_euclid(duration);
		} else {
			player.time = player.time.clamp(0.0, duration);
			if player.time >= duration {
				player.playing = false;
			}
		}
		scratch.push((entity, Arc::clone(&player.clip), player.time));
	}

	scratch.sort_unstable_by_key(|&(entity, _, _)| entity);

	for (mut target, mut transform) in &mut targets {
		let Ok(idx) = scratch.binary_search_by_key(&target.player, |&(entity, _, _)| entity) else {
			continue;
		};
		let (_, clip, time) = &scratch[idx];
		let time = *time;

		// resolve joint_name → track index once per (target, clip); the hashed name lookup
		// only fires the first time a target is sampled, or when the player swaps clips.
		let clip_ptr = Arc::as_ptr(clip) as usize;
		let track_index = match target.cache {
			Some((cached_ptr, track_index)) if cached_ptr == clip_ptr => track_index,
			_ => {
				let resolved = clip.track_index(&target.joint_name).unwrap_or(usize::MAX);
				target.cache = Some((clip_ptr, resolved));
				resolved
			}
		};
		let Some(track) = clip.track(track_index) else {
			continue;
		};

		if let Some(translation) = track.sample_translation(time) {
			transform.translation = translation;
		}
		if let Some(rotation) = track.sample_rotation(time) {
			transform.rotation = rotation;
		}
		if let Some(scale) = track.sample_scale(time) {
			transform.scale = scale;
		}
	}
}
