//! app builder and time resource
//!
//! the app builder provides a fluent interface for configuring the engine.
//! game plugins register their systems, resources, and sub-plugins through the app.

use std::collections::VecDeque;
use std::time::Instant;

use bevy_ecs::prelude::*;
use bevy_ecs::system::RunSystemOnce;

use crate::engine::Engine;
use crate::game_loop::GameLoop;
use crate::state::EngineState;

/// time resource updated each frame
///
/// provides delta time for framerate-independent movement and elapsed time.
#[derive(Resource)]
pub struct Time {
    /// time since last frame in seconds (scaled)
    delta_seconds: f32,
    /// time since last frame in seconds (unscaled)
    raw_delta_seconds: f32,
    /// total elapsed time since engine start in seconds
    elapsed_seconds: f32,
    /// time multiplier (1.0 = normal, 0.5 = half speed, 2.0 = double speed)
    time_scale: f32,
    /// total frame count since engine start
    frame_count: u64,
    /// instant of the last frame
    last_frame: Instant,
}

impl Time {
    /// create a new time resource
    pub fn new() -> Self {
        Self {
            delta_seconds: 0.0,
            raw_delta_seconds: 0.0,
            elapsed_seconds: 0.0,
            time_scale: 1.0,
            frame_count: 0,
            last_frame: Instant::now(),
        }
    }

    /// get delta time in seconds (scaled)
    pub fn delta_seconds(&self) -> f32 {
        self.delta_seconds
    }

    /// get raw delta time in seconds (unscaled)
    pub fn raw_delta_seconds(&self) -> f32 {
        self.raw_delta_seconds
    }

    /// get total elapsed time in seconds
    pub fn elapsed_seconds(&self) -> f32 {
        self.elapsed_seconds
    }

    /// get the time scale multiplier
    pub fn time_scale(&self) -> f32 {
        self.time_scale
    }

    /// set the time scale multiplier
    pub fn set_time_scale(&mut self, scale: f32) {
        self.time_scale = scale.max(0.0);
    }

    /// get the total frame count
    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    /// update the time resource, called once per frame
    pub fn tick(&mut self) {
        let now = Instant::now();
        let delta = now - self.last_frame;
        self.last_frame = now;

        self.raw_delta_seconds = delta.as_secs_f32();
        self.delta_seconds = self.raw_delta_seconds * self.time_scale;
        self.elapsed_seconds += self.delta_seconds;
        self.frame_count += 1;
    }
}

impl Default for Time {
    fn default() -> Self {
        Self::new()
    }
}

/// app builder for configuring the engine
///
/// use the app to register systems, resources, and plugins before calling run().
pub struct App {
    /// the engine instance
    engine: Engine,
    /// plugins registered but not yet built
    pending_plugins: Vec<Box<dyn GamePlugin>>,
    /// names of plugins already built (for cycle detection)
    built_plugins: Vec<String>,
}

impl App {
    /// create a new app with default setup
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
        }
    }

    /// get mutable access to the world for direct manipulation
    pub fn world_mut(&mut self) -> &mut World {
        self.engine.world_mut()
    }

    /// insert a resource into the world
    pub fn insert_resource<R: Resource>(&mut self, resource: R) -> &mut Self {
        self.engine.world_mut().insert_resource(resource);
        self
    }

    /// add a system to the default schedule
    pub fn add_system<M>(&mut self, system: impl IntoSystem<(), (), M>) -> &mut Self {
        self.engine.schedule_mut().add_systems(system);
        self
    }

    /// add a startup system that runs once before the main loop
    pub fn add_startup_system<M>(&mut self, system: impl IntoSystem<(), (), M>) -> &mut Self {
        // startup systems are tracked separately and run once before the main schedule
        let _ = self.engine.world_mut().run_system_once(system);
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

        let mut queue = VecDeque::new();

        // find plugins with no dependencies
        for (i, plugin) in pending.iter().enumerate() {
            let deps = plugin.dependencies();
            if deps.is_empty() || deps.iter().all(|d| built.contains(&d.to_string())) {
                queue.push_back(i);
            }
        }

        let mut built_indices = Vec::new();

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
            built_indices.push(idx);

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
    }

    /// get a reference to the engine
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// get mutable access to the engine
    pub fn engine_mut(&mut self) -> &mut Engine {
        &mut self.engine
    }

    /// start the game loop with the given frame cap (0 = uncapped)
    pub fn run(&mut self, frame_cap: u32) {
        self.run_with_events(frame_cap, |_| {});
    }

    /// start the game loop with per-frame event processing
    /// the callback runs each frame before the ECS tick, giving you a chance to
    /// poll native events and update resources like InputState.
    pub fn run_with_events<F>(&mut self, frame_cap: u32, mut process_events: F)
    where
        F: FnMut(&mut World),
    {
        // build all pending plugins before starting
        self.build_plugins();

        let mut game_loop = GameLoop::new(frame_cap);

        while game_loop.is_running() {
            // process native events (SDL3 input, etc.)
            process_events(self.engine.world_mut());

            // check if the engine state signals stop
            if let Some(state) = self.engine.world().get_resource::<EngineState>()
                && state.is_stopping()
            {
                break;
            }

            let ticks = game_loop.tick();

            // run ECS ticks
            for _ in 0..ticks {
                // update time
                if let Some(mut time) = self.engine.world_mut().get_resource_mut::<Time>() {
                    time.tick();
                }
                // run systems
                self.engine.run();
            }

            // apply frame cap for sleep
            game_loop.apply_frame_cap();
        }
    }

    /// run a single frame tick (for use with external game loops like requestAnimationFrame)
    pub fn tick(&mut self) {
        // update time
        if let Some(mut time) = self.engine.world_mut().get_resource_mut::<Time>() {
            time.tick();
        }
        // run systems
        self.engine.run();
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
