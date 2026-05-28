//! 3D components and systems for the Lunar engine.
//!
//! this crate provides the building blocks for 3D scenes: transforms, cameras,
//! meshes, materials, lights, skeletal animation, collision detection, and frustum culling.
//! it is intentionally decoupled from any rendering backend.
//!
//! # design
//!
//! the component model mirrors id Tech 4 (Doom 3): per-pixel lighting with normal maps
//! is the baseline, not lightmaps. targets the Halo CE visual quality level.
//!
//! all propagation systems use persistent scratch resources to avoid per-frame heap
//! allocations — steady-state operation (no new entities) is fully allocation-free.
//!
//! # quick start
//!
//! ```ignore
//! use lunar::prelude::*;
//!
//! fn setup(mut commands: Commands) {
//!     commands.spawn(Camera3dBundle {
//!         local: LocalTransform3d::from_xyz(0.0, 2.0, 10.0),
//!         ..default()
//!     });
//!
//!     commands.spawn(Mesh3dBundle {
//!         local: LocalTransform3d::from_xyz(0.0, 0.0, 0.0),
//!         mesh: Mesh3d(mesh_handle),
//!         material: Material3d(mat_handle),
//!         ..default()
//!     });
//! }
//! ```

pub mod animation;
pub mod bundles;
pub mod collision;
pub mod fog;
pub mod mesh_registry;
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
pub use bundles::{
    Camera3dBundle, DirectionalLightBundle, Mesh3dBundle, PointLightBundle, ShadowMesh3dBundle,
    SpotLightBundle,
};
pub use camera::{ActiveCamera3d, AmbientLight, Camera3d, Projection, update_active_camera};
pub use collision::{
    Collider3d, ColliderEntryRef, ColliderShape3d, CollisionWorld3d, Ray3d, RayHit3d,
    build_collision_world_3d, raycast_3d,
};
pub use fog::{Fog, FogFalloff};
pub use light::{DirectionalLight, PointLight, SpotLight};
pub use material::{CullMode, Material3d, MaterialData, ShadingModel};
pub use mesh::{IndexBuffer, Mesh3d, MeshData, MeshUsage, SkinWeights, Vertex3d};
pub use mesh_registry::MeshRegistry;
pub use plugin::Plugin3d;
pub use systems::{TransformScratch3d, propagate_transforms_3d};
pub use transform::{LocalTransform3d, WorldTransform3d};
pub use visibility::{
    Aabb3d, ComputedVisibility, CullSoa, Frustum, RenderLayers, ShadowCaster, ShadowReceiver,
    Visibility, ViewportAspect, VisibilityScratch, build_cull_soa, propagate_visibility,
    update_frustum,
};
