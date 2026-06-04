//! 3d navigation for the Lunar engine.
//!
//! three-stage pipeline:
//!
//! 1. **baking** (map load) — [`baker::bake`] runs rerecast on raw triangle geometry
//!    and produces a [`NavMeshResource`].
//! 2. **pathfinding** (per bot, on goal change) — the pathfinding system queries the
//!    [`NavMeshResource`] and writes waypoints into each bot's [`NavPath`].
//! 3. **crowd avoidance** (every frame) — [`crowd::crowd_avoidance_system`] runs orca
//!    for all agents simultaneously and writes [`NavVelocity`] components that game
//!    code reads to actually move entities.
//!
//! # quick start
//!
//! ```ignore
//! use lunar_nav::{NavPlugin, NavMeshResource, NavAgent, NavVelocity, baker::{bake, BakeConfig, NavTriangleInput}};
//! use glam::Vec3;
//!
//! // bake at map load
//! let tris = vec![NavTriangleInput::walkable(...)];
//! let nav = NavMeshResource(bake(&tris, &BakeConfig::default()).unwrap());
//!
//! // in app setup
//! app.add_plugin(NavPlugin);
//! app.insert_resource(nav);
//!
//! // spawn a bot
//! commands.spawn((
//!     NavAgent { position: Vec3::ZERO, goal: Vec3::new(50.0, 0.0, 10.0), speed: 5.0, radius: 0.4, preferred_velocity: Vec3::ZERO },
//!     NavVelocity::default(),
//! ));
//!
//! // in a movement system, read NavVelocity to drive the character controller
//! fn move_agents(mut query: Query<(&NavVelocity, &mut Transform)>) {
//!     for (vel, mut transform) in &mut query {
//!         transform.translation += vel.0 * delta_time;
//!     }
//! }
//! ```

pub mod baker;
pub mod crowd;
mod plugin;

pub use crowd::{CrowdConfig, NavVelocity};
pub use plugin::NavPlugin;

use bevy_ecs::prelude::*;
use glam::Vec3;
use navmesh::{NavMesh, NavPathMode, NavQuery};

/// the baked navmesh — insert as a resource after calling [`baker::bake`].
#[derive(Resource)]
pub struct NavMeshResource(pub NavMesh);

/// attach to any entity that should be navigated by the engine.
///
/// set `goal` to trigger a path replan. read [`NavVelocity`] to move the entity.
#[derive(Component)]
pub struct NavAgent {
    /// current world position of this agent (keep synced with the transform)
    pub position: Vec3,
    /// where the agent wants to go
    pub goal: Vec3,
    /// max movement speed (m/s)
    pub speed: f32,
    /// collision avoidance radius (metres)
    pub radius: f32,
    /// preferred velocity toward next waypoint — written by pathfinding system,
    /// read by crowd avoidance system. game code does not need to set this.
    pub preferred_velocity: Vec3,
}

/// waypoints computed by the pathfinding system; consumed by crowd avoidance.
#[derive(Component, Default)]
pub struct NavPath {
    pub waypoints: Vec<Vec3>,
    pub current: usize,
}

impl NavPath {
    pub fn is_complete(&self) -> bool { self.current >= self.waypoints.len() }

    pub fn next_waypoint(&self) -> Option<Vec3> { self.waypoints.get(self.current).copied() }
}

/// per-frame system: replan paths for agents whose goal changed or path is stale,
/// then update each agent's preferred_velocity toward its next waypoint.
pub fn pathfinding_system(
    nav: Option<Res<NavMeshResource>>,
    mut agents: Query<(&mut NavAgent, &mut NavPath)>,
) {
    let Some(nav) = nav else { return };

    for (mut agent, mut path) in &mut agents {
        // replan if no path or arrived at last waypoint
        if path.is_complete() || path.waypoints.is_empty() {
            let from = navmesh::NavVec3::new(agent.position.x, agent.position.y, agent.position.z);
            let to   = navmesh::NavVec3::new(agent.goal.x,     agent.goal.y,     agent.goal.z);

            path.waypoints = nav.0
                .find_path(from, to, NavQuery::Accuracy, NavPathMode::Accuracy)
                .unwrap_or_default()
                .into_iter()
                .map(|p| Vec3::new(p.x, p.y, p.z))
                .collect();
            path.current = 0;
        }

        // advance past reached waypoints (within 0.3 m)
        while let Some(wp) = path.next_waypoint() {
            if agent.position.distance(wp) < 0.3 {
                path.current += 1;
            } else {
                break;
            }
        }

        // set preferred velocity toward next waypoint
        agent.preferred_velocity = match path.next_waypoint() {
            Some(wp) => (wp - agent.position).normalize_or_zero() * agent.speed,
            None => Vec3::ZERO,
        };
    }
}
