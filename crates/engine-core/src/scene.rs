//! scene system for game state management
//!
//! scenes are higher-level game states (main menu, gameplay, pause, game over)
//! that can be switched or stacked as overlays.
//!
//! # scene lifecycle
//!
//! 1. register scenes with [`SceneManager::register_scene`]
//! 2. switch to a scene with [`SceneManager::switch_to`] — triggers [`Scene::on_exit`] then [`Scene::on_enter`]
//! 3. push an overlay with [`SceneManager::push_overlay`] — stacks on top without unloading current
//! 4. pop an overlay with [`SceneManager::pop_overlay`] — removes top overlay

use bevy_ecs::prelude::*;

/// scene trait — implement to define a game scene.
///
/// scenes represent distinct game states like menus, gameplay, or cutscenes.
/// unlike zones, scenes can be stacked as overlays.
pub trait Scene: Send + Sync + 'static {
    /// called when the scene becomes active
    fn on_enter(&mut self, _world: &mut World) {}

    /// called each frame while the scene is active
    fn on_update(&mut self, _world: &mut World) {}

    /// called when the scene is deactivated
    fn on_exit(&mut self, _world: &mut World) {}
}

/// a boxed scene with its name
struct BoxedScene {
    scene: Box<dyn Scene>,
}

/// scene manager resource, manages scene switching and overlays.
///
/// switch between scenes with [`SceneManager::switch_to`] or stack
/// overlay scenes with [`SceneManager::push_overlay`].
#[derive(Resource)]
pub struct SceneManager {
    scenes: std::collections::HashMap<String, BoxedScene>,
    /// stack of active scene names (bottom = base, top = current overlay)
    scene_stack: Vec<String>,
}

impl SceneManager {
    /// create a new scene manager
    #[must_use]
    pub fn new() -> Self {
        Self {
            scenes: std::collections::HashMap::new(),
            scene_stack: Vec::new(),
        }
    }

    /// register a scene by name
    pub fn register_scene<S: Scene>(&mut self, name: &str, scene: S) {
        self.scenes.insert(
            name.to_string(),
            BoxedScene {
                scene: Box::new(scene),
            },
        );
        log::info!("SceneManager: registered scene '{name}'");
    }

    /// switch to a scene, replacing all current scenes.
    /// triggers `on_exit` on the current scene (if any), then `on_enter` on the new one.
    pub fn switch_to(&mut self, name: &str, world: &mut World) {
        if !self.scenes.contains_key(name) {
            log::warn!("SceneManager: scene '{name}' not registered");
            return;
        }

        // exit all current scenes
        for scene_name in self.scene_stack.drain(..).rev() {
            if let Some(boxed) = self.scenes.get_mut(&scene_name) {
                boxed.scene.on_exit(world);
            }
        }

        // enter the new scene
        if let Some(boxed) = self.scenes.get_mut(name) {
            boxed.scene.on_enter(world);
        }
        self.scene_stack.push(name.to_string());
        log::info!("SceneManager: switched to scene '{name}'");
    }

    /// push an overlay scene on top of the current scene stack.
    /// the current scene stays active underneath; the overlay's `on_enter` is called.
    pub fn push_overlay(&mut self, name: &str, world: &mut World) {
        if !self.scenes.contains_key(name) {
            log::warn!("SceneManager: scene '{name}' not registered");
            return;
        }

        if let Some(boxed) = self.scenes.get_mut(name) {
            boxed.scene.on_enter(world);
        }
        self.scene_stack.push(name.to_string());
        log::info!("SceneManager: pushed overlay '{name}'");
    }

    /// pop the top overlay scene.
    /// triggers `on_exit` on the overlay, then removes it from the stack.
    /// does nothing if only one scene is active.
    pub fn pop_overlay(&mut self, world: &mut World) {
        if self.scene_stack.len() <= 1 {
            return;
        }

        if let Some(name) = self.scene_stack.pop() {
            if let Some(boxed) = self.scenes.get_mut(&name) {
                boxed.scene.on_exit(world);
            }
            log::info!("SceneManager: popped overlay '{name}'");
        }
    }

    /// get the current (top) scene name
    #[must_use]
    pub fn current_scene(&self) -> Option<&str> {
        self.scene_stack.last().map(std::string::String::as_str)
    }

    /// update all active scenes from bottom to top
    pub fn update_all(&mut self, world: &mut World) {
        for name in &self.scene_stack {
            if let Some(boxed) = self.scenes.get_mut(name) {
                boxed.scene.on_update(world);
            }
        }
    }
}

impl Default for SceneManager {
    fn default() -> Self {
        Self::new()
    }
}
