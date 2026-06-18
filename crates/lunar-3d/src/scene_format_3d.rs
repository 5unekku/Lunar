//! 3D scene definition format: RON authoring and binary runtime.
//!
//! # authoring format (`.ls3`)
//!
//! ```ron
//! Scene3d(
//!     name: "level_1",
//!     entities: [
//!         (
//!             id: Some("camera"),
//!             position: (0.0, 5.0, -10.0),
//!             rotation: (0.0, 0.0, 0.0),
//!             camera: Some((fov_y: 60.0, near: 0.1, far: 1000.0)),
//!         ),
//!         (
//!             id: Some("ground"),
//!             scale: (10.0, 0.1, 10.0),
//!             mesh: Some(Primitive("cube")),
//!             material: Some((base_color: "#888888", roughness: 0.9, metallic: 0.0)),
//!         ),
//!         (
//!             id: Some("sun"),
//!             rotation: (-45.0, 30.0, 0.0),
//!             directional_light: Some((color: "#fffae8", illuminance: 10000.0)),
//!         ),
//!     ],
//! )
//! ```
//!
//! rotation is euler angles in degrees applied XYZ order. position and scale default
//! to (0,0,0) and (1,1,1) respectively. built-in primitive names: `"cube"`, `"sphere"`,
//! `"plane"`, `"cylinder"`.

#![allow(clippy::cast_precision_loss)]

use bevy_ecs::prelude::*;
use rustc_hash::FxHashMap as HashMap;
use serde::{Deserialize, Serialize};

use lunar_core::{Children, Parent};
use lunar_math::{Color, Quat, Vec3};

use crate::camera::{Camera3d, Projection};
use crate::light::{DirectionalLight, PointLight, SpotLight};
use crate::material::{Material3d, MaterialData, ShadingModel};
use crate::mesh::Mesh3d;
use crate::mesh_registry::MeshRegistry;
use crate::transform::{LocalTransform3d, WorldTransform3d};
use crate::visibility::{ComputedVisibility, RenderLayers, Visibility};

// ── authoring types ───────────────────────────────────────────────────────────

/// authoring-time 3D scene definition (`.ls3` RON format, also used for binary).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename = "Scene3d")]
pub struct SceneDefinition3d {
    pub name: String,
    #[serde(default)]
    pub entities: Vec<EntityDefinition3d>,
}

/// authoring-time 3D entity definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityDefinition3d {
    /// optional identifier for parenting and editor selection
    pub id: Option<String>,
    /// id of the parent entity
    pub parent: Option<String>,
    /// position in local space
    #[serde(default)]
    pub position: (f32, f32, f32),
    /// rotation as euler angles in degrees, applied XYZ order
    #[serde(default)]
    pub rotation: (f32, f32, f32),
    /// scale in local space
    #[serde(default = "default_scale")]
    pub scale: (f32, f32, f32),
    /// mesh: built-in primitive or an asset file path
    pub mesh: Option<MeshRef>,
    /// material parameters (used when `mesh` is set)
    pub material: Option<MaterialDef>,
    /// camera component
    pub camera: Option<CameraDef>,
    /// directional light
    pub directional_light: Option<DirectionalLightDef>,
    /// point light
    pub point_light: Option<PointLightDef>,
    /// spot light
    pub spot_light: Option<SpotLightDef>,
    /// path to another `.ls3` scene to instance as children
    pub sub_scene: Option<String>,
    /// custom tags
    #[serde(default)]
    pub tags: Vec<String>,
}

/// reference to a mesh.
///
/// built-in names: `"cube"`, `"sphere"`, `"plane"`, `"cylinder"`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MeshRef {
    /// procedurally generated primitive by name
    Primitive(String),
    /// path to a mesh asset file (e.g. `"meshes/barrel.glb"`)
    Asset(String),
}

/// material parameters for a scene entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaterialDef {
    /// albedo hex color (`"#rrggbb"` or `"#rrggbbaa"`)
    #[serde(default = "default_white")]
    pub base_color: String,
    /// perceptual roughness [0, 1]
    #[serde(default = "default_roughness")]
    pub roughness: f32,
    /// metallic factor [0, 1]
    #[serde(default)]
    pub metallic: f32,
    /// emissive hex color; absent = no emission
    pub emissive: Option<String>,
}

impl Default for MaterialDef {
    fn default() -> Self {
        Self {
            base_color: "#ffffff".to_string(),
            roughness: 0.5,
            metallic: 0.0,
            emissive: None,
        }
    }
}

/// perspective camera parameters.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CameraDef {
    /// vertical field of view in degrees
    #[serde(default = "default_fov")]
    pub fov_y: f32,
    #[serde(default = "default_near")]
    pub near: f32,
    #[serde(default = "default_far")]
    pub far: f32,
}

/// directional light parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectionalLightDef {
    #[serde(default = "default_white")]
    pub color: String,
    /// illuminance in lux; 80_000 ≈ full sun
    #[serde(default = "default_illuminance")]
    pub illuminance: f32,
}

/// point light parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PointLightDef {
    #[serde(default = "default_white")]
    pub color: String,
    /// luminous intensity in candela
    #[serde(default = "default_intensity")]
    pub intensity: f32,
    /// world-space falloff radius
    #[serde(default = "default_range")]
    pub range: f32,
}

/// spot light parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotLightDef {
    #[serde(default = "default_white")]
    pub color: String,
    #[serde(default = "default_intensity")]
    pub intensity: f32,
    #[serde(default = "default_range")]
    pub range: f32,
    /// inner cone half-angle in degrees
    #[serde(default = "default_inner_angle")]
    pub inner_angle: f32,
    /// outer cone half-angle in degrees
    #[serde(default = "default_outer_angle")]
    pub outer_angle: f32,
}

// ── serde defaults ────────────────────────────────────────────────────────────

fn default_scale() -> (f32, f32, f32) { (1.0, 1.0, 1.0) }
fn default_white() -> String { "#ffffff".to_string() }
fn default_roughness() -> f32 { 0.5 }
fn default_fov() -> f32 { 60.0 }
fn default_near() -> f32 { 0.1 }
fn default_far() -> f32 { 1000.0 }
fn default_illuminance() -> f32 { 80_000.0 }
fn default_intensity() -> f32 { 800.0 }
fn default_range() -> f32 { 20.0 }
fn default_inner_angle() -> f32 { 22.5 }
fn default_outer_angle() -> f32 { 45.0 }

// ── I/O ───────────────────────────────────────────────────────────────────────

impl SceneDefinition3d {
    /// parse a scene from a RON string.
    /// # Errors
    /// returns an error if parsing fails.
    pub fn from_ron(source: &str) -> Result<Self, String> {
        ron::from_str(source).map_err(|e| format!("failed to parse scene3d ron: {e}"))
    }

    /// load a scene from a `.ls3` file.
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
            Err("file loading not supported on wasm".to_string())
        }
    }

    /// serialize to binary using bincode.
    /// # Errors
    /// returns an error if serialization fails.
    pub fn to_binary(&self) -> Result<Vec<u8>, String> {
        bincode::serialize(self).map_err(|e| format!("failed to serialize scene3d: {e}"))
    }

    /// deserialize from binary.
    /// # Errors
    /// returns an error if deserialization fails.
    pub fn from_binary(bytes: &[u8]) -> Result<Self, String> {
        bincode::deserialize(bytes).map_err(|e| format!("failed to deserialize scene3d: {e}"))
    }
}

// ── marker components ─────────────────────────────────────────────────────────

/// marker component for entities spawned from a `.ls3` scene.
#[derive(Debug, Clone, Component)]
pub struct SceneEntity3d {
    pub scene_name: String,
    pub entity_id: Option<String>,
}

/// marker component for scene entity tags.
#[derive(Debug, Clone, Component)]
pub struct SceneTags3d(pub Vec<String>);

// ── loader ────────────────────────────────────────────────────────────────────

/// spawns entities from a [`SceneDefinition3d`] into the ECS world.
pub struct SceneLoader3d;

impl SceneLoader3d {
    /// spawn all entities from a scene definition into the world.
    ///
    /// returns a map of entity ids (from the scene file) to spawned [`Entity`] handles.
    /// `registry` is used to register procedural meshes and materials.
    pub fn spawn_scene(
        commands: &mut Commands,
        registry: &mut MeshRegistry,
        scene: &SceneDefinition3d,
        scene_registry: Option<&HashMap<String, SceneDefinition3d>>,
    ) -> HashMap<String, Entity> {
        Self::spawn_internal(commands, registry, scene, scene_registry, None)
    }

    /// load a scene from a `.ls3` file and spawn it into the world.
    /// # Errors
    /// returns an error if the file cannot be read or parsed.
    pub fn load_and_spawn(
        commands: &mut Commands,
        registry: &mut MeshRegistry,
        path: &str,
        scene_registry: Option<&HashMap<String, SceneDefinition3d>>,
    ) -> Result<HashMap<String, Entity>, String> {
        let scene = SceneDefinition3d::from_file(path)?;
        Ok(Self::spawn_scene(commands, registry, &scene, scene_registry))
    }

    fn spawn_internal(
        commands: &mut Commands,
        registry: &mut MeshRegistry,
        scene: &SceneDefinition3d,
        scene_registry: Option<&HashMap<String, SceneDefinition3d>>,
        parent_entity: Option<Entity>,
    ) -> HashMap<String, Entity> {
        let mut id_map: HashMap<String, Entity> = HashMap::default();
        let mut parent_refs: Vec<(Entity, String)> = Vec::new();
        let mut sub_scene_roots: Vec<(Entity, String)> = Vec::new();

        for def in &scene.entities {
            let local = local_transform(def);
            let marker = SceneEntity3d {
                scene_name: scene.name.clone(),
                entity_id: def.id.clone(),
            };

            let mut spawn = commands.spawn((local, WorldTransform3d::default(), marker));

            if !def.tags.is_empty() {
                spawn.insert(SceneTags3d(def.tags.clone()));
            }

            if let Some(ref sub) = def.sub_scene {
                sub_scene_roots.push((spawn.id(), sub.clone()));
            }

            if let Some(ref mesh_ref) = def.mesh {
                let mat_def = def.material.as_ref().cloned().unwrap_or_default();
                let mat_data = material_data(&mat_def);
                let mat_handle = registry.add_material(mat_data);

                match mesh_ref {
                    MeshRef::Primitive(name) => {
                        if let Some(mesh_data) = primitive_mesh(name) {
                            let mesh_handle = registry.add_mesh(mesh_data);
                            spawn.insert((
                                Mesh3d(mesh_handle),
                                Material3d(mat_handle),
                                Visibility::Inherited,
                                ComputedVisibility::default(),
                                RenderLayers::DEFAULT,
                            ));
                        } else {
                            log::warn!("SceneLoader3d: unknown primitive '{name}', skipping mesh");
                        }
                    }
                    MeshRef::Asset(path) => {
                        log::warn!("SceneLoader3d: asset mesh '{path}' not yet supported, skipping");
                    }
                }
            }

            if let Some(ref cam) = def.camera {
                spawn.insert(Camera3d {
                    projection: Projection::Perspective {
                        fov_y: cam.fov_y.to_radians(),
                        near: cam.near,
                        far: cam.far,
                    },
                    ..Camera3d::default()
                });
                spawn.insert((
                    Visibility::Visible,
                    ComputedVisibility(true),
                    RenderLayers::DEFAULT,
                ));
            }

            if let Some(ref light) = def.directional_light {
                let color = parse_hex_color(&light.color).unwrap_or(Color::WHITE);
                spawn.insert(DirectionalLight {
                    color,
                    illuminance: light.illuminance,
                    ..DirectionalLight::default()
                });
            }

            if let Some(ref light) = def.point_light {
                let color = parse_hex_color(&light.color).unwrap_or(Color::WHITE);
                spawn.insert(PointLight {
                    color,
                    intensity: light.intensity,
                    radius: light.range,
                    ..PointLight::default()
                });
            }

            if let Some(ref light) = def.spot_light {
                let color = parse_hex_color(&light.color).unwrap_or(Color::WHITE);
                spawn.insert(SpotLight {
                    color,
                    intensity: light.intensity,
                    radius: light.range,
                    inner_angle: light.inner_angle.to_radians(),
                    outer_angle: light.outer_angle.to_radians(),
                    ..SpotLight::default()
                });
            }

            let entity = spawn.id();

            if let Some(ref id) = def.id {
                id_map.insert(id.clone(), entity);
            }

            if let Some(ref parent_id) = def.parent {
                parent_refs.push((entity, parent_id.clone()));
            }
        }

        // second pass: wire parent/child relationships
        let mut parent_to_children: HashMap<Entity, Vec<Entity>> = HashMap::default();
        for (entity, parent_id) in parent_refs {
            if let Some(&parent) = id_map.get(&parent_id) {
                commands.entity(entity).insert(Parent(parent));
                parent_to_children.entry(parent).or_default().push(entity);
            } else {
                log::warn!("SceneLoader3d: parent '{parent_id}' not found");
            }
        }
        for (parent, children) in parent_to_children {
            commands
                .entity(parent)
                .insert(Children(children.into()));
        }

        // third pass: resolve sub-scene instances
        for (entity, sub_name) in sub_scene_roots {
            if let Some(registry_map) = scene_registry
                && let Some(sub_scene) = registry_map.get(&sub_name)
            {
                let sub_map = Self::spawn_internal(
                    commands,
                    registry,
                    sub_scene,
                    Some(registry_map),
                    Some(entity),
                );
                let sub_children: Vec<Entity> = sub_map.values().copied().collect();
                for &child in &sub_children {
                    commands.entity(child).insert(Parent(entity));
                }
                if !sub_children.is_empty() {
                    commands.entity(entity).insert(Children(sub_children.into()));
                }
            } else {
                log::warn!("SceneLoader3d: sub-scene '{sub_name}' not found in registry");
            }
        }

        // parent all root entities under the caller-supplied parent
        if let Some(parent) = parent_entity {
            let root_children: Vec<Entity> = id_map.values().copied().collect();
            for &child in &root_children {
                commands.entity(child).insert(Parent(parent));
            }
            if !root_children.is_empty() {
                commands.entity(parent).insert(Children(root_children.into()));
            }
        }

        id_map
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn local_transform(def: &EntityDefinition3d) -> LocalTransform3d {
    let (x, y, z) = def.rotation;
    let rotation = Quat::from_euler(
        lunar_math::glam::EulerRot::XYZ,
        x.to_radians(),
        y.to_radians(),
        z.to_radians(),
    );
    LocalTransform3d {
        translation: Vec3::new(def.position.0, def.position.1, def.position.2),
        rotation,
        scale: Vec3::new(def.scale.0, def.scale.1, def.scale.2),
    }
}

fn material_data(def: &MaterialDef) -> MaterialData {
    let base_color = parse_hex_color(&def.base_color).unwrap_or(Color::WHITE);
    MaterialData {
        shading: ShadingModel::Pbr,
        base_color,
        roughness: def.roughness,
        metallic: def.metallic,
        ..MaterialData::default()
    }
}

fn primitive_mesh(name: &str) -> Option<crate::mesh::MeshData> {
    use crate::primitives;
    match name {
        "cube"     => Some(primitives::unit_cube()),
        "sphere"   => Some(primitives::sphere_mesh(0.5, 16, 16)),
        "plane"    => Some(primitives::quad_mesh(0.5, 0.5)),
        "cylinder" => Some(primitives::cylinder_mesh(0.5, 1.0, 16, true)),
        _          => None,
    }
}

fn parse_hex_color(hex: &str) -> Option<Color> {
    let hex = hex.trim_start_matches('#');
    let (r, g, b, a) = match hex.len() {
        3 => {
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

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_scene() {
        let ron = r#"Scene3d(name: "empty")"#;
        let scene = SceneDefinition3d::from_ron(ron).unwrap();
        assert_eq!(scene.name, "empty");
        assert!(scene.entities.is_empty());
    }

    #[test]
    fn parse_scene_with_camera() {
        let ron = r##"
Scene3d(
    name: "test",
    entities: [
        (
            id: Some("cam"),
            position: (0.0, 5.0, -10.0),
            camera: Some((fov_y: 60.0, near: 0.1, far: 1000.0)),
        ),
    ],
)
"##;
        let scene = SceneDefinition3d::from_ron(ron).unwrap();
        assert_eq!(scene.entities.len(), 1);
        let cam = scene.entities[0].camera.as_ref().unwrap();
        assert_eq!(cam.fov_y, 60.0);
    }

    #[test]
    fn parse_scene_with_mesh() {
        let ron = r##"
Scene3d(
    name: "mesh_test",
    entities: [
        (
            id: Some("cube"),
            mesh: Some(Primitive("cube")),
            material: Some((base_color: "#ff0000", roughness: 0.8, metallic: 0.0)),
        ),
    ],
)
"##;
        let scene = SceneDefinition3d::from_ron(ron).unwrap();
        let entity = &scene.entities[0];
        assert!(matches!(entity.mesh, Some(MeshRef::Primitive(ref n)) if n == "cube"));
        assert_eq!(entity.material.as_ref().unwrap().roughness, 0.8);
    }

    #[test]
    fn binary_roundtrip() {
        let ron = r##"
Scene3d(
    name: "roundtrip",
    entities: [
        (
            id: Some("e1"),
            position: (1.0, 2.0, 3.0),
            rotation: (45.0, 0.0, 0.0),
            scale: (2.0, 2.0, 2.0),
            directional_light: Some((color: "#fffae8", illuminance: 50000.0)),
            tags: ["static"],
        ),
    ],
)
"##;
        let original = SceneDefinition3d::from_ron(ron).unwrap();
        let bytes = original.to_binary().unwrap();
        let restored = SceneDefinition3d::from_binary(&bytes).unwrap();
        assert_eq!(restored.name, original.name);
        assert_eq!(restored.entities.len(), 1);
        let e = &restored.entities[0];
        assert_eq!(e.position, (1.0, 2.0, 3.0));
        assert_eq!(e.rotation, (45.0, 0.0, 0.0));
        assert_eq!(e.tags, vec!["static"]);
        let light = e.directional_light.as_ref().unwrap();
        assert_eq!(light.illuminance, 50_000.0);
    }

    #[test]
    fn parse_hex_color_variants() {
        assert_eq!(parse_hex_color("#fff"), Some(Color::WHITE));
        assert_eq!(parse_hex_color("#ffffff"), Some(Color::WHITE));
        assert!(parse_hex_color("invalid").is_none());
    }

    #[test]
    fn default_scale_is_one() {
        let ron = r#"Scene3d(name: "s", entities: [()])"#;
        let scene = SceneDefinition3d::from_ron(ron).unwrap();
        assert_eq!(scene.entities[0].scale, (1.0, 1.0, 1.0));
    }
}
