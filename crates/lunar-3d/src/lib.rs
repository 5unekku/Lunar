//! 3D components and systems for the Lunar engine.
//!
//! this crate provides the building blocks for 3D scenes: transforms, cameras,
//! meshes, materials, lights, skeletal animation, and collision detection.
//! it is intentionally decoupled from any rendering backend — no wgpu code lives here.
//! the render system (a future `lunar-render-3d` crate) reads these components and
//! issues GPU draw calls.
//!
//! # design
//!
//! the component model mirrors id Tech 4 (Doom 3) more than id Tech 3 (Quake 3):
//! per-pixel lighting with normal maps is the baseline, not lightmaps. this suits
//! the Halo CE visual target (dynamic lights, per-object specular, skeletal meshes).
//!
//! # quick start
//!
//! ```ignore
//! use lunar::prelude::*;
//!
//! fn setup(mut commands: Commands) {
//!     // camera
//!     commands.spawn((
//!         LocalTransform3d::from_xyz(0.0, 2.0, 10.0),
//!         WorldTransform3d::default(),
//!         Camera3d::default(),
//!     ));
//!
//!     // mesh entity (static, no shadows)
//!     commands.spawn((
//!         LocalTransform3d::from_xyz(0.0, 0.0, 0.0),
//!         WorldTransform3d::default(),
//!         Mesh3d(mesh_handle),
//!         Material3d(material_handle),
//!         Visibility::Visible,
//!         ComputedVisibility::default(),
//!         RenderLayers::DEFAULT,
//!     ));
//!
//!     // mesh with shadow casting and frustum culling bounds
//!     commands.spawn((
//!         LocalTransform3d::from_xyz(5.0, 0.0, 0.0),
//!         WorldTransform3d::default(),
//!         Mesh3d(mesh_handle),
//!         Material3d(material_handle),
//!         Aabb3d::from_positions(&[...]),
//!         ShadowCaster,
//!     ));
//! }
//! ```

pub mod animation;
pub mod collision;
pub mod fog;
pub mod primitives;
pub mod visibility;

mod camera;
mod light;
mod material;
mod mesh;
mod plugin;
mod systems;
mod transform;

pub use animation::{
    AnimationClip, AnimationPlayer, AnimationTarget, JointTrack, Keyframe, advance_animations,
};
pub use camera::{ActiveCamera3d, AmbientLight, Camera3d, Projection, update_active_camera};
pub use collision::{
    Collider3d, ColliderShape3d, CollisionWorld3d, build_collision_world_3d,
};
pub use fog::{Fog, FogFalloff};
pub use light::{DirectionalLight, PointLight, SpotLight};
pub use material::{CullMode, Material3d, MaterialData, ShadingModel};
pub use mesh::{IndexBuffer, Mesh3d, MeshData, MeshUsage, SkinWeights, Vertex3d};
pub use plugin::Plugin3d;
pub use systems::propagate_transforms_3d;
pub use transform::{LocalTransform3d, WorldTransform3d};
pub use visibility::{
    Aabb3d, ComputedVisibility, Frustum, RenderLayers, ShadowCaster, ShadowReceiver, Visibility,
    propagate_visibility, update_frustum,
};
