//! the bench scenes. each scene is fully procedural (no asset files): it
//! registers a startup system that builds its content and any per-frame systems
//! that animate it. scenes are deterministic — seeded placement, fixed tick —
//! so golden frames are stable across runs.

use lunar::prelude::App;

pub mod dynamic_swarm;
pub mod feature_reel;
pub mod sprite_storm;
pub mod static_city;

/// which engine a scene drives.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Dim {
	D3,
	D2,
}

/// a benchable scene: a name, which engine it needs, and a registrar that wires
/// its systems into the app the harness just built.
pub struct Scene {
	pub name: &'static str,
	pub dim: Dim,
	pub register: fn(&mut App),
}

/// every scene, in report order.
pub fn all() -> Vec<Scene> {
	vec![
		Scene { name: "static-city", dim: Dim::D3, register: static_city::register },
		Scene { name: "dynamic-swarm", dim: Dim::D3, register: dynamic_swarm::register },
		Scene { name: "feature-reel", dim: Dim::D3, register: feature_reel::register },
		Scene { name: "sprite-storm", dim: Dim::D2, register: sprite_storm::register },
	]
}

/// look up a single scene by name for `--scene`.
pub fn by_name(name: &str) -> Option<Scene> {
	all().into_iter().find(|s| s.name == name)
}
