//! app builder and time resource
//!
//! the app builder provides a fluent interface for configuring the engine.
//! game plugins register their systems, resources, and sub-plugins through the app.

use bevy_ecs::prelude::*;
use bevy_ecs::schedule::IntoScheduleConfigs;
use bevy_ecs::system::ScheduleSystem;

use crate::engine::Engine;
use crate::game_loop::{GameLoop, TickRate};
use crate::schedule::UpdateStage;
use crate::state::EngineState;

/// runtime-switchable logic tick rate.
///
/// write `rate` to change the tick rate at any time (e.g. from a settings menu).
/// the game loop detects the change each frame and calls `GameLoop::set_tick_rate`.
#[derive(Resource, Clone, Copy, PartialEq, Eq)]
pub struct TickRateConfig {
	/// the active logic tick rate. write this from game code to change tick rate at runtime.
	pub rate: TickRate,
}

/// timing parameters for the game loop, passed to [`App::run`].
///
/// the one typed representation of loop timing — render-side configs
/// (`RenderConfig`, `RenderConfig3d`) expose a `loop_config()` that produces this,
/// so authoring stays in one place and `run` takes a single self-documenting value.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct LoopConfig {
	/// render frame cap in fps. `0` = uncapped (vsync-limited).
	pub frame_cap: u32,
	/// fixed logic tick rate, independent of the render frame rate.
	pub tick_rate: TickRate,
}

impl Default for LoopConfig {
	fn default() -> Self {
		Self {
			frame_cap: 0,
			tick_rate: TickRate::Hz60,
		}
	}
}

/// time resource updated each frame
///
/// provides delta time for framerate-independent movement and elapsed time.
#[derive(Resource)]
pub struct Time {
	/// fixed logic delta in seconds (scaled by time_scale).
	/// always exactly 1/tick_hz — use this for all game logic and physics.
	delta_seconds: f32,
	/// fixed logic delta in seconds (unscaled).
	raw_delta_seconds: f32,
	/// wall-clock seconds since the last render frame (unscaled).
	/// use this for animation blending and rendering interpolation only.
	real_delta_seconds: f32,
	/// total simulated time in seconds (sum of fixed deltas, scaled)
	elapsed_seconds: f32,
	/// time multiplier (1.0 = normal, 0.5 = half speed, 2.0 = double speed)
	scale: f32,
	/// total logic tick count since engine start
	frame_count: u64,
	/// render interpolation alpha: how far we are between the last tick and the next.
	/// 0.0 = just ticked, 1.0 = about to tick. use for lerping render-side transforms.
	interp_alpha: f32,
}

impl Time {
	/// create a new time resource
	#[must_use]
	pub fn new() -> Self {
		Self {
			delta_seconds: 0.0,
			raw_delta_seconds: 0.0,
			real_delta_seconds: 0.0,
			elapsed_seconds: 0.0,
			scale: 1.0,
			frame_count: 0,
			interp_alpha: 0.0,
		}
	}

	/// get delta time in seconds (scaled)
	#[must_use]
	pub const fn delta_seconds(&self) -> f32 {
		self.delta_seconds
	}

	/// get raw delta time in seconds (unscaled)
	/// unscaled fixed tick delta — same value as `delta_seconds / time_scale`
	#[must_use]
	pub const fn raw_delta_seconds(&self) -> f32 {
		self.raw_delta_seconds
	}

	/// wall-clock seconds since the last render frame.
	/// use only for rendering/animation interpolation — NOT for game logic.
	#[must_use]
	pub const fn real_delta_seconds(&self) -> f32 {
		self.real_delta_seconds
	}

	/// total simulated time in seconds (sum of fixed deltas, scaled)
	#[must_use]
	pub const fn elapsed_seconds(&self) -> f32 {
		self.elapsed_seconds
	}

	/// current time scale multiplier
	#[must_use]
	pub const fn time_scale(&self) -> f32 {
		self.scale
	}

	/// set the time scale multiplier (0.0+ range; 0 = frozen)
	pub fn set_time_scale(&mut self, scale: f32) {
		self.scale = scale.max(0.0);
	}

	/// total logic tick count since engine start
	#[must_use]
	pub const fn frame_count(&self) -> u64 {
		self.frame_count
	}

	/// set delta directly — for unit tests only
	pub fn set_delta_seconds(&mut self, delta: f32) {
		self.delta_seconds = delta;
		self.raw_delta_seconds = delta;
	}

	/// advance by one logic tick using the fixed delta from the tick rate.
	///
	/// `fixed_delta` must be `tick_rate.delta_seconds()`. never pass wall-clock
	/// time here — the whole point is that this is always exactly 1/tick_hz.
	pub fn advance(&mut self, fixed_delta: f32) {
		self.raw_delta_seconds = fixed_delta;
		self.delta_seconds = fixed_delta * self.scale;
		self.elapsed_seconds += self.delta_seconds;
		self.frame_count += 1;
	}

	/// update the wall-clock render delta — called once per render frame, not per tick.
	pub fn set_real_delta(&mut self, real_delta: f32) {
		self.real_delta_seconds = real_delta;
	}

	/// render interpolation alpha: 0.0 = just ticked, 1.0 = about to tick.
	/// use this to lerp entity transforms on the render side for smooth motion.
	#[must_use]
	pub const fn interp_alpha(&self) -> f32 {
		self.interp_alpha
	}

	/// set the interpolation alpha — called by the game loop once per render frame.
	pub fn set_interp_alpha(&mut self, alpha: f32) {
		self.interp_alpha = alpha;
	}
}

impl Default for Time {
	fn default() -> Self {
		Self::new()
	}
}

/// app builder for configuring the engine
///
/// use the app to register systems, resources, and plugins before calling `run()`.
pub struct App {
	/// the engine instance
	engine: Engine,
	/// plugins registered but not yet built
	pending_plugins: Vec<Box<dyn GamePlugin>>,
	/// names of plugins already built (for cycle detection)
	built_plugins: Vec<String>,
	/// whether startup systems have been run
	startup_run: bool,
	/// fixed-timestep accumulator for [`App::pump_frame`] (external pacing loops)
	pump_accumulator: f32,
}

impl App {
	/// create a new app with default setup
	#[must_use]
	pub fn new() -> Self {
		let mut engine = Engine::new();
		// insert the time resource
		engine.world_mut().insert_resource(Time::new());
		// insert the engine state resource
		engine.world_mut().insert_resource(EngineState::Running);
		Self {
			engine,
			pending_plugins: Vec::new(),
			built_plugins: Vec::new(),
			startup_run: false,
			pump_accumulator: 0.0,
		}
	}

	/// get mutable access to the world for direct manipulation
	pub const fn world_mut(&mut self) -> &mut World {
		self.engine.world_mut()
	}

	/// insert a resource into the world
	pub fn insert_resource<R: Resource>(&mut self, resource: R) -> &mut Self {
		self.engine.world_mut().insert_resource(resource);
		self
	}

	/// add one or more systems to the default Update stage.
	/// accepts a single system or a tuple — use `(a, b, c).chain()` to
	/// enforce ordering when systems share `ResMut` borrows.
	pub fn add_system<M>(
		&mut self,
		systems: impl IntoScheduleConfigs<ScheduleSystem, M>,
	) -> &mut Self {
		self.add_system_to_stage(UpdateStage::Update, systems)
	}

	/// add one or more systems to a specific update stage.
	/// systems are grouped by stage and run in order each frame:
	/// Input → Physics → Update → Render.
	/// pass a tuple with `.chain()` to enforce intra-stage ordering.
	pub fn add_system_to_stage<M>(
		&mut self,
		stage: UpdateStage,
		systems: impl IntoScheduleConfigs<ScheduleSystem, M>,
	) -> &mut Self {
		self.engine.stage_schedule_mut(stage).add_systems(systems);
		self
	}

	/// add systems to the default Update stage, enforcing sequential execution order.
	/// equivalent to `add_system((a, b, c).chain())` but without needing to import
	/// `IntoScheduleConfigs` in game code.
	pub fn add_ordered_systems<M>(
		&mut self,
		systems: impl IntoScheduleConfigs<ScheduleSystem, M>,
	) -> &mut Self {
		self.add_system(systems.chain())
	}

	/// add systems to a specific stage, enforcing sequential execution order.
	/// equivalent to `add_system_to_stage(stage, (a, b, c).chain())` but without
	/// needing to import `IntoScheduleConfigs` in game code.
	pub fn add_ordered_systems_to_stage<M>(
		&mut self,
		stage: UpdateStage,
		systems: impl IntoScheduleConfigs<ScheduleSystem, M>,
	) -> &mut Self {
		self.add_system_to_stage(stage, systems.chain())
	}

	/// add one or more startup systems that run once before the main loop
	pub fn add_startup_system<M>(
		&mut self,
		systems: impl IntoScheduleConfigs<ScheduleSystem, M>,
	) -> &mut Self {
		self.engine.startup_schedule_mut().add_systems(systems);
		self
	}

	/// add a plugin to the app
	/// plugins are built in dependency order using topological sort.
	/// each plugin's dependencies must be built before the plugin itself.
	pub fn add_plugin(&mut self, plugin: impl GamePlugin + 'static) -> &mut Self {
		self.pending_plugins.push(Box::new(plugin));
		self
	}

	/// build all pending plugins in dependency order
	fn build_plugins(&mut self) {
		// name-keyed topological build: each round drains every plugin whose
		// declared dependencies are already built and defers the rest. plugins
		// registered during build() are absorbed before the next round. the loop
		// ends once a full round builds nothing new — any leftovers have missing
		// or circular dependencies. `built` accumulates across calls.
		let mut built = std::mem::take(&mut self.built_plugins);
		let mut pending = std::mem::take(&mut self.pending_plugins);
		let mut ready: Vec<Box<dyn GamePlugin>> = Vec::new();

		loop {
			// absorb anything registered by a previous round's build() calls
			pending.append(&mut self.pending_plugins);
			if pending.is_empty() {
				break;
			}

			let mut progressed = false;
			for mut plugin in std::mem::take(&mut pending) {
				let deps_met = plugin
					.dependencies()
					.iter()
					.all(|dep| built.iter().any(|name| name.as_str() == *dep));
				if deps_met {
					plugin.build(self);
					built.push(plugin.name().to_string());
					ready.push(plugin);
					progressed = true;
				} else {
					pending.push(plugin);
				}
			}

			// nothing became buildable and nothing new was registered → stuck
			if !progressed && self.pending_plugins.is_empty() {
				break;
			}
		}

		// put back any plugins that couldn't be built (circular deps or missing deps)
		self.pending_plugins = pending;
		self.built_plugins = built;

		if !self.pending_plugins.is_empty() {
			log::warn!(
				"{} plugins could not be built (missing dependencies or circular deps)",
				self.pending_plugins.len()
			);
		}

		// second pass: finish all successfully built plugins
		for mut plugin in ready {
			plugin.finish(self);
		}
	}

	/// get a reference to the engine
	pub const fn engine(&self) -> &Engine {
		&self.engine
	}

	/// get mutable access to the engine
	pub const fn engine_mut(&mut self) -> &mut Engine {
		&mut self.engine
	}

	/// start the game loop with the given timing ([`LoopConfig`]).
	pub fn run(&mut self, config: LoopConfig) {
		self.run_with_events(config, |_| {});
	}

	/// start the game loop with per-frame event processing.
	///
	/// `time.delta_seconds()` inside systems is always exactly `1 / tick_hz`.
	/// `time.real_delta_seconds()` is wall-clock render frame time for interpolation.
	pub fn run_with_events<F>(&mut self, config: LoopConfig, mut process_events: F)
	where
		F: FnMut(&mut World),
	{
		self.build_plugins();
		if !self.startup_run {
			self.engine.run_startup();
			self.startup_run = true;
		}

		// insert TickRateConfig so game code can change tick rate at runtime
		self.engine.world_mut().insert_resource(TickRateConfig {
			rate: config.tick_rate,
		});

		let mut fixed_delta = config.tick_rate.delta_seconds();
		let mut game_loop = GameLoop::new(config.frame_cap, config.tick_rate);

		while game_loop.is_running() {
			// check if game code changed the tick rate via TickRateConfig
			if let Some(cfg) = self.engine.world().get_resource::<TickRateConfig>()
				&& cfg.rate != game_loop.tick_rate()
			{
				game_loop.set_tick_rate(cfg.rate);
				fixed_delta = cfg.rate.delta_seconds();
			}

			let (ticks, frame_delta) = game_loop.tick();
			let alpha = game_loop.interpolation_alpha();

			if let Some(mut time) = self.engine.world_mut().get_resource_mut::<Time>() {
				time.set_real_delta(frame_delta);
				time.set_interp_alpha(alpha);
			}

			// run 0-5 logic ticks for this frame (fixed timestep accumulator)
			for _ in 0..ticks {
				if let Some(mut time) = self.engine.world_mut().get_resource_mut::<Time>() {
					time.advance(fixed_delta);
				}
				self.engine.run_logic_tick();
			}
			// render + post-update always fire exactly once per display frame,
			// even when ticks == 0 (frame ran faster than the tick interval).
			// this decouples render rate from logic rate so uncapped framerates work.
			self.engine.run_render_and_post();

			if let Some(state) = self.engine.world().get_resource::<EngineState>()
				&& state.is_stopping()
			{
				break;
			}

			game_loop.apply_frame_cap();
			process_events(self.engine.world_mut());
		}
	}

	/// drive one render frame from an external pacing source (requestAnimationFrame
	/// on wasm, or any host loop that reports real elapsed time).
	///
	/// runs the same fixed-timestep accumulator as [`App::run`]: 0-5 logic ticks at
	/// the [`TickRateConfig`] interval (Hz60 when the resource is absent), then
	/// exactly one render. unlike [`App::tick`], game speed stays correct whatever
	/// the host frame rate — a 120hz display gets interpolated frames, not 2× speed.
	///
	/// `real_delta_seconds` is wall-clock time since the previous call, clamped to
	/// 0.25s so a suspended tab resumes smoothly instead of bursting ticks.
	pub fn pump_frame(&mut self, real_delta_seconds: f32) {
		if !self.pending_plugins.is_empty() {
			self.build_plugins();
		}
		if !self.startup_run {
			self.engine.run_startup();
			self.startup_run = true;
		}

		let tick_rate = self
			.engine
			.world()
			.get_resource::<TickRateConfig>()
			.map_or(TickRate::Hz60, |config| config.rate);
		let fixed_delta = tick_rate.delta_seconds();

		let real_delta = real_delta_seconds.clamp(0.0, 0.25);
		self.pump_accumulator += real_delta;
		let mut ticks = 0u32;
		while self.pump_accumulator >= fixed_delta && ticks < 5 {
			self.pump_accumulator -= fixed_delta;
			ticks += 1;
		}
		// time beyond the 5-tick cap is dropped, matching GameLoop's spiral guard
		if self.pump_accumulator >= fixed_delta {
			self.pump_accumulator %= fixed_delta;
		}
		let alpha = (self.pump_accumulator / fixed_delta).clamp(0.0, 1.0);

		if let Some(mut time) = self.engine.world_mut().get_resource_mut::<Time>() {
			time.set_real_delta(real_delta);
			time.set_interp_alpha(alpha);
		}
		for _ in 0..ticks {
			if let Some(mut time) = self.engine.world_mut().get_resource_mut::<Time>() {
				time.advance(fixed_delta);
			}
			self.engine.run_logic_tick();
		}
		self.engine.run_render_and_post();
	}

	/// run a single frame tick (for external loops like requestAnimationFrame).
	/// `fixed_delta` should be `tick_rate.delta_seconds()` for your chosen rate.
	pub fn tick(&mut self, fixed_delta: f32) {
		if !self.pending_plugins.is_empty() {
			self.build_plugins();
		}
		if !self.startup_run {
			self.engine.run_startup();
			self.startup_run = true;
		}
		if let Some(mut time) = self.engine.world_mut().get_resource_mut::<Time>() {
			time.advance(fixed_delta);
		}
		self.engine.run_stages();
	}
}

impl Default for App {
	fn default() -> Self {
		Self::new()
	}
}

/// trait for game plugins
///
/// plugins configure the app by adding systems, resources, and other plugins.
pub trait GamePlugin: Send {
	/// get the plugin name for dependency resolution
	fn name(&self) -> &str;

	/// get the list of plugin names this plugin depends on
	fn dependencies(&self) -> &[&str] {
		&[]
	}

	/// build the plugin, adding systems and resources to the app
	fn build(&mut self, _app: &mut App) {}

	/// finish the plugin, called after all plugins have been built
	fn finish(&mut self, _app: &mut App) {}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::sync::{Arc, Mutex};

	type Log = Arc<Mutex<Vec<String>>>;
	/// callback that registers more plugins mid-build()
	type Spawn = Box<dyn FnOnce(&mut App) + Send>;

	/// test plugin that records its build/finish calls into a shared log.
	/// `spawn` optionally registers another plugin during build() to exercise
	/// mid-build registration absorption.
	struct Recorder {
		name: &'static str,
		deps: Vec<&'static str>,
		log: Log,
		spawn: Option<Spawn>,
	}

	impl GamePlugin for Recorder {
		fn name(&self) -> &str {
			self.name
		}
		fn dependencies(&self) -> &[&str] {
			&self.deps
		}
		fn build(&mut self, app: &mut App) {
			self.log
				.lock()
				.unwrap()
				.push(format!("build:{}", self.name));
			if let Some(spawn) = self.spawn.take() {
				spawn(app);
			}
		}
		fn finish(&mut self, _app: &mut App) {
			self.log
				.lock()
				.unwrap()
				.push(format!("finish:{}", self.name));
		}
	}

	fn calls(log: &Log) -> Vec<String> {
		log.lock().unwrap().clone()
	}

	#[test]
	fn builds_dependencies_before_dependents() {
		let log: Log = Arc::new(Mutex::new(Vec::new()));
		let mut app = App::new();
		// register the dependent first to prove ordering isn't just insertion order
		app.add_plugin(Recorder {
			name: "b",
			deps: vec!["a"],
			log: log.clone(),
			spawn: None,
		});
		app.add_plugin(Recorder {
			name: "a",
			deps: vec![],
			log: log.clone(),
			spawn: None,
		});
		app.build_plugins();

		let c = calls(&log);
		let build_a = c.iter().position(|x| x == "build:a").expect("a built");
		let build_b = c.iter().position(|x| x == "build:b").expect("b built");
		assert!(build_a < build_b, "a must build before b: {c:?}");
		// every build runs before any finish
		let last_build = c.iter().rposition(|x| x.starts_with("build:")).unwrap();
		let first_finish = c.iter().position(|x| x.starts_with("finish:")).unwrap();
		assert!(
			last_build < first_finish,
			"all builds precede finishes: {c:?}"
		);
	}

	#[test]
	fn absorbs_plugin_registered_during_build() {
		let log: Log = Arc::new(Mutex::new(Vec::new()));
		let log_for_child = log.clone();
		let mut app = App::new();
		app.add_plugin(Recorder {
			name: "parent",
			deps: vec![],
			log: log.clone(),
			spawn: Some(Box::new(move |app| {
				app.add_plugin(Recorder {
					name: "child",
					deps: vec!["parent"],
					log: log_for_child.clone(),
					spawn: None,
				});
			})),
		});
		app.build_plugins();

		let c = calls(&log);
		assert!(
			c.contains(&"build:parent".to_string()),
			"parent built: {c:?}"
		);
		assert!(
			c.contains(&"build:child".to_string()),
			"child registered during build must be built: {c:?}"
		);
		assert!(app.pending_plugins.is_empty(), "no plugins left pending");
	}

	#[test]
	fn leaves_unresolved_dependency_unbuilt() {
		let log: Log = Arc::new(Mutex::new(Vec::new()));
		let mut app = App::new();
		app.add_plugin(Recorder {
			name: "needy",
			deps: vec!["missing"],
			log: log.clone(),
			spawn: None,
		});
		app.build_plugins();

		assert!(
			!calls(&log).iter().any(|x| x == "build:needy"),
			"plugin with a missing dep must not build"
		);
		assert_eq!(
			app.pending_plugins.len(),
			1,
			"the unresolved plugin stays pending"
		);
	}
}
