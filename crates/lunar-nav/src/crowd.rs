//! per-frame crowd avoidance via dodgy_3d (orca/rvo2-3d).
//!
//! the crowd system runs after pathfinding — it takes each agent's preferred
//! velocity (toward the next waypoint) and steers it to avoid other agents.

use bevy_ecs::prelude::*;
use dodgy_3d::{Agent as OrcaAgent, AvoidanceOptions};
use glam::Vec3;

use super::NavAgent;

/// orca options. tune once and insert as a resource.
#[derive(Resource)]
pub struct CrowdConfig {
    /// how many seconds ahead agents look when computing avoidance
    pub time_horizon: f32,
}

impl Default for CrowdConfig {
    fn default() -> Self { Self { time_horizon: 2.0 } }
}

/// per-agent velocity output by the crowd system.
///
/// game code reads this each frame to move the entity.
#[derive(Component, Default)]
pub struct NavVelocity(pub Vec3);

/// per-frame system: compute orca-adjusted velocities for all nav agents.
pub fn crowd_avoidance_system(
    config: Res<CrowdConfig>,
    mut agents: Query<(&NavAgent, &mut NavVelocity)>,
) {
    let options = AvoidanceOptions { time_horizon: config.time_horizon };

    // snapshot all agent states into dodgy's format.
    // bridge: dodgy_3d re-exports its own Vec3 (glam 0.29); extract raw floats.
    let orca_agents: Vec<(OrcaAgent, Vec3)> = agents
        .iter()
        .map(|(agent, _vel)| {
            let p = agent.position;
            let v = agent.preferred_velocity;
            let orca = OrcaAgent {
                position: to_dodgy(p),
                velocity: to_dodgy(v),
                radius: agent.radius,
                avoidance_responsibility: 0.5,
            };
            (orca, v)
        })
        .collect();

    for (i, (_agent, mut velocity)) in agents.iter_mut().enumerate() {
        use std::borrow::Cow;
        let neighbours: Vec<Cow<OrcaAgent>> = orca_agents
            .iter()
            .enumerate()
            .filter(|&(j, _)| j != i)
            .map(|(_, (a, _))| Cow::Borrowed(a))
            .collect();

        let preferred = orca_agents[i].1;
        let max_speed = preferred.length().max(0.001);

        let adjusted = orca_agents[i].0.compute_avoiding_velocity(
            &neighbours,
            to_dodgy(preferred),
            max_speed,
            max_speed * 4.0,
            &options,
        );

        velocity.0 = from_dodgy(adjusted);
    }
}

/// bridge: glam 0.33 Vec3 → dodgy_3d's Vec3 (glam 0.29) via raw floats.
#[inline]
fn to_dodgy(v: Vec3) -> dodgy_3d::Vec3 { dodgy_3d::Vec3::new(v.x, v.y, v.z) }

/// bridge: dodgy_3d's Vec3 (glam 0.29) → glam 0.33 Vec3 via raw floats.
#[inline]
fn from_dodgy(v: dodgy_3d::Vec3) -> Vec3 { Vec3::new(v.x, v.y, v.z) }
