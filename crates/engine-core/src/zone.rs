//! world zone management
//!
//! zones are collections of entities and systems that can be loaded/unloaded independently.
//! the world persists across zone transitions, enabling seamless RPG-style area changes.

use std::collections::HashMap;

use bevy_ecs::prelude::*;
use engine_math::{Color, Rect, Vec2};

use crate::app::App;

/// fade configuration for zone transitions
#[derive(Debug, Clone)]
pub struct FadeConfig {
    /// transition duration in seconds
    pub duration: f32,
    /// fade color
    pub color: Color,
}

/// a transition point that triggers when an entity enters the area
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

/// zone trait — implement to define a world zone
pub trait Zone: Send + Sync + 'static {
    /// called when the zone is being loaded (async asset loading)
    fn on_load(&mut self, _app: &mut App) {}

    /// called when the zone becomes active
    fn on_enter(&mut self, _app: &mut App) {}

    /// called when the zone is being unloaded
    fn on_exit(&mut self, _app: &mut App) {}

    /// optional: define transition points
    fn transitions(&self) -> Vec<ZoneTransition> {
        Vec::new()
    }
}

/// a boxed zone with its name
struct BoxedZone {
    zone: Box<dyn Zone>,
}

/// world manager resource, manages zone loading and transitions
#[derive(Resource)]
pub struct WorldManager {
    zones: HashMap<String, BoxedZone>,
    current_zone: Option<String>,
    pending_transition: Option<ZoneTransition>,
}

impl WorldManager {
    /// create a new world manager
    pub fn new() -> Self {
        WorldManager {
            zones: HashMap::new(),
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
        log::info!("WorldManager: registered zone '{}'", name);
    }

    /// transition to a zone (keeps world state)
    pub fn enter_zone(&mut self, name: &str) {
        if let Some(current) = &self.current_zone
            && current == name
        {
            return;
        }

        if !self.zones.contains_key(name) {
            log::warn!("WorldManager: zone '{}' not registered", name);
            return;
        }

        // exit current zone
        if let Some(current_name) = self.current_zone.take()
            && let Some(current_boxed) = self.zones.get_mut(&current_name)
        {
            current_boxed.zone.on_exit(&mut App::new());
        }

        // load and enter new zone
        if let Some(boxed) = self.zones.get_mut(name) {
            boxed.zone.on_load(&mut App::new());
            boxed.zone.on_enter(&mut App::new());
        }
        self.current_zone = Some(name.to_string());
        log::info!("WorldManager: entered zone '{}'", name);
    }

    /// get the current zone name
    pub fn current_zone(&self) -> Option<&str> {
        self.current_zone.as_deref()
    }

    /// get transitions for the current zone
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
    pub fn process_transitions(&mut self) {
        if let Some(transition) = self.pending_transition.take() {
            self.enter_zone(&transition.target_zone);
        }
    }
}

impl Default for WorldManager {
    fn default() -> Self {
        Self::new()
    }
}
