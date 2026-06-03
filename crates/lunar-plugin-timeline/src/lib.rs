//! timed track sequencer for cutscenes.
//!
//! a [`Timeline`] holds named tracks of (timestamp, action) keyframes.
//! the plugin ticks all active timelines each frame, firing actions as the playhead advances.
//!
//! # usage
//!
//! ```ignore
//! use lunar_timeline::{Timeline, TimelineTrack, TimelineKey, TimelineAction};
//! use bevy_ecs::prelude::*;
//! use lunar_math::Vec2;
//!
//! let timeline = Timeline::new(vec![
//!     TimelineTrack::new(Entity::PLACEHOLDER, vec![
//!         TimelineKey { time: 0.5, action: TimelineAction::SetVisible(false) },
//!         TimelineKey { time: 2.0, action: TimelineAction::SetVisible(true) },
//!     ]),
//! ]);
//! ```

use bevy_ecs::prelude::*;
use lunar_math::Vec2;

/// a single action to fire at a point in time.
#[derive(Debug, Clone)]
pub enum TimelineAction {
	/// move an entity to the given world-space position.
	MoveTo(Vec2),
	/// snap an entity's position (no interpolation).
	TeleportTo(Vec2),
	/// set the entity's visibility flag in the ECS.
	SetVisible(bool),
	/// fire a custom event string — game code handles it via a listener system.
	FireEvent(String),
}

/// a timestamp-action pair on a track.
#[derive(Debug, Clone)]
pub struct TimelineKey {
	/// time in seconds from the start of the timeline.
	pub time: f32,
	/// action to fire when this key is reached.
	pub action: TimelineAction,
}

/// a sequence of actions targeting a single entity.
pub struct TimelineTrack {
	/// entity this track controls.
	pub target: Entity,
	/// keys sorted ascending by time.
	pub keys: Vec<TimelineKey>,
	/// index of the next key to fire (advances as the playhead moves forward).
	next_key: usize,
}

impl TimelineTrack {
	/// create a track; keys are sorted by time automatically.
	#[must_use]
	pub fn new(target: Entity, mut keys: Vec<TimelineKey>) -> Self {
		keys.sort_by(|a, b| a.time.total_cmp(&b.time));
		Self {
			target,
			keys,
			next_key: 0,
		}
	}

	fn reset(&mut self) {
		self.next_key = 0;
	}

	fn advance(&mut self, playhead: f32) -> impl Iterator<Item = (Entity, &TimelineAction)> {
		// find the contiguous run of keys at or before the playhead, advance the
		// cursor, then borrow that slice. computing the range up front means the
		// `&mut self` write to `next_key` is finished before we hand out `&self.keys`
		// references, so this needs no unsafe and allocates nothing.
		let start = self.next_key;
		let mut end = start;
		while end < self.keys.len() && self.keys[end].time <= playhead {
			end += 1;
		}
		self.next_key = end;
		let target = self.target;
		self.keys[start..end]
			.iter()
			.map(move |key| (target, &key.action))
	}
}

/// timeline state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimelineState {
	Stopped,
	Playing,
	Paused,
	Finished,
}

/// component — attach to an entity to drive a cutscene.
///
/// add [`TimelinePlugin`] to tick all active timelines automatically each frame.
#[derive(Component)]
pub struct Timeline {
	pub tracks: Vec<TimelineTrack>,
	/// current playhead position in seconds.
	pub playhead: f32,
	/// total duration (auto-computed as the max key time across all tracks).
	pub duration: f32,
	pub state: TimelineState,
	/// if true, loop back to start when finished.
	pub looping: bool,
	/// playback speed multiplier. default 1.0.
	pub speed: f32,
}

impl Timeline {
	/// create a timeline from the given tracks.
	#[must_use]
	pub fn new(tracks: Vec<TimelineTrack>) -> Self {
		let duration = tracks
			.iter()
			.flat_map(|t| t.keys.iter())
			.map(|k| k.time)
			.fold(0.0f32, f32::max);
		Self {
			tracks,
			playhead: 0.0,
			duration,
			state: TimelineState::Stopped,
			looping: false,
			speed: 1.0,
		}
	}

	/// start or resume playback.
	pub fn play(&mut self) {
		self.state = TimelineState::Playing;
	}

	/// pause playback without resetting the playhead.
	pub fn pause(&mut self) {
		if self.state == TimelineState::Playing {
			self.state = TimelineState::Paused;
		}
	}

	/// stop and reset to the beginning.
	pub fn stop(&mut self) {
		self.state = TimelineState::Stopped;
		self.playhead = 0.0;
		for track in &mut self.tracks {
			track.reset();
		}
	}
}

/// a fired event from a [`TimelineAction::FireEvent`] key.
#[derive(Debug, Clone)]
pub struct TimelineEvent {
	/// the timeline entity that fired the event.
	pub timeline_entity: Entity,
	/// target entity on the track.
	pub target: Entity,
	/// the event string from the key.
	pub name: String,
}

/// resource — collects [`TimelineEvent`]s fired this frame.
///
/// game code drains this each frame (via `events.take()`) to process custom timeline events.
#[derive(Resource, Default)]
pub struct TimelineEvents {
	pending: Vec<TimelineEvent>,
}

impl TimelineEvents {
	/// take all pending events, clearing the buffer.
	pub fn take(&mut self) -> Vec<TimelineEvent> {
		std::mem::take(&mut self.pending)
	}

	/// peek at pending events without consuming them.
	pub fn iter(&self) -> impl Iterator<Item = &TimelineEvent> {
		self.pending.iter()
	}
}

/// system — advance all playing [`Timeline`] components and apply their actions.
///
/// writes `LocalTransform.translation` for `MoveTo`/`TeleportTo`, inserts/removes
/// [`HiddenMarker`] for `SetVisible`, and pushes to [`TimelineEvents`] for `FireEvent`.
pub fn tick_timelines(
	time: Res<lunar_core::Time>,
	mut commands: Commands,
	mut timeline_events: ResMut<TimelineEvents>,
	mut timeline_query: Query<(Entity, &mut Timeline)>,
	mut transform_query: Query<&mut lunar_math::LocalTransform>,
) {
	let delta = time.delta_seconds();

	for (timeline_entity, mut timeline) in timeline_query.iter_mut() {
		if timeline.state != TimelineState::Playing {
			continue;
		}

		timeline.playhead += delta * timeline.speed;

		// collect (target, action) pairs without borrowing tracks through timeline
		let playhead = timeline.playhead;
		let mut pending: Vec<(Entity, TimelineAction)> = Vec::new();
		for track in &mut timeline.tracks {
			for (target, action) in track.advance(playhead) {
				pending.push((target, action.clone()));
			}
		}

		for (target, action) in pending {
			match action {
				TimelineAction::MoveTo(pos) | TimelineAction::TeleportTo(pos) => {
					if let Ok(mut transform) = transform_query.get_mut(target) {
						transform.translation = pos;
					}
				}
				TimelineAction::SetVisible(visible) => {
					if visible {
						commands.entity(target).remove::<HiddenMarker>();
					} else {
						commands.entity(target).insert(HiddenMarker);
					}
				}
				TimelineAction::FireEvent(name) => {
					timeline_events.pending.push(TimelineEvent {
						timeline_entity,
						target,
						name,
					});
				}
			}
		}

		if timeline.looping && timeline.duration > 0.0 {
			while timeline.playhead >= timeline.duration {
				timeline.playhead -= timeline.duration;
				for track in &mut timeline.tracks {
					track.reset();
				}
			}
		} else if timeline.playhead >= timeline.duration {
			timeline.state = TimelineState::Finished;
		}
	}
}

/// marker component inserted by `SetVisible(false)`, removed by `SetVisible(true)`.
///
/// game rendering can filter on this to skip draw calls.
#[derive(Component)]
pub struct HiddenMarker;

#[cfg(test)]
mod tests {
	use super::*;

	fn make_world() -> World {
		let mut world = World::new();
		world.insert_resource(lunar_core::Time::default());
		world.insert_resource(TimelineEvents::default());
		world
	}

	fn set_delta(world: &mut World, delta: f32) {
		world
			.resource_mut::<lunar_core::Time>()
			.set_delta_seconds(delta);
	}

	fn tick(world: &mut World) {
		let mut system = bevy_ecs::system::IntoSystem::into_system(tick_timelines);
		system.initialize(world);
		let _ = system.run((), world);
	}

	#[test]
	fn stopped_timeline_does_not_advance() {
		let mut world = make_world();
		let entity = world.spawn(Timeline::new(vec![])).id();
		set_delta(&mut world, 1.0);
		tick(&mut world);
		assert_eq!(world.get::<Timeline>(entity).unwrap().playhead, 0.0);
	}

	#[test]
	fn playing_timeline_advances_playhead() {
		let mut world = make_world();
		let mut timeline = Timeline::new(vec![]);
		timeline.play();
		let entity = world.spawn(timeline).id();
		set_delta(&mut world, 0.5);
		tick(&mut world);
		let t = world.get::<Timeline>(entity).unwrap().playhead;
		assert!((t - 0.5).abs() < 1e-5);
	}

	#[test]
	fn move_to_action_updates_transform() {
		let mut world = make_world();
		let target = world.spawn(lunar_math::LocalTransform::default()).id();

		let mut timeline = Timeline::new(vec![TimelineTrack::new(
			target,
			vec![TimelineKey {
				time: 0.1,
				action: TimelineAction::MoveTo(Vec2::new(3.0, 5.0)),
			}],
		)]);
		timeline.play();
		world.spawn(timeline);

		set_delta(&mut world, 0.2);
		tick(&mut world);

		let pos = world
			.get::<lunar_math::LocalTransform>(target)
			.unwrap()
			.translation;
		assert!((pos.x - 3.0).abs() < 1e-5);
		assert!((pos.y - 5.0).abs() < 1e-5);
	}

	#[test]
	fn set_visible_false_inserts_hidden_marker() {
		let mut world = make_world();
		let target = world.spawn_empty().id();

		let mut timeline = Timeline::new(vec![TimelineTrack::new(
			target,
			vec![TimelineKey {
				time: 0.1,
				action: TimelineAction::SetVisible(false),
			}],
		)]);
		timeline.play();
		world.spawn(timeline);

		set_delta(&mut world, 0.2);
		tick(&mut world);
		world.flush();

		assert!(world.get::<HiddenMarker>(target).is_some());
	}

	#[test]
	fn timeline_finishes_when_duration_reached() {
		let mut world = make_world();
		let target = world.spawn(lunar_math::LocalTransform::default()).id();
		let mut timeline = Timeline::new(vec![TimelineTrack::new(
			target,
			vec![TimelineKey {
				time: 0.5,
				action: TimelineAction::MoveTo(Vec2::ZERO),
			}],
		)]);
		timeline.play();
		let entity = world.spawn(timeline).id();

		set_delta(&mut world, 1.0);
		tick(&mut world);
		assert_eq!(
			world.get::<Timeline>(entity).unwrap().state,
			TimelineState::Finished
		);
	}

	#[test]
	fn looping_timeline_wraps_playhead() {
		let mut world = make_world();
		let target = world.spawn(lunar_math::LocalTransform::default()).id();
		let mut timeline = Timeline::new(vec![TimelineTrack::new(
			target,
			vec![TimelineKey {
				time: 0.5,
				action: TimelineAction::MoveTo(Vec2::ZERO),
			}],
		)]);
		timeline.looping = true;
		timeline.play();
		let entity = world.spawn(timeline).id();

		set_delta(&mut world, 0.8);
		tick(&mut world);
		let t = world.get::<Timeline>(entity).unwrap().playhead;
		// should have wrapped: 0.8 - 0.5 = 0.3
		assert!((t - 0.3).abs() < 1e-4);
		assert_eq!(
			world.get::<Timeline>(entity).unwrap().state,
			TimelineState::Playing
		);
	}
}

/// drop-in plugin: registers the timeline tick system and inserts [`TimelineEvents`].
/// drain `TimelineEvents` each frame to handle custom timeline events.
#[derive(Default)]
pub struct TimelinePlugin;

impl lunar_core::GamePlugin for TimelinePlugin {
	fn name(&self) -> &str {
		"TimelinePlugin"
	}
	fn build(&mut self, app: &mut lunar_core::App) {
		app.insert_resource(TimelineEvents::default());
		app.add_system_to_stage(lunar_core::UpdateStage::Update, tick_timelines);
	}
}

/// common, game-facing timeline types for `use lunar::prelude::*`.
pub mod prelude {
	pub use crate::{
		Timeline, TimelineAction, TimelineEvent, TimelineEvents, TimelineKey, TimelinePlugin,
		TimelineState, TimelineTrack,
	};
}
