//! world manifest: XML-based world definition with scenes and spatial chunks.
#![allow(
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::must_use_candidate
)]
//!
//! # authoring format (XML)
//!
//! ```xml
//! <world name="overworld" start="village">
//!     <scenes>
//!         <scene id="1" name="village" file="scenes/village.ron" />
//!         <scene id="2" name="dungeon" file="scenes/dungeon.ron" />
//!     </scenes>
//!     <chunks>
//!         <chunk id="10" name="village_center" scene="village"
//!                x_min="0" x_max="100" y_min="0" y_max="100" />
//!         <chunk id="11" name="forest_edge" scene="village"
//!                x_min="100" x_max="200" y_min="0" y_max="100" />
//!     </chunks>
//! </world>
//! ```
//!
//! # compiled output
//!
//! at build time, all unique strings are interned and replaced with u32 identifiers.
//! the compiled binary contains no loose strings in release builds.

use bevy_ecs::prelude::*;
use roxmltree::Document;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::scene_format::SceneDefinition;

// ── string interning ──────────────────────────

/// interner that maps strings to u32 identifiers.
///
/// at build time, all unique strings from authoring files are collected
/// and assigned sequential u32 ids. the compiled output references
/// these ids instead of raw strings.
#[derive(Debug, Clone, Default)]
pub struct StringInterner {
    forward: HashMap<String, u32>,
    reverse: Vec<String>,
}

impl StringInterner {
    /// create a new empty interner.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// intern a string and return its u32 id.
    /// if the string was already interned, returns the existing id.
    pub fn intern(&mut self, s: &str) -> u32 {
        if let Some(&id) = self.forward.get(s) {
            return id;
        }
        let id = u32::try_from(self.reverse.len())
            .unwrap_or_else(|_| panic!("string interner exceeded u32 capacity"));
        self.forward.insert(s.to_string(), id);
        self.reverse.push(s.to_string());
        id
    }

    /// resolve a u32 id back to its string.
    /// returns None if the id was never interned.
    #[must_use]
    pub fn resolve(&self, id: u32) -> Option<&str> {
        self.reverse.get(id as usize).map(String::as_str)
    }

    /// get the number of interned strings.
    #[must_use]
    pub fn len(&self) -> usize {
        self.reverse.len()
    }

    /// check if the interner is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.reverse.is_empty()
    }
}

// ── authoring types (XML) ───────────────────────

/// root world manifest parsed from XML.
#[derive(Debug, Clone, Serialize)]
pub struct WorldManifest {
    /// human-readable world name
    pub name: String,
    /// name of the start scene
    pub start_scene: String,
    /// list of scene entries
    pub scenes: Vec<SceneEntry>,
    /// list of chunk entries
    pub chunks: Vec<ChunkEntry>,
    /// name → index into `scenes` for O(1) lookup.
    #[serde(skip)]
    scene_index: HashMap<String, usize>,
}

/// a scene entry in the world manifest.
#[derive(Debug, Clone, Serialize)]
pub struct SceneEntry {
    /// numeric id
    pub id: u32,
    /// human-readable name
    pub name: String,
    /// path to the RON scene file
    pub file: String,
}

/// a spatial chunk entry in the world manifest.
#[derive(Debug, Clone, Serialize)]
pub struct ChunkEntry {
    /// numeric id
    pub id: u32,
    /// human-readable name
    pub name: String,
    /// which scene this chunk belongs to (by name)
    pub scene: String,
    /// spatial bounds
    pub x_min: f32,
    pub x_max: f32,
    pub y_min: f32,
    pub y_max: f32,
}

/// helper to get an attribute from a roxmltree node.
fn get_attr(node: &roxmltree::Node, key: &str) -> Option<String> {
    node.attribute(key).map(String::from)
}

/// helper to parse a required attribute or return an error.
fn require_attr(node: &roxmltree::Node, key: &str) -> Result<String, String> {
    get_attr(node, key).ok_or_else(|| format!("missing attribute '{key}'"))
}

/// helper to parse a required f32 attribute.
fn require_f32(node: &roxmltree::Node, key: &str) -> Result<f32, String> {
    let val = require_attr(node, key)?;
    val.parse::<f32>()
        .map_err(|_| format!("invalid f32 for '{key}': {val}"))
}

/// helper to parse a required u32 attribute.
fn require_u32(node: &roxmltree::Node, key: &str) -> Result<u32, String> {
    let val = require_attr(node, key)?;
    val.parse::<u32>()
        .map_err(|_| format!("invalid u32 for '{key}': {val}"))
}

impl WorldManifest {
    /// parse a world manifest from an XML string using roxmltree.
    pub fn from_xml(source: &str) -> Result<Self, String> {
        let doc = Document::parse(source)
            .map_err(|e| format!("failed to parse world manifest xml: {e}"))?;
        let root = doc.root_element();

        if root.tag_name().name() != "world" {
            return Err("root element must be <world>".to_string());
        }

        let name = require_attr(&root, "name")?;
        let start_scene = require_attr(&root, "start")?;

        let mut scenes = Vec::new();
        let mut chunks = Vec::new();

        for child in root.children() {
            match child.tag_name().name() {
                "scenes" => {
                    for scene_node in child.children() {
                        if scene_node.tag_name().name() == "scene" {
                            let id = require_u32(&scene_node, "id")?;
                            let scene_name = require_attr(&scene_node, "name")?;
                            let file = require_attr(&scene_node, "file")?;
                            scenes.push(SceneEntry {
                                id,
                                name: scene_name,
                                file,
                            });
                        }
                    }
                }
                "chunks" => {
                    for chunk_node in child.children() {
                        if chunk_node.tag_name().name() == "chunk" {
                            let id = require_u32(&chunk_node, "id")?;
                            let chunk_name = require_attr(&chunk_node, "name")?;
                            let scene_name = require_attr(&chunk_node, "scene")?;
                            let x_min = require_f32(&chunk_node, "x_min")?;
                            let x_max = require_f32(&chunk_node, "x_max")?;
                            let y_min = require_f32(&chunk_node, "y_min")?;
                            let y_max = require_f32(&chunk_node, "y_max")?;
                            chunks.push(ChunkEntry {
                                id,
                                name: chunk_name,
                                scene: scene_name,
                                x_min,
                                x_max,
                                y_min,
                                y_max,
                            });
                        }
                    }
                }
                _ => {}
            }
        }

        let scene_index = scenes.iter().enumerate().map(|(i, s)| (s.name.clone(), i)).collect();
        Ok(WorldManifest {
            name,
            start_scene,
            scenes,
            chunks,
            scene_index,
        })
    }

    /// load a world manifest from an XML file path.
    pub fn from_file(path: &str) -> Result<Self, String> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let content = std::fs::read_to_string(path)
                .map_err(|e| format!("failed to read world manifest file '{path}': {e}"))?;
            Self::from_xml(&content)
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = path;
            Err("file loading not supported on wasm, use bundled assets or fetch API".to_string())
        }
    }

    /// find a scene entry by name — O(1) via index.
    #[must_use]
    pub fn find_scene(&self, name: &str) -> Option<&SceneEntry> {
        self.scene_index.get(name).map(|&i| &self.scenes[i])
    }

    /// find the start scene entry.
    #[must_use]
    pub fn start_scene_entry(&self) -> Option<&SceneEntry> {
        self.find_scene(&self.start_scene)
    }

    /// iterate chunks that overlap a given bounding box — no allocation.
    pub fn chunks_in_bounds_iter(
        &self,
        x_min: f32,
        x_max: f32,
        y_min: f32,
        y_max: f32,
    ) -> impl Iterator<Item = &ChunkEntry> {
        self.chunks
            .iter()
            .filter(move |c| c.x_max > x_min && c.x_min < x_max && c.y_max > y_min && c.y_min < y_max)
    }

    /// find chunks that overlap a given bounding box.
    pub fn chunks_in_bounds(
        &self,
        x_min: f32,
        x_max: f32,
        y_min: f32,
        y_max: f32,
    ) -> Vec<&ChunkEntry> {
        self.chunks_in_bounds_iter(x_min, x_max, y_min, y_max).collect()
    }

    /// iterate chunks within a radius of a center point — no allocation.
    pub fn chunks_in_radius_iter(&self, cx: f32, cy: f32, radius: f32) -> impl Iterator<Item = &ChunkEntry> {
        let r2 = radius * radius;
        self.chunks.iter().filter(move |c| {
            let closest_x = cx.clamp(c.x_min, c.x_max);
            let closest_y = cy.clamp(c.y_min, c.y_max);
            let dx = closest_x - cx;
            let dy = closest_y - cy;
            dx * dx + dy * dy <= r2
        })
    }

    /// find chunks within a radius of a center point.
    pub fn chunks_in_radius(&self, cx: f32, cy: f32, radius: f32) -> Vec<&ChunkEntry> {
        self.chunks_in_radius_iter(cx, cy, radius).collect()
    }
}

// ── compiled binary types (no loose strings in release) ───────────

/// compiled world manifest with interned string ids.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledWorld {
    /// string table for resolving interned ids
    pub strings: Vec<String>,
    /// world name (interned id)
    pub name_id: u32,
    /// start scene name (interned id)
    pub start_scene_id: u32,
    /// compiled scene entries
    pub scenes: Vec<CompiledSceneEntry>,
    /// compiled chunk entries
    pub chunks: Vec<CompiledChunkEntry>,
}

/// compiled scene entry with interned string ids.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledSceneEntry {
    pub id: u32,
    pub name_id: u32,
    pub file_id: u32,
}

/// compiled chunk entry with interned string ids.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledChunkEntry {
    pub id: u32,
    pub name_id: u32,
    pub scene_id: u32,
    pub x_min: f32,
    pub x_max: f32,
    pub y_min: f32,
    pub y_max: f32,
}

impl WorldManifest {
    /// compile this manifest into a binary format with interned strings.
    pub fn compile(&self) -> Result<CompiledWorld, String> {
        let mut interner = StringInterner::new();

        // intern all strings first
        let name_id = interner.intern(&self.name);
        let start_id = interner.intern(&self.start_scene);

        let compiled_scenes = self
            .scenes
            .iter()
            .map(|s| CompiledSceneEntry {
                id: s.id,
                name_id: interner.intern(&s.name),
                file_id: interner.intern(&s.file),
            })
            .collect();

        let compiled_chunks = self
            .chunks
            .iter()
            .map(|c| CompiledChunkEntry {
                id: c.id,
                name_id: interner.intern(&c.name),
                scene_id: interner.intern(&c.scene),
                x_min: c.x_min,
                x_max: c.x_max,
                y_min: c.y_min,
                y_max: c.y_max,
            })
            .collect();

        Ok(CompiledWorld {
            strings: (0..interner.len())
                .filter_map(|i| {
                    let id = u32::try_from(i).ok()?;
                    interner.resolve(id).map(String::from)
                })
                .collect(),
            name_id,
            start_scene_id: start_id,
            scenes: compiled_scenes,
            chunks: compiled_chunks,
        })
    }
}

impl CompiledWorld {
    /// resolve an interned string id.
    #[must_use]
    pub fn resolve(&self, id: u32) -> Option<&str> {
        self.strings.get(id as usize).map(String::as_str)
    }

    /// get the world name.
    #[must_use]
    pub fn name(&self) -> &str {
        self.resolve(self.name_id).unwrap_or("<unknown>")
    }

    /// get the start scene name.
    #[must_use]
    pub fn start_scene_name(&self) -> &str {
        self.resolve(self.start_scene_id).unwrap_or("<unknown>")
    }

    /// find a scene entry by interned name id.
    #[must_use]
    pub fn find_scene_by_name(&self, name_id: u32) -> Option<&CompiledSceneEntry> {
        self.scenes.iter().find(|s| s.name_id == name_id)
    }

    /// find the start scene entry.
    #[must_use]
    pub fn start_scene_entry(&self) -> Option<&CompiledSceneEntry> {
        self.scenes
            .iter()
            .find(|s| s.name_id == self.start_scene_id)
    }

    /// find chunks that overlap a given bounding box.
    pub fn chunks_in_bounds(
        &self,
        x_min: f32,
        x_max: f32,
        y_min: f32,
        y_max: f32,
    ) -> Vec<&CompiledChunkEntry> {
        self.chunks
            .iter()
            .filter(|c| c.x_max > x_min && c.x_min < x_max && c.y_max > y_min && c.y_min < y_max)
            .collect()
    }

    /// find chunks within a radius of a center point.
    pub fn chunks_in_radius(&self, cx: f32, cy: f32, radius: f32) -> Vec<&CompiledChunkEntry> {
        let r2 = radius * radius;
        self.chunks
            .iter()
            .filter(|c| {
                let closest_x = cx.clamp(c.x_min, c.x_max);
                let closest_y = cy.clamp(c.y_min, c.y_max);
                let dx = closest_x - cx;
                let dy = closest_y - cy;
                dx * dx + dy * dy <= r2
            })
            .collect()
    }

    /// serialize to binary format using bincode.
    pub fn to_binary(&self) -> Result<Vec<u8>, String> {
        bincode::serialize(self).map_err(|e| format!("failed to serialize compiled world: {e}"))
    }

    /// deserialize from binary format.
    pub fn from_binary(bytes: &[u8]) -> Result<Self, String> {
        bincode::deserialize(bytes)
            .map_err(|e| format!("failed to deserialize compiled world: {e}"))
    }
}

// ── entity format: named component map ────────────────────────────

/// entity definition using a named component map.
///
/// instead of hardcoded fields, entities carry a map of component names
/// to their data. the engine recognizes its own built-in components
/// and passes unknown components through to game code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityData {
    /// optional entity identifier
    pub id: Option<String>,
    /// optional parent entity id (by name)
    pub parent: Option<String>,
    /// named component map: component name → json value
    #[serde(default)]
    pub components: HashMap<String, serde_json::Value>,
}

/// built-in component names recognized by the engine.
/// game code can define additional components freely.
pub mod builtin_components {
    /// component name for local transform data.
    pub const LOCAL_TRANSFORM: &str = "local_transform";
    /// component name for sprite rendering data.
    pub const SPRITE: &str = "sprite";
    /// component name for text rendering data.
    pub const TEXT: &str = "text";
    /// component name for render layer assignment.
    pub const LAYER: &str = "layer";
    /// component name for custom tags.
    pub const TAGS: &str = "tags";
}

impl EntityData {
    /// get a component value by name, deserializing from json.
    pub fn get_component<T: serde::de::DeserializeOwned>(&self, name: &str) -> Option<T> {
        self.components
            .get(name)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    /// set a component value by name, serializing to json.
    pub fn set_component<T: serde::Serialize>(&mut self, name: &str, value: &T) {
        if let Ok(json) = serde_json::to_value(value) {
            self.components.insert(name.to_string(), json);
        }
    }

    /// check if this entity has a specific component.
    #[must_use]
    pub fn has_component(&self, name: &str) -> bool {
        self.components.contains_key(name)
    }
}

/// scene definition using the new component map format.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename = "Scene")]
pub struct ComponentScene {
    /// scene name
    pub name: String,
    /// entity definitions using component maps
    #[serde(default)]
    pub entities: Vec<EntityData>,
}

impl ComponentScene {
    /// parse from a RON string.
    pub fn from_ron(source: &str) -> Result<Self, String> {
        ron::from_str(source).map_err(|e| format!("failed to parse scene ron: {e}"))
    }

    /// load from a RON file path.
    pub fn from_file(path: &str) -> Result<Self, String> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let content = std::fs::read_to_string(path)
                .map_err(|e| format!("failed to read scene file '{path}': {e}"))?;
            Self::from_ron(&content)
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = path;
            Err("file loading not supported on wasm, use bundled assets or fetch API".to_string())
        }
    }

    /// serialize to binary format.
    pub fn to_binary(&self) -> Result<Vec<u8>, String> {
        bincode::serialize(self).map_err(|e| format!("failed to serialize scene: {e}"))
    }

    /// deserialize from binary format.
    pub fn from_binary(bytes: &[u8]) -> Result<Self, String> {
        bincode::deserialize(bytes).map_err(|e| format!("failed to deserialize scene: {e}"))
    }
}

// ── scene loading modes ──────────────────────────────────

/// how a scene should be loaded relative to current state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LoadMode {
    /// unload current scene, then load the new one exclusively.
    Exclusive,
    /// load the new scene additively on top of current scenes.
    Additive,
    /// load spatial chunks within a radius (streaming).
    Streaming {
        /// center x for streaming radius
        center_x: f32,
        /// center y for streaming radius
        center_y: f32,
        /// loading radius in world units
        radius: f32,
    },
}

/// configuration for the streaming scene loader.
#[derive(Debug, Clone, Copy)]
pub struct StreamingConfig {
    /// default radius for streaming
    pub radius: f32,
    /// how often to re-evaluate loaded chunks (in seconds)
    pub update_interval: f32,
}

impl Default for StreamingConfig {
    fn default() -> Self {
        Self {
            radius: 200.0,
            update_interval: 1.0,
        }
    }
}

// ── scene loader with mode support ───────────────────────

/// tracks which chunks are currently loaded for streaming.
#[derive(Debug, Clone, Resource)]
pub struct StreamingState {
    /// currently loaded chunk ids
    pub loaded_chunk_ids: std::collections::HashSet<u32>,
    /// last center position used for streaming
    pub last_center_x: f32,
    pub last_center_y: f32,
    /// config for streaming evaluation
    pub config: StreamingConfig,
    /// time since last streaming evaluation
    pub time_since_eval: f32,
}

impl StreamingState {
    /// create a new streaming state with default config.
    #[must_use]
    pub fn new() -> Self {
        Self {
            loaded_chunk_ids: std::collections::HashSet::new(),
            last_center_x: 0.0,
            last_center_y: 0.0,
            config: StreamingConfig::default(),
            time_since_eval: 0.0,
        }
    }

    /// create with custom config.
    #[must_use]
    pub fn with_config(config: StreamingConfig) -> Self {
        Self {
            loaded_chunk_ids: std::collections::HashSet::new(),
            last_center_x: 0.0,
            last_center_y: 0.0,
            config,
            time_since_eval: 0.0,
        }
    }
}

impl Default for StreamingState {
    fn default() -> Self {
        Self::new()
    }
}

/// resource tracking loaded scenes for unload support.
#[derive(Debug, Resource)]
pub struct LoadedScenes {
    /// map of scene name to spawned entity ids
    pub scene_entity_maps: HashMap<String, HashMap<String, Entity>>,
    /// currently active scene names (for additive tracking)
    pub active_scenes: Vec<String>,
}

impl LoadedScenes {
    /// create a new loaded scenes tracker.
    #[must_use]
    pub fn new() -> Self {
        Self {
            scene_entity_maps: HashMap::new(),
            active_scenes: Vec::new(),
        }
    }
}

impl Default for LoadedScenes {
    fn default() -> Self {
        Self::new()
    }
}

/// advanced scene loader supporting multiple load modes.
pub struct AdvancedSceneLoader;

impl AdvancedSceneLoader {
    /// load a scene using the specified mode.
    /// returns entity id map for the newly loaded scene(s).
    pub fn load(
        commands: &mut Commands,
        world: &mut World,
        scene: &SceneDefinition,
        mode: LoadMode,
    ) -> HashMap<String, Entity> {
        match mode {
            LoadMode::Exclusive => Self::load_exclusive(commands, world, scene),
            LoadMode::Additive => Self::load_additive(commands, world, scene),
            LoadMode::Streaming {
                center_x,
                center_y,
                radius,
            } => Self::load_streaming(commands, world, scene, center_x, center_y, radius),
        }
    }

    /// exclusive load: unload all current scenes, then load the new one.
    fn load_exclusive(
        commands: &mut Commands,
        world: &mut World,
        scene: &SceneDefinition,
    ) -> HashMap<String, Entity> {
        // despawn all entities currently in the world that are scene entities
        Self::despawn_all_scene_entities(commands, world);

        // load the new scene
        let result = crate::scene_format::SceneLoader::spawn_scene(commands, scene, None);

        // track loaded scene
        if let Some(mut loaded) = world.get_resource_mut::<LoadedScenes>() {
            loaded.active_scenes.clear();
            loaded.active_scenes.push(scene.name.clone());
            loaded.scene_entity_maps.clear();
            loaded
                .scene_entity_maps
                .insert(scene.name.clone(), result.clone());
        }

        result
    }

    /// additive load: keep current scenes, layer the new one on top.
    fn load_additive(
        commands: &mut Commands,
        world: &mut World,
        scene: &SceneDefinition,
    ) -> HashMap<String, Entity> {
        let result = crate::scene_format::SceneLoader::spawn_scene(commands, scene, None);

        // track loaded scene
        if let Some(mut loaded) = world.get_resource_mut::<LoadedScenes>() {
            if !loaded.active_scenes.contains(&scene.name) {
                loaded.active_scenes.push(scene.name.clone());
            }
            loaded
                .scene_entity_maps
                .insert(scene.name.clone(), result.clone());
        }

        result
    }

    /// streaming load: load chunks within radius, unload those outside.
    fn load_streaming(
        commands: &mut Commands,
        world: &mut World,
        scene: &SceneDefinition,
        _center_x: f32,
        _center_y: f32,
        _radius: f32,
    ) -> HashMap<String, Entity> {
        // streaming is driven by the world manifest chunk system.
        // this mode loads the scene additively and sets up streaming state.
        let result = crate::scene_format::SceneLoader::spawn_scene(commands, scene, None);

        if let Some(mut loaded) = world.get_resource_mut::<LoadedScenes>() {
            if !loaded.active_scenes.contains(&scene.name) {
                loaded.active_scenes.push(scene.name.clone());
            }
            loaded
                .scene_entity_maps
                .insert(scene.name.clone(), result.clone());
        }

        // ensure streaming state exists
        if !world.contains_resource::<StreamingState>() {
            world.insert_resource(StreamingState::new());
        }

        result
    }

    /// despawn all entities that have the `SceneEntity` component.
    fn despawn_all_scene_entities(commands: &mut Commands, world: &mut World) {
        let scene_entities: Vec<Entity> = world
            .query_filtered::<Entity, With<crate::scene_format::SceneEntity>>()
            .iter(world)
            .collect();

        for entity in scene_entities {
            commands.entity(entity).despawn();
        }
    }

    /// update streaming: evaluate which chunks should be loaded/unloaded.
    pub fn update_streaming(
        _commands: &mut Commands,
        world: &mut World,
        compiled_world: &CompiledWorld,
        center_x: f32,
        center_y: f32,
        delta_time: f32,
    ) {
        let Some(mut streaming) = world.get_resource_mut::<StreamingState>() else {
            return;
        };

        streaming.time_since_eval += delta_time;
        if streaming.time_since_eval < streaming.config.update_interval {
            return;
        }
        streaming.time_since_eval = 0.0;

        // skip if center hasn't changed significantly
        let dx = center_x - streaming.last_center_x;
        let dy = center_y - streaming.last_center_y;
        if dx * dx + dy * dy < 1.0 {
            return;
        }

        streaming.last_center_x = center_x;
        streaming.last_center_y = center_y;

        let radius = streaming.config.radius;
        let chunks_to_load = compiled_world.chunks_in_radius(center_x, center_y, radius);
        let chunks_to_unload: Vec<u32> = streaming
            .loaded_chunk_ids
            .iter()
            .copied()
            .filter(|id| !chunks_to_load.iter().any(|c| c.id == *id))
            .collect();

        // unload chunks (game-specific logic would go here)
        for chunk_id in chunks_to_unload {
            streaming.loaded_chunk_ids.remove(&chunk_id);
            log::info!("streaming: unloaded chunk {chunk_id}");
        }

        // load new chunks (game-specific logic would go here)
        for chunk in chunks_to_load {
            if streaming.loaded_chunk_ids.insert(chunk.id) {
                log::info!("streaming: loaded chunk {}", chunk.id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interner_basic() {
        let mut interner = StringInterner::new();
        let a = interner.intern("hello");
        let b = interner.intern("world");
        let c = interner.intern("hello");
        assert_eq!(a, c);
        assert_ne!(a, b);
        assert_eq!(interner.len(), 2);
        assert_eq!(interner.resolve(a), Some("hello"));
        assert_eq!(interner.resolve(b), Some("world"));
    }

    #[test]
    fn parse_world_manifest_xml() {
        let xml = r#"
<world name="overworld" start="village">
    <scenes>
        <scene id="1" name="village" file="scenes/village.ron" />
        <scene id="2" name="dungeon" file="scenes/dungeon.ron" />
    </scenes>
    <chunks>
        <chunk id="10" name="village_center" scene="village"
               x_min="0" x_max="100" y_min="0" y_max="100" />
    </chunks>
</world>"#;
        let world = WorldManifest::from_xml(xml).unwrap();
        assert_eq!(world.name, "overworld");
        assert_eq!(world.start_scene, "village");
        assert_eq!(world.scenes.len(), 2);
        assert_eq!(world.chunks.len(), 1);
        assert_eq!(world.scenes[0].name, "village");
        assert_eq!(world.chunks[0].name, "village_center");
    }

    #[test]
    fn manifest_find_scene() {
        let xml = r#"
<world name="test" start="a">
    <scenes>
        <scene id="1" name="a" file="a.ron" />
        <scene id="2" name="b" file="b.ron" />
    </scenes>
    <chunks></chunks>
</world>"#;
        let world = WorldManifest::from_xml(xml).unwrap();
        assert!(world.find_scene("b").is_some());
        assert!(world.find_scene("c").is_none());
    }

    #[test]
    fn manifest_chunks_in_bounds() {
        let xml = r#"
<world name="test" start="a">
    <scenes><scene id="1" name="a" file="a.ron" /></scenes>
    <chunks>
        <chunk id="1" name="c1" scene="a" x_min="0" x_max="50" y_min="0" y_max="50" />
        <chunk id="2" name="c2" scene="a" x_min="100" x_max="150" y_min="100" y_max="150" />
    </chunks>
</world>"#;
        let world = WorldManifest::from_xml(xml).unwrap();
        let found = world.chunks_in_bounds(0.0, 60.0, 0.0, 60.0);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "c1");
    }

    #[test]
    fn manifest_chunks_in_radius() {
        let xml = r#"
<world name="test" start="a">
    <scenes><scene id="1" name="a" file="a.ron" /></scenes>
    <chunks>
        <chunk id="1" name="near" scene="a" x_min="0" x_max="20" y_min="0" y_max="20" />
        <chunk id="2" name="far" scene="a" x_min="200" x_max="220" y_min="200" y_max="220" />
    </chunks>
</world>"#;
        let world = WorldManifest::from_xml(xml).unwrap();
        let found = world.chunks_in_radius(10.0, 10.0, 50.0);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "near");
    }

    #[test]
    fn compile_world_roundtrip() {
        let xml = r#"
<world name="overworld" start="village">
    <scenes>
        <scene id="1" name="village" file="scenes/village.ron" />
    </scenes>
    <chunks>
        <chunk id="10" name="center" scene="village"
               x_min="0" x_max="100" y_min="0" y_max="100" />
    </chunks>
</world>"#;
        let manifest = WorldManifest::from_xml(xml).unwrap();
        let compiled = manifest.compile().unwrap();
        assert_eq!(compiled.name(), "overworld");
        assert_eq!(compiled.start_scene_name(), "village");
        assert_eq!(compiled.scenes.len(), 1);
        assert_eq!(
            compiled.resolve(compiled.scenes[0].name_id),
            Some("village")
        );

        let binary = compiled.to_binary().unwrap();
        let restored = CompiledWorld::from_binary(&binary).unwrap();
        assert_eq!(restored.name(), "overworld");
        assert_eq!(restored.scenes[0].id, 1);
    }

    #[test]
    fn entity_data_component_map() {
        let mut entity = EntityData {
            id: Some("player".to_string()),
            parent: None,
            components: HashMap::new(),
        };

        entity.set_component(&builtin_components::LAYER, &1);
        assert!(entity.has_component(builtin_components::LAYER));
        assert_eq!(
            entity.get_component::<i32>(builtin_components::LAYER),
            Some(1)
        );
    }

    #[test]
    fn component_scene_roundtrip() {
        let scene = ComponentScene {
            name: "test".to_string(),
            entities: vec![EntityData {
                id: Some("e1".to_string()),
                parent: None,
                components: HashMap::new(),
            }],
        };
        let binary = scene.to_binary().unwrap();
        let restored = ComponentScene::from_binary(&binary).unwrap();
        assert_eq!(restored.name, "test");
        assert_eq!(restored.entities[0].id.as_deref(), Some("e1"));
    }
}
