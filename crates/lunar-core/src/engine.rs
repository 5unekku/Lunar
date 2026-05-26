//! engine wrapper around `bevy_ecs` world and schedule
//!
//! the engine owns the ECS world and manages system execution.
//! game code interacts with the world through the [`App`] builder.
//!
//! # stage-based ordering
//!
//! systems can be added to named stages (Input, Physics, Update, Render).
//! stages run in a fixed order each frame, with `apply_deferred` between them
//! to flush commands from the previous stage.

use bevy_ecs::prelude::*;
use bevy_ecs::schedule::ScheduleLabel;

/// schedule for startup systems that run once before the main loop
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct Startup;

/// the engine owns the ECS world and schedules.
///
/// this is the low-level wrapper around `bevy_ecs`.
/// most game code should interact with the engine through [`crate::app::App`] instead.
///
/// # example
///
/// ```ignore
/// use lunar_core::Engine;
/// use lunar_core::app::App;
///
/// let mut app = App::new();
/// app.add_plugin(MyGamePlugin);
/// // Engine wraps the bevy_ecs World and Schedule
/// // most code interacts through App instead
/// ```
pub struct Engine {
    /// the ECS world containing all entities, components, and resources
    world: World,
    /// the startup schedule (run once before main loop)
    startup_schedule: Schedule,
    /// per-stage schedules for ordered system execution
    stage_schedules: [Schedule; 5],
}

impl Engine {
    /// create a new empty engine
    #[must_use]
    pub fn new() -> Self {
        use crate::schedule::UpdateStage;
        Self {
            world: World::new(),
            startup_schedule: Schedule::new(Startup),
            stage_schedules: [
                Schedule::new(UpdateStage::Input),
                Schedule::new(UpdateStage::Physics),
                Schedule::new(UpdateStage::Update),
                Schedule::new(UpdateStage::Render),
                Schedule::new(UpdateStage::PostUpdate),
            ],
        }
    }

    /// get mutable access to the world
    pub const fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    /// get a reference to the world
    pub const fn world(&self) -> &World {
        &self.world
    }

    /// get mutable access to the startup schedule
    pub const fn startup_schedule_mut(&mut self) -> &mut Schedule {
        &mut self.startup_schedule
    }

    /// get mutable access to a stage schedule
    pub const fn stage_schedule_mut(
        &mut self,
        stage: crate::schedule::UpdateStage,
    ) -> &mut Schedule {
        &mut self.stage_schedules[stage as usize]
    }

    /// run all startup systems once
    pub fn run_startup(&mut self) {
        self.startup_schedule.run(&mut self.world);
    }

    /// run all stage schedules in order: Input → Physics → Update → Render → PostUpdate
    /// applies deferred commands between each stage so entity changes are visible.
    pub fn run_stages(&mut self) {
        const STAGE_COUNT: usize = 5;
        for i in 0..STAGE_COUNT {
            self.stage_schedules[i].run(&mut self.world);
            if i < STAGE_COUNT - 1 {
                self.world.flush();
            }
        }
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}
