//! scene definition format: RON authoring and binary runtime.
#![allow(clippy::cast_precision_loss)]
//!
//! # authoring format (RON)
//!
//! scenes are authored as RON files with entity definitions:
//!
//! ```ron
//! Scene(
//!     name: "level_1",
//!     entities: [
//!         (
//!             id: Some("player"),
//!             x: 100.0,
//!             y: 200.0,
//!             sprite_texture: Some("player.png"),
//!             sprite_width: 32.0,
//!             sprite_height: 32.0,
//!             layer: 1,
//!         ),
//!         (
//!             id: Some("enemy"),
//!             parent: Some("player"),
//!             x: 50.0,
//!             sprite_texture: Some("enemy.png"),
//!             sprite_width: 24.0,
//!             sprite_height: 24.0,
//!         ),
//!     ],
//! )
//! ```
//!
//! # binary runtime
//!
//! at build time, RON scenes are converted to a compact binary format
//! using bincode for fast loading at runtime.

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use rustc_hash::FxHashMap as HashMap;

use lunar_math::{Color, LocalTransform, Vec2};

use crate::hierarchy::{Children, Parent};

/// authoring-time scene definition (RON format).
///
/// use [`SceneDefinition::from_ron`] to parse from a RON string.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename = "Scene")]
pub struct SceneDefinition {
    /// scene name
    pub name: String,
    /// entity definitions
    #[serde(default)]
    pub entities: Vec<EntityDefinition>,
}

/// authoring-time entity definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityDefinition {
    /// optional entity identifier for referencing
    pub id: Option<String>,
    /// optional parent entity id
    pub parent: Option<String>,
    /// x position
    #[serde(default)]
    pub x: f32,
    /// y position
    #[serde(default)]
    pub y: f32,
    /// rotation in radians
    #[serde(default)]
    pub rotation: f32,
    /// x scale
    #[serde(default = "default_one")]
    pub scale_x: f32,
    /// y scale
    #[serde(default = "default_one")]
    pub scale_y: f32,
    /// optional sprite texture path
    pub sprite_texture: Option<String>,
    /// sprite width (required if `sprite_texture` is set)
    #[serde(default)]
    pub sprite_width: f32,
    /// sprite height (required if `sprite_texture` is set)
    #[serde(default)]
    pub sprite_height: f32,
    /// optional sprite tint color (hex string like "#ff0000")
    pub sprite_tint: Option<String>,
    /// optional atlas region name
    pub sprite_region: Option<String>,
    /// optional text content
    pub text: Option<String>,
    /// text font size
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    /// optional font path
    pub font: Option<String>,
    /// optional text color
    pub text_color: Option<String>,
    /// render layer (default 0)
    #[serde(default)]
    pub layer: i32,
    /// optional custom tags
    #[serde(default)]
    pub tags: Vec<String>,
    /// optional sub-scene to instance (nest another scene under this entity)
    pub sub_scene: Option<String>,
}

fn default_one() -> f32 {
    1.0
}

fn default_font_size() -> f32 {
    16.0
}

/// sprite definition for runtime use.
#[derive(Debug, Clone)]
pub struct SpriteDef {
    pub texture: String,
    pub width: f32,
    pub height: f32,
    pub tint: Color,
    pub region: Option<String>,
}

/// text definition for runtime use.
#[derive(Debug, Clone)]
pub struct TextDef {
    pub content: String,
    pub font_size: f32,
    pub font: Option<String>,
    pub color: Color,
}

/// transform definition for runtime use.
#[derive(Debug, Clone, Copy)]
pub struct TransformDef {
    pub x: f32,
    pub y: f32,
    pub rotation: f32,
    pub scale_x: f32,
    pub scale_y: f32,
}

impl EntityDefinition {
    /// get the transform for this entity.
    #[must_use]
    pub const fn transform(&self) -> TransformDef {
        TransformDef {
            x: self.x,
            y: self.y,
            rotation: self.rotation,
            scale_x: self.scale_x,
            scale_y: self.scale_y,
        }
    }

    /// get the sprite definition if present.
    #[must_use]
    pub fn sprite(&self) -> Option<SpriteDef> {
        self.sprite_texture.as_ref().map(|texture| SpriteDef {
            texture: texture.clone(),
            width: self.sprite_width,
            height: self.sprite_height,
            tint: self
                .sprite_tint
                .as_ref()
                .and_then(|s| parse_hex_color(s))
                .unwrap_or(Color::WHITE),
            region: self.sprite_region.clone(),
        })
    }

    /// get the text definition if present.
    #[must_use]
    pub fn text_def(&self) -> Option<TextDef> {
        self.text.as_ref().map(|content| TextDef {
            content: content.clone(),
            font_size: self.font_size,
            font: self.font.clone(),
            color: self
                .text_color
                .as_ref()
                .and_then(|s| parse_hex_color(s))
                .unwrap_or(Color::WHITE),
        })
    }
}

impl SceneDefinition {
    /// parse a scene from a RON string.
    /// # Errors
    /// returns an error if the RON string fails to parse.
    pub fn from_ron(source: &str) -> Result<Self, String> {
        ron::from_str(source).map_err(|e| format!("failed to parse scene ron: {e}"))
    }

    /// load a scene from a RON file path.
    /// # Errors
    /// returns an error if the file cannot be read or parsed.
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

    /// serialize the scene to binary format using bincode.
    /// # Errors
    /// returns an error if serialization fails.
    pub fn to_binary(&self) -> Result<Vec<u8>, String> {
        let binary = BinaryScene {
            name: self.name.clone(),
            entities: self
                .entities
                .iter()
                .map(EntityDefinition::to_binary)
                .collect(),
        };
        bincode::serialize(&binary).map_err(|e| format!("failed to serialize scene: {e}"))
    }

    /// deserialize a scene from binary format.
    /// # Errors
    /// returns an error if deserialization fails.
    pub fn from_binary(bytes: &[u8]) -> Result<Self, String> {
        let binary: BinaryScene =
            bincode::deserialize(bytes).map_err(|e| format!("failed to deserialize scene: {e}"))?;
        Ok(Self {
            name: binary.name,
            entities: binary
                .entities
                .into_iter()
                .map(BinaryEntityDefinition::into_authoring)
                .collect(),
        })
    }
}

/// binary runtime scene format (compact, fast to load).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct BinaryScene {
    name: String,
    entities: Vec<BinaryEntityDefinition>,
}

/// binary runtime entity definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct BinaryEntityDefinition {
    id: Option<String>,
    parent: Option<String>,
    x: f32,
    y: f32,
    rotation: f32,
    scale_x: f32,
    scale_y: f32,
    sprite_texture: Option<String>,
    sprite_width: f32,
    sprite_height: f32,
    sprite_tint: Option<String>,
    sprite_region: Option<String>,
    text: Option<String>,
    font_size: f32,
    font: Option<String>,
    text_color: Option<String>,
    layer: i32,
    tags: Vec<String>,
    sub_scene: Option<String>,
}

impl EntityDefinition {
    fn to_binary(&self) -> BinaryEntityDefinition {
        BinaryEntityDefinition {
            id: self.id.clone(),
            parent: self.parent.clone(),
            x: self.x,
            y: self.y,
            rotation: self.rotation,
            scale_x: self.scale_x,
            scale_y: self.scale_y,
            sprite_texture: self.sprite_texture.clone(),
            sprite_width: self.sprite_width,
            sprite_height: self.sprite_height,
            sprite_tint: self.sprite_tint.clone(),
            sprite_region: self.sprite_region.clone(),
            text: self.text.clone(),
            font_size: self.font_size,
            font: self.font.clone(),
            text_color: self.text_color.clone(),
            layer: self.layer,
            tags: self.tags.clone(),
            sub_scene: self.sub_scene.clone(),
        }
    }
}

impl BinaryEntityDefinition {
    fn into_authoring(self) -> EntityDefinition {
        EntityDefinition {
            id: self.id,
            parent: self.parent,
            x: self.x,
            y: self.y,
            rotation: self.rotation,
            scale_x: self.scale_x,
            scale_y: self.scale_y,
            sprite_texture: self.sprite_texture,
            sprite_width: self.sprite_width,
            sprite_height: self.sprite_height,
            sprite_tint: self.sprite_tint,
            sprite_region: self.sprite_region,
            text: self.text,
            font_size: self.font_size,
            font: self.font,
            text_color: self.text_color,
            layer: self.layer,
            tags: self.tags,
            sub_scene: self.sub_scene,
        }
    }
}

/// scene loader: spawns entities from a scene definition.
///
/// use [`SceneLoader::spawn_scene`] to load a scene into the world.
pub struct SceneLoader;

/// marker component for entities spawned from a scene.
#[derive(Debug, Clone, Component)]
pub struct SceneEntity {
    /// the scene this entity belongs to
    pub scene_name: String,
    /// optional entity id from the scene file
    pub entity_id: Option<String>,
}

/// marker component for entities that instance a sub-scene.
/// the sub-scene's root entities are spawned as children of this entity.
#[derive(Debug, Clone, Component)]
pub struct SceneInstance {
    /// path or name of the instanced sub-scene
    pub scene_path: String,
}

/// component storing the raw custom data from the scene definition.
#[derive(Debug, Clone, Component)]
pub struct SceneData(pub Option<serde_json::Value>);

impl SceneLoader {
    /// spawn all entities from a scene definition into the world.
    /// returns a map of entity ids (from the scene file) to spawned [`Entity`] handles.
    /// sub-scene references are resolved via the provided scene registry (name → `SceneDefinition`).
    pub fn spawn_scene(
        commands: &mut Commands,
        scene: &SceneDefinition,
        scene_registry: Option<&HashMap<String, SceneDefinition>>,
    ) -> HashMap<String, Entity> {
        Self::spawn_scene_internal(commands, scene, scene_registry, None)
    }

    fn spawn_scene_internal(
        commands: &mut Commands,
        scene: &SceneDefinition,
        scene_registry: Option<&HashMap<String, SceneDefinition>>,
        parent_entity: Option<Entity>,
    ) -> HashMap<String, Entity> {
        let mut id_map: HashMap<String, Entity> = HashMap::default();
        let mut parent_refs: Vec<(Entity, String)> = Vec::new();
        let mut sub_scene_roots: Vec<(Entity, String)> = Vec::new();

        // first pass: spawn entities and store components
        for entity_def in &scene.entities {
            let mut spawn = commands.spawn((
                LocalTransform {
                    translation: Vec2::new(entity_def.x, entity_def.y),
                    rotation: entity_def.rotation,
                    scale: Vec2::new(entity_def.scale_x, entity_def.scale_y),
                },
                lunar_math::WorldTransform::default(),
                SceneLayer(entity_def.layer),
                SceneEntity {
                    scene_name: scene.name.clone(),
                    entity_id: entity_def.id.clone(),
                },
            ));

            // add sub-scene instance if present
            if let Some(ref sub_scene) = entity_def.sub_scene {
                sub_scene_roots.push((spawn.id(), sub_scene.clone()));
            }

            // add sprite if present
            if let Some(sprite) = entity_def.sprite() {
                spawn.insert(SceneSprite {
                    texture: sprite.texture,
                    width: sprite.width,
                    height: sprite.height,
                    tint: sprite.tint,
                    region: sprite.region,
                });
            }

            // add text if present
            if let Some(text) = entity_def.text_def() {
                spawn.insert(SceneText {
                    content: text.content,
                    font_size: text.font_size,
                    font: text.font,
                    color: text.color,
                });
            }

            // add tags
            if !entity_def.tags.is_empty() {
                spawn.insert(SceneTags(entity_def.tags.clone()));
            }

            let entity = spawn.id();

            // store id mapping
            if let Some(ref id) = entity_def.id {
                id_map.insert(id.clone(), entity);
            }

            // store parent reference for second pass
            if let Some(ref parent_id) = entity_def.parent {
                parent_refs.push((entity, parent_id.clone()));
            }
        }

        // second pass: resolve parent references — group children per parent, insert once
        let mut parent_to_children: HashMap<Entity, smallvec::SmallVec<[Entity; 4]>> =
            HashMap::default();
        for (entity, parent_id) in parent_refs {
            if let Some(&parent_entity) = id_map.get(&parent_id) {
                commands.entity(entity).insert(Parent(parent_entity));
                parent_to_children
                    .entry(parent_entity)
                    .or_default()
                    .push(entity);
            } else {
                log::warn!("SceneLoader: parent '{parent_id}' not found for entity");
            }
        }
        for (parent_entity, children) in parent_to_children {
            commands.entity(parent_entity).insert(Children(children));
        }

        // third pass: resolve sub-scene instances
        for (entity, sub_scene_name) in sub_scene_roots {
            if let Some(registry) = scene_registry
                && let Some(sub_scene) = registry.get(&sub_scene_name)
            {
                commands.entity(entity).insert(SceneInstance {
                    scene_path: sub_scene_name.clone(),
                });
                let sub_id_map =
                    Self::spawn_scene_internal(commands, sub_scene, Some(registry), Some(entity));
                // parent all sub-scene root entities under this entity in one Children insert
                let mut sub_children: smallvec::SmallVec<[Entity; 4]> = smallvec::SmallVec::new();
                for sub_entity in sub_id_map.values() {
                    commands.entity(*sub_entity).insert(Parent(entity));
                    sub_children.push(*sub_entity);
                }
                if !sub_children.is_empty() {
                    commands.entity(entity).insert(Children(sub_children));
                }
            } else {
                log::warn!("SceneLoader: sub-scene '{sub_scene_name}' not found in registry");
            }
        }

        // if this scene was spawned under a parent, parent all root entities in one Children insert
        if let Some(parent) = parent_entity {
            let mut root_children: smallvec::SmallVec<[Entity; 4]> = smallvec::SmallVec::new();
            for entity in id_map.values() {
                commands.entity(*entity).insert(Parent(parent));
                root_children.push(*entity);
            }
            if !root_children.is_empty() {
                commands.entity(parent).insert(Children(root_children));
            }
        }

        id_map
    }

    /// load and spawn a scene from a RON file path.
    ///
    /// # Errors
    ///
    /// returns an error if the file cannot be read or parsed.
    pub fn load_and_spawn(
        commands: &mut Commands,
        path: &str,
        scene_registry: Option<&HashMap<String, SceneDefinition>>,
    ) -> Result<HashMap<String, Entity>, String> {
        let scene = SceneDefinition::from_file(path)?;
        Ok(Self::spawn_scene(commands, &scene, scene_registry))
    }
}

/// component for scene-defined sprites.
#[derive(Debug, Clone, Component)]
pub struct SceneSprite {
    pub texture: String,
    pub width: f32,
    pub height: f32,
    pub tint: Color,
    pub region: Option<String>,
}

/// component for scene-defined text.
#[derive(Debug, Clone, Component)]
pub struct SceneText {
    pub content: String,
    pub font_size: f32,
    pub font: Option<String>,
    pub color: Color,
}

/// component for scene-defined tags.
#[derive(Debug, Clone, Component)]
pub struct SceneTags(pub Vec<String>);

/// component for scene-defined render layer.
#[derive(Debug, Clone, Copy, Component)]
pub struct SceneLayer(pub i32);

/// parse a hex color string like "#ff0000" or "#f00" into a Color.
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
fn parse_hex_color(hex: &str) -> Option<Color> {
    let hex = hex.trim_start_matches('#');
    let (r, g, b, a) = match hex.len() {
        3 => {
            // duplicate each nibble: #rgb → #rrggbb without allocating
            let r = u8::from_str_radix(&hex[0..1], 16).ok()?;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()?;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()?;
            ((r << 4) | r, (g << 4) | g, (b << 4) | b, 255)
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            (r, g, b, 255)
        }
        8 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
            (r, g, b, a)
        }
        _ => return None,
    };
    Some(Color::rgba(
        f32::from(r) / 255.0,
        f32::from(g) / 255.0,
        f32::from(b) / 255.0,
        f32::from(a) / 255.0,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_scene() {
        let ron = r#"Scene(name: "empty")"#;
        let scene = SceneDefinition::from_ron(ron).unwrap();
        assert_eq!(scene.name, "empty");
        assert!(scene.entities.is_empty());
    }

    #[test]
    fn parse_scene_with_entity() {
        let ron = r##"
Scene(
    name: "test",
    entities: [
        (
            id: Some("player"),
            x: 100.0,
            y: 200.0,
            sprite_texture: Some("player.png"),
            sprite_width: 32.0,
            sprite_height: 32.0,
            layer: 1,
        ),
    ],
)
"##;
        let scene = SceneDefinition::from_ron(ron).unwrap();
        assert_eq!(scene.name, "test");
        assert_eq!(scene.entities.len(), 1);
        let entity = &scene.entities[0];
        assert_eq!(entity.id.as_deref(), Some("player"));
        assert_eq!(entity.x, 100.0);
        assert_eq!(entity.y, 200.0);
        assert_eq!(entity.sprite_texture.as_deref(), Some("player.png"));
        assert_eq!(entity.layer, 1);
    }

    #[test]
    fn parse_scene_with_parent() {
        let ron = r##"
Scene(
    name: "hierarchy",
    entities: [
        (id: Some("parent"), x: 100.0, y: 100.0),
        (id: Some("child"), parent: Some("parent"), x: 10.0, y: 0.0),
    ],
)
"##;
        let scene = SceneDefinition::from_ron(ron).unwrap();
        assert_eq!(scene.entities.len(), 2);
        assert_eq!(scene.entities[1].parent.as_deref(), Some("parent"));
    }

    #[test]
    fn binary_roundtrip() {
        let ron = r##"
Scene(
    name: "roundtrip",
    entities: [
        (
            id: Some("e1"),
            x: 10.0,
            y: 20.0,
            rotation: 1.5,
            scale_x: 2.0,
            scale_y: 2.0,
            sprite_texture: Some("tex.png"),
            sprite_width: 16.0,
            sprite_height: 16.0,
            sprite_tint: Some("#ff0000"),
            layer: 2,
            tags: ["enemy"],
        ),
    ],
)
"##;
        let original = SceneDefinition::from_ron(ron).unwrap();
        let binary = original.to_binary().unwrap();
        let restored = SceneDefinition::from_binary(&binary).unwrap();
        assert_eq!(restored.name, original.name);
        assert_eq!(restored.entities.len(), original.entities.len());
        let e = &restored.entities[0];
        assert_eq!(e.x, 10.0);
        assert_eq!(e.rotation, 1.5);
        assert_eq!(e.sprite_tint.as_deref(), Some("#ff0000"));
        assert_eq!(e.tags, vec!["enemy"]);
    }

    #[test]
    fn parse_hex_color_variants() {
        assert_eq!(parse_hex_color("#fff"), Some(Color::WHITE));
        assert_eq!(parse_hex_color("#ffffff"), Some(Color::WHITE));
        assert_eq!(parse_hex_color("#ffffffff"), Some(Color::WHITE));
        assert_eq!(parse_hex_color("#f00"), Some(Color::RED));
        assert_eq!(parse_hex_color("#ff0000"), Some(Color::RED));
        assert!(parse_hex_color("invalid").is_none());
    }
}
