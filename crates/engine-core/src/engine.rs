//! engine wrapper around bevy_ecs world and schedule
//!
//! the engine owns the ECS world and manages system execution.
//! game code interacts with the world through the [`App`] builder.

use bevy_ecs::prelude::*;
use bevy_ecs::schedule::ScheduleLabel;

/// default schedule label for the main update loop.
///
/// all systems added via [`App::add_system`] are registered under this schedule.
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct Update;

/// the engine owns the ECS world and schedule.
///
/// this is the low-level wrapper around bevy_ecs.
/// most game code should interact with the engine through [`App`] instead.
pub struct Engine {
    /// the ECS world containing all entities, components, and resources
    world: World,
    /// the schedule containing all systems
    schedule: Schedule,
}

impl Engine {
    /// create a new empty engine
    pub fn new() -> Self {
        Self {
            world: World::new(),
            schedule: Schedule::new(Update),
        }
    }

    /// get mutable access to the world
    pub fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    /// get a reference to the world
    pub fn world(&self) -> &World {
        &self.world
    }

    /// get mutable access to the schedule
    pub fn schedule_mut(&mut self) -> &mut Schedule {
        &mut self.schedule
    }

    /// get a reference to the schedule
    pub fn schedule(&self) -> &Schedule {
        &self.schedule
    }

    /// run all systems in the schedule
    pub fn run(&mut self) {
        self.schedule.run(&mut self.world);
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}
