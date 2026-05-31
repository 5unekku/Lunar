//! app builder and time resource
//!
//! the app builder provides a fluent interface for configuring the engine.
//! game plugins register their systems, resources, and sub-plugins through the app.

use std::collections::VecDeque;

use bevy_ecs::prelude::*;
use bevy_ecs::schedule::{IntoScheduleConfigs, ScheduleLabel};
use bevy_ecs::system::ScheduleSystem;

use crate::engine::Engine;
use crate::game_loop::{GameLoop, TickRate};
use crate::schedule::{StageOrder, UpdateStage};
use crate::state::EngineState;

/// runtime-switchable logic tick rate.
///
/// write `rate` to change the tick rate at any time (e.g. from a settings menu).
/// the game loop detects the change each frame and calls `GameLoop::set_tick_rate`.
#[derive(Resource, Clone, Copy, PartialEq, Eq)]
pub struct TickRateConfig {
    pub rate: TickRate,
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

    /// add a custom stage with the given ordering relative to built-in stages.
    /// **note:** custom stages are not yet implemented — this is a no-op placeholder.
    /// full stage support requires `bevy_ecs`'s schedule graph, which is a planned upgrade.
    pub fn add_stage<S: ScheduleLabel>(&mut self, _stage: S, _order: StageOrder) -> &mut Self {
        log::warn!("add_stage: custom stages not yet implemented, call ignored");
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
        // simple topological sort using Kahn's algorithm
        let mut built = std::mem::take(&mut self.built_plugins);
        let mut pending = std::mem::take(&mut self.pending_plugins);
        let mut ready: Vec<Box<dyn GamePlugin>> = Vec::new();

        let mut queue = VecDeque::new();

        // find plugins with no dependencies
        for (i, plugin) in pending.iter().enumerate() {
            let deps = plugin.dependencies();
            if deps.is_empty() || deps.iter().all(|d| built.contains(&d.to_string())) {
                queue.push_back(i);
            }
        }

        while let Some(idx) = queue.pop_front() {
            let mut plugin = pending.remove(idx);
            let name = plugin.name().to_string();

            // adjust remaining indices
            for i in &mut queue {
                if *i > idx {
                    *i -= 1;
                }
            }

            plugin.build(self);
            built.push(name.clone());
            ready.push(plugin);

            // absorb any plugins registered during build() before checking deps
            pending.extend(std::mem::take(&mut self.pending_plugins));

            // check if any pending plugins now have all deps met
            for (i, p) in pending.iter().enumerate() {
                let deps = p.dependencies();
                if deps.iter().all(|d| built.contains(&d.to_string())) && !queue.contains(&i) {
                    queue.push_back(i);
                }
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

    /// start the game loop. `frame_cap` = 0 means uncapped/vsync. `tick_rate` is
    /// the fixed logic rate, independent of render rate.
    pub fn run(&mut self, frame_cap: u32, tick_rate: crate::game_loop::TickRate) {
        self.run_with_events(frame_cap, tick_rate, |_| {});
    }

    /// start the game loop with per-frame event processing.
    ///
    /// `time.delta_seconds()` inside systems is always exactly `1 / tick_hz`.
    /// `time.real_delta_seconds()` is wall-clock render frame time for interpolation.
    pub fn run_with_events<F>(
        &mut self,
        frame_cap: u32,
        tick_rate: crate::game_loop::TickRate,
        mut process_events: F,
    )
    where
        F: FnMut(&mut World),
    {
        self.build_plugins();
        if !self.startup_run {
            self.engine.run_startup();
            self.startup_run = true;
        }

        // insert TickRateConfig so game code can change tick rate at runtime
        self.engine.world_mut().insert_resource(TickRateConfig { rate: tick_rate });

        let mut fixed_delta = tick_rate.delta_seconds();
        let mut game_loop = GameLoop::new(frame_cap, tick_rate);

        while game_loop.is_running() {
            // check if game code changed the tick rate via TickRateConfig
            if let Some(cfg) = self.engine.world().get_resource::<TickRateConfig>()
                && cfg.rate != game_loop.tick_rate() {
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
