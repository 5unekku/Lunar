//! world zone management
//!
//! zones are collections of entities and systems that can be loaded/unloaded independently.
//! the world persists across zone transitions, enabling seamless RPG-style area changes.
//!
//! # zone lifecycle
//!
//! 1. register zones with [`WorldManager::register_zone`]
//! 2. enter a zone with [`WorldManager::enter_zone`] — triggers [`Zone::on_load`] and [`Zone::on_enter`]
//! 3. define [`ZoneTransition`]s for automatic area changes
//! 4. exit a zone — triggers [`Zone::on_exit`]

use rustc_hash::FxHashMap as HashMap;

use bevy_ecs::prelude::*;
use lunar_math::{Color, Rect, Vec2};

/// fade configuration for zone transitions.
///
/// controls the visual effect when moving between zones.
#[derive(Debug, Clone)]
pub struct FadeConfig {
	/// transition duration in seconds
	pub duration: f32,
	/// fade color
	pub color: Color,
}

/// a transition point that triggers when an entity enters the area.
///
/// define these in [`Zone::transitions`] to create automatic area changes
/// when the player walks into a trigger zone.
#[derive(Debug, Clone)]
pub struct ZoneTransition {
	/// trigger area
	pub trigger_area: Rect,
	/// target zone name
	pub target_zone: String,
	/// spawn position in the target zone
	pub spawn_position: Vec2,
	/// optional fade transition
	pub fade: Option<FadeConfig>,
}

/// zone trait — implement to define a world zone.
///
/// each zone type defines its own lifecycle hooks for loading,
/// entering, and exiting. implement this trait to create custom zones.
pub trait Zone: Send + Sync + 'static {
	/// called when the zone is being loaded (async asset loading)
	fn on_load(&mut self, _world: &mut World) {}

	/// called when the zone becomes active
	fn on_enter(&mut self, _world: &mut World) {}

	/// called when the zone is being unloaded
	fn on_exit(&mut self, _world: &mut World) {}

	/// optional: define transition points
	fn transitions(&self) -> Vec<ZoneTransition> {
		Vec::new()
	}
}

/// a boxed zone with its name
struct BoxedZone {
	zone: Box<dyn Zone>,
}

/// world manager resource, manages zone loading and transitions.
///
/// register zones with [`WorldManager::register_zone`] and transition
/// between them with [`WorldManager::enter_zone`]. the world state
/// persists across transitions, allowing seamless area changes.
#[derive(Resource)]
pub struct WorldManager {
	zones: HashMap<String, BoxedZone>,
	current_zone: Option<String>,
	pending_transition: Option<ZoneTransition>,
}

impl WorldManager {
	/// create a new world manager
	#[must_use]
	pub fn new() -> Self {
		Self {
			zones: HashMap::default(),
			current_zone: None,
			pending_transition: None,
		}
	}

	/// register a zone by name
	pub fn register_zone<Z: Zone>(&mut self, name: &str, zone: Z) {
		self.zones.insert(
			name.to_string(),
			BoxedZone {
				zone: Box::new(zone),
			},
		);
		log::info!("WorldManager: registered zone '{name}'");
	}

	/// transition to a zone (keeps world state)
	pub fn enter_zone(&mut self, name: &str, world: &mut World) {
		if let Some(current) = &self.current_zone
			&& current == name
		{
			return;
		}

		if !self.zones.contains_key(name) {
			log::warn!("WorldManager: zone '{name}' not registered");
			return;
		}

		// exit current zone
		if let Some(current_name) = self.current_zone.take()
			&& let Some(current_boxed) = self.zones.get_mut(&current_name)
		{
			current_boxed.zone.on_exit(world);
		}

		// load and enter new zone
		if let Some(boxed) = self.zones.get_mut(name) {
			boxed.zone.on_load(world);
			boxed.zone.on_enter(world);
		}
		self.current_zone = Some(name.to_string());
		log::info!("WorldManager: entered zone '{name}'");
	}

	/// get the current zone name
	#[must_use]
	pub fn current_zone(&self) -> Option<&str> {
		self.current_zone.as_deref()
	}

	/// get transitions for the current zone
	#[must_use]
	pub fn current_transitions(&self) -> Vec<ZoneTransition> {
		if let Some(name) = &self.current_zone
			&& let Some(boxed) = self.zones.get(name)
		{
			return boxed.zone.transitions();
		}
		Vec::new()
	}

	/// queue a transition
	pub fn queue_transition(&mut self, transition: ZoneTransition) {
		self.pending_transition = Some(transition);
	}

	/// process pending transitions
	pub fn process_transitions(&mut self, world: &mut World) {
		if let Some(transition) = self.pending_transition.take() {
			self.enter_zone(&transition.target_zone, world);
		}
	}
}

impl Default for WorldManager {
	fn default() -> Self {
		Self::new()
	}
}

/// drop-in plugin: inserts a default [`WorldManager`] resource. register your
/// zones on it in a setup system, then drive transitions via `enter_zone` /
/// `queue_transition`.
#[derive(Default)]
pub struct ZonePlugin;

impl lunar_core::GamePlugin for ZonePlugin {
	fn name(&self) -> &str {
		"ZonePlugin"
	}
	fn build(&mut self, app: &mut lunar_core::App) {
		app.insert_resource(WorldManager::new());
	}
}

/// common, game-facing zone types for `use lunar::prelude::*`.
pub mod prelude {
	pub use crate::{FadeConfig, WorldManager, Zone, ZonePlugin, ZoneTransition};
}
