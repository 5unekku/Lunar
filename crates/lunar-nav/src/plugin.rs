//! ECS plugin that registers the nav systems.

use bevy_ecs::prelude::*;
use lunar_core::{App, GamePlugin};

use crate::crowd::{CrowdConfig, crowd_avoidance_system};
use crate::pathfinding_system;

pub struct NavPlugin;

impl GamePlugin for NavPlugin {
    fn name(&self) -> &'static str { "NavPlugin" }

    fn dependencies(&self) -> &[&str] { &[] }

    fn build(&mut self, app: &mut App) {
        app.insert_resource(CrowdConfig::default());
        app.add_system(pathfinding_system);
        app.add_system(crowd_avoidance_system);
        log::info!("NavPlugin: pathfinding + crowd avoidance registered");
    }
}
