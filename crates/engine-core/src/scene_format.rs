//! scene definition format: TOML authoring and binary runtime.
//!
//! # authoring format (TOML)
//!
//! scenes are authored as TOML files with entity definitions:
//!
//! ```toml
//! name = "level_1"
//!
//! [[entities]]
//! id = "player"
//! x = 100.0
//! y = 200.0
//! layer = 1
//! sprite_texture = "player.png"
//! sprite_width = 32.0
//! sprite_height = 32.0
//!
//! [[entities]]
//! id = "enemy"
//! parent = "player"
//! x = 50.0
//! y = 0.0
//! layer = 1
//! sprite_texture = "enemy.png"
//! sprite_width = 24.0
//! sprite_height = 24.0
//! ```
//!
//! # binary runtime
//!
//! at build time, TOML scenes are converted to a compact binary format
//! using bincode for fast loading at runtime.

use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use engine_math::{Color, LocalTransform, Vec2, Vec3};

use crate::hierarchy::{Children, Parent};

/// authoring-time scene definition (TOML format).
///
/// use [`SceneDefinition::from_toml`] to parse from a TOML string.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// optional parent entity id
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sprite_texture: Option<String>,
    /// sprite width (required if sprite_texture is set)
    #[serde(default)]
    pub sprite_width: f32,
    /// sprite height (required if sprite_texture is set)
    #[serde(default)]
    pub sprite_height: f32,
    /// optional sprite tint color (hex string like "#ff0000")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sprite_tint: Option<String>,
    /// optional atlas region name
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sprite_region: Option<String>,
    /// optional text content
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// text font size
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    /// optional font path
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    /// optional text color
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_color: Option<String>,
    /// render layer (default 0)
    #[serde(default)]
    pub layer: i32,
    /// optional custom tags
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
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
    /// parse a scene from a TOML string.
    pub fn from_toml(source: &str) -> Result<Self, String> {
        toml::from_str(source).map_err(|e| format!("failed to parse scene toml: {e}"))
    }

    /// load a scene from a TOML file path.
    pub fn from_file(path: &str) -> Result<Self, String> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let content = std::fs::read_to_string(path)
                .map_err(|e| format!("failed to read scene file '{path}': {e}"))?;
            Self::from_toml(&content)
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = path;
            Err("file loading not supported on wasm, use bundled assets or fetch API".to_string())
        }
    }

    /// serialize the scene to binary format using bincode.
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

/// component storing the raw custom data from the scene definition.
#[derive(Debug, Clone, Component)]
pub struct SceneData(pub Option<serde_json::Value>);

impl SceneLoader {
    /// spawn all entities from a scene definition into the world.
    /// returns a map of entity ids (from the scene file) to spawned [`Entity`] handles.
    pub fn spawn_scene(
        commands: &mut Commands,
        scene: &SceneDefinition,
    ) -> HashMap<String, Entity> {
        let mut id_map: HashMap<String, Entity> = HashMap::new();
        let mut parent_refs: Vec<(Entity, String)> = Vec::new();

        // first pass: spawn entities and store components
        for entity_def in &scene.entities {
            let mut spawn = commands.spawn((
                LocalTransform {
                    translation: Vec3::new(entity_def.x, entity_def.y, 0.0),
                    rotation: entity_def.rotation,
                    scale: Vec2::new(entity_def.scale_x, entity_def.scale_y),
                },
                engine_math::WorldTransform::default(),
                SceneLayer(entity_def.layer),
                SceneEntity {
                    scene_name: scene.name.clone(),
                    entity_id: entity_def.id.clone(),
                },
            ));

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

        // second pass: resolve parent references and add Parent/Children components
        for (entity, parent_id) in parent_refs {
            if let Some(&parent_entity) = id_map.get(&parent_id) {
                commands.entity(entity).insert(Parent(parent_entity));
                commands
                    .entity(parent_entity)
                    .insert(Children(smallvec::smallvec![entity]));
            } else {
                log::warn!("SceneLoader: parent '{parent_id}' not found for entity");
            }
        }

        id_map
    }

    /// load and spawn a scene from a TOML file path.
    pub fn load_and_spawn(
        commands: &mut Commands,
        path: &str,
    ) -> Result<HashMap<String, Entity>, String> {
        let scene = SceneDefinition::from_file(path)?;
        Ok(Self::spawn_scene(commands, &scene))
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
fn parse_hex_color(hex: &str) -> Option<Color> {
    let hex = hex.trim_start_matches('#');
    let (r, g, b, a) = match hex.len() {
        3 => {
            let r = u8::from_str_radix(&hex[0..1].repeat(2), 16).ok()?;
            let g = u8::from_str_radix(&hex[1..2].repeat(2), 16).ok()?;
            let b = u8::from_str_radix(&hex[2..3].repeat(2), 16).ok()?;
            (r, g, b, 255)
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
        r as f32 / 255.0,
        g as f32 / 255.0,
        b as f32 / 255.0,
        a as f32 / 255.0,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_scene() {
        let toml = r#"name = "empty""#;
        let scene = SceneDefinition::from_toml(toml).unwrap();
        assert_eq!(scene.name, "empty");
        assert!(scene.entities.is_empty());
    }

    #[test]
    fn parse_scene_with_entity() {
        let toml = r#"
name = "test"

[[entities]]
id = "player"
x = 100.0
y = 200.0
sprite_texture = "player.png"
sprite_width = 32.0
sprite_height = 32.0
layer = 1
"#;
        let scene = SceneDefinition::from_toml(toml).unwrap();
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
        let toml = r#"
name = "hierarchy"

[[entities]]
id = "parent"
x = 100.0
y = 100.0

[[entities]]
id = "child"
parent = "parent"
x = 10.0
y = 0.0
"#;
        let scene = SceneDefinition::from_toml(toml).unwrap();
        assert_eq!(scene.entities.len(), 2);
        assert_eq!(scene.entities[1].parent.as_deref(), Some("parent"));
    }

    #[test]
    fn binary_roundtrip() {
        let toml = r##"
name = "roundtrip"

[[entities]]
id = "e1"
x = 10.0
y = 20.0
rotation = 1.5
scale_x = 2.0
scale_y = 2.0
sprite_texture = "tex.png"
sprite_width = 16.0
sprite_height = 16.0
sprite_tint = "#ff0000"
layer = 2
tags = ["enemy"]
"##;
        let original = SceneDefinition::from_toml(toml).unwrap();
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
